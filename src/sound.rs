//! The noise the login screen makes.
//!
//! A console greeter answers a button twice: the highlight moves, and it
//! clicks. The click is the half of the acknowledgement that survives the user
//! looking somewhere else — at the pad in their hands, at the keyboard they are
//! about to type a password on, at the room — and this is a screen people use
//! without looking at it more than any other, because the thing they are about
//! to do is type something they know by heart.
//!
//! Four recordings, copied from LineXinBar and shipped in this repository beside
//! the fonts and the glyphs, for the same reason those are: a login screen runs
//! before any desktop does, and there may be no theme of sounds on the machine
//! to borrow one from. They are that project's clips deliberately — see
//! `vendor/line-xinbar/ORIGIN.md` — so that signing in and using the shell that
//! follows are one instrument rather than two.
//!
//! Three of them answer a control the user pressed:
//!
//! - [`Sounds::moved`] is the highlight arriving somewhere new,
//! - [`Sounds::selected`] is a press this greeter acts on, and
//! - [`Sounds::key`] is a key of the on-screen keyboard going down, because the
//!   board is its own instrument.
//!
//! The fourth answers something that *happened*: [`Sounds::error`], a refused
//! password. That one is not a control at all — it arrives from PAM, whole
//! seconds after the press that asked for it — which is exactly why it is here.
//! The user has already looked away.
//!
//! # Only what a button did
//!
//! The three press sounds belong to **button control**: the pad, and the keys
//! that drive the column. A pointer gets none of them. That is not a detail of
//! taste, it is what the sounds are *for*: a click answers a control somebody
//! cannot see themselves operating, and a mouse is a control they are watching
//! the whole time — the highlight follows the pointer under their hand, and a
//! sweep across the column would be a stream of clicks answering a question
//! nobody asked. The refusal is the exception, and for the same reason it is a
//! sound at all: it answers no press of anybody's, so there is no route it
//! could belong to.
//!
//! Structurally rather than by convention: every one of these is spent from
//! `Application::apply_action` and the two keys that reach past it, which is
//! the button path, and `Application::click`/`Application::hover` — the whole
//! of the pointer path — never call into this module.
//!
//! # Silence is a working state
//!
//! A machine with no sound card, a greeter whose seat has not been granted the
//! sound devices, a clip that will not decode: all of them are a login screen
//! that works and makes no noise. Nothing here is allowed to be the reason
//! somebody cannot sign in, so every failure is logged once and then accepted.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rodio::buffer::SamplesBuffer;
use rodio::cpal::{self, traits::HostTrait, StreamError};
use rodio::{
    Decoder, DeviceSinkBuilder, DeviceSinkError, DeviceTrait, MixerDeviceSink, Sample, Source,
};

/// One of the recordings, and the one place their order is decided: the samples
/// and the last time each was started are held in arrays under these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Effect {
    Move,
    Select,
    Key,
    Error,
}

impl Effect {
    const ALL: [Effect; 4] = [Effect::Move, Effect::Select, Effect::Key, Effect::Error];

    fn recording(self) -> &'static [u8] {
        match self {
            Effect::Move => MOVE,
            Effect::Select => SELECT,
            Effect::Key => KEY,
            Effect::Error => ERROR,
        }
    }

    /// What that recording is called, for a warning somebody has to act on.
    fn name(self) -> &'static str {
        match self {
            Effect::Move => "press-guide.ogg",
            Effect::Select => "press-selected.ogg",
            Effect::Key => "keyboard-click.ogg",
            Effect::Error => "error.ogg",
        }
    }
}

/// The click the highlight makes arriving somewhere new.
///
/// LineXinBar spends this one on its Home Button guide rather than on the bar,
/// and it is the right one to borrow: the guide is that shell's overlay — a
/// short column of controls standing over everything else — which is the same
/// thing this whole login screen is.
const MOVE: &[u8] = include_bytes!("../assets/sounds/press-guide.ogg");

/// A press this greeter acts on: a profile taken, a session chosen out of the
/// menu, the conversation opened, an answer sent, the board raised, a screen
/// backed out of.
///
/// The counterpart of [`MOVE`]. Moving the highlight and pressing what it is on
/// are the two halves of using the column, and each half has to answer or the
/// other one is describing a control that might be dead.
const SELECT: &[u8] = include_bytes!("../assets/sounds/press-selected.ogg");

/// A key of the on-screen keyboard going down.
///
/// Its own sound rather than [`SELECT`], because the board is its own
/// instrument: walking across its keys is walking a column and clicks like one,
/// and putting a key *down* is the thing that only happens there. Every key,
/// including the ones that type nothing — Shift, and the key that puts the
/// board away — because a board with two silent keys on it is a board with two
/// keys the user will press twice.
const KEY: &[u8] = include_bytes!("../assets/sounds/keyboard-click.ogg");

