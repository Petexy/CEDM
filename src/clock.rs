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
//! the mercy of `LC_TIME` in an environment it does not control.
//!
//! The words in them come from [`crate::i18n`] instead, which reads the
//! machine's language rather than this process's environment. The shape of the
//! line is the language's too, and is not always a weekday followed by a
//! number: Chinese puts the day first and marks it.

use crate::i18n;
use std::time::{SystemTime, UNIX_EPOCH};

/// Which of the two clocks a time of day is written on.
///
/// The shell's own setting — Settings > System > Clock — which reaches this
/// screen the way the accent and the material do: through the copy of
/// `shell.toml` the account published on its way into its last session. See
/// [`crate::look::Look::clock`].
///
/// AM and PM are the same two marks in every language this greeter speaks, so
/// they are not in [`crate::i18n::Strings`]. They are also the reason the
/// clock's alphabet has letters in it at all; see
/// [`crate::visual::letters::SET`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clock {
    /// Nobody has chosen, so the **language** answers: an account reading
    /// English as America writes it gets `8:38 PM`, and every other account
    /// gets `20:38`.
    ///
    /// The default, and what every published look written before the shell had
    /// this setting says by saying nothing — so a machine that is not set to
    /// English (US) goes on showing exactly the clock it always showed.
    #[default]
    FromLanguage,
    TwentyFourHour,
    TwelveHour,
}

