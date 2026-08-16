//! What the machine is playing through, asked from inside a session that can
//! see it.
//!
//! The login screen has to come out of the same speakers as the desktop, at the
//! same volume, and it cannot work either of those out for itself. It runs as
//! an account of its own with no sound server of its own: no default sink, no
//! stored volume, and an ALSA `default` that is a plugin waiting to connect to
//! a server that is not there. Left to guess it guesses badly — on the desk this
//! was written at it opened the third socket of a graphics card, with nothing
//! plugged into it, and reported that its audio was ready.
//!
//! So it is not guessed. This runs where the answer exists — inside the user's
//! own session, from `cedm --publish-look`, as the account itself — and what it
//! finds is published for the login screen exactly as the accent and the display
//! settings are. See [`crate::look`].
//!
//! Two things are worth carrying, and only two:
//!
//! - **which card**, as ALSA's own id for it, because that is the one name a
//!   greeter with no sound server can still use. Not the sink's name: that one
//!   belongs to PipeWire, and PipeWire is exactly what will not be running.
//! - **how loud**, as a plain multiplier. The session's volume is applied by the
//!   sound server, in software, on top of whatever the card's own mixer is set
//!   to. A login screen that opened the card directly and played at full scale
//!   would answer a button several times louder than the desktop it is about to
//!   hand over to — which, at a login screen at night, is worse than silence.
//!
//! Through `pactl`, because that is the interface every sound server on this
//! machine answers, and because the alternative is a PipeWire client library in
//! a program that needs one number twice a session. It is asked for JSON, so
//! nothing here is parsing a table meant for a person to read. A machine with no
//! `pactl`, or no server for it to reach, publishes nothing and leaves the
//! greeter to its own devices — see `crate::sound`, which still has to work on a
//! machine that has never run any of this.

use serde::Deserialize;
use std::process::Command;

/// The most of `pactl`'s answer that will ever be read.
///
/// A sink list is a few kilobytes. This is not about them; it is that a
/// subprocess writing without bound into a session's memory is not something to
/// leave open because today's output is small.
const MAX_BYTES: usize = 1024 * 1024;

/// Where the session plays, and how loud.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemOutput {
    /// ALSA's own id for the card the default sink is on, which is the `CARD=`
    /// in the device names a greeter has to choose between. Short and given by
    /// the driver rather than by the hardware's marketing.
    pub card: String,
    /// What the session multiplies its audio by, between 0 and 1. Muted is 0,
    /// deliberately: a machine somebody silenced is one whose login screen is
    /// silent too.
    pub gain: f32,
}

