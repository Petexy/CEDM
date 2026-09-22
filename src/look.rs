//! The look an account publishes for its login screen: the palette its shell
//! is set to, and the displays it is set up on.
//!
//! # Why anything is published at all
//!
//! A greeter cannot read a home directory. Homes are commonly `0700` and are
//! `0710` on the desk this was written at, and the account the greeter runs as
//! is in nobody's group — so the shell's own settings, which is where all of
//! this lives, are out of reach. That is not a misconfiguration to work
//! around. It is the boundary this project draws everywhere else, and
//! [`crate::faces`] says the same thing about avatars: what a login screen may
//! know about an account is what was published for it, not what is inside it.
//!
//! Nothing published it, so the login screen knew nothing: every account was
//! drawn in the default purple whatever its shell was set to, and every
//! display was brought up in whatever mode its compositor picked first.
//!
//! So each account publishes for itself, as its session starts, from
//! `cedm-session`. What it writes is a copy of the parts of its LineXinBar
//! configuration a login screen has any use for, in the shell's own spelling,
//! and the greeter believes a file only for the account that owns it.
//!
//! # What it is for
//!
//! Two things, one of which the greeter does itself and one of which it cannot:
//!
//! - the accent, which is the colour the login screen is drawn in, and
//! - the displays, which is [`Look::compositor_config`]: the greeter is a
//!   Wayland client and cannot set a mode, place an output or turn HDR on, so
//!   what it does instead is hand its own compositor the same settings the
//!   session's compositor would have come up in.
//!
//! # How fresh it is
//!
//! As fresh as the settings themselves, because publishing happens where they
//! are changed. Writing the copy once at sign-in would leave it wrong for the
//! rest of the session — change the accent to red at lunchtime, sign out in the
//! evening, and the login screen that comes up is still the purple it was that
//! morning, with nothing on that screen to explain why.
//!
//! So the shell publishes as it saves: LineXinBar runs `--publish-look` at the
//! end of the same function that writes `shell.toml`, and this reads the file
//! that has just been written. There is no watcher and nothing resident in
//! anybody's session — a login screen has no business keeping a process inside
//! one — and no window between a setting changing and the copy following it,
//! which is the window somebody who changes a setting and signs straight out
//! would fall into.
//!
//! A machine whose shell is older, or is not LineXinBar at all, still publishes
//! once as the session starts, from `cedm-session`. That is the floor: never
//! staler than the session's own beginning.

use crate::accent;
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// Where accounts publish, and where the greeter reads.
///
/// Beside the broker's own state rather than inside it. `state.toml` is
/// root-owned and unforgeable by design; this is the opposite — a directory
/// accounts write their own file into — which is why nothing here is believed
/// without checking who wrote it. See [`published_in`] and the tmpfiles
/// fragment that creates the directory `1733`.
pub const PUBLISHED: &str = "/var/lib/console-experience-desktop-manager/published";

/// The most of one of these that will ever be read.
///
/// The same bound the greeter puts on every other file it reads before anyone
/// has logged in. A published file is a few hundred bytes; this is not about
/// them, it is that nothing on this side of a login is read unbounded.
const MAX_BYTES: u64 = 256 * 1024;

/// How many displays a published look may describe.
///
/// Far more screens than a machine has, and small enough that the file stays
/// something a greeter reads without thinking about it. Deliberately not
/// [`crate::displays::MAX`], which is how many login screens are composed
/// separately — a machine may perfectly well have settings kept for displays
/// that are not plugged in today.
const MAX_DISPLAYS: usize = 32;

/// The eight ways up a picture can be drawn, spelled as the compositor spells
/// them.
const TRANSFORMS: [&str; 8] = [
    "normal",
    "90",
    "180",
    "270",
    "flipped",
    "flipped-90",
    "flipped-180",
    "flipped-270",
];

/// How outputs with no position of their own are arranged.
const LAYOUTS: [&str; 3] = ["horizontal", "vertical", "mirror"];

/// The coldest and warmest the night light goes, as the shell's own settings
/// page bounds it.
pub(crate) const NEUTRAL_KELVIN: u16 = 6500;
const WARMEST_KELVIN: u16 = 2000;

/// Everything a login screen may know about how an account's desktop is set
/// up.
///
/// Deliberately the shell's own spelling: the top-level keys and the
/// `[display.NAME]` sections are exactly `shell.toml`'s, so the same reader
/// understands a published copy and the real thing, and a published file can
/// be compared with the settings it came from by eye.
#[derive(Debug, Default, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Look {
    /// The palette the shell is set to, canonical or not — [`Look::accent`]
    /// is what answers that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    /// How much material each half of that shell draws itself with — `Default`
    /// or `Simple`, canonical or not, with [`Look::theme`] answering that. One
    /// answer for the picture behind everything and one for every mark on top of
    /// it, because they are two settings.
    ///
    /// Carried for the same reason the accent is, and it matters more: an
    /// account whose machine cannot afford the water has said so, and a login
    /// screen that arrived in front of it drawing three lit sheets of it would be
    /// the one screen on that machine ignoring the setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_wallpaper: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_icons: Option<String>,
    /// What the two above were written under before they were two settings.
    ///
    /// Read where a half has nothing of its own, and published again where it is
    /// all a look was read with, so a copy of an old file stays an old file
    /// rather than becoming a silent statement about a setting it never made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Which of the two clocks the account writes a time on — `24-hour` or
    /// `12-hour`, canonical or not, with [`Look::clock`] answering that.
    ///
    /// Carried for the accent's reason: the hour on the right of this screen
    /// is the shell's own clock in the shell's own material, and a greeter
    /// that showed `20:38` in front of a console set to the twelve-hour clock
    /// would be the one screen on the machine ignoring the setting. A look
    /// that says nothing — every one written before the shell had the row —
    /// leaves the account's language to answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
    /// Whether that account's shell writes what its buttons do wherever it has
    /// room to — Settings > System > Button hints, on in a shell nobody has
    /// asked otherwise.
    ///
    /// Carried for the clock's reason, and it is the setting with the widest
    /// reach of any of them: it is already the one key in `shell.toml` that
    /// *applications* read as well, so a session with the hints off has them
    /// off in every program built on the toolkit. A login screen that drew a
    /// row of button pictures in front of somebody who had switched them off
    /// everywhere else would be the last screen on the machine still
    /// explaining itself.
    ///
    /// A look that says nothing leaves them on — which is what the shell does
    /// with a silent file, so that a settings file older than the row does not
    /// read as somebody having turned it off. See [`Look::button_hints`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button_hints: Option<bool>,
    /// Whether the controller is the control that account last reached for,
    /// rather than a keyboard.
    ///
    /// Nothing chooses it; the shell watches for it and writes down what it
    /// saw, because it is a statement about a person rather than about a
    /// session — somebody who spent all of last night typing does not become a
    /// controller user again by turning the machine off.
    ///
    /// It decides which control the legend draws a picture of, and that is the
    /// whole of what it is for here. It is only ever the *first* answer: the
    /// greeter has its own eyes, and the first press it sees settles the
    /// question for the rest of the screen's life. See [`Look::pad_in_hand`].
    ///
    /// A look that says nothing leaves the pad, which is the shell's own
    /// default and the console's: a machine with nobody's habits recorded yet
    /// is a machine in front of a sofa.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller_in_hand: Option<bool>,
    /// Which arrangement the account's keyboards are set to, as the one key
    /// `shell.toml` writes both halves of that answer in — `pl (qwertz)`, or a
    /// bare layout where there is no variant.
    ///
    /// The one thing carried here that is about what somebody can *type* rather
    /// than what they are looking at, and the reason it is carried is the same
    /// as the reason it is a setting at all: a password with a Polish or a
    /// French letter in it cannot be typed on a board offering American ones,
    /// and this login screen draws the board. See [`Look::keyboard`].
    ///
    /// It says nothing about the physical keyboard, which belongs to whichever
    /// compositor is holding the seat and is the machine's own business before
    /// anybody has signed in. This is what the *picture* of one prints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard_layout: Option<String>,
    /// What a display with no section of its own is set to, which is the
    /// shell's own arrangement for these four keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr_sdr_brightness: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr_srgb_intensity: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr_peak_brightness: Option<u16>,
    /// Where the machine is, for the night light kept from sunset to sunrise,
    /// on a machine whose settings file names a place.
    ///
    /// Nothing in either project writes these: the shell derives the position
    /// from the time zone, and so does this greeter — see [`crate::sun`]. They
    /// are here because the shell *reads* them, as the one escape hatch for
    /// somebody who lives a long way from their zone's representative city,
    /// and a login screen that ignored them would put sunset somewhere else
    /// than the session does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub night_light_latitude: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub night_light_longitude: Option<f64>,
    /// The ALSA card the session plays through, and the gain it plays at — see
    /// [`crate::audio`].
    ///
    /// Neither is in `shell.toml` and neither is the shell's to keep: which
    /// device the machine comes out of belongs to the sound server, and
    /// LineXinBar deliberately does not hold a second opinion about it. They are
    /// asked of the server itself, in the account's own session, at the moment
    /// the look is published — which is the only place and the only moment
    /// either can be known.
    ///
    /// They are here because a greeter cannot ask. It has no sound server of its
    /// own, so without these it is reduced to guessing which of five cards a
    /// machine has is the one somebody is listening to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound_card: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound_gain: Option<f32>,
    /// How outputs with no position of their own are arranged, from the
    /// compositor's own config rather than the shell's settings. There is no
    /// key for this in `shell.toml`; it is carried so that a desk arranged
    /// vertically is still arranged vertically at the login screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_layout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_gap: Option<i32>,
    /// One section per connector, under the name the compositor gives it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub display: BTreeMap<String, DisplayLook>,
}

