//! Versioned greeter-to-LineXinBar wallpaper handoff.
//!
//! LineXinBar's wallpaper is analytic: its complete animation state is the
//! palette and a number of seconds. A sample from Linux's monotonic clock lets
//! an unrelated session process advance from the exact phase the greeter last
//! drew without trusting the adjustable wall clock.

use crate::accent;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

pub const ENV: &str = "LXB_BACKGROUND_HANDOFF";
pub const VERSION: &str = "1";
pub const VISUAL: &str = "lxb-wallpaper-v1";
const MAX_ENCODED_BYTES: usize = 1024;
const MAX_HANDOFF_AGE_NS: u64 = 30_000_000_000;
const REQUIRED_FIELDS: u8 = 0b0111_1111;
/// What an anchor file may be, in bytes. It is one short line.
const MAX_ANCHOR_BYTES: u64 = 4096;

/// The greeter wallpaper clock, anchored directly to Linux monotonic time.
///
/// Keeping this separate from the application's `Instant` means the shader
/// and the handoff record use the same clock sample. The fallback exists only
/// so the greeter can keep drawing on an unusual platform where
/// `CLOCK_MONOTONIC` is unavailable; such a fallback clock is never exported.
#[derive(Debug)]
pub struct SceneClock {
    monotonic_origin_ns: Option<u64>,
    fallback_started: Instant,
}

impl SceneClock {
    pub fn start() -> Self {
        Self {
            monotonic_origin_ns: monotonic_ns(),
            fallback_started: Instant::now(),
        }
    }

    /// The same clock, continuing whatever wallpaper is already on screen.
    ///
    /// The greeter is not always the first thing to draw one. Where its own
    /// compositor is LineXinBar's, that compositor is up first and paints the
    /// wallpaper itself for the few hundred milliseconds it takes this program
    /// to open a window — from a record this program wrote for it, so that the
    /// screen the user is handed is not a black one. Starting the animation
    /// again from zero here would put a jump in the middle of that, at the one
    /// moment the interface arrives over it.
    ///
    /// So the origin is moved back by however far the record says the scene
    /// had already run. It is the same record, the same encoder and the same
    /// checks as the one this hands *on* at the other end of a login — a
    /// wallpaper clock is a wallpaper clock, whichever direction it is
    /// crossing.
    ///
    /// Anything unusable is no record at all and the clock starts at zero: a
    /// greeter that could not read one still comes up, and comes up drawing.
    pub fn resume(record: Option<&std::ffi::OsStr>) -> Self {
        let clock = Self::start();
        let Some(scene) = record
            .and_then(|record| record.to_str())
            .and_then(BackgroundHandoff::parse)
            .zip(boot_id())
            .and_then(|(handoff, boot)| handoff.scene_time(&boot, monotonic_ns()?))
        else {
            return clock;
        };
        let scene_ns = u64::try_from(scene.as_nanos()).unwrap_or(u64::MAX);
        Self {
            // Only where there is a raw clock to move: the fallback clock is
            // never exported and has nothing to continue from either.
            monotonic_origin_ns: clock
                .monotonic_origin_ns
                .map(|origin| origin.saturating_sub(scene_ns)),
            ..clock
        }
    }

    /// The clock this machine's wallpaper has been running on since its first
    /// login screen of this boot, anchored in `path`.
    ///
    /// A login screen is not the beginning of the animation, except the first
    /// time. Every one after it follows a session that was drawing the same
    /// wallpaper from this same clock a moment earlier — and the session got
    /// that clock from the login screen before it, which is what makes them
    /// one continuous picture rather than four. Starting again from zero here
    /// puts the whole of a session's worth of animation into the instant the
    /// user signs out: not a jump, a different wallpaper.
    ///
    /// Nothing needs to be sent back from the session for this. Scene time is
    /// the age of one instant, so the anchor is that instant, and the greeter
    /// account's own state directory is the right place to keep it: the same
    /// account writes it, reads it, and is the one thing present at both ends
    /// of every login. What it records is a monotonic timestamp, so it means
    /// nothing after a reboot and says which boot it belongs to.
    ///
    /// An anchor that cannot be read, parsed, or trusted is no anchor, and the
    /// clock starts where it always did: a login screen that has forgotten
    /// where the wallpaper was still comes up drawing one.
    pub fn of_this_boot(path: &Path) -> Self {
        let clock = match SceneAnchor::read(path).and_then(|anchor| anchor.resume()) {
            Some(clock) => return clock,
            None => Self::start(),
        };
        if let Some(anchor) = SceneAnchor::of(&clock) {
            // Never fatal. A login screen whose wallpaper restarts at the next
            // sign-out is worth immeasurably more than no login screen.
            if let Err(err) = anchor.write(path) {
                tracing::warn!(path = %path.display(), %err, "could not write down where the wallpaper's clock starts");
            }
        }
        clock
    }

