//! The wall clock on the greeter's right-hand side.
//!
//! Deliberately separate from [`crate::handoff::SceneClock`], which is the
//! wallpaper's animation clock and is anchored to `CLOCK_MONOTONIC` precisely
//! so that changing the time cannot move it. This one is the opposite: it is
//! the time the user reads, so it is the adjustable civil time, in the
//! machine's own timezone, and it is allowed to jump when `ntpd` corrects it.
//!
//! No date library. `localtime_r` is the C library's own reading of
//! `/etc/localtime`, which is the same answer every other program on the
//! machine gets, and the two fields the greeter actually shows are formatted
//! here rather than through `strftime` — that would put the greeter's clock at
//! the mercy of `LC_TIME` in an environment it does not control, and a login
//! screen showing a different date format from the desktop behind it is worse
//! than one showing English.

use std::time::{SystemTime, UNIX_EPOCH};

/// A civil time, already broken down and bounded.
///
/// The first four fields are the clock the greeter shows. The last three are
/// for the one setting that is about a *date* as well as a time — the night
/// light kept from sunset to sunrise, which [`crate::sun`] works out — and
/// they are here rather than read again separately, because the hour a
/// schedule is compared against and the day whose sunset it is compared with
/// have to come from one reading. Two readings either side of midnight would
/// answer one of them for yesterday.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    pub hour: u8,
    pub minute: u8,
    /// Days since Sunday, 0..=6.
    pub weekday: u8,
    /// Day of the month, 1..=31.
    pub day: u8,
    /// Days since the first of January, counted from zero — which is what the
    /// solar equations take.
    pub yday: u16,
    /// The year, for the one thing it decides here: whether this one has 366
    /// days in it.
    pub year: i32,
    /// Seconds east of UTC. Summer time is part of it, because it is part of
    /// what the clock in the room says — and because the sun is worked out
    /// against the clock in the room.
    pub offset: i32,
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

impl Now {
    /// Read the machine's local civil time.
    ///
    /// `None` on the platforms and configurations where that cannot be done at
    /// all, which the greeter renders as no clock rather than as a wrong one.
    pub fn read() -> Option<Self> {
        Self::at(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_secs()
                .try_into()
                .ok()?,
        )
    }

    fn at(seconds: i64) -> Option<Self> {
        let mut broken_down: libc::tm = unsafe { std::mem::zeroed() };
        let timestamp = seconds as libc::time_t;
        // SAFETY: `localtime_r` writes into the caller's `tm` and reads only
        // the `time_t` behind the pointer. Both live for the whole call, and
        // the reentrant form takes no process-wide static to race over.
        let result = unsafe { libc::localtime_r(&timestamp, &mut broken_down) };
        if result.is_null() {
            return None;
        }
        Self::from_tm(&broken_down)
    }

    /// Reject anything outside the ranges C guarantees, rather than clamping.
    ///
    /// A leap second arrives as `tm_sec == 60`, which is not read here. Every
    /// other field out of range means the conversion did not do what it says,
    /// and showing 61:99 would be worse than showing nothing.
    ///
    /// The three solar fields are read the same way and are refused the same
    /// way: a `tm` this cannot believe is not one to work a sunset out of
    /// either. `tm_gmtoff` is a GNU extension the C library fills in from the
    /// zone file, summer time included, which is the whole reason it is taken
    /// from here rather than calculated.
    fn from_tm(broken_down: &libc::tm) -> Option<Self> {
        Some(Self {
            hour: u8::try_from(broken_down.tm_hour).ok().filter(|h| *h < 24)?,
            minute: u8::try_from(broken_down.tm_min).ok().filter(|m| *m < 60)?,
            weekday: u8::try_from(broken_down.tm_wday).ok().filter(|d| *d < 7)?,
            day: u8::try_from(broken_down.tm_mday)
                .ok()
                .filter(|d| (1..=31).contains(d))?,
            yday: u16::try_from(broken_down.tm_yday)
                .ok()
                .filter(|d| *d < 366)?,
            // `tm_year` is years since 1900, and the range is what a clock
            // that has not been set can hand back: the epoch itself is 1970,
            // and nothing this side of it is a date to follow the sun on.
            year: broken_down
                .tm_year
                .checked_add(1900)
                .filter(|year| (1970..=9999).contains(year))?,
            // Bounded at a day either way, which is more than any zone has
            // ever been offset by and is what stops a nonsense value moving
            // sunset into another day.
            offset: i32::try_from(broken_down.tm_gmtoff)
                .ok()
                .filter(|offset| offset.abs() <= 86_400)?,
        })
    }