/// One display's settings, gathered from both places LineXinBar keeps them.
///
/// The first eleven keys are `shell.toml`'s, written by Settings > Display as
/// the user changes them. `position`, `scale`, `enabled` and `adaptive-sync`
/// have no page in Settings and live only in the compositor's own config; they
/// are here because a login screen that ignored them would light a screen the
/// user had turned off, or stack two monitors the user had placed side by side.
#[derive(Debug, Default, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct DisplayLook {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[i32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive_sync: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr_sdr_brightness: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr_srgb_intensity: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr_peak_brightness: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub night_light: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub night_light_temperature: Option<u16>,
    /// When the light burns: `all-day`, `sunset-to-sunrise`, or `hours`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub night_light_schedule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub night_light_from: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub night_light_until: Option<u8>,
}

impl Look {
    /// The palette this look asks for, if the shell offers one by that name.
    pub fn accent(&self) -> Option<&'static str> {
        accent::canonical(self.accent.as_deref()?)
    }

    /// Which clock it writes a time on. A look that says nothing, or names a
    /// clock this build has not got, leaves the language to answer — which is
    /// [`crate::clock::Clock::FromLanguage`], the default.
    pub fn clock(&self) -> crate::clock::Clock {
        self.clock
            .as_deref()
            .and_then(crate::clock::Clock::parse)
            .unwrap_or_default()
    }

    /// Whether this account's screen says what its buttons do. A look that says
    /// nothing leaves them on — see [`Look::button_hints`] the field, where the
    /// default is argued.
    pub fn button_hints(&self) -> bool {
        self.button_hints.unwrap_or(true)
    }

    /// Whether the pad is the control this account last reached for. A look
    /// that says nothing leaves the pad.
    pub fn pad_in_hand(&self) -> bool {
        self.controller_in_hand.unwrap_or(true)
    }

    /// The material it asks one half of the screen to be drawn in, if this
    /// greeter has one by that name.
    ///
    /// The half's own key first, then the one both halves shared before they were
    /// split: a look copied out of a file written by the older shell says one
    /// thing about the whole of it, and it meant it about both.
    pub fn theme(&self, part: crate::visual::theme::Part) -> Option<&'static str> {
        let named = match part {
            crate::visual::theme::Part::Wallpaper => self.theme_wallpaper.as_deref(),
            crate::visual::theme::Part::Icons => self.theme_icons.as_deref(),
        }
        .or(self.theme.as_deref())?;
        accent::canonical_theme(named)
    }

    /// The layout and variant this look asks a board to print, as xkb names
    /// them.
    ///
    /// Not checked against xkeyboard-config here, because this greeter has no
    /// business holding a second list of every layout in the world: a name it
    /// cannot compile is found out by compiling it, and the board falls back to
    /// its own ANSI rows. See [`crate::keyboard::note_layout`].
    pub fn keyboard(&self) -> Option<(String, String)> {
        crate::keyboard::layout_key(self.keyboard_layout.as_deref()?)
    }

    /// Where the sun is worked out for.
    ///
    /// The account's own settings first, then the machine's time zone, in the
    /// order the shell consults them: a file that says where the machine is
    /// has the last word, and everything else falls back to the zone table.
    /// `None` is a machine that has neither, which [`burning`] answers rather
    /// than guesses at.
    ///
    /// Asked once per written configuration rather than once per display: it
    /// reads a file, and every display on a machine is on the same machine.
    pub fn here(&self) -> Option<crate::sun::Location> {
        self.night_light_latitude
            .zip(self.night_light_longitude)
            .and_then(|(latitude, longitude)| crate::sun::Location::exact(latitude, longitude))
            .or_else(crate::sun::location)
    }

    /// Read an account's own LineXinBar configuration.
    ///
    /// Both files, because the settings are kept in both: `shell.toml` is what
    /// the Settings column writes as the user changes it, and `config.toml` is
    /// what the compositor was started with. Where they overlap the shell wins,
    /// for the same reason it wins inside a session — it is the later of the
    /// two, and it is the one with a page in front of it.
    ///
    /// Only readable by the account itself on most machines. This runs there.
    pub fn read(home: &Path, config_home: Option<&Path>) -> Self {
        let settings = accent::settings_path_with_config_home(home, config_home);
        let mut look = read_toml::<Self>(&settings).unwrap_or_default();
        let compositor = settings.with_file_name("config.toml");
        if let Some(config) = read_toml::<CompositorConfig>(&compositor) {
            look.output_layout = config.general.output_layout;
            look.output_gap = config.general.output_gap;
            // The shell first, as everywhere else here: `[input]` is what the
            // session *starts* at, and `keyboard-layout` is what somebody has
            // since chosen on a page in front of them. A machine whose owner
            // set the layout by hand and never opened that page has only the
            // one answer, and it is this one.
            look.keyboard_layout = look.keyboard_layout.take().or_else(|| config.input.key());
            for output in config.outputs {
                let display = look.display.entry(output.name).or_default();
                display.position = display.position.or(output.position);
                display.scale = display.scale.or(output.scale);
                display.enabled = display.enabled.or(output.enabled);
                display.adaptive_sync = display.adaptive_sync.or(output.adaptive_sync);
                display.mode = display.mode.take().or(output.mode);
                display.transform = display.transform.take().or(output.transform);
                display.hdr = display.hdr.or(output.hdr);
                display.hdr_sdr_brightness =
                    display.hdr_sdr_brightness.or(output.hdr_sdr_brightness);
                display.hdr_srgb_intensity =
                    display.hdr_srgb_intensity.or(output.hdr_srgb_intensity);
                display.hdr_peak_brightness =
                    display.hdr_peak_brightness.or(output.hdr_peak_brightness);
                display.night_light = display.night_light.or(output.night_light);
                display.night_light_temperature = display
                    .night_light_temperature
                    .or(output.night_light_temperature);
            }
        }
        look.sane()
    }

    /// Drop everything that is not a setting, and bring the rest inside its
    /// bounds.
    ///
    /// Applied on the way out and again on the way in. On the way out because
    /// there is no reason to publish what will be refused; on the way in
    /// because what is read has been in a file an account can write, and it is
    /// about to become a configuration file the greeter's own compositor is
    /// started with. A value out of range is brought to the nearest end, as
    /// the shell does with a hand-written one; a value that is not a value at
    /// all is dropped, leaving that display whatever the compositor would have
    /// done unasked.
    pub fn sane(mut self) -> Self {
        self.accent = self
            .accent
            .and_then(|name| accent::canonical(&name).map(str::to_string));
        for named in [
            &mut self.theme_wallpaper,
            &mut self.theme_icons,
            &mut self.theme,
        ] {
            *named = named
                .take()
                .and_then(|name| accent::canonical_theme(&name).map(str::to_string));
        }
        self.hdr_srgb_intensity = self.hdr_srgb_intensity.map(|value| value.min(100));
        // Dropped rather than clamped, and dropped as a pair: half a
        // coordinate is not a place, and a latitude brought to the nearest
        // pole would be a sunset worked out for somewhere nobody lives. What
        // is left is the time zone, which is where a machine that named
        // nothing was already getting its answer.
        let named = self
            .night_light_latitude
            .zip(self.night_light_longitude)
            .filter(|(latitude, longitude)| {
                crate::sun::Location::exact(*latitude, *longitude).is_some()
            });
        (self.night_light_latitude, self.night_light_longitude) = match named {
            Some((latitude, longitude)) => (Some(coarse(latitude)), Some(coarse(longitude))),
            None => (None, None),
        };
        // An ALSA card id, which is what it will be compared against and never
        // interpolated into anything. Bounded and restricted all the same: it
        // arrives in a file an account writes, and the greeter is about to log
        // it and match it against its own device list.
        self.sound_card = self.sound_card.filter(|card| {
            !card.is_empty()
                && card.len() <= 32
                && card
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
        });
        // A multiplier, so anything outside it is not one. Dropped rather than
        // clamped where it is not a number at all: a login screen with no gain
        // published plays at the card's own level, which is a better answer
        // than one invented here.
        self.sound_gain = self
            .sound_gain
            .filter(|gain| gain.is_finite() && (0.0..=1.0).contains(gain));
        // An xkb layout and variant, and it arrives in a file an account
        // writes. Bounded and held to the characters xkeyboard-config names its
        // own layouts and variants with, on the same terms as the sound card
        // above: it is about to be handed to a keymap compiler and written into
        // the log.
        self.keyboard_layout = self.keyboard_layout.filter(|key| {
            !key.is_empty()
                && key.len() <= 64
                && key.chars().all(|character| {
                    character.is_ascii_alphanumeric() || "-_() ".contains(character)
                })
        });
        self.output_layout = self
            .output_layout
            .filter(|layout| LAYOUTS.contains(&layout.as_str()));
        self.output_gap = self.output_gap.map(|gap| gap.clamp(0, 4096));
        self.display.retain(|name, _| is_connector_name(name));
        while self.display.len() > MAX_DISPLAYS {
            // Whichever the ordering leaves last, so that a file with more
            // sections than a machine could have is bounded rather than
            // refused: the displays somebody is actually looking at are as
            // likely to be kept as any.
            let last = self
                .display
                .keys()
                .next_back()
                .cloned()
                .expect("a map longer than the limit has a last key");
            self.display.remove(&last);
        }
        for display in self.display.values_mut() {
            display.mode = display.mode.take().filter(|mode| is_mode(mode));
            display.transform = display
                .transform
                .take()
                .filter(|transform| TRANSFORMS.contains(&transform.as_str()));
            display.position = display
                .position
                .map(|[x, y]| [x.clamp(-65536, 65536), y.clamp(-65536, 65536)]);
            display.scale = display
                .scale
                .filter(|scale| scale.is_finite() && (0.25..=8.0).contains(scale));
            display.hdr_srgb_intensity = display.hdr_srgb_intensity.map(|value| value.min(100));
            display.night_light_temperature = display
                .night_light_temperature
                .map(|kelvin| kelvin.clamp(WARMEST_KELVIN, NEUTRAL_KELVIN));
            display.night_light_schedule = display
                .night_light_schedule
                .take()
                .filter(|schedule| SCHEDULES.contains(&schedule.as_str()));
            display.night_light_from = display.night_light_from.filter(|hour| *hour < 24);
            display.night_light_until = display.night_light_until.filter(|hour| *hour < 24);
        }
        self
    }
}

