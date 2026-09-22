//! The names the shell's look is written in, and where an account keeps them.
//!
//! Vocabulary rather than reading. This module used to open a selected
//! account's `shell.toml` and take the accent, the two materials and the
//! keyboard straight out of it — "without adopting their account", which was
//! true about privilege and not about trust. A greeter that opens a file inside
//! a home directory is a greeter an account can point at anything it likes, and
//! every one of those four settings already reaches the login screen the way it
//! is supposed to: in the copy the account publishes on its way into a session,
//! read under [`crate::reading`]'s rules and understood by [`crate::look::Look`],
//! which knows all four keys and the one they used to share.
//!
//! What is left here is what the names mean — which palettes and materials
//! exist, how a hand-typed one is matched, and where in a home the shell keeps
//! them, for the account itself to find when it publishes.

use std::path::{Path, PathBuf};

/// Every accent name this greeter will accept out of a user's settings file.
///
/// Read off the palettes themselves rather than written out again here. There
/// is one list of accents in this program — `visual::theme::ACCENTS`, which is
/// also what draws them — and a second spelling of it would be a list that can
/// fall behind the colours it names without anything saying so.
pub const DEFAULT_ACCENT: &str = crate::visual::theme::ACCENTS[0].name;

/// Every material the shell can be set to draw itself in, in the order its
/// Settings column lists them — the same two for the wallpaper as for the marks.
/// The first is what an unreadable or unrecognised setting falls back to.
pub const THEMES: [&str; 2] = ["Default", "Simple"];
pub const DEFAULT_THEME: &str = THEMES[0];

/// The third thing the shell's *wallpaper* can be set to: a picture or a film of
/// the user's own, standing where the scene would be.
///
/// Named here so that this greeter recognises it rather than merely failing to,
/// because the two look identical from a settings file and mean opposite things.
/// An unknown value is a shell newer than this program, or a hand-typed
/// mistake, and falling back is a guess. This one is neither: it is a setting
/// this program understands perfectly and deliberately does not carry out.
///
/// It cannot. The picture is a file under one account's home directory —
/// `wallpaper-file` in the same settings file — and this login screen stands in
/// front of every account on the machine, running as its own user, before any of
/// them has been unlocked. Reading it would mean a greeter that opens files out
/// of people's home directories, which is not a thing this program is going to
/// be. So the answer is the shell's own scene, in the accent that account chose,
/// which is what the session behind it draws for the same setting whenever its
/// picture cannot be read.
///
/// The shell's own name for this is `lxb_protocol::wallpaper::CUSTOM`, and the
/// two spellings have to match for the same reason `THEMES` and its two do.
pub const CUSTOM_WALLPAPER: &str = "Custom wallpaper";

/// The key each half of the shell's Theme setting is written under, in the order
/// its Settings page lists them: the picture behind everything, then every mark
/// drawn on top of it.
///
/// Both are read here, because this login screen is both — it draws that same
/// wallpaper and it draws the shell's own marks in its clock, its arrows and its
/// buttons. See `visual::theme::Part`, which is the half itself.
pub const THEME_KEYS: [&str; 2] = ["theme-wallpaper", "theme-icons"];

/// What both halves were written under before they were two settings.
///
/// Read where a half has no key of its own and never written by anything: one
/// `theme` key said what the whole shell was made of, and a file left by that
/// shell meant it about both. Without this a machine deliberately stood down to
/// `Simple` comes back up in the water at the next login.
pub const LEGACY_THEME_KEY: &str = "theme";

/// How much material one half of the shell — and of this login screen in front
/// of it — is drawn with.
///
/// The shell's own look is expensive on purpose: the wallpaper's current is
/// three sheets of water lit as bodies, and every mark is a bead of water shaded
/// out of its own distance field. `Simple` stands the whole of that down to the
/// drawing underneath, for a machine that cannot afford it. The *drawings* are
/// the same either way, which is what keeps one screen recognisable as the next.
///
/// One value of this is half an answer: the wallpaper and the marks are separate
/// settings, because they are separate expenses and separate tastes.
///
/// This is LineXinBar's `wallpaper::Style` under another name, because this
/// program vendors that scene rather than depending on the crate. The two
/// spellings and their meanings have to match; `lxb-wallpaper-v2` covers both
/// materials, since either end of a handover reads the same key and draws what
/// it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    Default,
    Simple,
}