/// A password refused.
///
/// The one recording here that answers something which happened rather than
/// something that was pressed, and the reason it exists is the interval: an
/// answer goes to PAM, PAM takes its time, and the refusal lands whole seconds
/// after the press that asked for it. By then the user is very often not
/// looking at the screen — they have typed a password they know by heart and
/// glanced away — and the message that comes up is the one thing on this screen
/// somebody must not miss, because it is the difference between typing it again
/// and waiting for a session that is never going to start.
///
/// Only a refusal, and only PAM's. A failure of the login *service* — greetd
/// gone, the socket, this greeter's own worker — puts the same red screen up
/// but is not the user having got something wrong, and answering the two the
/// same way would teach the sound to mean nothing.
const ERROR: &[u8] = include_bytes!("../assets/sounds/error.ogg");

/// The shortest gap between two copies of one clip.
///
/// Copies of one recording laid over each other add, and being identical they
/// add in phase: several of the same click started in the same instant is that
/// click many decibels louder. A held direction on a pad asks for exactly that
/// if anything ever delivers its repeats in a burst, and so does a key
/// repeating under a finger.
///
/// Below this the request is dropped rather than mixed, which is the honest
/// answer: two clicks a sixteenth of a second apart are not two things anybody
/// can hear separately, so nothing is lost and the greeter cannot be made loud
/// by being driven quickly. Longer than each of the three clicks and shorter
/// than the pad's own repeat interval, so a held direction is still a run of
/// them.
const RESTED: Duration = Duration::from_millis(60);

/// How long to leave the output alone after failing to open it.
///
/// The greeter is very often on screen before the machine's sound is up: both
/// come from the same boot, and this program draws its first frame in well
/// under a second. So a device that is not there at start-up is a device that
/// is probably coming, and the next sound tries again — but no faster than
/// this, because the other reason a device fails to open is that the machine
/// has no sound card at all, and that answer must not be paid for on every
/// press of the D-pad.
const RETRY_AFTER: Duration = Duration::from_secs(5);

/// What has been asked for since the greeter came up.
///
/// Counted because the rule about which button makes which noise — and which
/// makes none — is the part of this worth testing, and a test cannot listen: a
/// machine building this has no sound card as often as not, and one that has no
/// sound card is a machine where every one of these is correctly silent. So the
/// tally is of the *request*, taken before the device is consulted, which is
/// exactly the decision the caller is responsible for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Spent {
    pub moved: u32,
    pub selected: u32,
    pub key: u32,
    pub error: u32,
}

/// Where the login screen should be heard, as the last account published it.
///
/// Empty is a machine that has never signed anybody in through this greeter, or
/// one whose sound server could not be asked. Everything here then falls back to
/// working it out, which it can do badly — see [`tier`] — and that is the whole
/// reason this type exists.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Want {
    /// ALSA's id for the card the user's session plays through.
    pub card: Option<String>,
    /// The multiplier that session plays at, so the login screen is as loud as
    /// the desktop either side of it and no louder. See [`crate::audio`].
    pub gain: Option<f32>,
}

/// The login screen's sounds, and the output they go to.
pub struct Sounds {
    /// Whether this greeter is allowed to make a noise at all. An
    /// administrator's answer, taken once — see [`crate::config::Config`].
    allowed: bool,
    /// What the last account to sign in was playing through, and how loud.
    want: Want,
    /// The open output device. Nothing ever reads it — it is held because
    /// dropping it closes the device.
    device: Option<MixerDeviceSink>,
    /// Set by the output thread when its device disappears or its stream is
    /// invalidated. The next sound drops that dead stream and opens the
    /// machine's current default instead.
    device_failed: Arc<AtomicBool>,
    /// The clips, decoded once at start-up rather than on each press, in the
    /// order [`Effect`] lists them. Between them they are under a second of
    /// audio, and decoding Vorbis inside the input handler would put a codec on
    /// the path between a button and the frame that answers it.
    effects: [Option<SamplesBuffer>; Effect::ALL.len()],
    /// When each of them was last put on the output, so that a copy is never
    /// laid on top of one still sounding. See [`RESTED`].
    played_at: [Option<Instant>; Effect::ALL.len()],
    /// How many times each has been asked for. See [`Spent`].
    asked: [u32; Effect::ALL.len()],
    /// Whether the first clip of this run has been reported. See [`Sounds::play`].
    spoken: bool,
    /// The earliest another attempt at opening the device may be made.
    retry_at: Instant,
}

