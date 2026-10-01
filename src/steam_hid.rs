//! A second-generation Steam Controller, and a Steam Deck's controls, read
//! from hidraw.
//!
//! ## The Deck
//!
//! The Deck's controls have a kernel driver, and it is no use to a login
//! screen: `hid-steam` leaves the firmware in its keyboard-and-mouse mode, and
//! its gamepad says nothing until ☰ is held for half a second. The compositor
//! drops that keyboard now — it told the shell a keyboard was in the user's
//! hands — so the Deck is read here from its report, as the puck is: 64 bytes
//! beginning `01 00 09`, at the offsets `hid-steam`'s own
//! `steam_deck_button_mappings` reads. Opening the node makes the kernel take
//! its gamepad away while this holds it, so nothing arrives twice.
//!
//! ## The Steam Controller 2
//!
//! The 2026 Steam Controller comes in four ways — on a cable (`28de:1302`),
//! over Bluetooth (`1303`), through its puck (`1304`), and through a Steam
//! Machine's own receiver (`1305`) — with the same report on every one: `0x42`,
//! 54 bytes, or over Bluetooth `0x45`, the same bytes less the last eight. It
//! is the Deck's case over again on a kernel that drives it, and has no kernel
//! gamepad at all on one that does not. `hid-steam` learned it in Linux 7.3 —
//! handheld kernels carry it earlier, and CachyOS's `deckify` 7.2.3 claims all
//! four — and before that claims only `1102`, `1142` and `1205`, the original
//! controller, its receiver and the Deck. There the controller falls through to
//! `hid-generic` and stays in the firmware's lizard mode: a mouse and a
//! keyboard, and no joystick node at all, which leaves [`crate::controller`]
//! with no pad to map. Where `hid-steam` has it, it is in lizard mode all the
//! same, with a gamepad that says nothing until Start is held and goes away
//! while this holds the raw node.
//!
//! ## Always the raw node
//!
//! LineXinBar's shell chooses between the raw node and the kernel's gamepad,
//! and leaves the pad to the kernel where `hid-steam` has it with its
//! `lizard_mode` off: that gamepad answers from the first press and carries
//! rumble and motion for the games it starts. The login screen starts no games
//! and wants none of that, and the raw node answers whatever driver has the pad
//! and whatever mode it is in, so here it is read on every machine. Holding it
//! takes the kernel's gamepad away only while the login screen is up.
//!
//! For a while only the Steam button was read here, because lizard mode is a
//! *real* USB keyboard and every other button reached the shell as a keystroke
//! through the compositor. That stopped being true the moment Steam was
//! launched: Steam claims the pad and writes lizard mode off, the keystrokes
//! stop, and the shell is left with one working button on a dead controller.
//!
//! So the whole report is decoded. It is the only source that survives both
//! states — captured on hardware with Steam running and with Steam closed, the
//! button bits are identical — and reading it costs nothing, because hidraw
//! reads are not exclusive and run happily alongside Steam's own.
//!
//! Opening hidraw read-only does not disturb lizard mode; leaving it means
//! *writing* feature reports, which is Steam's business and not ours. The
//! duplicate that read-only leaves behind — lizard mode still typing the same
//! buttons as keys — is settled in the compositor, which drops the pad's
//! keyboard so this is the only path in.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The Steam Controller 2's four ways in, as `hid-steam` names them in Linux
/// 7.3: a cable (Ibex), Bluetooth (Ibex BLE), its puck (Proteus) and a Steam
/// Machine's own receiver (Nereid).
const CONTROLLER_PRODUCTS: [u16; 4] = [0x1302, 0x1303, 0x1304, 0x1305];

/// The Deck's.
const DECK_PRODUCT: u16 = 0x1205;

/// Which of the two pads a raw node belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pad {
    /// The Steam Controller 2, by any of its four ways in.
    Puck,
    /// The Steam Deck's controls, `28de:1205`.
    Deck,
}

impl Pad {
    /// One report off this pad's node, or `None` for anything else the node
    /// says — housekeeping, or, on a Deck, the keyboard and mouse its firmware
    /// pretends to be.
    fn decode(self, report: &[u8]) -> Option<Reading> {
        match self {
            Pad::Puck => is_a_controller_report(report).then(|| Reading {
                held: decode_buttons(report, BUTTON_BITS),
                left: decode_stick(report, LEFT_STICK_X, LEFT_STICK_Y),
                right: decode_stick(report, RIGHT_STICK_X, RIGHT_STICK_Y),
            }),
            Pad::Deck => {
                (report.len() == DECK_REPORT_LEN && report[..3] == DECK_REPORT_HEAD).then(|| {
                    Reading {
                        held: decode_buttons(report, DECK_BUTTON_BITS),
                        left: decode_stick(report, DECK_LEFT_STICK_X, DECK_LEFT_STICK_Y),
                        right: decode_stick(report, DECK_RIGHT_STICK_X, DECK_RIGHT_STICK_Y),
                    }
                })
            }
        }
    }
}

