//! What the login screen says, and which language it says it in.
//!
//! Every word this greeter writes on a screen is in this file, once per
//! language, as a `&'static str` compiled into the binary. No catalogue is
//! loaded from disk and no message is looked up by name: a login screen is the
//! one program on the machine that has to work before anything else does, and
//! a missing `.mo` file under `/usr/share/locale` would be a login screen that
//! comes up in a language nobody chose — or, with the usual gettext handling,
//! one that comes up showing the msgid. [`Strings`] is a struct, so a language
//! that forgot a sentence is a build failure rather than an English word in the
//! middle of a Polish column.
//!
//! It leaves out exactly two kinds of writing, and for the same reason both
//! times: they are somebody else's words. A desktop entry's `Name` is the
//! desktop's own — localised from the entry's own `Name[xx]` keys, see
//! [`crate::sessions`] — and a failure the login *service* reports is greetd's
//! sentence about a machine, which is the only description of it anybody has.
//! See [`Strings::service_refused`].
//!
//! ## Where the language comes from
//!
//! From the machine, because there is nobody signed in to have a preference
//! yet. That is one question with a different answer on nearly every distro,
//! so it is asked in order and the first answer wins:
//!
//! 1. `--language`, for somebody reviewing the screen by hand.
//! 2. `language` in [`crate::config::Config`], which is the administrator
//!    saying so outright.
//! 3. `LC_ALL`, `LC_MESSAGES`, `LANG` in this process's own environment, in
//!    POSIX's order of precedence.
//! 4. The files in [`FILES`], in order, first value found.
//! 5. English.
//!
//! An environment that says `C` or `POSIX` is treated as *nothing said* rather
//! than as a choice of English. That is what those names mean — no locale has
//! been selected — and a greeter started by a system unit inherits them
//! routinely. Reading them as an answer would make the login screen ignore the
//! very file the machine's language is written in.

use std::cell::Cell;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

/// The languages the login screen is written in.
///
/// A closed set, because every one of them is a catalogue compiled into this
/// binary; there is no path by which a locale name can ask for a language that
/// was never written. Anything else on the machine falls back to English.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// English as this greeter is written in it, and the language everything
    /// unrecognised falls back to.
    English,
    /// English as America writes it, which on **this** screen is the same
    /// English.
    ///
    /// The two differ over the order of a date and a handful of spellings, and
    /// the login screen writes neither: its date line is a weekday and a day
    /// of the month — `Mon 17` — with no month name in it, and no word on the
    /// screen is one of the handful. So this reads [`ENGLISH`], and a
    /// catalogue of its own would be a copy of that file with nothing changed
    /// in it, which is the one thing a translation must never be.
    ///
    /// It is a language of its own all the same, because two things about a
    /// machine really do turn on it: which of the two clocks a time is written
    /// on where nobody has chosen ([`crate::clock::Clock::FromLanguage`]), and
    /// which `Name[…]` a session's desktop entry is read under.
    AmericanEnglish,
    French,
    German,
    Hindi,
    Polish,
    /// Brazilian Portuguese. European Portuguese reads this catalogue too —
    /// see [`Language::from_locale`].
    Portuguese,
    Russian,
    Spanish,
    /// Simplified Chinese.
    Chinese,
}

/// Every language, in the order this file writes them.
pub const ALL: [Language; 10] = [
    Language::English,
    Language::AmericanEnglish,
    Language::French,
    Language::German,
    Language::Hindi,
    Language::Polish,
    Language::Portuguese,
    Language::Russian,
    Language::Spanish,
    Language::Chinese,
];

/// The files a distribution may keep the machine's locale in, in the order
/// they are asked.
///
/// There is no single answer to this. `/etc/locale.conf` is systemd's, and is
/// what Arch, Fedora and openSUSE write; Debian and Ubuntu have always kept
/// theirs in `/etc/default/locale` and still do; `/etc/sysconfig/i18n` is the
/// older Red Hat and SUSE spelling; Gentoo writes `/etc/env.d/02locale`, and
/// wrote `/etc/conf.d/locale` before that. `/etc/environment` is last because
/// it is not a locale file at all — it is PAM's environment file, which some
/// installers put `LANG` into as well.
///
/// Every one of them is `KEY=value` lines, which is the only reason one parser
/// serves all six. A machine with two of them is a machine mid-upgrade, and
/// taking the first that answers rather than the first that exists is what
/// gets it through: an empty `/etc/locale.conf` beside a populated
/// `/etc/default/locale` is an ordinary Debian.
pub const FILES: [&str; 6] = [
    "/etc/locale.conf",
    "/etc/default/locale",
    "/etc/sysconfig/i18n",
    "/etc/env.d/02locale",
    "/etc/conf.d/locale",
    "/etc/environment",
];

/// The variables that name a language, in POSIX's order of precedence.
///
/// `LC_ALL` overrides everything, `LC_MESSAGES` is the category this is
/// actually about — which language a program *talks* in — and `LANG` is the
/// default behind both. The same order is used inside the files, because the
/// files are those variables written down.
const KEYS: [&str; 3] = ["LC_ALL", "LC_MESSAGES", "LANG"];

/// The largest locale file this will read. These are three short lines on
/// every machine that has ever shipped; anything larger is not one.
const MAX_LOCALE_FILE_BYTES: u64 = 64 * 1024;