impl Sounds {
    /// Decode the clips and open the machine's output.
    ///
    /// `allowed` is the administrator's answer. A greeter told to be silent
    /// decodes nothing and opens nothing: on a machine where the login screen
    /// must not make a noise, it should also not be holding a sound device open
    /// for the length of its run.
    ///
    /// `want` is the last account's own — which card their desktop plays
    /// through and how loud — so that the login screen comes out of the same
    /// speakers at the same level as the session either side of it.
    pub fn new(allowed: bool, want: Want) -> Self {
        let mut sounds = Self {
            allowed,
            want,
            device: None,
            device_failed: Arc::new(AtomicBool::new(false)),
            effects: if allowed {
                Effect::ALL.map(|effect| decode(effect.name(), effect.recording()))
            } else {
                [const { None }; Effect::ALL.len()]
            },
            played_at: [None; Effect::ALL.len()],
            asked: [0; Effect::ALL.len()],
            spoken: false,
            retry_at: Instant::now(),
        };
        if allowed {
            // Eagerly, so the first click of the session is as prompt as every
            // one after it: opening a device takes long enough to hear.
            sounds.open();
        }
        sounds
    }

    /// The highlight has arrived somewhere new, because a button moved it.
    ///
    /// Only where it really moved. A direction pressed at the end of a row that
    /// does not wrap, or on a carousel holding one profile, has moved nothing,
    /// and a click for it would be the greeter reporting a step it did not
    /// take.
    pub fn moved(&mut self) {
        self.play(Effect::Move);
    }

    /// A press this greeter acted on.
    ///
    /// Every press that changed anything, including the ones that go backwards:
    /// leaving a prompt and putting the board away are things the user asked
    /// for and got. A press that changed nothing is silent — that is a control
    /// which did nothing, and saying so is more use than a click that means the
    /// button works.
    pub fn selected(&mut self) {
        self.play(Effect::Select);
    }

    /// A key of the on-screen keyboard going down. See [`KEY`].
    pub fn key(&mut self) {
        self.play(Effect::Key);
    }

    /// A password refused. See [`ERROR`].
    pub fn error(&mut self) {
        self.play(Effect::Error);
    }

    /// What has been asked for so far. See [`Spent`].
    pub fn spent(&self) -> Spent {
        Spent {
            moved: self.asked[Effect::Move as usize],
            selected: self.asked[Effect::Select as usize],
            key: self.asked[Effect::Key as usize],
            error: self.asked[Effect::Error as usize],
        }
    }

    /// Put one clip on the machine's output.
    fn play(&mut self, effect: Effect) {
        // Counted first and unconditionally: this is the caller saying which
        // button was pressed, and that is true of a machine with no sound card
        // and of a login screen an administrator has silenced.
        self.asked[effect as usize] = self.asked[effect as usize].saturating_add(1);
        if !self.allowed {
            return;
        }
        let now = Instant::now();
        if !rested(self.played_at[effect as usize], now) {
            return;
        }
        // Cloning is cheap: the samples are shared, and what is copied is a
        // cursor over them.
        let Some(clip) = self.effects[effect as usize].clone() else {
            return;
        };
        // CPAL reports a device disappearing on the audio thread. It cannot
        // safely rebuild the output there, so the callback leaves one bit for
        // this thread to consume on the next press. A fresh flag is installed
        // with every stream, which keeps a late callback from an old device
        // from tearing down its replacement.
        self.refresh_failed_output(now);
        if self.device.is_none() {
            self.open();
        }
        if let Some(device) = &self.device {
            // At the level the user's own session plays at, where that is
            // known. A plain multiplier rather than a curve applied here: the
            // number was worked out from what the sound server is really doing
            // — see [`crate::audio`] — and a second curve on top of it would
            // make this louder or quieter than the desktop it is imitating,
            // which is the whole thing it is trying not to be.
            //
            // Unknown is full scale, which is the old behaviour and the only
            // honest answer when nothing has been published: the alternative is
            // inventing a level for somebody.
            match self.want.gain {
                Some(gain) => device.mixer().add(clip.amplify(gain)),
                None => device.mixer().add(clip),
            }
            // Noted only once it is really on the output. A press that found no
            // device made no sound, and must not stand in the way of the next
            // one that finds a device to make it on.
            self.played_at[effect as usize] = Some(now);
            // Said once, for the first clip of the run. Between "a device
            // opened" and "somebody heard something" there were two links with
            // nothing to show for them — whether a press ever asked for a sound
            // at all, and whether the clip reached the mixer — and a login
            // screen that is silent for either reason looks identical from the
            // journal to one that is silent for neither. One line settles it;
            // the rest are at debug, because there is one per press.
            if !self.spoken {
                self.spoken = true;
                tracing::info!(
                    sound = effect.name(),
                    "the login screen made its first sound"
                );
            } else {
                tracing::debug!(sound = effect.name(), "sound");
            }
        }
    }

