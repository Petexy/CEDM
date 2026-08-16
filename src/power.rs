//! Sleep, restart and shut down, offered before anybody has signed in.
//!
//! The greeter does not perform any of these. It asks `systemctl`, which asks
//! `logind`, which asks polkit — and polkit's answer for an unauthenticated
//! user at a console is the machine's policy, not this program's. That
//! layering is the point: a greeter that could power a machine down by itself
//! would be a way to power a machine down by itself.
//!
//! So there are two independent gates, and a request has to pass both. The
//! administrator's [`crate::config::Config`] decides whether the greeter
//! *offers* the action at all, and polkit decides whether the request is
//! carried out. A machine where the buttons should not exist sets the first; a
//! machine where they may exist but not for everybody sets the second.
//!
//! Nothing here goes near a shell. `systemctl` is executed as an argument
//! vector with a fixed, non-configurable verb — the same rule the session
//! launcher follows, for the same reason.

use crate::i18n;
use std::process::{Command, Stdio};

/// One of exactly three things the greeter may ask for.
///
/// An enum rather than a string so there is no path by which a configuration
/// file, a desktop entry or a preference can name a command: the set is closed
/// at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Sleep,
    Restart,
    ShutDown,
}

impl Action {
    /// The label under the mark on the panel, in the machine's language.
    pub fn label(self) -> &'static str {
        let text = i18n::text();
        match self {
            Self::Sleep => text.sleep,
            Self::Restart => text.restart,
            Self::ShutDown => text.shut_down,
        }
    }

    /// What is said to `systemctl`, and what this action is called in the
    /// journal.
    ///
    /// A log line is not a place for a translated word: whoever reads it is as
    /// likely to be reading somebody else's journal as their own, and the verb
    /// is the thing that was actually asked for.
    pub fn verb(self) -> &'static str {
        match self {
            Self::Sleep => "suspend",
            Self::Restart => "reboot",
            Self::ShutDown => "poweroff",
        }
    }

    /// What the user is told when the machine declines.
    pub fn refusal(self) -> String {
        i18n::fill(i18n::text().power_not_permitted, self.label())
    }
}

/// Every action, in the order the panel lays them out.
pub const ALL: [Action; 3] = [Action::Sleep, Action::Restart, Action::ShutDown];

/// Ask the machine to carry out `action`.
///
/// Returns whether the request was *accepted*, which is not the same as
/// whether it happened: a shut-down that is accepted takes the greeter down
/// with it, and there is nothing left to report to. What this does catch is
/// the case that actually needs reporting — polkit refusing, or `systemctl`
/// not being installed — because that leaves the user looking at a button that
/// did nothing.
pub fn request(action: Action) -> Result<(), String> {
    // `--no-ask-password` so a refusal is a refusal. Without it `systemctl`
    // may try to authenticate on a terminal that this process does not have,
    // and the greeter would hang on a prompt nobody can see or answer.
    let status = Command::new("systemctl")
        .arg("--no-ask-password")
        .arg(action.verb())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => {
            tracing::warn!(action = action.verb(), ?status, "power request refused");
            Err(action.refusal())
        }
        Err(error) => {
            tracing::warn!(action = action.verb(), %error, "could not ask systemctl");
            Err(i18n::fill(i18n::text().power_unavailable, action.label()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_lays_the_actions_out_in_one_fixed_order() {
        i18n::with_language(i18n::Language::English, || {
            assert_eq!(
                ALL.map(Action::label),
                ["Sleep", "Restart", "Shut Down"],
                "the order the design shows, left to right"
            );
        });
        // The order is the design's and does not move with the language; only
        // the words in it do.
        i18n::with_language(i18n::Language::Polish, || {
            assert_eq!(
                ALL.map(Action::label),
                ["Uśpienie", "Uruchom ponownie", "Wyłącz"]
            );
        });
    }

    /// The whole of the safety argument for this module is that a verb cannot
    /// come from outside it. If this list ever grows a value that is not a
    /// `systemctl` verb, that argument has stopped being true.
    #[test]
    fn every_verb_is_one_of_the_three_closed_words() {
        for action in ALL {
            assert!(matches!(action.verb(), "suspend" | "reboot" | "poweroff"));
            assert!(action.verb().chars().all(|c| c.is_ascii_lowercase()));
        }
    }

    /// In every language, because several of them write the sentence around
    /// the action rather than in front of it, and one that dropped the name
    /// would read as a refusal of nothing in particular.
    #[test]
    fn a_refusal_names_the_action_it_refused() {
        i18n::with_language(i18n::Language::English, || {
            assert!(Action::ShutDown.refusal().contains("Shut Down"));
            assert!(Action::Sleep.refusal().contains("Sleep"));
        });
        for language in i18n::ALL {
            i18n::with_language(language, || {
                for action in ALL {
                    let refusal = action.refusal();
                    assert!(
                        refusal.contains(action.label()),
                        "{language:?}: {refusal:?} does not name {}",
                        action.label()
                    );
                    assert!(!refusal.contains("{}"), "{language:?}: {refusal:?}");
                }
            });
        }
    }
}