impl Language {
    /// The tag this language is named by, in configuration and in the journal.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::English => "en-GB",
            Self::AmericanEnglish => "en-US",
            Self::French => "fr",
            Self::German => "de",
            Self::Hindi => "hi",
            Self::Polish => "pl",
            Self::Portuguese => "pt-BR",
            Self::Russian => "ru",
            Self::Spanish => "es",
            Self::Chinese => "zh-CN",
        }
    }

    /// What this language calls itself. For the journal, where a line saying
    /// `hi` is a line somebody has to look up.
    pub const fn endonym(self) -> &'static str {
        match self {
            Self::English => "English (UK)",
            Self::AmericanEnglish => "English (US)",
            Self::French => "français",
            Self::German => "Deutsch",
            Self::Hindi => "हिन्दी",
            Self::Polish => "polski",
            Self::Portuguese => "português (Brasil)",
            Self::Russian => "русский",
            Self::Spanish => "español",
            Self::Chinese => "中文",
        }
    }

    /// The `Name[…]` suffixes a desktop entry may carry for this language, in
    /// the order [`crate::sessions`] prefers them.
    ///
    /// Desktop entries are keyed by POSIX locale names rather than by BCP 47,
    /// so a Brazilian entry is `Name[pt_BR]` and a Chinese one is `Name[zh_CN]`
    /// — with `zh_Hans` appearing in newer entries written to the script rather
    /// than to a country. The bare language comes last, so a translation
    /// written for the country wins over one written for the language.
    pub const fn desktop_keys(self) -> &'static [&'static str] {
        match self {
            Self::English => &["en_GB", "en"],
            Self::AmericanEnglish => &["en_US", "en"],
            Self::French => &["fr"],
            Self::German => &["de"],
            Self::Hindi => &["hi"],
            Self::Polish => &["pl"],
            Self::Portuguese => &["pt_BR", "pt"],
            Self::Russian => &["ru"],
            Self::Spanish => &["es"],
            Self::Chinese => &["zh_CN", "zh_Hans", "zh"],
        }
    }

    /// The language a locale name asks for, or `None` for one that asks for
    /// nothing this greeter is written in.
    ///
    /// Takes what a locale name actually looks like on a machine rather than
    /// what the specification says: `pt_BR.UTF-8`, `zh_CN.utf8`, `en_GB@euro`,
    /// the BCP 47 `pt-BR` that a session manager may have exported instead, and
    /// a bare `de`.
    ///
    /// The country is read **first**, and then the language on its own. That
    /// order matters for one pair and settles the rest: `en_US` is American
    /// English and every other English — `en_GB`, `en_AU`, a bare `en` — is
    /// the English this greeter is written in. It changes nothing for
    /// Portuguese or Chinese, which is the point: Brazilian Portuguese is the
    /// Portuguese that was written and Simplified Chinese is the Chinese that
    /// was written, so `pt_PT` and `zh_TW` find no country of their own in the
    /// first pass and land on those in the second, which is a better answer
    /// for a reader of the other variant than English is.
    ///
    /// `C` and `POSIX` are `None`, not English: they say that no language has
    /// been chosen, and something further down the list may know which one was.
    pub fn from_locale(locale: &str) -> Option<Self> {
        let locale = locale.trim();
        // `.UTF-8` is an encoding and `@euro` is a modifier; neither names a
        // language. The separator between language and country is `_` in a
        // POSIX locale name and `-` in a language tag.
        let head = locale
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .split(['_', '-'])
            .next()
            .unwrap_or_default();
        if head.is_empty() || head.eq_ignore_ascii_case("C") || head.eq_ignore_ascii_case("POSIX") {
            return None;
        }
        let country = locale
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .split(['_', '-'])
            .nth(1)
            .unwrap_or_default();
        ALL.into_iter()
            .find(|language| {
                language
                    .tag()
                    .split_once('-')
                    .is_some_and(|(base, theirs)| {
                        head.eq_ignore_ascii_case(base) && country.eq_ignore_ascii_case(theirs)
                    })
            })
            .or_else(|| {
                ALL.into_iter().find(|language| {
                    let tag = language.tag();
                    let tag = tag.split('-').next().unwrap_or(tag);
                    head.eq_ignore_ascii_case(tag)
                })
            })
    }

    /// Everything this language says.
    pub const fn strings(self) -> &'static Strings {
        match self {
            Self::English => &ENGLISH,
            // Deliberately the same file, and the one place in this table
            // where two languages share one — see [`Language::AmericanEnglish`].
            Self::AmericanEnglish => &ENGLISH,
            Self::French => &FRENCH,
            Self::German => &GERMAN,
            Self::Hindi => &HINDI,
            Self::Polish => &POLISH,
            Self::Portuguese => &PORTUGUESE,
            Self::Russian => &RUSSIAN,
            Self::Spanish => &SPANISH,
            Self::Chinese => &CHINESE,
        }
    }

    fn code(self) -> u8 {
        ALL.iter()
            .position(|language| *language == self)
            .expect("registered language") as u8
    }

    fn from_code(code: u8) -> Self {
        ALL.get(usize::from(code)).copied().unwrap_or(Self::English)
    }
}

/// Which language the screen is in, and how that was decided.
///
/// The second half is here because "the login screen came up in English" has
/// half a dozen causes and no symptom that tells them apart. A journal line
/// naming the file — or naming the variable that overrode the file — is the
/// difference between a five-minute answer and an afternoon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub language: Language,
    /// Where the answer came from: an option, a variable, or a path. `nothing`
    /// where the machine said nothing at all and English was the fallback.
    pub origin: &'static str,
    /// The locale name that decided it, exactly as it was written.
    pub locale: Option<String>,
}

impl Choice {
    fn found(language: Language, origin: &'static str, locale: &str) -> Self {
        Self {
            language,
            origin,
            locale: Some(locale.to_string()),
        }
    }
}

/// Work out which language this machine is set to.
///
/// `requested` is `--language` and `configured` is the administrator's
/// `language`; both are locale names or tags, and both are ignored when they
/// name a language this greeter is not written in — an unreadable option must
/// not be a login screen that refuses to come up.
pub fn detect(requested: Option<&str>, configured: Option<&str>) -> Choice {
    for (origin, value) in [("--language", requested), ("config language", configured)] {
        if let Some(language) = value.and_then(Language::from_locale) {
            return Choice::found(language, origin, value.unwrap_or_default());
        }
    }
    for key in KEYS {
        let Some(value) = std::env::var_os(key).and_then(|value| value.into_string().ok()) else {
            continue;
        };
        if let Some(language) = Language::from_locale(&value) {
            return Choice::found(language, key, &value);
        }
    }
    if let Some((path, locale, language)) = first_locale_in(&FILES) {
        return Choice::found(language, path, &locale);
    }
    Choice {
        language: Language::English,
        origin: "nothing",
        locale: None,
    }
}

/// The first of `files` that names a language this greeter is written in.
///
/// Taken apart from [`detect`] so the order of [`FILES`] can be checked on its
/// own: that order is a claim about how a machine mid-way between two
/// distributions' conventions reads, and the only way to check a claim about
/// two files is to put two files side by side. The path comes back rather than
/// the key inside it — what somebody reading the journal needs is the file to
/// go and look at, and there are six candidates for that.
fn first_locale_in<'a>(files: &[&'a str]) -> Option<(&'a str, String, Language)> {
    files.iter().find_map(|path| {
        let (_, locale) = read_locale_file(Path::new(path))?;
        let language = Language::from_locale(&locale)?;
        Some((*path, locale, language))
    })
}

