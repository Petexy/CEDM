//! Whether this machine already has a keyboard on it.
//!
//! The on-screen board exists because a console in a living room has a pad and
//! nothing else. On a desk it is in the way: it covers half the screen to offer
//! a worse version of the keys already under the user's hands, and it appears
//! at the exact moment they were about to start typing. So it is raised by
//! itself only where there is nothing else to type on — and offered, always, as
//! a button, because this file is a heuristic and the user is not.
//!
//! Read out of `/proc/bus/input/devices`, which is world-readable, needs no
//! device to be opened and no privilege the greeter would otherwise want. The
//! greeter takes its own input through Wayland and never touches evdev; this is
//! a question about the machine rather than about the seat.

use std::path::Path;

const DEVICES: &str = "/proc/bus/input/devices";

/// `EV_KEY`: it reports keys at all.
const EV_KEY: u32 = 1;
/// `EV_REL` and `EV_ABS`: it reports movement, so it is a pointer or a pad.
const EV_REL: u32 = 2;
const EV_ABS: u32 = 3;
/// `EV_LED`: it has lamps — Num, Caps, Scroll.
const EV_LED: u32 = 17;

/// The first key and the last of the run every keyboard has: `KEY_ESC` through
/// `KEY_D`, which spans Escape, the number row, `Q`–`P` and `A`–`D`.
const KEY_ESC: u32 = 1;
const KEY_D: u32 = 32;

/// Vendors whose devices are pads, whatever their descriptors claim.
///
/// A Steam Controller enumerates a full keyboard — that is lizard mode, the
/// firmware's own fallback, typing the same buttons as keys so a pad works in a
/// program that has never heard of one. Four of them appear with a base station
/// attached. Nothing about that makes the machine a machine with a keyboard on
/// it; it makes it the console this greeter was written for, and treating the
/// pad as a reason to withhold the on-screen board takes the board away from
/// exactly the room it exists for.
///
/// A named exception rather than a consequence of the tests below, and checked
/// before any of them, because those tests are heuristics and this is a fact:
/// Valve makes controllers, not keyboards. If a firmware update gives the puck
/// lock lamps or drops a key from its range, the tests change their answer and
/// this does not. The rest of the greeter already works this way — see
/// [`crate::steam_hid`], which reads the pad over hidraw precisely because its
/// keyboard is a duplicate to be dropped rather than an input to be believed.
const PAD_VENDORS: [u16; 1] = [crate::steam_hid::VENDOR];

/// Whether a keyboard someone could type a password on is plugged in.
///
/// False when the file cannot be read at all, which is the safe answer: it
/// means the on-screen board is offered where it might not have been needed,
/// rather than withheld from someone with no other way to sign in.
pub fn typing_keyboard() -> bool {
    keyboard_among(&std::fs::read_to_string(Path::new(DEVICES)).unwrap_or_default())
}

/// The same question asked of the file's contents, so it can be asked of a
/// recording of somebody else's machine.
fn keyboard_among(devices: &str) -> bool {
    devices.split("\n\n").any(is_typing_keyboard)
}

/// Whether one device record describes a keyboard.
///
/// A pad is not one, by name, before anything is measured — see
/// [`PAD_VENDORS`]. What follows is for everything else, and every one of the
/// three tests is here because something on the author's own desk fails it
/// while passing the others:
///
/// * The key range is udev's own `ID_INPUT_KEYBOARD` test — `KEY_ESC` through
///   `KEY_D` all present. It is what "you could type a password on this" means.
/// * Lamps. Num, Caps and Scroll Lock are a physical keyboard's own lights, and
///   a keyboard synthesised by a remote control or a power button does not have
///   them.
/// * And no relative or absolute axes. A gaming mouse carries a full key bitmap
///   *and* lamps, because its extra buttons are bound to keystrokes and its
///   lighting is an LED device — but it also reports movement, and a keyboard
///   does not.
fn is_typing_keyboard(record: &str) -> bool {
    if vendor(record).is_some_and(|vendor| PAD_VENDORS.contains(&vendor)) {
        return false;
    }
    let events = match bits(record, "B: EV=") {
        Some(events) => events,
        None => return false,
    };
    if !set(&events, EV_KEY) || !set(&events, EV_LED) {
        return false;
    }
    if set(&events, EV_REL) || set(&events, EV_ABS) {
        return false;
    }
    let keys = match bits(record, "B: KEY=") {
        Some(keys) => keys,
        None => return false,
    };
    (KEY_ESC..=KEY_D).all(|key| set(&keys, key))
}

/// Who made the device, off the `I:` line the kernel opens each record with.
fn vendor(record: &str) -> Option<u16> {
    let line = record
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("I: "))?;
    let field = line
        .split_whitespace()
        .find_map(|field| field.strip_prefix("Vendor="))?;
    u16::from_str_radix(field, 16).ok()
}

/// One `B: NAME=` bitmap as words, least significant first.
///
/// The kernel prints these most significant word first, in the width of a
/// `long` and without padding a word whose top bits happen to be zero — so the
/// words are read from the end, where the low bits reliably are, and a run that
/// stops short simply has no more bits to give.
fn bits(record: &str, field: &str) -> Option<Vec<u64>> {
    let line = record
        .lines()
        .find_map(|line| line.trim_start().strip_prefix(field))?;
    Some(
        line.split_whitespace()
            .rev()
            .map(|word| u64::from_str_radix(word, 16).unwrap_or(0))
            .collect(),
    )
}