    /// Consume the audio thread's signal before using its mixer again.
    fn refresh_failed_output(&mut self, now: Instant) {
        if self.device_failed.swap(false, Ordering::AcqRel) {
            tracing::warn!("audio output was lost; reopening it");
            self.device = None;
            self.retry_at = now;
        }
    }

    /// Open the machine's output, unless an attempt failed too recently.
    fn open(&mut self) {
        if Instant::now() < self.retry_at {
            return;
        }
        let device_failed = Arc::new(AtomicBool::new(false));
        match open_output(&self.want, Arc::clone(&device_failed)) {
            Ok(output) => {
                // Named, and said with what was asked for beside it, because
                // "audio ready" on its own is the one log line that looks like
                // success and is compatible with hearing nothing: a machine
                // with several cards in it can perfectly well open a display
                // socket with no cable in it while somebody sits in front of it
                // wearing a headset. That is not a hypothetical — it is what
                // this did before any of it was published, and the journal
                // could not say so.
                tracing::info!(
                    device = %output.name,
                    wanted = self.want.card.as_deref().unwrap_or("nothing published"),
                    gain = self.want.gain.unwrap_or(1.0),
                    "login screen audio ready"
                );
                self.device_failed = device_failed;
                self.device = Some(output.sink);
            }
            Err(err) => {
                // Logged at every attempt rather than only the first: the
                // attempts are seconds apart at worst, and what stopped the
                // sound is the one thing somebody with a silent login screen
                // will go looking for. The likeliest answer on a greeter is
                // that its seat was never granted the sound devices, which is
                // logind's ACL and not this program's to arrange.
                tracing::warn!(%err, "no audio output; the login screen will be silent");
                self.retry_at = Instant::now() + RETRY_AFTER;
            }
        }
    }
}

/// Whether enough has passed since one clip last played for it to play again.
///
/// A clip that has never played has, so the first of anything is always heard.
fn rested(played_at: Option<Instant>, now: Instant) -> bool {
    played_at.is_none_or(|last| now.saturating_duration_since(last) >= RESTED)
}

/// An opened output, and what it is called.
///
/// The name is carried out of here because this is the only place that knows
/// it: rodio hands back a sink with no way to ask which device is under it, and
/// the answer is the first thing anybody debugging a silent login screen needs.
struct Output {
    sink: MixerDeviceSink,
    name: String,
}

/// How a device is named to ALSA — `hdmi:CARD=NAME,DEV=1` — and how it names
/// itself to a person, which is the card's own description and the monitor or
/// jack on the far end of it.
///
/// Both, because the first is what the rules below are written in and the
/// second is the only one worth logging or writing in a configuration file.
/// Both come off the one description, whose `driver` is the ALSA name on this
/// backend — `name()` says the same thing and is deprecated.
fn names(device: &cpal::Device) -> (String, String) {
    let Ok(description) = device.description() else {
        return (String::new(), "unnamed".to_string());
    };
    (
        description.driver().unwrap_or_default().to_string(),
        description.name().to_string(),
    )
}

/// How willingly the login screen would be heard on an ALSA device, lowest
/// first, or `None` for one it will not use at all.
///
/// cpal's list is ALSA's own name hints, and that is **not** a list of
/// speakers. Most of it is plugins — `jack`, `oss`, `pulse`, `pipewire`, three
/// rate converters, an upmixer — which are ways of *reaching* an output rather
/// than outputs, and several of them open perfectly well on a machine where
/// nothing is listening. Taking the first entry that opens is how a login
/// screen ends up playing into a resampler, or into a display socket with no
/// cable in it.
///
/// So: a real card, named the way ALSA names one, and preferring the aliases
/// that mean "this card's ordinary output" over the raw device and over one
/// particular multichannel arrangement of it. Within a tier the order is
/// ALSA's, which is card order — the same order the machine itself calls
/// default, and the same answer `aplay` with no `-D` would give.
fn tier(id: &str) -> Option<u8> {
    if !id.contains("CARD=") {
        return None;
    }
    if id.starts_with("sysdefault:") {
        return Some(0);
    }
    if id.starts_with("front:") {
        return Some(1);
    }
    // A socket on a graphics card, and only one with something plugged in.
    if id.starts_with("hdmi:") {
        return display_is_listening(id).then_some(2);
    }
    if id.starts_with("plughw:") {
        return Some(3);
    }
    // `hw:` wants an exact format and is a duplicate of `plughw:`;
    // `usbstream:` is a raw stream node rather than a playback device;
    // `surround*` and `iec958:` are particular arrangements of a card whose
    // plain output is already above. None of them is where a click belongs.
    None
}

