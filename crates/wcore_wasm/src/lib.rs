//! The WebAssembly module Whisper runs in a ChromeWorker (the "guard engine"):
//! `dl_guard` and `proto_guard` behind a byte-buffer interface, with no imports at all,
//! so the module cannot touch files, the network or the browser; it only gets the
//! bytes it is given and returns JSON.
//!
//! Interface (wasm32 only, in `abi`): the caller reserves memory with `wg_alloc`, writes
//! a request there, calls `wg_dl_inspect` or `wg_proto`, gets back `(ptr << 32) | len`
//! of the JSON answer, reads it and frees both buffers with `wg_free`.
//!
//! Requests are tag-length-value records: one tag byte, a little-endian u32 length,
//! then the value. For `wg_dl_inspect`: 1 file name, 2 name in the address, 3 MIME type
//! (UTF-8), 4 file size (u64), 5 origin (u8: 0 https, 1 insecure, 2 made by the page,
//! 3 local), 6 site allowed (u8), 7 a chunk (u64 offset, then the bytes). For
//! `wg_proto`: 1 scheme, 2 allowed `ms-` schemes (comma separated), 3 a command line.
//!
//! Everything but the four exported functions is safe Rust and runs natively in tests.

use dl_guard::{Bytes, Chunk, Input, Origin, Outcome, Reason};

/// One record of a request.
struct Field<'a> {
    tag: u8,
    value: &'a [u8],
}

fn fields(mut input: &[u8]) -> Result<Vec<Field<'_>>, &'static str> {
    let mut out = Vec::new();
    while !input.is_empty() {
        if input.len() < 5 {
            return Err("truncated record");
        }
        let tag = input[0];
        let len = u32::from_le_bytes([input[1], input[2], input[3], input[4]]) as usize;
        let value = input
            .get(5..5 + len)
            .ok_or("record longer than the request")?;
        out.push(Field { tag, value });
        input = &input[5 + len..];
    }
    Ok(out)
}

fn text(value: &[u8]) -> Result<&str, &'static str> {
    std::str::from_utf8(value).map_err(|_| "text is not UTF-8")
}

fn u64_of(value: &[u8]) -> Result<u64, &'static str> {
    Ok(u64::from_le_bytes(
        value.try_into().map_err(|_| "bad number")?,
    ))
}

