//! Broker-provided display state and unprivileged greeter preferences.
//!
//! Preferences contain only account names and desktop-file IDs. Authentication
//! answers and session commands never belong in this file.

use crate::config::{read_bounded, Config, MAX_TOML_BYTES};
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Read-only state supplied by the future privileged seat broker.
pub const PATH: &str = "/var/lib/console-experience-desktop-manager/state.toml";
pub const PREFERENCES_VERSION: u32 = 1;
const APPLICATION_DIRECTORY: &str = "console-experience-desktop-manager";
const PREFERENCES_FILENAME: &str = "preferences.toml";
const WALLPAPER_CLOCK_FILENAME: &str = "wallpaper-clock";
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

/// Read-only state written by the privileged broker. Keep this separate from
/// the greeter-owned preferences so a compromised greeter cannot impersonate
/// broker-provided accent data.
#[derive(Debug, Default, Deserialize)]
pub struct State {
    pub last_user: Option<String>,
    #[serde(default)]
    pub accents: BTreeMap<String, String>,
}

impl State {
    pub fn load() -> Self {
        let path = Path::new(PATH);
        if !path.exists() {
            return Self::default();
        }
        match Self::try_load_path(path) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    %error,
                    "ignoring invalid broker state"
                );
                Self::default()
            }
        }
    }

    pub fn load_path(path: &Path) -> Self {
        Self::try_load_path(path).unwrap_or_default()
    }

    pub fn try_load_path(path: &Path) -> anyhow::Result<Self> {
        let raw = read_bounded(path)?;
        toml::from_str(&raw)
            .with_context(|| format!("could not parse broker state {}", path.display()))
    }

    pub fn accent_for(&self, username: &str) -> Option<&str> {
        self.accents.get(username).map(String::as_str)
    }
}

/// Public, versioned schema owned by the unprivileged greeter account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    pub version: u32,
    pub last_user: Option<String>,
    pub last_session: Option<String>,
    #[serde(default)]
    pub preferred_sessions: BTreeMap<String, String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: PREFERENCES_VERSION,
            last_user: None,
            last_session: None,
            preferred_sessions: BTreeMap::new(),
        }
    }
}

