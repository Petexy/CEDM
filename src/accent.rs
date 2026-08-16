//! Read the selected user's LineXinBar palette without adopting their account.

use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const DEFAULT_ACCENT: &str = "Purple";
pub const ACCENTS: [&str; 5] = ["Purple", "Blue", "Green", "Yellow", "Red"];
const MAX_SETTINGS_BYTES: u64 = 256 * 1024;

#[derive(Debug, Default, Deserialize)]
struct ShellSettings {
    accent: Option<String>,
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

pub fn read_path(path: &Path) -> Option<String> {
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
    let stored: ShellSettings = toml::from_str(&raw).ok()?;
    canonical(&stored.accent?).map(str::to_string)
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
