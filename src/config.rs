//! Read-only administrator policy for the greeter.
//!
//! This file contains preferences, not session commands. A configured desktop
//! ID must still be resolved against the sessions discovered for the current
//! machine before it can be offered or launched.

use anyhow::{bail, Context};
use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub const PATH: &str = "/etc/cedm/config.toml";
pub(crate) const MAX_TOML_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Desktop-file ID (the filename without `.desktop`), never a command.
    pub default_session: Option<String>,
    pub remember_user: bool,
    pub remember_session: bool,
    /// Which language the login screen is written in, as a locale name or a
    /// language tag: `pl`, `pl_PL.UTF-8` and `pt-BR` are all understood.
    ///
    /// Unset — which is the normal case — means the machine's own, worked out
    /// from the environment and then from whichever file this distribution
    /// keeps its locale in. See [`crate::i18n`]. This is here for the machine
    /// whose login screen should not be in the machine's language: a shared
    /// terminal in a building where the desks are set up in one language and
    /// the people signing in read another.
    ///
    /// A language this greeter is not written in is ignored rather than
    /// fatal. The alternative is a machine that will not present a login
    /// screen at all because of a typo in an optional preference.
    pub language: Option<String>,
    /// Whether the login screen answers a button with a sound — see
    /// [`crate::sound`].
    ///
    /// A switch rather than a level, because a greeter has nowhere to keep a
    /// level and nobody to set one: it runs as an account of its own, before
    /// anybody has signed in, and the person in front of it has no settings
    /// here. How loud it is, is how loud the machine is.
    pub sound: bool,
    pub power: Power,
}

/// Which of the three power actions the greeter offers.
///
/// Only whether they are *shown*. Whether they are carried out is polkit's
/// answer and is not configurable from here; see [`crate::power`]. A kiosk or
/// a shared machine turns these off so the buttons do not exist to be pressed;
/// a machine that wants them but not for everybody leaves them on and says so
/// in polkit instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Power {
    pub sleep: bool,
    pub restart: bool,
    pub shut_down: bool,
}

impl Default for Power {
    fn default() -> Self {
        Self {
            sleep: true,
            restart: true,
            shut_down: true,
        }
    }
}

impl Power {
    /// A policy that offers nothing, for the fail-closed path.
    const fn none() -> Self {
        Self {
            sleep: false,
            restart: false,
            shut_down: false,
        }
    }

    pub fn allows(&self, action: crate::power::Action) -> bool {
        match action {
            crate::power::Action::Sleep => self.sleep,
            crate::power::Action::Restart => self.restart,
            crate::power::Action::ShutDown => self.shut_down,
        }
    }

    pub fn any(&self) -> bool {
        self.sleep || self.restart || self.shut_down
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_session: None,
            remember_user: true,
            remember_session: true,
            language: None,
            sound: true,
            power: Power::default(),
        }
    }
}

impl Config {
    /// What an unreadable policy file means.
    ///
    /// Every option here is one an administrator may have set *restrictively*,
    /// so a file that cannot be understood must not be read as its absence:
    /// history stops being kept, and the machine stops being offered a way to
    /// turn itself off from the login screen.
    fn fail_closed() -> Self {
        Self {
            default_session: None,
            remember_user: false,
            remember_session: false,
            // Not a restriction, so it is not withdrawn: an unreadable policy
            // file means the machine's own language, which is what a greeter
            // with no policy at all shows.
            language: None,
            // Quiet, on the same grounds as the rest of this: a machine whose
            // administrator turned the noise off is one where a typo in this
            // file must not turn it back on. A silent login screen is a working
            // one; a shared office that starts clicking overnight is a fault
            // somebody has to come and find.
            sound: false,
            power: Power::none(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = Path::new(PATH);
        if !path.exists() {
            return Self::default();
        }
        match Self::try_load_path(path) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    %error,
                    "invalid desktop-manager configuration; history is disabled"
                );
                Self::fail_closed()
            }
        }
    }

    /// Load a specific present file, disabling history on any error. Missing
    /// configuration is handled by [`Self::load`] and uses normal defaults.
    pub fn load_path(path: &Path) -> Self {
        Self::try_load_path(path).unwrap_or_else(|_| Self::fail_closed())
    }

    /// Load a specific file while preserving the diagnostic for a caller that
    /// wants to report it. Reads are bounded before TOML is parsed.
    pub fn try_load_path(path: &Path) -> anyhow::Result<Self> {
        let raw = read_bounded(path)?;
        toml::from_str(&raw)
            .with_context(|| format!("could not parse administrator config {}", path.display()))
    }

    /// Return only a configured desktop-file ID. Discovery and command
    /// validation deliberately remain the caller's responsibility.
    pub fn default_session_id(&self) -> Option<&str> {
        self.default_session
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
    }
}