/// The locale a `KEY=value` file names, and which key named it.
///
/// Shell syntax as far as these files ever use it: comments, an optional
/// `export`, and a value that may be quoted. Nothing here expands anything —
/// a locale file is read, never run, and a greeter that ran `/etc/environment`
/// would be a greeter that runs whatever is in `/etc/environment`.
fn read_locale_file(path: &Path) -> Option<(&'static str, String)> {
    let raw = read_bounded(path)?;
    let mut best: Option<(usize, String)> = None;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let Some(rank) = KEYS.iter().position(|candidate| *candidate == key) else {
            continue;
        };
        let value = unquote(value.trim());
        if value.is_empty() {
            continue;
        }
        // Earlier in `KEYS` wins, whatever order the file happens to be in.
        if best.as_ref().is_none_or(|(found, _)| rank < *found) {
            best = Some((rank, value));
        }
    }
    best.map(|(rank, value)| (KEYS[rank], value))
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        return value[1..value.len() - 1].to_string();
    }
    value.to_string()
}

/// Read a small file, or nothing.
///
/// Bounded before it is read rather than after, on the same terms as the
/// administrator's policy file: this runs as the login screen comes up, and
/// `/etc` is not somewhere a greeter takes a length on trust.
fn read_bounded(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_LOCALE_FILE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// The language every `text()` in this process answers in.
///
/// One value for the life of the program. Unlike the accent — which the
/// account being signed in decides, and which therefore moves under the
/// interface — nothing that happens at a login screen changes the language the
/// machine is set to, so this is set once before the first frame and read
/// everywhere after.
static LANGUAGE: AtomicU8 = AtomicU8::new(0);

thread_local! {
    /// The language one thread is speaking, where something has asked it to speak
    /// a particular one.
    ///
    /// Only ever set inside [`with_language`], and only ever by a test. The greeter
    /// itself sets [`LANGUAGE`] once from `main` and never touches this, so what it
    /// costs outside a test is one thread-local read for each word drawn.
    ///
    /// A thread-local and not a second global, which is the whole point: a test
    /// asking for a screen in Chinese must not put another test's screen into
    /// Chinese halfway through drawing it.
    static SPOKEN: Cell<Option<Language>> = const { Cell::new(None) };
}

/// Fix the language for this process. Called once, from `main`.
pub fn set(language: Language) {
    LANGUAGE.store(language.code(), Ordering::Relaxed);
}

pub fn language() -> Language {
    match SPOKEN.get() {
        Some(spoken) => spoken,
        None => Language::from_code(LANGUAGE.load(Ordering::Relaxed)),
    }
}

/// Everything the screen says, in the language it is set to.
pub fn text() -> &'static Strings {
    language().strings()
}

/// Run `body` with the screen speaking `language`, and put it back afterwards.
///
/// The answer is kept to the calling thread — see [`SPOKEN`] — so a sweep
/// through all ten languages is invisible to every other test running beside
/// it. It was a lock and the process-wide language once, and a lock is the
/// wrong shape for this: it stops two tests *writing* at the same time and does
/// nothing about the ones reading, which is every test that draws a screen with
/// a word on it. Those were then correct only by luck, and about once in ten
/// runs one of them was handed half a screen in somebody else's language.
pub fn with_language<T>(language: Language, body: impl FnOnce() -> T) -> T {
    let restore = Restore(SPOKEN.replace(Some(language)));
    let answer = body();
    drop(restore);
    answer
}

/// Put the thread back to whatever it was speaking, panic or no panic.
struct Restore(Option<Language>);

impl Drop for Restore {
    fn drop(&mut self) {
        SPOKEN.set(self.0);
    }
}

/// Put `value` into a sentence written around it.
///
/// One placeholder, spelled `{}`, and it may stand anywhere in the sentence —
/// which is the whole reason the sentences are written out per language rather
/// than assembled from pieces. Russian names the action in quotation marks
/// after a neuter subject because its three actions are three genders, and
/// Polish puts it at the end for the same reason; neither is reachable by
/// gluing a noun to a translated verb.
pub fn fill(template: &str, value: &str) -> String {
    template.replace("{}", value)
}

/// Every sentence the login screen writes, in one language.
///
/// A field per message rather than a map: this is a small, closed interface —
/// one screen, with one job — and a struct is what makes a language that
/// missed a message fail to compile instead of falling back to English at the
/// one moment somebody is reading it.
#[derive(Debug)]
pub struct Strings {
    // The address at the top of the column: a greeting on one line and the
    // account's own name under it, set as one sentence across two lines.
    pub good_morning: &'static str,
    pub good_afternoon: &'static str,
    pub good_evening: &'static str,
    pub good_night: &'static str,
    /// Where the machine could not be asked what time it is, so there is no
    /// hour to greet anybody by.
    pub welcome_back: &'static str,

    /// The button on the bottom row for an account the greeter cannot
    /// enumerate.
    pub different_user_action: &'static str,
    /// The same account as a profile at the top of the column. Separate from
    /// the button because one is a thing to press and the other is a thing to
    /// be signing in as, and several of these languages spell those
    /// differently.
    pub different_user_profile: &'static str,

    /// Where a machine has no session to offer, which is a machine that cannot
    /// be signed in to at all.
    pub no_sessions: &'static str,

    /// The field before PAM has been asked anything.
    pub sign_in: &'static str,
    /// What is being typed on the route where the account name is typed too.
    pub account_name: &'static str,
    /// The button a refusal leaves behind.
    pub try_again: &'static str,
    /// PAM's usual question, translated where PAM asked it in English — see
    /// [`Strings::prompt`].
    pub password: &'static str,

    pub starting_authentication: &'static str,
    pub checking: &'static str,
    pub opening_session: &'static str,

    pub worker_unavailable: &'static str,
    pub attempt_lost: &'static str,
    pub incorrect_password: &'static str,
    pub incorrect_account_or_password: &'static str,
    /// What a refused attempt says when the login service refused it without
    /// saying why. Where greetd *does* say why, its own sentence is shown
    /// instead and is not translated: it is a description of this machine that
    /// nothing here could rewrite without inventing part of it.
    pub service_refused: &'static str,
    /// What stands in front of a message PAM marked as an error.
    pub error_prefix: &'static str,

    pub name_empty: &'static str,
    pub name_too_long: &'static str,
    pub name_whitespace: &'static str,
    pub name_control: &'static str,

    pub sleep: &'static str,
    pub restart: &'static str,
    pub shut_down: &'static str,
    /// `{}` is the action. polkit said no.
    pub power_not_permitted: &'static str,
    /// `{}` is the action. There is no `systemctl` to ask.
    pub power_unavailable: &'static str,
    /// `{}` is the action. A preview does not turn a machine off.
    pub power_not_in_preview: &'static str,