/// One report's worth of what the login screen reads off a pad.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Reading {
    held: Buttons,
    /// Each stick as `(x, y)` in −1.0..=1.0 with y positive up, dead zone
    /// already applied.
    left: (f32, f32),
    right: (f32, f32),
}

/// The Deck's input report: 64 bytes, `01 00 09` at the front — a type of 1,
/// then `ID_CONTROLLER_DECK_STATE`, as `steam_raw_event` checks them.
const DECK_REPORT_LEN: usize = 64;
const DECK_REPORT_HEAD: [u8; 3] = [0x01, 0x00, 0x09];

/// Where each button is in the Deck's report: `hid-steam`'s
/// `steam_deck_button_mappings`, for the buttons the login screen has names
/// for.
const DECK_BUTTON_BITS: &[(usize, u8, Buttons)] = &[
    (8, 0x80, Buttons::A),
    (8, 0x20, Buttons::B),
    (8, 0x40, Buttons::X),
    (8, 0x10, Buttons::Y),
    (8, 0x08, Buttons::L1),
    (8, 0x04, Buttons::R1),
    (9, 0x01, Buttons::UP),
    (9, 0x02, Buttons::RIGHT),
    (9, 0x04, Buttons::LEFT),
    (9, 0x08, Buttons::DOWN),
    (9, 0x10, Buttons::VIEW),
    (9, 0x20, Buttons::STEAM),
    (9, 0x40, Buttons::MENU),
    (10, 0x40, Buttons::L3),
    (11, 0x04, Buttons::R3),
];

/// The Deck's sticks, as signed 16-bit little-endian pairs counting up —
/// `hid-steam`'s `steam_deck_axis_mappings`.
const DECK_LEFT_STICK_X: usize = 48;
const DECK_LEFT_STICK_Y: usize = 50;
const DECK_RIGHT_STICK_X: usize = 52;
const DECK_RIGHT_STICK_Y: usize = 54;

const _: () = assert!(DECK_RIGHT_STICK_Y + 2 <= DECK_REPORT_LEN);

/// Valve's vendor ID on its own, for anything that has to recognise one of
/// their pads without caring which. `/proc/bus/input/devices` spells the same
/// number `Vendor=28de`.
///
/// Here rather than beside the reader that needs it because this file is where
/// the project already knows what a Steam Controller is.
pub(crate) const VENDOR: u16 = 0x28de;

/// The pad's input report: `0x42` in the first byte, 54 bytes long.
const REPORT_ID: u8 = 0x42;
const REPORT_LEN: usize = 54;

/// And the one it sends over Bluetooth: `0x45`, 46 bytes — the same bytes less
/// the eight at the end, which carry nothing read here.
const SHORT_REPORT_ID: u8 = 0x45;
const SHORT_REPORT_LEN: usize = 46;

/// Whether a report off a Steam Controller 2's node is its input report, of
/// either length.
fn is_a_controller_report(report: &[u8]) -> bool {
    matches!(
        (report.first(), report.len()),
        (Some(&REPORT_ID), REPORT_LEN) | (Some(&SHORT_REPORT_ID), SHORT_REPORT_LEN)
    )
}

/// How often to look for a pad that was not there last time.
///
/// The dongle is a USB device the user can plug in mid-session, and four of its
/// five interfaces stay silent until a puck actually pairs to one. Rescanning
/// is a directory listing, so the interval only has to be short enough that
/// plugging a pad in feels like it worked.
const RESCAN_INTERVAL: Duration = Duration::from_secs(2);

/// One button, as a bit of its own.
///
/// A set rather than a struct of `bool`s because that is what the work is: two
/// pads are merged by OR-ing them, and the presses since the last report are
/// `now & !before`. Both are one instruction on a bitfield and a loop over
/// named fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Buttons(u16);

