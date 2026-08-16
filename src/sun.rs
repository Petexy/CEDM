//! Where this machine is, and when the sun rises and sets there.
//!
//! One caller: [`crate::look`], which has to work out whether an account's
//! night light should be burning before it writes the configuration its
//! compositor comes up in. A compositor has no clock and no time zone — see
//! `night_light` in LineXinBar's own compositor config — so the schedule is
//! answered on this side, and `sunset-to-sunrise` is a schedule that needs a
//! latitude.
//!
//! # This is a transcription
//!
//! It is `lxb-desktop`'s `sun.rs`, term for term, and it has to stay that way:
//! the shell resolves the same setting for the same displays a second later,
//! and a login screen that disagreed with the session about whether it is
//! night would warm the screen and then cool it as the shell connected. What
//! is *not* carried over is everything only a running shell needs — the
//! caching, the settings page's `next_edge`, the writable override — because
//! this is asked once, by a program that exits.
//!
//! The alternative was to have the shell publish its coordinates. It does
//! write them, when a settings file names them by hand, and [`crate::look`]
//! carries those. But a machine that has never hand-edited that file publishes
//! nothing, and the answer was already on the disk: the zone table is system
//! data, world-readable, and says where every zone is.
//!
//! # Where the location comes from
//!
//! From the time zone, out of the zone table the C library's own data ships —
//! `/usr/share/zoneinfo/zone1970.tab`, which gives a representative coordinate
//! for every zone there is. `/etc/localtime` says which zone this machine is
//! in, and that one line says where that is.
//!
//! What it gives is a *city*, not a position: the zone's own representative
//! point. Somewhere in a large zone that can be a few hundred kilometres away,
//! which moves sunset by some tens of minutes — worth knowing, and far inside
//! what a night light cares about.
//!
//! # The sun itself
//!
//! The NOAA solar position equations, in the short form: a fractional year, an
//! equation of time, a declination, and the hour angle at which the centre of
//! the sun is 0.833° below the horizon — the standard definition of sunrise,
//! which includes the refraction that makes the sun visible while it is
//! geometrically already down. Good to about a minute, which is a great deal
//! better than the setting needs.

/// A place on the earth.
///
/// No name on it, unlike the shell's: the shell puts the zone in front of the
/// user, on a page that has to be able to explain why their light came on an
/// hour late. Nothing here is shown to anybody.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location {
    /// Degrees north, negative south.
    pub latitude: f64,
    /// Degrees east, negative west.
    pub longitude: f64,
}

impl Location {
    /// A location written down rather than looked up, for the settings file
    /// that names one.
    ///
    /// The shell's escape hatch for the one thing the zone table cannot do:
    /// say where somebody actually is inside a zone the size of a country.
    /// Nothing writes it — there is no page for it in either project — but a
    /// file that carries it is believed on both sides of the login.
    pub fn exact(latitude: f64, longitude: f64) -> Option<Self> {
        (latitude.is_finite()
            && longitude.is_finite()
            && (-90.0..=90.0).contains(&latitude)
            && (-180.0..=180.0).contains(&longitude))
        .then_some(Self {
            latitude,
            longitude,
        })
    }
}

/// What the sun does on one day at one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sun {
    /// It rises and sets, at these minutes of local time.
    Daily { sunrise: u16, sunset: u16 },
    /// It does not rise at all: the polar night, and a day the night light
    /// should burn the whole of.
    NeverRises,
    /// It does not set: the midnight sun, and a day with no night in it.
    NeverSets,
}

/// Where this machine is, as far as anything on it says.
///
/// `None` on a machine with no zone data, or one whose zone the table does not
/// carry — both of which are legal, and both of which mean the night light has
/// no sun to follow. What the caller does with that is what the shell does:
/// see [`crate::look::burning`].
pub fn location() -> Option<Location> {
    let zone = zone_name()?;
    let table = std::fs::read_to_string("/usr/share/zoneinfo/zone1970.tab")
        // The older table, which carries some zone names the 1970 one dropped.
        // A machine that has one and not the other is ordinary.
        .or_else(|_| std::fs::read_to_string("/usr/share/zoneinfo/zone.tab"))
        .ok()?;
    let (latitude, longitude) = coordinates_of(&zone, &table)?;
    Location::exact(latitude, longitude)
}