fn set(words: &[u64], bit: u32) -> bool {
    let width = usize::BITS;
    words
        .get((bit / width) as usize)
        .is_some_and(|word| word >> (bit % width) & 1 == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records off the author's machine, verbatim. The comment above each is
    /// what makes it interesting, and every one of them is a device that a
    /// simpler test gets wrong.
    const WOOTING: &str = "\
I: Bus=0003 Vendor=31e3 Product=1230 Version=0111
N: Name=\"Wooting Wooting 60HE v2\"
H: Handlers=sysrq kbd event6 leds
B: PROP=0
B: EV=120013
B: KEY=1000000000007 ff980000000007ff febeffdfffefffff fffffffffffffffe
B: MSC=10
B: LED=1f";

    /// A pad that advertises a whole keyboard so games can be sent keystrokes.
    /// Four of these enumerate with a base station attached, and a room with
    /// one of them in it is exactly the room the on-screen board is for.
    const STEAM_PUCK: &str = "\
I: Bus=0003 Vendor=28de Product=1304 Version=0111
N: Name=\"Valve Software Steam Controller Puck Keyboard\"
H: Handlers=sysrq kbd event14
B: PROP=0
B: EV=100013
B: KEY=e080ffdf01cfffff fffffffffffffffe
B: MSC=10";

    /// A mouse with macro keys bound to keystrokes and lighting of its own: it
    /// carries the full key bitmap *and* the lamps, and only its movement gives
    /// it away.
    const GAMING_MOUSE: &str = "\
I: Bus=0003 Vendor=046d Product=c08b Version=0111
N: Name=\"Logitech G502\"
H: Handlers=sysrq kbd mouse2 event5 leds
B: PROP=0
B: EV=12001f
B: KEY=3f00733fff 0 0 483ffff17aff32d bfd4444600000000 ffff0001 130ff38b17d007 ffff7bfad9415fff ffbeffdfffefffff fffffffffffffffe
B: REL=1943
B: LED=1f";

    /// It is handled by `kbd` and sends one key. It is a button on a case.
    const POWER_BUTTON: &str = "\
I: Bus=0019 Vendor=0000 Product=0001 Version=0000
N: Name=\"Power Button\"
H: Handlers=kbd event0
B: PROP=0
B: EV=3
B: KEY=8000 10000000000000 0";

    #[test]
    fn a_keyboard_is_a_keyboard() {
        assert!(is_typing_keyboard(WOOTING));
    }

    /// A Steam Controller is a controller. It passes udev's own test outright —
    /// lizard mode advertises a whole keyboard so a pad works in a program that
    /// has never heard of one — which is the whole reason there is more than
    /// udev's own test here.
    #[test]
    fn a_steam_controller_is_never_a_keyboard() {
        let keys = bits(STEAM_PUCK, "B: KEY=").unwrap();
        assert!((KEY_ESC..=KEY_D).all(|key| set(&keys, key)));
        assert!(!is_typing_keyboard(STEAM_PUCK));

        // And it stays a controller when the measurements stop saying so. The
        // same puck with lock lamps and no axes passes every test below, and is
        // still a pad — that is what makes this an exception by name rather
        // than something the heuristics happen to get right today.
        let with_lamps = STEAM_PUCK
            .replace("B: EV=100013", "B: EV=120013")
            .to_string()
            + "\nB: LED=1f";
        assert!(
            !is_typing_keyboard(&with_lamps),
            "the exception must not depend on the tests it stands in front of"
        );
        let anyone_else = with_lamps.replace("Vendor=28de", "Vendor=31e3");
        assert!(
            is_typing_keyboard(&anyone_else),
            "and it must be the vendor doing the work, not the record's shape"
        );
    }

    #[test]
    fn a_mouse_with_macro_keys_and_lighting_is_not_a_keyboard() {
        let events = bits(GAMING_MOUSE, "B: EV=").unwrap();
        assert!(set(&events, EV_LED), "it really does have lamps");
        assert!(!is_typing_keyboard(GAMING_MOUSE));
    }

    #[test]
    fn a_power_button_is_not_a_keyboard() {
        assert!(!is_typing_keyboard(POWER_BUTTON));
    }

    #[test]
    fn one_keyboard_among_the_pads_is_enough_and_none_is_none() {
        let console = [STEAM_PUCK, POWER_BUTTON, STEAM_PUCK].join("\n\n");
        let desk = [STEAM_PUCK, POWER_BUTTON, WOOTING, GAMING_MOUSE].join("\n\n");
        assert!(!keyboard_among(&console));
        assert!(keyboard_among(&desk));
    }

    /// Unreadable, empty, or a kernel that prints something else entirely: the
    /// board is offered. Withholding it from someone with no other way to type
    /// is the one failure that cannot be recovered from at a login screen.
    #[test]
    fn nothing_it_can_read_means_no_keyboard_it_knows_of() {
        assert!(!keyboard_among(""));
        assert!(!keyboard_among("I: Bus=0003\nN: Name=\"Something\""));
    }
}
