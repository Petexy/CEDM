//! The login screen left alone: dimmed, then dark, then asleep.
//!
//! A handheld switched on and put down spends its night on this screen if
//! nobody signs in, and a login screen that stayed lit and awake until the
//! battery died was the one screen on the machine ignoring how long it should
//! wait. So it waits as the machine is set to — LineXinBar's Settings > Power,
//! which is the *machine's* setting rather than any account's and is kept
//! where a greeter can read it, `/etc/lxb/power.toml`. The same four waits as
//! the session, with the same defaults where nothing has been chosen: the
//! screen dims after two minutes and goes dark after five, and the machine
//! sleeps a quarter of an hour into its battery and an hour into the mains.
//!
//! What counts as somebody being there is anything this screen hears — a key,
//! the pointer, a button on a controller — which is everything, because it is
//! the only program on the seat. Nothing holds sleep off: there is nothing to
//! play and nothing to download before anybody has signed in, and whatever the
//! system itself holds, logind refuses the request for, which this asks again
//! a minute later.
//!
//! The press that lights a dark screen does only that, as it does in the
//! session: somebody who pressed A to see what the machine was showing did not
//! mean to sign in.
//!
//! How the displays are dimmed and switched off is [`crate::display_power`]'s,
//! and the power button is the login manager's — LineXinBar writes its answer
//! as `HandlePowerKey` — except for the Power menu answer, which logind has no
//! word for and this screen answers by moving to its own power buttons.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// The machine's power settings, as LineXinBar writes them.
pub const SETTINGS_FILE: &str = "/etc/lxb/power.toml";

/// How long a sleep logind refused is left before it is asked again.
const RETRY_SLEEP: Duration = Duration::from_secs(60);

/// The longest wait read from the file: a week. The page offers three hours.
const LONGEST_WAIT: u32 = 7 * 24 * 3600;

/// How far the machine's sleeping clock has to have moved between two passes
/// to count as the machine having slept.
const SLEPT: Duration = Duration::from_secs(2);

/// What the power button is set to do. Only [`Button::Menu`] is this screen's
/// to answer; the rest are logind's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Sleep,
    Hibernate,
    PowerOff,
    Menu,
    Nothing,
}

impl Button {
    fn from_key(key: &str) -> Option<Self> {
        match key {
            "sleep" => Some(Self::Sleep),
            "hibernate" => Some(Self::Hibernate),
            "power-off" => Some(Self::PowerOff),
            "menu" => Some(Self::Menu),
            "nothing" => Some(Self::Nothing),
            _ => None,
        }
    }
}

/// The waits, in seconds, 0 being never, and the button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub dim_after: u32,
    pub screen_off_after: u32,
    pub sleep_on_battery: u32,
    pub sleep_plugged_in: u32,
    pub button: Button,
}

impl Settings {
    /// LineXinBar's own defaults, for a machine that has never been set.
    pub const DEFAULT: Self = Self {
        dim_after: 120,
        screen_off_after: 300,
        sleep_on_battery: 900,
        sleep_plugged_in: 3600,
        button: Button::Sleep,
    };

    /// The machine's settings, or the defaults for whatever it does not say.
    pub fn read() -> Self {
        Self::read_from(Path::new(SETTINGS_FILE))
    }

    /// Read the same way as everything else this screen reads before anybody
    /// has signed in — see [`crate::reading`] — and believed only from root,
    /// whose file it is: one an account could have put there is one that
    /// could keep every screen lit.
    pub fn read_from(path: &Path) -> Self {
        Self::read_owned_by(path, crate::reading::Owner::Uid(0))
    }

    /// Read a file this account owns — a development aid's, never the
    /// machine's.
    pub fn read_own(path: &Path) -> Self {
        Self::read_owned_by(path, crate::reading::Owner::Uid(unsafe { libc::geteuid() }))
    }

