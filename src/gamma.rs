//! The night light, on a login screen whose compositor is not LineXinBar's.
//!
//! # Why this exists at all
//!
//! [`crate::look`] hands the greeter's compositor the account's display
//! settings, night light included, and that is the whole answer where the
//! compositor is LineXinBar's own: it holds the DRM master, it owns the gamma
//! stage, and it warms the connector before there is a client to ask.
//!
//! It is not the compositor this package depends on. The one it depends on is
//! Cage, which is what a default installation runs, and Cage is a kiosk
//! compositor with no display settings of any kind — it is handed no
//! configuration and would have nowhere to put one. Under it the login screen
//! came up cold on a machine whose every screen warms the moment the session
//! starts, and the first thing a user sees at night was the one screen of the
//! day nobody had filtered.
//!
//! So the greeter warms them itself, as a client, which is a thing a client
//! can do: `zwlr_gamma_control_v1` is wlroots' protocol for exactly this and
//! is what every night-light program on this platform uses. Cage offers it
//! because wlroots offers it.
//!
//! # Which mechanism runs when
//!
//! Whichever the compositor allows, and never both. LineXinBar's compositor
//! does not implement this protocol — it has no reason to, since it warms its
//! displays from its own configuration and its own settings protocol — so
//! under it the manager global is simply absent and this does nothing at all.
//! Under Cage, or any other wlroots compositor somebody points
//! `CEDM_GREETER_COMPOSITOR` at, the global is there and this is what warms
//! the screen. There is no arrangement in which two things are setting one
//! ramp.
//!
//! # The same warmth, not a similar one
//!
//! The curve is `lxb-compositor`'s, transcribed: the same closed-form
//! Planckian fit, the same division through by the white point at 6500 K so
//! that neutral is exactly no filter, and the same decode-scale-encode over
//! the ramp rather than a multiply on the coded value. A login screen and the
//! session it hands to are four seconds apart on the same panel, and a filter
//! that agreed with the shell's in the midtones and parted from it in the
//! shadows would be visible as the handover happening.
//!
//! # A second connection
//!
//! winit owns the connection the window is on and does not lend out its
//! registry, so this opens its own to the same compositor. That is what a
//! separate night-light program would do, and the compositor cannot tell the
//! difference. It costs one socket and one thread that spends its life
//! blocked.
//!
//! The ramp lasts as long as the connection: wlroots puts a display back the
//! way it found it when the client that set the gamma goes away. That is the
//! right lifetime — the greeter exits at the end of a login and the session's
//! own compositor sets its own ramp a moment later — and it is why the
//! connection is held rather than closed after the request.

use crate::look::{burning, Look, NEUTRAL_KELVIN};
use std::io::Write;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_output, wl_registry};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::gamma_control::v1::client::{
    zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1,
    zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
};

/// The `wl_output` version that reports a connector name.
///
/// Version 4 added `wl_output.name`, and the name it gives under wlroots is
/// the connector's — `DP-1`, `HDMI-A-1`. Without it there is no way to tell
/// which of two screens a `[display.NAME]` section is about, so an output
/// bound below this version is left alone rather than warmed on a guess.
const OUTPUT_WITH_NAMES: u32 = 4;

/// The largest ramp this will write.
///
/// A gamma LUT is a few thousand entries — 4096 on the hardware this was
/// written against. This is far above any real one and is here for the same
/// reason every other bound in this program is: the size arrives from outside
/// the process, and it decides an allocation made before anybody has logged
/// in. The compositor's own answer is believed up to here and refused past it.
const MAX_RAMP: u32 = 1 << 16;

/// The warmest a ramp can encode, which is not the warmest the settings page
/// offers.
///
/// `lxb-compositor` clamps here rather than at the 2000 K the shell's own bar
/// stops at, because below roughly this the blue channel is already at zero
/// and there is nothing left to take away. Nothing published reaches it — a
/// look is bounded to the page's range on the way in — and it is the
/// compositor's number because the curve is the compositor's curve.
const WARMEST_ENCODED: u16 = 1000;

/// The night light, running for as long as this is held.
///
/// Dropping it closes the connection, which puts every display it warmed back
/// the way it was. Nothing does that deliberately; it exists so the borrow is
/// honest about what keeps the filter on screen.
pub struct NightLight {
    _worker: std::thread::JoinHandle<()>,
}

