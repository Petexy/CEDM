//! Discover desktop sessions from freedesktop session desktop files.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Wayland,
    X11,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub comment: Option<String>,
    pub command: Vec<String>,
    pub desktop_names: Vec<String>,
    pub kind: Kind,
    pub source: PathBuf,
    pub line_xin_bar: bool,
}

/// Where the packaged session wrapper lives when nothing says otherwise.
const DEFAULT_LAUNCHER: &str = "/usr/bin/cedm-session";

/// The greeter's own wrapper names the installed path here, because it knows
/// where it was installed and this binary does not. A package that puts both
/// somewhere other than `/usr/bin` — Nix does — stays consistent for free.
const LAUNCHER_ENV: &str = "CEDM_SESSION_LAUNCHER";

/// The session wrapper to start sessions behind, if one is installed.
///
/// Resolved against the filesystem rather than assumed: a wrapper that is not
/// there is not an error, and a login screen that refuses to log anybody in
/// because a helper script is missing would be a worse failure than the
/// console output the wrapper exists to prevent.
pub fn launcher() -> Option<String> {
    let configured = std::env::var_os(LAUNCHER_ENV).filter(|value| !value.is_empty());
    let path = configured.unwrap_or_else(|| DEFAULT_LAUNCHER.into());
    let path = Path::new(&path);
    (path.is_absolute() && is_executable(path))
        .then(|| path.to_str())
        .flatten()
        .map(str::to_string)
}

impl Session {
    /// The argv greetd is asked to start.
    ///
    /// The desktop entry's own command behind the packaged wrapper, which is
    /// what moves a session's output off the login VT and sources the profile
    /// greetd has been told to stop sourcing. [`Session::command`] itself
    /// stays the entry's argv: that is what the desktop file promised, and
    /// what this greeter shows the person choosing it.
    pub fn launch_command(&self, launcher: Option<&str>) -> Vec<String> {
        let Some(launcher) = launcher else {
            return self.command.clone();
        };
        let mut argv = Vec::with_capacity(self.command.len() + 1);
        argv.push(launcher.to_string());
        argv.extend_from_slice(&self.command);
        argv
    }

    pub fn environment(&self) -> Vec<String> {
        let current_desktop = self.desktop_names.join(":");
        let session_type = match self.kind {
            Kind::Wayland => "wayland",
            Kind::X11 => "x11",
        };
        vec![
            format!("XDG_SESSION_TYPE={session_type}"),
            format!("XDG_SESSION_DESKTOP={}", self.id),
            format!("XDG_CURRENT_DESKTOP={current_desktop}"),
            format!("DESKTOP_SESSION={}", self.id),
        ]
    }
}

pub fn standard_directories() -> Vec<(PathBuf, Kind)> {
    let mut roots = Vec::new();
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .filter(|roots| !roots.is_empty())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });
    for root in data_dirs {
        roots.push((root.join("wayland-sessions"), Kind::Wayland));
        roots.push((root.join("xsessions"), Kind::X11));
    }
    roots
}

pub fn discover(roots: &[(PathBuf, Kind)]) -> Vec<Session> {
    let mut found = BTreeMap::new();
    for (root, kind) in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("desktop") {
                continue;
            }
            if let Some(session) = parse(&path, *kind) {
                // A distribution may intentionally provide both x11 and
                // Wayland entries with the same filename. Keep both while the
                // first XDG data root still wins within each session kind.
                found
                    .entry((session.id.clone(), session.kind))
                    .or_insert(session);
            }
        }
    }
    found.into_values().collect()
}