/// JSON string literal.
fn js(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn reason_json(r: &Reason) -> String {
    let mut parts = vec![format!("\"code\":{}", js(r.code()))];
    match r {
        Reason::RiskyType { kind, ext } => {
            parts.push(format!("\"kind\":{}", js(kind.name())));
            parts.push(format!("\"ext\":{}", js(ext)));
        }
        Reason::Program { ext } | Reason::ProgramMadeByPage { ext } => {
            parts.push(format!("\"ext\":{}", js(ext)))
        }
        Reason::DirectionMarks => {}
        Reason::ReversedExtension { shown, real } | Reason::DoubleExtension { shown, real } => {
            parts.push(format!("\"shown\":{}", js(shown)));
            parts.push(format!("\"real\":{}", js(real)));
        }
        Reason::ContentMismatch { claimed, found } => {
            parts.push(format!("\"claimed\":{}", js(claimed)));
            parts.push(format!("\"found\":{}", js(found.name())));
        }
        Reason::ArchiveHolds {
            entry,
            kind,
            encrypted,
        } => {
            parts.push(format!("\"entry\":{}", js(entry)));
            parts.push(format!("\"kind\":{}", js(kind.name())));
            parts.push(format!("\"encrypted\":{encrypted}"));
        }
        Reason::DiscImageHolds { entry } => parts.push(format!("\"entry\":{}", js(entry))),
    }
    format!("{{{}}}", parts.join(","))
}

/// A `wg_dl_inspect` request in, the verdict as JSON out.
pub fn dl_inspect(request: &[u8]) -> String {
    match dl_inspect_inner(request) {
        Ok(json) => json,
        Err(e) => format!("{{\"error\":{}}}", js(e)),
    }
}

fn dl_inspect_inner(request: &[u8]) -> Result<String, &'static str> {
    let mut file_name = "";
    let mut url_name = "";
    let mut mime = "";
    let mut size = 0u64;
    let mut origin = Origin::Https;
    let mut site_allowed = false;
    let mut chunks = Vec::new();
    for f in fields(request)? {
        match f.tag {
            1 => file_name = text(f.value)?,
            2 => url_name = text(f.value)?,
            3 => mime = text(f.value)?,
            4 => size = u64_of(f.value)?,
            5 => {
                origin = match f.value.first() {
                    Some(0) => Origin::Https,
                    Some(1) => Origin::Insecure,
                    Some(2) => Origin::MadeByPage,
                    Some(3) => Origin::Local,
                    _ => return Err("bad origin"),
                }
            }
            6 => site_allowed = f.value.first() == Some(&1),
            7 => {
                let offset = u64_of(f.value.get(..8).ok_or("bad chunk")?)?;
                chunks.push(Chunk {
                    offset,
                    data: &f.value[8..],
                });
            }
            _ => return Err("unknown record"),
        }
    }
    let outcome = dl_guard::inspect(&Input {
        file_name,
        url_name,
        mime,
        origin,
        site_allowed,
        bytes: Bytes {
            size,
            chunks: &chunks,
        },
    });
    let format_json = |f: Option<dl_guard::Format>| f.map_or("null".to_string(), |f| js(f.name()));
    Ok(match outcome {
        Outcome::Allow { kind, format } => format!(
            "{{\"verdict\":\"allow\",\"kind\":{},\"format\":{}}}",
            js(kind.name()),
            format_json(format)
        ),
        Outcome::Hold {
            kind,
            format,
            reasons,
        } => format!(
            "{{\"verdict\":\"hold\",\"kind\":{},\"format\":{},\"reasons\":[{}]}}",
            js(kind.name()),
            format_json(format),
            reasons
                .iter()
                .map(reason_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Outcome::Need(ranges) => format!(
            "{{\"verdict\":\"need\",\"ranges\":[{}]}}",
            ranges
                .iter()
                .map(|(o, l)| format!("[{o},{l}]"))
                .collect::<Vec<_>>()
                .join(",")
        ),
    })
}

/// The ranges to read before the first `wg_dl_inspect` call, as JSON.
pub fn dl_first_ranges(size: u64) -> String {
    let ranges = dl_guard::first_ranges(size);
    format!(
        "[{}]",
        ranges
            .iter()
            .map(|(o, l)| format!("[{o},{l}]"))
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// A `wg_proto` request in, the tier and the program as JSON out.
pub fn proto(request: &[u8]) -> String {
    let run = || -> Result<String, &'static str> {
        let mut scheme = "";
        let mut allowed = "";
        let mut command = "";
        for f in fields(request)? {
            match f.tag {
                1 => scheme = text(f.value)?,
                2 => allowed = text(f.value)?,
                3 => command = text(f.value)?,
                _ => return Err("unknown record"),
            }
        }
        let allowed: Vec<&str> = allowed
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let tier = match proto_guard::classify(scheme, &allowed) {
            proto_guard::Tier::Block => "block",
            proto_guard::Tier::Ask => "ask",
        };
        let program =
            proto_guard::program_from_command(command).map_or("null".to_string(), |p| js(&p));
        let scheme = proto_guard::normalize(scheme).map_or("null".to_string(), |s| js(&s));
        Ok(format!(
            "{{\"tier\":\"{tier}\",\"scheme\":{scheme},\"program\":{program}}}"
        ))
    };
    run().unwrap_or_else(|e| format!("{{\"error\":{}}}", js(e)))
}

/// The exported functions. The only unsafe code: turning the caller's pointer and
/// length into a slice, and handing an answer buffer out and back.
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
mod abi {
    use std::alloc::{Layout, alloc, dealloc};

    fn layout(len: usize) -> Option<Layout> {
        Layout::from_size_align(len, 1).ok()
    }

    /// Reserves `len` bytes for a request; 0 if that is impossible.
    #[unsafe(no_mangle)]
    pub extern "C" fn wg_alloc(len: usize) -> *mut u8 {
        match layout(len) {
            // SAFETY: the layout has a non-zero size.
            Some(l) if len > 0 => unsafe { alloc(l) },
            _ => std::ptr::null_mut(),
        }
    }

    /// Frees a buffer from `wg_alloc` or an answer.
    ///
    /// # Safety
    /// `ptr` and `len` must describe a buffer from `wg_alloc` or an answer, not yet freed.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn wg_free(ptr: *mut u8, len: usize) {
        if let (false, Some(l)) = (ptr.is_null() || len == 0, layout(len)) {
            // SAFETY: the caller passes back exactly what was allocated with this layout.
            unsafe { dealloc(ptr, l) };
        }
    }

    /// # Safety
    /// `ptr` and `len` must describe initialised memory from `wg_alloc`.
    unsafe fn answer(ptr: *const u8, len: usize, f: fn(&[u8]) -> String) -> u64 {
        let request: &[u8] = if ptr.is_null() || len == 0 {
            &[]
        } else {
            // SAFETY: the caller wrote `len` bytes at `ptr`, which wg_alloc returned.
            unsafe { std::slice::from_raw_parts(ptr, len) }
        };
        let out = f(request).into_bytes().into_boxed_slice();
        let out_len = out.len();
        // Leaked here, freed by wg_free with the same length (the layout of a boxed
        // byte slice is (len, 1)).
        let out_ptr = Box::into_raw(out) as *mut u8;
        ((out_ptr as usize as u64) << 32) | out_len as u64
    }

    /// # Safety
    /// See `answer`.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn wg_dl_inspect(ptr: *const u8, len: usize) -> u64 {
        // SAFETY: passed through from the caller.
        unsafe { answer(ptr, len, super::dl_inspect) }
    }

    /// # Safety
    /// See `answer`.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn wg_proto(ptr: *const u8, len: usize) -> u64 {
        // SAFETY: passed through from the caller.
        unsafe { answer(ptr, len, super::proto) }
    }

    /// The first ranges to read for a file of `size` bytes, as JSON.
    #[unsafe(no_mangle)]
    pub extern "C" fn wg_dl_first_ranges(size: u64) -> u64 {
        let size_bytes = size.to_le_bytes();
        // SAFETY: a local array, valid for its length.
        unsafe {
            answer(size_bytes.as_ptr(), 8, |b| {
                super::dl_first_ranges(u64::from_le_bytes(b.try_into().unwrap_or_default()))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(tag: u8, value: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value);
        out
    }

    fn dl_request(name: &str, data: &[u8], origin: u8, allowed: bool) -> Vec<u8> {
        let mut req = record(1, name.as_bytes());
        req.extend(record(4, &(data.len() as u64).to_le_bytes()));
        req.extend(record(5, &[origin]));
        req.extend(record(6, &[allowed as u8]));
        let mut chunk = 0u64.to_le_bytes().to_vec();
        chunk.extend_from_slice(data);
        req.extend(record(7, &chunk));
        req
    }

    #[test]
    fn verdicts_as_json() {
        assert_eq!(
            dl_inspect(&dl_request("report.pdf", b"%PDF-1.4", 0, false)),
            "{\"verdict\":\"allow\",\"kind\":\"document\",\"format\":\"PDF\"}"
        );
        assert_eq!(
            dl_inspect(&dl_request("disk.iso", b"x", 0, false)),
            "{\"verdict\":\"hold\",\"kind\":\"disk-image\",\"format\":null,\"reasons\":[{\"code\":\"disk-image\",\"kind\":\"disk-image\",\"ext\":\"iso\"}]}"
        );
        let held = dl_inspect(&dl_request("a\"\u{202E}b.exe", b"MZ", 2, false));
        assert!(
            held.contains("\"code\":\"direction-marks\"") && held.contains("program-made-by-page"),
            "{held}"
        );
    }

    #[test]
    fn bad_requests_are_errors_not_panics() {
        assert_eq!(
            dl_inspect(&[1, 9, 0, 0, 0, b'a']),
            "{\"error\":\"record longer than the request\"}"
        );
        assert_eq!(
            dl_inspect(&record(1, &[0xFF, 0xFE])),
            "{\"error\":\"text is not UTF-8\"}"
        );
        assert_eq!(
            dl_inspect(&record(4, b"short")),
            "{\"error\":\"bad number\"}"
        );
        assert_eq!(
            dl_inspect(&record(42, b"")),
            "{\"error\":\"unknown record\"}"
        );
        assert_eq!(proto(&[7]), "{\"error\":\"truncated record\"}");
    }

    #[test]
    fn random_requests_never_panic() {
        // A cheap stand-in for a fuzzer: random records and random file bytes.
        let mut seed = 0x9E37_79B9_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for _ in 0..3000 {
            let len = (next() % 300) as usize;
            let junk: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let _ = dl_inspect(&junk);
            let _ = proto(&junk);
            let name = ["a.zip", "b.iso", "c.exe", "d.pdf", "e"][(next() % 5) as usize];
            let _ = dl_inspect(&dl_request(
                name,
                &junk,
                (next() % 4) as u8,
                next() % 2 == 0,
            ));
            // Archive and disc image signatures with garbage after them.
            let mut zipish = b"PK\x03\x04".to_vec();
            zipish.extend_from_slice(&junk);
            zipish.extend_from_slice(b"PK\x05\x06");
            zipish.extend((0..18).map(|_| next() as u8));
            let _ = dl_inspect(&dl_request("x.zip", &zipish, 0, false));
            let mut isoish = vec![0u8; 0x8800];
            isoish[0x8000] = 1;
            isoish[0x8001..0x8006].copy_from_slice(b"CD001");
            for b in isoish[0x8006..0x8800].iter_mut() {
                *b = next() as u8;
            }
            let _ = dl_inspect(&dl_request("x.iso", &isoish, 0, false));
        }
    }

    #[test]
    fn protocol_answers() {
        let mut req = record(1, b"search-ms");
        req.extend(record(3, b"explorer.exe %1"));
        assert_eq!(
            proto(&req),
            "{\"tier\":\"block\",\"scheme\":\"search-ms\",\"program\":\"explorer.exe\"}"
        );
        let mut req = record(1, b"ZoomMTG:");
        req.extend(record(3, b"\"/opt/zoom/zoom\" %u"));
        assert_eq!(
            proto(&req),
            "{\"tier\":\"ask\",\"scheme\":\"zoommtg\",\"program\":\"/opt/zoom/zoom\"}"
        );
        let mut req = record(1, b"ms-teams");
        req.extend(record(2, b"ms-teams, ms-word"));
        assert!(proto(&req).starts_with("{\"tier\":\"ask\""));
    }

    #[test]
    fn first_ranges() {
        assert_eq!(dl_first_ranges(1000), "[[0,1000],[488,512],[0,1000]]");
    }
}
