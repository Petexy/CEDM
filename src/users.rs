//! Enumerate interactive local accounts without depending on a desktop stack.

use crate::i18n;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_UID_MIN: u32 = 1000;
const DEFAULT_UID_MAX: u32 = 60_000;
/// Large enough for directory-qualified names while keeping all greeter-side
/// buffers and greetd request frames predictably bounded.
pub const MAX_LOGIN_NAME_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginNameError {
    Empty,
    TooLong,
    Whitespace,
    ControlCharacter,
}

impl LoginNameError {
    /// What is written under the field, in the machine's language.
    ///
    /// Still a `&'static str`: the language is settled before the first frame
    /// and does not move again, so a refusal held in the screen's state is
    /// held in the language it will be read in.
    pub fn message(self) -> &'static str {
        let text = i18n::text();
        match self {
            Self::Empty => text.name_empty,
            Self::TooLong => text.name_too_long,
            Self::Whitespace => text.name_whitespace,
            Self::ControlCharacter => text.name_control,
        }
    }
}

/// Validate a login identifier without resolving or normalising it.
///
/// PAM modules may accept local names, NIS names, UPNs such as
/// `person@example.test`, or domain-qualified names such as `DOMAIN\\person`.
/// CEDM therefore keeps the character policy deliberately broad. It rejects
/// only values that cannot be safely and legibly entered as one identifier,
/// and passes an accepted value to greetd byte-for-byte.
pub fn validate_login_name(name: &str) -> Result<(), LoginNameError> {
    if name.is_empty() {
        return Err(LoginNameError::Empty);
    }
    if name.len() > MAX_LOGIN_NAME_BYTES {
        return Err(LoginNameError::TooLong);
    }
    if name.chars().any(char::is_control) {
        return Err(LoginNameError::ControlCharacter);
    }
    if name.chars().any(char::is_whitespace) {
        return Err(LoginNameError::Whitespace);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub name: String,
    pub display_name: String,
    pub uid: u32,
    pub home: PathBuf,
    pub shell: PathBuf,
    /// The account's published picture, if the system has one and the greeter
    /// could make sense of it.
    ///
    /// Set from `/var/lib/AccountsService/icons` — see [`crate::faces`] for why
    /// that is the only place looked in and why an unprivileged login screen is
    /// allowed to read it. Cleared again if the file turns out not to be a
    /// picture this can decode, so that anything holding a `User` can take this
    /// as "there is a face to draw" rather than "there is a file to try".
    pub avatar: Option<PathBuf>,
}

pub fn discover() -> Vec<User> {
    let (minimum_uid, maximum_uid) = uid_range(Path::new("/etc/login.defs"));
    let login_shells = valid_shells(Path::new("/etc/shells"));
    parse_passwd_impl(
        Path::new("/etc/passwd"),
        minimum_uid,
        maximum_uid,
        (!login_shells.is_empty()).then_some(&login_shells),
    )
}

pub fn parse_passwd(path: &Path, minimum_uid: u32) -> Vec<User> {
    parse_passwd_impl(path, minimum_uid, u32::MAX, None)
}

fn parse_passwd_impl(
    path: &Path,
    minimum_uid: u32,
    maximum_uid: u32,
    login_shells: Option<&HashSet<PathBuf>>,
) -> Vec<User> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut users = Vec::new();
    let mut seen_names = HashSet::new();
    for line in raw.lines() {
        let fields = line.split(':').collect::<Vec<_>>();
        if fields.len() != 7 || fields[0].is_empty() {
            continue;
        }
        let Ok(uid) = fields[2].parse::<u32>() else {
            continue;
        };
        let shell = PathBuf::from(fields[6]);
        let home = PathBuf::from(fields[5]);
        if !(minimum_uid..=maximum_uid).contains(&uid)
            || !home.is_absolute()
            || !is_login_shell(&shell)
            || login_shells.is_some_and(|shells| !shells.contains(&shell))
            || !seen_names.insert(fields[0].to_string())
        {
            continue;
        }
        let display_name = fields[4]
            .split(',')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(fields[0]);
        users.push(User {
            avatar: crate::faces::published(fields[0]),
            name: fields[0].to_string(),
            display_name: display_name.to_string(),
            uid,
            home,
            shell,
        });
    }
    users.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });
    users
}

fn uid_range(path: &Path) -> (u32, u32) {
    let Ok(raw) = fs::read_to_string(path) else {
        return (DEFAULT_UID_MIN, DEFAULT_UID_MAX);
    };
    let mut minimum = DEFAULT_UID_MIN;
    let mut maximum = DEFAULT_UID_MAX;
    for original in raw.lines() {
        let line = original.split('#').next().unwrap_or_default().trim();
        let mut fields = line.split_whitespace();
        let Some(key) = fields.next() else { continue };
        let Some(value) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        match key {
            "UID_MIN" => minimum = value,
            "UID_MAX" => maximum = value,
            _ => {}
        }
    }
    if minimum <= maximum {
        (minimum, maximum)
    } else {
        (DEFAULT_UID_MIN, DEFAULT_UID_MAX)
    }
}

fn valid_shells(path: &Path) -> HashSet<PathBuf> {
    fs::read_to_string(path)
        .ok()
        .into_iter()
        .flat_map(|raw| {
            raw.lines()
                .map(str::trim)
                .filter(|line| line.starts_with('/'))
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn is_login_shell(shell: &Path) -> bool {
    if !shell.is_absolute() {
        return false;
    }
    !matches!(
        shell.file_name().and_then(|name| name.to_str()),
        None | Some("nologin" | "false")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_human_accounts_and_excludes_service_accounts() {
        let path = std::env::temp_dir().join(format!("cedm-passwd-{}", std::process::id()));
        fs::write(&path, "daemon:x:2:2:Daemon:/sbin:/usr/bin/nologin\nalex:x:1000:1000:Alex Example:/home/alex:/bin/bash\nservice:x:1001:1001::/srv/service:/bin/false\n").unwrap();
        let users = parse_passwd(&path, 1000);
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].display_name, "Alex Example");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn recognizes_disabled_shells_at_nonstandard_paths() {
        let path = std::env::temp_dir().join(format!("cedm-disabled-{}", std::process::id()));
        fs::write(
            &path,
            "alex:x:1000:1000:Alex:/home/alex:/usr/local/sbin/nologin\n",
        )
        .unwrap();
        assert!(parse_passwd(&path, 1000).is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn reads_the_distribution_login_uid_range() {
        let path = std::env::temp_dir().join(format!("cedm-login-defs-{}", std::process::id()));
        fs::write(&path, "UID_MIN 1200\nUID_MAX 64000 # local accounts\n").unwrap();
        assert_eq!(uid_range(&path), (1200, 64_000));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn directory_login_names_are_broad_but_bounded() {
        for name in [
            "alex",
            "hidden-user",
            "person@example.test",
            "EXAMPLE\\person",
            "münchen.user",
        ] {
            assert_eq!(validate_login_name(name), Ok(()), "{name}");
        }
        assert_eq!(validate_login_name(""), Err(LoginNameError::Empty));
        assert_eq!(
            validate_login_name("two people"),
            Err(LoginNameError::Whitespace)
        );
        assert_eq!(
            validate_login_name("person\nadmin"),
            Err(LoginNameError::ControlCharacter)
        );
        assert_eq!(
            validate_login_name(&"x".repeat(MAX_LOGIN_NAME_BYTES + 1)),
            Err(LoginNameError::TooLong)
        );
        assert_eq!(
            validate_login_name(&"x".repeat(MAX_LOGIN_NAME_BYTES)),
            Ok(())
        );
    }
}