    /// `20:38`. Twenty-four hour, which is what the design shows and what
    /// needs no locale to be read correctly.
    pub fn time(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// `Mon 17`.
    pub fn date(&self) -> String {
        format!("{} {}", WEEKDAYS[self.weekday as usize % 7], self.day)
    }

    /// How to greet somebody at this hour.
    ///
    /// Four bands rather than three: at two in the morning "Good evening" is
    /// wrong in a way the user notices, and a greeter is disproportionately
    /// often read at that hour.
    pub fn greeting(&self) -> &'static str {
        match self.hour {
            5..=11 => "Good morning",
            12..=17 => "Good afternoon",
            18..=21 => "Good evening",
            _ => "Good night",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tm(hour: i32, minute: i32, weekday: i32, day: i32) -> libc::tm {
        let mut value: libc::tm = unsafe { std::mem::zeroed() };
        value.tm_hour = hour;
        value.tm_min = minute;
        value.tm_wday = weekday;
        value.tm_mday = day;
        // A zeroed `tm` is the first of January 1900 at UTC, which `from_tm`
        // refuses. Every test below is about the clock rather than the date,
        // so they are all set on one ordinary day.
        value.tm_yday = day.max(1) - 1;
        value.tm_year = 126;
        value
    }

    #[test]
    fn renders_the_two_fields_the_panel_shows() {
        let now = Now::from_tm(&tm(20, 38, 1, 17)).expect("valid time");
        assert_eq!(now.time(), "20:38");
        assert_eq!(now.date(), "Mon 17");
    }

    #[test]
    fn pads_to_a_stable_width_so_the_clock_does_not_jump() {
        let now = Now::from_tm(&tm(9, 5, 0, 1)).expect("valid time");
        assert_eq!(now.time(), "09:05");
        assert_eq!(now.date(), "Sun 1");
    }

    #[test]
    fn every_hour_of_the_day_has_a_greeting() {
        for hour in 0..24 {
            let now = Now::from_tm(&tm(hour, 0, 3, 12)).expect("valid time");
            assert!(!now.greeting().is_empty());
        }
        assert_eq!(
            Now::from_tm(&tm(7, 0, 3, 12)).unwrap().greeting(),
            "Good morning"
        );
        assert_eq!(
            Now::from_tm(&tm(20, 38, 3, 12)).unwrap().greeting(),
            "Good evening"
        );
        assert_eq!(
            Now::from_tm(&tm(2, 0, 3, 12)).unwrap().greeting(),
            "Good night"
        );
    }

    #[test]
    fn an_impossible_conversion_is_no_clock_rather_than_a_wrong_one() {
        assert!(Now::from_tm(&tm(24, 0, 0, 1)).is_none());
        assert!(Now::from_tm(&tm(12, 60, 0, 1)).is_none());
        assert!(Now::from_tm(&tm(12, 0, 7, 1)).is_none());
        assert!(Now::from_tm(&tm(12, 0, 0, 0)).is_none());
        assert!(Now::from_tm(&tm(-1, 0, 0, 1)).is_none());
    }

    /// The three fields the night light's schedule is worked out from are
    /// carried, and are refused on the same terms as the clock's own.
    #[test]
    fn the_date_the_sun_is_worked_out_for_comes_from_the_same_reading() {
        let mut value = tm(20, 38, 1, 17);
        value.tm_yday = 78;
        value.tm_year = 126;
        value.tm_gmtoff = 3600;
        let now = Now::from_tm(&value).expect("valid time");
        assert_eq!((now.yday, now.year, now.offset), (78, 2026, 3600));

        let refused = |edit: fn(&mut libc::tm)| {
            let mut value = tm(20, 38, 1, 17);
            value.tm_yday = 78;
            value.tm_year = 126;
            edit(&mut value);
            Now::from_tm(&value).is_none()
        };
        assert!(refused(|value| value.tm_yday = 366));
        assert!(refused(|value| value.tm_yday = -1));
        assert!(refused(|value| value.tm_year = 0));
        assert!(refused(|value| value.tm_gmtoff = 90_000));

        // And the machine's own reading is a date the sun can be worked out
        // for, whatever machine this is.
        let now = Now::read().expect("local time");
        assert!(now.yday < 366 && now.year >= 1970);
    }

    #[test]
    fn the_machines_own_clock_can_be_read() {
        let now = Now::read().expect("local time");
        assert!(now.hour < 24 && now.minute < 60);
        assert_eq!(now.time().len(), 5);
    }
}