/// A coordinate rounded to a tenth of a degree, which is a town rather than a
/// house.
///
/// Applied on the way out, which is the direction that matters: a published
/// look is world-readable — it has to be, since the greeter reads it as nobody
/// in particular — and these two numbers are the only thing in the file that
/// says where its owner is. A tenth of a degree is about eleven kilometres, and
/// the question they are here to answer is what time the sun sets, which over
/// eleven kilometres moves by under a minute. Nobody looking at a login screen
/// can tell; anybody with an account on the machine can tell the difference
/// between a town and a street.
///
/// The account's own settings keep whatever precision was written in them. This
/// is about the copy that comes out to meet the greeter, and it is the only
/// thing in that copy that is deliberately less exact than its source.
fn coarse(degrees: f64) -> f64 {
    (degrees * 10.0).round() / 10.0
}

/// When the night light burns, as the shell's settings spell it.
const SCHEDULES: [&str; 3] = ["all-day", "sunset-to-sunrise", "hours"];

/// LineXinBar's compositor config, as this writes one.
///
/// A separate set of types from the ones above, and deliberately so: this is
/// the other project's schema, in the other project's spelling — snake_case
/// keys, `[[output]]` blocks, `adaptive_sync` rather than `adaptive-sync` —
/// and it is `deny_unknown_fields` at the far end, so a key invented here is a
/// compositor that refuses to start and a machine with no login screen.
///
/// Nothing in it is a command except `shell`, which is this program's own path
/// and never comes from a published file. `autostart`, `env` and the
/// keybindings are not modelled at all: an account's commands must not become
/// the greeter's.
#[derive(Debug, Serialize)]
struct CompositorFile {
    general: CompositorFileGeneral,
    #[serde(rename = "output", skip_serializing_if = "Vec::is_empty")]
    outputs: Vec<CompositorFileOutput>,
}

#[derive(Debug, Serialize)]
struct CompositorFileGeneral {
    shell: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_gap: Option<i32>,
}

#[derive(Debug, Default, Serialize)]
struct CompositorFileOutput {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<[i32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transform: Option<String>,
    /// Never `Some(false)`, and in practice never `Some` at all.
    ///
    /// This is the one setting in a published look that can decide whether
    /// there is a login screen. The rest describe how a picture appears —
    /// wrong mode, wrong corner, wrong colour, all of them visible and all of
    /// them recoverable by the person looking at them. `enabled = false` is
    /// the compositor leaving a connector dark, and a look naming every
    /// connector on the machine that way is a machine whose next login screen
    /// is on no screen at all.
    ///
    /// It cannot be repaired by counting, either. A greeter cannot check that
    /// one enabled display is left, because it has no idea which displays are
    /// there: the file is written before the compositor has opened the DRM
    /// device, and an account that leaves `DP-1` enabled and `HDMI-A-1`
    /// disabled has darkened a machine that today has only the second one
    /// plugged in. A stale file does it by accident, and a shared machine lets
    /// one account do it to everybody else on purpose.
    ///
    /// So the greeter's compositor is never told to turn a display off. What
    /// the account meant by it is still honoured — by the session's own
    /// compositor, a second later, which is where the setting belongs and
    /// where it can be undone by whoever set it. A login screen is the one
    /// thing on this machine that has to come up on everything.
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adaptive_sync: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hdr: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hdr_sdr_brightness: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hdr_srgb_intensity: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hdr_peak_brightness: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    night_light: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    night_light_temperature: Option<u16>,
}

impl Look {
    /// This look as a compositor configuration for the greeter to come up in,
    /// with `shell` as the client that compositor starts.
    ///
    /// The greeter is a Wayland client. It cannot set a mode, place an output,
    /// turn a display off or drive one in HDR — every one of those is decided
    /// by whoever holds the DRM master, before there is a surface to draw on.
    /// So what the greeter does with the settings it was published is hand
    /// them to its own compositor, which brings the displays up exactly as the
    /// session's compositor would have, and then draws a login screen on them.
    ///
    /// `now` is the machine's civil time, for the one setting that depends on
    /// it. See [`burning`].
    pub fn compositor_config(
        &self,
        shell: &str,
        now: Option<crate::clock::Now>,
    ) -> anyhow::Result<String> {
        let mut outputs = Vec::with_capacity(self.display.len() + 1);
        // Once, for every display in the file. See [`Look::here`].
        let here = self.here();
        // What a connector with no settings of its own comes up in. The
        // compositor takes the whole of the entry that matches — the most
        // specific one, or this — rather than merging them, so this carries
        // only what is inherited and every named entry below is complete.
        if self.hdr.is_some()
            || self.hdr_sdr_brightness.is_some()
            || self.hdr_srgb_intensity.is_some()
            || self.hdr_peak_brightness.is_some()
        {
            outputs.push(CompositorFileOutput {
                name: "*".to_string(),
                hdr: self.hdr,
                hdr_sdr_brightness: self.hdr_sdr_brightness,
                hdr_srgb_intensity: self.hdr_srgb_intensity,
                hdr_peak_brightness: self.hdr_peak_brightness,
                ..CompositorFileOutput::default()
            });
        }
        for (name, display) in &self.display {
            if name == "*" {
                continue;
            }
            outputs.push(CompositorFileOutput {
                name: name.clone(),
                mode: display.mode.clone(),
                position: display.position,
                scale: display.scale,
                transform: display.transform.clone(),
                // Deliberately never carried. See `enabled` on
                // [`CompositorFileOutput`] for the whole of why.
                enabled: None,
                adaptive_sync: display.adaptive_sync,
                hdr: display.hdr.or(self.hdr),
                hdr_sdr_brightness: display.hdr_sdr_brightness.or(self.hdr_sdr_brightness),
                hdr_srgb_intensity: display.hdr_srgb_intensity.or(self.hdr_srgb_intensity),
                hdr_peak_brightness: display.hdr_peak_brightness.or(self.hdr_peak_brightness),
                night_light: Some(burning(display, now, here)),
                night_light_temperature: display.night_light_temperature,
            });
        }
        let file = CompositorFile {
            general: CompositorFileGeneral {
                shell: shell.to_string(),
                output_layout: self.output_layout.clone(),
                output_gap: self.output_gap,
            },
            outputs,
        };
        let body = toml::to_string_pretty(&file)
            .context("could not serialize the greeter compositor's configuration")?;
        Ok(format!(
            "# The displays the login screen is drawn on, written by\n\
             # console-experience-desktop-manager before its compositor starts.\n\
             #\n\
             # Rewritten from scratch every time the greeter comes up, from the\n\
             # look the last signed-in account published, so that the login\n\
             # screen brings the displays up in the same modes, the same places\n\
             # and the same dynamic range as the session about to start. Editing\n\
             # it by hand is pointless; edit the settings it came from.\n\
             {body}"
        ))
    }
}

/// Whether a display's night light should be burning at `now`.
///
/// The compositor has no clock and no timezone — LineXinBar's own config says
/// so, and gives it a plain on-or-off for a session with no shell to work it
/// out. This greeter *is* a session with no shell, so it works it out here,
/// and it answers all three of the schedules the shell offers:
///
/// - `all-day` burns whenever the light is on at all,
/// - `hours` is the local clock against the two hours the user kept,
/// - `sunset-to-sunrise` is the sun where this machine is, which
///   [`crate::sun`] works out from the time zone — or from the coordinates the
///   settings file names, on a machine whose owner wrote them down.
///
/// Every answer here is the shell's own answer, because a login screen that
/// disagreed with the session about whether it is night would warm a display
/// and then let the shell cool it a second later, in front of somebody who had
/// asked for neither. So the fallbacks are the shell's too, down to which way
/// each of them errs: hours that meet are no window rather than a whole day,
/// a machine that cannot say where it is burns rather than staying cold — the
/// switch is on, and "on" is what a user who cannot be shown a sunset meant by
/// it — and a day at a latitude where the sun does not come up is a day that
/// is night.
///
/// `now` is `None` only where the C library cannot read local time at all, on
/// a machine with no zone data. That is treated as having no schedule rather
/// than an unsatisfied one, which is again what the shell does: the switch
/// then means what it says.
///
/// `here` is where the machine is, resolved once for the whole file rather
/// than per display — see [`Look::here`].
pub(crate) fn burning(
    display: &DisplayLook,
    now: Option<crate::clock::Now>,
    here: Option<crate::sun::Location>,
) -> bool {
    if display.night_light != Some(true) {
        return false;
    }
    let Some(now) = now else {
        return true;
    };
    let minute = now.hour as u16 * 60 + now.minute as u16;
    match display.night_light_schedule.as_deref() {
        None | Some("all-day") => true,
        Some("hours") => {
            let (Some(from), Some(until)) = (display.night_light_from, display.night_light_until)
            else {
                // Half a window is not one. The shell writes both hours
                // whenever it writes either, so this is a hand-edited file,
                // and All day is what it falls back to there as well.
                return true;
            };
            within(from as u16 * 60, until as u16 * 60, minute)
        }
        Some("sunset-to-sunrise") => match here.map(|at| sun_at(&at, now)) {
            Some(crate::sun::Sun::Daily { sunrise, sunset }) => within(sunset, sunrise, minute),
            // A day the sun does not come up is a day that is night, and one
            // it does not go down is a day that is not. Both are the truthful
            // reading of "from sunset to sunrise" at a latitude where neither
            // happens.
            Some(crate::sun::Sun::NeverRises) => true,
            Some(crate::sun::Sun::NeverSets) => false,
            // Nothing on this machine says where it is. The shell's own
            // Settings page does not offer the sun where that is so, and puts
            // a file that names it anyway back to All day on the way in.
            None => true,
        },
        // A word neither project has. The shell keeps whatever the switch
        // says and warns; there is nobody here to warn, and the switch still
        // says on.
        _ => true,
    }
}

/// The sun where this display's account says the machine is, on `now`'s day.
fn sun_at(at: &crate::sun::Location, now: crate::clock::Now) -> crate::sun::Sun {
    crate::sun::sun(now.yday, now.year, at, now.offset)
}

/// Whether `minute` falls in the window from `from` until `until`, both
/// minutes of a day that wraps.
///
/// The end is exclusive: a light set to go off at 07:00 is off at seven, not a
/// minute past. The two being equal is an empty window rather than a full one,
/// and has to be its own arm — the wrapping test below would read it as every
/// minute instead, which is the one answer nobody asked for.
fn within(from: u16, until: u16, minute: u16) -> bool {
    match from.cmp(&until) {
        // An ordinary daytime window: 07:00 to 21:00.
        std::cmp::Ordering::Less => (from..until).contains(&minute),
        // One that wraps past midnight, which is what an evening is — and what
        // sunset to sunrise always is.
        std::cmp::Ordering::Greater => minute >= from || minute < until,
        std::cmp::Ordering::Equal => false,
    }
}

/// A connector name, as a compositor gives one: `DP-1`, `HDMI-A-1`, `eDP-1`.
///
/// Checked because a name read from a published file becomes a key in a
/// configuration file the greeter's compositor is started with, and because
/// the account that wrote it chose it. The serializer would escape anything
/// awkward, so this is not what stops a file being forged into something else;
/// it is that a name which is not a connector cannot match one, and carrying
/// it would only put junk in front of whoever reads the config next.
fn is_connector_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.:*".contains(character))
}