impl Buttons {
    pub const A: Self = Self(1 << 0);
    pub const B: Self = Self(1 << 1);
    pub const X: Self = Self(1 << 2);
    pub const Y: Self = Self(1 << 3);
    pub const UP: Self = Self(1 << 4);
    pub const DOWN: Self = Self(1 << 5);
    pub const LEFT: Self = Self(1 << 6);
    pub const RIGHT: Self = Self(1 << 7);
    pub const L1: Self = Self(1 << 8);
    pub const R1: Self = Self(1 << 9);
    pub const VIEW: Self = Self(1 << 10);
    pub const MENU: Self = Self(1 << 11);
    pub const STEAM: Self = Self(1 << 12);
    pub const L3: Self = Self(1 << 13);
    pub const R3: Self = Self(1 << 14);

    pub const fn empty() -> Self {
        Self(0)
    }

    /// Whether every button in `which` is down. Used with one button at a time.
    pub const fn has(self, which: Self) -> bool {
        self.0 & which.0 == which.0
    }

    #[allow(dead_code)]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Both sets at once — two pads merged, or the several buttons of a chord
    /// written down together.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// What is down here and was not down in `before`.
    const fn newly_down(self, before: Self) -> Self {
        Self(self.0 & !before.0)
    }
}

/// Where each button lives in the report: byte, mask, and what it is.
///
/// Captured on the hardware rather than taken from a document — three presses
/// of each control, twice over, once with Steam running and once without. Every
/// entry scored exactly three both times; the pad's gyro throws bits that score
/// three *once*, and disagreeing across the two runs is what rejected them.
///
/// `STEAM` is the one value that predates the capture, and the capture agreed
/// with it, which is the best evidence available that the rest are right too.
const BUTTON_BITS: &[(usize, u8, Buttons)] = &[
    (2, 0x01, Buttons::A),
    (2, 0x02, Buttons::B),
    (2, 0x04, Buttons::X),
    (2, 0x08, Buttons::Y),
    (2, 0x20, Buttons::R3),
    (2, 0x40, Buttons::MENU),
    (3, 0x02, Buttons::R1),
    (3, 0x04, Buttons::DOWN),
    (3, 0x08, Buttons::RIGHT),
    (3, 0x10, Buttons::LEFT),
    (3, 0x20, Buttons::UP),
    (3, 0x40, Buttons::VIEW),
    (3, 0x80, Buttons::L3),
    (4, 0x01, Buttons::STEAM),
    (4, 0x08, Buttons::L1),
];

/// The sticks, as signed 16-bit little-endian pairs.
const LEFT_STICK_X: usize = 10;
const LEFT_STICK_Y: usize = 12;
const RIGHT_STICK_X: usize = 14;
const RIGHT_STICK_Y: usize = 16;

/// Four separate pairs, in order, inside the report — checked while the crate
/// is compiled rather than while it is tested.
///
/// These are four numbers taken off a capture, and the way they go wrong is
/// somebody editing one of them: two axes that overlap read the same bytes, so
/// a stick pushed sideways moves diagonally, and a pair past the end of the
/// report panics on the first frame the pad sends. Neither is anything the
/// hardware could tell us — the offsets are constants, so the answer is known
/// before the program runs, and a build is a far better place to learn it than
/// a hand on a stick.
const _: () = {
    assert!(LEFT_STICK_Y >= LEFT_STICK_X + 2);
    assert!(RIGHT_STICK_X >= LEFT_STICK_Y + 2);
    assert!(RIGHT_STICK_Y >= RIGHT_STICK_X + 2);
    assert!(RIGHT_STICK_Y + 2 <= REPORT_LEN);
    // The Bluetooth report carries everything read here too.
    assert!(RIGHT_STICK_Y + 2 <= SHORT_REPORT_LEN);
};

/// Whether the report's vertical axes count upwards.
///
/// The shell's convention is positive-up, as every gamepad API normalises it,
/// and Valve's pads report the same way — `hid-steam` negates their Y to get
/// evdev's positive-down. If vertical navigation or the pointer ever comes out
/// inverted on this pad, this is the single line to flip; the D-pad is decoded
/// digitally and is unaffected either way.
const STICK_Y_IS_UP: bool = true;

/// How far off centre a stick has to sit before it is believed.
///
/// The sticks rest a little away from zero — measured around 2% of full travel
/// on this hardware, and it is not the same offset on each axis. Everything
/// inside this is reported as centred, which keeps a resting pad from slowly
/// walking the menu on its own.
const STICK_DEADZONE: f32 = 0.08;

