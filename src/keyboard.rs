//! The LineXinBar on-screen keyboard's model and exact ANSI geometry.
//!
//! The character keys are not *translated* and never will be. This is a picture
//! of a keyboard, laid out in ANSI's own widths, and the letters printed on it
//! are the letters it types — a board whose caps said one thing and typed
//! another would be worse in every language than one that says QWERTY. What
//! the machine's language does decide is the handful of caps that are *words*:
//! see [`crate::i18n`], where the rule is that a legend stays Latin wherever
//! that language's own keyboards carry Latin legends.
//!
//! **What the machine's keyboard *layout* decides is the letters themselves.**
//! That is not a translation and it does not break the rule above: the caps and
//! what they type both come from the layout, so they still cannot disagree. A
//! login screen is exactly where this matters most — somebody with a Polish or
//! a French keyboard has a password with their own letters in it, and a board
//! that could only offer American ones is a board they cannot sign in with.
//! See [`note_layout`] and [`system_layout`].
//!
//! The *arrangement* stays ANSI whatever is printed on it. A board driven with
//! a thumb cannot change shape between layouts — the user is hunting for a
//! letter by looking — and xkb already maps the ANSI positions to whatever the
//! layout prints there. So a French layout puts A where ANSI prints Q, and the
//! key itself does not move.

use std::sync::Mutex;

use xkbcommon::xkb;
use xkbcommon::xkb::Keysym;

use crate::i18n;

/// The character rows a board with no layout to read shows.
///
/// The fallback and not the board: what is drawn until the system's own layout
/// has been compiled, and what is drawn if it will not compile.
const NUMBER_ROW: (&str, &str) = ("`1234567890-=", "~!@#$%^&*()_+");
const UPPER_ROW: (&str, &str) = ("qwertyuiop[]", "QWERTYUIOP{}");
const HOME_ROW: (&str, &str) = ("asdfghjkl;'", "ASDFGHJKL:\"");
const LOWER_ROW: (&str, &str) = ("zxcvbnm,./", "ZXCVBNM<>?");

/// The X11 keycode of every character key on the board, by row.
///
/// What makes the caps follow the layout: a keymap answers "what does this key
/// produce" about a *keycode*, so the board's ANSI positions have to be named
/// in the only language xkb has for them. X11's numbering, which is evdev's
/// plus eight.
///
/// The counts are fixed here — thirteen, thirteen, eleven, ten — which is what
/// stops a layout changing the width of a row. Every row but the function row
/// comes to exactly [`COLUMNS`], and a keymap has no say in that.
const KEYCODES: [&[u32]; 4] = [
    // <TLDE> and <AE01>..<AE12>
    &[49, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21],
    // <AD01>..<AD12>, then <BKSL> — which the row draws last and wider.
    &[24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 51],
    // <AC01>..<AC11>
    &[38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48],
    // <AB01>..<AB10>
    &[52, 53, 54, 55, 56, 57, 58, 59, 60, 61],
];

/// What this machine's keyboards are set to, as caps this board can print.
///
/// `None` until [`note_layout`] has been called and succeeded, which is also
/// where it stays if the layout will not compile. A static rather than a field
/// on [`Board`] because it is a fact about the machine: reading a keymap is a
/// file opened and a grammar parsed, and the board is built fresh every time
/// somebody asks for it.
static CAPS: Mutex<Option<Arrangement>> = Mutex::new(None);

/// The caps of the four character rows, in [`KEYCODES`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Arrangement {
    rows: [Vec<Cap>; 4],
    /// Whether anything on it is reached with AltGr, which decides whether the
    /// board draws that key at all. An American board has no AltGr, and one
    /// that drew a dead key in the bottom row would be a key doing nothing on
    /// the layout most people use.
    altgr: bool,
}

/// Read the layout this machine is configured for, and keep it.
///
/// Called once at startup. Answers whether anything was read — `false` leaves
/// the ANSI/US fallback, which is what a machine with no xkeyboard-config or an
/// unreadable configuration gets.
pub fn note_layout(layout: &str, variant: &str) -> bool {
    let read = read_arrangement(layout, variant);
    if read.is_none() {
        tracing::warn!(
            layout,
            variant,
            "that keyboard layout would not compile; the board keeps the US arrangement"
        );
    }
    let read_anything = read.is_some();
    *CAPS.lock().unwrap() = read;
    read_anything
}