#[derive(Debug, Deserialize)]
struct Info {
    #[serde(default)]
    default_sink_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Sink {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mute: bool,
    #[serde(default)]
    volume: std::collections::BTreeMap<String, Channel>,
    #[serde(default)]
    properties: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Channel {
    #[serde(default)]
    db: Option<String>,
    #[serde(default)]
    value: Option<f64>,
}

/// Ask the session's sound server where it plays and how loud.
///
/// `None` is a machine with no `pactl`, no server, or no default sink — none of
/// which is a fault here. It is the ordinary state of a machine that does its
/// audio some other way, and the login screen has a fallback for exactly that.
pub fn read() -> Option<SystemOutput> {
    let info: Info = ask(&["--format=json", "info"])?;
    let wanted = info.default_sink_name?;
    let sinks: Vec<Sink> = ask(&["--format=json", "list", "sinks"])?;
    let sink = sinks
        .into_iter()
        .find(|sink| sink.name.as_deref() == Some(wanted.as_str()))?;
    let card = sink
        .properties
        .get("alsa.id")
        .and_then(serde_json::Value::as_str)?
        .to_string();
    if card.is_empty() {
        return None;
    }
    let gain = if sink.mute { 0.0 } else { gain_of(&sink) };
    Some(SystemOutput { card, gain })
}

/// Run `pactl` and read its JSON, bounded.
fn ask<T: serde::de::DeserializeOwned>(arguments: &[&str]) -> Option<T> {
    let output = Command::new("pactl").args(arguments).output().ok()?;
    if !output.status.success() {
        tracing::debug!(
            arguments = ?arguments,
            status = ?output.status.code(),
            "pactl would not answer"
        );
        return None;
    }
    if output.stdout.len() > MAX_BYTES {
        tracing::debug!("pactl answered with more than this will read");
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

/// The multiplier a sink applies, from whichever of its channels is loudest.
///
/// The loudest rather than an average, because what is being reproduced is how
/// loud the machine sounds, and a sink with one channel pulled down is not half
/// as loud — it is as loud as its louder side.
///
/// Decibels where the server gives them, because that is the actual gain and it
/// spares this having to know which curve the server applies to a percentage.
/// The raw value is the fallback, cubed, which is the curve PulseAudio's own
/// scale uses.
fn gain_of(sink: &Sink) -> f32 {
    sink.volume
        .values()
        .filter_map(|channel| {
            channel
                .db
                .as_deref()
                .and_then(decibels)
                .or_else(|| channel.value.map(|value| cubic(value / 65536.0)))
        })
        .fold(0.0_f32, f32::max)
        .clamp(0.0, 1.0)
}

/// A gain out of `pactl`'s decibels — `-20.00 dB` — or `-inf dB` for silence.
fn decibels(text: &str) -> Option<f32> {
    let number = text.trim().strip_suffix("dB")?.trim();
    if number.eq_ignore_ascii_case("-inf") {
        return Some(0.0);
    }
    let decibels: f32 = number.parse().ok()?;
    Some(10.0_f32.powf(decibels / 20.0))
}

fn cubic(fraction: f64) -> f32 {
    let fraction = fraction.clamp(0.0, 1.0) as f32;
    fraction * fraction * fraction
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape `pactl --format=json` answers in, cut to what is read.
    ///
    /// Invented hardware, deliberately. A fixture carrying the sinks of the
    /// machine this was written on reads as an assertion about that machine's
    /// speakers, and the levels in it would be a note of what somebody had
    /// their volume at one afternoon. The numbers here are chosen to be
    /// obvious instead: -20 dB is exactly a tenth, -6.02 dB is exactly a half.
    const SINKS: &str = r#"
    [
      {
        "name": "alsa_output.test-display-socket.hdmi-stereo",
        "mute": false,
        "volume": { "front-left": { "value": 32768, "value_percent": "50%", "db": "-6.02 dB" } },
        "properties": { "alsa.id": "TESTCARDTWO" }
      },
      {
        "name": "alsa_output.test-headset.analog-stereo",
        "mute": false,
        "volume": {
          "front-left": { "value": 29319, "value_percent": "45%", "db": "-20.00 dB" },
          "front-right": { "value": 29319, "value_percent": "45%", "db": "-20.00 dB" }
        },
        "properties": { "alsa.id": "TESTCARD" }
      }
    ]"#;

    fn sinks() -> Vec<Sink> {
        serde_json::from_str(SINKS).expect("the fixture is what pactl answers")
    }

    #[test]
    fn the_card_and_the_gain_come_off_the_default_sink() {
        let sink = sinks()
            .into_iter()
            .find(|sink| sink.name.as_deref() == Some("alsa_output.test-headset.analog-stereo"))
            .expect("the default sink is in the list");
        assert_eq!(
            sink.properties.get("alsa.id").and_then(|id| id.as_str()),
            Some("TESTCARD")
        );
        // The decibels are the gain. The percentage beside them is not: a
        // session at 45% is really playing at a tenth, and a login screen that
        // took the percentage for a multiplier would answer a button four and a
        // half times too loud.
        let gain = gain_of(&sink);
        assert!((gain - 0.1).abs() < 0.001, "{gain}");
    }

    /// The louder side decides, and a muted machine is a silent login screen.
    #[test]
    fn silence_is_carried_as_faithfully_as_a_level() {
        assert_eq!(decibels("-inf dB"), Some(0.0));
        assert_eq!(decibels("0.00 dB"), Some(1.0));
        assert_eq!(decibels("not a level"), None);

        let lopsided: Sink = serde_json::from_str(
            r#"{"name":"x","mute":false,"volume":{
                 "front-left":{"value":29319,"db":"-20.00 dB"},
                 "front-right":{"value":65536,"db":"0.00 dB"}},
               "properties":{"alsa.id":"TESTCARD"}}"#,
        )
        .unwrap();
        assert_eq!(gain_of(&lopsided), 1.0);

        // Without decibels, the raw value on PulseAudio's own cubic scale.
        let plain: Sink = serde_json::from_str(
            r#"{"name":"x","mute":false,"volume":{"mono":{"value":32768}},
                "properties":{"alsa.id":"TESTCARD"}}"#,
        )
        .unwrap();
        assert!((gain_of(&plain) - 0.125).abs() < 0.001);
    }
}
