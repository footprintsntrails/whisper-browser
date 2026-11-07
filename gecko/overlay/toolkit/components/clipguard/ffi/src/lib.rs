/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! C ABI of the clipguard crate, for nsBaseClipboard (toolkit/components/clipguard/
//! ClipboardGuard.cpp). Strings cross as Gecko strings; enums cross as numbers that the
//! clipguard crate turns back into its own types (an unknown number gets the safe
//! meaning, see `Source::from_u8` and friends). Nothing here keeps state.

use clipguard::{Context, Level, Mode, Reasons, Source, Verdict, Visibility};
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

/// Classifies `text`; `is_html` for a text/html flavor.
#[no_mangle]
pub extern "C" fn clipguard_classify(text: &nsAString, is_html: bool, out: &mut ClipguardVerdict) {
    let text = string(text);
    let v = if is_html {
        clipguard::classify_html(&text)
    } else {
        clipguard::classify(&text)
    };
    out.level = v.level as u8;
    out.reasons = v.reasons.bits();
    out.known_installer = v.known_installer;
}

/// Applies the decision table. `visibility`: 0 unknown, 1 on screen, 2 not on screen.
#[no_mangle]
pub extern "C" fn clipguard_decide(
    verdict: &ClipguardVerdict,
    context: &ClipguardContext,
    visibility: u8,
) -> ClipguardDecision {
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
}

/// Whether a `copy` handler swapped the person's `selection` for something more
/// dangerous (`clip_level` is the level of `clipboard`).
#[no_mangle]
pub extern "C" fn clipguard_is_substitution(
    selection: &nsAString,
    clipboard: &nsAString,
    clip_level: u8,
) -> bool {
    clipguard::is_substitution(&string(selection), &string(clipboard), Level::from_u8(clip_level))
}

/// Whether every line of `command` is in the visible page text `candidates`.
#[no_mangle]
pub extern "C" fn clipguard_is_visible(command: &nsAString, candidates: &ThinVec<nsString>) -> bool {
    let texts: Vec<String> = candidates.iter().map(|c| string(c)).collect();
    clipguard::is_visible(&string(command), &texts)
}

/// The pieces of `command` the page is searched for.
#[no_mangle]
pub extern "C" fn clipguard_visibility_probes(command: &nsAString, out: &mut ThinVec<nsString>) {
    out.clear();
    for p in clipguard::visibility_probes(&string(command)) {
        out.push(nsString::from(p.as_str()));
    }
}

/// The text as the person is shown it (see `clipguard::display_text`).
#[no_mangle]
pub extern "C" fn clipguard_display_text(text: &nsAString, out: &mut nsAString) {
    out.assign_str(&clipguard::display_text(&string(text)));
}

/// The hosts in `text`, separated by ", ".
#[no_mangle]
pub extern "C" fn clipguard_hosts(text: &nsAString, out: &mut nsACString) {
    out.assign(clipguard::hosts(&string(text)).join(", ").as_str());
}

/// The length of `text` without a line break or ESC at its end, or -1 if there is
/// none to cut.
#[no_mangle]
pub extern "C" fn clipguard_trailing_strip_len(text: &nsAString) -> i64 {
    clipguard::trailing_strip_len_utf16(text)
        .and_then(|n| i64::try_from(n).ok())
        .unwrap_or(-1)
}

/// What the clipboard holds instead of a blocked command.
#[no_mangle]
pub extern "C" fn clipguard_placeholder(out: &mut nsAString) {
    out.assign_str(clipguard::placeholder());
}