/// `WIDTHxHEIGHT`, `WIDTHxHEIGHT@REFRESH`, or `preferred`.
fn is_mode(mode: &str) -> bool {
    if mode == "preferred" {
        return true;
    }
    let (size, refresh) = match mode.split_once('@') {
        Some((size, refresh)) => (size, Some(refresh)),
        None => (mode, None),
    };
    let Some((width, height)) = size.split_once(['x', 'X']) else {
        return false;
    };
    let pixels = |value: &str| {
        value
            .parse::<u32>()
            .is_ok_and(|value| (1..=32768).contains(&value))
    };
    let hertz = |value: &str| {
        value
            .parse::<f64>()
            .is_ok_and(|value| value.is_finite() && (1.0..=1000.0).contains(&value))
    };
    pixels(width) && pixels(height) && refresh.is_none_or(hertz)
}

/// The subset of LineXinBar's compositor config this reads. Deliberately not
/// the whole schema: `autostart`, `env`, `shell` and the keybindings are
/// commands, and a greeter that copied an account's commands into its own
/// configuration would be running them as itself. Nothing here is a command.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CompositorConfig {
    general: CompositorGeneral,
    input: CompositorInput,
    #[serde(rename = "output")]
    outputs: Vec<CompositorOutput>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CompositorGeneral {
    output_layout: Option<String>,
    output_gap: Option<i32>,
}

/// What the session's keyboards start at, which is the answer on a machine
/// whose owner has never opened the shell's own page for it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CompositorInput {
    keyboard_layout: Option<String>,
    keyboard_variant: Option<String>,
}

