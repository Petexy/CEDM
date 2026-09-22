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

Use `cedm --demo --windowed --language pl` for Polish, `--language en-GB` or
`--language en-US` for the two Englishes. Preview mode never authenticates or
starts a desktop session. `--demo --shot out.png --size 1280x720 --language
en-US` renders one frame with no seat, which is how the twelve-hour clock was
looked at.

## Where the language comes from

From the machine, because nobody has signed in yet to have a preference. That is
one question with a different answer on nearly every distribution, so it is
asked in order and the first answer wins:

1. `--language`, for reviewing a translation by hand.
2. `language` in `/etc/cedm/config.toml`, which is the administrator saying so
   outright — for the machine whose login screen should *not* be in the
   machine's language, such as a shared terminal in a building where the desks
   were set up in one language and the people signing in read another.
3. `LC_ALL`, `LC_MESSAGES`, `LANG` in the greeter's own environment, in POSIX's
   order of precedence.
4. The machine's locale file, in this order, first value found:
   `/etc/locale.conf` (systemd — Arch, Fedora, openSUSE), `/etc/default/locale`
   (Debian, Ubuntu), `/etc/sysconfig/i18n` (older Red Hat and SUSE),
   `/etc/env.d/02locale` and `/etc/conf.d/locale` (Gentoo), and
   `/etc/environment` (PAM's environment file, which some installers put `LANG`
   into as well). All six are `KEY=value` lines, which is why one parser serves
   them; they are read, never run.
5. English (UK).

**The first file that *answers* wins rather than the first that exists**,
because a machine can carry two of them: an empty `/etc/locale.conf` beside a
populated `/etc/default/locale` is an ordinary Debian, and stopping at the empty
one would read the machine as having no language at all.

**An environment that says `C` or `POSIX` is treated as *nothing said*** rather
than as a choice of English. That is what those names mean — no locale has been
selected — and a greeter started by a system unit inherits them routinely;
reading them as an answer is how a login screen ends up ignoring the very file
the machine's language is written in. It is verifiable from the journal, which
names both the language and where the answer came from:

```text
the login screen speaks the language this machine is set to
    language=pl name=polski locale=pl_PL.UTF-8 from=/etc/default/locale
```

A locale names a language, and it names a **country** only where two of these
ten answer to one language, which is exactly `en_US`. So `en_US` is English
(US), and every other English — `en_GB`, `en_AU`, a bare `en` — is English (UK),
the English this greeter is written in. It changes nothing for Portuguese or
Chinese: Brazilian Portuguese is the Portuguese that was written and Simplified
Chinese is the Chinese that was written, so `pt_PT` and `zh_TW` find no country
of their own and land on those, which is a better answer for a reader of the
other variant than English is.

That is also how the login screen follows the desktop. LineXinBar's Settings ▸
Language sets the machine's locale through `systemd-localed`, which writes
`/etc/locale.conf` — the first file in the list above. Nothing has to be told;
the next greeter to start reads it, and nothing is shared between the two beyond
that file. An administrator who wants the login screen in a different language
from the machine says so in `/etc/cedm/config.toml`, which outranks it.

## What is not translated, and why

Two kinds of writing, both for the same reason: they are somebody else's words.

- **A session's name** is the desktop's own, and comes from that desktop entry's
  own `Name[xx]` keys — nothing here could know that GNOME's Polish entry says
  "GNOME na Xorgu". The country-qualified spelling is preferred over the bare
  language, so `Name[pt_BR]` beats `Name[pt]`, and an entry with no translation
  for the reader's language falls back to its unqualified `Name`.
- **A failure the login service reports.** A socket that would not open or a
  session that would not start is greetd's news about this machine, and its
  sentence is the only description of it anybody has; replacing it with a
  translated generality would throw the news away in order to say something in
  the right language. The greeter's own refusals — a wrong password, an action
  polkit declined — are translated, because those are the greeter's to write.

PAM sits between the two. It localises its own conversation, but only where the
process holding it has a language to localise into, and that process is greetd —
a system unit whose environment is whatever the unit gives it. What comes back
over the socket on nearly every machine is therefore `Password:`, in the middle
of a column that is otherwise entirely in Polish, so that one question and the
two or three beside it are translated here. Anything else PAM asks is passed
through exactly as it arrived: an unrecognised prompt is a module with something
specific to say — a hardware token, a one-time code, an expiring password — and
a greeter that guessed at those would be answering a question nobody asked.

## The keyboard and the fonts

**The on-screen board's character keys are not translated and will not be.** It
is a picture of an ANSI keyboard and the letters printed on it are the letters
it types; a board whose caps said one thing and typed another would be worse in
every language. What the machine's language does decide is the handful of caps
that are *words*, and the rule there is that a legend stays Latin wherever that
language's own keyboards carry Latin legends — a Russian keyboard has `Shift`
written on it, so the board does too, while French, German, Spanish and
Portuguese boards say `Maj`, `Umschalt`, `Mayús` and `Shift`.

The clock's second line is a pattern rather than a weekday with a number after
it, because the languages disagree about the order and about whether the number
is marked: `Mon 17`, `Mo 17.`, `пн 17`, `17日 周一`. The line above it — the hour
itself — is `HH:MM` in every language, which is what allows it to be drawn out of
measured shapes instead of shaped as text; see
[the clock](design.md#the-clock).

Roboto carries Latin, Latin Extended, Greek and Cyrillic — nine of the ten — and
no Devanagari and no Han at all, so two Noto faces are bundled beside it in
`assets/fonts/` and compiled into the binary with everything else. Without them
the Hindi and Chinese columns rasterise to rows of empty boxes, and nothing else
in the build would say a word about it. Devanagari is subset to the whole
script, so an account named in it draws too; the Han face is subset to the
characters this program's own words are made of, because the whole of Noto Sans
CJK is twenty megabytes. An account or a session named in Han falls back to
whatever the machine has installed — which on a machine with a Chinese desktop
is a full CJK face, and on one without is a machine with no Han names to draw.

## Two checks, because a translation is not a string

`cargo test` shapes every sentence of every language in the faces the greeter
ships, loaded into an **empty** font database — the machine's own fonts are
deliberately kept out of it, because the question is what happens on a machine
that has just been installed and has none. A character no shipped face can draw
fails the build.

Then it lays every screen out, in every language, on displays from 1280×720 to
4K, and measures each run against the box it was given. Nothing here wraps onto
the column: the reserved line under the button is one line tall at every size,
the caps of the on-screen board are the width of the keys under them, and text
laid out past its rectangle is *cut*. A sentence that does not fit is not a
longer sentence — it is half a sentence, and the half that goes missing is the
end of it. Both checks prove they have teeth by asserting that a script nothing
carries, and a sentence far too long for the line, are reported rather than
passed over.

That check is what several of the shorter sentences here are: the reserved line
holds about forty Latin characters at the tightest size the greeter is drawn at,
and where a natural translation ran past it, the sentence was written shorter
rather than the line made longer. It caught an English one too.

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
