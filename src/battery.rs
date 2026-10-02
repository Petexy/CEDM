//! What is left in this machine's battery, for the mark in the login screen's
//! corner.
//!
//! Read out of `/sys/class/power_supply`, which is the kernel's own list of
//! everything the machine is powered by or powers and is there before any
//! daemon is. UPower is what a desktop would ask, and a login screen is the
//! worst place to depend on it: this runs before anybody has signed in, on a
//! machine whose services may not be up, and a handheld whose login screen
//! showed no battery because a daemon had not started would be wrong about the
//! hardware. It is the bargain [`crate::idle`] already makes when it asks
//! whether the machine is on its battery, and the two agree on which supplies
//! are the machine's because they ask the same question of them.
//!
//! It is the same reading LineXinBar's start screen draws its own mark from,
//! and it is kept in step with it on purpose: a console whose login screen said
//! sixty-one per cent and whose desktop said fifty-eight a few seconds later
//! would have two answers to one question. What counts as a battery, how two of
//! them are made one charge, and when a battery is "charging" are the shell's,
//! decided there and repeated here.
//!
//! # What is a battery, and what only calls itself one
//!
//! The directory lists the mains brick, the battery under the keyboard and the
//! cell in the wireless mouse on the desk beside it together. The last of those
//! is a battery by `type` and emphatically not this machine's, and a login
//! screen that quietly reported a mouse's charge would be worse than one that
//! showed nothing. `scope` says which is which: `Device` is a peripheral's own
//! cell, and anything else — including the file being absent, which is what
//! most laptop drivers do — is the system's. A laptop with its battery taken
//! out still lists the bay, with `present` at zero, and a mark drawn for
//! hardware that is not in the machine would be a lie of the same kind.
//!
//! # Where a number comes from
//!
//! Summed where the energies are there to sum, because a machine with two
//! batteries has one charge: a full small cell beside an empty cell three times
//! its size is a quarter full, and the mean of the two per cents says it is
//! half. `energy_*` is µWh and `charge_*` is µAh, a driver reports one pair or
//! the other, and either divides out to the same fraction. A driver that
//! reports only `capacity` still gets its reading used; it cannot be weighed
//! against anything, so it is averaged in as an equal. A battery that answers
//! nothing usable says nothing, and a machine whose every battery does is a
//! machine with no mark rather than one with a flat battery invented for it.
//!
//! # Synchronous, and so not for the frame loop
//!
//! There is no thread here and no uevent socket: [`read`] is a handful of small
//! file reads and it returns. That is cheap on most machines and not on the
//! ones this matters for. On a laptop `capacity` and `status` are answered by
//! the embedded controller over ACPI, and a slow controller turns a read into
//! tens of milliseconds of blocking, which on a frame loop is frames dropped on
//! the handhelds and none on anything else. How often to ask is the caller's to
//! decide, and the answer is every few seconds and never every frame.
//!
//! # Nothing in a supply directory is trusted to be small
//!
//! The kernel's attributes are a few bytes each, but the directory is also
//! whatever `--debug-power-supply` is pointed at, and this is a login screen,
//! where nothing is read unbounded. Each attribute is opened the way
//! [`crate::reading`] opens everything else this greeter reads — a named pipe
//! in a supply's directory must not hold the screen — and read no further than
//! [`LONGEST_ATTRIBUTE`] bytes, and a longer one is refused rather than
//! truncated: the front of a file that will not stop is not a value it meant.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::reading::{self, Owner};

/// Where the kernel lists everything that supplies this machine with power.
///
/// Discovered from here at runtime and never named further in: which batteries
/// a machine has, and what they are called, is the machine's business, so no
/// supply is looked for by its name.
pub const SUPPLIES: &str = "/sys/class/power_supply";

/// The most of one attribute that is read.
///
/// A number is at most twenty digits and the longest word the kernel writes
/// into `status` is a dozen letters, so this is a great deal more than any of
/// them needs and a great deal less than anything worth reading to the end.
const LONGEST_ATTRIBUTE: u64 = 256;

/// What is in the battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Charge {
    /// How full it is, 0 to 100.
    pub percent: u8,
    /// Whether it is filling. Sitting on the mains at full is *not* this: the
    /// kernel says `Full` or `Not charging` there, and what the corner should
    /// show then is a full battery rather than one that is forever filling.
    pub charging: bool,
}