/// Warm the displays this look asks for, on a compositor that allows it.
///
/// Answers `None` and says nothing where there is no compositor to ask, no
/// gamma protocol on it — which is the ordinary case under LineXinBar's own
/// compositor — or nothing in the look to do. None of those is a failure: the
/// night light is the last thing on a login screen that should be able to stop
/// one happening.
pub fn start(look: &Look, now: Option<crate::clock::Now>) -> Option<NightLight> {
    let here = look.here();
    let wanted: Vec<(String, u16)> = look
        .display
        .iter()
        .filter(|(_, display)| burning(display, now, here))
        .map(|(name, display)| {
            (
                name.clone(),
                display.night_light_temperature.unwrap_or(NEUTRAL_KELVIN),
            )
        })
        // A display asked for daylight is a display asked for no filter, and
        // writing an identity ramp for it would only take a stage out of
        // whatever state the compositor had it in.
        .filter(|(_, kelvin)| *kelvin < NEUTRAL_KELVIN)
        .collect();
    if wanted.is_empty() {
        return None;
    }

    tracing::info!(
        displays = wanted.len(),
        "this account warms a display at this hour; asking the compositor"
    );
    let worker = std::thread::Builder::new()
        .name("cedm-night-light".to_string())
        .spawn(move || match run(wanted) {
            Ok(()) => {}
            // Said out loud rather than swallowed. Every way this ends is a
            // login screen that is not warm on a machine whose session will
            // be, and the difference between "this compositor cannot" and
            // "these are not the screens the account named" is the whole of
            // what somebody looking into it needs.
            Err(error) => tracing::info!(%error, "the login screen is not warming any display"),
        })
        .ok()?;
    Some(NightLight { _worker: worker })
}

/// Bind what the compositor offers, warm what the look named, and stay.
fn run(wanted: Vec<(String, u16)>) -> anyhow::Result<()> {
    let connection = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&connection)?;
    let handle = queue.handle();

    // The one global that decides whether any of this happens. Its absence is
    // the ordinary case under LineXinBar's compositor and is not an error
    // anybody has to see.
    let manager: ZwlrGammaControlManagerV1 = globals
        .bind(&handle, 1..=1, ())
        .map_err(|_| anyhow::anyhow!("this compositor has no zwlr_gamma_control_manager_v1"))?;

    let mut state = State {
        wanted,
        outputs: Vec::new(),
    };
    // Listed first and bound afterwards: the list cannot be walked while a
    // request is being made on the registry it belongs to.
    let mut advertised = Vec::new();
    globals.contents().with_list(|globals| {
        for global in globals {
            if global.interface == wl_output::WlOutput::interface().name
                && global.version >= OUTPUT_WITH_NAMES
            {
                advertised.push(global.name);
            }
        }
    });
    for name in advertised {
        let output: wl_output::WlOutput =
            globals
                .registry()
                .bind(name, OUTPUT_WITH_NAMES, &handle, state.outputs.len());
        state.outputs.push(Output {
            output,
            connector: None,
        });
    }
    if state.outputs.is_empty() {
        anyhow::bail!("this compositor names none of its outputs");
    }

    // Names first: which screen is which decides which of them is warmed.
    queue.roundtrip(&mut state)?;

    let mut asked = 0;
    for index in 0..state.outputs.len() {
        let Some(connector) = state.outputs[index].connector.clone() else {
            continue;
        };
        let Some((_, kelvin)) = state
            .wanted
            .iter()
            .find(|(name, _)| *name == connector)
            .cloned()
        else {
            continue;
        };
        manager.get_gamma_control(&state.outputs[index].output, &handle, (connector, kelvin));
        asked += 1;
    }
    if asked == 0 {
        // The names on both sides, because this is the failure that looks
        // exactly like the feature not existing. A published look names the
        // connectors the *session's* compositor gave it; a greeter running
        // under a different compositor may be shown different ones — a nested
        // one calls its output `WL-1` — and then nothing matches and nothing
        // is warmed, with nothing anywhere saying why.
        let seen: Vec<&str> = state
            .outputs
            .iter()
            .filter_map(|output| output.connector.as_deref())
            .collect();
        let named: Vec<&str> = state.wanted.iter().map(|(name, _)| name.as_str()).collect();
        anyhow::bail!(
            "this seat shows {seen:?} and the account warms {named:?}, which name no screen in common"
        );
    }

    // And then stay: the ramp lives as long as this connection does.
    loop {
        queue.blocking_dispatch(&mut state)?;
    }
}

struct Output {
    output: wl_output::WlOutput,
    /// The connector name, once the compositor has said it.
    connector: Option<String>,
}

struct State {
    /// Connector name and temperature, for the displays this account warms.
    wanted: Vec<(String, u16)>,
    outputs: Vec<Output>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrGammaControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrGammaControlManagerV1,
        _: <ZwlrGammaControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, usize> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            if let Some(output) = state.outputs.get_mut(*index) {
                output.connector = Some(name);
            }
        }
    }
}