/// What this machine's keyboard is set to, as an xkb layout and variant.
///
/// Two places, in the order a login screen should trust them.
///
/// `XKB_DEFAULT_LAYOUT` first, because it is what libxkbcommon itself honours
/// and what a compositor that has been told a layout exports for its children —
/// under LineXinBar that is the answer chosen in Settings > Input > Keyboard,
/// so the login screen and the session agree without either knowing about the
/// other.
///
/// Then the system's own X11 keyboard configuration, which is what
/// `localectl set-x11-keymap` writes and what every session on this machine
/// starts from. Nobody has logged in yet when this is read, so there is no
/// session to ask and no user configuration that could be meant instead.
///
/// Neither, and the answer is `us` — which is not a guess about the machine but
/// the same default libxkbcommon has.
pub fn system_layout() -> (String, String) {
    if let Ok(layout) = std::env::var("XKB_DEFAULT_LAYOUT") {
        if !layout.trim().is_empty() {
            let variant = std::env::var("XKB_DEFAULT_VARIANT").unwrap_or_default();
            return (layout.trim().to_string(), variant.trim().to_string());
        }
    }
    configured_layout().unwrap_or_else(|| ("us".to_string(), String::new()))
}

/// The layout and variant one settings key names.
///
/// LineXinBar keeps both halves of the answer in one key — `pl (qwertz)`, or a
/// bare `pl` where the layout has no variant — because a layout and a variant of
/// it are two halves of one choice, and that is how xkeyboard-config's own
/// registry names the pair. This reads it tolerantly, because the key can be
/// typed by hand: the bracket may never be closed and the spacing is not part of
/// the answer.
///
/// `None` for a key with no layout in it, which is a keymap nothing could
/// compile.
pub fn layout_key(key: &str) -> Option<(String, String)> {
    let (layout, variant) = match key.trim().split_once('(') {
        Some((layout, rest)) => (layout.trim(), rest.trim_end().trim_end_matches(')').trim()),
        None => (key.trim(), ""),
    };
    (!layout.is_empty()).then(|| (layout.to_string(), variant.to_string()))
}

/// The layout out of the system's X11 keyboard configuration.
///
/// Both the directory `localectl` writes into and the single file some
/// distributions ship instead. Only the first `XkbLayout` is taken, and only
/// the first of a comma-separated list: xkb can hold several layouts at once
/// with a key to switch between them, and this board has no such key — so it
/// offers the one the machine comes up in rather than pretending to offer all
/// of them.
fn configured_layout() -> Option<(String, String)> {
    let files = [
        "/etc/X11/xorg.conf.d/00-keyboard.conf",
        "/etc/X11/xorg.conf.d/90-keyboard.conf",
        "/etc/X11/xorg.conf",
    ];
    for path in files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let option = |name: &str| {
            text.lines()
                .filter_map(|line| {
                    let line = line.trim();
                    let rest = line.strip_prefix("Option")?.trim_start();
                    let rest = rest.strip_prefix(&format!("\"{name}\""))?.trim_start();
                    Some(rest.trim_matches('"').split(',').next()?.trim().to_string())
                })
                .find(|value| !value.is_empty())
        };
        if let Some(layout) = option("XkbLayout") {
            return Some((layout, option("XkbVariant").unwrap_or_default()));
        }
    }
    None
}