    fn read_owned_by(path: &Path, owner: crate::reading::Owner) -> Self {
        use std::io::Read;
        let Some(file) = crate::reading::open(path, owner) else {
            return Self::DEFAULT;
        };
        let mut text = String::new();
        if file.take(64 * 1024).read_to_string(&mut text).is_err() {
            return Self::DEFAULT;
        }
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        #[derive(Default, Deserialize)]
        #[serde(rename_all = "kebab-case")]
        struct Written {
            dim_screen_after: Option<u32>,
            screen_off_after: Option<u32>,
            sleep_on_battery_after: Option<u32>,
            sleep_plugged_in_after: Option<u32>,
            power_button: Option<String>,
        }
        let Ok(written) = toml::from_str::<Written>(text) else {
            tracing::warn!("the machine's power settings are not a settings file");
            return Self::DEFAULT;
        };
        let wait = |seconds: Option<u32>, default: u32| {
            seconds.filter(|s| *s <= LONGEST_WAIT).unwrap_or(default)
        };
        let default = Self::DEFAULT;
        Self {
            dim_after: wait(written.dim_screen_after, default.dim_after),
            screen_off_after: wait(written.screen_off_after, default.screen_off_after),
            sleep_on_battery: wait(written.sleep_on_battery_after, default.sleep_on_battery),
            sleep_plugged_in: wait(written.sleep_plugged_in_after, default.sleep_plugged_in),
            button: written
                .power_button
                .as_deref()
                .and_then(Button::from_key)
                .unwrap_or(default.button),
        }
    }

    /// The three waits, on the battery or on the mains. Dimming is left out
    /// where it would come at or after the screen going dark, as the session
    /// leaves it out.
    fn waits(self, on_battery: bool) -> [Option<Duration>; 3] {
        let wait = |seconds: u32| (seconds > 0).then(|| Duration::from_secs(seconds.into()));
        let dims_first = self.screen_off_after == 0 || self.dim_after < self.screen_off_after;
        [
            wait(self.dim_after).filter(|_| dims_first),
            wait(self.screen_off_after),
            wait(if on_battery {
                self.sleep_on_battery
            } else {
                self.sleep_plugged_in
            }),
        ]
    }
}

/// What the displays are to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screens {
    #[default]
    Awake,
    Dim,
    Off,
}

/// What a pass wants done.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Doing {
    /// The displays changed; apply this.
    pub screens: Option<Screens>,
    /// Ask the machine to sleep.
    pub sleep: bool,
}

/// The policy's state across passes.
pub struct Idle {
    settings: Settings,
    last_input: Instant,
    applied: Screens,
    sleep_asked: Option<Instant>,
    /// How long the machine had spent asleep as of the last pass.
    slept: Duration,
}