    pub fn elapsed(&self) -> Duration {
        self.monotonic_origin_ns
            .and_then(|origin| monotonic_ns()?.checked_sub(origin))
            .map(Duration::from_nanos)
            .unwrap_or_else(|| self.fallback_started.elapsed())
    }

    /// Capture the scene and timestamp from one raw-clock sample.
    pub fn capture(&self, accent: &str) -> Option<BackgroundHandoff> {
        let origin_ns = self.monotonic_origin_ns?;
        let sample_ns = monotonic_ns()?;
        let scene_ns = sample_ns.checked_sub(origin_ns)?;
        BackgroundHandoff::from_sample(boot_id()?, sample_ns, scene_ns, accent)
    }
}

/// Where the wallpaper's clock reads zero on this machine, this boot.
///
/// Deliberately not a [`BackgroundHandoff`]. That record is a handover in
/// flight: it carries an accent because the far end has to draw in one, and it
/// goes stale in half a minute because a handover that took longer than that
/// did not happen. This is the other kind of thing — an anchor, good for the
/// whole of a boot and meaningless past it, with no accent in it because the
/// accent belongs to whoever signed in last and is worked out afresh at every
/// login screen.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SceneAnchor {
    boot_id: String,
    origin_ns: u64,
}

impl SceneAnchor {
    /// The anchor a running clock sits on, where there is a raw clock under it
    /// to name one. The fallback clock is never written down for the same
    /// reason it is never exported: nothing else can read it.
    fn of(clock: &SceneClock) -> Option<Self> {
        Some(Self {
            boot_id: boot_id()?,
            origin_ns: clock.monotonic_origin_ns?,
        })
    }

    /// The clock this anchor describes, if it belongs to this boot and does
    /// not start in the future — which would run the wallpaper backwards.
    fn resume(&self) -> Option<SceneClock> {
        if self.boot_id != boot_id()? || self.origin_ns > monotonic_ns()? {
            return None;
        }
        Some(SceneClock {
            monotonic_origin_ns: Some(self.origin_ns),
            fallback_started: Instant::now(),
        })
    }

