/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Whisper's ClickFix check.
//!
//! A ClickFix page gets a person to paste a command into the Run box, a terminal or File
//! Explorer, after the page has put that command on the clipboard. The browser calls
//! this crate from `nsBaseClipboard::SetDataImpl` (the one place every clipboard write in
//! the parent process passes) with the text and who wrote it, and acts on the answer:
//!
//! * [`classify`] gives the text a [`Level`] and the [`Reasons`] for it;
//! * [`decide`] applies the decision table (who wrote it, how dangerous it looks, whether
//!   the command is visible on the page) and returns an [`Action`];
//! * [`is_visible`] tells whether a command is shown on the page, given the text of the
//!   page's visible elements that the browser collected;
//! * [`display_text`], [`hosts`] and [`placeholder`] build what the person sees.
//!
//! What this does not promise: a person can still type a command by hand, follow
//! instructions on another device or copy from another browser. The check stops a page
//! from silently putting a command on the clipboard that is not on screen.
//!
//! Everything runs on the parent's main thread when a page writes to the clipboard, so
//! the work is bounded: the text is folded once, an Aho-Corasick pass over fixed anchor
//! words decides which rules run, and the rules are regular expressions with linear
//! matching time, compiled the first time they are needed.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod classify;
mod compare;
mod decide;
mod display;
mod hosts;
mod normalize;
mod rules;

pub use classify::{classify, classify_html, html_to_text};
pub use compare::{is_substitution, is_visible, visibility_probes};
pub use decide::{decide, Action, Context, Decision, Mode, Notice, Source, Visibility};
pub use display::{display_text, trailing_strip_len_utf16, DISPLAY_CHARS};
pub use hosts::hosts;

/// How dangerous a text looks if pasted into a shell, the Run box or File Explorer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Level {
    /// Not a command, or nothing that downloads or runs code.
    None = 0,
    /// Obfuscation or a Windows tool attackers use, without a download-and-run chain.
    Low = 1,
    /// Downloads code and runs it (`curl … | sh`, `irm … | iex`).
    Med = 2,
    /// Downloads or decodes code and runs it, and hides, obfuscates or uses a bare IP.
    High = 3,
    /// Built to deceive: a fake "verification" comment, a command pushed out of view by
    /// spaces, invisible or look-alike characters, or terminal control characters.
    Critical = 4,
}

impl Level {
    /// The level for a number from the C ABI; anything unknown is `None`.
    pub fn from_u8(v: u8) -> Level {
        match v {
            1 => Level::Low,
            2 => Level::Med,
            3 => Level::High,
            4 => Level::Critical,
            _ => Level::None,
        }
    }
}

/// One reason for a level. The values are bits of [`Reasons`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Reason {
    /// Downloads code and runs it.
    DownloadRun = 1 << 0,
    /// Decodes hidden (for example Base64) code and runs it.
    DecodeRun = 1 << 1,
    /// A Windows tool loads code from the network or a share.
    RemoteTool = 1 << 2,
    /// Runs a program from a network share.
    UncRun = 1 << 3,
    /// Hides its window or runs in the background.
    HiddenWindow = 1 << 4,
    /// Written so that it is hard to read.
    Obfuscated = 1 << 5,
    /// Uses a bare IP address instead of a name.
    RawIp = 1 << 6,
    /// A comment that pretends to be a "verification".
    LureComment = 1 << 7,
    /// The command is pushed out of view by spaces.
    PaddedComment = 1 << 8,
    /// Removes macOS's download quarantine.
    QuarantineRemoval = 1 << 9,
    /// Starts a Windows program attackers often use.
    AbusedTool = 1 << 10,
    /// Contains invisible characters.
    InvisibleChars = 1 << 11,
    /// Contains letters from another alphabet that look like Latin ones.
    LookalikeChars = 1 << 12,
    /// Contains terminal control characters (such as ESC).
    ControlChars = 1 << 13,
    /// Ends with a line break, so pasting it into a terminal would run it at once.
    TrailingNewline = 1 << 14,
    /// The write was larger than the browser scans, so it could be hiding a command in
    /// the part that was not scanned; it is treated as at least MED.
    Oversize = 1 << 15,
}

impl Reason {
    /// Every reason, in bit order.
    pub const ALL: [Reason; 16] = [
        Reason::DownloadRun,
        Reason::DecodeRun,
        Reason::RemoteTool,
        Reason::UncRun,
        Reason::HiddenWindow,
        Reason::Obfuscated,
        Reason::RawIp,
        Reason::LureComment,
        Reason::PaddedComment,
        Reason::QuarantineRemoval,
        Reason::AbusedTool,
        Reason::InvisibleChars,
        Reason::LookalikeChars,
        Reason::ControlChars,
        Reason::TrailingNewline,
        Reason::Oversize,
    ];
}

/// A set of [`Reason`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Reasons(u32);

impl Reasons {
    /// The set as bits (see [`Reason`]).
    pub fn bits(self) -> u32 {
        self.0
    }

    /// The set for bits from [`Reasons::bits`]; unknown bits are dropped.
    pub fn from_bits(bits: u32) -> Reasons {
        Reasons(bits & Reason::ALL.iter().fold(0, |all, r| all | *r as u32))
    }

    /// Whether the set holds `r`.
    pub fn contains(self, r: Reason) -> bool {
        self.0 & r as u32 != 0
    }

    /// Adds `r`.
    pub fn insert(&mut self, r: Reason) {
        self.0 |= r as u32;
    }

    /// Whether the set is empty.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The reasons in the set, in bit order.
    pub fn iter(self) -> impl Iterator<Item = Reason> {
        Reason::ALL.into_iter().filter(move |r| self.contains(*r))
    }
}

/// What [`classify`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// How dangerous the text looks.
    pub level: Level,
    /// Why.
    pub reasons: Reasons,
    /// Every address in a download-and-run command belongs to an installer its project
    /// publishes (rules.toml, `[[installer]]`).
    pub known_installer: bool,
    /// Ids of the rules that matched, for tests and logs.
    pub features: Vec<&'static str>,
}

impl Verdict {
    /// A verdict for text that is not a command.
    pub fn none() -> Verdict {
        Verdict {
            level: Level::None,
            reasons: Reasons::default(),
            known_installer: false,
            features: Vec::new(),
        }
    }
}

/// What the clipboard holds instead of a blocked command.
pub fn placeholder() -> &'static str {
    rules::PLACEHOLDER
}

/// Runs `f` at the C ABI boundary, returning `default` if it panics, so a classifier bug
/// degrades to a safe answer instead of aborting the parent process. This catches only
/// when the build unwinds; a libxul build uses `panic = "abort"`, where a panic aborts
/// before this can run, so it is defence in depth — the classifier is written not to
/// panic. The default each FFI function passes is the safe one (treat as MED, block, not
/// visible, or empty); see `clipguard_ffi`.
#[doc(hidden)]
pub fn ffi_guard<T>(default: T, f: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(_) => default,
    }
}

#[cfg(test)]
mod lib_tests {
    use super::*;

    #[test]
    fn ffi_guard_returns_the_default_on_panic() {
        assert_eq!(ffi_guard(7u8, || 3), 3);
        assert_eq!(ffi_guard(7u8, || panic!("boom")), 7);
        // The safe default for classify is MED, so a panicking classify blocks.
        assert_eq!(ffi_guard(Level::Med, || panic!()), Level::Med);
    }
}