/// One poll's worth of the pad.
#[derive(Debug, Clone, Copy, Default)]
pub struct Frame {
    /// What is held down now.
    pub held: Buttons,
    /// What went down since the last poll. Accumulated across every report
    /// drained, so a press and release inside one poll interval still counts.
    pub pressed: Buttons,
    /// What came back up since the last poll. Only the pointer's borrowed
    /// buttons need this — a menu row activated on the way back up would fire
    /// twice — but a click held down has to be let go of eventually.
    pub released: Buttons,
    /// The sticks, each `(x, y)` in −1.0..=1.0 with y positive up, dead zone
    /// already applied.
    pub left_stick: (f32, f32),
    pub right_stick: (f32, f32),
}

/// Reads every second-generation pad the machine has.
pub struct SteamPad {
    devices: Vec<Device>,
    next_scan: Option<Duration>,
    /// Whether the "found one" line has already been logged, so a pad that
    /// disconnects and comes back does not narrate itself every two seconds.
    announced: bool,
}

struct Device {
    path: PathBuf,
    /// Which pad this node is.
    pad: Pad,
    file: File,
    /// The buttons at the last report, so presses are reported as the edges
    /// they are rather than once per report for as long as one is held.
    held: Buttons,
    left_stick: (f32, f32),
    right_stick: (f32, f32),
    /// Whether this node has ever sent an input report. The dongle presents one
    /// interface per pad slot and four of them stay silent all session.
    speaking: bool,
}

impl SteamPad {
    /// Watches for pads, or does nothing at all when controller input is off.
    pub fn new(enabled: bool) -> Self {
        Self {
            devices: Vec::new(),
            // `None` means "scan on the first poll"; every later scan is
            // scheduled from the clock the caller passes in.
            next_scan: if enabled { None } else { Some(Duration::MAX) },
            announced: false,
        }
    }

    /// What the pad is doing, or `None` when there is no pad to ask.
    ///
    /// Drains every pending report: at 8 ms between polls a pad sending a
    /// report every 4 ms would otherwise fall steadily further behind, and the
    /// buttons would answer late by however long the session had been running.
    pub fn poll(&mut self, now: Duration) -> Option<Frame> {
        self.rescan_if_due(now);

        let mut frame = Frame::default();
        let mut heard = false;
        let mut lost = Vec::new();
        let mut buf = [0u8; 64];

        for (index, device) in self.devices.iter_mut().enumerate() {
            loop {
                match device.file.read(&mut buf) {
                    Ok(0) => break,
                    Ok(len) => {
                        // Reports that are not the input report are the pad's
                        // own housekeeping — on the puck, battery on `0x43`
                        // and `0x7b` every half second — or, on a Deck, the
                        // keyboard and mouse its firmware pretends to be.
                        let Some(reading) = device.pad.decode(&buf[..len]) else {
                            continue;
                        };
                        device.speaking = true;
                        let held = reading.held;
                        frame.pressed = frame.pressed.union(held.newly_down(device.held));
                        frame.released = frame.released.union(device.held.newly_down(held));
                        device.held = held;
                        device.left_stick = reading.left;
                        device.right_stick = reading.right;
                    }
                    Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                    Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                    Err(err) => {
                        // Unplugged, almost always. Drop it and let the next
                        // scan pick the pad up if it comes back.
                        tracing::debug!(
                            path = %device.path.display(),
                            %err,
                            "steam controller hidraw closed"
                        );
                        lost.push(index);
                        break;
                    }
                }
            }

            if !device.speaking {
                continue;
            }
            heard = true;
            // Two pads plugged in are merged rather than one being chosen, so
            // a session is drivable from either.
            frame.held = frame.held.union(device.held);
            frame.left_stick = further(frame.left_stick, device.left_stick);
            frame.right_stick = further(frame.right_stick, device.right_stick);
        }

        for index in lost.into_iter().rev() {
            self.devices.remove(index);
        }
        heard.then_some(frame)
    }