    /// Sunday first, which is what `tm_wday` counts from.
    pub weekdays: [&'static str; 7],
    /// How the clock's second line is set, from `{weekday}` and `{day}`.
    /// Chinese puts the day first and marks it, so this is a pattern rather
    /// than a separator.
    pub date: &'static str,

    // The on-screen keyboard's word caps. The character keys are the board's
    // own ANSI geometry and are not translated — this is a picture of a
    // keyboard, and the letters on it are the letters it types.
    //
    // Latin legends stay Latin where that language's own keyboards carry Latin
    // legends, which is Hindi, Polish, Russian and Chinese: a Russian keyboard
    // has `Shift` written on it, and a board that said `Сдвиг` would be a board
    // nobody has ever seen. They are translated where the physical keyboards
    // are, which is French, German, Spanish and Portuguese.
    pub key_escape: &'static str,
    pub key_backspace: &'static str,
    pub key_tab: &'static str,
    pub key_enter: &'static str,
    pub key_space: &'static str,
    pub key_shift: &'static str,
    pub key_caps: &'static str,
    pub key_ctrl: &'static str,
    pub key_alt: &'static str,
}

impl Strings {
    /// Translate a question PAM asked, where it is one of the questions PAM
    /// asks in English on nearly every machine.
    ///
    /// PAM's own words arrive already localised *when greetd's process has a
    /// language*, and on most machines it does not: greetd is a system unit,
    /// its environment is whatever the unit gives it, and what comes back over
    /// the socket is `Password:`. Recognising that one — and the two or three
    /// beside it — is the difference between a Polish login screen and a Polish
    /// login screen with an English word in the middle of it.
    ///
    /// Only exact, well-known questions, trimmed of the trailing colon PAM
    /// writes for a terminal. Anything else is passed through untouched,
    /// because anything else is a module with something specific to say — a
    /// hardware token, a one-time code, an expiry — and half-translating it
    /// would be worse than leaving it in the language it was written in.
    pub fn prompt(&self, message: &str) -> Option<&'static str> {
        let asked = message.trim().trim_end_matches(':').trim();
        // `Password` is what `pam_unix` asks; the rest is the same question
        // asked by a service that set its own prompt.
        ["Password", "password", "UNIX password", "Enter password"]
            .contains(&asked)
            .then_some(self.password)
    }
}

const ENGLISH: Strings = Strings {
    good_morning: "Good morning",
    good_afternoon: "Good afternoon",
    good_evening: "Good evening",
    good_night: "Good night",
    welcome_back: "Welcome back",
    different_user_action: "Different User",
    different_user_profile: "Different user",
    no_sessions: "No sessions found",
    sign_in: "Sign in",
    account_name: "Account name",
    try_again: "Try again",
    password: "Password",
    starting_authentication: "Starting authentication…",
    checking: "Checking…",
    opening_session: "Opening your session…",
    worker_unavailable: "The authentication worker is unavailable.",
    attempt_lost: "The authentication attempt was lost.",
    incorrect_password: "Incorrect password.",
    incorrect_account_or_password: "Incorrect account name or password.",
    service_refused: "The login service refused the attempt.",
    error_prefix: "Error: ",
    name_empty: "Enter an account name.",
    name_too_long: "That account name is too long.",
    name_whitespace: "Account names cannot contain spaces.",
    name_control: "That is not a valid account name.",
    sleep: "Sleep",
    restart: "Restart",
    shut_down: "Shut Down",
    power_not_permitted: "{} was not permitted.",
    power_unavailable: "{} is unavailable here.",
    power_not_in_preview: "{} is not performed in preview.",
    weekdays: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
    date: "{weekday} {day}",
    key_escape: "Esc",
    key_backspace: "Back",
    key_tab: "Tab",
    key_enter: "Enter",
    key_space: "Space",
    key_shift: "Shift",
    key_caps: "Caps",
    key_ctrl: "Ctrl",
    key_alt: "Alt",
};

const FRENCH: Strings = Strings {
    good_morning: "Bonjour",
    good_afternoon: "Bon après-midi",
    good_evening: "Bonsoir",
    good_night: "Bonne nuit",
    welcome_back: "Bon retour",
    different_user_action: "Autre utilisateur",
    different_user_profile: "Autre utilisateur",
    no_sessions: "Aucune session trouvée",
    sign_in: "Se connecter",
    account_name: "Nom du compte",
    try_again: "Réessayer",
    password: "Mot de passe",
    starting_authentication: "Démarrage de l'authentification…",
    checking: "Vérification…",
    opening_session: "Ouverture de votre session…",
    worker_unavailable: "Service d'authentification indisponible.",
    attempt_lost: "Tentative de connexion perdue.",
    incorrect_password: "Mot de passe incorrect.",
    incorrect_account_or_password: "Compte ou mot de passe incorrect.",
    service_refused: "Le service de connexion a refusé.",
    error_prefix: "Erreur : ",
    name_empty: "Saisissez un nom de compte.",
    name_too_long: "Ce nom de compte est trop long.",
    name_whitespace: "Pas d'espaces dans le nom du compte.",
    name_control: "Caractère non valide dans le nom.",
    sleep: "Veille",
    restart: "Redémarrer",
    shut_down: "Éteindre",
    power_not_permitted: "{} : action non autorisée.",
    power_unavailable: "{} : indisponible ici.",
    power_not_in_preview: "{} : sans effet en aperçu.",
    weekdays: ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."],
    date: "{weekday} {day}",
    key_escape: "Échap",
    key_backspace: "Retour",
    key_tab: "Tab",
    key_enter: "Entrée",
    key_space: "Espace",
    key_shift: "Maj",
    key_caps: "Verr Maj",
    key_ctrl: "Ctrl",
    key_alt: "Alt",
};