    fn encode(&self) -> String {
        format!(
            "v={VERSION};clock=linux-monotonic;boot={};origin-ns={}\n",
            self.boot_id, self.origin_ns
        )
    }

    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() || value.len() > MAX_ENCODED_BYTES || !value.is_ascii() {
            return None;
        }
        let mut version = None;
        let mut clock = None;
        let mut boot_id = None;
        let mut origin_ns = None;
        for part in value.split(';') {
            let (key, value) = part.split_once('=')?;
            match key {
                "v" => version = Some(value),
                "clock" => clock = Some(value),
                "boot" => boot_id = valid_boot_id(value).then(|| value.to_string()),
                "origin-ns" => origin_ns = parse_decimal(value),
                _ => return None,
            }
        }
        (version == Some(VERSION) && clock == Some("linux-monotonic")).then_some(Self {
            boot_id: boot_id?,
            origin_ns: origin_ns?,
        })
    }

    fn read(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        if !metadata.is_file() || metadata.len() > MAX_ANCHOR_BYTES {
            return None;
        }
        Self::parse(&fs::read_to_string(path).ok()?)
    }

    /// Written beside the file and renamed over it, so a greeter that is
    /// killed mid-write leaves the anchor it found rather than half of one.
    fn write(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let scratch = path.with_extension("new");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&scratch)?;
        let written = file
            .write_all(self.encode().as_bytes())
            .and_then(|()| file.sync_all());
        drop(file);
        if let Err(err) = written {
            let _ = fs::remove_file(&scratch);
            return Err(err);
        }
        fs::rename(&scratch, path).inspect_err(|_| {
            let _ = fs::remove_file(&scratch);
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundHandoff {
    pub boot_id: String,
    pub sample_ns: u64,
    pub scene_ns: u64,
    pub accent: String,
}

impl BackgroundHandoff {
    fn from_sample(boot_id: String, sample_ns: u64, scene_ns: u64, accent: &str) -> Option<Self> {
        Some(Self {
            boot_id,
            sample_ns,
            scene_ns,
            accent: accent::canonical(accent)?.to_string(),
        })
    }

    pub fn encode(&self) -> String {
        format!(
            "v={VERSION};visual={VISUAL};clock=linux-monotonic;boot={};sample-ns={};scene-ns={};accent={}",
            self.boot_id, self.sample_ns, self.scene_ns, self.accent
        )
    }

    pub fn environment(&self) -> String {
        format!("{ENV}={}", self.encode())
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value.is_empty() || value.len() > MAX_ENCODED_BYTES || !value.is_ascii() {
            return None;
        }
        let mut version = None;
        let mut visual = None;
        let mut clock = None;
        let mut boot_id = None;
        let mut sample_ns = None;
        let mut scene_ns = None;
        let mut accent = None;
        let mut seen = 0_u8;
        for part in value.split(';') {
            let (key, value) = part.split_once('=')?;
            if key.is_empty() || value.is_empty() {
                return None;
            }
            let field = match key {
                "v" => {
                    version = Some(value);
                    1 << 0
                }
                "visual" => {
                    visual = Some(value);
                    1 << 1
                }
                "clock" => {
                    clock = Some(value);
                    1 << 2
                }
                "boot" => {
                    boot_id = valid_boot_id(value).then(|| value.to_string());
                    1 << 3
                }
                "sample-ns" => {
                    sample_ns = parse_decimal(value);
                    1 << 4
                }
                "scene-ns" => {
                    scene_ns = parse_decimal(value);
                    1 << 5
                }
                "accent" => {
                    accent = accent::canonical(value).map(str::to_string);
                    1 << 6
                }
                _ => return None,
            };
            if seen & field != 0 {
                return None;
            }
            seen |= field;
        }
        (seen == REQUIRED_FIELDS
            && version == Some(VERSION)
            && visual == Some(VISUAL)
            && clock == Some("linux-monotonic"))
        .then_some(Self {
            boot_id: boot_id?,
            sample_ns: sample_ns?,
            scene_ns: scene_ns?,
            accent: accent?,
        })
    }

    /// Scene time at `now`, only when the sample came from this boot.
    pub fn scene_time(&self, current_boot_id: &str, now_ns: u64) -> Option<Duration> {
        if self.boot_id != current_boot_id || now_ns < self.sample_ns {
            return None;
        }
        let age_ns = now_ns - self.sample_ns;
        if age_ns > MAX_HANDOFF_AGE_NS {
            return None;
        }
        self.scene_ns.checked_add(age_ns).map(Duration::from_nanos)
    }
}

pub fn boot_id() -> Option<String> {
    let id = fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let id = id.trim();
    valid_boot_id(id).then(|| id.to_ascii_lowercase())
}

pub fn monotonic_ns() -> Option<u64> {
    let mut value = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `value` is a valid writable timespec and CLOCK_MONOTONIC accepts it.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut value) };
    if result != 0 || value.tv_sec < 0 || !(0..1_000_000_000).contains(&value.tv_nsec) {
        return None;
    }
    (value.tv_sec as u64)
        .checked_mul(1_000_000_000)?
        .checked_add(value.tv_nsec as u64)
}

fn valid_boot_id(value: &str) -> bool {
    value.len() == 36
        && value.chars().enumerate().all(|(index, character)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                character == '-'
            } else {
                character.is_ascii_digit() || ('a'..='f').contains(&character)
            }
        })
}