/// Compile a layout and read the four character rows off it.
///
/// The four faces the board can show are xkb's first four shift levels, which
/// is what a keyboard's four-level type is: plain, Shift, AltGr, and both.
///
/// A key with nothing on a level gets nothing rather than falling back to its
/// plain character. A cap that showed `a` on the AltGr face and typed `a` when
/// pressed would be a key ignoring the modifier the user is holding, and there
/// would be no way to tell it from one that does something.
fn read_arrangement(layout: &str, variant: &str) -> Option<Arrangement> {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_names(
        &context,
        "",
        "",
        layout,
        variant,
        None,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )?;
    let mut rows: [Vec<Cap>; 4] = Default::default();
    let mut altgr = false;
    for (row, keycodes) in KEYCODES.iter().enumerate() {
        for keycode in *keycodes {
            let cap = cap_of(&keymap, *keycode);
            altgr |= cap.has_altgr();
            rows[row].push(cap);
        }
    }
    // A keymap that compiled but says nothing about the alphabet is not an
    // arrangement — it is a layout this board cannot show, and the US fallback
    // is a better board than one with a blank home row.
    rows[2]
        .iter()
        .any(|cap| cap.at(Level::Plain).is_some())
        .then_some(Arrangement { rows, altgr })
}

/// What one key of the keymap types on each of the board's four faces.
fn cap_of(keymap: &xkb::Keymap, keycode: u32) -> Cap {
    let key = xkb::Keycode::new(keycode);
    let mut levels = [None; 4];
    for (index, slot) in levels.iter_mut().enumerate() {
        // One keysym or none. A level bound to several — which xkb allows and
        // almost nothing uses — is one keycap's worth of typing here.
        *slot = keymap
            .key_get_syms_by_level(key, 0, index as u32)
            .first()
            .copied()
            .and_then(stroke_of);
    }
    Cap { levels }
}

/// One keysym as this board would type it.
///
/// A character where it has one — which is nearly all of them — and the keysym
/// itself where it has not. The second is the dead keys: `dead_acute` types no
/// character, it changes what the *next* key types, and what a board can do
/// with it is show the accent and pass it on.
fn stroke_of(keysym: Keysym) -> Option<Stroke> {
    if let Some(character) =
        char::from_u32(xkb::keysym_to_utf32(keysym)).filter(|c| !c.is_control())
    {
        return Some(Stroke::Char(character));
    }
    dead_mark(keysym)
        .is_some()
        .then_some(Stroke::Keysym(keysym.raw()))
}

/// The accent printed on a dead key's cap.
///
/// The one place the board cannot ask the keymap what to print, because a dead
/// key has no character. What a real keycap shows is the accent itself, and
/// that is what this is. A dead key this table does not know is left off the
/// board rather than shown blank: a cap with nothing on it is a key nobody can
/// find out the meaning of by pressing.
fn dead_mark(keysym: Keysym) -> Option<&'static str> {
    let name = xkb::keysym_get_name(keysym);
    Some(match name.strip_prefix("dead_")? {
        "grave" => "`",
        "acute" => "´",
        "circumflex" => "^",
        "tilde" | "perispomeni" => "~",
        "macron" => "¯",
        "breve" => "˘",
        "abovedot" => "˙",
        "diaeresis" => "¨",
        "abovering" => "˚",
        "doubleacute" => "˝",
        "caron" => "ˇ",
        "cedilla" => "¸",
        "ogonek" => "˛",
        "iota" => "ͅ",
        "belowdot" => "̣",
        "hook" => "̉",
        "horn" => "̛",
        "stroke" => "̶",
        "abovecomma" | "psili" => "᾿",
        "abovereversedcomma" | "dasia" => "῾",
        "doublegrave" => "̏",
        "belowring" => "̥",
        "belowmacron" => "̱",
        "belowcircumflex" => "̭",
        "belowtilde" => "̰",
        "belowbreve" => "̮",
        "belowdiaeresis" => "̤",
        "invertedbreve" => "̑",
        "belowcomma" => "̦",
        "currency" => "¤",
        "greek" => "µ",
        _ => return None,
    })
}
const FUNCTION_KEYS: [&str; 12] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
];

pub const COLUMNS: f32 = 15.0;
pub const ROW_COUNT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A character key: what it types on each of the four faces the board can
    /// show. See [`Cap`].
    Char(Cap),
    Named(&'static str, Stroke),
    Arrow(Arrow),
    Shift,
    Caps,
    Ctrl,
    Alt,
    /// AltGr: the third and fourth faces of the board, where most layouts keep
    /// their accented letters and their currency signs.
    ///
    /// On the board only where the layout has something on those faces.
    AltGr,
    Close,
}