/// Which of the five drawings a charge is shown with.
///
/// Five, because what the corner has to say is which picture the battery is
/// rather than a number. The number is the account's setting, carried in its
/// look, and it is off unless somebody asked for it. A battery that is filling
/// has a drawing of its own beside these, which outranks whichever of the five
/// it would have had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Empty,
    Low,
    Half,
    High,
    Full,
}

/// Where one drawing gives way to the next.
///
/// Not even fifths. The two that matter are at the bottom: a battery under a
/// tenth is one the person has minutes of, and it gets a drawing of its own
/// that is plainly emptier than the one above it. The top three are the wide
/// middle of the range, where the difference between 62 and 71 per cent is
/// nothing anybody acts on. The same four edges the shell draws its own mark
/// with.
const LOW_AT: u8 = 10;
const HALF_AT: u8 = 35;
const HIGH_AT: u8 = 60;
const FULL_AT: u8 = 85;

impl Level {
    /// Which drawing a reading falls in.
    ///
    /// Plain edges, with no hysteresis, and the reason is what is being read. A
    /// radio's number wanders a point or two between readings and a mark on a
    /// boundary would flicker; a battery walks one way and slowly. And the
    /// number can be on screen beside the mark, where a guarded edge would be a
    /// drawing that disagreed with the digits next to it, which is a worse
    /// fault than the flicker the guard exists to prevent.
    pub fn of(percent: u8) -> Self {
        match percent {
            p if p >= FULL_AT => Level::Full,
            p if p >= HIGH_AT => Level::High,
            p if p >= HALF_AT => Level::Half,
            p if p >= LOW_AT => Level::Low,
            _ => Level::Empty,
        }
    }
}

/// This machine's charge, read now out of the kernel's own directory.
pub fn read() -> Option<Charge> {
    read_in(Path::new(SUPPLIES))
}

/// The same, out of a directory laid out like the kernel's.
///
/// `None` when there is nothing there this program would call a battery, and
/// also when there is one it cannot get a number out of: a mark drawn from a
/// reading that failed would be a lie about the hardware, and the corner has a
/// perfectly good answer for not knowing, which is to draw nothing. A directory
/// that does not exist is the ordinary answer on anything that is not Linux
/// with a power supply class, and is not worth a line in the journal.
pub fn read_in(root: &Path) -> Option<Charge> {
    let batteries = batteries(root);
    if batteries.is_empty() {
        return None;
    }

    // Wide enough that no pair of readings can overflow however a file lies:
    // two `u64`s fit in a `u128` many times over, where a sum of them in a
    // `u64` would panic a debug build and wrap a release one into a charge
    // that was somebody else's number.
    let mut stored = 0u128;
    let mut capacity = 0u128;
    let mut percents: Vec<u8> = Vec::new();
    let mut charging = false;

    for battery in &batteries {
        if attribute(battery, "status").as_deref() == Some("Charging") {
            charging = true;
        }
        match (
            number(battery, "energy_now").or_else(|| number(battery, "charge_now")),
            number(battery, "energy_full").or_else(|| number(battery, "charge_full")),
        ) {
            (Some(now), Some(full)) if full > 0 => {
                stored += u128::from(now);
                capacity += u128::from(full);
            }
            // A full of nothing is a division by zero and not a battery with
            // no charge, so it falls through to `capacity` as a missing pair
            // does, and says nothing if there is none.
            _ => {
                if let Some(percent) = number(battery, "capacity") {
                    percents.push(percent.min(100) as u8);
                }
            }
        }
    }

    // Two readings, each `None` when there was nothing to work it out from:
    // what the energies weigh out to, and the plain mean of whatever reported
    // only a per cent.
    let weighed = stored
        .saturating_mul(100)
        .checked_div(capacity)
        .map(|percent| percent.min(100) as u64);
    let mean = percents
        .iter()
        .map(|percent| u64::from(*percent))
        .sum::<u64>()
        .checked_div(percents.len() as u64);

    let percent = match (weighed, mean) {
        (Some(weighed), None) => weighed,
        (None, Some(mean)) => mean,
        // Both kinds at once, which is a machine nobody has. The energies are
        // the better answer and the bare per cents cannot be joined to them, so
        // the two are averaged as equals rather than one of them thrown away.
        (Some(weighed), Some(mean)) => (weighed + mean) / 2,
        (None, None) => return None,
    };

    Some(Charge {
        percent: percent as u8,
        charging,
    })
}