impl Idle {
    pub fn new(settings: Settings, now: Instant) -> Self {
        Self {
            settings,
            last_input: now,
            applied: Screens::Awake,
            sleep_asked: None,
            slept: time_asleep(),
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// What the displays are set to now.
    pub fn screens(&self) -> Screens {
        self.applied
    }

    /// Somebody did something. Returns whether that something should go no
    /// further, which is when it was what lit a dark screen.
    pub fn touched(&mut self, now: Instant) -> bool {
        self.last_input = now;
        self.sleep_asked = None;
        self.applied == Screens::Off
    }

    /// Work out what is to be done this pass.
    pub fn pass(&mut self, now: Instant, on_battery: bool) -> Doing {
        self.pass_with(now, on_battery, time_asleep())
    }

    fn pass_with(&mut self, now: Instant, on_battery: bool, slept: Duration) -> Doing {
        // The monotonic clock stands still while the machine sleeps, so a
        // machine that slept and woke would otherwise find its waits exactly
        // where they were and go straight back to sleep a minute later. Waking
        // is starting over, with the screens lit.
        if slept.saturating_sub(self.slept) >= SLEPT {
            tracing::info!(
                asleep_for = ?slept.saturating_sub(self.slept),
                "the machine has woken"
            );
            self.last_input = now;
            self.sleep_asked = None;
        }
        self.slept = slept;

        let idle = now.saturating_duration_since(self.last_input);
        let [dim, off, sleep] = self.settings.waits(on_battery);
        let reached = |wait: Option<Duration>| wait.is_some_and(|wait| idle >= wait);
        let wanted = if reached(off) {
            Screens::Off
        } else if reached(dim) {
            Screens::Dim
        } else {
            Screens::Awake
        };
        let mut doing = Doing::default();
        if wanted != self.applied {
            self.applied = wanted;
            doing.screens = Some(wanted);
        }
        let retry_due = self
            .sleep_asked
            .is_none_or(|asked| now.saturating_duration_since(asked) >= RETRY_SLEEP);
        if reached(sleep) && retry_due {
            self.sleep_asked = Some(now);
            doing.sleep = true;
        }
        doing
    }

    /// How long until something is due, so a loop that has nothing to draw
    /// knows how long it may wait.
    pub fn next_due(&self, now: Instant, on_battery: bool) -> Option<Duration> {
        let idle = now.saturating_duration_since(self.last_input);
        self.settings
            .waits(on_battery)
            .into_iter()
            .flatten()
            .filter(|wait| *wait > idle)
            .map(|wait| wait - idle)
            .min()
    }
}

/// How long the machine has spent asleep since it started: the boot clock,
/// which counts sleep, less the monotonic one, which does not.
fn time_asleep() -> Duration {
    let read = |clock| {
        let mut spec = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if unsafe { libc::clock_gettime(clock, &mut spec) } != 0 {
            return Duration::ZERO;
        }
        Duration::new(spec.tv_sec as u64, spec.tv_nsec as u32)
    };
    read(libc::CLOCK_BOOTTIME).saturating_sub(read(libc::CLOCK_MONOTONIC))
}

/// Whether the machine is running on its battery: it has a battery of its own
/// and no charger is plugged in. Read out of the kernel's own list of power
/// supplies, which a greeter can read like anybody.
pub fn on_battery() -> bool {
    on_battery_in(Path::new(crate::battery::SUPPLIES))
}

/// Which supplies are the machine's batteries is [`crate::battery`]'s to say,
/// and the same answer the mark in the corner is drawn from: a login screen
/// that waited as if it were on a battery it did not show, or drew one it was
/// not waiting for, would be about two different machines.
fn on_battery_in(directory: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return false;
    };
    let mut battery = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if crate::battery::is_system(&path) {
            battery = true;
            continue;
        }
        let read = |name: &str| crate::battery::attribute(&path, name);
        // A mouse's or a controller's charger is its own, not the machine's.
        if read("scope").as_deref() == Some("Device") {
            continue;
        }
        if matches!(read("type").as_deref(), Some("Mains" | "USB"))
            && read("online").as_deref() == Some("1")
        {
            return false;
        }
    }
    battery
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cedm-idle-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// No file is LineXinBar's defaults, and a file says only what it says.
    #[test]
    fn the_machines_file_is_read_with_the_shells_defaults() {
        let dir = scratch("read");
        assert_eq!(
            Settings::read_from(&dir.join("power.toml")),
            Settings::DEFAULT
        );
        std::fs::write(
            dir.join("power.toml"),
            "screen-off-after = 60\npower-button = \"menu\"\nbattery-saver = false\n",
        )
        .unwrap();
        if unsafe { libc::geteuid() } != 0 {
            assert_eq!(
                Settings::read_from(&dir.join("power.toml")),
                Settings::DEFAULT,
                "a file root does not own is not the machine's"
            );
        }
        let me = crate::reading::Owner::Uid(unsafe { libc::geteuid() });
        let read = Settings::read_owned_by(&dir.join("power.toml"), me);
        assert_eq!(read.screen_off_after, 60);
        assert_eq!(read.button, Button::Menu);
        assert_eq!(read.dim_after, 120);
        assert_eq!(
            Settings::parse("power-button = \"launch\"\n").button,
            Button::Sleep
        );
        assert_eq!(Settings::parse("[[["), Settings::DEFAULT);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Left alone, the screens dim, go dark, and the machine is asked to sleep
    /// — once, then again a minute later if it did not.
    #[test]
    fn left_alone_it_dims_goes_dark_and_asks_to_sleep() {
        let start = Instant::now();
        let settings = Settings {
            dim_after: 10,
            screen_off_after: 20,
            sleep_on_battery: 30,
            sleep_plugged_in: 40,
            button: Button::Sleep,
        };
        let mut idle = Idle::new(settings, start);
        let slept = Duration::ZERO;
        idle.slept = slept;
        let at = |s: u64| start + Duration::from_secs(s);
        assert_eq!(idle.pass_with(at(5), false, slept), Doing::default());
        assert_eq!(
            idle.pass_with(at(10), false, slept).screens,
            Some(Screens::Dim)
        );
        assert_eq!(
            idle.pass_with(at(20), false, slept).screens,
            Some(Screens::Off)
        );
        assert!(
            !idle.pass_with(at(35), false, slept).sleep,
            "plugged in: forty"
        );
        assert!(
            idle.pass_with(at(35), true, slept).sleep,
            "on the battery: thirty"
        );
        assert!(
            !idle.pass_with(at(40), true, slept).sleep,
            "asked a moment ago"
        );
        assert!(
            idle.pass_with(at(96), true, slept).sleep,
            "and again a minute on"
        );

        assert!(
            idle.touched(at(97)),
            "the press that lights a dark screen goes no further"
        );
        assert_eq!(
            idle.pass_with(at(97), true, slept).screens,
            Some(Screens::Awake)
        );
        assert!(!idle.touched(at(98)), "a lit screen's press is a press");
    }

    /// Waking is starting over: the waits count from the moment the machine
    /// woke, not from before it slept.
    #[test]
    fn a_machine_that_woke_starts_over() {
        let start = Instant::now();
        let mut idle = Idle::new(Settings::DEFAULT, start);
        idle.slept = Duration::ZERO;
        let later = start + Duration::from_secs(3600);
        assert!(idle.pass_with(later, false, Duration::ZERO).sleep);
        let woke = idle.pass_with(
            later + Duration::from_secs(1),
            false,
            Duration::from_secs(600),
        );
        assert_eq!(woke.screens, Some(Screens::Awake));
        assert!(!woke.sleep);
    }

    /// A dim at or after the dark is no dim at all; Never is no wait.
    #[test]
    fn a_dim_after_the_dark_is_skipped() {
        let mut settings = Settings::DEFAULT;
        settings.dim_after = 300;
        assert_eq!(settings.waits(false)[0], None);
        settings.screen_off_after = 0;
        assert_eq!(settings.waits(false)[0], Some(Duration::from_secs(300)));
        assert_eq!(settings.waits(false)[1], None);
    }

    /// A machine runs on its battery only with no charger in, and a mouse's
    /// battery is not the machine's.
    #[test]
    fn the_battery_is_the_machines_own() {
        let dir = scratch("supply");
        let supply = |name: &str, fields: &[(&str, &str)]| {
            let path = dir.join(name);
            std::fs::create_dir_all(&path).unwrap();
            for (field, value) in fields {
                std::fs::write(path.join(field), format!("{value}\n")).unwrap();
            }
        };
        assert!(!on_battery_in(&dir), "a desktop");
        supply(
            "hidpp_battery_0",
            &[("type", "Battery"), ("scope", "Device")],
        );
        assert!(!on_battery_in(&dir), "a mouse is not the machine");
        supply("BAT0", &[("type", "Battery"), ("present", "1")]);
        assert!(on_battery_in(&dir), "a laptop unplugged");
        supply("AC", &[("type", "Mains"), ("online", "1")]);
        assert!(!on_battery_in(&dir), "and plugged in");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
