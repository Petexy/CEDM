//! The LineXinBar on-screen keyboard's model and exact ANSI geometry.
//!
//! The character keys are not translated and never will be. This is a picture
//! of a keyboard, laid out in ANSI's own widths, and the letters printed on it
//! are the letters it types — a board whose caps said one thing and typed
//! another would be worse in every language than one that says QWERTY. What
//! the machine's language does decide is the handful of caps that are *words*:
//! see [`crate::i18n`], where the rule is that a legend stays Latin wherever
//! that language's own keyboards carry Latin legends.

use crate::i18n;

const NUMBER_ROW: (&str, &str) = ("`1234567890-=", "~!@#$%^&*()_+");
const UPPER_ROW: (&str, &str) = ("qwertyuiop[]", "QWERTYUIOP{}");
const HOME_ROW: (&str, &str) = ("asdfghjkl;'", "ASDFGHJKL:\"");
const LOWER_ROW: (&str, &str) = ("zxcvbnm,./", "ZXCVBNM<>?");
const FUNCTION_KEYS: [&str; 12] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
];

pub const COLUMNS: f32 = 15.0;
pub const ROW_COUNT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char, char),
    Named(&'static str, Stroke),
    Arrow(Arrow),
    Shift,
    Caps,
    Ctrl,
    Alt,
    Close,
}

impl Key {
    pub fn cap(self, shifted: bool) -> String {
        let text = i18n::text();
        match self {
            Self::Char(_, shifted_character) if shifted => shifted_character.to_string(),
            Self::Char(character, _) => character.to_string(),
            // Keyed off the stroke rather than off the English cap beside it,
            // so the function row — which is `F1` on every keyboard ever sold
            // — falls through to the name it was built with and nothing has to
            // list twelve keys that are the same in every language.
            Self::Named(cap, stroke) => match stroke {
                Stroke::Named("Escape") => text.key_escape,
                Stroke::Named("BackSpace") => text.key_backspace,
                Stroke::Named("Tab") => text.key_tab,
                Stroke::Named("Return") => text.key_enter,
                Stroke::Char(' ') => text.key_space,
                _ => cap,
            }
            .to_string(),
            Self::Arrow(_) | Self::Close => String::new(),
            Self::Shift => text.key_shift.to_string(),
            Self::Caps => text.key_caps.to_string(),
            Self::Ctrl => text.key_ctrl.to_string(),
            Self::Alt => text.key_alt.to_string(),
        }
    }