pub fn parse(path: &Path, kind: Kind) -> Option<Session> {
    let raw = fs::read_to_string(path).ok()?;
    let mut section = false;
    let mut fields = BTreeMap::<String, String>::new();
    for original in raw.lines() {
        let line = original.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line == "[Desktop Entry]";
            continue;
        }
        if !section || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        // Keep the unqualified name/comment; locale selection is a renderer concern.
        if key.contains('[') {
            continue;
        }
        fields
            .entry(key.to_string())
            .or_insert_with(|| value.to_string());
    }
    if fields
        .get("Type")
        .is_some_and(|value| value != "Application")
    {
        return None;
    }
    if fields
        .get("Hidden")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
        || fields
            .get("NoDisplay")
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    {
        return None;
    }
    let name = fields.remove("Name")?;
    if fields
        .get("TryExec")
        .is_some_and(|command| !executable_available(command))
    {
        return None;
    }
    let icon = fields.get("Icon").cloned();
    let command = parse_exec(&fields.remove("Exec")?, &name, icon.as_deref(), path)?;
    let id = path.file_stem()?.to_str()?.to_string();
    let mut desktop_names = fields
        .remove("DesktopNames")
        .map(|names| {
            names
                .split(';')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| vec![id.clone()]);
    if desktop_names.is_empty() {
        desktop_names.push(id.clone());
    }
    let line_xin_bar = desktop_names
        .iter()
        .any(|name| name.eq_ignore_ascii_case("LineXinBar"))
        || command.first().is_some_and(|command| {
            Path::new(command)
                .file_name()
                .is_some_and(|name| name == "lxb-session")
        });
    Some(Session {
        id,
        name,
        comment: fields.remove("Comment"),
        command,
        desktop_names,
        kind,
        source: path.to_path_buf(),
        line_xin_bar,
    })
}

fn parse_exec(exec: &str, name: &str, icon: Option<&str>, source: &Path) -> Option<Vec<String>> {
    let words = shlex::split(exec)?;
    let source = source.to_str()?;
    let mut result = Vec::new();
    for (index, word) in words.into_iter().enumerate() {
        if word == "%i" {
            if index == 0 {
                return None;
            }
            if let Some(icon) = icon.filter(|icon| !icon.is_empty()) {
                result.push("--icon".to_string());
                result.push(icon.to_string());
            }
            continue;
        }
        let Some(clean) = expand_exec_word(&word, name, source)? else {
            if index == 0 {
                return None;
            }
            continue;
        };
        if index == 0 && clean.is_empty() {
            return None;
        }
        result.push(clean);
    }
    let executable = result.first()?;
    if executable.contains('=') || result.iter().any(|argument| argument.contains('\0')) {
        return None;
    }
    Some(result)
}

/// Expand desktop-entry field codes without invoking a shell. Login sessions
/// have no file/URL argument, so those codes disappear. Unknown codes make the
/// entry invalid instead of silently changing its command line.
fn expand_exec_word(word: &str, name: &str, source: &str) -> Option<Option<String>> {
    let mut expanded = String::new();
    let mut removed = false;
    let mut chars = word.chars();
    while let Some(character) = chars.next() {
        if character != '%' {
            expanded.push(character);
            continue;
        }
        match chars.next()? {
            '%' => expanded.push('%'),
            'f' | 'F' | 'u' | 'U' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm' => removed = true,
            'c' => expanded.push_str(name),
            'k' => expanded.push_str(source),
            // %i expands to two argv entries and is only valid as its own word.
            'i' => return None,
            _ => return None,
        }
    }
    if expanded.is_empty() && removed {
        Some(None)
    } else {
        Some(Some(expanded))
    }
}