pub(crate) fn read_bounded(path: &Path) -> anyhow::Result<String> {
    let file =
        File::open(path).with_context(|| format!("could not open TOML file {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("could not inspect TOML file {}", path.display()))?;
    if metadata.len() > MAX_TOML_BYTES as u64 {
        bail!(
            "TOML file {} is larger than {} bytes",
            path.display(),
            MAX_TOML_BYTES
        );
    }

    // The metadata check is not enough on its own because a file can grow
    // between stat and read (and pseudo-files often report a length of zero).
    let mut bytes = Vec::with_capacity((metadata.len() as usize).min(MAX_TOML_BYTES));
    file.take(MAX_TOML_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("could not read TOML file {}", path.display()))?;
    if bytes.len() > MAX_TOML_BYTES {
        bail!(
            "TOML file {} is larger than {} bytes",
            path.display(),
            MAX_TOML_BYTES
        );
    }
    String::from_utf8(bytes).with_context(|| format!("TOML file {} is not UTF-8", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    fn test_file(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "cedm-config-{}-{}-{name}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root.join("config.toml")
    }

    fn remove_test_file(path: &Path) {
        fs::remove_file(path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn defaults_remember_choices_without_inventing_a_session() {
        let config = Config::default();
        assert!(config.remember_user);
        assert!(config.remember_session);
        assert_eq!(config.default_session_id(), None);
    }

    #[test]
    fn loads_all_administrator_options() {
        let path = test_file("options");
        fs::write(
            &path,
            "default_session = \"lxb\"\nremember_user = false\nremember_session = false\n",
        )
        .unwrap();
        let config = Config::try_load_path(&path).unwrap();
        assert_eq!(config.default_session_id(), Some("lxb"));
        assert!(!config.remember_user);
        assert!(!config.remember_session);
        remove_test_file(&path);
    }

    #[test]
    fn corrupt_or_oversized_files_fail_closed() {
        let corrupt = test_file("corrupt");
        fs::write(&corrupt, "remember_user = [not valid").unwrap();
        assert_eq!(Config::load_path(&corrupt), Config::fail_closed());
        remove_test_file(&corrupt);

        let oversized = test_file("oversized");
        fs::write(&oversized, vec![b'x'; MAX_TOML_BYTES + 1]).unwrap();
        assert!(Config::try_load_path(&oversized).is_err());
        assert_eq!(Config::load_path(&oversized), Config::fail_closed());
        remove_test_file(&oversized);
    }

    #[test]
    fn misspelled_privacy_keys_are_rejected_and_fail_closed() {
        let path = test_file("unknown-option");
        fs::write(&path, "remember_users = false\n").unwrap();
        assert!(Config::try_load_path(&path).is_err());
        let config = Config::load_path(&path);
        assert!(!config.remember_user);
        assert!(!config.remember_session);
        remove_test_file(&path);
    }

    #[test]
    fn power_actions_are_offered_by_default_and_can_be_withdrawn_one_at_a_time() {
        assert!(Config::default().power.any());
        let path = test_file("power");
        fs::write(&path, "[power]\nshut_down = false\n").unwrap();
        let config = Config::try_load_path(&path).unwrap();
        assert!(config.power.allows(crate::power::Action::Sleep));
        assert!(config.power.allows(crate::power::Action::Restart));
        assert!(!config.power.allows(crate::power::Action::ShutDown));
        remove_test_file(&path);
    }

    /// A policy file the greeter cannot understand may be one that took these
    /// away. Offering them anyway would be reading a mistake as permission.
    #[test]
    fn an_unreadable_policy_offers_no_power_action_at_all() {
        let path = test_file("corrupt-power");
        fs::write(&path, "[power]\nsleep = yes please").unwrap();
        let config = Config::load_path(&path);
        assert!(!config.power.any());
        for action in crate::power::ALL {
            assert!(!config.power.allows(action));
        }
        remove_test_file(&path);
    }

    #[test]
    fn blank_default_session_is_not_a_desktop_id() {
        let config = Config {
            default_session: Some("   ".to_string()),
            ..Config::default()
        };
        assert_eq!(config.default_session_id(), None);
    }
}