impl Key {
    /// A character key with a plain and a shifted character and nothing on
    /// AltGr, which is how the fallback arrangement is written.
    pub const fn letter(plain: char, shifted: char) -> Self {
        Self::Char(Cap::letter(plain, shifted))
    }

    pub fn cap(self, level: Level) -> String {
        let text = i18n::text();
        match self {
            Self::Char(cap) => cap.printed(level),
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
            // Not in [`i18n`], and it is the rule that keeps it out rather than
            // an omission: a legend stays Latin wherever that language's own
            // keyboards carry a Latin one, and every keyboard that has this key
            // has "AltGr" printed on it — German, French and Polish included.
            Self::AltGr => "AltGr".to_string(),
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

/// Which face of the board is showing.
///
/// xkb's first four shift levels, which is what a keyboard's four-level type
/// is. The board reaches them with two keys: Shift, and AltGr where the layout
/// has anything on the far two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Level {
    #[default]
    Plain,
    Shift,
    AltGr,
    AltGrShift,
}

impl Level {
    fn of(shifted: bool, altgr: bool) -> Self {
        match (altgr, shifted) {
            (false, false) => Self::Plain,
            (false, true) => Self::Shift,
            (true, false) => Self::AltGr,
            (true, true) => Self::AltGrShift,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Plain => 0,
            Self::Shift => 1,
            Self::AltGr => 2,
            Self::AltGrShift => 3,
        }
    }
}

/// What one character key types, on each face the board can show.
///
/// Four answers rather than the two a keycap is printed with, because a layout
/// keeps its accented letters on the far two: `ą` is AltGr and `a` on a Polish
/// keyboard, and a board offering only the near pair is a board a Pole cannot
/// type their own password on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cap {
    levels: [Option<Stroke>; 4],
}

impl Cap {
    /// A key with a plain and a shifted character and nothing on AltGr.
    pub const fn letter(plain: char, shifted: char) -> Self {
        Self {
            levels: [
                Some(Stroke::Char(plain)),
                Some(Stroke::Char(shifted)),
                None,
                None,
            ],
        }
    }

    fn at(self, level: Level) -> Option<Stroke> {
        self.levels[level.index()]
    }

    /// What is printed on it on one face.
    fn printed(self, level: Level) -> String {
        match self.at(level) {
            Some(Stroke::Char(character)) => character.to_string(),
            // The accent a dead key carries, which is what a real keycap shows.
            Some(Stroke::Keysym(raw)) => dead_mark(Keysym::new(raw)).unwrap_or("").to_string(),
            Some(Stroke::Named(name)) => name.to_string(),
            None => String::new(),
        }
    }

