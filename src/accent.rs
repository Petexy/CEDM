//! Read the selected user's LineXinBar palette without adopting their account.

use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const DEFAULT_ACCENT: &str = "Purple";
pub const ACCENTS: [&str; 5] = ["Purple", "Blue", "Green", "Yellow", "Red"];
const MAX_SETTINGS_BYTES: u64 = 256 * 1024;

/// Every material the shell can be set to draw itself in, in the order its
/// Settings column lists them — the same two for the wallpaper as for the marks.
/// The first is what an unreadable or unrecognised setting falls back to.
pub const THEMES: [&str; 2] = ["Default", "Simple"];
pub const DEFAULT_THEME: &str = THEMES[0];

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

/// The style of that name, or the default one for anything else.
pub fn style(name: &str) -> Style {
    match canonical_theme(name) {
        Some("Simple") => Style::Simple,
        _ => Style::Default,
    }
}

/// The canonical spelling of a theme this greeter knows, matched the way
/// [`canonical`] matches an accent: without regard to case, because the shell
/// reads a hand-typed `simple` as the setting too.
pub fn canonical_theme(value: &str) -> Option<&'static str> {
    THEMES
        .into_iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(value))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct ShellSettings {
    accent: Option<String>,
    theme_wallpaper: Option<String>,
    theme_icons: Option<String>,
    /// What the two above were written under before they were two settings. See
    /// [`LEGACY_THEME_KEY`].
    theme: Option<String>,
}

impl ShellSettings {
    /// The material named for one half, or the one the whole shell was set to
    /// before there were two halves.
    fn theme(&self, part: crate::visual::theme::Part) -> Option<&String> {
        match part {
            crate::visual::theme::Part::Wallpaper => self.theme_wallpaper.as_ref(),
            crate::visual::theme::Part::Icons => self.theme_icons.as_ref(),
        }
        .or(self.theme.as_ref())
    }
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

pub fn read_for_home(home: &Path) -> String {
    read_path(&settings_path(home)).unwrap_or_else(|| DEFAULT_ACCENT.to_string())
}

/// The material an account's shell is set to draw one half of itself in, out of
/// the same file the accent comes from.
///
/// Read separately rather than returned beside the accent: the two are wanted in
/// different places — one tints the whole screen and the other decides what it is
/// made of — and a settings file is a few hundred bytes read once per account.
pub fn read_theme_for_home(home: &Path, part: crate::visual::theme::Part) -> Option<String> {
    read_theme_path(&settings_path(home), part)
}

pub fn read_theme_path(path: &Path, part: crate::visual::theme::Part) -> Option<String> {
    canonical_theme(read_settings(path)?.theme(part)?).map(str::to_string)
}

pub fn read_path(path: &Path) -> Option<String> {
    canonical(&read_settings(path)?.accent?).map(str::to_string)
}

fn read_settings(path: &Path) -> Option<ShellSettings> {
    // A corrupt or hostile user-owned settings file must not make the greeter
    // allocate without bound. Reading from the opened descriptor also avoids
    // a metadata/read time-of-check race.
    let file = File::open(path).ok()?;
    let mut raw = String::new();
    file.take(MAX_SETTINGS_BYTES + 1)
        .read_to_string(&mut raw)
        .ok()?;
    if raw.len() as u64 > MAX_SETTINGS_BYTES {
        return None;
    }
    toml::from_str(&raw).ok()
}

pub fn canonical(value: &str) -> Option<&'static str> {
    ACCENTS
        .into_iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_the_shells_top_level_accent_and_ignores_the_rest() {
        let root = std::env::temp_dir().join(format!("cedm-accent-{}", std::process::id()));
        let path = root.join("shell.toml");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, "accent = \"green\"\n[display.TEST]\nhdr = true\n").unwrap();
        assert_eq!(read_path(&path).as_deref(), Some("Green"));
        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
    }

    /// The two halves of the shell's Theme setting, and the key they shared
    /// before there were two of them.
    ///
    /// This greeter draws both — that wallpaper, and the shell's own marks in its
    /// clock and its buttons — so it reads both keys. A file from the older shell
    /// says one thing about the whole of it and meant it about both, which is the
    /// difference between a machine that was stood down to `Simple` staying there
    /// across an update and one that comes back up in the water.
    #[test]
    fn reads_both_halves_of_the_theme_and_the_key_they_used_to_share() {
        let root = std::env::temp_dir().join(format!("cedm-theme-{}", std::process::id()));
        let path = root.join("shell.toml");
        fs::create_dir_all(&root).unwrap();
        let wallpaper = crate::visual::theme::Part::Wallpaper;
        let icons = crate::visual::theme::Part::Icons;

        fs::write(&path, "theme-wallpaper = \"simple\"\n").unwrap();
        assert_eq!(
            read_theme_path(&path, wallpaper).as_deref(),
            Some("Simple"),
            "matched without regard to case, as the accent is"
        );
        assert_eq!(read_theme_path(&path, icons), None);

        fs::write(&path, "theme = \"Simple\"\n").unwrap();
        for part in [wallpaper, icons] {
            assert_eq!(read_theme_path(&path, part).as_deref(), Some("Simple"));
        }

        fs::write(&path, "theme = \"Simple\"\ntheme-icons = \"Default\"\n").unwrap();
        assert_eq!(read_theme_path(&path, wallpaper).as_deref(), Some("Simple"));
        assert_eq!(
            read_theme_path(&path, icons).as_deref(),
            Some("Default"),
            "the newer, narrower key outranks the one it replaced"
        );

        fs::write(&path, "theme-wallpaper = \"Frosted\"\n").unwrap();
        assert_eq!(
            read_theme_path(&path, wallpaper),
            None,
            "a material this greeter has not got is no answer at all"
        );

        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn refuses_a_palette_the_shell_does_not_offer() {
        let root = std::env::temp_dir().join(format!("cedm-bad-accent-{}", std::process::id()));
        let path = root.join("shell.toml");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, "accent = \"Chartreuse\"\n").unwrap();
        assert_eq!(read_path(&path), None);
        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
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

    #[test]
    fn rejects_an_oversized_settings_file() {
        let root = std::env::temp_dir().join(format!("cedm-large-accent-{}", std::process::id()));
        let path = root.join("shell.toml");
        fs::create_dir_all(&root).unwrap();
        let mut contents = String::from("accent = \"Blue\"\n#");
        contents.push_str(&"x".repeat(MAX_SETTINGS_BYTES as usize));
        fs::write(&path, contents).unwrap();
        assert_eq!(read_path(&path), None);
        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