/// Which zone this machine is in, by its IANA name.
///
/// `TZ` first, because a session that sets it means it. Then the symlink every
/// distribution points at the zone's own file, and then the file Debian writes
/// the name into. Anything else is a machine that does not say.
fn zone_name() -> Option<String> {
    if let Some(named) = std::env::var_os("TZ") {
        let named = named.to_string_lossy();
        // `TZ` may hold a whole POSIX rule — `CET-1CEST,M3.5.0,M10.5.0/3` —
        // rather than a zone name. Only a name can be looked up, and the
        // leading colon some systems use is not part of it.
        let named = named.trim_start_matches(':').trim();
        if named.contains('/') {
            return Some(named.to_string());
        }
    }
    if let Ok(target) = std::fs::read_link("/etc/localtime") {
        let path = target.to_string_lossy();
        // `../usr/share/zoneinfo/Europe/Warsaw`, or the absolute form. What is
        // wanted is everything after the directory, which is two components
        // for most zones and one for a few.
        if let Some((_, zone)) = path.split_once("zoneinfo/") {
            let zone = zone.trim_matches('/');
            if !zone.is_empty() {
                return Some(zone.to_string());
            }
        }
    }
    let named = std::fs::read_to_string("/etc/timezone").ok()?;
    let named = named.trim();
    (!named.is_empty()).then(|| named.to_string())
}

/// Find a zone in the table and read its coordinate.
///
/// Split out so the parsing can be tested against lines written here rather
/// than against whatever this machine happens to have installed.
fn coordinates_of(zone: &str, table: &str) -> Option<(f64, f64)> {
    for line in table.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut fields = line.split('\t');
        let (_countries, coordinates, name) = (fields.next()?, fields.next(), fields.next());
        if name? != zone {
            continue;
        }
        return parse_iso6709(coordinates?);
    }
    None
}

/// `+5215+02100`, or `+521500+0210000`: the ISO 6709 form the zone table uses.
///
/// Latitude is two degree digits, longitude three, each followed by minutes and
/// optionally seconds. Anything else is a table this does not understand, which
/// comes back as no location rather than as a guess.
fn parse_iso6709(raw: &str) -> Option<(f64, f64)> {
    let raw = raw.trim();
    // The second sign is where the longitude starts; the first is at 0.
    let split = raw
        .char_indices()
        .skip(1)
        .find(|(_, character)| *character == '+' || *character == '-')
        .map(|(at, _)| at)?;
    let (latitude, longitude) = raw.split_at(split);
    Some((sexagesimal(latitude, 2)?, sexagesimal(longitude, 3)?))
}

/// One signed `±DD[D]MM[SS]` figure, in degrees.
fn sexagesimal(raw: &str, degree_digits: usize) -> Option<f64> {
    let sign = match raw.chars().next()? {
        '+' => 1.0,
        '-' => -1.0,
        _ => return None,
    };
    let digits = &raw[1..];
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // Degrees and minutes, or degrees, minutes and seconds. No other length is
    // a coordinate.
    let seconds_too = match digits.len() {
        length if length == degree_digits + 2 => false,
        length if length == degree_digits + 4 => true,
        _ => return None,
    };
    let number = |from: usize, to: usize| digits[from..to].parse::<f64>().ok();
    let degrees = number(0, degree_digits)?;
    let minutes = number(degree_digits, degree_digits + 2)?;
    let seconds = match seconds_too {
        true => number(degree_digits + 2, degree_digits + 4)?,
        false => 0.0,
    };
    Some(sign * (degrees + minutes / 60.0 + seconds / 3600.0))
}