    fn has_altgr(self) -> bool {
        self.levels[2].is_some() || self.levels[3].is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    Char(char),
    Named(&'static str),
    /// A keysym with no character of its own that the layout put on the
    /// alphabet: the dead keys, which is how a French or a German keyboard
    /// reaches its accented letters.
    ///
    /// This board types into its own field rather than through a keymap, so a
    /// dead key here has nothing to compose with and is dropped by the caller.
    /// It is on the board all the same: a key that is on the keyboard and
    /// missing from the picture of it is a picture that is wrong.
    Keysym(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Type(Stroke),
    Shifted,
    /// The key has nothing on the face the board is showing, so the press did
    /// nothing at all — the armed modifier included, because the user is still
    /// reaching for the key it was armed for.
    Nothing,
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
    let mut keys = Vec::new();
    match row {
        0 => {
            let span = COLUMNS / 13.0;
            keys.push((Key::Named("Esc", Stroke::ESCAPE), span));
            keys.extend(FUNCTION_KEYS.map(|name| (Key::Named(name, Stroke::Named(name)), span)));
        }
        1 => {
            keys.extend(caps_in(row).into_iter().map(|cap| (Key::Char(cap), 1.0)));
            keys.push((Key::Named("Back", Stroke::BACKSPACE), 2.0));
        }
        2 => {
            keys.push((Key::Named("Tab", Stroke::TAB), 1.5));
            // The backslash is the row's last key and is drawn wider, which is
            // where ANSI puts it. It is a character key like the twelve before
            // it, so the layout has its say about what it prints — on a German
            // keyboard that position is `#`.
            let caps = caps_in(row);
            let (letters, wide) = caps.split_at(caps.len().saturating_sub(1));
            keys.extend(letters.iter().map(|cap| (Key::Char(*cap), 1.0)));
            keys.extend(wide.iter().map(|cap| (Key::Char(*cap), 1.5)));
        }
        3 => {
            keys.push((Key::Caps, 1.75));
            keys.extend(caps_in(row).into_iter().map(|cap| (Key::Char(cap), 1.0)));
            keys.push((Key::Named("Enter", Stroke::ENTER), 2.25));
        }
        4 => {
            keys.push((Key::Shift, 2.25));
            keys.extend(caps_in(row).into_iter().map(|cap| (Key::Char(cap), 1.0)));
            keys.push((Key::Shift, 2.75));
        }
        _ => {
            keys.push((Key::Ctrl, 1.5));
            keys.push((Key::Alt, 1.5));
            // AltGr takes a key and a half out of the space bar, and only where
            // the layout has something on the faces it reaches. Right of the
            // space bar, which is where a keyboard that has one puts it.
            match altgr_on_the_board() {
                true => {
                    keys.push((Key::Named("Space", Stroke::SPACE), 4.0));
                    keys.push((Key::AltGr, 1.5));
                }
                false => keys.push((Key::Named("Space", Stroke::SPACE), 5.5)),
            }
            keys.extend(
                [Arrow::Left, Arrow::Down, Arrow::Up, Arrow::Right]
                    .map(|arrow| (Key::Arrow(arrow), 1.0)),
            );
            keys.push((Key::Close, 2.5));
        }
    }
    keys
}

/// The caps of one character row: the layout's, or the ANSI/US fallback where
/// nothing has said what the layout is.
///
/// `row` is the board's own row number — 1 to 4 — which is [`KEYCODES`]'s index
/// plus one.
fn caps_in(row: usize) -> Vec<Cap> {
    if let Some(held) = CAPS.lock().unwrap().as_ref() {
        if let Some(caps) = row.checked_sub(1).and_then(|index| held.rows.get(index)) {
            return caps.clone();
        }
    }
    let (plain, shifted) = match row {
        1 => NUMBER_ROW,
        2 => UPPER_ROW,
        3 => HOME_ROW,
        _ => LOWER_ROW,
    };
    let mut caps: Vec<Cap> = plain
        .chars()
        .zip(shifted.chars())
        .map(|(plain, shifted)| Cap::letter(plain, shifted))
        .collect();
    // The backslash, which the fallback rows above do not carry because it is
    // the one character key drawn at a width of its own.
    if row == 2 {
        caps.push(Cap::letter('\\', '|'));
    }
    caps
}

/// Whether the board draws an AltGr key at all.
fn altgr_on_the_board() -> bool {
    CAPS.lock().unwrap().as_ref().is_some_and(|held| held.altgr)
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
    altgr: Latch,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            row: 3,
            column: 1,
            shift: Latch::Off,
            ctrl: Latch::Off,
            alt: Latch::Off,
            altgr: Latch::Off,
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

    /// Which face the board is showing, which is what every cap on it says and
    /// what the next press will type.
    pub fn level(&self) -> Level {
        Level::of(self.shift.is_on(), self.altgr.is_on())
    }

    pub fn latched(&self, key: Key) -> Latch {
        match key {
            Key::Shift => self.shift,
            Key::Ctrl => self.ctrl,
            Key::Alt => self.alt,
            Key::AltGr => self.altgr,
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
            Key::Char(cap) => {
                // Read before spending: it is this press the armed shift is for.
                match cap.at(self.level()) {
                    Some(stroke) => {
                        self.spend();
                        Press::Type(stroke)
                    }
                    // Nothing on this face, so nothing happens — the latches
                    // included. A blank cap that spent the AltGr the user had
                    // just armed would take the modifier away for the key they
                    // were actually reaching for.
                    None => Press::Nothing,
                }
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
            Key::AltGr => {
                self.altgr = self.altgr.pressed();
                Press::Shifted
            }
            Key::Close => Press::Close,
        }
    }

    /// Press Enter without the cursor being on it — the board's half of what
    /// Start does. See `Application::submit_board`.
    ///
    /// It spends the latches exactly as pressing the key itself would: a Shift
    /// armed for the next keystroke is armed for this one. The cursor is
    /// deliberately left where it was, because the board is going away and
    /// where it stood is where it should come back.
    pub fn submit(&mut self) -> Press {
        self.spend();
        Press::Type(Stroke::ENTER)
    }

    fn spend(&mut self) {
        self.shift = self.shift.spent();
        self.ctrl = self.ctrl.spent();
        self.alt = self.alt.spent();
        self.altgr = self.altgr.spent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test at a time, because the caps are a fact about the *machine*.
    ///
    /// [`CAPS`] is a static — reading a keymap is a file opened and a grammar
    /// parsed, and a board built fresh for every prompt could not afford it —
    /// so a test that sets a layout changes what every other test's board is
    /// made of.
    static LOCK: Mutex<()> = Mutex::new(());

    /// Hold the lock, and put the machine's arrangement back however the test
    /// ends. A panicking test must not leave a Polish keyboard behind.
    struct Held(
        #[allow(dead_code)] std::sync::MutexGuard<'static, ()>,
        Option<Arrangement>,
    );

    impl Drop for Held {
        fn drop(&mut self) {
            *CAPS.lock().unwrap() = self.1.take();
        }
    }

    fn alone() -> Held {
        let held = LOCK.lock().unwrap_or_else(|held| held.into_inner());
        let caps = CAPS.lock().unwrap().clone();
        Held(held, caps)
    }

    /// Whether this machine's xkeyboard-config can compile a layout, which a
    /// build container may not have.
    fn has(layout: &str, variant: &str) -> bool {
        read_arrangement(layout, variant).is_some()
    }

    /// What one row of the board says, key by key, on its plain face.
    fn printed(row: usize) -> Vec<String> {
        row_spans(row)
            .into_iter()
            .map(|(key, _)| key.cap(Level::Plain))
            .collect()
    }

    #[test]
    fn every_full_row_is_exactly_ansi_width() {
        let _held = alone();
        for row in 1..ROW_COUNT {
            let total: f32 = row_spans(row).iter().map(|(_, span)| span).sum();
            assert!((total - COLUMNS).abs() < f32::EPSILON, "row {row}: {total}");
        }
    }

    #[test]
    fn board_starts_on_the_home_rows_first_letter() {
        let _held = alone();
        let board = Board::default();
        assert_eq!(board.selected(), (3, 1));
        assert_eq!(row_spans(3)[1].0, Key::letter('a', 'A'));
    }

    /// What the board types is the board's, and what it *says* is the
    /// language's. A cap that moved a letter would be a keyboard that lies.
    #[test]
    fn the_letters_are_the_boards_and_the_words_are_the_languages() {
        let _held = alone();
        for language in i18n::ALL {
            i18n::with_language(language, || {
                for row in 0..ROW_COUNT {
                    for (key, _) in row_spans(row) {
                        let Key::Char(cap) = key else {
                            continue;
                        };
                        // What the cap says is what the key types, on every
                        // face — which is the whole of the rule, and the reason
                        // the layout may change the letters while the language
                        // may not.
                        for level in [Level::Plain, Level::Shift, Level::AltGr] {
                            let printed = key.cap(level);
                            match cap.at(level) {
                                Some(Stroke::Char(character)) => {
                                    assert_eq!(printed, character.to_string())
                                }
                                Some(_) => assert!(!printed.is_empty()),
                                None => assert!(printed.is_empty()),
                            }
                        }
                    }
                }
                // The function row is `F1` on every keyboard ever sold.
                assert_eq!(row_spans(0)[1].0.cap(Level::Plain), "F1");
            });
        }
        i18n::with_language(i18n::Language::German, || {
            assert_eq!(Key::Ctrl.cap(Level::Plain), "Strg");
            assert_eq!(Key::Shift.cap(Level::Plain), "Umschalt");
            assert_eq!(
                Key::Named("Enter", Stroke::ENTER).cap(Level::Plain),
                "Enter"
            );
        });
        i18n::with_language(i18n::Language::French, || {
            assert_eq!(
                Key::Named("Enter", Stroke::ENTER).cap(Level::Plain),
                "Entrée"
            );
            assert_eq!(
                Key::Named("Space", Stroke::SPACE).cap(Level::Plain),
                "Espace"
            );
        });
        // A Russian keyboard has `Shift` written on it. A board that said
        // anything else would be a board nobody has ever seen.
        i18n::with_language(i18n::Language::Russian, || {
            assert_eq!(Key::Shift.cap(Level::Plain), "Shift");
            assert_eq!(Key::Ctrl.cap(Level::Plain), "Ctrl");
        });
    }

    /// The caps come off the layout, and the keys stay where they are.
    ///
    /// German is the clearest pair to check both halves at once: it is QWERTZ,
    /// so the key ANSI prints Y types z and the key it prints Z types y — the
    /// letters swapped and the keys exactly where they were.
    #[test]
    fn the_caps_come_off_the_layout_and_the_keys_stay_where_they_are() {
        let _held = alone();
        if !has("de", "") {
            return;
        }
        assert!(note_layout("de", ""));
        // The lower row is a Shift, ten characters and a Shift.
        assert_eq!(
            printed(4)[1..11],
            ["y", "x", "c", "v", "b", "n", "m", ",", ".", "-"]
        );
        // The upper row is Tab and then the letters.
        assert_eq!(printed(2)[1..7], ["q", "w", "e", "r", "t", "z"]);
        // And every row is still exactly the width of the grid: a layout may
        // say what the keys print, not how many there are.
        for row in 1..ROW_COUNT {
            let total: f32 = row_spans(row).iter().map(|(_, span)| span).sum();
            assert!((total - COLUMNS).abs() < 1e-4, "row {row}: {total}");
        }
    }

    /// A layout that keeps letters behind AltGr grows the key that reaches
    /// them, and pressing it changes what the caps say.
    ///
    /// Polish is the case this exists for: it is QWERTY, so without AltGr the
    /// board would look right and be unable to write a single Polish word — or
    /// a Polish password.
    #[test]
    fn a_layout_with_letters_behind_altgr_grows_the_key_that_reaches_them() {
        let _held = alone();
        if !has("pl", "") {
            return;
        }
        assert!(note_layout("pl", ""));

        let bottom: Vec<Key> = row_spans(ROW_COUNT - 1)
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        let altgr = bottom
            .iter()
            .position(|key| *key == Key::AltGr)
            .expect("a layout with a third level has the key for it");

        let mut board = Board::default();
        assert_eq!(board.press(), Press::Type(Stroke::Char('a')));
        assert!(board.select(ROW_COUNT - 1, altgr));
        assert_eq!(board.press(), Press::Shifted);
        assert_eq!(board.level(), Level::AltGr);
        assert!(board.select(3, 1));
        assert_eq!(row_spans(3)[1].0.cap(Level::AltGr), "ą");
        assert_eq!(board.press(), Press::Type(Stroke::Char('ą')));
        // Spent by the key it was armed for, like every other latch.
        assert_eq!(board.level(), Level::Plain);
    }

    /// An American board has no AltGr key, because there it would do nothing.
    #[test]
    fn a_layout_with_nothing_on_the_far_faces_draws_no_altgr() {
        let _held = alone();
        if !has("us", "") {
            return;
        }
        assert!(note_layout("us", ""));
        let bottom: Vec<Key> = row_spans(ROW_COUNT - 1)
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert!(!bottom.contains(&Key::AltGr));
        assert_eq!(
            bottom.len(),
            8,
            "Ctrl, Alt, Space, four arrows and the way out"
        );
    }

    /// A key with nothing on the face the board is showing types nothing, and
    /// does not spend the modifier armed for the key next to it.
    ///
    /// Against a built arrangement rather than a real layout: which keys a
    /// layout leaves blank is xkeyboard-config's business and changes with the
    /// package — Polish, the obvious candidate, includes `latin` and so has
    /// something on AltGr for every key.
    #[test]
    fn a_blank_cap_types_nothing_and_keeps_the_modifier_it_was_armed_with() {
        let _held = alone();
        let mut rows: [Vec<Cap>; 4] = Default::default();
        for (index, keycodes) in KEYCODES.iter().enumerate() {
            rows[index] = keycodes.iter().map(|_| Cap::letter('a', 'A')).collect();
        }
        rows[2][0] = Cap {
            levels: [
                Some(Stroke::Char('a')),
                Some(Stroke::Char('A')),
                Some(Stroke::Char('ą')),
                None,
            ],
        };
        *CAPS.lock().unwrap() = Some(Arrangement { rows, altgr: true });

        let bottom: Vec<Key> = row_spans(ROW_COUNT - 1)
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        let altgr = bottom.iter().position(|key| *key == Key::AltGr).unwrap();
        let mut board = Board::default();
        assert!(board.select(ROW_COUNT - 1, altgr));
        assert_eq!(board.press(), Press::Shifted);

        // The neighbour has nothing on this face, and the press changes
        // nothing at all — the armed AltGr included, because the user is still
        // reaching for the key it was armed for.
        assert!(board.select(3, 2));
        assert_eq!(row_spans(3)[2].0.cap(Level::AltGr), "");
        assert_eq!(board.press(), Press::Nothing);
        assert_eq!(board.level(), Level::AltGr);
        assert!(board.select(3, 1));
        assert_eq!(board.press(), Press::Type(Stroke::Char('ą')));
        assert_eq!(board.level(), Level::Plain);
    }

    /// A layout that will not compile leaves the board on its own ANSI
    /// arrangement rather than on a guess.
    #[test]
    fn a_layout_that_will_not_compile_leaves_the_ansi_board() {
        let _held = alone();
        assert!(!note_layout("no-such-layout-anywhere", ""));
        assert!(CAPS.lock().unwrap().is_none());
        assert_eq!(printed(3)[1..12].concat(), HOME_ROW.0);
        let bottom: Vec<Key> = row_spans(ROW_COUNT - 1)
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert!(!bottom.contains(&Key::AltGr));
    }

    /// Both halves of the answer out of the one key the shell writes them in.
    #[test]
    fn one_settings_key_holds_a_layout_and_its_variant() {
        assert_eq!(
            layout_key("pl (qwertz)"),
            Some(("pl".to_string(), "qwertz".to_string()))
        );
        assert_eq!(layout_key("us"), Some(("us".to_string(), String::new())));
        // Typed by hand, and still an answer.
        assert_eq!(
            layout_key("  de ( neo  "),
            Some(("de".to_string(), "neo".to_string()))
        );
        // Not an answer: there is no keymap named nothing.
        assert_eq!(layout_key(""), None);
        assert_eq!(layout_key("   "), None);
        assert_eq!(layout_key("(qwertz)"), None);
    }

    /// The environment libxkbcommon itself honours comes before the system's
    /// own configuration — under LineXinBar that variable carries the layout
    /// chosen in Settings, so the login screen and the session agree.
    #[test]
    fn the_environment_is_asked_before_the_system_configuration() {
        let _held = alone();
        // SAFETY: the module lock is held, so nothing else in this binary is
        // reading the environment while it is changed and put back.
        unsafe {
            std::env::set_var("XKB_DEFAULT_LAYOUT", "pl");
            std::env::set_var("XKB_DEFAULT_VARIANT", "qwertz");
        }
        assert_eq!(system_layout(), ("pl".to_string(), "qwertz".to_string()));
        // Empty is not an answer: it is a variable somebody exported blank, and
        // the machine's own configuration is a better guess than "us".
        unsafe {
            std::env::set_var("XKB_DEFAULT_LAYOUT", "  ");
            std::env::remove_var("XKB_DEFAULT_VARIANT");
        }
        let fallen_back = system_layout();
        assert_ne!(fallen_back.0, "  ");
        assert!(!fallen_back.0.is_empty());
        unsafe {
            std::env::remove_var("XKB_DEFAULT_LAYOUT");
        }
    }
}