/// Whether a display is plugged into an output that is a socket on a graphics
/// card.
///
/// One of these exists whether or not anything is attached to it, and a
/// graphics card commonly presents more of them than the machine has monitors.
/// Playing into an empty one is silence that looks exactly like success — which
/// is what this function exists to stop, and what it was written after: the
/// login screen opened a socket with no cable in it, logged that its audio was
/// ready, and was never going to be heard by anybody.
///
/// The kernel already knows. `/proc/asound/CARD/eld#codec.pin` is what the far
/// end said it can play, and `eld_valid` is whether it said anything at all.
/// The pin index is the `DEV` of the `hdmi:` alias — the aliases are numbered
/// across that card's HDMI devices in the same order.
///
/// Anything that cannot be worked out is treated as attached. An unreadable
/// proc file is not evidence that nobody is listening, and refusing a device
/// out of ignorance would be this same bug stood on its head.
fn display_is_listening(id: &str) -> bool {
    let Some((card, pin)) = card_and_device(id) else {
        return true;
    };
    let Ok(entries) = std::fs::read_dir(std::path::Path::new("/proc/asound").join(card)) else {
        return true;
    };
    let suffix = format!(".{pin}");
    for entry in entries.flatten() {
        let file = entry.file_name();
        let Some(file) = file.to_str() else { continue };
        if !file.starts_with("eld#") || !file.ends_with(&suffix) {
            continue;
        }
        let Ok(eld) = std::fs::read_to_string(entry.path()) else {
            return true;
        };
        return eld_valid(&eld);
    }
    true
}

/// The card name and device index out of an ALSA device name.
fn card_and_device(id: &str) -> Option<(&str, u32)> {
    let card = card_of(id)?;
    let device = id.split("DEV=").nth(1)?.parse().ok()?;
    Some((card, device))
}

/// Whether an ELD report says the far end answered.
fn eld_valid(eld: &str) -> bool {
    eld.lines().any(|line| {
        let mut fields = line.split_whitespace();
        fields.next() == Some("eld_valid") && fields.next() == Some("1")
    })
}

/// Whether an ALSA device name is on a particular card.
fn on_card(id: &str, card: &str) -> bool {
    card_of(id).is_some_and(|found| found.eq_ignore_ascii_case(card))
}

/// The card name out of an ALSA device name.
fn card_of(id: &str) -> Option<&str> {
    let card = id.split("CARD=").nth(1)?.split(',').next()?;
    (!card.is_empty()).then_some(card)
}

/// Open the machine's output with a callback that can tell [`Sounds`] it has
/// ceased to be usable.
///
/// Three answers, in the order they deserve to be asked:
///
/// - **the card the last account's own session plays through**, published by
///   that account as its session started. This is the answer, on any machine
///   where anybody has signed in: it is not a guess about which of five cards
///   somebody is listening to, it is what they were listening to an hour ago.
///   Which alias on that card is [`tier`]'s decision, and on a card with several
///   sockets that is where the empty ones get skipped.
/// - **the default device**, which is right whenever there is a sound server to
///   have one — a preview running on somebody's desktop, or a machine that keeps
///   its ALSA configuration by hand.
/// - **anything else that is a real output**, by [`tier`], for a machine that
///   has never signed anybody in through this greeter. Deliberately last: it is
///   the answer that was here before any of this was published, and it is the
///   one that opened a socket with nothing plugged into it.
fn open_output(want: &Want, device_failed: Arc<AtomicBool>) -> Result<Output, DeviceSinkError> {
    let callback = output_error_callback(device_failed);
    let host = cpal::default_host();
    let open = |device: cpal::Device, human: String| -> Option<Output> {
        DeviceSinkBuilder::from_device(device)
            .and_then(|builder| {
                builder
                    .with_error_callback(callback.clone())
                    .open_sink_or_fallback()
            })
            .ok()
            .map(|sink| Output { sink, name: human })
    };

    let mut candidates: Vec<(u8, String, String, cpal::Device)> = Vec::new();
    match host.output_devices() {
        Ok(devices) => {
            for device in devices {
                let (id, human) = names(&device);
                if let Some(tier) = tier(&id) {
                    candidates.push((tier, id, human, device));
                }
            }
        }
        Err(err) => tracing::error!(%err, "could not list the machine's audio outputs"),
    }
    // Stable: `sort_by_key` keeps ALSA's own order, which is card order, inside
    // each tier.
    candidates.sort_by_key(|(tier, ..)| *tier);

    if let Some(card) = want.card.as_deref() {
        let (wanted, rest): (Vec<_>, Vec<_>) = candidates
            .into_iter()
            .partition(|(_, id, _, _)| on_card(id, card));
        if wanted.is_empty() {
            // The card the last session played through is not on this machine:
            // a headset unplugged, a dock left at the office. Worth saying,
            // because what follows is the guess this exists to avoid.
            tracing::info!(card, "the card the last session used is not here");
        }
        for (_, id, human, device) in wanted {
            match open(device, human) {
                Some(output) => return Ok(output),
                None => tracing::debug!(device = %id, "the published card would not open"),
            }
        }
        candidates = rest;
    }

    if let Some(device) = host.default_output_device() {
        let (id, human) = names(&device);
        match open(device, human) {
            Some(output) => return Ok(output),
            None => tracing::debug!(device = %id, "the default output would not open"),
        }
    }

    for (_, id, human, device) in candidates {
        match open(device, human) {
            Some(output) => return Ok(output),
            None => tracing::debug!(device = %id, "this output would not open"),
        }
    }
    Err(DeviceSinkError::NoDevice)
}