impl Style {
    pub fn name(self) -> &'static str {
        match self {
            Self::Default => THEMES[0],
            Self::Simple => THEMES[1],
        }
    }
}

/// The style of that name, or the default one for anything else — which
/// includes a wallpaper of the user's own; see [`CUSTOM_WALLPAPER`].
pub fn style(name: &str) -> Style {
    match canonical_theme(name) {
        Some("Simple") => Style::Simple,
        _ => Style::Default,
    }
}

/// The canonical spelling of a theme this greeter knows, matched the way
/// [`canonical`] matches an accent: without regard to case, because the shell
/// reads a hand-typed `simple` as the setting too.
///
/// A wallpaper of the user's own answers with the default material rather than
/// with `None`, which is the difference between a value this program understands
/// and cannot carry out and a value it does not recognise at all. Both end up
/// drawing the same picture; only one of them is a decision. See
/// [`CUSTOM_WALLPAPER`].
pub fn canonical_theme(value: &str) -> Option<&'static str> {
    if value.eq_ignore_ascii_case(CUSTOM_WALLPAPER) {
        return Some(DEFAULT_THEME);
    }
    THEMES
        .into_iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(value))
}

/// LineXinBar's default settings path for a user with no `XDG_CONFIG_HOME`
/// override.
pub fn settings_path(home: &Path) -> PathBuf {
    settings_path_with_config_home(home, None)
}

/// Resolve the same settings location as LineXinBar. Relative
/// `XDG_CONFIG_HOME` values are deliberately ignored, matching the shell.
pub fn settings_path_with_config_home(home: &Path, config_home: Option<&Path>) -> PathBuf {
    config_home
        .filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".config"))
        .join("lxb/shell.toml")
}

pub fn canonical(value: &str) -> Option<&'static str> {
    crate::visual::theme::ACCENTS
        .iter()
        .map(|accent| accent.name)
        .find(|candidate| candidate.eq_ignore_ascii_case(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names, matched the way a hand-typed setting has to be matched.
    ///
    /// What used to be tested through a file is tested here on the words
    /// themselves, because the file is no longer this module's to open — the
    /// published copy is, and `look::Look` is what reads it. See the tests
    /// beside [`crate::look::Look::theme`] for the same questions asked of the
    /// thing that now answers them.
    #[test]
    fn a_palette_and_a_material_are_matched_without_regard_to_case() {
        assert_eq!(canonical("green"), Some("Green"));
        assert_eq!(canonical("GREEN"), Some("Green"));
        assert_eq!(
            canonical("Chartreuse"),
            None,
            "a palette the shell does not offer is no answer at all"
        );
        assert_eq!(canonical_theme("simple"), Some("Simple"));
        assert_eq!(canonical_theme("Frosted"), None);
    }

    /// An account whose shell stands one of their own pictures behind
    /// everything is greeted by the shell's own scene instead.
    ///
    /// Not a fallback: the value is understood and deliberately not carried
    /// out, because the picture is a file under that account's home directory
    /// and this program does not open those. What must never happen is the
    /// greeter refusing the whole look over it.
    #[test]
    fn a_shell_showing_the_users_own_picture_is_greeted_by_the_default_scene() {
        assert_eq!(canonical_theme(CUSTOM_WALLPAPER), Some(DEFAULT_THEME));
        assert_eq!(style(CUSTOM_WALLPAPER), Style::Default);
    }

    #[test]
    fn follows_only_absolute_config_home_overrides() {
        let home = Path::new("/home/alex");
        assert_eq!(
            settings_path_with_config_home(home, Some(Path::new("/var/lib/alex-config"))),
            Path::new("/var/lib/alex-config/lxb/shell.toml")
        );
        assert_eq!(
            settings_path_with_config_home(home, Some(Path::new("relative-config"))),
            Path::new("/home/alex/.config/lxb/shell.toml")
        );
    }
}