    fn rescan_if_due(&mut self, now: Duration) {
        match self.next_scan {
            // Disabled outright.
            Some(deadline) if deadline == Duration::MAX => return,
            Some(deadline) if now < deadline => return,
            _ => {}
        }
        self.next_scan = Some(now + RESCAN_INTERVAL);

        for (path, pad) in hidraw_nodes() {
            if self.devices.iter().any(|device| device.path == path) {
                continue;
            }
            match OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
            {
                Ok(file) => {
                    if !self.announced {
                        self.announced = true;
                        match pad {
                            Pad::Puck => tracing::info!(
                                "Steam Controller 2 found; reading it from hidraw, the one \
                                 source that answers whether or not the kernel drives it"
                            ),
                            Pad::Deck => tracing::info!(
                                "Steam Deck controls found; reading them from hidraw, the one \
                                 source that answers in the firmware's keyboard mode"
                            ),
                        }
                    }
                    tracing::debug!(path = %path.display(), ?pad, "steam controller hidraw opened");
                    self.devices.push(Device {
                        path,
                        pad,
                        file,
                        held: Buttons::empty(),
                        left_stick: (0.0, 0.0),
                        right_stick: (0.0, 0.0),
                        speaking: false,
                    });
                }
                Err(err) => {
                    // Losing this is not fatal on a machine with a keyboard,
                    // but it is the whole controller now rather than one
                    // button, and it is invisible without a word here.
                    tracing::warn!(
                        path = %path.display(),
                        %err,
                        "cannot read Steam Controller hidraw; the pad will not work"
                    );
                }
            }
        }
    }
}

/// Which of the buttons in `table` a report says are down.
fn decode_buttons(report: &[u8], table: &[(usize, u8, Buttons)]) -> Buttons {
    let mut buttons = Buttons::empty();
    for (byte, mask, button) in table {
        if report[*byte] & mask != 0 {
            buttons = buttons.union(*button);
        }
    }
    buttons
}

/// One stick, as `(x, y)` in −1.0..=1.0 with y positive up.
fn decode_stick(report: &[u8], x_at: usize, y_at: usize) -> (f32, f32) {
    let x = axis(report, x_at);
    let y = axis(report, y_at);
    let y = if STICK_Y_IS_UP { y } else { -y };
    (deadzone(x), deadzone(y))
}

/// One signed 16-bit little-endian axis, normalised.
fn axis(report: &[u8], at: usize) -> f32 {
    let raw = i16::from_le_bytes([report[at], report[at + 1]]);
    // i16::MIN would give a shade over 1.0 the other way, which nothing here
    // wants to have to think about.
    (raw as f32 / i16::MAX as f32).clamp(-1.0, 1.0)
}

fn deadzone(value: f32) -> f32 {
    if value.abs() < STICK_DEADZONE {
        0.0
    } else {
        value
    }
}

/// Whichever of two stick positions is further from rest, axis by axis.
fn further(current: (f32, f32), candidate: (f32, f32)) -> (f32, f32) {
    let pick = |a: f32, b: f32| if b.abs() > a.abs() { b } else { a };
    (pick(current.0, candidate.0), pick(current.1, candidate.1))
}

/// Every hidraw node belonging to a second-generation Steam Controller or a
/// Steam Deck, with which it is.
///
/// Matched on `HID_ID` from sysfs rather than on the device name, which is a
/// string the firmware picks, or on the node number, which is whatever order
/// the machine happened to enumerate its USB devices in. A Deck has three —
/// its firmware's keyboard, its mouse, and the controls — and only the one
/// that speaks the Deck's report is ever read as a pad.
fn hidraw_nodes() -> Vec<(PathBuf, Pad)> {
    let Ok(entries) = std::fs::read_dir("/sys/class/hidraw") else {
        return Vec::new();
    };
    let mut nodes: Vec<(PathBuf, Pad)> = entries
        .flatten()
        .filter_map(|entry| {
            let pad = pad_of(&entry.path())?;
            Some((Path::new("/dev").join(entry.file_name()), pad))
        })
        .filter(|(node, _)| node.exists())
        .collect();
    // The dongle presents one interface per pad slot, and `read_dir` is in no
    // particular order. Sorting only makes the logs reproducible.
    nodes.sort_by(|a, b| a.0.cmp(&b.0));
    nodes
}

/// Which pad a HID device's sysfs directory is, by its `HID_ID`.
fn pad_of(sysfs: &Path) -> Option<Pad> {
    let uevent = std::fs::read_to_string(sysfs.join("device/uevent")).ok()?;
    pad_by_hid_id(&uevent)
}