const GERMAN: Strings = Strings {
    good_morning: "Guten Morgen",
    good_afternoon: "Guten Tag",
    good_evening: "Guten Abend",
    good_night: "Gute Nacht",
    welcome_back: "Willkommen zurück",
    different_user_action: "Anderer Benutzer",
    different_user_profile: "Anderer Benutzer",
    no_sessions: "Keine Sitzungen gefunden",
    sign_in: "Anmelden",
    account_name: "Kontoname",
    try_again: "Erneut versuchen",
    password: "Passwort",
    starting_authentication: "Anmeldung wird gestartet…",
    checking: "Wird geprüft…",
    opening_session: "Ihre Sitzung wird geöffnet…",
    worker_unavailable: "Der Anmeldedienst ist nicht erreichbar.",
    attempt_lost: "Der Anmeldeversuch ging verloren.",
    incorrect_password: "Falsches Passwort.",
    incorrect_account_or_password: "Kontoname oder Passwort ist falsch.",
    service_refused: "Der Anmeldedienst hat abgelehnt.",
    error_prefix: "Fehler: ",
    name_empty: "Geben Sie einen Kontonamen ein.",
    name_too_long: "Dieser Kontoname ist zu lang.",
    name_whitespace: "Keine Leerzeichen im Kontonamen.",
    name_control: "Ungültiges Zeichen im Kontonamen.",
    sleep: "Bereitschaft",
    restart: "Neu starten",
    shut_down: "Ausschalten",
    power_not_permitted: "„{}“ wurde nicht zugelassen.",
    power_unavailable: "„{}“ ist hier nicht verfügbar.",
    power_not_in_preview: "„{}“: nicht in der Vorschau.",
    weekdays: ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"],
    date: "{weekday} {day}.",
    key_escape: "Esc",
    key_backspace: "Rück",
    key_tab: "Tab",
    key_enter: "Enter",
    key_space: "Leer",
    key_shift: "Umschalt",
    key_caps: "Feststell",
    key_ctrl: "Strg",
    key_alt: "Alt",
};

const HINDI: Strings = Strings {
    good_morning: "सुप्रभात",
    good_afternoon: "नमस्कार",
    good_evening: "शुभ संध्या",
    good_night: "शुभ रात्रि",
    welcome_back: "वापसी पर स्वागत है",
    different_user_action: "अन्य उपयोगकर्ता",
    different_user_profile: "अन्य उपयोगकर्ता",
    no_sessions: "कोई सत्र नहीं मिला",
    sign_in: "साइन इन करें",
    account_name: "खाता नाम",
    try_again: "फिर से आज़माएँ",
    password: "पासवर्ड",
    starting_authentication: "प्रमाणीकरण शुरू हो रहा है…",
    checking: "जाँच हो रही है…",
    opening_session: "आपका सत्र खुल रहा है…",
    worker_unavailable: "प्रमाणीकरण सेवा उपलब्ध नहीं है।",
    attempt_lost: "प्रमाणीकरण का प्रयास खो गया।",
    incorrect_password: "पासवर्ड गलत है।",
    incorrect_account_or_password: "खाता नाम या पासवर्ड गलत है।",
    service_refused: "लॉगिन सेवा ने यह प्रयास अस्वीकार कर दिया।",
    error_prefix: "त्रुटि: ",
    name_empty: "खाता नाम दर्ज करें।",
    name_too_long: "यह खाता नाम बहुत लंबा है।",
    name_whitespace: "खाता नाम में रिक्त स्थान नहीं हो सकते।",
    name_control: "इस खाता नाम में अमान्य वर्ण है।",
    sleep: "स्लीप",
    restart: "पुनः आरंभ",
    shut_down: "बंद करें",
    power_not_permitted: "{} की अनुमति नहीं थी।",
    power_unavailable: "{} यहाँ उपलब्ध नहीं है।",
    power_not_in_preview: "पूर्वावलोकन में {} नहीं किया जाता।",
    weekdays: ["रवि", "सोम", "मंगल", "बुध", "गुरु", "शुक्र", "शनि"],
    date: "{weekday} {day}",
    key_escape: "Esc",
    key_backspace: "Back",
    key_tab: "Tab",
    key_enter: "Enter",
    key_space: "Space",
    key_shift: "Shift",
    key_caps: "Caps",
    key_ctrl: "Ctrl",
    key_alt: "Alt",
};

const POLISH: Strings = Strings {
    good_morning: "Dzień dobry",
    good_afternoon: "Dzień dobry",
    good_evening: "Dobry wieczór",
    good_night: "Dobranoc",
    welcome_back: "Witaj ponownie",
    different_user_action: "Inny użytkownik",
    different_user_profile: "Inny użytkownik",
    no_sessions: "Nie znaleziono sesji",
    sign_in: "Zaloguj się",
    account_name: "Nazwa konta",
    try_again: "Spróbuj ponownie",
    password: "Hasło",
    starting_authentication: "Rozpoczynanie uwierzytelniania…",
    checking: "Sprawdzanie…",
    opening_session: "Otwieranie sesji…",
    worker_unavailable: "Usługa uwierzytelniania jest niedostępna.",
    attempt_lost: "Próba uwierzytelnienia została utracona.",
    incorrect_password: "Nieprawidłowe hasło.",
    incorrect_account_or_password: "Nieprawidłowa nazwa konta lub hasło.",
    service_refused: "Usługa logowania odrzuciła tę próbę.",
    error_prefix: "Błąd: ",
    name_empty: "Wpisz nazwę konta.",
    name_too_long: "Ta nazwa konta jest za długa.",
    name_whitespace: "Nazwa konta nie może zawierać spacji.",
    name_control: "Nieprawidłowy znak w nazwie konta.",
    sleep: "Uśpienie",
    restart: "Uruchom ponownie",
    shut_down: "Wyłącz",
    power_not_permitted: "Nie zezwolono na: {}.",
    power_unavailable: "Niedostępne: {}.",
    power_not_in_preview: "Nie w podglądzie: {}.",
    weekdays: ["niedz.", "pon.", "wt.", "śr.", "czw.", "pt.", "sob."],
    date: "{weekday} {day}",
    key_escape: "Esc",
    key_backspace: "Back",
    key_tab: "Tab",
    key_enter: "Enter",
    key_space: "Spacja",
    key_shift: "Shift",
    key_caps: "Caps",
    key_ctrl: "Ctrl",
    key_alt: "Alt",
};