impl Dispatch<ZwlrGammaControlV1, (String, u16)> for State {
    fn event(
        _: &mut Self,
        control: &ZwlrGammaControlV1,
        event: zwlr_gamma_control_v1::Event,
        (connector, kelvin): &(String, u16),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            // How many entries this display's ramp takes. It is the first
            // thing the compositor says and the only thing the curve needs.
            zwlr_gamma_control_v1::Event::GammaSize { size } => {
                if size == 0 || size > MAX_RAMP {
                    tracing::warn!(
                        connector,
                        size,
                        "ignoring a gamma ramp size no display could want"
                    );
                    return;
                }
                match ramp_file(size, *kelvin) {
                    Ok(file) => {
                        control.set_gamma(file.as_fd());
                        tracing::info!(connector, kelvin, size, "warmed this display");
                    }
                    Err(error) => {
                        tracing::warn!(connector, %error, "could not write a gamma ramp")
                    }
                }
            }
            // Two things arrive as one refusal: an output with no gamma stage
            // to set — a nested session's, which owns no CRTC — and one whose
            // gamma another client already holds. Neither is this greeter's to
            // argue with, and the display simply stays as it is.
            zwlr_gamma_control_v1::Event::Failed => tracing::info!(
                connector,
                "this compositor will not let the login screen warm this display"
            ),
            _ => {}
        }
    }
}

/// The ramp for one display, in a file the compositor can read.
///
/// `zwlr_gamma_control_v1.set_gamma` takes a descriptor holding three ramps of
/// `size` little-endian `u16`s, red then green then blue. A memory file is
/// what every implementation of this passes: it never touches a filesystem,
/// it is sized exactly, and it goes away with the descriptor.
fn ramp_file(size: u32, kelvin: u16) -> std::io::Result<OwnedFd> {
    let gains = gains(kelvin);
    let mut bytes = Vec::with_capacity(size as usize * 3 * 2);
    for gain in gains {
        for index in 0..size {
            // Decoded, scaled, encoded again — `lxb-compositor`'s own curve.
            // The gains are sRGB's coded values, so this is very nearly
            // `coded * gain`, but only very nearly, and doing it in light is
            // what makes this filter and the shell's the same white point
            // rather than two that agree in the midtones and part in the
            // shadows.
            let light = srgb_to_linear(index as f32 / (size - 1).max(1) as f32);
            let coded = linear_to_srgb(light * srgb_to_linear(gain));
            let level = (coded.clamp(0.0, 1.0) * u16::MAX as f32).round() as u16;
            bytes.extend_from_slice(&level.to_ne_bytes());
        }
    }

    // SAFETY: `memfd_create` takes a NUL-terminated name and a flag word, and
    // returns a descriptor this takes ownership of. Nothing else touches it.
    let raw = unsafe { libc::memfd_create(c"cedm-gamma".as_ptr(), libc::MFD_CLOEXEC) };
    if raw < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `raw` is a fresh descriptor this call owns, checked above.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let mut file = std::fs::File::from(fd.try_clone()?);
    file.write_all(&bytes)?;
    file.flush()?;
    Ok(fd)
}

/// The per-channel scale the picture is multiplied by, red first.
///
/// `lxb-compositor::hdr::NightLight::gains`, transcribed. In sRGB's *coded*
/// values, which is what every gamma-ramp night light on this platform has
/// always meant by a colour temperature. Red is always 1: warming is taking
/// blue and green away, never adding red, so nothing can clip and the display
/// never gets brighter than it was.
fn gains(kelvin: u16) -> [f32; 3] {
    let kelvin = kelvin.clamp(WARMEST_ENCODED, NEUTRAL_KELVIN);
    // Divided through by the white point at daylight so that the neutral
    // temperature comes out exactly [1, 1, 1]. Without it the fit leaves green
    // and blue a percent or two short at 6500 K, and "no filter" would be a
    // slightly warm picture that nothing could take back off.
    let neutral = planckian(NEUTRAL_KELVIN);
    let wanted = planckian(kelvin);
    [
        (wanted[0] / neutral[0]).clamp(0.0, 1.0),
        (wanted[1] / neutral[1]).clamp(0.0, 1.0),
        (wanted[2] / neutral[2]).clamp(0.0, 1.0),
    ]
}