impl CompositorInput {
    /// The pair written the way `shell.toml` writes it, so that a look carries
    /// one key whichever of the two files answered and nothing downstream has
    /// to know which did.
    fn key(&self) -> Option<String> {
        let layout = self.keyboard_layout.as_deref()?.trim();
        if layout.is_empty() {
            return None;
        }
        match self.keyboard_variant.as_deref().unwrap_or_default().trim() {
            "" => Some(layout.to_string()),
            variant => Some(format!("{layout} ({variant})")),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CompositorOutput {
    name: String,
    mode: Option<String>,
    position: Option<[i32; 2]>,
    scale: Option<f64>,
    transform: Option<String>,
    enabled: Option<bool>,
    adaptive_sync: Option<bool>,
    hdr: Option<bool>,
    hdr_sdr_brightness: Option<u16>,
    hdr_srgb_intensity: Option<u8>,
    hdr_peak_brightness: Option<u16>,
    night_light: Option<bool>,
    night_light_temperature: Option<u16>,
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    read_open(File::open(path).ok()?)
}

/// Read a TOML document from an already-open file, bounded.
///
/// From the descriptor rather than the path: the published file is opened
/// under rules of its own — see [`published_in`] — and reading what was opened
/// is also what keeps a check on a file and the read of that file from being
/// two different files.
fn read_open<T: serde::de::DeserializeOwned>(file: File) -> Option<T> {
    let mut raw = String::new();
    file.take(MAX_BYTES + 1).read_to_string(&mut raw).ok()?;
    if raw.len() as u64 > MAX_BYTES {
        return None;
    }
    toml::from_str(&raw).ok()
}

/// Where `name`'s look is published, or `None` for anything that is not one
/// path component.
///
/// A login name may be directory-qualified — see
/// [`crate::users::validate_login_name`] — so this is not a formality: a name
/// with a separator in it would leave the directory the system published.
pub fn published_path_in(directory: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('/') || name.starts_with('.') || name.contains('\0') {
        return None;
    }
    Some(directory.join(format!("{name}.toml")))
}

pub fn published(name: &str, uid: u32) -> Option<Look> {
    published_in(Path::new(PUBLISHED), name, uid)
}

/// What `uid` last published, if that is who published it.
///
/// The directory is one accounts write into, so a file found under an
/// account's name is not yet that account's file, and it is not even yet a
/// file. [`crate::reading::open`] settles both: it refuses anything that is
/// not a plain file, it refuses a plain file the account does not own, and it
/// refuses all of that without ever waiting on what it was pointed at — which
/// is the part a login screen cannot do without, because a named pipe left
/// under somebody's name would otherwise hold the whole screen before anyone
/// had signed in. What is left to a squatter is denying somebody a colour,
/// which is where every login screen was before any of this existed.
pub fn published_in(directory: &Path, name: &str, uid: u32) -> Option<Look> {
    let path = published_path_in(directory, name)?;
    let file = crate::reading::open(&path, crate::reading::Owner::Uid(uid))?;
    Some(read_open::<Look>(file)?.sane())
}

/// Publish `look` for `name`, as `name`.
///
/// Called by the account itself, on the way into its session. The greeter has
/// no way to run this and no way to write a file that would pass the check on
/// the way back in. The write is atomic because the file it replaces is one
/// another process reads at arbitrary moments, and half a settings file is not
/// a settings file.
pub fn publish_in(directory: &Path, name: &str, look: &Look) -> anyhow::Result<PathBuf> {
    let path = published_path_in(directory, name)
        .with_context(|| format!("{name} is not a name a file can be published under"))?;
    let document = document(&look.clone().sane())?;
    // Same directory as the destination, so the rename is a rename rather than
    // a copy, and named for this process so that two sessions starting at once
    // cannot write over each other's half-written file.
    let temporary = directory.join(format!(".{name}.{}.toml", std::process::id()));
    let written = (|| -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            // Readable by the greeter, which is the whole point of it, and
            // writable by nobody else.
            .mode(0o644)
            .open(&temporary)
            .with_context(|| format!("could not create {}", temporary.display()))?;
        file.write_all(document.as_bytes())
            .and_then(|()| file.sync_all())
            .with_context(|| format!("could not write {}", temporary.display()))
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&temporary, &path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error).with_context(|| format!("could not publish {}", path.display()));
    }
    Ok(path)
}

fn document(look: &Look) -> anyhow::Result<String> {
    let body = toml::to_string_pretty(look).context("could not serialize a published look")?;
    Ok(format!(
        "# How this account's LineXinBar is set up, published for the login\n\
         # screen by console-experience-desktop-manager as the session started.\n\
         #\n\
         # Written by the account, read by the greeter, and believed by it only\n\
         # while this file belongs to the account it is named for. Editing it by\n\
         # hand changes what the login screen looks like until the next sign-in,\n\
         # which rewrites it from the shell's own settings.\n\
         {body}"
    ))
}

/// The account running this process, and where its settings are.
struct Publisher {
    name: String,
    /// The directory LineXinBar keeps both of its files in, kept for what it
    /// can say when there is nothing in it to publish.
    settings: PathBuf,
    home: PathBuf,
    config_home: Option<PathBuf>,
}

impl Publisher {
    fn current() -> anyhow::Result<Self> {
        // SAFETY: `getuid` cannot fail and touches no memory this owns.
        let uid = unsafe { libc::getuid() };
        let user = crate::users::discover()
            .into_iter()
            .find(|user| user.uid == uid)
            .with_context(|| format!("uid {uid} is not an account this greeter would offer"))?;
        // Honoured here and nowhere else in this module: this runs inside the
        // account's own session, where the variable means what the shell means
        // by it, rather than in a greeter that has no business following one
        // account's environment into another account's files.
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
            .unwrap_or_else(|| user.home.clone());
        let config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let settings = accent::settings_path_with_config_home(&home, config_home.as_deref());
        Ok(Self {
            name: user.name,
            settings: settings
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join(".config/lxb")),
            home,
            config_home,
        })
    }

    /// This account's look, and where its session plays.
    ///
    /// `sounded` is what was published last, for the one case where the sound
    /// server cannot be asked — see [`carry_sound_forward`].
    fn look(&self, sounded: Option<&Look>) -> Look {
        let mut look = Look::read(&self.home, self.config_home.as_deref());
        // Asked of the sound server rather than read out of a file, because
        // there is no file: the device and the volume belong to the server, and
        // this is running in the one session that can see it. See
        // [`crate::audio`].
        match crate::audio::read() {
            Some(output) => {
                look.sound_card = Some(output.card);
                look.sound_gain = Some(output.gain);
            }
            None => carry_sound_forward(&mut look, sounded),
        }
        look
    }
}

/// Keep the last known sound output when this run could not ask for it.
///
/// Publishing is otherwise the whole state every time, deliberately: a display
/// that is no longer configured has to disappear from the copy rather than
/// linger in it. The sound output is the one thing here that cannot follow that
/// rule, because of *when* the first publish of a session happens.
///
/// `cedm-session` publishes before it execs the session — before the compositor,
/// before the shell, and therefore before that account's sound server exists.
/// `pactl` has nothing to answer at that moment and never will have. If that
/// publish wrote "no sound output" over what the last session left, every login
/// would erase the one fact the login screen needs, and the greeter would be
/// back to guessing between five cards on the very next logout.
///
/// So the answer is kept until a run that can actually ask replaces it: the
/// shell publishing when a setting changes, or on its way out. What is carried
/// is stale by a session at worst — the speakers somebody was using yesterday —
/// and that is a far better answer than none.
fn carry_sound_forward(look: &mut Look, sounded: Option<&Look>) {
    let Some(previous) = sounded else { return };
    if look.sound_card.is_none() {
        look.sound_card = previous.sound_card.clone();
        look.sound_gain = previous.sound_gain;
    }
}

/// Publish for the account running this process, and say what was published.
pub fn publish_for_current_account(directory: &Path) -> anyhow::Result<(Look, PathBuf)> {
    let publisher = Publisher::current()?;
    // SAFETY: `getuid` cannot fail and touches no memory this owns.
    let uid = unsafe { libc::getuid() };
    let sounded = published_in(directory, &publisher.name, uid);
    let look = publisher.look(sounded.as_ref());
    if look == Look::default() {
        bail!(
            "nothing to publish: {} has no LineXinBar settings this greeter understands",
            publisher.settings.display()
        );
    }
    let path = publish_in(directory, &publisher.name, &look)?;
    Ok((look, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(what: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "cedm-{what}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// A home with both of LineXinBar's files in it, as a machine that has run
    /// the shell has.
    fn home_with(shell: &str, compositor: Option<&str>) -> PathBuf {
        let home = scratch("home");
        let config = home.join(".config/lxb");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("shell.toml"), shell).unwrap();
        if let Some(compositor) = compositor {
            fs::write(config.join("config.toml"), compositor).unwrap();
        }
        home
    }

    /// A machine with two screens set up differently, as the shell spells it.
    ///
    /// The connectors are named the way `lxb-desktop`'s own tests name them,
    /// and for the same reason: nothing here may be a description of the desk
    /// this was written at. A fixture carrying real connector names is a test
    /// that reads as an assertion about one machine's monitors, and the next
    /// person to look at it cannot tell which half of it is the naming scheme
    /// and which half is somebody's second display. The scheme itself is
    /// checked where it belongs, in
    /// [`modes_and_connector_names_are_the_shapes_a_compositor_uses`].
    const SHELL: &str = r#"
accent = "Red"
clock = "12-hour"
button-hints = false
controller-in-hand = false
theme-wallpaper = "Simple"
hdr = false
hdr-sdr-brightness = 200
sound-volume = 1.0
steam-sort = "installed-first"

[display.TEST-OUT-1]
hdr = true
hdr-sdr-brightness = 250
hdr-srgb-intensity = 0
hdr-peak-brightness = 0
night-light = true
night-light-temperature = 3600
night-light-schedule = "sunset-to-sunrise"
night-light-from = 21
night-light-until = 7

[display.TEST-OUT-2]
night-light = true
night-light-temperature = 4000
night-light-schedule = "all-day"

[media-sort]
Music = "modified-newest-first"
"#;

    #[test]
    fn an_accounts_own_settings_are_read_as_the_shell_wrote_them() {
        let home = home_with(SHELL, None);
        let look = Look::read(&home, None);
        assert_eq!(look.accent(), Some("Red"));
        assert_eq!(look.clock(), crate::clock::Clock::TwelveHour);
        assert_eq!(
            look.theme(crate::visual::theme::Part::Wallpaper),
            Some("Simple")
        );
        assert_eq!(
            look.theme(crate::visual::theme::Part::Icons),
            None,
            "a half the file says nothing about is a half this greeter has not been told about"
        );
        assert_eq!(look.hdr, Some(false));
        assert_eq!(look.display["TEST-OUT-1"].hdr, Some(true));
        assert_eq!(look.display["TEST-OUT-1"].hdr_sdr_brightness, Some(250));
        assert_eq!(
            look.display["TEST-OUT-2"].night_light_temperature,
            Some(4000)
        );
        // Everything else in that file is the shell's business and none of the
        // greeter's: sound, sorting, and whatever is added next.
        assert_eq!(look.display.len(), 2);
        fs::remove_dir_all(home).unwrap();
    }

    /// The clock survives being published and read back, and a file that says
    /// nothing about it leaves the account's language to answer.
    ///
    /// The second half is what every look written before the shell had the row
    /// looks like, which is every look on every machine today — so it has to
    /// be the one that changes nothing.
    #[test]
    fn the_clock_is_carried_and_a_look_that_says_nothing_leaves_it_to_the_language() {
        let home = home_with(SHELL, None);
        let look = Look::read(&home, None);
        assert_eq!(look.clock(), crate::clock::Clock::TwelveHour);

        let published = toml::to_string(&look).expect("a look is written as TOML");
        assert!(published.contains("clock = \"12-hour\""), "{published}");
        let back: Look = toml::from_str(&published).expect("and read back");
        assert_eq!(back.clock(), crate::clock::Clock::TwelveHour);

        assert_eq!(
            Look::default().clock(),
            crate::clock::Clock::FromLanguage,
            "a look with nothing in it names no clock"
        );
        // And neither does one naming a clock this build has not got, which is
        // the same answer a mistyped display mode gets.
        let odd = Look {
            clock: Some("sundial".into()),
            ..Look::default()
        };
        assert_eq!(odd.clock(), crate::clock::Clock::FromLanguage);
        assert!(
            !toml::to_string(&Look::default())
                .expect("a look is written as TOML")
                .contains("clock"),
            "a look that says nothing writes nothing"
        );
        fs::remove_dir_all(home).unwrap();
    }

    /// The two settings the legend is drawn from survive the same round trip,
    /// and a look that says nothing about either leaves the row on and the pad
    /// in hand.
    ///
    /// The second half is the one that matters: every look published before
    /// the greeter had a legend is silent about both, which is every look on
    /// every machine today. Reading that silence as "no hints" would turn the
    /// row off on precisely the machines it was added for, and reading it as
    /// "keyboard" would draw keycaps on a console.
    #[test]
    fn the_legends_two_settings_are_carried_and_silence_leaves_the_row_on() {
        let home = home_with(SHELL, None);
        let look = Look::read(&home, None);
        assert!(!look.button_hints(), "the file says the hints are off");
        assert!(
            !look.pad_in_hand(),
            "and that a keyboard is what is in hand"
        );

        let published = toml::to_string(&look).expect("a look is written as TOML");
        assert!(published.contains("button-hints = false"), "{published}");
        assert!(
            published.contains("controller-in-hand = false"),
            "{published}"
        );
        let back: Look = toml::from_str(&published).expect("and read back");
        assert!(!back.button_hints());
        assert!(!back.pad_in_hand());

        assert!(
            Look::default().button_hints(),
            "a look with nothing in it still says what the buttons do"
        );
        assert!(
            Look::default().pad_in_hand(),
            "and is a console until something says otherwise"
        );
        let silent = toml::to_string(&Look::default()).expect("a look is written as TOML");
        assert!(
            !silent.contains("button-hints") && !silent.contains("controller-in-hand"),
            "a look that says nothing writes nothing: {silent}"
        );
        fs::remove_dir_all(home).unwrap();
    }

    /// Where both files describe one display, the shell's settings win: they
    /// are what the user changed last, from a page made for changing them.
    #[test]
    fn the_shells_settings_win_and_the_compositors_placement_is_kept() {
        let home = home_with(
            SHELL,
            Some(
                r#"
[general]
output_layout = "vertical"
output_gap = 16
shell = "something-that-must-not-be-copied"
autostart = ["also-not-this"]

[[output]]
name = "TEST-OUT-1"
mode = "1920x1080@60"
position = [0, 0]
hdr = false

[[output]]
name = "TEST-OUT-2"
position = [1920, 0]
enabled = false
"#,
            ),
        );
        let look = Look::read(&home, None);
        assert_eq!(look.output_layout.as_deref(), Some("vertical"));
        assert_eq!(look.output_gap, Some(16));
        // The compositor's config is the only place a mode or a corner is
        // written, so both are carried.
        assert_eq!(
            look.display["TEST-OUT-1"].mode.as_deref(),
            Some("1920x1080@60")
        );
        assert_eq!(look.display["TEST-OUT-1"].position, Some([0, 0]));
        assert_eq!(look.display["TEST-OUT-2"].enabled, Some(false));
        // And HDR is the shell's, which says the opposite of the file the
        // session was started with.
        assert_eq!(look.display["TEST-OUT-1"].hdr, Some(true));
        fs::remove_dir_all(home).unwrap();
    }

    /// The two halves of the shell's Theme setting, and the key they shared
    /// before there were two of them.
    ///
    /// This greeter draws both — the wallpaper, and the shell's own marks in
    /// its clock and its buttons — so it reads both keys. A file from the older
    /// shell says one thing about the whole of it and meant it about both,
    /// which is the difference between a machine deliberately stood down to
    /// `Simple` staying there across an update and one that comes back up in
    /// the water.
    ///
    /// Asked of `Look` because `Look` is what answers it. It used to be asked
    /// of a reader in `accent` that opened a selected account's home directory,
    /// and that reader is gone; the same file reaches the login screen as the
    /// copy the account publishes, and this is the type that understands it.
    #[test]
    fn both_halves_of_the_theme_and_the_key_they_used_to_share() {
        let wallpaper = crate::visual::theme::Part::Wallpaper;
        let icons = crate::visual::theme::Part::Icons;

        let look = Look::read(&home_with("theme-wallpaper = \"simple\"\n", None), None);
        assert_eq!(
            look.theme(wallpaper),
            Some("Simple"),
            "matched without regard to case, as the accent is"
        );
        assert_eq!(look.theme(icons), None);

        let look = Look::read(&home_with("theme = \"Simple\"\n", None), None);
        for part in [wallpaper, icons] {
            assert_eq!(look.theme(part), Some("Simple"));
        }

        let look = Look::read(
            &home_with("theme = \"Simple\"\ntheme-icons = \"Default\"\n", None),
            None,
        );
        assert_eq!(look.theme(wallpaper), Some("Simple"));
        assert_eq!(
            look.theme(icons),
            Some("Default"),
            "the newer, narrower key outranks the one it replaced"
        );

        let look = Look::read(&home_with("theme-wallpaper = \"Frosted\"\n", None), None);
        assert_eq!(
            look.theme(wallpaper),
            None,
            "a material this greeter has not got is no answer at all"
        );
    }

    /// An account whose shell stands one of their own pictures behind
    /// everything is greeted by the shell's own scene, in their accent.
    ///
    /// The value is understood and deliberately not carried out — the picture
    /// is a file inside that account's home directory and this login screen
    /// stands in front of every account on the machine. What must never happen
    /// is the greeter refusing the whole look over it and coming up in
    /// somebody else's colour.
    #[test]
    fn a_shell_showing_the_users_own_picture_keeps_the_rest_of_its_look() {
        let home = home_with(
            &format!(
                "accent = \"Green\"\ntheme-wallpaper = \"{}\"\n\
                 theme-icons = \"Simple\"\nwallpaper-file = \"/home/somebody/x.jpg\"\n",
                accent::CUSTOM_WALLPAPER
            ),
            None,
        );
        let look = Look::read(&home, None);
        assert_eq!(
            look.theme(crate::visual::theme::Part::Wallpaper),
            Some(accent::DEFAULT_THEME),
            "the picture cannot be read here, so the scene is what stands in for it"
        );
        assert_eq!(look.accent(), Some("Green"));
        assert_eq!(
            look.theme(crate::visual::theme::Part::Icons),
            Some("Simple")
        );
    }

    /// A settings file larger than a login screen reads is no settings file.
    #[test]
    fn refuses_an_oversized_settings_file() {
        let mut contents = String::from("accent = \"Blue\"\n#");
        contents.push_str(&"x".repeat(MAX_BYTES as usize));
        let look = Look::read(&home_with(&contents, None), None);
        assert_eq!(look.accent(), None);
    }

    /// The keyboard an account types on is carried like the accent is, and out
    /// of both files: the shell's own setting is what somebody chose on a page,
    /// and the compositor's `[input]` is what the session starts at on a
    /// machine whose owner has never opened that page.
    #[test]
    fn the_keyboard_an_account_types_on_is_carried_out_of_whichever_file_has_it() {
        let chosen = home_with(
            "keyboard-layout = \"pl (qwertz)\"\n",
            Some("[input]\nkeyboard_layout = \"de\"\nkeyboard_variant = \"neo\"\n"),
        );
        assert_eq!(
            Look::read(&chosen, None).keyboard(),
            Some(("pl".to_string(), "qwertz".to_string())),
            "the page somebody chose on outranks the file the session started with"
        );
        fs::remove_dir_all(chosen).unwrap();

        let started = home_with(
            "accent = \"Red\"\n",
            Some("[input]\nkeyboard_layout = \"de\"\nkeyboard_variant = \"neo\"\n"),
        );
        assert_eq!(
            Look::read(&started, None).keyboard(),
            Some(("de".to_string(), "neo".to_string()))
        );
        fs::remove_dir_all(started).unwrap();

        // A layout with no variant is one key either way round, and a file
        // that names no keyboard at all names none.
        let bare = home_with(
            "accent = \"Red\"\n",
            Some("[input]\nkeyboard_layout = \"fr\"\n"),
        );
        assert_eq!(
            Look::read(&bare, None).keyboard(),
            Some(("fr".to_string(), String::new()))
        );
        fs::remove_dir_all(bare).unwrap();

        let silent = home_with(SHELL, None);
        assert_eq!(Look::read(&silent, None).keyboard(), None);
        fs::remove_dir_all(silent).unwrap();
    }

    /// The key arrives in a file an account writes, and goes to a keymap
    /// compiler and into the log. Anything that is not an xkb name is not one.
    #[test]
    fn a_keyboard_key_that_is_not_an_xkb_name_is_dropped() {
        let kept = |key: &str| {
            Look {
                keyboard_layout: Some(key.to_string()),
                ..Look::default()
            }
            .sane()
            .keyboard_layout
        };
        assert_eq!(kept("pl (qwertz)").as_deref(), Some("pl (qwertz)"));
        assert_eq!(kept("us"), None.or(Some("us".to_string())));
        assert_eq!(kept(""), None);
        assert_eq!(kept("pl; rm -rf /"), None);
        assert_eq!(kept("../../etc/passwd"), None);
        assert_eq!(kept(&"a".repeat(65)), None);
    }

    #[test]
    fn a_published_look_is_read_back_for_the_account_that_owns_it() {
        let root = scratch("published");
        let home = home_with(SHELL, None);
        let look = Look::read(&home, None);
        publish_in(&root, "alex", &look).unwrap();

        // SAFETY: `getuid` cannot fail and touches no memory this owns.
        let mine = unsafe { libc::getuid() };
        let read = published_in(&root, "alex", mine).expect("published for its owner");
        assert_eq!(read, look);
        assert_eq!(read.accent(), Some("Red"));
        assert_eq!(published_in(&root, "alex", mine.wrapping_add(1)), None);
        assert_eq!(published_in(&root, "nobody", mine), None);

        // Published again, over the top, leaving nothing behind.
        publish_in(&root, "alex", &look).unwrap();
        assert_eq!(
            fs::read_dir(&root).unwrap().count(),
            1,
            "publishing left a temporary file behind"
        );

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    /// A published file is one component under the directory, whatever the
    /// account is called. Login names can be directory-qualified, and a name
    /// with a separator in it must not reach out of the directory the system
    /// published — in either direction.
    #[test]
    fn a_name_that_is_a_path_is_not_published_or_read() {
        let root = scratch("published-names");
        for name in ["../escape", "a/b", "", ".hidden", "nul\0byte"] {
            assert_eq!(published_path_in(&root, name), None, "{name:?}");
            assert!(
                publish_in(&root, name, &Look::default()).is_err(),
                "{name:?}"
            );
            assert_eq!(published_in(&root, name, 0), None, "{name:?}");
        }
        assert_eq!(
            published_path_in(&root, "DOMAIN\\person"),
            Some(root.join("DOMAIN\\person.toml"))
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// A symlink planted under an account's name is not that account's file,
    /// whatever it points at and whoever owns what it points at.
    #[test]
    fn a_published_look_is_never_followed_to_somewhere_else() {
        let root = scratch("published-symlink");
        let elsewhere = root.join("elsewhere.toml");
        fs::write(&elsewhere, "accent = \"Green\"\n").unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join("alex.toml")).unwrap();
        // SAFETY: `getuid` cannot fail and touches no memory this owns.
        let mine = unsafe { libc::getuid() };
        assert_eq!(published_in(&root, "alex", mine), None);
        fs::remove_dir_all(root).unwrap();
    }

    /// What arrives in a published file was written by an account, and what it
    /// becomes is a configuration file the greeter's own compositor is started
    /// with. Everything in it is either a setting or gone.
    #[test]
    fn a_published_look_is_brought_inside_its_bounds_before_it_is_believed() {
        let root = scratch("published-bounds");
        let path = root.join("alex.toml");
        fs::write(
            &path,
            r#"
accent = "Chartreuse"
output-layout = "diagonal"
output-gap = -5

[display."TEST-OUT-1"]
mode = "not a mode"
transform = "sideways"
scale = 0.0
position = [999999, -999999]
hdr-srgb-intensity = 250
night-light-temperature = 12000
night-light-schedule = "whenever"
night-light-from = 99

[display."rm -rf /"]
hdr = true
"#,
        )
        .unwrap();
        // SAFETY: `getuid` cannot fail and touches no memory this owns.
        let mine = unsafe { libc::getuid() };
        let look = published_in(&root, "alex", mine).expect("a file this account owns");
        assert_eq!(look.accent(), None);
        assert_eq!(look.output_layout, None);
        assert_eq!(look.output_gap, Some(0));
        let display = &look.display["TEST-OUT-1"];
        assert_eq!(display.mode, None);
        assert_eq!(display.transform, None);
        assert_eq!(display.scale, None);
        assert_eq!(display.position, Some([65536, -65536]));
        assert_eq!(display.hdr_srgb_intensity, Some(100));
        assert_eq!(display.night_light_temperature, Some(NEUTRAL_KELVIN));
        assert_eq!(display.night_light_schedule, None);
        assert_eq!(display.night_light_from, None);
        assert!(
            !look.display.contains_key("rm -rf /"),
            "a section name that is not a connector was kept"
        );

        // And nothing at all when it is not a settings file.
        fs::write(&path, "accent = [broken").unwrap();
        assert_eq!(published_in(&root, "alex", mine), None);
        let mut oversized = String::from("accent = \"Blue\"\n#");
        oversized.push_str(&"x".repeat(MAX_BYTES as usize));
        fs::write(&path, oversized).unwrap();
        assert_eq!(published_in(&root, "alex", mine), None);

        fs::remove_dir_all(root).unwrap();
    }

    /// A name squatted with something that is not a file cannot hold the login
    /// screen.
    ///
    /// The published directory is writable by every account — that is the whole
    /// design of it — so anybody can create `somebody-else.toml`, and what they
    /// create does not have to be a file. A named pipe with no writer is what
    /// `open` never comes back from, and the greeter reads the published look
    /// of every account it enumerates as it starts: not the selected one, every
    /// one. So one pipe, planted under any account's name, used to be a login
    /// screen that never appeared, for everybody, until somebody found a
    /// console.
    ///
    /// Run on a thread with a deadline, because a regression here does not fail
    /// a test — it hangs the suite, which is exactly what it does to the screen.
    #[test]
    fn a_pipe_under_an_accounts_name_cannot_hold_the_login_screen() {
        use std::sync::mpsc;
        use std::time::Duration;

        let root = scratch("squatted");
        let path = root.join("alex.toml");
        let name = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        // SAFETY: `name` is a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o644) }, 0);

        // SAFETY: `getuid` cannot fail and touches no memory this owns.
        let mine = unsafe { libc::getuid() };
        let (answer, answered) = mpsc::channel();
        let squatted = root.clone();
        std::thread::spawn(move || {
            // Both the owner it would refuse and the owner it would accept: the
            // refusal has to happen at the door, not after a wait.
            let _ = answer.send((
                published_in(&squatted, "alex", mine),
                published_in(&squatted, "alex", mine.wrapping_add(1)),
            ));
        });
        assert_eq!(
            answered.recv_timeout(Duration::from_secs(5)),
            Ok((None, None)),
            "the greeter waited on something that is not a file"
        );

        // A directory under the name does the same job less patiently, and is
        // refused for the same reason: a name in this directory is not a file
        // until the descriptor says it is one.
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(published_in(&root, "alex", mine), None);

        fs::remove_dir_all(root).unwrap();
    }

    /// What the greeter's compositor is started with, from an account's
    /// settings.
    ///
    /// The point of the whole module: one of those two screens is the one HDR
    /// was turned on for, and it has to arrive at the login screen as HDR on
    /// that connector and nothing else's.
    #[test]
    fn a_look_becomes_the_configuration_its_compositor_comes_up_in() {
        let home = home_with(SHELL, None);
        let document = Look::read(&home, None)
            .compositor_config("/usr/bin/console-experience-desktop-manager", None)
            .unwrap();
        let config: toml::Table = document.parse().expect("a compositor config is TOML");

        assert_eq!(
            config["general"]["shell"].as_str(),
            Some("/usr/bin/console-experience-desktop-manager")
        );
        let outputs = config["output"].as_array().expect("output blocks");
        let named = |name: &str| {
            outputs
                .iter()
                .find(|output| output["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("no [[output]] for {name}"))
                .clone()
        };

        // The screen HDR was turned on for, at the brightness it was set to.
        let dp2 = named("TEST-OUT-1");
        assert_eq!(dp2["hdr"].as_bool(), Some(true));
        assert_eq!(dp2["hdr_sdr_brightness"].as_integer(), Some(250));

        // The screen beside it inherits the shell's top-level answer, which is
        // that it is not an HDR screen — the settings are per connector, and
        // arriving on the wrong one is the failure this is guarding.
        let hdmi = named("TEST-OUT-2");
        assert_eq!(hdmi["hdr"].as_bool(), Some(false));
        assert_eq!(hdmi["hdr_sdr_brightness"].as_integer(), Some(200));

        // And a connector this file has never heard of gets the same.
        let inherited = named("*");
        assert_eq!(inherited["hdr"].as_bool(), Some(false));
        assert!(
            inherited.get("mode").is_none() && inherited.get("position").is_none(),
            "the catch-all entry claimed a mode or a corner of the layout"
        );

        fs::remove_dir_all(home).unwrap();
    }

    /// One account must not be able to leave everybody else without a login
    /// screen.
    ///
    /// `enabled = false` is the one published setting that decides whether the
    /// screen exists rather than what it looks like, and the directory it
    /// arrives in is one every account writes into. A look that turns off every
    /// connector on the machine — published deliberately, or left behind by a
    /// desk that has since been rearranged — used to be carried into the
    /// greeter's own compositor word for word, and the compositor does what it
    /// is told: "output disabled by config, leaving it dark".
    ///
    /// So it is not carried. Everything else about those two displays still is,
    /// which is the other half of the test: this is one setting withheld, not
    /// the login screen giving up on an account's displays.
    #[test]
    fn a_published_look_cannot_turn_the_login_screens_displays_off() {
        // Every connector the account has settings for, turned off — which on
        // a machine with these two screens is every screen it has.
        let off = SHELL
            .replace(
                "[display.TEST-OUT-1]",
                "[display.TEST-OUT-1]\nenabled = false",
            )
            .replace(
                "[display.TEST-OUT-2]",
                "[display.TEST-OUT-2]\nenabled = false",
            );
        let look = Look::read(&home_with(&off, None), None);
        assert_eq!(
            look.display["TEST-OUT-1"].enabled,
            Some(false),
            "the setting is read and kept — this is about what is passed on"
        );

        let document = look
            .compositor_config("/usr/bin/console-experience-desktop-manager", None)
            .unwrap();
        assert!(
            !document.contains("enabled"),
            "the greeter's compositor was told to leave a connector dark:\n{document}"
        );

        let config: toml::Table = document.parse().expect("a compositor config is TOML");
        let outputs = config["output"].as_array().expect("output blocks");
        for name in ["TEST-OUT-1", "TEST-OUT-2"] {
            let output = outputs
                .iter()
                .find(|output| output["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("no [[output]] for {name}"));
            assert!(
                output.get("enabled").is_none(),
                "{name} still carries an enabled flag"
            );
        }
        // The rest of what those screens were set to is untouched.
        assert_eq!(
            outputs
                .iter()
                .find(|output| output["name"].as_str() == Some("TEST-OUT-1"))
                .and_then(|output| output["hdr"].as_bool()),
            Some(true)
        );
    }

    /// A day of the year with a long night on it, so a machine built in
    /// December and one built in June get the same answer out of the tests
    /// below. Counted from zero, as the solar equations count.
    const MIDWINTER: u16 = 355;

    /// One hour of one midwinter day, on a clock at UTC.
    fn at(hour: u8) -> Option<crate::clock::Now> {
        Some(crate::clock::Now {
            hour,
            minute: 0,
            weekday: 5,
            day: 22,
            yday: MIDWINTER,
            year: 2026,
            offset: 0,
        })
    }

    fn light(schedule: Option<&str>, from: Option<u8>, until: Option<u8>) -> DisplayLook {
        DisplayLook {
            night_light: Some(true),
            night_light_schedule: schedule.map(str::to_string),
            night_light_from: from,
            night_light_until: until,
            ..DisplayLook::default()
        }
    }

    /// The one setting that depends on what time it is.
    #[test]
    fn the_night_light_burns_on_the_schedule_the_account_keeps() {
        let nowhere = None;

        // All day is all day, clock or no clock.
        assert!(burning(&light(Some("all-day"), None, None), None, nowhere));
        assert!(burning(&light(None, None, None), at(12), nowhere));

        // An evening: on at ten, off at six, and the far end is exclusive.
        let evening = light(Some("hours"), Some(22), Some(6));
        assert!(burning(&evening, at(23), nowhere));
        assert!(burning(&evening, at(2), nowhere));
        assert!(burning(&evening, at(22), nowhere));
        assert!(!burning(&evening, at(6), nowhere));
        assert!(!burning(&evening, at(12), nowhere));
        // A daytime pair is read the same way round.
        let daytime = light(Some("hours"), Some(9), Some(17));
        assert!(burning(&daytime, at(9), nowhere));
        assert!(!burning(&daytime, at(17), nowhere));
        // Two hours the same are no hours at all.
        assert!(!burning(
            &light(Some("hours"), Some(7), Some(7)),
            at(7),
            nowhere
        ));

        // A machine whose local time cannot be read at all has no schedule
        // rather than an unsatisfied one, which is the shell's own reading:
        // the switch then means what it says.
        assert!(burning(&evening, None, nowhere));

        // And a light that is off is off, whatever the schedule says.
        assert!(!burning(
            &DisplayLook {
                night_light: Some(false),
                night_light_schedule: Some("all-day".to_string()),
                ..DisplayLook::default()
            },
            at(23),
            nowhere
        ));
    }

    /// The schedule this greeter used to answer by leaving the light off, and
    /// the one the shell's own settings page offers by default.
    ///
    /// It is the whole of the difference between a login screen that warms the
    /// same displays the session is about to warm and one that hands over a
    /// cold screen and lets the shell change it a second later, in front of
    /// somebody who asked for neither.
    #[test]
    fn the_night_light_follows_the_sun_where_the_machine_is() {
        let sunlit = light(Some("sunset-to-sunrise"), Some(21), Some(7));
        // A northern city this machine is not in, written down here rather
        // than read off it: at 56° north in the week before Christmas the sun
        // is down before four and not up again until nearly nine.
        let here = crate::sun::Location::exact(55.95, -3.19);
        assert!(burning(&sunlit, at(23), here), "midnight in December");
        assert!(burning(&sunlit, at(5), here), "before a winter sunrise");
        assert!(!burning(&sunlit, at(12), here), "noon");

        // Midsummer at the same latitude is the other way round, and the hours
        // the user typed are not consulted at all — which is what this pair is
        // for. Five in the morning is dark in December and broad daylight in
        // June, and `21..7` calls both of them dark.
        let midsummer = crate::clock::Now {
            yday: 172,
            ..at(5).expect("a clock")
        };
        assert!(
            !burning(&sunlit, Some(midsummer), here),
            "five in the morning in June, which the typed hours would have called dark"
        );

        // A latitude where the sun does not come up is a day that is night,
        // and one where it does not go down is a day that is not.
        let arctic = crate::sun::Location::exact(78.22, 15.65);
        assert!(burning(&sunlit, at(12), arctic), "the polar night");
        let midnight_sun = crate::clock::Now {
            yday: 172,
            ..at(0).expect("a clock")
        };
        assert!(!burning(&sunlit, Some(midnight_sun), arctic));

        // And a machine that cannot say where it is burns rather than staying
        // cold: the shell puts a file like this back to All day on the way in,
        // and All day is what the switch above it says.
        assert!(burning(&sunlit, at(12), None));
    }

    /// Where the sun is worked out for, and in which order the two answers are
    /// consulted.
    #[test]
    fn a_settings_file_that_names_a_place_outranks_the_time_zone() {
        let named = Look {
            night_light_latitude: Some(-33.87),
            night_light_longitude: Some(151.21),
            ..Look::default()
        };
        assert_eq!(
            named.here(),
            crate::sun::Location::exact(-33.87, 151.21),
            "the coordinates the file named were not used"
        );

        // Half a coordinate is not a place, and neither is one off the earth.
        // Both fall back to the zone table, which is where a file that named
        // nothing was already getting its answer.
        let zone = Look::default().here();
        for (latitude, longitude) in [
            (Some(51.5), None),
            (None, Some(-0.12)),
            (Some(91.0), Some(0.0)),
            (Some(0.0), Some(200.0)),
            (Some(f64::NAN), Some(0.0)),
        ] {
            let look = Look {
                night_light_latitude: latitude,
                night_light_longitude: longitude,
                ..Look::default()
            }
            .sane();
            assert_eq!(look.night_light_latitude, None, "{latitude:?}");
            assert_eq!(look.night_light_longitude, None, "{longitude:?}");
            assert_eq!(look.here(), zone);
        }
    }

    /// A published look says which town its owner is in, and not which street.
    ///
    /// The file is world-readable, because a greeter that runs as nobody in
    /// particular has to be able to read it, and every account on the machine
    /// can therefore read every other account's. These two numbers are the only
    /// thing in it that is about a person rather than about a desktop. A tenth
    /// of a degree is about eleven kilometres and moves sunset by under a
    /// minute, which is nothing to a login screen and a great deal to whoever
    /// the coordinates belong to.
    #[test]
    fn a_published_place_is_a_town_rather_than_an_address() {
        let exact = Look {
            night_light_latitude: Some(52.237_049),
            night_light_longitude: Some(21.017_532),
            ..Look::default()
        }
        .sane();
        assert_eq!(exact.night_light_latitude, Some(52.2));
        assert_eq!(exact.night_light_longitude, Some(21.0));

        // And it still names a place, so the sun is still worked out for
        // somewhere rather than falling back to the zone.
        assert_eq!(exact.here(), crate::sun::Location::exact(52.2, 21.0));
        assert_ne!(exact.here(), Look::default().here());

        // The rounding cannot put a coordinate off the earth on its way past
        // the check that it is on it.
        let edge = Look {
            night_light_latitude: Some(-89.98),
            night_light_longitude: Some(179.97),
            ..Look::default()
        }
        .sane();
        assert_eq!(edge.night_light_latitude, Some(-90.0));
        assert_eq!(edge.night_light_longitude, Some(180.0));
        assert!(edge.here().is_some());
    }

    /// The two things a published file names that have a shape rather than a
    /// range, checked against the shapes themselves.
    ///
    /// This is the one place real connector names belong, and the reason the
    /// fixtures above do not use any: what is being checked here is the naming
    /// scheme every DRM driver uses, so the list is spread across the
    /// interfaces rather than being one desk's two monitors.
    #[test]
    fn modes_and_connector_names_are_the_shapes_a_compositor_uses() {
        for mode in [
            "2560x1440@144",
            "1920x1080",
            "preferred",
            "1280X1024@59.951",
        ] {
            assert!(is_mode(mode), "{mode:?}");
        }
        for mode in [
            "",
            "x",
            "0x0",
            "1920x",
            "99999x1080",
            "1920x1080@0",
            "1920x1080@x",
        ] {
            assert!(!is_mode(mode), "{mode:?}");
        }
        for name in [
            "DP-1",
            "HDMI-A-2",
            "eDP-1",
            "DVI-I-1",
            "DSI-1",
            "LVDS-1",
            "VGA-1",
            "Virtual-1",
            "*",
        ] {
            assert!(is_connector_name(name), "{name:?}");
        }
        for name in ["", "a b", "a/b", "[weird]", &"x".repeat(65)] {
            assert!(!is_connector_name(name), "{name:?}");
        }
    }

    /// A setting changed inside a session reaches the login screen without
    /// anybody signing in first.
    ///
    /// The failure this is about is a quiet one: the accent is changed to red
    /// at lunchtime, the user signs out in the evening, and the login screen
    /// that comes up is the purple it was that morning — a copy that was
    /// correct once and has been wrong ever since.
    ///
    /// What this exercises is the second half of that: the shell rewrites its
    /// settings and immediately says so, and what is published is what the file
    /// says *now*. The saying-so is the shell's, at the end of the same
    /// function that writes the file, so that there is no interval between the
    /// two for a logout to land in.
    #[test]
    fn a_setting_changed_in_the_session_reaches_the_published_copy() {
        let root = scratch("published-again");
        let home = home_with("accent = \"Purple\"\n", None);
        let publisher = Publisher {
            name: "alex".to_string(),
            settings: home.join(".config/lxb"),
            home: home.clone(),
            config_home: None,
        };
        // SAFETY: `getuid` cannot fail and touches no memory this owns.
        let mine = unsafe { libc::getuid() };
        let publish = || publish_in(&root, &publisher.name, &publisher.look(None)).unwrap();
        let published = || published_in(&root, "alex", mine).expect("a published copy");

        publish();
        assert_eq!(published().accent(), Some("Purple"));

        // As the shell writes it: a new file renamed over the old one, and then
        // the word that it has been written.
        let settings = home.join(".config/lxb/shell.toml");
        let temporary = settings.with_extension("toml.new");
        fs::write(
            &temporary,
            "accent = \"Red\"\n[display.TEST-OUT-1]\nhdr = true\n",
        )
        .unwrap();
        fs::rename(&temporary, &settings).unwrap();
        publish();
        assert_eq!(published().accent(), Some("Red"));
        assert_eq!(
            published().display["TEST-OUT-1"].hdr,
            Some(true),
            "the displays that changed with it did not follow"
        );

        // And again, with the settings put back: publishing is the whole
        // state every time rather than a change to be applied to the last one,
        // so nothing of the old settings can survive into the new copy.
        fs::write(&settings, "accent = \"Green\"\n").unwrap();
        publish();
        assert_eq!(published().accent(), Some("Green"));
        assert!(
            published().display.is_empty(),
            "a display that is no longer configured stayed in the published copy"
        );

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    /// The sound output is the one thing here that survives a publish which
    /// could not work it out.
    ///
    /// Everything else is the whole state every time — a display no longer
    /// configured has to *leave* the copy — and this is the exception, because
    /// of when the first publish of a session happens. `cedm-session` runs
    /// before it execs the session, which is before that account's sound server
    /// exists, so `pactl` has nothing to answer and never will have. Writing
    /// "no sound output" over what the last session left would erase, at every
    /// single login, the one fact the login screen cannot work out for itself.
    #[test]
    fn the_speakers_survive_a_publish_that_could_not_ask_for_them() {
        let sounded = Look {
            sound_card: Some("TESTCARD".to_string()),
            sound_gain: Some(0.1),
            ..Look::default()
        };

        // A publish from before the sound server is up keeps what is known.
        let mut fresh = Look {
            accent: Some("Green".to_string()),
            ..Look::default()
        };
        carry_sound_forward(&mut fresh, Some(&sounded));
        assert_eq!(fresh.sound_card.as_deref(), Some("TESTCARD"));
        assert_eq!(fresh.sound_gain, Some(0.1));

        // A publish that *could* ask never reaches this at all, so a session
        // that moved to another card is not held to the old one. Asserted here
        // as the shape of it: something already answered is left alone.
        let mut moved = Look {
            sound_card: Some("TESTCARDTWO".to_string()),
            sound_gain: Some(1.0),
            ..Look::default()
        };
        carry_sound_forward(&mut moved, Some(&sounded));
        assert_eq!(moved.sound_card.as_deref(), Some("TESTCARDTWO"));
        assert_eq!(moved.sound_gain, Some(1.0));

        // And the first login of a machine has nothing to carry.
        let mut first = Look::default();
        carry_sound_forward(&mut first, None);
        assert_eq!(first.sound_card, None);
    }

    /// What arrives in these two keys was written by an account and is about to
    /// be logged and matched against this machine's own device list.
    #[test]
    fn a_published_card_and_gain_are_brought_inside_their_bounds() {
        let sane = |card: &str, gain: f32| {
            Look {
                sound_card: Some(card.to_string()),
                sound_gain: Some(gain),
                ..Look::default()
            }
            .sane()
        };
        let kept = sane("TESTCARD_1", 0.5);
        assert_eq!(kept.sound_card.as_deref(), Some("TESTCARD_1"));
        assert_eq!(kept.sound_gain, Some(0.5));

        for card in ["", "a card with spaces", "rm -rf /", &"x".repeat(33)] {
            assert_eq!(sane(card, 0.5).sound_card, None, "{card:?}");
        }
        // A multiplier, so anything that is not one is not a level. Dropped
        // rather than clamped: a login screen with no gain published plays at
        // the card's own, which beats a number invented here.
        for gain in [-0.1, 1.5, f32::NAN, f32::INFINITY] {
            assert_eq!(sane("TESTCARD", gain).sound_gain, None, "{gain}");
        }
        // Silence is a level somebody chose, and is carried as one.
        assert_eq!(sane("TESTCARD", 0.0).sound_gain, Some(0.0));
    }

    /// The whole of what `--publish-look` does, on whatever machine this is
    /// built on: work out which account is running, read its settings, and
    /// leave the file under its own name.
    ///
    /// Both outcomes are real. A machine whose build account has LineXinBar
    /// settings publishes them; one that has never run the shell has nothing
    /// to publish and says so, which is what a first login gives and is not a
    /// failure of this code.
    #[test]
    fn an_account_publishes_the_settings_it_has() {
        let root = scratch("published-account");
        // SAFETY: `getuid` cannot fail and touches no memory this owns.
        let uid = unsafe { libc::getuid() };
        let Some(user) = crate::users::discover()
            .into_iter()
            .find(|user| user.uid == uid)
        else {
            // Built as root, or as an account /etc/passwd does not offer.
            fs::remove_dir_all(root).unwrap();
            return;
        };
        if let Ok((look, path)) = publish_for_current_account(&root) {
            assert_eq!(path, root.join(format!("{}.toml", user.name)));
            assert_eq!(published_in(&root, &user.name, uid), Some(look));
        }
        fs::remove_dir_all(root).unwrap();
    }
}