const PORTUGUESE: Strings = Strings {
    good_morning: "Bom dia",
    good_afternoon: "Boa tarde",
    good_evening: "Boa noite",
    good_night: "Boa madrugada",
    welcome_back: "Bem-vindo de volta",
    different_user_action: "Outro usuário",
    different_user_profile: "Outro usuário",
    no_sessions: "Nenhuma sessão encontrada",
    sign_in: "Entrar",
    account_name: "Nome da conta",
    try_again: "Tentar novamente",
    password: "Senha",
    starting_authentication: "Iniciando a autenticação…",
    checking: "Verificando…",
    opening_session: "Abrindo sua sessão…",
    worker_unavailable: "Serviço de autenticação indisponível.",
    attempt_lost: "A tentativa de autenticação foi perdida.",
    incorrect_password: "Senha incorreta.",
    incorrect_account_or_password: "Nome da conta ou senha incorretos.",
    service_refused: "O serviço de login recusou a tentativa.",
    error_prefix: "Erro: ",
    name_empty: "Digite um nome de conta.",
    name_too_long: "Esse nome de conta é longo demais.",
    name_whitespace: "O nome não pode ter espaços.",
    name_control: "Caractere inválido no nome da conta.",
    sleep: "Suspender",
    restart: "Reiniciar",
    shut_down: "Desligar",
    power_not_permitted: "{}: não permitido.",
    power_unavailable: "{}: indisponível aqui.",
    power_not_in_preview: "{}: não executado na prévia.",
    weekdays: ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"],
    date: "{weekday} {day}",
    key_escape: "Esc",
    key_backspace: "Back",
    key_tab: "Tab",
    key_enter: "Enter",
    key_space: "Espaço",
    key_shift: "Shift",
    key_caps: "Caps",
    key_ctrl: "Ctrl",
    key_alt: "Alt",
};

const RUSSIAN: Strings = Strings {
    good_morning: "Доброе утро",
    good_afternoon: "Добрый день",
    good_evening: "Добрый вечер",
    good_night: "Доброй ночи",
    welcome_back: "С возвращением",
    different_user_action: "Другой пользователь",
    different_user_profile: "Другой пользователь",
    no_sessions: "Сеансы не найдены",
    sign_in: "Войти",
    account_name: "Имя учётной записи",
    try_again: "Повторить",
    password: "Пароль",
    starting_authentication: "Запуск проверки подлинности…",
    checking: "Проверка…",
    opening_session: "Открытие сеанса…",
    worker_unavailable: "Служба входа недоступна.",
    attempt_lost: "Попытка входа была потеряна.",
    incorrect_password: "Неверный пароль.",
    incorrect_account_or_password: "Неверное имя или пароль.",
    service_refused: "Служба входа отклонила попытку.",
    error_prefix: "Ошибка: ",
    name_empty: "Введите имя учётной записи.",
    name_too_long: "Слишком длинное имя.",
    name_whitespace: "В имени не может быть пробелов.",
    name_control: "Недопустимый символ в имени.",
    sleep: "Спящий режим",
    restart: "Перезагрузка",
    shut_down: "Выключение",
    power_not_permitted: "«{}»: не разрешено.",
    power_unavailable: "«{}»: недоступно.",
    power_not_in_preview: "«{}»: не в предпросмотре.",
    weekdays: ["вс", "пн", "вт", "ср", "чт", "пт", "сб"],
    date: "{weekday} {day}",
    key_escape: "Esc",
    key_backspace: "Back",
    key_tab: "Tab",
    key_enter: "Enter",
    key_space: "Space",
    key_shift: "Shift",
    key_caps: "Caps",
    key_ctrl: "Ctrl",
    key_alt: "Alt",
};

const SPANISH: Strings = Strings {
    good_morning: "Buenos días",
    good_afternoon: "Buenas tardes",
    good_evening: "Buenas noches",
    good_night: "Buenas noches",
    welcome_back: "Bienvenido de nuevo",
    different_user_action: "Otro usuario",
    different_user_profile: "Otro usuario",
    no_sessions: "No se encontraron sesiones",
    sign_in: "Iniciar sesión",
    account_name: "Nombre de cuenta",
    try_again: "Reintentar",
    password: "Contraseña",
    starting_authentication: "Iniciando la autenticación…",
    checking: "Comprobando…",
    opening_session: "Abriendo tu sesión…",
    worker_unavailable: "Servicio de autenticación no disponible.",
    attempt_lost: "Se perdió el intento de autenticación.",
    incorrect_password: "Contraseña incorrecta.",
    incorrect_account_or_password: "Cuenta o contraseña incorrectas.",
    service_refused: "El servicio de acceso rechazó el intento.",
    error_prefix: "Error: ",
    name_empty: "Escribe un nombre de cuenta.",
    name_too_long: "Ese nombre de cuenta es muy largo.",
    name_whitespace: "El nombre no puede tener espacios.",
    name_control: "Carácter no válido en el nombre.",
    sleep: "Suspender",
    restart: "Reiniciar",
    shut_down: "Apagar",
    power_not_permitted: "{}: no permitido.",
    power_unavailable: "{}: no disponible aquí.",
    power_not_in_preview: "{}: no se realiza en la vista previa.",
    weekdays: ["dom", "lun", "mar", "mié", "jue", "vie", "sáb"],
    date: "{weekday} {day}",
    key_escape: "Esc",
    key_backspace: "Retro",
    key_tab: "Tab",
    key_enter: "Intro",
    key_space: "Espacio",
    key_shift: "Mayús",
    key_caps: "Bloq",
    key_ctrl: "Ctrl",
    key_alt: "Alt",
};

const CHINESE: Strings = Strings {
    good_morning: "早上好",
    good_afternoon: "下午好",
    good_evening: "晚上好",
    good_night: "晚安",
    welcome_back: "欢迎回来",
    different_user_action: "其他用户",
    different_user_profile: "其他用户",
    no_sessions: "未找到会话",
    sign_in: "登录",
    account_name: "账户名",
    try_again: "重试",
    password: "密码",
    starting_authentication: "正在启动身份验证…",
    checking: "正在检查…",
    opening_session: "正在打开会话…",
    worker_unavailable: "身份验证服务不可用。",
    attempt_lost: "身份验证请求已丢失。",
    incorrect_password: "密码错误。",
    incorrect_account_or_password: "账户名或密码错误。",
    service_refused: "登录服务拒绝了此次尝试。",
    error_prefix: "错误：",
    name_empty: "请输入账户名。",
    name_too_long: "该账户名过长。",
    name_whitespace: "账户名不能包含空格。",
    name_control: "该账户名包含无效字符。",
    sleep: "睡眠",
    restart: "重启",
    shut_down: "关机",
    power_not_permitted: "不允许执行“{}”。",
    power_unavailable: "此处无法执行“{}”。",
    power_not_in_preview: "预览模式下不执行“{}”。",
    weekdays: ["周日", "周一", "周二", "周三", "周四", "周五", "周六"],
    date: "{day}日 {weekday}",
    key_escape: "Esc",
    key_backspace: "Back",
    key_tab: "Tab",
    key_enter: "Enter",
    key_space: "Space",
    key_shift: "Shift",
    key_caps: "Caps",
    key_ctrl: "Ctrl",
    key_alt: "Alt",
};