fn parse_decimal(value: &str) -> Option<u64> {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| value.parse().ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Keep this canonical producer fixture byte-for-byte identical to the
    // consumer fixture in LineXinBar's `wallpaper_clock` tests.
    const BOOT: &str = "01234567-89ab-cdef-0123-456789abcdef";

    #[test]
    fn round_trip_is_strict_and_advances_on_the_same_boot() {
        let state = BackgroundHandoff {
            boot_id: BOOT.to_string(),
            sample_ns: 10_000_000_000,
            scene_ns: 42_000_000_000,
            accent: "Blue".to_string(),
        };
        let parsed = BackgroundHandoff::parse(&state.encode()).unwrap();
        assert_eq!(parsed, state);
        assert_eq!(
            state.encode(),
            "v=1;visual=lxb-wallpaper-v1;clock=linux-monotonic;boot=01234567-89ab-cdef-0123-456789abcdef;sample-ns=10000000000;scene-ns=42000000000;accent=Blue"
        );
        assert_eq!(
            parsed.scene_time(BOOT, 10_250_000_000),
            Some(Duration::from_millis(42_250))
        );
    }

    #[test]
    fn rejects_stale_boots_unknown_visuals_and_unknown_accents() {
        let valid = format!(
            "v=1;visual={VISUAL};clock=linux-monotonic;boot={BOOT};sample-ns=2;scene-ns=4;accent=Green"
        );
        assert!(BackgroundHandoff::parse(&valid).is_some());
        assert!(BackgroundHandoff::parse(&valid.replace(VISUAL, "other")).is_none());
        assert!(BackgroundHandoff::parse(&valid.replace("Green", "Orange")).is_none());
        assert!(BackgroundHandoff::parse(&valid)
            .unwrap()
            .scene_time("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", 5)
            .is_none());
    }

    #[test]
    fn rejects_duplicate_or_missing_fields_and_overlong_values() {
        let valid = format!(
            "v=1;visual={VISUAL};clock=linux-monotonic;boot={BOOT};sample-ns=2;scene-ns=4;accent=Green"
        );
        assert!(BackgroundHandoff::parse(&format!("{valid};accent=Blue")).is_none());
        assert!(BackgroundHandoff::parse(&valid.replace(";scene-ns=4", "")).is_none());
        assert!(BackgroundHandoff::parse(&"x".repeat(MAX_ENCODED_BYTES + 1)).is_none());
        assert!(BackgroundHandoff::parse(&valid.replace("sample-ns=2", "sample-ns=+2")).is_none());
        assert!(
            BackgroundHandoff::parse(&valid.replace(BOOT, &BOOT.to_ascii_uppercase())).is_none()
        );
        assert!(BackgroundHandoff::parse("accent=Bl\u{00fa}e").is_none());
    }

    #[test]
    fn rejects_scene_clock_overflow_instead_of_freezing_at_u64_max() {
        let state = BackgroundHandoff {
            boot_id: BOOT.to_string(),
            sample_ns: 2,
            scene_ns: u64::MAX,
            accent: "Blue".to_string(),
        };
        assert_eq!(state.scene_time(BOOT, 3), None);
    }

    #[test]
    fn rejects_a_record_that_waited_too_long_to_be_consumed() {
        let state = BackgroundHandoff {
            boot_id: BOOT.to_string(),
            sample_ns: 2,
            scene_ns: 4,
            accent: "Blue".to_string(),
        };
        assert_eq!(
            state.scene_time(BOOT, state.sample_ns + MAX_HANDOFF_AGE_NS + 1),
            None
        );
    }

    /// A record this greeter wrote for its own compositor, read back by the
    /// greeter that compositor then started. Both ends are here, which is the
    /// point: it is the same encoder and the same checks in both directions.
    #[test]
    fn a_resumed_clock_carries_on_from_the_wallpaper_already_on_screen() {
        let compositor = SceneClock::start();
        let record = compositor
            .capture("Blue")
            .expect("a machine with a monotonic clock and a boot id")
            .encode();

        let greeter = SceneClock::resume(Some(std::ffi::OsStr::new(&record)));
        let carried = greeter.elapsed();
        // At least what the compositor's clock had reached, never less: a
        // resumed clock that ran backwards would be a wallpaper that jumped
        // the wrong way at the one moment this exists to smooth over.
        assert!(
            carried
                >= compositor
                    .elapsed()
                    .saturating_sub(Duration::from_millis(1))
        );
        assert!(carried < Duration::from_secs(1));

        // And nothing usable is a clock that starts where it always did.
        for unusable in ["", "nonsense", &record.replace("Blue", "Orange")] {
            let fresh = SceneClock::resume(Some(std::ffi::OsStr::new(unusable)));
            assert!(fresh.elapsed() < Duration::from_secs(1), "{unusable:?}");
        }
        assert!(SceneClock::resume(None).elapsed() < Duration::from_secs(1));

        // A record from another boot is another machine's clock. It is
        // refused here for the same reason it is refused at a login.
        let elsewhere = BackgroundHandoff {
            boot_id: BOOT.to_string(),
            sample_ns: 1,
            scene_ns: 9_000_000_000,
            accent: "Blue".to_string(),
        };
        let clock = SceneClock::resume(Some(std::ffi::OsStr::new(&elsewhere.encode())));
        assert!(clock.elapsed() < Duration::from_secs(1));
    }

    fn anchor_path(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "cedm-wallpaper-clock-{}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root.join("wallpaper-clock")
    }

    /// The logout end of the same continuity the record gives the login end.
    /// A greeter started after a session must come up on the wallpaper that
    /// session was showing, and this is a different process every time: the
    /// only thing carrying the clock across is the anchor on disk.
    #[test]
    fn a_login_screen_carries_on_from_the_one_before_it() {
        let path = anchor_path("carries-on");
        let _ = fs::remove_file(&path);

        let first = SceneClock::of_this_boot(&path);
        assert!(path.exists(), "the first login screen writes the anchor");
        std::thread::sleep(Duration::from_millis(30));

        // A whole session later, in a process that knows nothing about the
        // first one.
        let second = SceneClock::of_this_boot(&path);
        let carried = second.elapsed();
        assert!(
            carried >= first.elapsed().saturating_sub(Duration::from_millis(1)),
            "the wallpaper went backwards: {carried:?}"
        );
        assert!(
            carried >= Duration::from_millis(30),
            "the wallpaper started over: {carried:?}"
        );

        // And the anchor it read is the one it wrote, unchanged: the origin is
        // a fixed instant, not something that drifts a little at every login.
        let anchor = SceneAnchor::read(&path).expect("an anchor");
        assert_eq!(anchor, SceneAnchor::of(&first).expect("an anchor"));
    }

    #[test]
    fn an_anchor_that_cannot_be_trusted_starts_the_wallpaper_over() {
        let path = anchor_path("untrusted");
        let now = monotonic_ns().expect("monotonic clock");
        let boot = boot_id().expect("a boot id");

        for unusable in [
            String::new(),
            "nonsense".to_string(),
            // Another boot: another machine's clock, as far as this one knows.
            format!("v=1;clock=linux-monotonic;boot={BOOT};origin-ns=1"),
            // A clock that has not started yet would run the wallpaper
            // backwards for as long as it took to catch up.
            format!(
                "v=1;clock=linux-monotonic;boot={boot};origin-ns={}",
                now + 60_000_000_000
            ),
            format!("v=2;clock=linux-monotonic;boot={boot};origin-ns=1"),
            format!("v=1;clock=wall;boot={boot};origin-ns=1"),
            format!("v=1;clock=linux-monotonic;boot={boot};origin-ns=-1"),
            format!("v=1;clock=linux-monotonic;boot={boot}"),
        ] {
            fs::write(&path, &unusable).unwrap();
            let clock = SceneClock::of_this_boot(&path);
            assert!(
                clock.elapsed() < Duration::from_secs(1),
                "carried on from {unusable:?}"
            );
            // And what it could not use has been replaced by something it can.
            assert!(
                SceneAnchor::read(&path)
                    .and_then(|anchor| anchor.resume())
                    .is_some(),
                "no usable anchor left behind after {unusable:?}"
            );
        }
    }

    /// A greeter with no state directory it can write is still a greeter.
    #[test]
    fn an_unwritable_anchor_is_not_fatal() {
        let path = anchor_path("unwritable")
            .join("not-a-directory")
            .join("clock");
        let clock = SceneClock::of_this_boot(&path);
        assert!(clock.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn an_anchor_round_trips_through_its_file() {
        let path = anchor_path("round-trip");
        let anchor = SceneAnchor {
            boot_id: BOOT.to_string(),
            origin_ns: 1_234_567_890,
        };
        anchor.write(&path).expect("writes");
        assert_eq!(SceneAnchor::read(&path).expect("reads"), anchor);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "v=1;clock=linux-monotonic;boot=01234567-89ab-cdef-0123-456789abcdef;origin-ns=1234567890\n"
        );
        // Nothing is left beside it: a scratch file that survived would be
        // read as an anchor by nothing, but it would still be litter in a
        // directory the greeter account owns.
        assert!(!path.with_extension("new").exists());
    }

    #[test]
    fn scene_clock_capture_has_one_exact_raw_clock_origin() {
        let clock = SceneClock::start();
        let origin_ns = clock.monotonic_origin_ns.expect("monotonic clock");
        let handoff = clock.capture("Blue").expect("captured handoff");
        assert_eq!(
            handoff.sample_ns.checked_sub(handoff.scene_ns),
            Some(origin_ns)
        );
    }
}