impl Preferences {
    pub fn load() -> Self {
        let Some(path) = preferences_path() else {
            return Self::default();
        };
        if !path.exists() {
            return Self::default();
        }
        match Self::try_load_path(&path) {
            Ok(preferences) => preferences,
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    %error,
                    "ignoring invalid greeter preferences"
                );
                Self::default()
            }
        }
    }

    pub fn load_path(path: &Path) -> Self {
        Self::try_load_path(path).unwrap_or_default()
    }

    pub fn try_load_path(path: &Path) -> anyhow::Result<Self> {
        let raw = read_bounded(path)?;
        let preferences: Self = toml::from_str(&raw)
            .with_context(|| format!("could not parse greeter preferences {}", path.display()))?;
        if preferences.version != PREFERENCES_VERSION {
            bail!(
                "unsupported greeter preferences version {} in {}",
                preferences.version,
                path.display()
            );
        }
        Ok(preferences)
    }

    /// Save to the default unprivileged state location. Errors are returned so
    /// the caller can warn and continue the login; preferences are never a
    /// reason to prevent a session from starting.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = preferences_path().context(
            "cannot save greeter preferences because XDG_STATE_HOME and HOME are unavailable",
        )?;
        self.save_path(&path)
    }

    /// Atomically replace a preferences file using a same-directory temporary
    /// file. The containing application directory is mode 0700 and the file is
    /// mode 0600.
    pub fn save_path(&self, path: &Path) -> anyhow::Result<()> {
        if self.version != PREFERENCES_VERSION {
            bail!(
                "refusing to write unsupported preferences version {}",
                self.version
            );
        }
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .with_context(|| {
                format!(
                    "preferences path {} has no parent directory",
                    path.display()
                )
            })?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "could not create preferences directory {}",
                parent.display()
            )
        })?;
        let parent_metadata = fs::symlink_metadata(parent).with_context(|| {
            format!(
                "could not inspect preferences directory {}",
                parent.display()
            )
        })?;
        if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
            bail!(
                "preferences parent {} is not a real directory",
                parent.display()
            );
        }
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).with_context(|| {
            format!(
                "could not secure preferences directory {}",
                parent.display()
            )
        })?;

        let document = toml::to_string_pretty(self).context("could not serialize preferences")?;
        if document.len() > MAX_TOML_BYTES {
            bail!(
                "serialized preferences are larger than {} bytes",
                MAX_TOML_BYTES
            );
        }

        let temporary = temporary_path(path)?;
        let write_result = (|| -> anyhow::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
                .with_context(|| {
                    format!(
                        "could not create temporary preferences {}",
                        temporary.display()
                    )
                })?;
            file.write_all(document.as_bytes()).with_context(|| {
                format!(
                    "could not write temporary preferences {}",
                    temporary.display()
                )
            })?;
            file.sync_all().with_context(|| {
                format!(
                    "could not sync temporary preferences {}",
                    temporary.display()
                )
            })?;
            drop(file);
            fs::rename(&temporary, path).with_context(|| {
                format!(
                    "could not atomically replace preferences {}",
                    path.display()
                )
            })?;
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .with_context(|| {
                    format!("could not sync preferences directory {}", parent.display())
                })?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result
    }

    /// Update the public preference fields after a session was successfully
    /// accepted by greetd. Policy-disabled categories are removed rather than
    /// silently retained for a future run.
    pub fn record_success(&mut self, config: &Config, username: &str, session_id: &str) {
        if config.remember_user {
            self.last_user = nonempty(username).map(str::to_string);
        } else {
            self.last_user = None;
        }

        if config.remember_session {
            self.last_session = nonempty(session_id).map(str::to_string);
            if config.remember_user {
                if let (Some(username), Some(session_id)) =
                    (nonempty(username), nonempty(session_id))
                {
                    self.preferred_sessions
                        .insert(username.to_string(), session_id.to_string());
                }
            } else {
                // A per-user mapping still records the account even if it is
                // not called `last_user`. Honour the account-memory policy by
                // retaining only the anonymous global session choice.
                self.preferred_sessions.clear();
            }
        } else {
            self.last_session = None;
            self.preferred_sessions.clear();
        }
    }

    /// Remember only the non-identifying part of a successful login.
    ///
    /// The "Other account" route is suitable for hidden and directory users.
    /// Its typed identifier must not become a profile, a `last_user`, or a
    /// per-user preference key. The global session choice remains useful and
    /// does not identify the account.
    pub fn record_anonymous_success(&mut self, config: &Config, session_id: &str) {
        if !config.remember_user {
            self.last_user = None;
        }
        if config.remember_session {
            self.last_session = nonempty(session_id).map(str::to_string);
        } else {
            self.last_session = None;
        }
        if !config.remember_user || !config.remember_session {
            self.preferred_sessions.clear();
        }
    }

    /// User remembered by greeter preferences when administrator policy allows
    /// it. The caller still decides whether that account should be displayed.
    pub fn remembered_user<'a>(&'a self, config: &Config) -> Option<&'a str> {
        config
            .remember_user
            .then_some(self.last_user.as_deref())
            .flatten()
            .map(str::trim)
            .filter(|username| !username.is_empty())
    }

    /// Return preference candidates in precedence order. The caller validates
    /// each desktop-file ID against fresh discovery before trying the next;
    /// this lets a removed per-user desktop fall through to a valid global or
    /// administrator choice instead of skipping the rest of the chain.
    pub fn session_candidates<'a>(
        &'a self,
        config: &'a Config,
        username: Option<&str>,
    ) -> Vec<&'a str> {
        let mut candidates = Vec::with_capacity(3);
        if config.remember_session {
            if config.remember_user {
                if let Some(id) = username
                    .and_then(|username| self.preferred_sessions.get(username))
                    .map(String::as_str)
                    .and_then(nonempty)
                {
                    candidates.push(id);
                }
            }
            if let Some(id) = self.last_session.as_deref().and_then(nonempty) {
                if !candidates.contains(&id) {
                    candidates.push(id);
                }
            }
        }
        if let Some(id) = config.default_session_id() {
            if !candidates.contains(&id) {
                candidates.push(id);
            }
        }
        candidates
    }

    /// First unvalidated candidate, retained as a compact convenience API.
    pub fn session_id<'a>(&'a self, config: &'a Config, username: Option<&str>) -> Option<&'a str> {
        self.session_candidates(config, username).into_iter().next()
    }
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