impl Strings {
    /// Every sentence in this catalogue, for the checks that are about all of
    /// them at once: that none is empty, that none was left in English, and —
    /// in [`crate::visual`] — that the shipped faces can actually draw them.
    pub fn every_message(&self) -> Vec<&'static str> {
        let mut all = vec![
            self.good_morning,
            self.good_afternoon,
            self.good_evening,
            self.good_night,
            self.welcome_back,
            self.different_user_action,
            self.different_user_profile,
            self.no_sessions,
            self.sign_in,
            self.account_name,
            self.try_again,
            self.password,
            self.starting_authentication,
            self.checking,
            self.opening_session,
            self.worker_unavailable,
            self.attempt_lost,
            self.incorrect_password,
            self.incorrect_account_or_password,
            self.service_refused,
            self.error_prefix,
            self.name_empty,
            self.name_too_long,
            self.name_whitespace,
            self.name_control,
            self.sleep,
            self.restart,
            self.shut_down,
            self.power_not_permitted,
            self.power_unavailable,
            self.power_not_in_preview,
            self.date,
            self.key_escape,
            self.key_backspace,
            self.key_tab,
            self.key_enter,
            self.key_space,
            self.key_shift,
            self.key_caps,
            self.key_ctrl,
            self.key_alt,
        ];
        all.extend_from_slice(&self.weekdays);
        all
    }

    /// The word caps of the on-screen keyboard, with the width in key units
    /// the board gives each of them. See the fit check in [`crate::ui`].
    pub fn every_key_cap(&self) -> [(&'static str, f32); 9] {
        [
            (self.key_escape, 15.0 / 13.0),
            (self.key_backspace, 2.0),
            (self.key_tab, 1.5),
            (self.key_enter, 2.25),
            (self.key_space, 5.5),
            (self.key_shift, 2.25),
            (self.key_caps, 1.75),
            (self.key_ctrl, 1.5),
            (self.key_alt, 1.5),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicU64;

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    fn test_file(name: &str, body: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "cedm-i18n-{}-{}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join(name);
        fs::write(&path, body).unwrap();
        path
    }

    fn remove_test_file(path: &Path) {
        fs::remove_file(path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn every_language_answers_to_its_own_tag() {
        for language in ALL {
            assert_eq!(
                Language::from_locale(language.tag()),
                Some(language),
                "{} does not answer to {}",
                language.endonym(),
                language.tag()
            );
        }
    }

    /// The names locales are actually written in, on the machines this runs
    /// on. Every one of these has been seen in a `LANG=`.
    #[test]
    fn reads_the_locale_names_machines_are_set_to() {
        for (locale, want) in [
            ("pl_PL.UTF-8", Language::Polish),
            ("pl_PL.utf8", Language::Polish),
            ("pl", Language::Polish),
            ("pt_BR.UTF-8", Language::Portuguese),
            ("pt_PT.UTF-8", Language::Portuguese),
            ("pt-BR", Language::Portuguese),
            ("zh_CN.UTF-8", Language::Chinese),
            ("zh_TW.UTF-8", Language::Chinese),
            ("hi_IN", Language::Hindi),
            ("ru_RU.UTF-8", Language::Russian),
            ("de_AT@euro", Language::German),
            ("fr_CA.ISO-8859-1", Language::French),
            ("es_MX.UTF-8", Language::Spanish),
            ("en_GB.UTF-8", Language::English),
            ("  de_DE.UTF-8  ", Language::German),
        ] {
            assert_eq!(
                Language::from_locale(locale),
                Some(want),
                "{locale} did not read as {}",
                want.endonym()
            );
        }
    }

    /// `C` is not a language, it is the absence of one. Reading it as English
    /// would stop the search at the environment a system unit happens to have,
    /// and the machine's own file would never be opened.
    #[test]
    fn the_c_locale_and_nonsense_name_no_language() {
        for locale in ["C", "POSIX", "C.UTF-8", "", "   ", "@euro", "kl_GL.UTF-8"] {
            assert_eq!(Language::from_locale(locale), None, "{locale:?}");
        }
    }

    #[test]
    fn a_locale_file_is_read_whatever_shell_dressing_it_has() {
        let path = test_file(
            "locale.conf",
            "# set by the installer\nLANG=\"pl_PL.UTF-8\"\n",
        );
        assert_eq!(
            read_locale_file(&path),
            Some(("LANG", "pl_PL.UTF-8".to_string()))
        );
        remove_test_file(&path);

        let path = test_file(
            "i18n",
            "export LANG='ru_RU.UTF-8'\nexport LC_TIME=en_GB.UTF-8\n",
        );
        assert_eq!(
            read_locale_file(&path),
            Some(("LANG", "ru_RU.UTF-8".to_string()))
        );
        remove_test_file(&path);
    }

    /// Debian writes `LANG` and `LC_ALL` into one file often enough that the
    /// order they land in must not decide the answer.
    #[test]
    fn the_strongest_key_in_a_file_wins_whatever_order_it_is_in() {
        let path = test_file("locale", "LANG=en_US.UTF-8\nLC_MESSAGES=de_DE.UTF-8\n");
        assert_eq!(
            read_locale_file(&path),
            Some(("LC_MESSAGES", "de_DE.UTF-8".to_string()))
        );
        remove_test_file(&path);

        let path = test_file("locale", "LC_MESSAGES=de_DE.UTF-8\nLC_ALL=fr_FR.UTF-8\n");
        assert_eq!(
            read_locale_file(&path),
            Some(("LC_ALL", "fr_FR.UTF-8".to_string()))
        );
        remove_test_file(&path);
    }

    #[test]
    fn a_file_with_nothing_to_say_says_nothing() {
        let path = test_file("locale", "# nothing here\nLC_TIME=en_GB.UTF-8\nLANG=\n");
        assert_eq!(read_locale_file(&path), None);
        remove_test_file(&path);
        assert_eq!(
            read_locale_file(Path::new("/nonexistent/locale.conf")),
            None
        );
    }

    /// Both spellings of the same fact, on the two distributions that write
    /// them, read as the same language. This is the whole point of [`FILES`].
    #[test]
    fn arch_and_debian_spell_one_answer_two_ways() {
        let arch = test_file("locale.conf", "LANG=pl_PL.UTF-8\n");
        let debian = test_file(
            "locale",
            "#  File generated by update-locale\nLANG=\"pl_PL.UTF-8\"\n",
        );
        for path in [&arch, &debian] {
            let (_, locale) = read_locale_file(path).expect("a locale");
            assert_eq!(Language::from_locale(&locale), Some(Language::Polish));
        }
        remove_test_file(&arch);
        remove_test_file(&debian);
    }

    /// A machine with two of these files is a machine mid-upgrade, and the
    /// walk has to get through it: an empty `/etc/locale.conf` sitting beside a
    /// populated `/etc/default/locale` is an ordinary Debian, and stopping at
    /// the first file that *exists* would read it as a machine with no
    /// language at all.
    ///
    #[test]
    fn an_empty_file_is_not_an_answer_and_the_walk_carries_on() {
        let empty = test_file("locale.conf", "# nothing has been set here\n");
        let populated = test_file("locale", "LANG=\"ru_RU.UTF-8\"\n");
        let files = [empty.to_str().unwrap(), populated.to_str().unwrap()];
        let (path, locale, language) = first_locale_in(&files).expect("the second file");
        assert_eq!(language, Language::Russian);
        assert_eq!(locale, "ru_RU.UTF-8");
        assert_eq!(path, files[1]);

        // And a machine where none of them says anything falls through
        // entirely, rather than answering with the first file it found.
        assert!(first_locale_in(&[files[0]]).is_none());
        remove_test_file(&empty);
        remove_test_file(&populated);
    }

    #[test]
    fn an_option_outranks_the_machine_and_a_bad_one_is_ignored() {
        let chosen = detect(Some("fr_FR.UTF-8"), Some("de_DE.UTF-8"));
        assert_eq!(chosen.language, Language::French);
        assert_eq!(chosen.origin, "--language");

        let chosen = detect(None, Some("de_DE.UTF-8"));
        assert_eq!(chosen.language, Language::German);
        assert_eq!(chosen.origin, "config language");

        // A language nobody wrote a catalogue for is not an answer, so the
        // search carries on past it rather than the greeter refusing to start.
        let chosen = detect(Some("kl_GL.UTF-8"), Some("es_ES.UTF-8"));
        assert_eq!(chosen.language, Language::Spanish);
    }

    #[test]
    fn every_catalogue_is_complete() {
        for language in ALL {
            for message in language.strings().every_message() {
                assert!(
                    !message.trim().is_empty(),
                    "{} has an empty message",
                    language.endonym()
                );
            }
        }
    }

    /// A catalogue that is still English is a language somebody started and
    /// did not finish, and nothing else in the build would notice.
    #[test]
    fn no_catalogue_was_left_in_english() {
        let english = Language::English.strings();
        for language in ALL {
            // The two Englishes are one catalogue on purpose — this screen
            // writes neither a month nor one of the handful of words the two
            // spell differently, so there is nothing for a second file to
            // hold. See [`Language::AmericanEnglish`].
            if matches!(language, Language::English | Language::AmericanEnglish) {
                continue;
            }
            let strings = language.strings();
            for (mine, theirs) in [
                (strings.sign_in, english.sign_in),
                (strings.account_name, english.account_name),
                (strings.try_again, english.try_again),
                (strings.password, english.password),
                (strings.incorrect_password, english.incorrect_password),
                (strings.shut_down, english.shut_down),
                (strings.good_morning, english.good_morning),
                (strings.welcome_back, english.welcome_back),
                (strings.no_sessions, english.no_sessions),
                (strings.checking, english.checking),
            ] {
                assert_ne!(mine, theirs, "{} is still English", language.endonym());
            }
        }
    }

    /// A sentence written around an action has to have somewhere to put it.
    #[test]
    fn every_sentence_about_an_action_names_it_exactly_once() {
        for language in ALL {
            let strings = language.strings();
            for template in [
                strings.power_not_permitted,
                strings.power_unavailable,
                strings.power_not_in_preview,
            ] {
                assert_eq!(
                    template.matches("{}").count(),
                    1,
                    "{}: {template:?}",
                    language.endonym()
                );
            }
            assert!(strings.date.contains("{weekday}") && strings.date.contains("{day}"));
        }
    }

    #[test]
    fn fills_a_sentence_wherever_the_hole_is() {
        assert_eq!(
            fill("{} was not permitted.", "Sleep"),
            "Sleep was not permitted."
        );
        assert_eq!(
            fill("Nie zezwolono na: {}.", "Uśpienie"),
            "Nie zezwolono na: Uśpienie."
        );
    }

    /// PAM's English question is answered in the reader's language; anything
    /// else is a module with something specific to say and is left alone.
    #[test]
    fn only_pams_own_words_are_translated() {
        let polish = Language::Polish.strings();
        for asked in ["Password:", "Password: ", "password:", " Password "] {
            assert_eq!(polish.prompt(asked), Some("Hasło"), "{asked:?}");
        }
        for asked in [
            "Enter PIN for token:",
            "Verification code:",
            "YubiKey for `alex':",
            "Hasło:",
        ] {
            assert_eq!(polish.prompt(asked), None, "{asked:?}");
        }
    }

    #[test]
    fn the_language_is_set_once_and_read_everywhere() {
        with_language(Language::Polish, || {
            assert_eq!(language(), Language::Polish);
            assert_eq!(text().sign_in, "Zaloguj się");
        });
        with_language(Language::Chinese, || assert_eq!(text().sign_in, "登录"));
    }

    /// Desktop entries are keyed by POSIX locale names, and the country half
    /// of one is not optional where it is the country that was translated.
    #[test]
    fn registry_roundtrips_and_keeps_english_and_polish() {
        assert!(ALL.contains(&Language::English));
        assert!(ALL.contains(&Language::Polish));
        let mut tags = std::collections::BTreeSet::new();
        for language in ALL {
            assert!(tags.insert(language.tag()));
            assert_eq!(Language::from_code(language.code()), language);
            assert_eq!(Language::from_locale(language.tag()), Some(language));
            assert!(!language.endonym().is_empty());
        }
        assert_eq!(Language::from_code(u8::MAX), Language::English);
        assert_eq!(Language::from_locale("pl_PL.UTF-8"), Some(Language::Polish));
        assert_eq!(Language::Polish.strings().sign_in, "Zaloguj się");
    }

    #[test]
    fn a_desktop_entry_is_looked_up_by_the_key_it_actually_carries() {
        assert_eq!(Language::Portuguese.desktop_keys(), ["pt_BR", "pt"]);
        assert_eq!(Language::Chinese.desktop_keys()[0], "zh_CN");
        for language in ALL {
            assert!(!language.desktop_keys().is_empty());
        }
    }
}