/// The callback carried by one device stream.
///
/// Underruns are glitches the stream itself can survive. A vanished device or
/// invalid configuration cannot recover in place; those are the two errors
/// rodio documents as requiring the stream to be destroyed and rebuilt.
fn output_error_callback(
    device_failed: Arc<AtomicBool>,
) -> impl FnMut(StreamError) + Clone + Send + 'static {
    move |err| {
        let needs_reopen = matches!(
            err,
            StreamError::DeviceNotAvailable | StreamError::StreamInvalidated
        );
        if needs_reopen {
            device_failed.store(true, Ordering::Release);
        }
        tracing::error!(%err, needs_reopen, "audio output stream error");
    }
}

/// Decode one of the shipped clips into samples ready to play.
///
/// A failure here is a fault in the build rather than in the machine — the clip
/// is compiled into the binary — so it is reported and then left alone: there
/// is no later attempt that could go differently.
fn decode(name: &str, clip: &'static [u8]) -> Option<SamplesBuffer> {
    let decoder = match Decoder::new(Cursor::new(clip)) {
        Ok(decoder) => decoder,
        Err(err) => {
            tracing::warn!(sound = name, %err, "bundled sound will not decode");
            return None;
        }
    };
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    let samples: Vec<Sample> = decoder.collect();
    if samples.is_empty() {
        tracing::warn!(sound = name, "bundled sound decoded to nothing");
        return None;
    }
    Some(SamplesBuffer::new(channels, sample_rate, samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sound card no machine has, which is the point of it twice over.
    ///
    /// Nothing here may be a description of the desk this was written at — a
    /// fixture carrying a real card's id reads as an assertion about one
    /// machine's hardware, and the next person cannot tell which half of it is
    /// ALSA's naming and which half is somebody's headset. It also has to be a
    /// name `/proc/asound` will not find: [`display_is_listening`] reads that
    /// directory, so a fixture naming a card this host really has would pass or
    /// fail by what is plugged into the machine running the tests.
    const CARD: &str = "TESTCARD";

    /// A second one, for the tests that are about telling two cards apart.
    const OTHER: &str = "TESTCARDTWO";

    /// Every clip this greeter ships is a sound, whatever the machine running
    /// the tests can play. Nothing here opens a device: this asserts about the
    /// bytes in the binary, which is the half of it a build can break.
    #[test]
    fn the_bundled_clips_decode() {
        for effect in Effect::ALL {
            let name = effect.name();
            let clip = decode(name, effect.recording()).unwrap_or_else(|| panic!("{name} decodes"));
            assert!(
                clip.total_duration()
                    .is_some_and(|held| held.as_millis() > 0),
                "{name} has no samples in it, so it is a silent one"
            );
        }
    }

    /// The rule that keeps a control driven quickly from answering in one very
    /// loud click: several of the same clip asked for in the same instant are
    /// identical samples laid over each other, and identical samples add in
    /// phase.
    #[test]
    fn a_clip_is_never_laid_on_top_of_a_copy_of_itself() {
        let start = Instant::now();
        assert!(rested(None, start), "the first of anything is heard");
        assert!(!rested(Some(start), start), "the same instant is not twice");
        assert!(!rested(
            Some(start),
            start + RESTED - Duration::from_millis(1)
        ));
        assert!(rested(Some(start), start + RESTED));

        // Which holds for every click, because none of them lasts that long.
        // The refusal is left out: it is most of a second long, it answers PAM
        // rather than a button, and two of them that close together would mean
        // two whole attempts had been refused inside a sixteenth of a second.
        for effect in [Effect::Move, Effect::Select, Effect::Key] {
            let clip = decode(effect.name(), effect.recording()).expect("a click decodes");
            let held = clip.total_duration().expect("a click has a length");
            assert!(
                held <= RESTED,
                "{} is {held:?}, so two of them {RESTED:?} apart would overlap",
                effect.name()
            );
        }

        // And a held direction is still a run of clicks rather than one click:
        // the pad's repeats are further apart than this.
        assert!(RESTED < crate::controller::REPEAT_INTERVAL);
    }

    /// What ALSA's name hints actually contain, and which of it is a speaker.
    ///
    /// This list is the reason the login screen was silent. Most of it is
    /// plugins — ways of reaching an output rather than outputs — and several
    /// of them open perfectly well on a machine where nothing is listening, so
    /// walking it and taking the first thing that opened put a click into a
    /// resampler as readily as into a card.
    #[test]
    fn only_a_real_card_is_somewhere_to_be_heard() {
        for plugin in [
            "sysdefault",
            "default",
            "lavrate",
            "samplerate",
            "speexrate",
            "jack",
            "oss",
            "pipewire",
            "pulse",
            "upmix",
            "vdownmix",
            "",
        ] {
            assert_eq!(tier(plugin), None, "{plugin:?} is not a card");
        }

        // A card's own default and its plain stereo output come first, and in
        // that order: both mean "this card", and the first of them is the one
        // the card itself nominates.
        assert_eq!(tier(&format!("sysdefault:CARD={CARD}")), Some(0));
        assert_eq!(tier(&format!("front:CARD={CARD},DEV=0")), Some(1));
        assert_eq!(tier(&format!("plughw:CARD={CARD},DEV=0")), Some(3));
        // A display socket is usable but comes after both — and only because
        // the card below is one no machine has, so nothing is known against it.
        assert_eq!(tier(&format!("hdmi:CARD={CARD},DEV=0")), Some(2));

        // And the arrangements that are not simply "the output" are not used at
        // all: a raw device wanting an exact format, a particular multichannel
        // layout, a digital passthrough, a raw USB stream node.
        for particular in ["hw", "surround51", "iec958"] {
            let name = format!("{particular}:CARD={CARD},DEV=0");
            assert_eq!(tier(&name), None, "{name:?}");
        }
        assert_eq!(tier(&format!("usbstream:CARD={CARD}")), None);
    }

    /// A socket on a graphics card with nothing plugged into it.
    ///
    /// The whole of the bug this was reported as: the login screen opened a
    /// display socket with no cable in it, logged that its audio was ready, and
    /// played into it while somebody sat in front of the machine wearing a
    /// headset.
    #[test]
    fn an_empty_display_socket_is_not_somewhere_to_be_heard() {
        // What the kernel writes in `/proc/asound/CARD/eld#codec.pin`.
        assert!(eld_valid(
            "monitor_present\t1\neld_valid\t1\nmonitor_name\tA MONITOR"
        ));
        assert!(!eld_valid("monitor_present\t0\neld_valid\t0"));
        // Nothing at all is not a report that nobody is listening.
        assert!(!eld_valid(""));

        // A card no machine has, so this asks the shape of the name and never
        // this machine's own hardware.
        assert_eq!(
            card_and_device(&format!("hdmi:CARD={CARD},DEV=2")),
            Some((CARD, 2))
        );
        assert_eq!(card_of(&format!("front:CARD={CARD},DEV=0")), Some(CARD));
        assert_eq!(card_of("pulse"), None);
        assert_eq!(card_and_device(&format!("sysdefault:CARD={CARD}")), None);

        // A card that cannot be found cannot be checked, and is therefore
        // allowed: refusing a device out of ignorance is this same bug stood on
        // its head.
        assert!(display_is_listening(&format!("hdmi:CARD={CARD},DEV=0")));
        assert!(display_is_listening(&format!("front:CARD={CARD},DEV=0")));
    }

    /// The published card is matched however either side spells its case, and
    /// nothing else on the machine is mistaken for it.
    #[test]
    fn the_published_card_picks_out_its_own_devices() {
        assert!(on_card(&format!("sysdefault:CARD={CARD}"), CARD));
        assert!(on_card(
            &format!("front:CARD={CARD},DEV=0"),
            &CARD.to_lowercase()
        ));
        assert!(!on_card(&format!("hdmi:CARD={OTHER},DEV=1"), CARD));
        assert!(!on_card("pulse", CARD));
        // Not a prefix match. ALSA hands a second card of the same kind a name
        // built on the first one's — `NAME` and `NAME_1` — and those are two
        // cards, one of which may be a graphics card's sockets and the other a
        // headset.
        let sibling = format!("{CARD}_1");
        assert!(!on_card(&format!("hdmi:CARD={CARD},DEV=0"), &sibling));
        assert!(on_card(&format!("sysdefault:CARD={sibling}"), &sibling));
    }

    /// A greeter an administrator has silenced holds no sound device and
    /// decodes nothing, rather than deciding at each press.
    #[test]
    fn a_silenced_greeter_opens_no_output_at_all() {
        let mut sounds = Sounds::new(false, Want::default());
        assert!(sounds.device.is_none());
        assert!(sounds.effects.iter().all(Option::is_none));
        // And every route into it is inert, which is what makes it safe to
        // leave the calls where they are rather than guarding each one.
        for spend in [Sounds::moved, Sounds::selected, Sounds::key, Sounds::error] {
            spend(&mut sounds);
        }
        assert!(sounds.played_at.iter().all(Option::is_none));
        // The tally still counts the asking, which is what the rules above this
        // are tested through: what the caller decided is true whether or not
        // this machine could make a noise about it.
        assert_eq!(
            sounds.spent(),
            Spent {
                moved: 1,
                selected: 1,
                key: 1,
                error: 1
            }
        );
    }

    /// The whole path, on a real device: decode, open the machine's output, and
    /// put each of the four clips on it.
    ///
    /// Ignored by default because it makes a noise on whatever this machine is
    /// listening to, and because a build host with no sound card is not a
    /// failing build — silence is a working state everywhere else in this file
    /// and it would be perverse for the test suite to disagree.
    ///
    /// Run it deliberately, into somewhere that can be recorded:
    ///
    /// ```sh
    /// pactl load-module module-null-sink sink_name=cedmtest media.class=Audio/Sink
    /// printf 'pcm.!default { type pipewire playback_node "cedmtest" capture_node "-1" }\n' \
    ///     > "$scratch/.asoundrc"
    /// parec --device=cedmtest.monitor --latency-msec=20 --file-format=wav out.wav &
    /// HOME=$scratch cargo test --lib -- --ignored --nocapture sound::
    /// ```
    ///
    /// Four bursts, a second apart, in the order below. Never record off the
    /// default sink's monitor: whatever else this machine is playing is on it.
    #[test]
    #[ignore = "opens the machine's sound output and makes a noise on it"]
    fn every_clip_reaches_a_real_output() {
        // Said out loud, because "it opened something" and "the user can hear
        // it" are different claims and this is where they part company. Run
        // under the greeter's own conditions — no sound server to connect to —
        // this prints whichever card each of them settles on, which is the one
        // question a silent login screen turns on.
        let want = std::env::var("CEDM_TEST_SOUND_CARD")
            .ok()
            .filter(|card| !card.is_empty());
        for want in [
            Want::default(),
            Want {
                card: want,
                gain: None,
            },
        ] {
            let chosen = open_output(&want, Arc::new(AtomicBool::new(false)));
            println!(
                "wanted {:<12} opened: {}",
                want.card.as_deref().unwrap_or("(nothing)"),
                match &chosen {
                    Ok(output) => output.name.clone(),
                    Err(err) => format!("nothing ({err})"),
                }
            );
            drop(chosen);
        }

        let mut sounds = Sounds::new(true, Want::default());
        assert!(
            sounds.device.is_some(),
            "no output device; this test needs one to be about anything"
        );
        for (index, spend) in [
            Sounds::moved as fn(&mut Sounds),
            Sounds::selected,
            Sounds::key,
            Sounds::error,
        ]
        .into_iter()
        .enumerate()
        {
            spend(&mut sounds);
            assert!(
                sounds.played_at[index].is_some(),
                "{} never reached the output",
                Effect::ALL[index].name()
            );
            // Far enough apart to be told apart in a recording, and further
            // than the longest of them.
            std::thread::sleep(Duration::from_millis(1000));
        }
    }

    #[test]
    fn a_lost_or_invalidated_output_is_reopened_but_an_underrun_is_not() {
        let failed = Arc::new(AtomicBool::new(false));
        let mut callback = output_error_callback(Arc::clone(&failed));

        callback(StreamError::BufferUnderrun);
        assert!(!failed.load(Ordering::Acquire));

        callback(StreamError::DeviceNotAvailable);
        assert!(failed.swap(false, Ordering::AcqRel));

        callback(StreamError::StreamInvalidated);
        assert!(failed.load(Ordering::Acquire));
    }
}