/// Resolve the greeter-owned preference location according to the XDG base
/// directory specification. Relative or empty environment paths are ignored.
pub fn preferences_path() -> Option<PathBuf> {
    preferences_path_from(
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

pub fn preferences_path_from(
    xdg_state_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Option<PathBuf> {
    state_path_from(xdg_state_home, home, PREFERENCES_FILENAME)
}

/// Where the wallpaper's clock is anchored for this boot. Beside the
/// preferences, and greeter-owned for the same reason: it is written and read
/// by this account and nothing outside it has any business with it. See
/// [`crate::handoff::SceneClock::of_this_boot`].
pub fn wallpaper_clock_path() -> Option<PathBuf> {
    state_path_from(
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
        WALLPAPER_CLOCK_FILENAME,
    )
}

fn state_path_from(
    xdg_state_home: Option<&OsStr>,
    home: Option<&OsStr>,
    filename: &str,
) -> Option<PathBuf> {
    absolute_nonempty(xdg_state_home)
        .map(|root| root.join(APPLICATION_DIRECTORY).join(filename))
        .or_else(|| {
            absolute_nonempty(home).map(|root| {
                root.join(".local")
                    .join("state")
                    .join(APPLICATION_DIRECTORY)
                    .join(filename)
            })
        })
}

fn absolute_nonempty(value: Option<&OsStr>) -> Option<PathBuf> {
    let path = PathBuf::from(value?);
    (path.is_absolute() && !path.as_os_str().is_empty()).then_some(path)
}

fn temporary_path(destination: &Path) -> anyhow::Result<PathBuf> {
    let parent = destination
        .parent()
        .context("preferences destination has no parent")?;
    let filename = destination
        .file_name()
        .context("preferences destination has no filename")?
        .to_string_lossy();
    for _ in 0..32 {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{filename}.tmp-{}-{sequence}", std::process::id()));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!(
        "could not allocate a temporary preferences name beside {}",
        destination.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    fn test_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "cedm-state-{}-{}-{name}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn loads_last_user_and_cached_palette_from_broker_state() {
        let root = test_root("broker");
        let path = root.join("state.toml");
        fs::write(&path, "last_user = \"alex\"\n[accents]\nalex = \"Blue\"\n").unwrap();
        let state = State::load_path(&path);
        assert_eq!(state.last_user.as_deref(), Some("alex"));
        assert_eq!(state.accent_for("alex"), Some("Blue"));
        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn broker_state_reads_are_bounded_and_corruption_is_ignored() {
        let root = test_root("broker-invalid");
        let corrupt = root.join("corrupt.toml");
        fs::write(&corrupt, "accents = [broken").unwrap();
        assert!(State::try_load_path(&corrupt).is_err());
        assert!(State::load_path(&corrupt).last_user.is_none());

        let oversized = root.join("oversized.toml");
        fs::write(&oversized, vec![b'x'; MAX_TOML_BYTES + 1]).unwrap();
        assert!(State::try_load_path(&oversized).is_err());
        fs::remove_file(corrupt).unwrap();
        fs::remove_file(oversized).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn preferences_round_trip_atomically_with_private_modes() {
        let root = test_root("roundtrip");
        let path = root.join("app").join("preferences.toml");
        let mut preferences = Preferences {
            last_user: Some("alex".into()),
            last_session: Some("plasma".into()),
            ..Preferences::default()
        };
        preferences
            .preferred_sessions
            .insert("alex".into(), "lxb".into());
        preferences.save_path(&path).unwrap();
        assert_eq!(Preferences::try_load_path(&path).unwrap(), preferences);
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        preferences.last_session = Some("gamescope-session".into());
        preferences.save_path(&path).unwrap();
        assert_eq!(
            Preferences::try_load_path(&path)
                .unwrap()
                .last_session
                .as_deref(),
            Some("gamescope-session")
        );

        Preferences::default().save_path(&path).unwrap();
        assert_eq!(
            Preferences::try_load_path(&path).unwrap(),
            Preferences::default()
        );
        fs::remove_file(path).unwrap();
        fs::remove_dir(root.join("app")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn invalid_preferences_fall_back_without_becoming_current_schema() {
        let root = test_root("invalid-preferences");
        let corrupt = root.join("corrupt.toml");
        fs::write(&corrupt, "version = 1\nlast_user = [broken").unwrap();
        assert_eq!(Preferences::load_path(&corrupt), Preferences::default());

        let future = root.join("future.toml");
        fs::write(
            &future,
            "version = 99\nlast_user = \"alex\"\npreferred_sessions = {}\n",
        )
        .unwrap();
        assert!(Preferences::try_load_path(&future).is_err());

        let oversized = root.join("oversized.toml");
        fs::write(&oversized, vec![b'x'; MAX_TOML_BYTES + 1]).unwrap();
        assert!(Preferences::try_load_path(&oversized).is_err());
        fs::remove_file(corrupt).unwrap();
        fs::remove_file(future).unwrap();
        fs::remove_file(oversized).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn resolves_session_ids_without_treating_them_as_commands() {
        let mut preferences = Preferences {
            last_session: Some("plasma".into()),
            ..Preferences::default()
        };
        preferences
            .preferred_sessions
            .insert("alex".into(), "lxb".into());
        let config = Config {
            default_session: Some("fallback".into()),
            ..Config::default()
        };
        assert_eq!(preferences.session_id(&config, Some("alex")), Some("lxb"));
        assert_eq!(preferences.session_id(&config, Some("sam")), Some("plasma"));
        assert_eq!(
            preferences.session_candidates(&config, Some("alex")),
            ["lxb", "plasma", "fallback"]
        );

        let no_memory = Config {
            remember_user: false,
            remember_session: false,
            ..config.clone()
        };
        preferences.last_user = Some("alex".into());
        assert_eq!(preferences.remembered_user(&no_memory), None);
        assert_eq!(
            preferences.session_id(&no_memory, Some("alex")),
            Some("fallback")
        );

        preferences.record_success(&no_memory, "sam", "plasma");
        assert_eq!(preferences.last_user, None);
        assert_eq!(preferences.last_session, None);
        assert!(preferences.preferred_sessions.is_empty());

        preferences.record_success(&config, " sam ", " lxb ");
        assert_eq!(preferences.last_user.as_deref(), Some("sam"));
        assert_eq!(preferences.last_session.as_deref(), Some("lxb"));
        assert_eq!(
            preferences
                .preferred_sessions
                .get("sam")
                .map(String::as_str),
            Some("lxb")
        );

        let global_only = Config {
            remember_user: false,
            remember_session: true,
            ..config
        };
        preferences.record_success(&global_only, "private-user", "plasma");
        assert_eq!(preferences.last_user, None);
        assert_eq!(preferences.last_session.as_deref(), Some("plasma"));
        assert!(preferences.preferred_sessions.is_empty());
        assert_eq!(
            preferences.session_id(&global_only, Some("private-user")),
            Some("plasma")
        );
    }

    #[test]
    fn anonymous_success_never_persists_the_typed_account() {
        let config = Config::default();
        let mut preferences = Preferences {
            last_user: Some("alex".into()),
            last_session: Some("lxb".into()),
            preferred_sessions: BTreeMap::from([("alex".into(), "lxb".into())]),
            ..Preferences::default()
        };

        preferences.record_anonymous_success(&config, "plasma");

        assert_eq!(preferences.last_user.as_deref(), Some("alex"));
        assert_eq!(preferences.last_session.as_deref(), Some("plasma"));
        assert_eq!(
            preferences.preferred_sessions,
            BTreeMap::from([("alex".into(), "lxb".into())])
        );
        let private = Config {
            remember_user: false,
            remember_session: true,
            ..Config::default()
        };
        preferences.record_anonymous_success(&private, "gamescope");
        assert_eq!(preferences.last_user, None);
        assert_eq!(preferences.last_session.as_deref(), Some("gamescope"));
        assert!(preferences.preferred_sessions.is_empty());
    }

    #[test]
    fn resolves_xdg_state_home_then_home_fallback() {
        assert_eq!(
            preferences_path_from(Some(OsStr::new("/state")), Some(OsStr::new("/home/alex"))),
            Some(PathBuf::from(
                "/state/console-experience-desktop-manager/preferences.toml"
            ))
        );
        assert_eq!(
            preferences_path_from(Some(OsStr::new("relative")), Some(OsStr::new("/home/alex"))),
            Some(PathBuf::from(
                "/home/alex/.local/state/console-experience-desktop-manager/preferences.toml"
            ))
        );
        assert_eq!(preferences_path_from(None, None), None);
    }
}