/// Which pad a uevent's `HID_ID` names, read as numbers — `BBBB:VVVVVVVV:
/// PPPPPPPP` in hexadecimal — so that the same pad on a cable and over
/// Bluetooth is one pad.
fn pad_by_hid_id(uevent: &str) -> Option<Pad> {
    let id = uevent.lines().find_map(|line| line.strip_prefix("HID_ID="))?;
    let mut parts = id.trim().split(':');
    let mut next = || u32::from_str_radix(parts.next()?, 16).ok();
    let (bus, vendor, product) = (next()?, next()?, next()?);
    // USB and Bluetooth, the only two these arrive on.
    if !matches!(bus, 0x0003 | 0x0005) || vendor != u32::from(VENDOR) {
        return None;
    }
    let product = u16::try_from(product).ok()?;
    if product == DECK_PRODUCT {
        Some(Pad::Deck)
    } else {
        CONTROLLER_PRODUCTS.contains(&product).then_some(Pad::Puck)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> [u8; REPORT_LEN] {
        let mut report = [0u8; REPORT_LEN];
        report[0] = REPORT_ID;
        report
    }

    /// The one number that predates the capture, spelled out so a careless
    /// edit to the table has to break a test that names the button.
    #[test]
    fn the_steam_button_is_byte_four_bit_zero() {
        let mut at_rest = report();
        assert!(
            !decode_buttons(&at_rest, BUTTON_BITS).has(Buttons::STEAM),
            "released"
        );

        at_rest[4] = 0x01;
        assert!(decode_buttons(&at_rest, BUTTON_BITS).has(Buttons::STEAM));

        // The bits either side of it are other buttons, and none of them is
        // the Steam button.
        let mut neighbours = report();
        neighbours[4] = 0xfe;
        assert!(!decode_buttons(&neighbours, BUTTON_BITS).has(Buttons::STEAM));
    }

    /// Every button, at the offset the hardware capture put it at. This is the
    /// map; if it is wrong the pad is wrong, so it is written out in full.
    #[test]
    fn every_button_decodes_from_the_bit_it_was_captured_at() {
        for (byte, mask, button) in BUTTON_BITS {
            let mut one = report();
            one[*byte] = *mask;
            let decoded = decode_buttons(&one, BUTTON_BITS);
            assert!(decoded.has(*button), "byte {byte} mask {mask:#04x}");
            // And nothing else came with it.
            assert_eq!(
                decoded, *button,
                "byte {byte} mask {mask:#04x} decoded extra"
            );
        }
    }

    #[test]
    fn a_resting_report_is_no_buttons_at_all() {
        assert!(decode_buttons(&report(), BUTTON_BITS).is_empty());
    }

    /// The D-pad's four bits are four *different* bits. They sit in one byte
    /// next to the shoulder and the small buttons, and a transposed pair there
    /// would make the menu walk sideways when asked to go down.
    #[test]
    fn the_dpad_directions_do_not_collide() {
        let mut seen = Vec::new();
        for direction in [Buttons::UP, Buttons::DOWN, Buttons::LEFT, Buttons::RIGHT] {
            let (byte, mask, _) = BUTTON_BITS
                .iter()
                .find(|(_, _, button)| *button == direction)
                .expect("every direction is mapped");
            assert!(!seen.contains(&(byte, mask)), "two directions on one bit");
            seen.push((byte, mask));
        }

        // And pressing one is exactly one direction.
        let mut down = report();
        down[3] = 0x04;
        let decoded = decode_buttons(&down, BUTTON_BITS);
        assert!(decoded.has(Buttons::DOWN));
        assert!(!decoded.has(Buttons::UP));
        assert!(!decoded.has(Buttons::LEFT));
        assert!(!decoded.has(Buttons::RIGHT));
    }

    /// Several buttons at once is the normal case — the keyboard chord is two
    /// of them — and a decode that only ever found the first would break it.
    #[test]
    fn buttons_combine() {
        let mut both = report();
        both[3] = 0x40; // View
        both[2] = 0x04; // X
        let decoded = decode_buttons(&both, BUTTON_BITS);
        assert!(decoded.has(Buttons::VIEW));
        assert!(decoded.has(Buttons::X));
    }

    #[test]
    fn presses_are_the_edge_and_not_the_hold() {
        let down = Buttons::A;
        assert_eq!(
            down.newly_down(Buttons::empty()),
            Buttons::A,
            "first report"
        );
        assert_eq!(down.newly_down(Buttons::A), Buttons::empty(), "still held");
        assert_eq!(
            Buttons::empty().newly_down(Buttons::A),
            Buttons::empty(),
            "letting go is not a press"
        );
    }

    #[test]
    fn a_centred_stick_reads_as_centred() {
        let rest = report();
        assert_eq!(decode_stick(&rest, LEFT_STICK_X, LEFT_STICK_Y), (0.0, 0.0));
    }

    /// The offsets the capture found: each axis reads its own two bytes and
    /// leaves the next axis alone.
    ///
    /// That the four fields do not overlap at all is not asserted here — it is
    /// arithmetic on constants, and it is checked where they are declared, on
    /// every build rather than on every test run.
    #[test]
    fn the_sticks_are_four_separate_little_endian_pairs() {
        for (x_at, y_at) in [(LEFT_STICK_X, LEFT_STICK_Y), (RIGHT_STICK_X, RIGHT_STICK_Y)] {
            let mut pushed = report();
            // Full deflection on x only.
            pushed[x_at..x_at + 2].copy_from_slice(&i16::MAX.to_le_bytes());
            let (x, y) = decode_stick(&pushed, x_at, y_at);
            assert!((x - 1.0).abs() < 1e-3, "x reached full travel: {x}");
            assert_eq!(y, 0.0, "the other axis did not move");
        }
    }

    #[test]
    fn the_vertical_axis_counts_upwards() {
        let mut pushed = report();
        pushed[LEFT_STICK_Y..LEFT_STICK_Y + 2].copy_from_slice(&i16::MAX.to_le_bytes());
        let (_, y) = decode_stick(&pushed, LEFT_STICK_X, LEFT_STICK_Y);
        assert!(y > 0.0, "a positive raw axis is up");
    }

    /// A resting stick sits a little off zero on this hardware, and without a
    /// dead zone that is a menu that walks on its own.
    #[test]
    fn a_stick_resting_off_centre_still_reads_as_centred() {
        let drift = (i16::MAX as f32 * 0.02) as i16;
        let mut resting = report();
        resting[LEFT_STICK_X..LEFT_STICK_X + 2].copy_from_slice(&drift.to_le_bytes());
        assert_eq!(decode_stick(&resting, LEFT_STICK_X, LEFT_STICK_Y).0, 0.0);

        // But a real push is not swallowed.
        let push = (i16::MAX as f32 * 0.5) as i16;
        let mut pushed = report();
        pushed[LEFT_STICK_X..LEFT_STICK_X + 2].copy_from_slice(&push.to_le_bytes());
        assert!(decode_stick(&pushed, LEFT_STICK_X, LEFT_STICK_Y).0 > 0.4);
    }

    #[test]
    fn the_report_is_the_one_the_pad_actually_sends() {
        // Measured off the hardware and matching the published layout: 54
        // bytes, `0x42` first. The battery and housekeeping reports the pad
        // also sends are shorter, which is what the length check rejects.
        assert_eq!(REPORT_LEN, 54);
        assert_eq!(REPORT_ID, 0x42);
        assert_ne!(REPORT_LEN, 13, "0x7b housekeeping");
        assert_ne!(REPORT_LEN, 15, "0x43 battery");
    }

    #[test]
    fn disabled_never_scans() {
        let mut pad = SteamPad::new(false);
        assert!(pad.poll(Duration::ZERO).is_none());
        assert!(pad.poll(Duration::from_secs(3600)).is_none());
        assert!(pad.devices.is_empty(), "nothing opened while disabled");
    }

    /// A node that has never sent a report is not a pad.
    ///
    /// This is what keeps the dongle's four silent interfaces — one per pad
    /// slot, and empty until a puck actually pairs to one — from being taken
    /// for a controller that is there.
    ///
    /// Built from `/dev/null` rather than by asking the machine what it has
    /// plugged in: the box this was written on has a puck on it, so a test
    /// meaning "nothing is there" would otherwise pass or fail depending on
    /// which machine ran it, and on whether a report happened to be waiting.
    #[test]
    fn a_node_that_has_never_spoken_is_not_a_pad() {
        let mut pad = SteamPad::new(false);
        pad.devices.push(Device {
            path: PathBuf::from("/dev/null"),
            pad: Pad::Puck,
            file: File::open("/dev/null").expect("/dev/null is always openable"),
            held: Buttons::empty(),
            left_stick: (0.0, 0.0),
            right_stick: (0.0, 0.0),
            speaking: false,
        });

        assert!(
            pad.poll(Duration::ZERO).is_none(),
            "a silent node is no pad"
        );
        // And still nothing later: silence is not a state that expires.
        assert!(pad.poll(Duration::from_secs(3600)).is_none());
    }

    #[test]
    fn two_pads_merge_rather_than_one_winning() {
        assert_eq!(Buttons::A.union(Buttons::B), Buttons(0b11));
        assert_eq!(further((0.2, 0.0), (-0.9, 0.1)), (-0.9, 0.1));
        assert_eq!(further((0.2, -0.5), (0.1, 0.4)), (0.2, -0.5));
    }

    /// Every way the Steam Controller 2 comes in, on a cable and over
    /// Bluetooth, and the Deck; never the 2015 controller and its receiver,
    /// whose gamepad is the kernel's on every kernel there is.
    #[test]
    fn a_pad_is_found_by_its_ids_on_either_bus() {
        let pad = |id: &str| pad_by_hid_id(&format!("DRIVER=hid-steam\nHID_ID={id}\n"));
        for product in ["1302", "1304", "1305"] {
            assert_eq!(pad(&format!("0003:000028DE:0000{product}")), Some(Pad::Puck));
        }
        assert_eq!(pad("0005:000028DE:00001303"), Some(Pad::Puck));
        assert_eq!(pad("0003:000028DE:00001205"), Some(Pad::Deck));
        for not_ours in ["0003:000028DE:00001102", "0003:000028DE:00001142", "0003:0000045E:00001304"] {
            assert_eq!(pad(not_ours), None, "{not_ours}");
        }
        assert_eq!(pad("0018:000028DE:00001304"), None);
        assert_eq!(pad_by_hid_id("DRIVER=hid-generic\n"), None);
    }

    /// Over Bluetooth the controller sends `0x45`, the long report less its
    /// last eight bytes, and it reads the same.
    #[test]
    fn the_bluetooth_report_reads_as_the_long_one() {
        let mut long = report();
        long[2] = 0x01;
        long[4] = 0x01;
        long[10..12].copy_from_slice(&20000i16.to_le_bytes());
        let mut short = [0u8; SHORT_REPORT_LEN];
        short.copy_from_slice(&long[..SHORT_REPORT_LEN]);
        short[0] = SHORT_REPORT_ID;
        let read = Pad::Puck.decode(&long).expect("the long report");
        assert_eq!(Pad::Puck.decode(&short), Some(read));
        let mut wrong = short;
        wrong[0] = REPORT_ID;
        assert_eq!(Pad::Puck.decode(&wrong), None);
    }

    /// A Deck report with only `(byte, mask)` set.
    fn deck_report(set: &[(usize, u8)]) -> [u8; DECK_REPORT_LEN] {
        let mut report = [0u8; DECK_REPORT_LEN];
        report[..3].copy_from_slice(&DECK_REPORT_HEAD);
        for (byte, mask) in set {
            report[*byte] |= mask;
        }
        report
    }

    /// Every button at the bit `hid-steam` reads it from on a Deck, and the
    /// ones a wrong guess would hurt most spelled out.
    #[test]
    fn every_deck_button_decodes_from_the_bit_the_kernel_reads_it_at() {
        for (byte, mask, button) in DECK_BUTTON_BITS {
            let reading = Pad::Deck.decode(&deck_report(&[(*byte, *mask)])).unwrap();
            assert_eq!(reading.held, *button, "byte {byte} mask {mask:#04x}");
        }
        let at = |byte, mask| {
            Pad::Deck
                .decode(&deck_report(&[(byte, mask)]))
                .unwrap()
                .held
        };
        assert_eq!(at(8, 0x80), Buttons::A);
        assert_eq!(at(8, 0x20), Buttons::B);
        assert_eq!(at(9, 0x20), Buttons::STEAM);
        assert_eq!(at(9, 0x40), Buttons::MENU);
    }

    /// The Deck's firmware keyboard and mouse have nodes of their own, and
    /// nothing they say is the Deck; nor is the puck's report read on a
    /// Deck's node, or the other way round.
    #[test]
    fn only_the_decks_own_report_is_read_as_the_deck() {
        assert!(Pad::Deck.decode(&deck_report(&[])).is_some());
        assert!(Pad::Deck.decode(&[0, 0, 0x28, 0, 0, 0, 0, 0]).is_none());
        let mut other = deck_report(&[]);
        other[2] = 0x01;
        assert!(Pad::Deck.decode(&other).is_none());
        assert!(Pad::Puck.decode(&deck_report(&[])).is_none());
    }

    /// The Deck's sticks where the kernel reads them, the vertical counting up.
    #[test]
    fn the_decks_sticks_are_where_the_kernel_reads_them() {
        let mut report = deck_report(&[]);
        report[DECK_LEFT_STICK_X..DECK_LEFT_STICK_X + 2].copy_from_slice(&i16::MAX.to_le_bytes());
        report[DECK_RIGHT_STICK_Y..DECK_RIGHT_STICK_Y + 2]
            .copy_from_slice(&(-i16::MAX).to_le_bytes());
        let reading = Pad::Deck.decode(&report).unwrap();
        assert_eq!(reading.left, (1.0, 0.0));
        assert_eq!(reading.right, (0.0, -1.0));
    }
}