fn executable_available(command: &str) -> bool {
    if command.is_empty() || command.contains('\0') {
        return false;
    }
    let candidate = Path::new(command);
    if candidate.components().count() > 1 {
        return candidate.is_absolute() && is_executable(candidate);
    }
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|root| root.join(candidate))
                .any(|candidate| is_executable(&candidate))
        })
        .unwrap_or(false)
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_line_xin_bar_without_hard_coding_its_install_location() {
        let root = std::env::temp_dir().join(format!("cedm-session-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("lxb.desktop");
        fs::write(
            &path,
            "[Desktop Entry]\nName=LineXinBar\nExec=lxb-session\nDesktopNames=LineXinBar;\n",
        )
        .unwrap();
        let session = parse(&path, Kind::Wayland).unwrap();
        assert!(session.line_xin_bar);
        assert_eq!(session.command, ["lxb-session"]);
        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
    }

    /// The wrapper goes in front of the entry's argv and changes nothing else.
    /// Its absence has to stay survivable: a machine whose wrapper did not get
    /// installed must still be able to log somebody in, because the wrapper
    /// exists to keep console output off a screen, not to gate authentication.
    #[test]
    fn a_session_starts_behind_the_wrapper_and_without_it_when_there_is_none() {
        let root = std::env::temp_dir().join(format!("cedm-launch-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("plasma.desktop");
        fs::write(
            &path,
            "[Desktop Entry]\nName=Plasma\nExec=/usr/bin/startplasma-wayland --wait\n",
        )
        .unwrap();
        let session = parse(&path, Kind::Wayland).unwrap();

        assert_eq!(
            session.launch_command(Some("/usr/bin/cedm-session")),
            [
                "/usr/bin/cedm-session",
                "/usr/bin/startplasma-wayland",
                "--wait"
            ]
        );
        assert_eq!(
            session.launch_command(None),
            session.command,
            "a missing wrapper starts the session, it does not refuse it"
        );

        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
    }

    /// The environment variable is the greeter wrapper telling the binary where
    /// it was installed. A relative path, or one naming something that is not
    /// there, must resolve to no wrapper rather than to a broken argv[0].
    #[test]
    fn only_an_absolute_executable_path_is_accepted_as_the_wrapper() {
        let root = std::env::temp_dir().join(format!("cedm-launcher-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let missing = root.join("not-installed");
        assert!(!is_executable(&missing));

        let present = root.join("cedm-session");
        fs::write(&present, "#!/bin/sh\nexec \"$@\"\n").unwrap();
        fs::set_permissions(&present, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&present));
        assert!(!is_executable(Path::new("cedm-session")));

        fs::remove_file(&present).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn strips_desktop_field_codes_from_other_sessions() {
        let root = std::env::temp_dir().join(format!("cedm-plasma-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("plasma.desktop");
        fs::write(&path, "[Desktop Entry]\nName=Plasma\nExec=/usr/bin/startplasma-wayland %f\nDesktopNames=KDE;\n").unwrap();
        let session = parse(&path, Kind::Wayland).unwrap();
        assert!(!session.line_xin_bar);
        assert_eq!(session.command, ["/usr/bin/startplasma-wayland"]);
        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn keeps_direct_argv_and_rejects_unknown_field_codes() {
        let root = std::env::temp_dir().join(format!("cedm-argv-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let valid = root.join("valid.desktop");
        fs::write(
            &valid,
            "[Desktop Entry]\nName=Other Desktop\nExec=/opt/session --label %c --literal %% %U\n",
        )
        .unwrap();
        assert_eq!(
            parse(&valid, Kind::Wayland).unwrap().command,
            ["/opt/session", "--label", "Other Desktop", "--literal", "%"]
        );

        let invalid = root.join("invalid.desktop");
        fs::write(
            &invalid,
            "[Desktop Entry]\nName=Bad\nExec=/opt/session %Z\n",
        )
        .unwrap();
        assert!(parse(&invalid, Kind::Wayland).is_none());
        fs::remove_file(valid).unwrap();
        fs::remove_file(invalid).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn classifies_only_authoritative_lxb_names_or_executables() {
        let root = std::env::temp_dir().join(format!("cedm-lxb-class-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let unrelated = root.join("lxb.desktop");
        fs::write(
            &unrelated,
            "[Desktop Entry]\nName=Unrelated\nExec=/opt/not-lxb-session\n",
        )
        .unwrap();
        assert!(!parse(&unrelated, Kind::Wayland).unwrap().line_xin_bar);
        let executable = root.join("renamed.desktop");
        fs::write(
            &executable,
            "[Desktop Entry]\nName=LineXinBar\nExec=/usr/bin/lxb-session\n",
        )
        .unwrap();
        assert!(parse(&executable, Kind::Wayland).unwrap().line_xin_bar);
        fs::remove_file(unrelated).unwrap();
        fs::remove_file(executable).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
