/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! The decision table: what the browser does with a clipboard write.
//!
//! | Who wrote it                   | Level    | On screen | Action                                   |
//! |--------------------------------|----------|-----------|------------------------------------------|
//! | a script (P2–P4)               | CRITICAL | any       | block                                    |
//! | a script                       | HIGH     | no        | block                                    |
//! | a script                       | HIGH     | yes       | copy, warn once                          |
//! | a script                       | MED      | no        | block                                    |
//! | a script                       | MED      | yes       | copy; known installer: nothing, else a notice once per site |
//! | a script                       | LOW      | any       | copy, icon in the address bar            |
//! | the person (P1)                | HIGH+    | –         | copy, warn once (`clipguard.user_copy_notice`) |
//! | anyone                         | NONE     | –         | nothing                                  |
//!
//! "A script" is `execCommand("copy")` (P3), `navigator.clipboard` (P4), and a `copy`
//! handler that replaced the person's selection with something more dangerous (P2;
//! a handler that only tidies the selection counts as the person's copy). For a script,
//! a line break or ESC at the end is cut at LOW and above, so pasting into a terminal
//! does not run the command by itself. Whether the command is on screen is not known
//! when the write arrives: [`Action::CheckVisibility`] asks the browser to find out and
//! call [`decide`] again. `clipguard.mode = warn` turns every block into a copy with a
//! warning, `off` turns the check off; `clipguard.strict_script_copy` blocks every
//! script write at LOW and above; a site allowed for this session is not blocked.

use crate::{Level, Verdict};

/// Who put the text on the clipboard (`IPCTransferable.whisperCopySource`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Source {
    /// Not labelled (treated as a script).
    Unknown = 0,
    /// P1: the person copied a selection (Ctrl+C, the context menu).
    UserCopy = 1,
    /// P2: the person copied, and the page's `copy` handler replaced the data.
    PageReplacedCopy = 2,
    /// P3: `document.execCommand("copy")` (with or without a `copy` handler).
    ExecCommand = 3,
    /// P4: `navigator.clipboard.write()` or `writeText()`.
    AsyncClipboard = 4,
    /// Came from a content process without a label (treated as a script).
    ContentUnlabeled = 5,
}

impl Source {
    /// The source for a number from the C ABI; anything unknown is `ContentUnlabeled`.
    pub fn from_u8(v: u8) -> Source {
        match v {
            0 => Source::Unknown,
            1 => Source::UserCopy,
            2 => Source::PageReplacedCopy,
            3 => Source::ExecCommand,
            4 => Source::AsyncClipboard,
            _ => Source::ContentUnlabeled,
        }
    }
}

/// `clipguard.mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    /// Block (the default).
    Block = 0,
    /// Copy, but warn where it would block.
    Warn = 1,
    /// No check at all.
    Off = 2,
}

impl Mode {
    /// The mode for a number from the C ABI; anything unknown is `Block`.
    pub fn from_u8(v: u8) -> Mode {
        match v {
            1 => Mode::Warn,
            2 => Mode::Off,
            _ => Mode::Block,
        }
    }
}

/// Whether the command is shown on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Visibility {
    /// Not checked yet.
    Unknown = 0,
    /// Every line is on screen.
    Visible = 1,
    /// Not on screen, or the page could not be checked.
    NotVisible = 2,
}

impl Visibility {
    /// The value for a number from the C ABI; anything unknown is `Unknown`.
    pub fn from_u8(v: u8) -> Visibility {
        match v {
            1 => Visibility::Visible,
            2 => Visibility::NotVisible,
            _ => Visibility::Unknown,
        }
    }
}

/// Everything besides the text that the decision depends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    /// Who wrote it.
    pub source: Source,
    /// `clipguard.mode`.
    pub mode: Mode,
    /// `clipguard.strict_script_copy`.
    pub strict_script_copy: bool,
    /// `clipguard.user_copy_notice`.
    pub user_copy_notice: bool,
    /// The person allowed commands from this site for this session.
    pub site_allowed: bool,
    /// For [`Source::PageReplacedCopy`]: see [`crate::is_substitution`].
    pub substituted: bool,
}

/// What the browser does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Action {
    /// Write as is, say nothing.
    Ignore = 0,
    /// Write (after cutting a trailing line break if `strip_trailing`), then show
    /// `notice`.
    Allow = 1,
    /// Hold the write, find out whether the command is on screen, decide again.
    CheckVisibility = 2,
    /// Put the placeholder on the clipboard instead and explain why.
    Block = 3,
}