/// When the sun rises and sets at `at`, on the `yday`th day of `year` counted
/// from zero, for a clock `utc_offset` seconds east of UTC.
///
/// The offset is passed in rather than worked out, because it is the one part
/// of this the C library has already answered — and answered including whatever
/// summer time is in force today, which no amount of arithmetic here would get
/// right. See [`crate::clock::Now`].
pub fn sun(yday: u16, year: i32, at: &Location, utc_offset: i32) -> Sun {
    let days = days_in_year(year) as f64;
    // NOAA's fractional year, taken at noon: the equation of time and the
    // declination both move slowly enough that the middle of the day is a good
    // enough moment to evaluate them for both ends of it.
    let gamma = std::f64::consts::TAU / days * yday as f64;

    let (sin1, cos1) = gamma.sin_cos();
    let (sin2, cos2) = (2.0 * gamma).sin_cos();
    let (sin3, cos3) = (3.0 * gamma).sin_cos();

    // Minutes by which apparent solar time runs ahead of mean solar time.
    let equation_of_time = 229.18
        * (0.000_075 + 0.001_868 * cos1 - 0.032_077 * sin1 - 0.014_615 * cos2 - 0.040_849 * sin2);
    // How far north of the equator the sun is overhead, in radians.
    let declination = 0.006_918 - 0.399_912 * cos1 + 0.070_257 * sin1 - 0.006_758 * cos2
        + 0.000_907 * sin2
        - 0.002_697 * cos3
        + 0.001_48 * sin3;

    let latitude = at.latitude.to_radians();
    // 90.833°: the sun's centre is a little below the horizon at the moment its
    // upper limb appears, because the atmosphere bends the light round and
    // because the disc has a width.
    let zenith: f64 = 90.833_f64.to_radians();
    let hour_angle = (zenith.cos() / (latitude.cos() * declination.cos())
        - latitude.tan() * declination.tan())
    .clamp(-2.0, 2.0);

    // Out of range in either direction is not a failure: it is a latitude where
    // the sun does not cross the horizon today, which is the whole of the
    // answer for a night light.
    if hour_angle > 1.0 {
        return Sun::NeverRises;
    }
    if hour_angle < -1.0 {
        return Sun::NeverSets;
    }
    let hour_angle = hour_angle.acos().to_degrees();

    let local = |angle: f64| {
        // 720 is solar noon at longitude 0; four minutes is a degree of
        // rotation. The offset then puts it on the clock in the room.
        let minutes =
            720.0 - 4.0 * (at.longitude + angle) - equation_of_time + utc_offset as f64 / 60.0;
        // A day wraps: a place far enough east of its own zone can have a
        // sunrise that lands on the previous day's clock.
        let wrapped = minutes.rem_euclid(24.0 * 60.0);
        wrapped.round().clamp(0.0, 1439.0) as u16
    };
    Sun::Daily {
        sunrise: local(hour_angle),
        sunset: local(-hour_angle),
    }
}