impl Clock {
    /// What `shell.toml` writes, and what a published look carries.
    pub const fn key(self) -> &'static str {
        match self {
            Self::FromLanguage => "language",
            Self::TwentyFourHour => "24-hour",
            Self::TwelveHour => "12-hour",
        }
    }

    /// Read one back. `None` for a word this build has no clock for, which the
    /// caller reads as nothing having been chosen: the file is one the user is
    /// entitled to open, and an unknown word is not a reason to invent a clock.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "language" => Some(Self::FromLanguage),
            "24-hour" => Some(Self::TwentyFourHour),
            "12-hour" => Some(Self::TwelveHour),
            _ => None,
        }
    }

    /// Whether a time is written with AM or PM after it.
    ///
    /// Where nobody has chosen, the account's **language** answers, and two of
    /// the ten read the twelve-hour clock: English (US) and हिन्दी — the clock
    /// America and India both read. That is CLDR's preferred hour cycle for
    /// each, and the same pair the shell and lxb-toolkit answer for, so a
    /// machine nobody has set writes the same time on the login screen, on the
    /// start screen and in an application.
    pub fn twelve_hour(self) -> bool {
        match self {
            Self::TwelveHour => true,
            Self::TwentyFourHour => false,
            Self::FromLanguage => matches!(
                i18n::language(),
                i18n::Language::AmericanEnglish | i18n::Language::Hindi
            ),
        }
    }
}

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

    /// One hour of an ordinary day, for a test that wants every hour of it
    /// rather than the one it happens to be.
    #[cfg(test)]
    pub fn at_hour(hour: u8, minute: u8) -> Self {
        Self {
            hour,
            minute,
            weekday: 1,
            day: 17,
            yday: 16,
            year: 2026,
            offset: 0,
        }
    }

    /// `20:38`, or `8:38 PM` — whichever clock the account being signed in to
    /// keeps.
    ///
    /// It was the twenty-four hour clock whatever the machine said, on the
    /// argument that it needs no locale to be read correctly. That argument
    /// was about a *locale*, which nobody chose; it does not survive a row
    /// somebody pressed in Settings > System > Clock, and a login screen that
    /// went on writing 20:38 in front of a console set to the twelve-hour
    /// clock would be the one screen on the machine ignoring the setting.
    ///
    /// The hour keeps its leading zero on the twenty-four hour clock and loses
    /// it on the twelve, which is what each is written with.
    pub fn time(&self, clock: Clock) -> String {
        if !clock.twelve_hour() {
            return format!("{:02}:{:02}", self.hour, self.minute);
        }
        // Midnight is twelve, not zero, and so is noon: the hour rolls to
        // twelve at each end rather than counting from it.
        let half = if self.hour < 12 { "AM" } else { "PM" };
        let hour = match self.hour % 12 {
            0 => 12,
            other => other,
        };
        format!("{hour}:{:02} {half}", self.minute)
    }

    /// `Mon 17`, in the language the machine is set to.
    pub fn date(&self) -> String {
        let text = i18n::text();
        text.date
            .replace("{weekday}", text.weekdays[self.weekday as usize % 7])
            .replace("{day}", &self.day.to_string())
    }

    /// How to greet somebody at this hour.
    ///
    /// Four bands rather than three: at two in the morning "Good evening" is
    /// wrong in a way the user notices, and a greeter is disproportionately
    /// often read at that hour.
    ///
    /// The bands are the same in every language even where that language has
    /// fewer greetings than four to put in them. Spanish says *buenas noches*
    /// from six in the evening until morning, so the last two bands hold the
    /// same words; that is Spanish being right about the evening rather than
    /// this being wrong about the bands.
    pub fn greeting(&self) -> &'static str {
        let text = i18n::text();
        match self.hour {
            5..=11 => text.good_morning,
            12..=17 => text.good_afternoon,
            18..=21 => text.good_evening,
            _ => text.good_night,
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
        assert_eq!(now.time(Clock::TwentyFourHour), "20:38");
        i18n::with_language(i18n::Language::English, || {
            assert_eq!(now.date(), "Mon 17");
        });
    }

    #[test]
    fn pads_to_a_stable_width_so_the_clock_does_not_jump() {
        let now = Now::from_tm(&tm(9, 5, 0, 1)).expect("valid time");
        assert_eq!(now.time(Clock::TwentyFourHour), "09:05");
        i18n::with_language(i18n::Language::English, || {
            assert_eq!(now.date(), "Sun 1");
        });
    }

    /// The other clock, and the language that answers for an account nobody
    /// has asked.
    ///
    /// The hour loses its leading zero on the twelve-hour clock, which is what
    /// that clock is written with — `8:38 PM`, never `08:38 PM` — so the run
    /// is one character narrower and one wider than the other's by turns. The
    /// clock is centred in its rectangle, so nothing jumps.
    #[test]
    fn the_twelve_hour_clock_rolls_to_twelve_at_each_end() {
        let at = |hour, minute| {
            Now::from_tm(&tm(hour, minute, 1, 17))
                .expect("valid time")
                .time(Clock::TwelveHour)
        };
        assert_eq!(at(20, 38), "8:38 PM");
        assert_eq!(at(0, 5), "12:05 AM", "midnight is twelve");
        assert_eq!(at(12, 0), "12:00 PM", "and so is noon");
        assert_eq!(at(11, 59), "11:59 AM");
        assert_eq!(at(23, 59), "11:59 PM");

        // Nothing chosen: the account's own language answers, and English (US)
        // and Hindi are the two that write AM and PM.
        let now = Now::from_tm(&tm(20, 38, 1, 17)).expect("valid time");
        for language in [i18n::Language::AmericanEnglish, i18n::Language::Hindi] {
            i18n::with_language(language, || {
                assert!(Clock::FromLanguage.twelve_hour());
                assert_eq!(now.time(Clock::FromLanguage), "8:38 PM");
            });
        }
        // And the other eight read the twenty-four hour clock.
        for language in i18n::ALL.into_iter().filter(|language| {
            !matches!(
                language,
                i18n::Language::AmericanEnglish | i18n::Language::Hindi
            )
        }) {
            i18n::with_language(language, || {
                assert!(!Clock::FromLanguage.twelve_hour());
                assert_eq!(now.time(Clock::FromLanguage), "20:38");
            });
        }

        // And a look that names one outranks the language in both directions.
        i18n::with_language(i18n::Language::AmericanEnglish, || {
            assert_eq!(now.time(Clock::TwentyFourHour), "20:38");
        });
        assert_eq!(Clock::parse("12-hour"), Some(Clock::TwelveHour));
        assert_eq!(Clock::parse("sundial"), None);
        for clock in [
            Clock::FromLanguage,
            Clock::TwentyFourHour,
            Clock::TwelveHour,
        ] {
            assert_eq!(Clock::parse(clock.key()), Some(clock));
        }
        assert_eq!(Clock::default(), Clock::FromLanguage);
    }

    /// Every character either clock can write is one the greeter's own
    /// alphabet has a cell for — or the whole clock is drawn as nothing.
    ///
    /// This is the check that caught the twelve-hour clock: the alphabet was
    /// the ten digits and a colon, and `8:38 PM` has four characters outside
    /// it. See [`crate::visual::letters::SET`].
    #[test]
    fn every_clock_is_written_out_of_the_alphabet_the_greeter_ships() {
        for hour in 0..24 {
            for clock in [Clock::TwentyFourHour, Clock::TwelveHour] {
                let now = Now::from_tm(&tm(hour, 5, 1, 17)).expect("valid time");
                let written = now.time(clock);
                assert!(
                    crate::visual::letters::run(&written).is_some(),
                    "the greeter cannot draw {written:?}"
                );
            }
        }
    }

    /// The second line is a *pattern*, not a weekday with a number stuck on
    /// the end of it: the languages disagree about which comes first and about
    /// whether the number is marked.
    #[test]
    fn the_date_is_set_the_way_the_language_sets_it() {
        let now = Now::from_tm(&tm(20, 38, 1, 17)).expect("valid time");
        for (language, want) in [
            (i18n::Language::Polish, "pon. 17"),
            (i18n::Language::Russian, "пн 17"),
            (i18n::Language::German, "Mo 17."),
            (i18n::Language::Hindi, "सोम 17"),
            (i18n::Language::Chinese, "17日 周一"),
        ] {
            i18n::with_language(language, || assert_eq!(now.date(), want));
        }
    }

    #[test]
    fn every_hour_of_the_day_has_a_greeting() {
        for language in i18n::ALL {
            i18n::with_language(language, || {
                for hour in 0..24 {
                    let now = Now::from_tm(&tm(hour, 0, 3, 12)).expect("valid time");
                    assert!(!now.greeting().is_empty(), "{hour} in {language:?}");
                }
            });
        }
        i18n::with_language(i18n::Language::English, || {
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
        });
        i18n::with_language(i18n::Language::French, || {
            assert_eq!(
                Now::from_tm(&tm(7, 0, 3, 12)).unwrap().greeting(),
                "Bonjour"
            );
            assert_eq!(
                Now::from_tm(&tm(20, 38, 3, 12)).unwrap().greeting(),
                "Bonsoir"
            );
        });
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
        assert_eq!(now.time(Clock::TwentyFourHour).len(), 5);
    }
}