/// The colour of a black body at `kelvin`, as sRGB values in 0..=1.
///
/// The closed-form approximation everything from desktop night lights to
/// photographic tools uses. Not normalised: [`gains`] does that, and has to,
/// because the fit does not quite reach white at daylight.
fn planckian(kelvin: u16) -> [f32; 3] {
    let temperature = kelvin as f32 / 100.0;
    let red = if temperature <= 66.0 {
        255.0
    } else {
        329.698_73 * (temperature - 60.0).powf(-0.133_204_76)
    };
    let green = if temperature <= 66.0 {
        99.470_8 * temperature.ln() - 161.119_57
    } else {
        288.122_16 * (temperature - 60.0).powf(-0.075_514_85)
    };
    let blue = if temperature >= 66.0 {
        255.0
    } else if temperature <= 19.0 {
        // Below roughly 1900 K a black body has no blue left to speak of, and
        // the logarithm below would run off to negative infinity rather than
        // saying so.
        0.0
    } else {
        138.517_73 * (temperature - 10.0).ln() - 305.044_8
    };
    [
        (red / 255.0).clamp(0.0, 1.0),
        (green / 255.0).clamp(0.0, 1.0),
        (blue / 255.0).clamp(0.0, 1.0),
    ]
}

fn srgb_to_linear(coded: f32) -> f32 {
    if coded <= 0.040_45 {
        coded / 12.92
    } else {
        ((coded + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(light: f32) -> f32 {
    if light <= 0.003_130_8 {
        light * 12.92
    } else {
        1.055 * light.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Neutral is exactly no filter, and that is the whole reason the fit is
    /// divided through by its own white point: a login screen set to daylight
    /// must show the picture the display shows with the light switched off, to
    /// the last code.
    #[test]
    fn daylight_is_the_picture_with_no_filter_on_it() {
        assert_eq!(gains(NEUTRAL_KELVIN), [1.0, 1.0, 1.0]);
        for size in [2_u32, 256, 1024, 4096] {
            let file = ramp_file(size, NEUTRAL_KELVIN).expect("a ramp");
            let bytes = read_back(&file, size);
            // The identity: entry n of a ramp of n+1 is n's share of full
            // scale, in all three channels.
            for channel in 0..3 {
                assert_eq!(bytes[channel * size as usize], 0);
                assert_eq!(bytes[(channel + 1) * size as usize - 1], u16::MAX);
            }
        }
    }

    /// Warming is taking blue and green away against red, and never the other
    /// way round. A ramp that raised a channel would be a display that got
    /// brighter when the filter came on.
    #[test]
    fn warming_takes_light_away_and_never_adds_it() {
        let mut last_blue = 1.0_f32;
        for kelvin in [6000_u16, 5000, 4000, 3600, 2700, 2000] {
            let [red, green, blue] = gains(kelvin);
            assert_eq!(red, 1.0, "{kelvin} K moved red");
            assert!(green <= 1.0 && blue <= 1.0, "{kelvin} K added light");
            assert!(blue < last_blue, "{kelvin} K is not warmer than the last");
            assert!(blue <= green, "{kelvin} K took more green than blue");
            last_blue = blue;
        }

        // And the ramp itself never rises above the identity anywhere.
        let size = 1024_u32;
        let file = ramp_file(size, 3600).expect("a ramp");
        let bytes = read_back(&file, size);
        for index in 0..size as usize {
            let identity = (index as f32 / (size - 1) as f32 * u16::MAX as f32).round() as u16;
            for channel in 0..3 {
                let level = bytes[channel * size as usize + index];
                assert!(
                    level <= identity.saturating_add(1),
                    "channel {channel} entry {index} is brighter than no filter"
                );
            }
        }
    }

    /// The curve is `lxb-compositor`'s, and this is the assertion that says
    /// so: the same decode, scale and encode, checked at the temperature this
    /// desk's own settings name and at both ends of the ramp.
    #[test]
    fn the_ramp_is_the_compositors_own_curve() {
        let size = 4096_u32;
        let kelvin = 3600;
        let file = ramp_file(size, kelvin).expect("a ramp");
        let bytes = read_back(&file, size);
        let gains = gains(kelvin);
        for index in [0_u32, 1, size / 2, size - 2, size - 1] {
            let light = srgb_to_linear(index as f32 / (size - 1) as f32);
            for (channel, gain) in gains.iter().enumerate() {
                let coded = linear_to_srgb(light * srgb_to_linear(*gain));
                let expected = (coded.clamp(0.0, 1.0) * u16::MAX as f32).round() as u16;
                assert_eq!(
                    bytes[channel * size as usize + index as usize],
                    expected,
                    "channel {channel} entry {index}"
                );
            }
        }
    }

    fn read_back(fd: &OwnedFd, size: u32) -> Vec<u16> {
        use std::io::{Read, Seek};
        let mut file = std::fs::File::from(fd.try_clone().expect("a second descriptor"));
        file.rewind().expect("a memory file rewinds");
        let mut raw = Vec::new();
        file.read_to_end(&mut raw).expect("the ramp reads back");
        assert_eq!(raw.len(), size as usize * 3 * 2);
        raw.as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_ne_bytes(*pair))
            .collect()
    }
}