/// Every supply in the directory that is this machine's own battery.
///
/// Sorted, so that a machine with two of them is read in the same order every
/// pass and the answer cannot depend on what order the filesystem happened to
/// hand them over in.
fn batteries(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_system(path))
        .collect();
    found.sort();
    found
}

/// Whether one supply is a battery of this machine's.
///
/// Three questions, and each of them has bitten a console somewhere. Is it a
/// battery at all, or the mains brick beside it. Is it *this machine's*, or the
/// cell in a wireless mouse, where an absent `scope` has to mean the system
/// because that is what nearly every laptop's driver says. And is there
/// anything in the bay.
///
/// Asked by [`crate::idle`] as well, for whether the machine is on its battery,
/// so that the mark and the waits are never about two different machines.
pub(crate) fn is_system(path: &Path) -> bool {
    if attribute(path, "type").as_deref() != Some("Battery") {
        return false;
    }
    if attribute(path, "scope").as_deref() == Some("Device") {
        return false;
    }
    attribute(path, "present").as_deref() != Some("0")
}

/// One attribute of a supply, trimmed. `None` for a file that is not there,
/// will not read, is not text, or is longer than [`LONGEST_ATTRIBUTE`] — which
/// in this directory is ordinary rather than exceptional for the first two: the
/// attributes a supply publishes depend on its driver, and a battery that has
/// just been taken out answers some of them with an error.
pub(crate) fn attribute(supply: &Path, name: &str) -> Option<String> {
    let file = reading::open(&supply.join(name), Owner::Anyone)?;
    // One byte past the bound, so that a file that is exactly as long as it may
    // be and one that goes on can be told apart.
    let mut bytes = Vec::new();
    file.take(LONGEST_ATTRIBUTE + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > LONGEST_ATTRIBUTE {
        return None;
    }
    String::from_utf8(bytes)
        .ok()
        .map(|text| text.trim().to_string())
}

/// The same, as a number. A sign, a unit, a fraction or more digits than a
/// `u64` holds are all not numbers.
fn number(supply: &Path, name: &str) -> Option<u64> {
    attribute(supply, name)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Build a `power_supply` directory: `(name, [(attribute, value)])`.
    fn tree(test: &str, supplies: &[(&str, &[(&str, &str)])]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cedm-battery-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for (name, attributes) in supplies {
            let supply = root.join(name);
            std::fs::create_dir_all(&supply).unwrap();
            for (attribute, value) in *attributes {
                std::fs::write(supply.join(attribute), format!("{value}\n")).unwrap();
            }
        }
        root
    }

    /// A laptop's one battery, with the attributes a fixture below varies left
    /// out so that each test says only what it is about.
    fn laptop(test: &str) -> PathBuf {
        tree(test, &[("BAT0", &[("type", "Battery")])])
    }

    fn charge(percent: u8, charging: bool) -> Option<Charge> {
        Some(Charge { percent, charging })
    }

    /// The whole point of `scope`, and the case a desk with a wireless mouse on
    /// it is: a battery that is emphatically not the machine's, beside the
    /// mains brick, which is not a battery at all.
    #[test]
    fn a_mouses_cell_and_the_mains_are_not_this_machines_battery() {
        let root = tree(
            "peripheral",
            &[
                (
                    "mouse_battery_0",
                    &[
                        ("type", "Battery"),
                        ("scope", "Device"),
                        ("capacity", "55"),
                        ("status", "Discharging"),
                    ],
                ),
                ("AC", &[("type", "Mains"), ("online", "1")]),
            ],
        );
        assert_eq!(read_in(&root), None, "a desktop has no battery");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A mouse on charge beside a laptop that is not does not make the laptop
    /// charging, and a mouse's flat cell does not drag its charge down.
    #[test]
    fn a_peripheral_beside_a_real_battery_changes_nothing() {
        let root = tree(
            "beside",
            &[
                (
                    "BAT0",
                    &[
                        ("type", "Battery"),
                        ("capacity", "80"),
                        ("status", "Discharging"),
                    ],
                ),
                (
                    "mouse_battery_0",
                    &[
                        ("type", "Battery"),
                        ("scope", "Device"),
                        ("capacity", "2"),
                        ("status", "Charging"),
                    ],
                ),
            ],
        );
        assert_eq!(read_in(&root), charge(80, false));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A laptop, which is the ordinary case: one battery, no `scope` file at
    /// all, and the reading comes back as it stands.
    #[test]
    fn a_laptop_battery_reads_even_with_no_scope_file() {
        let root = tree(
            "laptop",
            &[(
                "BAT0",
                &[
                    ("type", "Battery"),
                    ("present", "1"),
                    ("capacity", "96"),
                    ("status", "Discharging"),
                ],
            )],
        );
        assert_eq!(read_in(&root), charge(96, false));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An empty bay still lists the battery it has not got. Drawing an empty
    /// battery for it would be reporting hardware that is not in the machine.
    #[test]
    fn a_bay_with_nothing_in_it_is_not_a_battery() {
        let root = tree(
            "absent",
            &[(
                "BAT0",
                &[
                    ("type", "Battery"),
                    ("present", "0"),
                    ("capacity", "0"),
                    ("status", "Unknown"),
                ],
            )],
        );
        assert_eq!(read_in(&root), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Two batteries are one charge, and it is the energies that say what it
    /// is. A full 24 Wh cell beside an empty 72 Wh one is a quarter full; the
    /// average of the two per cents would call it half.
    #[test]
    fn two_batteries_are_weighed_rather_than_averaged() {
        let root = tree(
            "pair",
            &[
                (
                    "BAT0",
                    &[
                        ("type", "Battery"),
                        ("capacity", "100"),
                        ("energy_now", "24000000"),
                        ("energy_full", "24000000"),
                        ("status", "Discharging"),
                    ],
                ),
                (
                    "BAT1",
                    &[
                        ("type", "Battery"),
                        ("capacity", "0"),
                        ("energy_now", "0"),
                        ("energy_full", "72000000"),
                        ("status", "Discharging"),
                    ],
                ),
            ],
        );
        assert_eq!(
            read_in(&root).map(|charge| charge.percent),
            Some(25),
            "the mean of the two per cents would be 50"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `charge_*` in µAh is the other pair a driver may report, and it divides
    /// out to the same fraction.
    #[test]
    fn a_battery_reporting_amp_hours_reads_the_same_way() {
        let root = tree(
            "amps",
            &[(
                "BAT0",
                &[
                    ("type", "Battery"),
                    ("charge_now", "1500000"),
                    ("charge_full", "3000000"),
                    ("status", "Charging"),
                ],
            )],
        );
        assert_eq!(read_in(&root), charge(50, true));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A battery that reports only a per cent is believed, and one that reports
    /// per cents beside another that reports energies is joined to it as an
    /// equal rather than thrown away.
    #[test]
    fn a_bare_per_cent_is_used_and_joined_to_the_energies_as_an_equal() {
        let bare = tree(
            "bare",
            &[("BAT0", &[("type", "Battery"), ("capacity", "42")])],
        );
        assert_eq!(read_in(&bare), charge(42, false));
        let _ = std::fs::remove_dir_all(&bare);

        let both = tree(
            "both-kinds",
            &[
                ("BAT0", &[("type", "Battery"), ("capacity", "40")]),
                (
                    "BAT1",
                    &[
                        ("type", "Battery"),
                        ("energy_now", "100"),
                        ("energy_full", "100"),
                    ],
                ),
            ],
        );
        assert_eq!(read_in(&both), charge(70, false));
        let _ = std::fs::remove_dir_all(&both);
    }

    /// Sitting on the mains at full is not charging. The kernel says so, and
    /// the corner has a drawing for full that is not the one that means
    /// filling.
    #[test]
    fn full_on_the_mains_is_not_charging() {
        let root = tree(
            "topped-up",
            &[(
                "BAT0",
                &[("type", "Battery"), ("capacity", "100"), ("status", "Full")],
            )],
        );
        assert_eq!(read_in(&root), charge(100, false));
        let _ = std::fs::remove_dir_all(&root);

        let held = tree(
            "held",
            &[(
                "BAT0",
                &[
                    ("type", "Battery"),
                    ("capacity", "80"),
                    ("status", "Not charging"),
                ],
            )],
        );
        assert_eq!(
            read_in(&held),
            charge(80, false),
            "a charger that is in and not being used is not charging either"
        );
        let _ = std::fs::remove_dir_all(&held);
    }

    /// A machine with two batteries is charging when either of them is.
    #[test]
    fn either_battery_filling_is_the_machine_charging() {
        let root = tree(
            "either",
            &[
                (
                    "BAT0",
                    &[
                        ("type", "Battery"),
                        ("capacity", "50"),
                        ("status", "Discharging"),
                    ],
                ),
                (
                    "BAT1",
                    &[
                        ("type", "Battery"),
                        ("capacity", "50"),
                        ("status", "Charging"),
                    ],
                ),
            ],
        );
        assert_eq!(read_in(&root), charge(50, true));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// More than a hundred is a driver's rounding or a firmware's mistake, and
    /// either way what a battery holds cannot be more than all of it.
    #[test]
    fn more_than_all_of_it_is_brought_to_a_hundred() {
        let capacity = tree(
            "over-capacity",
            &[("BAT0", &[("type", "Battery"), ("capacity", "250")])],
        );
        assert_eq!(read_in(&capacity), charge(100, false));
        let _ = std::fs::remove_dir_all(&capacity);

        let energy = tree(
            "over-energy",
            &[(
                "BAT0",
                &[
                    ("type", "Battery"),
                    ("energy_now", "9000"),
                    ("energy_full", "3000"),
                ],
            )],
        );
        assert_eq!(read_in(&energy), charge(100, false));
        let _ = std::fs::remove_dir_all(&energy);
    }

    /// A battery that answers nothing at all is not a reading of zero. Nothing
    /// is drawn rather than a flat battery invented for the person.
    #[test]
    fn a_battery_with_no_numbers_in_it_says_nothing() {
        let root = tree("mute", &[("BAT0", &[("type", "Battery")])]);
        assert_eq!(read_in(&root), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A machine with no such directory — anything that is not Linux with a
    /// power supply class — is a machine with no battery, quietly. So is a
    /// name that is a file, and a directory with nothing in it.
    #[test]
    fn no_such_directory_is_no_battery() {
        let root = std::env::temp_dir().join(format!("cedm-battery-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(read_in(&root), None);

        std::fs::write(&root, "BAT0\n").unwrap();
        assert_eq!(
            read_in(&root),
            None,
            "a file is not a directory of supplies"
        );
        std::fs::remove_file(&root).unwrap();

        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(read_in(&root), None, "and an empty one has no battery");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The edges, each side of each of them.
    #[test]
    fn every_drawing_covers_the_readings_it_is_for() {
        assert_eq!(Level::of(0), Level::Empty);
        assert_eq!(Level::of(LOW_AT - 1), Level::Empty);
        assert_eq!(Level::of(LOW_AT), Level::Low);
        assert_eq!(Level::of(HALF_AT - 1), Level::Low);
        assert_eq!(Level::of(HALF_AT), Level::Half);
        assert_eq!(Level::of(HIGH_AT - 1), Level::Half);
        assert_eq!(Level::of(HIGH_AT), Level::High);
        assert_eq!(Level::of(FULL_AT - 1), Level::High);
        assert_eq!(Level::of(FULL_AT), Level::Full);
        assert_eq!(Level::of(100), Level::Full);
    }

    /// A file with a number in it that does not stop is not that number, and
    /// the bound is on the file and not on what is made of it: the same two
    /// digits padded out to a megabyte would read as fifty if everything were
    /// read and trimmed.
    #[test]
    fn a_file_that_goes_on_is_refused_rather_than_read_from_the_front() {
        let padded = |length: usize| format!("50{}", " ".repeat(length - 2));
        let root = laptop("long");
        let capacity = root.join("BAT0/capacity");

        std::fs::write(&capacity, padded(LONGEST_ATTRIBUTE as usize)).unwrap();
        assert_eq!(read_in(&root), charge(50, false), "as long as it may be");
        std::fs::write(&capacity, padded(LONGEST_ATTRIBUTE as usize + 1)).unwrap();
        assert_eq!(read_in(&root), None, "one byte past it");
        std::fs::write(&capacity, padded(4 * 1024 * 1024)).unwrap();
        assert_eq!(read_in(&root), None, "and far past it");

        // The same for a word: a status that goes on is not `Charging`.
        std::fs::write(&capacity, "50\n").unwrap();
        let status = root.join("BAT0/status");
        std::fs::write(&status, format!("Charging{}", " ".repeat(4 * 1024 * 1024))).unwrap();
        assert_eq!(read_in(&root), charge(50, false));
        std::fs::write(&status, "Charging\n").unwrap();
        assert_eq!(read_in(&root), charge(50, true));

        // And for what makes it a battery in the first place.
        std::fs::write(
            root.join("BAT0/type"),
            format!("Battery{}", " ".repeat(4 * 1024 * 1024)),
        )
        .unwrap();
        assert_eq!(read_in(&root), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Bytes that are not text are not a value, wherever they are, and are not
    /// a panic either.
    #[test]
    fn a_file_that_is_not_text_says_nothing() {
        let root = laptop("binary");
        let capacity = root.join("BAT0/capacity");
        for bytes in [
            &[0xff, 0xfe, b'5', b'0', b'\n'][..],
            &[b'5', b'0', 0x00, b'\n'][..],
            &[0xc3, 0x28][..],
            &[0x00; 64][..],
        ] {
            std::fs::write(&capacity, bytes).unwrap();
            assert_eq!(read_in(&root), None, "{bytes:?}");
        }

        std::fs::write(&capacity, "64\n").unwrap();
        std::fs::write(root.join("BAT0/status"), [0xff, 0xfe, 0xfd]).unwrap();
        assert_eq!(
            read_in(&root),
            charge(64, false),
            "a status that is not text is not charging, and the charge is still the charge"
        );

        std::fs::write(root.join("BAT0/type"), [0xff, 0xfe, 0xfd]).unwrap();
        assert_eq!(
            read_in(&root),
            None,
            "and a type that is not one is no battery"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// What `capacity` may say that is not a per cent.
    #[test]
    fn a_capacity_that_is_not_a_number_is_not_a_reading() {
        let root = laptop("capacity");
        let capacity = root.join("BAT0/capacity");
        for text in [
            "",
            "\n",
            "   ",
            "-5",
            "-0",
            "+-5",
            "5.5",
            "50%",
            "5 0",
            "NaN",
            "full",
            "0x32",
            "1e2",
            // One past what a `u64` holds, and a great deal past it.
            "18446744073709551616",
            "99999999999999999999999999999999999999999999",
        ] {
            std::fs::write(&capacity, text).unwrap();
            assert_eq!(read_in(&root), None, "{text:?}");
        }
        // What does fit in one is a battery holding more than all of it.
        for text in ["18446744073709551615", "101", "100"] {
            std::fs::write(&capacity, text).unwrap();
            assert_eq!(read_in(&root), charge(100, false), "{text:?}");
        }
        std::fs::write(&capacity, "0").unwrap();
        assert_eq!(read_in(&root), charge(0, false), "flat is a reading");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A battery that says it holds nothing at all is a division by zero for
    /// anything that trusts it. It falls back to `capacity` where there is one
    /// and says nothing where there is not.
    #[test]
    fn a_battery_that_is_full_of_nothing_is_not_divided_by() {
        let nothing = tree(
            "no-energy",
            &[(
                "BAT0",
                &[
                    ("type", "Battery"),
                    ("energy_now", "0"),
                    ("energy_full", "0"),
                ],
            )],
        );
        assert_eq!(read_in(&nothing), None);
        let _ = std::fs::remove_dir_all(&nothing);

        let some = tree(
            "no-energy-some",
            &[(
                "BAT0",
                &[
                    ("type", "Battery"),
                    ("energy_now", "5000"),
                    ("energy_full", "0"),
                ],
            )],
        );
        assert_eq!(read_in(&some), None, "charge held in a battery of no size");
        let _ = std::fs::remove_dir_all(&some);

        let fallback = tree(
            "no-energy-capacity",
            &[(
                "BAT0",
                &[
                    ("type", "Battery"),
                    ("charge_now", "0"),
                    ("charge_full", "0"),
                    ("capacity", "40"),
                ],
            )],
        );
        assert_eq!(read_in(&fallback), charge(40, false));
        let _ = std::fs::remove_dir_all(&fallback);
    }

    /// Numbers as large as a file can make them are summed without overflow:
    /// two of them add up to more than a `u64` holds.
    #[test]
    fn enormous_energies_are_summed_without_overflowing() {
        let most = u64::MAX.to_string();
        let root = tree(
            "enormous",
            &[
                (
                    "BAT0",
                    &[
                        ("type", "Battery"),
                        ("energy_now", &most),
                        ("energy_full", &most),
                    ],
                ),
                (
                    "BAT1",
                    &[
                        ("type", "Battery"),
                        ("energy_now", &most),
                        ("energy_full", &most),
                    ],
                ),
            ],
        );
        assert_eq!(read_in(&root), charge(100, false));
        let _ = std::fs::remove_dir_all(&root);

        let lopsided = tree(
            "lopsided",
            &[(
                "BAT0",
                &[
                    ("type", "Battery"),
                    ("energy_now", &most),
                    ("energy_full", "1"),
                ],
            )],
        );
        assert_eq!(read_in(&lopsided), charge(100, false));
        let _ = std::fs::remove_dir_all(&lopsided);
    }

    /// A name where a file should be is no reading, and is no panic.
    #[test]
    fn a_directory_where_a_file_should_be_says_nothing() {
        let root = laptop("directory");
        std::fs::create_dir(root.join("BAT0/capacity")).unwrap();
        assert_eq!(read_in(&root), None);

        std::fs::remove_dir(root.join("BAT0/capacity")).unwrap();
        std::fs::write(root.join("BAT0/capacity"), "70\n").unwrap();
        std::fs::create_dir(root.join("BAT0/status")).unwrap();
        assert_eq!(
            read_in(&root),
            charge(70, false),
            "a status that is a directory is not charging"
        );

        std::fs::remove_file(root.join("BAT0/type")).unwrap();
        std::fs::create_dir(root.join("BAT0/type")).unwrap();
        assert_eq!(read_in(&root), None, "and a type that is one is no battery");

        // A supply that is itself a plain file lists nothing.
        let _ = std::fs::remove_dir_all(root.join("BAT0"));
        std::fs::write(root.join("BAT0"), "Battery\n").unwrap();
        assert_eq!(read_in(&root), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A named pipe where a file should be is what `File::open` never comes
    /// back from. Run on a thread with a deadline, because a regression here
    /// does not fail a test; it hangs the suite, as it would hang the screen.
    #[test]
    fn a_named_pipe_where_a_file_should_be_is_refused_rather_than_waited_on() {
        let root = laptop("fifo");
        let name = std::ffi::CString::new(root.join("BAT0/capacity").to_str().unwrap()).unwrap();
        // SAFETY: `name` is a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o644) }, 0);

        let (answer, answered) = mpsc::channel();
        let asked = root.clone();
        std::thread::spawn(move || {
            let _ = answer.send(read_in(&asked));
        });
        assert_eq!(
            answered.recv_timeout(Duration::from_secs(5)),
            Ok(None),
            "reading a supply with a pipe in it must come back, and say nothing"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A link where a file should be is not followed, whatever it points at: the
    /// kernel's attributes are plain files, and a link is somebody's idea of one.
    #[test]
    fn a_link_where_a_file_should_be_is_not_followed() {
        let root = laptop("link");
        let elsewhere = root.join("elsewhere");
        std::fs::write(&elsewhere, "50\n").unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join("BAT0/capacity")).unwrap();
        assert_eq!(read_in(&root), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A supply that is itself a link is followed, because that is what the
    /// kernel's own entries are: `BAT0` is a link into `/sys/devices`.
    #[test]
    fn a_supply_that_is_a_link_is_read_like_the_kernels_are() {
        let root = std::env::temp_dir().join(format!("cedm-battery-linked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let devices = root.join("devices/battery");
        std::fs::create_dir_all(&devices).unwrap();
        std::fs::write(devices.join("type"), "Battery\n").unwrap();
        std::fs::write(devices.join("capacity"), "33\n").unwrap();
        let supplies = root.join("power_supply");
        std::fs::create_dir_all(&supplies).unwrap();
        std::os::unix::fs::symlink(&devices, supplies.join("BAT0")).unwrap();
        assert_eq!(read_in(&supplies), charge(33, false));
        let _ = std::fs::remove_dir_all(&root);
    }
}
