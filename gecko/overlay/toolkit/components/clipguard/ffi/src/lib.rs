/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! C ABI of the clipguard crate, for nsBaseClipboard (toolkit/components/clipguard/
//! ClipboardGuard.cpp). Strings cross as Gecko strings; enums cross as numbers that the
//! clipguard crate turns back into its own types (an unknown number gets the safe
//! meaning, see `Source::from_u8` and friends). Nothing here keeps state.
//!
//! Every entry point runs inside `clipguard::ffi_guard`, so a panic in the classifier
//! degrades to the safe answer (treat as MED / block / not visible) instead of a value
//! that could let a command through. In a libxul build (`panic = "abort"`) a panic aborts
//! before the guard can catch it, so the guard's real value is in an unwinding build and
//! as documentation of the safe defaults; the classifier is also written not to panic.

use clipguard::{ffi_guard, Context, Level, Mode, Reasons, Source, Verdict, Visibility};
use nsstring::{nsACString, nsAString, nsString};
use thin_vec::ThinVec;

/// What `clipguard_classify` found (see `clipguard::Verdict`).
#[repr(C)]
pub struct ClipguardVerdict {
    /// `clipguard::Level`: 0 NONE, 1 LOW, 2 MED, 3 HIGH, 4 CRITICAL.
    pub level: u8,
    /// `clipguard::Reason` bits.
    pub reasons: u32,
    /// Every address in it is an installer its project publishes.
    pub known_installer: bool,
}

/// What the decision depends on besides the text (see `clipguard::Context`).
#[repr(C)]
pub struct ClipguardContext {
    /// `clipguard::Source`.
    pub source: u8,
    /// `clipguard::Mode`: 0 block, 1 warn, 2 off.
    pub mode: u8,
    pub strict_script_copy: bool,
    pub user_copy_notice: bool,
    pub site_allowed: bool,
    pub substituted: bool,
}

/// The decision (see `clipguard::Decision`).
#[repr(C)]
pub struct ClipguardDecision {
    /// `clipguard::Action`: 0 ignore, 1 allow, 2 check visibility, 3 block.
    pub action: u8,
    pub strip_trailing: bool,
    /// `clipguard::Notice`.
    pub notice: u8,
}

fn string(s: &nsAString) -> String {
    String::from_utf16_lossy(s)
}

/// Classifies `text`; `is_html` for a text/html flavor. On an internal panic, reports MED
/// (so the write is held/blocked) rather than letting it through.
#[no_mangle]
pub extern "C" fn clipguard_classify(text: &nsAString, is_html: bool, out: &mut ClipguardVerdict) {
    let s = string(text);
    *out = ffi_guard(
        ClipguardVerdict {
            level: Level::Med as u8,
            reasons: 0,
            known_installer: false,
        },
        || {
            let v = if is_html {
                clipguard::classify_html(&s)
            } else {
                clipguard::classify(&s)
            };
            ClipguardVerdict {
                level: v.level as u8,
                reasons: v.reasons.bits(),
                known_installer: v.known_installer,
            }
        },
    );
}

/// Applies the decision table. `visibility`: 0 unknown, 1 on screen, 2 not on screen.
#[no_mangle]
pub extern "C" fn clipguard_decide(
    verdict: &ClipguardVerdict,
    context: &ClipguardContext,
    visibility: u8,
) -> ClipguardDecision {
    // On panic, Block: the write is already being evaluated, so blocking is the safe
    // outcome.
    let block = ClipguardDecision {
        action: 3,
        strip_trailing: true,
        notice: 0,
    };
    ffi_guard(block, || {
        let v = Verdict {
            level: Level::from_u8(verdict.level),
            reasons: Reasons::from_bits(verdict.reasons),
            known_installer: verdict.known_installer,
            features: Vec::new(),
        };
        let c = Context {
            source: Source::from_u8(context.source),
            mode: Mode::from_u8(context.mode),
            strict_script_copy: context.strict_script_copy,
            user_copy_notice: context.user_copy_notice,
            site_allowed: context.site_allowed,
            substituted: context.substituted,
        };
        let d = clipguard::decide(&v, &c, Visibility::from_u8(visibility));
        ClipguardDecision {
            action: d.action as u8,
            strip_trailing: d.strip_trailing,
            notice: d.notice as u8,
        }
    })
}

/// Whether a `copy` handler swapped the person's `selection` for something more
/// dangerous (`clip_level` is the level of `clipboard`). On panic, true (treat as a
/// substitution, i.e. as a script write).
#[no_mangle]
pub extern "C" fn clipguard_is_substitution(
    selection: &nsAString,
    clipboard: &nsAString,
    clip_level: u8,
) -> bool {
    let (sel, clip) = (string(selection), string(clipboard));
    ffi_guard(true, || {
        clipguard::is_substitution(&sel, &clip, Level::from_u8(clip_level))
    })
}

/// Whether every line of `command` is in the visible page text `candidates`. On panic,
/// false (not visible, so the write is blocked).
#[no_mangle]
pub extern "C" fn clipguard_is_visible(
    command: &nsAString,
    candidates: &ThinVec<nsString>,
) -> bool {
    let cmd = string(command);
    let texts: Vec<String> = candidates.iter().map(|s| string(s)).collect();
    ffi_guard(false, || clipguard::is_visible(&cmd, &texts))
}

/// The pieces of `command` the page is searched for. On panic, empty (so the page has
/// nothing to match and the command is treated as not visible).
#[no_mangle]
pub extern "C" fn clipguard_visibility_probes(command: &nsAString, out: &mut ThinVec<nsString>) {
    let cmd = string(command);
    let probes = ffi_guard(Vec::new(), || clipguard::visibility_probes(&cmd));
    out.clear();
    for p in probes {
        out.push(nsString::from(p.as_str()));
    }
}

/// The text as the person is shown it (see `clipguard::display_text`). On panic, empty.
#[no_mangle]
pub extern "C" fn clipguard_display_text(text: &nsAString, out: &mut nsAString) {
    let s = string(text);
    out.assign_str(&ffi_guard(String::new(), || clipguard::display_text(&s)));
}

/// The hosts in `text`, separated by ", ". On panic, empty.
#[no_mangle]
pub extern "C" fn clipguard_hosts(text: &nsAString, out: &mut nsACString) {
    let s = string(text);
    out.assign(ffi_guard(String::new(), || clipguard::hosts(&s).join(", ")).as_str());
}

/// The length of `text` without a line break or ESC at its end, or -1 if there is
/// none to cut. On panic, -1 (strip nothing).
#[no_mangle]
pub extern "C" fn clipguard_trailing_strip_len(text: &nsAString) -> i64 {
    ffi_guard(-1, || {
        clipguard::trailing_strip_len_utf16(text)
            .and_then(|n| i64::try_from(n).ok())
            .unwrap_or(-1)
    })
}

/// What the clipboard holds instead of a blocked command.
#[no_mangle]
pub extern "C" fn clipguard_placeholder(out: &mut nsAString) {
    out.assign_str(ffi_guard(
        "# BLOCKED BY BROWSER page tried to place a command here do not run it",
        clipguard::placeholder,
    ));
}