/// 366 in a leap year, 365 otherwise. The fractional year above is divided by
/// it, and getting it wrong moves the answer by about a minute at the solstices
/// — small, and free to be right.
fn days_in_year(year: i32) -> u16 {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    match leap {
        true => 366,
        false => 365,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A place with a name nobody's machine is set to, so nothing here can be
    /// passing because of where this was built. The coordinates are real —
    /// they have to be, or the arithmetic could not be checked — but they are
    /// written down in the test rather than read off the machine.
    fn at(latitude: f64, longitude: f64) -> Location {
        Location {
            latitude,
            longitude,
        }
    }

    /// The zone table's own format, including the two shapes of coordinate and
    /// the rows that are not a zone at all.
    #[test]
    fn a_zone_is_found_in_the_table_by_its_own_name() {
        let table = "\
# comment\tnot\ta\tzone
PL\t+5215+02100\tTest/North
NZ\t-3652+17446\tTest/South\tsome comment
US\t+433458-0794217\tTest/Seconds
";
        let (latitude, longitude) = coordinates_of("Test/North", table).unwrap();
        assert!((latitude - 52.25).abs() < 1e-9);
        assert!((longitude - 21.0).abs() < 1e-9);

        let (latitude, longitude) = coordinates_of("Test/South", table).unwrap();
        assert!((latitude + 36.866_666).abs() < 1e-5);
        assert!((longitude - 174.766_666).abs() < 1e-5);

        let (latitude, longitude) = coordinates_of("Test/Seconds", table).unwrap();
        assert!((latitude - 43.582_777).abs() < 1e-5);
        assert!((longitude + 79.704_722).abs() < 1e-5);

        assert_eq!(coordinates_of("Test/Nowhere", table), None);
    }

    #[test]
    fn a_coordinate_that_is_not_one_is_no_location_rather_than_a_wrong_one() {
        for raw in ["", "+5215", "5215+02100", "+52x5+02100", "+52150+02100"] {
            assert_eq!(parse_iso6709(raw), None, "{raw:?}");
        }
        assert_eq!(Location::exact(f64::NAN, 0.0), None);
        assert_eq!(Location::exact(0.0, 400.0), None);
        assert_eq!(Location::exact(91.0, 0.0), None);
    }

    /// The equinox, where the answer is known without a table: the sun is up
    /// for about twelve hours everywhere, so sunrise is about six hours before
    /// local solar noon and sunset about six after.
    ///
    /// Checked at a longitude the zone's own meridian, so solar noon and clock
    /// noon are within the equation of time of each other.
    #[test]
    fn the_sun_is_up_for_half_the_equinox() {
        // 2026 is not a leap year, so 20 March is day 78 counted from zero.
        // A northern city, written down here rather than read off this machine.
        let Sun::Daily { sunrise, sunset } = sun(78, 2026, &at(55.95, -3.19), 0) else {
            panic!("the sun rises in March at this latitude");
        };
        let day = sunset as i32 - sunrise as i32;
        assert!(
            (700..=740).contains(&day),
            "an equinox {day} minutes long: {sunrise}..{sunset}"
        );
        // And the middle of it is around noon on the clock in the room. Not
        // exactly noon, and the two reasons are the whole of what the
        // arithmetic above is for: this place is a few degrees off the
        // meridian its clock is set to, which is four minutes a degree, and
        // apparent solar time runs up to a quarter of an hour away from mean
        // solar time depending on the day.
        let noon = (sunrise as i32 + sunset as i32) / 2;
        assert!((noon - 12 * 60).abs() < 30, "solar noon at {noon}");
    }

    /// The two answers that are not times, at a latitude that has both. This
    /// is what decides a whole polar winter's worth of night light.
    #[test]
    fn the_poles_get_the_answer_that_is_not_a_time() {
        let tromso = at(78.22, 15.65);
        // Midwinter and midsummer, counted from zero.
        assert_eq!(sun(355, 2026, &tromso, 3600), Sun::NeverRises);
        assert_eq!(sun(172, 2026, &tromso, 3600), Sun::NeverSets);
        // And the equinox between them is an ordinary day.
        assert!(matches!(sun(78, 2026, &tromso, 3600), Sun::Daily { .. }));
    }

    /// Sunset moves through the year, which is the whole reason this schedule
    /// exists rather than two hours the user typed.
    #[test]
    fn sunset_moves_between_midwinter_and_midsummer() {
        let here = at(55.95, -3.19);
        let evening = |yday| match sun(yday, 2026, &here, 0) {
            Sun::Daily { sunset, .. } => sunset,
            other => panic!("no sunset on day {yday}: {other:?}"),
        };
        // Hours apart at this latitude, and the summer end is the later one
        // whichever way the clocks have gone.
        assert!(evening(172) > evening(355) + 120);
    }

    #[test]
    fn a_leap_year_has_a_day_more_in_it() {
        assert_eq!(days_in_year(2024), 366);
        assert_eq!(days_in_year(2026), 365);
        assert_eq!(days_in_year(1900), 365);
        assert_eq!(days_in_year(2000), 366);
    }

    /// Whatever this machine is set to, asking it must not panic and must not
    /// answer with a coordinate that is not one. Both outcomes are real: a
    /// container with no zone data says nothing, and says so.
    #[test]
    fn this_machine_is_asked_without_being_assumed() {
        if let Some(here) = location() {
            assert!((-90.0..=90.0).contains(&here.latitude));
            assert!((-180.0..=180.0).contains(&here.longitude));
        }
    }
}