    pub fn is_close(self) -> bool {
        self == Self::Close
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    Left,
    Down,
    Up,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    Char(char),
    Named(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Type(Stroke),
    Shifted,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Latch {
    #[default]
    Off,
    Once,
    Locked,
}

impl Latch {
    fn pressed(self) -> Self {
        match self {
            Self::Off => Self::Once,
            Self::Once => Self::Locked,
            Self::Locked => Self::Off,
        }
    }

    fn spent(self) -> Self {
        match self {
            Self::Once => Self::Off,
            other => other,
        }
    }

    pub fn is_on(self) -> bool {
        self != Self::Off
    }
}

impl Stroke {
    pub const BACKSPACE: Self = Self::Named("BackSpace");
    pub const ENTER: Self = Self::Named("Return");
    pub const TAB: Self = Self::Named("Tab");
    pub const ESCAPE: Self = Self::Named("Escape");
    pub const SPACE: Self = Self::Char(' ');
}

pub fn row_scale(row: usize) -> f32 {
    if row == 0 {
        0.56
    } else {
        1.0
    }
}

pub fn row_spans(row: usize) -> Vec<(Key, f32)> {
    fn chars(pair: (&'static str, &'static str)) -> Vec<(Key, f32)> {
        pair.0
            .chars()
            .zip(pair.1.chars())
            .map(|(plain, shifted)| (Key::Char(plain, shifted), 1.0))
            .collect()
    }
    let mut keys = Vec::new();
    match row {
        0 => {
            let span = COLUMNS / 13.0;
            keys.push((Key::Named("Esc", Stroke::ESCAPE), span));
            keys.extend(FUNCTION_KEYS.map(|name| (Key::Named(name, Stroke::Named(name)), span)));
        }
        1 => {
            keys.extend(chars(NUMBER_ROW));
            keys.push((Key::Named("Back", Stroke::BACKSPACE), 2.0));
        }
        2 => {
            keys.push((Key::Named("Tab", Stroke::TAB), 1.5));
            keys.extend(chars(UPPER_ROW));
            keys.push((Key::Char('\\', '|'), 1.5));
        }
        3 => {
            keys.push((Key::Caps, 1.75));
            keys.extend(chars(HOME_ROW));
            keys.push((Key::Named("Enter", Stroke::ENTER), 2.25));
        }
        4 => {
            keys.push((Key::Shift, 2.25));
            keys.extend(chars(LOWER_ROW));
            keys.push((Key::Shift, 2.75));
        }
        _ => {
            keys.push((Key::Ctrl, 1.5));
            keys.push((Key::Alt, 1.5));
            keys.push((Key::Named("Space", Stroke::SPACE), 5.5));
            keys.extend(
                [Arrow::Left, Arrow::Down, Arrow::Up, Arrow::Right]
                    .map(|arrow| (Key::Arrow(arrow), 1.0)),
            );
            keys.push((Key::Close, 2.5));
        }
    }
    keys
}

pub fn row_layout(row: usize) -> Vec<(f32, f32)> {
    let keys = row_spans(row);
    let width = keys.iter().map(|(_, span)| *span).sum::<f32>();
    let mut at = (COLUMNS - width) * 0.5;
    keys.into_iter()
        .map(|(_, span)| {
            let answer = (at, span);
            at += span;
            answer
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct Board {
    pub row: usize,
    pub column: usize,
    shift: Latch,
    ctrl: Latch,
    alt: Latch,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            row: 3,
            column: 1,
            shift: Latch::Off,
            ctrl: Latch::Off,
            alt: Latch::Off,
        }
    }
}

impl Board {
    pub fn selected(&self) -> (usize, usize) {
        (self.row, self.column)
    }
    pub fn shifted(&self) -> bool {
        self.shift.is_on()
    }

    pub fn latched(&self, key: Key) -> Latch {
        match key {
            Key::Shift => self.shift,
            Key::Ctrl => self.ctrl,
            Key::Alt => self.alt,
            Key::Caps if self.shift == Latch::Locked => Latch::Locked,
            _ => Latch::Off,
        }
    }

    pub fn select(&mut self, row: usize, column: usize) -> bool {
        if row >= ROW_COUNT || column >= row_spans(row).len() {
            return false;
        }
        let moved = (row, column) != self.selected();
        self.row = row;
        self.column = column;
        moved
    }

    pub fn left(&mut self) {
        let len = row_spans(self.row).len();
        self.column = if self.column == 0 {
            len - 1
        } else {
            self.column - 1
        };
    }

    pub fn right(&mut self) {
        self.column = (self.column + 1) % row_spans(self.row).len();
    }

    pub fn up(&mut self) {
        self.move_vertical(-1);
    }
    pub fn down(&mut self) {
        self.move_vertical(1);
    }

    fn move_vertical(&mut self, delta: isize) {
        let next = (self.row as isize + delta).rem_euclid(ROW_COUNT as isize) as usize;
        let wanted = {
            let (start, span) = row_layout(self.row)[self.column];
            start + span * 0.5
        };
        self.row = next;
        self.column = row_layout(next)
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let distance = |&(start, span): &(f32, f32)| (start + span * 0.5 - wanted).abs();
                distance(a).total_cmp(&distance(b))
            })
            .map(|(index, _)| index)
            .unwrap_or(0);
    }

    pub fn press(&mut self) -> Press {
        match &row_spans(self.row)[self.column].0 {
            Key::Char(plain, shifted) => {
                let stroke = Stroke::Char(if self.shifted() { *shifted } else { *plain });
                self.spend();
                Press::Type(stroke)
            }
            Key::Named(_, stroke) => {
                let stroke = *stroke;
                self.spend();
                Press::Type(stroke)
            }
            Key::Arrow(Arrow::Left) => {
                self.spend();
                Press::Type(Stroke::Named("Left"))
            }
            Key::Arrow(Arrow::Down) => {
                self.spend();
                Press::Type(Stroke::Named("Down"))
            }
            Key::Arrow(Arrow::Up) => {
                self.spend();
                Press::Type(Stroke::Named("Up"))
            }
            Key::Arrow(Arrow::Right) => {
                self.spend();
                Press::Type(Stroke::Named("Right"))
            }
            Key::Shift => {
                self.shift = self.shift.pressed();
                Press::Shifted
            }
            Key::Caps => {
                self.shift = if self.shift == Latch::Locked {
                    Latch::Off
                } else {
                    Latch::Locked
                };
                Press::Shifted
            }
            Key::Ctrl => {
                self.ctrl = self.ctrl.pressed();
                Press::Shifted
            }
            Key::Alt => {
                self.alt = self.alt.pressed();
                Press::Shifted
            }
            Key::Close => Press::Close,
        }
    }

    fn spend(&mut self) {
        self.shift = self.shift.spent();
        self.ctrl = self.ctrl.spent();
        self.alt = self.alt.spent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_full_row_is_exactly_ansi_width() {
        for row in 1..ROW_COUNT {
            let total: f32 = row_spans(row).iter().map(|(_, span)| span).sum();
            assert!((total - COLUMNS).abs() < f32::EPSILON, "row {row}: {total}");
        }
    }

    #[test]
    fn board_starts_on_the_home_rows_first_letter() {
        let board = Board::default();
        assert_eq!(board.selected(), (3, 1));
        assert_eq!(row_spans(3)[1].0, Key::Char('a', 'A'));
    }

    /// What the board types is the board's, and what it *says* is the
    /// language's. A cap that moved a letter would be a keyboard that lies.
    #[test]
    fn the_letters_are_the_boards_and_the_words_are_the_languages() {
        for language in i18n::ALL {
            i18n::with_language(language, || {
                for row in 0..ROW_COUNT {
                    for (key, _) in row_spans(row) {
                        if let Key::Char(plain, upper) = key {
                            assert_eq!(key.cap(false), plain.to_string());
                            assert_eq!(key.cap(true), upper.to_string());
                        }
                    }
                }
                // The function row is `F1` on every keyboard ever sold.
                assert_eq!(row_spans(0)[1].0.cap(false), "F1");
            });
        }
        i18n::with_language(i18n::Language::German, || {
            assert_eq!(Key::Ctrl.cap(false), "Strg");
            assert_eq!(Key::Shift.cap(false), "Umschalt");
            assert_eq!(Key::Named("Enter", Stroke::ENTER).cap(false), "Enter");
        });
        i18n::with_language(i18n::Language::French, || {
            assert_eq!(Key::Named("Enter", Stroke::ENTER).cap(false), "Entrée");
            assert_eq!(Key::Named("Space", Stroke::SPACE).cap(false), "Espace");
        });
        // A Russian keyboard has `Shift` written on it. A board that said
        // anything else would be a board nobody has ever seen.
        i18n::with_language(i18n::Language::Russian, || {
            assert_eq!(Key::Shift.cap(false), "Shift");
            assert_eq!(Key::Ctrl.cap(false), "Ctrl");
        });
    }
}