/// What the browser shows after an allowed write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Notice {
    /// Nothing.
    None = 0,
    /// LOW: an icon in the address bar.
    Indicator = 1,
    /// MED on screen, not a known installer: "downloads and runs a script from {host}",
    /// once per site per session.
    InstallerNotice = 2,
    /// HIGH on screen, or the person copied something HIGH: "looks like one used to
    /// install malware", once per site per session.
    CommandWarning = 3,
    /// P2: "This page changed what you copied."
    Substituted = 4,
    /// `clipguard.mode = warn`: would have been blocked.
    Warned = 5,
}

/// The result of [`decide`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    /// What to do.
    pub action: Action,
    /// Cut a line break or ESC at the end before writing (see
    /// [`crate::trailing_strip_len_utf16`]).
    pub strip_trailing: bool,
    /// What to show after an allowed write.
    pub notice: Notice,
}

impl Decision {
    fn new(action: Action, strip_trailing: bool, notice: Notice) -> Decision {
        Decision {
            action,
            strip_trailing,
            notice,
        }
    }
}

/// Applies the decision table (see the module documentation).
pub fn decide(verdict: &Verdict, ctx: &Context, visibility: Visibility) -> Decision {
    let ignore = Decision::new(Action::Ignore, false, Notice::None);
    if ctx.mode == Mode::Off || verdict.level == Level::None {
        return ignore;
    }

    let scripted = match ctx.source {
        Source::UserCopy => false,
        Source::PageReplacedCopy => ctx.substituted,
        Source::Unknown
        | Source::ExecCommand
        | Source::AsyncClipboard
        | Source::ContentUnlabeled => true,
    };
    if !scripted {
        return if verdict.level >= Level::High && ctx.user_copy_notice {
            Decision::new(Action::Allow, false, Notice::CommandWarning)
        } else {
            ignore
        };
    }

    // A script wrote something at LOW or above.
    let allow = |notice: Notice| {
        let notice = if ctx.substituted && notice != Notice::Warned {
            Notice::Substituted
        } else {
            notice
        };
        Decision::new(Action::Allow, true, notice)
    };
    let block = || match ctx.mode {
        Mode::Warn => allow(Notice::Warned),
        Mode::Block | Mode::Off => Decision::new(Action::Block, true, Notice::None),
    };
    if ctx.site_allowed {
        return Decision::new(Action::Allow, true, Notice::None);
    }
    if ctx.strict_script_copy {
        return block();
    }
    let on_screen = |if_visible: Notice| match visibility {
        Visibility::Unknown => Decision::new(Action::CheckVisibility, true, Notice::None),
        Visibility::NotVisible => block(),
        Visibility::Visible => allow(if_visible),
    };
    match verdict.level {
        Level::Critical => block(),
        Level::High => on_screen(Notice::CommandWarning),
        Level::Med => on_screen(if verdict.known_installer {
            Notice::None
        } else {
            Notice::InstallerNotice
        }),
        Level::Low => allow(Notice::Indicator),
        Level::None => ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Reasons;

    fn verdict(level: Level, known_installer: bool) -> Verdict {
        Verdict {
            level,
            reasons: Reasons::default(),
            known_installer,
            features: Vec::new(),
        }
    }

    fn ctx(source: Source) -> Context {
        Context {
            source,
            mode: Mode::Block,
            strict_script_copy: false,
            user_copy_notice: true,
            site_allowed: false,
            substituted: false,
        }
    }

    fn d(level: Level, known: bool, c: Context, v: Visibility) -> (Action, bool, Notice) {
        let r = decide(&verdict(level, known), &c, v);
        (r.action, r.strip_trailing, r.notice)
    }

    const SCRIPTS: [Source; 4] = [
        Source::ExecCommand,
        Source::AsyncClipboard,
        Source::ContentUnlabeled,
        Source::Unknown,
    ];

    #[test]
    fn script_rows_of_the_table() {
        use Action::*;
        use Visibility::*;
        for s in SCRIPTS {
            let c = ctx(s);
            for v in [Unknown, Visible, NotVisible] {
                assert_eq!(d(Level::Critical, false, c, v), (Block, true, Notice::None));
                assert_eq!(d(Level::Low, false, c, v), (Allow, true, Notice::Indicator));
                assert_eq!(d(Level::None, false, c, v), (Ignore, false, Notice::None));
            }
            for level in [Level::High, Level::Med] {
                assert_eq!(
                    d(level, false, c, Unknown),
                    (CheckVisibility, true, Notice::None)
                );
                assert_eq!(d(level, false, c, NotVisible), (Block, true, Notice::None));
            }
            assert_eq!(
                d(Level::High, false, c, Visible),
                (Allow, true, Notice::CommandWarning)
            );
            assert_eq!(d(Level::Med, true, c, Visible), (Allow, true, Notice::None));
            assert_eq!(
                d(Level::Med, false, c, Visible),
                (Allow, true, Notice::InstallerNotice)
            );
            // A known installer that is not on screen is still held back.
            assert_eq!(
                d(Level::Med, true, c, NotVisible),
                (Block, true, Notice::None)
            );
        }
    }

    #[test]
    fn the_persons_own_copy_is_never_blocked() {
        let c = ctx(Source::UserCopy);
        for v in [Visibility::Unknown, Visibility::NotVisible] {
            assert_eq!(
                d(Level::Critical, false, c, v),
                (Action::Allow, false, Notice::CommandWarning)
            );
            assert_eq!(
                d(Level::High, false, c, v),
                (Action::Allow, false, Notice::CommandWarning)
            );
            assert_eq!(
                d(Level::Med, false, c, v),
                (Action::Ignore, false, Notice::None)
            );
            assert_eq!(
                d(Level::Low, false, c, v),
                (Action::Ignore, false, Notice::None)
            );
        }
        let quiet = Context {
            user_copy_notice: false,
            ..c
        };
        assert_eq!(
            d(Level::Critical, false, quiet, Visibility::Unknown),
            (Action::Ignore, false, Notice::None)
        );
    }

    #[test]
    fn a_copy_handler_counts_as_a_script_only_when_it_substituted() {
        let tidy = ctx(Source::PageReplacedCopy);
        assert_eq!(
            d(Level::Med, false, tidy, Visibility::Unknown).0,
            Action::Ignore
        );
        let swapped = Context {
            substituted: true,
            ..tidy
        };
        assert_eq!(
            d(Level::Critical, false, swapped, Visibility::Unknown).0,
            Action::Block
        );
        assert_eq!(
            d(Level::Low, false, swapped, Visibility::Unknown),
            (Action::Allow, true, Notice::Substituted)
        );
        assert_eq!(
            d(Level::Med, true, swapped, Visibility::Visible),
            (Action::Allow, true, Notice::Substituted)
        );
    }

    #[test]
    fn modes_strictness_and_site_permission() {
        let base = ctx(Source::AsyncClipboard);
        let off = Context {
            mode: Mode::Off,
            ..base
        };
        assert_eq!(
            d(Level::Critical, false, off, Visibility::Unknown),
            (Action::Ignore, false, Notice::None)
        );
        let warn = Context {
            mode: Mode::Warn,
            ..base
        };
        assert_eq!(
            d(Level::Critical, false, warn, Visibility::Unknown),
            (Action::Allow, true, Notice::Warned)
        );
        assert_eq!(
            d(Level::Med, false, warn, Visibility::Unknown).0,
            Action::CheckVisibility
        );
        assert_eq!(
            d(Level::Med, false, warn, Visibility::NotVisible),
            (Action::Allow, true, Notice::Warned)
        );
        let strict = Context {
            strict_script_copy: true,
            ..base
        };
        assert_eq!(
            d(Level::Low, false, strict, Visibility::Unknown),
            (Action::Block, true, Notice::None)
        );
        let allowed = Context {
            site_allowed: true,
            ..base
        };
        assert_eq!(
            d(Level::Critical, false, allowed, Visibility::Unknown),
            (Action::Allow, true, Notice::None)
        );
    }

    #[test]
    fn numbers_from_the_c_abi_are_safe() {
        assert_eq!(Source::from_u8(200), Source::ContentUnlabeled);
        assert_eq!(Mode::from_u8(9), Mode::Block);
        assert_eq!(Visibility::from_u8(9), Visibility::Unknown);
        assert_eq!(Level::from_u8(9), Level::None);
    }
}
