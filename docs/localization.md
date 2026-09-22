# Localization

The greeter speaks ten languages: English (UK), English (US), French, German,
Hindi, Polish, Brazilian Portuguese, Russian, Spanish and Simplified Chinese.
Every word is in `src/i18n.rs`, once per language, compiled into the binary.

**The two Englishes share one `Strings`**, and that is the one place in the
registry where two languages do. They differ over the order of a date and a
handful of spellings; this screen writes neither — its date line is a weekday
and a day of the month, `Mon 17`, with no month name in it. A second constant
for American English would be a copy of the English one with nothing changed in
it, which is the one thing a translation must never be, and
`no_catalogue_was_left_in_english` exempts the pair by name rather than by
accident.

It is a language of its own all the same, because two things about a machine
really do turn on it: which of the two clocks a time is written on where the
account has not chosen (see "The clock" below), and which `Name[…]` a session's
desktop entry is read under — `Name[en_US]` before `Name[en]`.

It does **not** use the Fluent catalogs the shell and the toolkit use, and is
not going to: a language here is a `Strings` struct, so one that forgot a
sentence does not build — a completeness check a catalog cannot give — and a
login screen is the one program on the machine that cannot fall back to a
message directory it failed to find. The sentences it says are fixed and few,
and none of them counts anything, which is the part a catalog would be needed
for.

`ALL` in `src/i18n.rs` is the registry every other part reads: the internal
numeric codes are its positions rather than a second table written by hand,
and a test walks it for unique tags, round trips and an endonym.

Language selection follows `--language`, the administrator's
`/etc/cedm/config.toml`, the greeter's own `LC_ALL` / `LC_MESSAGES` / `LANG`,
the machine's locale files, then English — the README's "Where the language
comes from" has the whole of it. Unlike an application somebody has signed in
to run, the greeter reads `C` and `POSIX` as *nothing said* rather than as a
choice of English, and goes on to the locale files; a service that starts
before anybody has logged in inherits those names routinely.

That is what makes the login screen follow the desktop. LineXinBar's Settings >
Language sets the machine's locale through `systemd-localed`, which writes
`/etc/locale.conf`; the next greeter to start reads it. Nothing is shared
between the two beyond that file.

Use `cedm --preview --windowed --language pl` for Polish, `--language en-GB`
or `--language en-US` for the two Englishes. Preview mode never authenticates
or starts a desktop session. `--shot out.png --size 1280x720 --language en-US`
renders one frame with no seat, which is how the twelve-hour clock was looked
at.

## The clock

The hour on the right of the screen is `20:38` or `8:38 PM`, according to
Settings > System > Clock in the account the selection is standing on. It
reaches this screen the way the accent and the material do: through the copy of
`shell.toml` that account published on its way into its last session, as
`clock = "24-hour"` or `"12-hour"` (`cedm::look::Look::clock`). A published look
that says nothing — every one written before the shell had the row — leaves the
account's **language** to answer, and two of the ten write AM and PM: English
(US) and हिन्दी, the clock America and India both read. That is CLDR's preferred
hour cycle for each, and the same pair the shell and lxb-toolkit answer for, so
a machine nobody has set writes the same time here, on the start screen and in
an application. Moving along the carousel moves the clock with the accent,
because the screen is the account's.

AM and PM are the same two marks in every language here, so they are not in
`Strings`. They are the reason the clock's alphabet has letters in it at all:
`crate::visual::letters::SET` went from eleven characters to fifteen — the ten
digits, the colon, a space that moves the pen and draws nothing, and the `A`,
`M` and `P`. A time holding anything outside that set is drawn as no clock,
which is why `every_clock_is_written_out_of_the_alphabet_the_greeter_ships`
walks both clocks through all twenty-four hours, and why
`the_widest_time_either_clock_writes_fits_beside_the_column` measures the
longest of them against the room the layout gives it at every size.

## What the buttons do

The row in the bottom-right corner of the wallpaper says `Select`, `Keyboard`
and `Back`, and those three are the greeter's own words about its own buttons —
`hint_select`, `hint_keyboard`, `hint_back` in `Strings`. They are translated in
every language, unlike the word caps of the on-screen keyboard beside them: a
cap is a picture of a key with a legend printed on it and stays Latin wherever
the physical keyboards do, and there is no keyboard anywhere for these to match.

Each of the three is `lxb-desktop`'s own word for the same act — `shell-select`,
`shell-keyboard`, `shell-back` — and has to stay so. A legend is a promise, and
the login screen and the start screen one press later are either side of a
handover; a hand that has just read `Wybierz` here must read `Wybierz` there.
Take a new one from the shell's `locales/*.ftl` rather than translating it
again.

The row is laid out from its right-hand end leftwards and every word is set
right-aligned in a box estimated at 0.66 em a character (a full em for Han),
because nothing can measure a run before the renderer shapes it. A word wider
than its estimate wraps onto a line the box has no room for, which is half a
word on screen: `no_translation_overflows_the_place_it_is_written` walks all ten
languages against it, and `the_legend_stays_inside_the_corner_it_is_written_in`
walks them again against the corner the row has to fit in.

**A new Chinese word needs the font cut again.** `NotoSansCJKsc-{Regular,Bold}`
are subset to the characters this program's own words are made of, so a word
with a character that was not there before rasterises as nothing at all. The
three words above cost five: `选`, `择`, `键`, `盘`, `返`. Re-cut both faces from
a full Noto Sans CJK SC over the union of what they already carry and what is
being added — `every_shipped_word_can_be_drawn_by_a_shipped_face` loads them
into an empty database and is what catches a character that was missed.

## Adding a language

1. Add a `Language` variant and include it in `ALL` in `src/i18n.rs`.
2. Supply its tag, endonym and desktop-entry locale keys in the corresponding
   exhaustive matches, and recognize its locale in `from_locale`.
3. Copy the English `Strings` constant, translate all fields, and select it in
   `Language::strings`. Missing fields are compile errors. A *regional* variant
   of a language already here shares that language's constant, the way English
   (US) shares English's — a copy with nothing changed in it is not a
   translation. Keep PAM prompt
   matching separate from the localized display response.
4. Run `cargo test` and `cargo fmt --all -- --check`. Tests verify all catalogs
   are populated, common prompts, locale precedence, registry integrity and
   text measurements. Preview at supported display sizes to check the font and
   available space. Add font coverage when introducing a new script.

Desktop session names come from localized desktop-entry keys. Unknown PAM
prompts and service diagnostics are preserved, so authentication-specific
information is not lost. The greeter's own lost-conversation messages use the
existing localized `attempt_lost` string.
