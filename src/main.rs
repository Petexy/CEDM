use anyhow::{bail, Context};
use cedm::auth::{Actor, Command as AuthCommand, Event as AuthEvent, Failure};
use cedm::controller::{Action, Controller, POLL_INTERVAL};
use cedm::keyboard::{Board, Press, Stroke};
use cedm::sessions::Session;
use cedm::state::{Preferences, State};
use cedm::ui::{Focus, FooterItem, Menu, Phase, Target};
use cedm::users::User;
use cedm::visual::{self, Renderer};
use clap::Parser;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing_subscriber::EnvFilter;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Fullscreen, Window, WindowAttributes, WindowId};
use zeroize::{Zeroize, Zeroizing};

const MAX_PROMPT_INPUT_BYTES: usize = 4096;
/// How often the wall clock is re-read. The panel shows minutes, so this is
/// already far finer than anything it can display.
const CLOCK_INTERVAL: Duration = Duration::from_secs(1);

/// The bottom row of the column, in the order it is laid out.
///
/// The administrator's policy decides which of the three power actions exist.
/// The route for a non-enumerated account is always last and always present:
/// it is the only way in on a machine whose accounts live in a directory, and
/// withdrawing it would be withdrawing the login rather than an action.
fn footer_items(config: &cedm::config::Config) -> Vec<FooterItem> {
    cedm::power::ALL
        .into_iter()
        .filter(|action| config.power.allows(*action))
        .map(FooterItem::Power)
        .chain(std::iter::once(FooterItem::DifferentUser))
        .collect()
}

#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Run as a normal nested window, useful while developing from a desktop.
    #[arg(long)]
    windowed: bool,
    /// Render and exercise input without contacting greetd or starting sessions.
    #[arg(long)]
    preview: bool,
    /// Start preview mode on a secret prompt so the built-in keyboard can be reviewed.
    #[arg(long, requires = "preview")]
    preview_auth: bool,
    /// Start preview mode with the session menu open, so it can be reviewed.
    #[arg(long, requires = "preview", conflicts_with = "preview_auth")]
    preview_menu: bool,
    /// Print discovered sessions and exit.
    #[arg(long)]
    list_sessions: bool,
    /// Publish this account's LineXinBar look for the login screen, and exit.
    ///
    /// Run by the account itself, from `cedm-session`, as its session starts.
    /// A greeter cannot read a home directory — see [`cedm::look`] — so an
    /// account that never publishes is an account the login screen has to draw
    /// in the default palette, on displays brought up however its compositor
    /// happened to bring them up.
    ///
    /// Nothing about a login depends on it: a failure is reported and the
    /// session goes on.
    #[arg(long)]
    publish_look: bool,
    /// Write the greeter compositor's display configuration here, and exit.
    ///
    /// Run by `cedm-greeter-session` before it starts that compositor, because
    /// a mode, an output's place in a layout and whether a display is driven in
    /// HDR are all decided before there is a client to ask. What it writes is
    /// the last signed-in account's published look, in LineXinBar's own
    /// compositor config format. See [`write_compositor_config`].
    #[arg(long, value_name = "PATH")]
    compositor_config: Option<PathBuf>,
    /// Arguments for the greeter the written configuration starts, after `--`.
    ///
    /// The compositor starts the greeter itself rather than being handed it on
    /// a command line, so anything the greeter was going to be given has to go
    /// into the file with it. Only read with `--compositor-config`.
    #[arg(last = true, value_name = "GREETER ARGUMENT")]
    greeter_arguments: Vec<String>,
    /// Show the login screen in this language, whatever the machine is set to.
    ///
    /// A locale name or a language tag: `pl`, `pl_PL.UTF-8` and `pt-BR` are
    /// all understood. This is for reviewing the ten translations from one
    /// desk. The real login screen takes the machine's own language — see
    /// [`cedm::i18n`] — and an administrator who wants to override that says
    /// so in the configuration file rather than on a command line nothing
    /// runs. A language this greeter is not written in is ignored rather than
    /// fatal, for the same reason the configured one is.
    #[arg(long, value_name = "LANGUAGE")]
    language: Option<String>,
    /// Do not open standard or Steam Controller input devices.
    #[arg(long)]
    no_gamepad: bool,
    /// Do not open the machine's sound output, and answer no button with a
    /// noise.
    ///
    /// The companion of `--no-gamepad`, and here for the same reason: a greeter
    /// being looked at inside somebody's desktop should be able to leave that
    /// machine's devices alone. An administrator silences the real login screen
    /// with `sound = false` in the configuration instead — see
    /// [`cedm::config::Config`]; this is a switch for the person running it by
    /// hand.
    #[arg(long)]
    no_sound: bool,
    /// Write one composed frame to this PNG and exit. Implies `--preview`.
    ///
    /// For reviewing what the greeter draws without giving it a seat to draw
    /// on. It still needs a window to obtain a GPU surface, so it is a
    /// development aid rather than a headless renderer.
    #[arg(long, value_name = "PATH")]
    shot: Option<PathBuf>,
    /// Open the window at an exact surface size in physical pixels, as
    /// `WIDTHxHEIGHT`. Implies `--windowed`.
    ///
    /// The interface is laid out in the pixels the surface actually has, so
    /// the developer's window — which comes up at the size of the design
    /// canvas — is the one size at which nothing about scaling can be wrong.
    /// That is a poor place to review from: a length left unscaled is exactly
    /// as correct there as it is broken on a 4K panel. This is how such a
    /// panel gets reviewed without owning one.
    ///
    /// A window manager is free to ignore the request; `--shot` logs the size
    /// it was actually given.
    #[arg(long, value_name = "WxH", value_parser = parse_size)]
    size: Option<(u32, u32)>,
    /// Compose as though the window were these displays, as a comma-separated
    /// list of `WIDTHxHEIGHT+X+Y` in physical pixels. Implies `--windowed`.
    ///
    /// The greeter draws a whole login screen on every display it is given, and
    /// which displays those are comes from the compositor. A developer's window
    /// is nested inside a desktop and has no output layout of its own to be cut
    /// up, so a machine with one monitor could otherwise only ever review the
    /// one-display case — and the composition that is hardest to get right is
    /// the one where a column has to stop at a seam in the middle of a surface.
    ///
    /// Given here, these replace what the compositor says entirely, which is
    /// also how a layout that will not be met on this desk gets looked at:
    /// `--size 3200x1080 --displays 1920x1080+0+0,1280x1024+1920+0`.
    #[arg(long, value_name = "WxH+X+Y", value_delimiter = ',', value_parser = parse_display)]
    displays: Vec<cedm::displays::Display>,
}

/// `WIDTHxHEIGHT`, bounded at both ends: this opens a window and allocates
/// render targets from it, and neither is given a number straight off a
/// command line.
fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let (width, height) = text
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("expected WIDTHxHEIGHT, got {text:?}"))?;
    let side = |value: &str, which: &str| {
        value
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|value| (64..=16384).contains(value))
            .ok_or_else(|| format!("{which} must be a whole number of pixels from 64 to 16384"))
    };
    Ok((side(width, "the width")?, side(height, "the height")?))
}

/// One `WIDTHxHEIGHT+X+Y`, bounded exactly as `--size` is: this becomes a
/// rectangle a whole login screen is laid out in, and neither a size nor a
/// corner is taken off a command line unchecked.
fn parse_display(text: &str) -> Result<cedm::displays::Display, String> {
    let text = text.trim();
    let (size, corner) = text.split_once('+').unwrap_or((text, "0+0"));
    let (width, height) = parse_size(size)?;
    let (x, y) = corner
        .split_once('+')
        .ok_or_else(|| format!("expected WIDTHxHEIGHT+X+Y, got {text:?}"))?;
    let offset = |value: &str, which: &str| {
        value
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|value| *value <= 16384)
            .ok_or_else(|| format!("{which} must be a whole number of pixels up to 16384"))
    };
    Ok(cedm::displays::Display {
        rect: [
            offset(x, "the left edge")? as f32,
            offset(y, "the top edge")? as f32,
            width as f32,
            height as f32,
        ],
    })
}

/// What the greeter says about itself when nobody has asked for anything else.
///
/// The target of every event this program emits is its own module path, and
/// that path is the crate's real name rather than the `cedm` it is imported
/// under here — an alias is a name in this file and nothing a filter can see.
/// A directive naming the alias matched nothing at all, which on a machine
/// whose login screen came up on the wrong screens meant a journal with every
/// word wlroots had to say in it and not one line from the greeter.
const DEFAULT_LOG: &str = concat!(env!("CARGO_CRATE_NAME"), "=info");

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG)),
        )
        // Commentary on standard error, answers on standard output. What the
        // greeter has to say about itself has always gone to the journal
        // either way — the wrapper points both descriptors there — but two of
        // the things it is asked for are read by a program rather than by a
        // person: `--list-sessions`, and the wallpaper record the wrapper puts
        // in its compositor's environment. A log line in the middle of that
        // record is not a record.
        .with_writer(std::io::stderr)
        .without_time()
        .init();
    let mut args = Args::parse();
    // Before anything opens a window, a device or a socket: this runs inside
    // somebody's session rather than on a seat, and it is one file.
    if args.publish_look {
        let directory = std::path::Path::new(cedm::look::PUBLISHED);
        let (look, path) = cedm::look::publish_for_current_account(directory)
            .with_context(|| format!("could not publish a look under {}", directory.display()))?;
        // The sound output is said out loud beside the accent because it is the
        // one thing here that can be missing for a reason nothing else reports:
        // the accent and the displays come out of files that are always there,
        // and this comes from a sound server that may not be up yet. A copy
        // published without it is exactly what a silent login screen looks like
        // from this end, and the journal could not tell the two apart.
        tracing::info!(
            accent = look.accent().unwrap_or("none"),
            displays = look.display.len(),
            sound_card = look.sound_card.as_deref().unwrap_or("none"),
            sound_gain = look.sound_gain.unwrap_or(-1.0),
            path = %path.display(),
            "published this account's look for the login screen"
        );
        return Ok(());
    }
    // The same, from the other side of the login: what the greeter's own
    // compositor should bring the displays up in before it starts the greeter.
    if let Some(path) = args.compositor_config.clone() {
        let written = write_compositor_config(&path, &args.greeter_arguments)?;
        tracing::info!(
            path = %path.display(),
            account = written.account.as_deref().unwrap_or("nobody yet"),
            accent = written.accent.as_deref().unwrap_or("the default"),
            "wrote the greeter compositor's display configuration"
        );
        // The one thing that goes to standard output, because the wrapper
        // script puts it in the compositor's environment rather than in a
        // file. See `greeter_wallpaper`.
        if let Some(record) = written.wallpaper {
            println!("{record}");
        }
        return Ok(());
    }
    // A screenshot must never be able to open a PAM conversation, whatever
    // else was asked for on the same command line.
    if args.shot.is_some() {
        args.preview = true;
        args.windowed = true;
    }
    // A size is a request for a window of that size, and a fullscreen surface
    // is whatever the display is.
    if args.size.is_some() || !args.displays.is_empty() {
        args.windowed = true;
    }
    // Before the sessions are discovered, because a desktop entry carries its
    // own translations and which of them is read is this answer; and before
    // anything at all is drawn, because the language does not change again
    // afterwards. See [`cedm::i18n`].
    let config = cedm::config::Config::load();
    let language = cedm::i18n::detect(args.language.as_deref(), config.language.as_deref());
    cedm::i18n::set(language.language);
    // Named, and said out loud, because "the login screen came up in English"
    // has half a dozen causes on nine distributions and no symptom that tells
    // them apart. The origin is the file or the variable that decided it.
    tracing::info!(
        language = language.language.tag(),
        name = language.language.endonym(),
        locale = language.locale.as_deref().unwrap_or("none"),
        from = language.origin,
        "the login screen speaks the language this machine is set to"
    );
    // And the keyboard the on-screen board is a picture of, on the same terms:
    // once, before anything is drawn, and said out loud. A password with an
    // accented letter in it cannot be typed on a board that offers only
    // American ones, and "the keyboard came up QWERTY" has the same handful of
    // causes and no symptom that tells them apart.
    //
    // This is the machine's own answer and the floor under every account: the
    // board follows whichever account is being looked at, out of that account's
    // own settings, and falls back to here. See `Application::set_keyboard`.
    {
        let (layout, variant) = cedm::keyboard::system_layout();
        let read = cedm::keyboard::note_layout(&layout, &variant);
        tracing::info!(
            layout,
            variant = variant.as_str(),
            read,
            "the on-screen keyboard is a picture of this machine's keyboard"
        );
    }
    if args.displays.len() > cedm::displays::MAX {
        bail!(
            "at most {} displays are composed separately",
            cedm::displays::MAX
        );
    }
    let discovered = cedm::sessions::discover(&cedm::sessions::standard_directories());
    if args.list_sessions {
        for session in discovered {
            let support = if session.kind == cedm::sessions::Kind::Wayland {
                "supported"
            } else {
                "needs-x11-wrapper"
            };
            println!(
                "{}\t{}\t{:?}\t{}\t{}",
                session.id,
                session.name,
                session.kind,
                support,
                session.command.join(" ")
            );
        }
        return Ok(());
    }
    // An X11 desktop entry is not self-contained: a display manager must also
    // provision the X server and its authentication cookie. Until CEDM owns
    // that wrapper, showing those entries would offer a login that cannot
    // succeed. Wayland sessions (including Plasma and LineXinBar) are direct.
    let sessions = discovered
        .into_iter()
        .filter(|session| session.kind == cedm::sessions::Kind::Wayland)
        .collect::<Vec<_>>();
    if sessions.is_empty() {
        bail!("no supported Wayland desktop sessions were found");
    }
    // A machine backed only by LDAP, NIS, or another PAM directory can have
    // no enumerable local profile and still be perfectly login-capable. The
    // permanent "Other account" profile handles that case.
    let users = cedm::users::discover();

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut application = Application::new(args, config, users, sessions);
    event_loop.run_app(&mut application)?;
    if let Some(error) = application.fatal {
        Err(error)
    } else {
        Ok(())
    }
}

#[derive(Debug)]
enum Stage {
    Choose,
    Username { error: Option<&'static str> },
    Authenticating { prompt: String, secret: bool },
    Busy(String),
    Error(String),
    Departing { started: Instant, sent: bool },
}

#[derive(Debug, Clone)]
enum VisualStage {
    Choose,
    Username { error: Option<&'static str> },
    Authenticating { prompt: String, secret: bool },
    Busy(String),
    Error(String),
    Departing,
}

impl VisualStage {
    fn phase(&self) -> Phase<'_> {
        match self {
            Self::Choose => Phase::Choose,
            Self::Username { error } => Phase::Username {
                input: "",
                error: *error,
            },
            Self::Authenticating { prompt, secret } => Phase::Authenticating {
                prompt,
                secret: *secret,
                input: "",
            },
            Self::Busy(message) => Phase::Busy(message),
            Self::Error(message) => Phase::Error(message),
            Self::Departing => Phase::Departing(cedm::i18n::text().opening_session),
        }
    }
}

#[derive(Debug, Clone)]
struct StageTransition {
    previous: VisualStage,
    started: Instant,
}

/// Where the eye is: everything a direction can move. See `Application::aim`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Aim {
    focus: Focus,
    user: usize,
    session: usize,
    menu_row: usize,
    key: (usize, usize),
}

/// What is on the screen: everything a press can change. See
/// `Application::doing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Doing {
    aim: Aim,
    screen: std::mem::Discriminant<Stage>,
    /// Whether the name typed in was refused. Its own field because it does not
    /// move the screen: `Stage::Username` refusing a login name is the same
    /// screen carrying a message it did not have a moment ago.
    refused: bool,
    /// Whether the on-screen keyboard is up, and whether it is leaving.
    board: (bool, bool),
    /// The same two questions about the session menu.
    menu: (bool, bool),
}

/// Which of the three sounds answers one button, if any. See [`answer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Silent,
    Moved,
    Selected,
    Key,
}

/// What a button that has just been carried out should sound.
///
/// Free of the application so that the rule can be read and tested as a rule:
/// what went in, what the screen looked like on either side of it, and which
/// clip that comes to.
///
/// Three things decide it, in this order:
///
/// - A key of the on-screen keyboard going down is the board's own click,
///   whatever that key did. Shift and the key that puts the board away type
///   nothing and are still keys the user pressed; a board with silent keys on
///   it is a board whose keys get pressed twice.
/// - A direction sounds if it moved the highlight, and not otherwise. Every
///   direction in this greeter either moves the aim or does nothing at all —
///   the end of a row that will not wrap, a carousel holding one profile — and
///   a click for the second would be the screen reporting a step it did not
///   take.
/// - A press sounds if it changed anything the user can see. That includes the
///   ones that go backwards: leaving a prompt and putting the board away are
///   things somebody asked for and got. It excludes a press on a control that
///   did nothing, which is worth more said in silence than answered with a
///   click meaning "the button works".
fn answer(action: Action, on_board: bool, before: Doing, after: Doing) -> Answer {
    match action {
        // Start over the board is one of its keys too — Enter — and it sounds
        // like one. Off the board it never reaches here: it has already been
        // folded into Accept. See [`Application::apply_action`].
        Action::Accept | Action::Submit if on_board => Answer::Key,
        Action::Left
        | Action::Right
        | Action::Up
        | Action::Down
        | Action::Previous
        | Action::Next => {
            if before.aim == after.aim {
                Answer::Silent
            } else {
                Answer::Moved
            }
        }
        Action::Accept | Action::Submit | Action::Back | Action::ToggleKeyboard => {
            if before == after {
                Answer::Silent
            } else {
                Answer::Selected
            }
        }
    }
}

/// What an account's shell is made of: one material for the picture behind
/// everything, and one for every mark drawn on top of it.
///
/// Two rather than one because the shell's Theme setting is two — they cost
/// their own money and are wanted in their own combinations — and this login
/// screen is both halves at once: it draws that same wallpaper, and it draws the
/// shell's own marks in its clock, its arrows and its buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Materials {
    wallpaper: String,
    icons: String,
}

impl Default for Materials {
    fn default() -> Self {
        Self {
            wallpaper: cedm::accent::DEFAULT_THEME.to_string(),
            icons: cedm::accent::DEFAULT_THEME.to_string(),
        }
    }
}

impl Materials {
    fn of(&self, part: visual::theme::Part) -> &str {
        match part {
            visual::theme::Part::Wallpaper => &self.wallpaper,
            visual::theme::Part::Icons => &self.icons,
        }
    }

    /// Draw in these from this frame on, without choosing them.
    ///
    /// Whole rather than travelling, because a material has no halfway: the
    /// colour of the screen flows towards the account being looked at and what
    /// the screen is made of arrives with it.
    fn preview(&self) {
        for part in visual::theme::PARTS {
            visual::theme::preview_style(part, self.of(part));
        }
    }

    /// The startup path, where the account the screen opens on is known before
    /// there is a frame to answer with.
    fn set(&self) {
        for part in visual::theme::PARTS {
            visual::theme::set_style(part, self.of(part));
        }
    }
}

#[derive(Debug, Clone)]
struct PendingAttempt {
    id: cedm::auth::AttemptId,
    /// Present only for an enumerated profile. Directory identifiers are
    /// needed by greetd during authentication but never enter preferences.
    preference_username: Option<String>,
    session_id: String,
    accent: String,
    /// The material this account's *wallpaper* is drawn in, captured with the
    /// accent and for the same reason: what the session is handed has to be what
    /// the login screen was actually showing when the password went through, not
    /// whatever the selection has moved on to since.
    ///
    /// The wallpaper's half alone, because the record this ends up in is read by
    /// a compositor drawing a bridge frame, and a bridge frame is a wallpaper and
    /// nothing else. The marks are the shell's own business and it reads its own
    /// settings for them.
    wallpaper_material: String,
}

/// What was typed before there was anywhere to put it, and which attempt it
/// was typed into.
///
/// The attempt is what makes it safe to hold: an answer meant for one
/// conversation must never be posted into another one, so a buffer that has
/// been overtaken by a newer attempt is dropped rather than delivered.
struct TypedAhead {
    attempt: cedm::auth::AttemptId,
    text: Zeroizing<String>,
}

#[derive(Debug, Clone, Copy)]
struct CarouselMotion {
    shift: f32,
    started: Instant,
}

struct Application {
    args: Args,
    config: cedm::config::Config,
    preferences: Preferences,
    users: Vec<User>,
    sessions: Vec<Session>,
    selected_user: usize,
    user_motion: Option<CarouselMotion>,
    user_accents: Vec<String>,
    /// Which clock each account writes a time on, and the one the selection is
    /// standing on. Held the way the accents are, and for their reason: the
    /// screen is the account's, and moving along the carousel moves it.
    user_clocks: Vec<cedm::clock::Clock>,
    clock: cedm::clock::Clock,
    /// What each account's shell says about the row that explains its buttons —
    /// whether it is written, and which control that account last reached for —
    /// and the answer for the one the selection is standing on. Held the way
    /// the clocks above are, and for their reason: the screen is the account's.
    user_legends: Vec<cedm::ui::Legend>,
    legend: cedm::ui::Legend,
    /// Which control this greeter has itself seen a press from, once it has
    /// seen one at all.
    ///
    /// It outranks every account's published answer, and it has to. That answer
    /// is the shell's memory of a session that ended — as good a first guess as
    /// exists and nothing more — while this is somebody's hand on something,
    /// now, in front of this screen. It also has to survive the carousel: a
    /// person typing who pages to the next account must not be shown a pad
    /// because *that* account's last session was played with one.
    ///
    /// `None` until the first press, which is every screen nobody has touched
    /// yet — and those are the screens the published answer is for.
    hands_on_pad: Option<bool>,
    /// What each listed account's shell is made of, in the same order.
    user_themes: Vec<Materials>,
    /// The keyboard each listed account types on, in the same order again, as
    /// an xkb layout and variant. See `user_keyboard`.
    user_keyboards: Vec<(String, String)>,
    /// What this machine's keyboards are set to, under every account: the floor
    /// beneath the list above, and where a layout that will not compile lands.
    machine_keyboard: (String, String),
    /// What the on-screen board is currently a picture of. Held so that turning
    /// the carousel past five accounts on the same keyboard does not compile
    /// the same keymap five times.
    keyboard: (String, String),
    selected_session: usize,
    user_sessions: Vec<usize>,
    other_session: usize,
    accent: String,
    /// What the account being looked at is made of — the same spellings
    /// `shell.toml` uses, and the values `visual::theme` is set from.
    material: Materials,
    focus: Focus,
    /// The column vertical movement travels along, kept separate from where
    /// the focus currently is. See `move_focus`.
    desired_column: usize,
    stage: Stage,
    stage_transition: Option<StageTransition>,
    input: Zeroizing<String>,
    /// Characters typed while PAM had not yet asked for them. See
    /// [`Application::type_into_login`].
    typed_ahead: Option<TypedAhead>,
    board: Board,
    keyboard_opened: Option<Instant>,
    keyboard_closing: Option<Instant>,
    keyboard_restore_focus: Focus,
    /// Whether this machine already has a keyboard on it, asked once at start.
    ///
    /// Once, because it decides whether a board is *offered* rather than
    /// whether one is allowed, and re-asking it between two PAM prompts would
    /// mean the board coming up on one question and not the next. A keyboard
    /// plugged in while the greeter is on screen is answered by the button —
    /// or by being typed on, which is the one answer better than the question.
    keyboard_attached: bool,
    /// The row the open session menu is on. Not the selected session: the menu
    /// is a question, and nothing is chosen until it is answered.
    menu_row: usize,
    menu_opened: Option<Instant>,
    menu_closing: Option<Instant>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    /// The displays this surface covers, each as its rectangle on it.
    ///
    /// Never empty: a surface the compositor's outputs do not account for is
    /// one display, which is the whole of it. Re-asked whenever the surface
    /// changes size, because from inside a client whose compositor extends it
    /// across the whole output layout, that is what a monitor being plugged in
    /// or unplugged looks like.
    displays: Vec<cedm::displays::Display>,
    hits: Vec<cedm::ui::Hit>,
    pointer: PhysicalPosition<f64>,
    modifiers: ModifiersState,
    controller: Controller,
    /// The noise this screen answers a button with. Spent only from the button
    /// path and from a refused password — see [`cedm::sound`].
    sounds: cedm::sound::Sounds,
    auth: Option<Actor>,
    attempt: cedm::auth::AttemptId,
    pending_attempt: Option<PendingAttempt>,
    started: Instant,
    /// When this login screen first had a surface to draw on, for the rise into
    /// view — see [`Application::arrival`].
    ///
    /// The first frame rather than the start of the process: everything between
    /// the two is a GPU being opened, fonts being laid out and pictures being
    /// decoded, and a rise measured from before all of that would be over
    /// before anybody could see it.
    first_frame: Option<Instant>,
    wallpaper_clock: cedm::handoff::SceneClock,
    /// The bottom row of the column, fixed for the run: which actions exist is
    /// the administrator's decision and cannot change while the greeter is up.
    footer: Vec<FooterItem>,
    /// The civil time on the right-hand side, re-read once a second rather
    /// than once a frame. `localtime_r` may open the timezone file, and the
    /// wallpaper is drawing at sixty frames a second beside it.
    now: Option<cedm::clock::Now>,
    next_clock_read: Instant,
    last_frame: Instant,
    next_poll: Instant,
    session_started: bool,
    /// The blue-light filter this login screen put on the displays, where its
    /// compositor let it. Held rather than used: the filter lasts exactly as
    /// long as this does. See [`cedm::gamma`].
    _night_light: Option<cedm::gamma::NightLight>,
    fatal: Option<anyhow::Error>,
}

impl Application {
    /// The administrator's policy is handed in rather than read here, because
    /// one of the things in it — the language — has to be settled before this
    /// program draws anything, and a file read twice is a file that warns
    /// twice about being unreadable.
    fn new(
        args: Args,
        config: cedm::config::Config,
        users: Vec<User>,
        sessions: Vec<Session>,
    ) -> Self {
        let state = State::load();
        let preferences = Preferences::load();
        let remembered_user = config.remember_user.then(|| {
            preferences
                .remembered_user(&config)
                .or(state.last_user.as_deref())
        });
        let selected_user = remembered_user
            .flatten()
            .and_then(|name| users.iter().position(|user| user.name == name))
            .unwrap_or(0);
        let user_accents = users
            .iter()
            .map(|user| user_accent(&state, user))
            .collect::<Vec<_>>();
        let user_clocks = users.iter().map(user_clock).collect::<Vec<_>>();
        let clock = user_clocks.get(selected_user).copied().unwrap_or_default();
        let user_legends = users.iter().map(user_legend).collect::<Vec<_>>();
        let legend = user_legends.get(selected_user).copied().unwrap_or_default();
        let user_themes = users
            .iter()
            .map(|user| user_theme(&state, user))
            .collect::<Vec<_>>();
        let machine_keyboard = cedm::keyboard::system_layout();
        let user_keyboards = users
            .iter()
            .map(|user| user_keyboard(user, &machine_keyboard))
            .collect::<Vec<_>>();
        let user_sessions = users
            .iter()
            .map(|user| {
                resolve_session_index(
                    &sessions,
                    preferences.session_candidates(&config, Some(&user.name)),
                )
            })
            .collect::<Vec<_>>();
        let other_session =
            resolve_session_index(&sessions, preferences.session_candidates(&config, None));
        let selected_session = user_sessions
            .get(selected_user)
            .copied()
            .unwrap_or(other_session);
        let accent = user_accents
            .get(selected_user)
            .cloned()
            .unwrap_or_else(|| cedm::accent::DEFAULT_ACCENT.to_string());
        visual::theme::set_accent(&accent);
        let material = user_themes.get(selected_user).cloned().unwrap_or_default();
        material.set();
        let preview_auth = args.preview_auth;
        let preview_menu = args.preview_menu;
        let controller = Controller::new(!args.no_gamepad);
        let sounds = cedm::sound::Sounds::new(
            config.sound && !args.no_sound,
            sound_output(&state, &preferences, &users),
        );
        let auth = (!args.preview).then(Actor::spawn);
        // Warmed on the same terms the conversation is opened on. A preview is
        // a window on somebody's desktop, and a greeter being looked at must
        // not reach past its own window and put a filter over the session it
        // is being looked at from.
        let night_light = (!args.preview)
            .then(|| night_light(&state, &preferences, &users))
            .flatten();
        let now = Instant::now();
        // Continuing whatever is already on screen, where something is: the
        // greeter's own compositor draws this wallpaper before this program
        // has a window. See `SceneClock::resume`.
        let wallpaper_clock =
            cedm::handoff::SceneClock::resume(std::env::var_os(cedm::handoff::ENV).as_deref());
        let footer = footer_items(&config);
        let mut application = Self {
            args,
            config,
            preferences,
            users,
            sessions,
            selected_user,
            user_motion: None,
            user_accents,
            user_clocks,
            clock,
            user_legends,
            legend,
            hands_on_pad: None,
            user_themes,
            keyboard: machine_keyboard.clone(),
            user_keyboards,
            machine_keyboard,
            selected_session,
            user_sessions,
            other_session,
            accent,
            material,
            focus: Focus::Users,
            desired_column: 0,
            stage: Stage::Choose,
            stage_transition: None,
            input: Zeroizing::new(String::new()),
            typed_ahead: None,
            board: Board::default(),
            keyboard_opened: None,
            keyboard_closing: None,
            keyboard_restore_focus: Focus::Prompt,
            keyboard_attached: cedm::attached::typing_keyboard(),
            menu_row: 0,
            menu_opened: None,
            menu_closing: None,
            window: None,
            renderer: None,
            displays: Vec::new(),
            hits: Vec::new(),
            pointer: PhysicalPosition::new(-1.0, -1.0),
            modifiers: ModifiersState::empty(),
            controller,
            sounds,
            auth,
            attempt: 0,
            pending_attempt: None,
            started: now,
            first_frame: None,
            wallpaper_clock,
            footer,
            now: cedm::clock::Now::read(),
            next_clock_read: now + CLOCK_INTERVAL,
            last_frame: now,
            next_poll: now,
            session_started: false,
            _night_light: night_light,
            fatal: None,
        };
        // The board follows whichever account is being looked at, and at
        // startup that is whichever one was remembered. Done here rather than
        // above because it is the same step the carousel takes, and there is
        // one place it should live.
        let remembered = application
            .user_keyboards
            .get(selected_user)
            .cloned()
            .unwrap_or_else(|| application.machine_keyboard.clone());
        application.set_keyboard(remembered);
        if preview_auth {
            application.stage = Stage::Authenticating {
                prompt: cedm::i18n::text().password.to_string(),
                secret: true,
            };
            application.open_keyboard();
        }
        if preview_menu {
            application.open_session_menu();
            // Settled rather than unfolding, so a single captured frame shows
            // the menu itself and not a moment of its flight.
            application.menu_opened =
                Some(now - Duration::from_secs_f32(cedm::ui::MENU_UNFOLD + 0.1));
        }
        application
    }

    fn visual_stage(&self) -> VisualStage {
        match &self.stage {
            Stage::Choose => VisualStage::Choose,
            Stage::Username { error } => VisualStage::Username { error: *error },
            Stage::Authenticating { prompt, secret } => VisualStage::Authenticating {
                prompt: prompt.clone(),
                secret: *secret,
            },
            Stage::Busy(message) => VisualStage::Busy(message.clone()),
            Stage::Error(message) => VisualStage::Error(message.clone()),
            Stage::Departing { .. } => VisualStage::Departing,
        }
    }

    fn transition_to(&mut self, stage: Stage) {
        self.stage_transition = Some(StageTransition {
            previous: self.visual_stage(),
            started: Instant::now(),
        });
        self.stage = stage;
        // A new screen is a fresh start for the travelling column: the one the
        // user was working along belonged to the screen they have just left.
        self.desired_column = 0;
    }

    fn select_user(&mut self, index: usize) {
        let count = self.profile_count();
        let next = index % count;
        let delta = carousel_delta(next, self.selected_user, count);
        self.select_user_with_motion(next, delta);
    }

    fn select_other_account(&mut self) {
        // One past the enumerated profiles, which is deliberately outside the
        // carousel: it is reached from the bottom row, not by paging.
        let next = self.users.len();
        let delta = carousel_delta(
            next.min(self.profile_count() - 1),
            self.selected_user,
            self.profile_count(),
        );
        self.select_user_with_motion(next, delta);
    }

    /// How many profiles the carousel pages through.
    ///
    /// The enumerated accounts, and nothing else. The route for an account the
    /// greeter cannot enumerate has its own button on the bottom row, so
    /// counting it here would tell a machine with one account that it has two —
    /// and the dots under the avatar would say so.
    ///
    /// A machine with no enumerable account at all still has one page, and that
    /// page is that route: there has to be something selected to sign in as.
    fn profile_count(&self) -> usize {
        self.users.len().max(1)
    }

    fn other_account_selected(&self) -> bool {
        self.selected_user >= self.users.len()
    }

    /// Make the on-screen board a picture of this keyboard, where it is not
    /// already one.
    ///
    /// Compiling a keymap is a file opened and a grammar parsed, and every
    /// account on an ordinary machine types on the same keyboard — so the usual
    /// answer here is that there is nothing to do, and turning the carousel
    /// costs nothing.
    ///
    /// A layout that will not compile leaves the machine's own, which is what
    /// this greeter came up with. Not the ANSI rows the failed read has just
    /// left behind: a board is worse for being a picture of nothing, and an
    /// account naming a layout this machine's xkeyboard-config does not have is
    /// still an account whose machine has a keyboard.
    fn set_keyboard(&mut self, wanted: (String, String)) {
        if wanted == self.keyboard {
            return;
        }
        if cedm::keyboard::note_layout(&wanted.0, &wanted.1) {
            tracing::info!(
                layout = wanted.0,
                variant = wanted.1,
                "the on-screen keyboard follows the account being looked at"
            );
            self.keyboard = wanted;
            return;
        }
        // Read again whatever it was before, because the attempt that failed
        // has already put the board back on its own ANSI rows.
        cedm::keyboard::note_layout(&self.machine_keyboard.0, &self.machine_keyboard.1);
        self.keyboard = self.machine_keyboard.clone();
    }

    fn select_user_with_motion(&mut self, next: usize, delta: isize) {
        // One past the enumerated profiles is the non-enumerated route, which
        // is a legitimate selection without being a page of the carousel.
        let next = next.min(self.users.len());
        let now = Instant::now();
        let carried = self.carousel_shift(now);
        self.selected_user = next;
        let shift = carried + delta as f32;
        self.user_motion = (shift.abs() > f32::EPSILON).then_some(CarouselMotion {
            shift,
            started: now,
        });
        let keyboard = if self.other_account_selected() {
            self.selected_session = self.other_session;
            self.accent = cedm::accent::DEFAULT_ACCENT.to_string();
            self.material = Materials::default();
            // Nobody is named, so there is no account whose setting this could
            // be: the machine's own language answers, which is the default.
            self.clock = cedm::clock::Clock::default();
            // And for the same reason the row goes back to what a console says
            // before anybody has told it otherwise. Which control is drawn is
            // still whatever this greeter has seen in somebody's hands — see
            // `Application::button_legend`.
            self.legend = cedm::ui::Legend::default();
            // Nobody is named, so there is no account whose keyboard this
            // could be: the machine's own, which is what it was before any
            // account was looked at.
            self.machine_keyboard.clone()
        } else {
            self.selected_session = self.user_sessions[self.selected_user];
            self.accent = self.user_accents[self.selected_user].clone();
            self.material = self.user_themes[self.selected_user].clone();
            self.clock = self.user_clocks[self.selected_user];
            self.legend = self.user_legends[self.selected_user];
            self.user_keyboards[self.selected_user].clone()
        };
        visual::theme::preview_accent(&self.accent);
        self.material.preview();
        self.set_keyboard(keyboard);
    }

    fn cycle_user(&mut self, delta: isize) {
        let count = self.profile_count() as isize;
        // Paging away from the non-enumerated route enters the ring at whichever
        // end the movement came from, rather than landing one page inside it.
        let current = if self.other_account_selected() && !self.users.is_empty() {
            if delta >= 0 {
                -1
            } else {
                count
            }
        } else {
            self.selected_user as isize
        };
        let next = (current + delta).rem_euclid(count) as usize;
        self.select_user_with_motion(next, delta.signum());
    }

    fn carousel_shift(&self, now: Instant) -> f32 {
        const TRAVEL: f32 = 0.28;
        self.user_motion
            .map(|motion| {
                let progress =
                    (now.duration_since(motion.started).as_secs_f32() / TRAVEL).clamp(0.0, 1.0);
                motion.shift * (1.0 - ease(progress))
            })
            .unwrap_or(0.0)
    }

    fn cycle_session(&mut self, delta: isize) {
        let count = self.sessions.len() as isize;
        self.selected_session = (self.selected_session as isize + delta).rem_euclid(count) as usize;
        if self.other_account_selected() {
            self.other_session = self.selected_session;
        } else {
            self.user_sessions[self.selected_user] = self.selected_session;
        }
    }

    fn begin_login(&mut self) {
        if !matches!(self.stage, Stage::Choose | Stage::Error(_)) {
            return;
        }
        if self.other_account_selected() {
            self.input.zeroize();
            self.board = Board::default();
            self.transition_to(Stage::Username { error: None });
            self.focus = Focus::Prompt;
            self.offer_keyboard();
            return;
        }
        let username = self.users[self.selected_user].name.clone();
        self.start_login(username, true);
    }

    fn start_login(&mut self, username: String, listed_profile: bool) {
        if self.args.preview {
            self.input.zeroize();
            self.board = Board::default();
            self.transition_to(Stage::Authenticating {
                prompt: cedm::i18n::text().password.to_string(),
                secret: true,
            });
            self.offer_keyboard();
            return;
        }
        let session = self.sessions[self.selected_session].clone();
        self.attempt = self.attempt.wrapping_add(1).max(1);
        self.pending_attempt = Some(PendingAttempt {
            id: self.attempt,
            preference_username: listed_profile.then(|| username.clone()),
            session_id: session.id.clone(),
            accent: self.accent.clone(),
            wallpaper_material: self.material.wallpaper.clone(),
        });
        self.transition_to(Stage::Busy(
            cedm::i18n::text().starting_authentication.to_string(),
        ));
        if !self.auth.as_ref().is_some_and(|actor| {
            actor.send(AuthCommand::Begin {
                attempt: self.attempt,
                username,
                session,
            })
        }) {
            self.pending_attempt = None;
            self.transition_to(Stage::Error(
                cedm::i18n::text().worker_unavailable.to_string(),
            ));
        }
    }

    fn submit(&mut self) {
        match self.stage {
            Stage::Username { .. } => self.submit_username(),
            Stage::Authenticating { .. } => self.submit_answer(),
            _ => {}
        }
    }

    fn submit_username(&mut self) {
        if let Err(error) = cedm::users::validate_login_name(&self.input) {
            self.stage = Stage::Username {
                error: Some(error.message()),
            };
            self.focus = Focus::Prompt;
            return;
        }
        let username = std::mem::take(&mut *self.input);
        // VisualStage intentionally snapshots this screen with an empty field,
        // so the outgoing cross-fade cannot reveal the identifier after use.
        self.hide_keyboard(Focus::Continue);
        self.start_login(username, false);
    }

    fn submit_answer(&mut self) {
        let answer = Zeroizing::new(std::mem::take(&mut *self.input));
        self.hide_keyboard(Focus::Continue);
        if self.args.preview {
            drop(answer);
            self.transition_to(Stage::Choose);
            self.focus = Focus::Users;
            self.keyboard_restore_focus = Focus::Users;
            return;
        }
        self.transition_to(Stage::Busy(cedm::i18n::text().checking.to_string()));
        if !self.auth.as_ref().is_some_and(|actor| {
            actor.send(AuthCommand::Answer {
                attempt: self.attempt,
                answer,
            })
        }) {
            self.pending_attempt = None;
            self.transition_to(Stage::Error(
                cedm::i18n::text().worker_unavailable.to_string(),
            ));
        }
    }

    /// Whether the account this attempt is for was typed rather than chosen
    /// off the carousel. Only the typed route can have got the account itself
    /// wrong, and only it may be told so.
    fn typed_account(&self) -> bool {
        self.pending_attempt
            .as_ref()
            .is_some_and(|pending| pending.preference_username.is_none())
    }

    fn cancel(&mut self) {
        if matches!(self.stage, Stage::Departing { .. }) {
            return;
        }
        self.input.zeroize();
        self.forget_typed_ahead();
        self.hide_keyboard(Focus::Users);
        let cancelled_attempt = self.attempt;
        if let Some(actor) = &self.auth {
            actor.send(AuthCommand::Cancel {
                attempt: cancelled_attempt,
            });
        }
        // Invalidate events already queued by the cancelled conversation. The
        // worker may still be returning from greetd when this local transition
        // completes, so the UI must not accept its late prompt or failure.
        self.attempt = self.attempt.wrapping_add(1).max(1);
        self.pending_attempt = None;
        self.transition_to(Stage::Choose);
        self.focus = Focus::Users;
    }

    /// Decode each account's published picture into a cell of the atlas, once,
    /// as the window is being made.
    ///
    /// An account whose file will not decode has its `avatar` cleared here, so
    /// that from this point on "there is a picture" and "there is a picture on
    /// the GPU" are the same question — the layout asks the first and gets the
    /// second, and cannot draw a face into a cell that was never written.
    ///
    /// Accounts past what the atlas holds keep their initial. That is the same
    /// answer as an account with no published picture, which is the common case
    /// on a machine that has never had a desktop on it.
    fn load_faces(&mut self) -> Vec<Option<Vec<u8>>> {
        let size = cedm::visual::face_size();
        self.users
            .iter_mut()
            .enumerate()
            .map(|(index, user)| {
                let path = user
                    .avatar
                    .take()
                    .filter(|_| index < cedm::visual::MAX_FACES)?;
                let face = cedm::faces::load(&path, size);
                if face.is_none() {
                    tracing::debug!(user = %user.name, ?path, "no picture this can decode");
                }
                user.avatar = face.is_some().then_some(path);
                face
            })
            .collect()
    }

    /// Raise the board because the user asked for it: the button beside the
    /// field, or the pad's own keyboard button. Always, whatever is plugged in
    /// — someone who has just pressed the button asking for a keyboard is not
    /// to be told they already have one.
    fn open_keyboard(&mut self) {
        self.keyboard_closing = None;
        self.keyboard_opened = Some(Instant::now());
        self.focus = Focus::Keyboard;
    }

    /// Raise it because a prompt wants typing and there may be nothing to type
    /// on. This is every *automatic* opening, and it is the one that has to
    /// look first.
    ///
    /// A board that covers half the screen to offer a worse copy of the keys
    /// already under the user's hands, and does it at the moment they were
    /// about to start typing, is worse than no board at all. The button stays
    /// where it was for the cases this reads wrong; see [`cedm::attached`],
    /// which is honest about being a heuristic.
    fn offer_keyboard(&mut self) {
        if self.keyboard_attached {
            self.focus = Focus::Prompt;
            return;
        }
        self.open_keyboard();
    }

    fn hide_keyboard(&mut self, restore_focus: Focus) {
        self.keyboard_restore_focus = restore_focus;
        if self.keyboard_opened.is_some() && self.keyboard_closing.is_none() {
            self.keyboard_closing = Some(Instant::now());
        } else {
            self.focus = restore_focus;
        }
    }

    fn keyboard_arrival(&mut self, now: Instant) -> f32 {
        const TRAVEL: f32 = 0.28;
        if let Some(closing) = self.keyboard_closing {
            let progress = (now.duration_since(closing).as_secs_f32() / TRAVEL).clamp(0.0, 1.0);
            if progress >= 1.0 {
                self.keyboard_opened = None;
                self.keyboard_closing = None;
                self.focus = self.keyboard_restore_focus;
                return 0.0;
            }
            return 1.0 - ease(progress);
        }
        self.keyboard_opened
            .map(|opened| ease((now.duration_since(opened).as_secs_f32() / TRAVEL).clamp(0.0, 1.0)))
            .unwrap_or(0.0)
    }

    /// Everything a button can move the highlight to, as one value.
    ///
    /// Taken before and after, because "the highlight moved" is not something
    /// any one of these routes knows about itself: a direction on the identity
    /// row turns the carousel, the same direction one row down walks between
    /// two buttons, inside the menu it runs down the sessions and inside the
    /// board it steps across the keys. Comparing where the eye is before and
    /// after is the one question all five of them are answers to — and it is
    /// also what makes a direction that moved *nothing* silent, which no
    /// individual route reports either: `move_focus` wraps, `cycle_user` is a
    /// carousel that may hold one profile, and both of them return having done
    /// exactly nothing without saying so.
    fn aim(&self) -> Aim {
        Aim {
            focus: self.focus,
            user: self.selected_user,
            session: self.selected_session,
            menu_row: self.menu_row,
            key: self.board.selected(),
        }
    }

    /// Everything a press can change, as one value.
    ///
    /// The aim, and then what is on the screen: which screen it is, whether the
    /// name typed into it was refused, and whether the two panels that stand
    /// over the column are up or leaving. Deliberately nothing the user cannot
    /// see — not the attempt number, which `cancel` bumps whether or not there
    /// was a conversation to cancel, and not the answer in the field, which
    /// only the board and the keyboard put anything into.
    fn doing(&self) -> Doing {
        Doing {
            aim: self.aim(),
            screen: std::mem::discriminant(&self.stage),
            refused: matches!(self.stage, Stage::Username { error: Some(_) }),
            board: (
                self.keyboard_opened.is_some(),
                self.keyboard_closing.is_some(),
            ),
            menu: (self.menu_opened.is_some(), self.menu_closing.is_some()),
        }
    }

    /// Carry out what a key pressed outside [`Self::apply_action`] does, and
    /// answer it the way that path would.
    ///
    /// Enter and Escape reach past the action table on a real keyboard, because
    /// on a screen with a field on it they mean the field rather than the
    /// focus. They are still presses somebody made on a button, and a login
    /// screen where Enter was the one silent way to send an answer would be one
    /// with a dead key on it.
    fn button_press(&mut self, act: impl FnOnce(&mut Self)) {
        let before = self.doing();
        act(self);
        if self.doing() != before {
            self.sounds.selected();
        }
    }

    fn apply_action(&mut self, action: Action) {
        // Before anything moves, and before the board is asked whether it is
        // still up: a press that puts it away is answered as a press, and the
        // key that did it is one of its own keys.
        let before = self.doing();
        let on_board =
            !self.menu_live() && self.keyboard_opened.is_some() && self.keyboard_closing.is_none();
        // Start is Accept, and is only ever anything else over the board.
        // Folded here rather than at each of the places Accept is answered —
        // the column, the session menu, the power panel — because a button
        // that had to be listed twice everywhere would be dead wherever
        // somebody forgot to list it a second time.
        let action = match action {
            Action::Submit if !on_board => Action::Accept,
            other => other,
        };
        self.act(action);
        match answer(action, on_board, before, self.doing()) {
            Answer::Moved => self.sounds.moved(),
            Answer::Selected => self.sounds.selected(),
            Answer::Key => self.sounds.key(),
            Answer::Silent => {}
        }
    }

    /// What one action does. Every sound this greeter makes for a button is
    /// decided around this, in [`Self::apply_action`], so that the pointer —
    /// which reaches [`Self::click`] and [`Self::hover`] and never this — is
    /// silent by construction rather than by remembering to be.
    fn act(&mut self, action: Action) {
        if self.menu_live() {
            self.apply_menu_action(action);
            return;
        }
        if self.keyboard_opened.is_some() && self.keyboard_closing.is_none() {
            match action {
                Action::Left => self.board.left(),
                Action::Right => self.board.right(),
                Action::Up => self.board.up(),
                Action::Down => self.board.down(),
                Action::Accept => self.press_board(),
                Action::Submit => self.submit_board(),
                Action::Back => self.cancel(),
                Action::ToggleKeyboard => self.hide_keyboard(Focus::Prompt),
                Action::Previous | Action::Next => {}
            }
            return;
        }
        if matches!(self.stage, Stage::Departing { .. }) {
            return;
        }
        match action {
            // The identity row is the carousel: left and right move between
            // profiles rather than between controls, which is the one place
            // in the column where a row holds a list instead of buttons.
            Action::Left if self.focus == Focus::Users => self.cycle_user(-1),
            Action::Right if self.focus == Focus::Users => self.cycle_user(1),
            Action::Left if self.focus == Focus::Session => self.cycle_session(-1),
            Action::Right if self.focus == Focus::Session => self.cycle_session(1),
            Action::Left => self.move_focus(0, -1),
            Action::Right => self.move_focus(0, 1),
            Action::Up => self.move_focus(-1, 0),
            Action::Down => self.move_focus(1, 0),
            // Start arrives here only if the fold above ever stops covering
            // it, and it is Accept when it does: there is no board over this
            // column for it to be anything else about.
            Action::Accept | Action::Submit => self.accept_focus(),
            // The shoulder buttons change the session wherever the focus is,
            // and only while it is still the user's to change.
            Action::Previous if matches!(self.stage, Stage::Choose) => self.cycle_session(-1),
            Action::Next if matches!(self.stage, Stage::Choose) => self.cycle_session(1),
            Action::Previous | Action::Next => {}
            Action::Back => self.cancel(),
            Action::ToggleKeyboard => {
                if matches!(
                    self.stage,
                    Stage::Username { .. } | Stage::Authenticating { .. }
                ) {
                    self.open_keyboard();
                }
            }
        }
    }

    fn press_board(&mut self) {
        let press = self.board.press();
        self.act_on_key(press);
    }

    /// Start, pressed over the board: Enter, and the board away with it.
    ///
    /// The board's Enter key and the key that folds it away are at opposite
    /// ends of it, and a field that has been filled in is nearly always
    /// finished with both. What Enter does is exactly what its own key does —
    /// send the name, or send the answer — and both of those routes take the
    /// board with them as they go. The closing here is for the third case:
    /// a name the greeter refused, where the field stays on screen to be
    /// corrected. The board goes there too, because the error is written above
    /// the field rather than on the board, and Accept on the prompt raises it
    /// again with what was typed still in it.
    fn submit_board(&mut self) {
        let press = self.board.submit();
        self.act_on_key(press);
        if self.keyboard_opened.is_some() && self.keyboard_closing.is_none() {
            self.hide_keyboard(Focus::Prompt);
        }
    }

    /// What one key of the board does, however it came to be pressed.
    fn act_on_key(&mut self, press: Press) {
        match press {
            Press::Type(Stroke::Char(character)) => self.push_input(character),
            Press::Type(Stroke::Named("BackSpace")) => self.pop_input(),
            Press::Type(Stroke::Named("Return")) => self.submit(),
            Press::Type(Stroke::Named("Escape")) => self.cancel(),
            Press::Close => self.hide_keyboard(Focus::Prompt),
            // A dead key among them: the board types into this field rather
            // than through a keymap, so there is nothing here for an accent to
            // combine with and it is dropped. It stays *on* the board because a
            // key that is on the keyboard and missing from the picture of it is
            // a picture that is wrong — and because the letters it would have
            // made are on the AltGr face of this board anyway, where they can
            // be typed directly.
            Press::Type(_) | Press::Shifted | Press::Nothing => {}
        }
    }

    fn push_input(&mut self, character: char) {
        let maximum = if matches!(self.stage, Stage::Username { .. }) {
            cedm::users::MAX_LOGIN_NAME_BYTES
        } else {
            MAX_PROMPT_INPUT_BYTES
        };
        if self.input.len() + character.len_utf8() <= maximum {
            self.input.push(character);
            self.clear_username_error();
        }
    }

    fn pop_input(&mut self) {
        self.input.pop();
        self.clear_username_error();
    }

    /// Typing on a screen that has no field on it is the answer starting.
    ///
    /// Somebody at a keyboard who wants in types their password; asking them
    /// to press "Sign in" first is asking them to find out, one password at a
    /// time, that this screen ignores the first few letters of everything.
    /// So a printable key opens the conversation exactly as pressing the
    /// button does, and the characters go where they were always going.
    ///
    /// What is typed before greetd has asked for anything is *held* rather
    /// than dropped, and handed to the first prompt of that same attempt. A
    /// login that quietly eats the beginning of a password is worse than one
    /// that never took the keystroke: the password looks wrong, and nothing
    /// on the screen says why.
    fn type_into_login(&mut self, text: &str) {
        if self.menu_live() || self.keyboard_opened.is_some() {
            return;
        }
        // Somebody is typing, so there is a keyboard to type on, and the
        // board must not rise over a field that is already being answered.
        // This is not the heuristic being re-run — it is the one thing that
        // settles it outright, and it only ever settles it this way.
        self.keyboard_attached = true;
        if matches!(self.stage, Stage::Choose | Stage::Error(_)) {
            self.begin_login();
        }
        for character in text.chars().filter(|character| !character.is_control()) {
            if input_stage(&self.stage) {
                self.push_input(character);
            } else if matches!(self.stage, Stage::Busy(_)) {
                self.hold_typed(character);
            }
        }
    }

    fn hold_typed(&mut self, character: char) {
        let attempt = self.attempt;
        if self
            .typed_ahead
            .as_ref()
            .is_none_or(|held| held.attempt != attempt)
        {
            self.typed_ahead = Some(TypedAhead {
                attempt,
                text: Zeroizing::new(String::new()),
            });
        }
        if let Some(held) = &mut self.typed_ahead {
            if held.text.len() + character.len_utf8() <= MAX_PROMPT_INPUT_BYTES {
                held.text.push(character);
            }
        }
    }

    /// Take what was typed ahead, if it belongs to the attempt now asking.
    fn take_typed_ahead(&mut self) -> Option<Zeroizing<String>> {
        let held = self.typed_ahead.take()?;
        (held.attempt == self.attempt && !held.text.is_empty()).then_some(held.text)
    }

    fn forget_typed_ahead(&mut self) {
        self.typed_ahead = None;
    }

    fn clear_username_error(&mut self) {
        if matches!(self.stage, Stage::Username { error: Some(_) }) {
            self.stage = Stage::Username { error: None };
        }
    }

    /// The legend as it is actually drawn: what the account being looked at
    /// says about the row, and whichever control is in hand now.
    ///
    /// Deliberately not gated on `--no-gamepad`. That flag says this run is not
    /// *listening* to a pad, which is how the screen is looked at from inside
    /// somebody's desktop session and how `--shot` composes a frame — and a
    /// picture of the login screen that drew a different row because of the
    /// flag used to take it would be a picture of something nobody will ever
    /// see. It needs no help either way: with no pad being read nothing will
    /// ever be seen from one, and the first key pressed settles it.
    fn button_legend(&self) -> cedm::ui::Legend {
        cedm::ui::Legend {
            pad: self.hands_on_pad.unwrap_or(self.legend.pad),
            ..self.legend
        }
    }

    /// Whether the session menu is up and may be answered.
    fn menu_live(&self) -> bool {
        self.menu_opened.is_some() && self.menu_closing.is_none()
    }

    fn open_session_menu(&mut self) {
        if self.sessions.is_empty() || !matches!(self.stage, Stage::Choose) {
            return;
        }
        self.menu_row = self.selected_session.min(self.sessions.len() - 1);
        self.menu_opened = Some(Instant::now());
        self.menu_closing = None;
        self.focus = Focus::Session;
    }

    fn close_session_menu(&mut self) {
        if self.menu_live() {
            self.menu_closing = Some(Instant::now());
        }
    }

    /// How far out of its anchor the menu is, and whether it has finished
    /// leaving — in which case it stops being drawn at all.
    fn menu_progress(&mut self, now: Instant) -> Option<f32> {
        let opened = self.menu_opened?;
        if let Some(closing) = self.menu_closing {
            let progress = now.duration_since(closing).as_secs_f32() / cedm::ui::MENU_UNFOLD;
            if progress >= 1.0 {
                self.menu_opened = None;
                self.menu_closing = None;
                return None;
            }
            return Some(1.0 - ease(progress));
        }
        Some(ease(
            (now.duration_since(opened).as_secs_f32() / cedm::ui::MENU_UNFOLD).clamp(0.0, 1.0),
        ))
    }

    fn choose_session(&mut self, index: usize) {
        if index >= self.sessions.len() {
            return;
        }
        self.selected_session = index;
        if self.other_account_selected() {
            self.other_session = index;
        } else {
            self.user_sessions[self.selected_user] = index;
        }
        self.close_session_menu();
    }

    /// The menu owns every direction while it is up, exactly as the on-screen
    /// keyboard does: a panel standing over the column is what the next press
    /// is about.
    fn apply_menu_action(&mut self, action: Action) {
        let count = self.sessions.len();
        if count == 0 {
            self.close_session_menu();
            return;
        }
        match action {
            Action::Up | Action::Previous => {
                self.menu_row = (self.menu_row + count - 1) % count;
            }
            Action::Down | Action::Next => {
                self.menu_row = (self.menu_row + 1) % count;
            }
            Action::Accept | Action::Submit => {
                let row = self.menu_row;
                self.choose_session(row);
            }
            Action::Back | Action::ToggleKeyboard => self.close_session_menu(),
            Action::Left | Action::Right => {}
        }
    }

    fn footer_index(&self, item: FooterItem) -> Option<usize> {
        self.footer.iter().position(|candidate| *candidate == item)
    }

    /// Ask the machine to sleep, restart or shut down.
    ///
    /// Any conversation in progress is abandoned first. A machine that is
    /// about to go down must not be left holding an open PAM attempt, and the
    /// answer already typed into the field is not something to keep across an
    /// action the user chose instead of signing in.
    fn request_power(&mut self, action: cedm::power::Action) {
        if !self.config.power.allows(action) {
            return;
        }
        if self.args.preview {
            self.transition_to(Stage::Error(cedm::i18n::fill(
                cedm::i18n::text().power_not_in_preview,
                action.label(),
            )));
            self.focus = Focus::Continue;
            return;
        }
        self.cancel();
        if let Err(message) = cedm::power::request(action) {
            self.transition_to(Stage::Error(message));
            self.focus = Focus::Continue;
        }
    }

    fn click(&mut self, target: Target) {
        if !self.target_is_active(target) {
            return;
        }
        self.click_target(target);
        self.sync_desired_column();
    }

    fn click_target(&mut self, target: Target) {
        match target {
            Target::User(index) => {
                self.select_user(index);
                self.focus = Focus::Users;
            }
            Target::OtherAccount => {
                self.select_other_account();
                self.focus = Focus::Users;
            }
            Target::PreviousUser => {
                self.cycle_user(-1);
                self.focus = Focus::Users;
            }
            Target::NextUser => {
                self.cycle_user(1);
                self.focus = Focus::Users;
            }
            Target::PreviousSession => {
                self.cycle_session(-1);
                self.focus = Focus::Session;
            }
            Target::NextSession => {
                self.cycle_session(1);
                self.focus = Focus::Session;
            }
            Target::Session => {
                self.focus = Focus::Session;
                self.open_session_menu();
            }
            Target::SessionOption(index) => self.choose_session(index),
            Target::DismissMenu => self.close_session_menu(),
            Target::Continue | Target::Retry => {
                self.focus = Focus::Continue;
                if matches!(
                    self.stage,
                    Stage::Username { .. } | Stage::Authenticating { .. }
                ) {
                    self.submit();
                } else {
                    self.begin_login();
                }
            }
            Target::Back => self.cancel(),
            Target::Power(action) => self.request_power(action),
            Target::DifferentUser => {
                self.cancel();
                self.select_other_account();
                self.focus = Focus::Users;
            }
            Target::Prompt => {
                self.focus = Focus::Prompt;
                if matches!(
                    self.stage,
                    Stage::Username { .. } | Stage::Authenticating { .. }
                ) {
                    self.offer_keyboard();
                } else {
                    self.begin_login();
                }
            }
            Target::ShowKeyboard => self.open_keyboard(),
            Target::Key(row, column)
                if self.keyboard_opened.is_some()
                    && self.keyboard_closing.is_none()
                    && matches!(
                        self.stage,
                        Stage::Username { .. } | Stage::Authenticating { .. }
                    ) =>
            {
                self.board.select(row, column);
                self.press_board();
            }
            Target::Key(..) => {}
        }
    }

    fn hover(&mut self, target: Target) {
        if !self.target_is_active(target) {
            return;
        }
        let focus = match target {
            Target::User(..) | Target::OtherAccount | Target::PreviousUser | Target::NextUser => {
                Focus::Users
            }
            Target::PreviousSession | Target::NextSession | Target::Session => Focus::Session,
            Target::Continue | Target::Retry => Focus::Continue,
            Target::Back => Focus::Back,
            Target::Prompt => Focus::Prompt,
            Target::ShowKeyboard => Focus::KeyboardToggle,
            Target::Power(action) => match self.footer_index(FooterItem::Power(action)) {
                Some(index) => Focus::Footer(index),
                None => return,
            },
            Target::DifferentUser => match self.footer_index(FooterItem::DifferentUser) {
                Some(index) => Focus::Footer(index),
                None => return,
            },
            // Hovering a row is aiming at it, which is what the controller's
            // own movement through the list means too.
            Target::SessionOption(index) => {
                self.menu_row = index;
                Focus::Session
            }
            Target::DismissMenu => return,
            Target::Key(row, column)
                if self.keyboard_opened.is_some() && self.keyboard_closing.is_none() =>
            {
                self.board.select(row, column);
                Focus::Keyboard
            }
            Target::Key(..) => return,
        };
        self.focus = focus;
        self.sync_desired_column();
    }

    fn target_is_active(&self, target: Target) -> bool {
        match target {
            Target::User(..)
            | Target::OtherAccount
            | Target::PreviousUser
            | Target::NextUser
            | Target::PreviousSession
            | Target::NextSession
            | Target::Session => matches!(self.stage, Stage::Choose),
            Target::Continue => {
                matches!(
                    self.stage,
                    Stage::Choose | Stage::Username { .. } | Stage::Authenticating { .. }
                )
            }
            Target::Retry => matches!(self.stage, Stage::Error(_)),
            Target::Back => matches!(
                self.stage,
                Stage::Username { .. }
                    | Stage::Authenticating { .. }
                    | Stage::Busy(_)
                    | Stage::Error(_)
            ),
            Target::Prompt | Target::ShowKeyboard => {
                matches!(
                    self.stage,
                    Stage::Username { .. } | Stage::Authenticating { .. }
                )
            }
            // The bottom row belongs to the machine rather than to the login,
            // so it stays live on every screen — including a failed attempt,
            // which is exactly when somebody may want to turn the machine off
            // instead of trying again. Not during the handover: the session is
            // already starting and there is nothing left to interrupt.
            Target::Power(action) => {
                self.config.power.allows(action) && !matches!(self.stage, Stage::Departing { .. })
            }
            Target::DifferentUser => !matches!(self.stage, Stage::Departing { .. }),
            // The menu answers for itself while it is up, and does not exist
            // when it is not.
            Target::SessionOption(index) => self.menu_live() && index < self.sessions.len(),
            Target::DismissMenu => self.menu_live(),
            Target::Key(..) => {
                matches!(
                    self.stage,
                    Stage::Username { .. } | Stage::Authenticating { .. }
                ) && self.keyboard_opened.is_some()
                    && self.keyboard_closing.is_none()
            }
        }
    }

    /// The column, as rows of things a controller can move between.
    ///
    /// Built per frame from what is actually on screen rather than written out
    /// per screen, so a focus can never land on a control this phase does not
    /// draw — the bug that a table of hard-coded transitions invites every
    /// time a row is added to one screen and not another.
    fn focus_grid(&self) -> Vec<Vec<Focus>> {
        let mut grid: Vec<Vec<Focus>> = Vec::new();
        match &self.stage {
            Stage::Choose => {
                grid.push(vec![Focus::Users]);
                grid.push(vec![Focus::Prompt, Focus::Continue]);
                grid.push(vec![Focus::Session]);
            }
            Stage::Username { .. } | Stage::Authenticating { .. } => {
                grid.push(vec![Focus::Back]);
                grid.push(vec![Focus::Prompt, Focus::Continue]);
                if self.keyboard_opened.is_none() {
                    grid.push(vec![Focus::KeyboardToggle]);
                }
            }
            Stage::Error(_) => {
                grid.push(vec![Focus::Back]);
                grid.push(vec![Focus::Continue]);
            }
            Stage::Busy(_) => grid.push(vec![Focus::Back]),
            Stage::Departing { .. } => return Vec::new(),
        }
        if !self.footer.is_empty() {
            grid.push((0..self.footer.len()).map(Focus::Footer).collect());
        }
        grid
    }

    /// Where the current focus sits in that grid, or the first cell when it
    /// is not in it at all — which happens legitimately, on the frame a screen
    /// changes under a focus the new screen has no room for.
    fn focus_position(&self, grid: &[Vec<Focus>]) -> (usize, usize) {
        grid.iter()
            .enumerate()
            .find_map(|(row, cells)| {
                cells
                    .iter()
                    .position(|cell| *cell == self.focus)
                    .map(|column| (row, column))
            })
            .unwrap_or((0, 0))
    }

    fn move_focus(&mut self, rows: isize, columns: isize) {
        let grid = self.focus_grid();
        if grid.is_empty() {
            return;
        }
        let (row, column) = self.focus_position(&grid);
        if rows == 0 {
            let cells = &grid[row];
            let next = (column as isize + columns).rem_euclid(cells.len() as isize) as usize;
            self.focus = cells[next];
            self.desired_column = next;
            return;
        }
        // Vertical movement travels along the column the user last chose, not
        // the one they happen to be standing in. Rows are different widths —
        // the session row holds one control and the bottom row holds four — so
        // clamping to the row being passed through would quietly walk the
        // focus leftwards every time it crossed a narrow one.
        let next_row = (row as isize + rows).rem_euclid(grid.len() as isize) as usize;
        let cells = &grid[next_row];
        self.focus = cells[self.desired_column.min(cells.len() - 1)];
    }

    /// Take the column the pointer just chose as the one to travel along.
    fn sync_desired_column(&mut self) {
        let grid = self.focus_grid();
        if !grid.is_empty() {
            self.desired_column = self.focus_position(&grid).1;
        }
    }

    /// What pressing South does, wherever the focus is.
    fn accept_focus(&mut self) {
        match self.focus {
            Focus::Footer(index) => match self.footer.get(index).copied() {
                Some(FooterItem::Power(action)) => self.request_power(action),
                Some(FooterItem::DifferentUser) => {
                    self.cancel();
                    self.select_other_account();
                    self.focus = Focus::Users;
                }
                None => {}
            },
            Focus::Back => self.cancel(),
            Focus::KeyboardToggle => self.open_keyboard(),
            Focus::Session => self.open_session_menu(),
            Focus::Prompt if matches!(self.stage, Stage::Choose) => self.begin_login(),
            Focus::Prompt
                if matches!(
                    self.stage,
                    Stage::Username { .. } | Stage::Authenticating { .. }
                ) =>
            {
                self.open_keyboard()
            }
            _ => match self.stage {
                Stage::Username { .. } | Stage::Authenticating { .. } => self.submit(),
                Stage::Choose | Stage::Error(_) => self.begin_login(),
                _ => {}
            },
        }
    }

    fn poll_auth(&mut self) {
        let Some(actor) = &self.auth else { return };
        let mut events = Vec::new();
        while let Some(event) = actor.try_recv() {
            events.push(event);
        }
        for event in events {
            if event.attempt() != self.attempt {
                continue;
            }
            if !auth_event_allowed(&self.stage, &event) {
                continue;
            }
            match event {
                AuthEvent::Prompt {
                    message, secret, ..
                } if matches!(self.stage, Stage::Busy(_) | Stage::Authenticating { .. }) => {
                    self.input.zeroize();
                    self.board = Board::default();
                    let typed = self.take_typed_ahead();
                    self.transition_to(Stage::Authenticating {
                        prompt: translated_prompt(message),
                        secret,
                    });
                    self.focus = Focus::Prompt;
                    match typed {
                        // Answering already, on a keyboard the greeter can now
                        // be sure of. Raising the on-screen board over a field
                        // with typing already in it would be offering a worse
                        // copy of the keys the answer is coming from.
                        Some(text) => self.input = text,
                        None if secret => self.offer_keyboard(),
                        None => {}
                    }
                }
                AuthEvent::Status { message, .. }
                    if matches!(self.stage, Stage::Busy(_) | Stage::Authenticating { .. }) =>
                {
                    self.transition_to(Stage::Busy(message))
                }
                AuthEvent::Authenticated { .. } if matches!(self.stage, Stage::Busy(_)) => {
                    self.input.zeroize();
                    self.forget_typed_ahead();
                    self.hide_keyboard(Focus::Continue);
                    self.transition_to(Stage::Departing {
                        started: Instant::now(),
                        sent: false,
                    });
                }
                // greetd starts the authenticated session only after its
                // greeter terminates. The 300 ms departure above leaves the
                // wallpaper as the final visible frame before we exit.
                AuthEvent::Started { attempt }
                    if matches!(self.stage, Stage::Departing { sent: true, .. }) =>
                {
                    let Some(pending) = self
                        .pending_attempt
                        .as_ref()
                        .filter(|pending| pending.id == attempt)
                    else {
                        continue;
                    };
                    if let Some(username) = &pending.preference_username {
                        self.preferences.record_success(
                            &self.config,
                            username,
                            &pending.session_id,
                        );
                    } else {
                        self.preferences
                            .record_anonymous_success(&self.config, &pending.session_id);
                    }
                    if let Err(error) = self.preferences.save() {
                        tracing::warn!(%error, "could not save successful login preferences");
                    }
                    self.session_started = true;
                }
                AuthEvent::Failed {
                    failure, message, ..
                } if matches!(
                    self.stage,
                    Stage::Busy(_)
                        | Stage::Authenticating { .. }
                        | Stage::Departing { sent: true, .. }
                ) =>
                {
                    self.input.zeroize();
                    self.forget_typed_ahead();
                    self.hide_keyboard(Focus::Continue);
                    if refusal_sounds(failure) {
                        self.sounds.error();
                    }
                    let shown = refusal(failure, message, self.typed_account());
                    self.transition_to(Stage::Error(shown));
                    self.focus = Focus::Continue;
                    self.pending_attempt = None;
                }
                AuthEvent::Prompt { .. }
                | AuthEvent::Status { .. }
                | AuthEvent::Authenticated { .. }
                | AuthEvent::Started { .. }
                | AuthEvent::Failed { .. }
                | AuthEvent::Cancelled { .. } => {}
            }
        }
    }

    fn tick(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        self.poll_auth();
        if self.session_started {
            event_loop.exit();
            return;
        }
        if now >= self.next_poll {
            let actions = self.controller.poll(self.started.elapsed());
            // A press on the pad is a hand on the pad, whatever the account
            // being looked at last did. Asked of the poll rather than of each
            // action because it is one fact about one moment — see
            // `Application::hands_on_pad`.
            if !actions.is_empty() {
                self.hands_on_pad = Some(true);
            }
            for action in actions {
                self.apply_action(action);
            }
            self.next_poll = now + POLL_INTERVAL;
        }
        if now >= self.next_clock_read {
            self.now = cedm::clock::Now::read();
            self.next_clock_read = now + CLOCK_INTERVAL;
        }
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        visual::theme::animate(dt);
        if self
            .stage_transition
            .as_ref()
            .is_some_and(|transition| now.duration_since(transition.started).as_secs_f32() >= 0.28)
        {
            self.stage_transition = None;
        }
        // LineXinBar loads the saved accent for an enumerated user
        // immediately. On a passwordless or very fast login, the departure
        // can finish before CEDM's palette travel. Keep the clean wallpaper
        // frame until this attempt's selected endpoint is exact, avoiding a
        // first-frame colour snap for profiles whose setting CEDM can read.
        let accent_settled = self
            .pending_attempt
            .as_ref()
            .filter(|pending| pending.id == self.attempt)
            .is_some_and(|pending| visual::theme::settled_to(&pending.accent));
        let start_request = match &mut self.stage {
            Stage::Departing { started, sent }
                if !*sent
                    && ready_to_start_session(now.duration_since(*started), accent_settled) =>
            {
                *sent = true;
                self.pending_attempt
                    .as_ref()
                    .filter(|pending| pending.id == self.attempt)
                    .map(|pending| {
                        (
                            pending.id,
                            pending.accent.clone(),
                            pending.wallpaper_material.clone(),
                        )
                    })
            }
            _ => None,
        };
        if let Some((attempt, accent, material)) = start_request {
            let handoff = self.wallpaper_clock.capture(&accent, Some(&material));
            if !self
                .auth
                .as_ref()
                .is_some_and(|actor| actor.send(AuthCommand::Start { attempt, handoff }))
            {
                self.pending_attempt = None;
                self.transition_to(Stage::Error(
                    cedm::i18n::text().worker_unavailable.to_string(),
                ));
            }
        } else if matches!(self.stage, Stage::Departing { sent: true, .. })
            && self.pending_attempt.is_none()
        {
            self.transition_to(Stage::Error(cedm::i18n::text().attempt_lost.to_string()));
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            self.next_poll.min(now + Duration::from_millis(16)),
        ));
    }

    /// Work out which displays the surface covers, and where each one is on it.
    ///
    /// Asked of the compositor rather than assumed, and asked again on every
    /// resize: the greeter draws into one surface, and whether that surface is
    /// one screen or a row of them is not something a client can know without
    /// looking. What comes back is one display per output whenever the outputs
    /// account for this surface exactly, and the whole surface as one display
    /// whenever they do not — a nested development window, a mirrored pair, an
    /// output whose mode has been announced but not applied.
    fn refresh_displays(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let size = window.inner_size();
        let (width, height) = (size.width.max(1) as f32, size.height.max(1) as f32);
        // A developer's window has no layout to cut, so the displays to review
        // against are given on the command line instead. See `Args::displays`.
        self.displays = if self.args.displays.is_empty() {
            let monitors = window
                .available_monitors()
                .map(|monitor| {
                    let monitor = cedm::displays::Monitor {
                        name: monitor.name(),
                        position: monitor.position().into(),
                        size: monitor.size().into(),
                    };
                    // What the compositor said, before anything is concluded
                    // from it. A surface that was cut in the wrong place — or
                    // not cut at all — is a layout that did not describe it,
                    // and the layout is the half of that a log can carry.
                    tracing::debug!(
                        connector = monitor.name.as_deref().unwrap_or("unnamed"),
                        x = monitor.position.0,
                        y = monitor.position.1,
                        width = monitor.size.0,
                        height = monitor.size.1,
                        "an output the compositor advertises"
                    );
                    monitor
                })
                .collect::<Vec<_>>();
            cedm::displays::split(width, height, &monitors)
        } else {
            self.args.displays.clone()
        };
        tracing::info!(
            surface = format!("{}x{}", size.width, size.height),
            displays = self.displays.len(),
            "composing the login screen on every display"
        );
        for display in &self.displays {
            let [x, y, w, h] = display.rect;
            tracing::debug!(x, y, width = w, height = h, "a display");
        }
    }

    /// How far this login screen has risen into view, on its own smooth curve.
    ///
    /// Symmetrical: [`ease`] is a smoothstep, so the rise leaves nothing and
    /// settles into place at the same rate, with no edge at either end. A
    /// linear one arrives by stopping, which reads as a cut however long it is
    /// given.
    ///
    /// A single captured frame is fully arrived by definition — `--shot` draws
    /// one frame and exits, and a picture of the login screen a fraction into
    /// its own entrance is a picture of nothing much.
    fn arrival(&mut self, now: Instant) -> f32 {
        if self.args.shot.is_some() {
            return 1.0;
        }
        let began = *self.first_frame.get_or_insert(now);
        ease(now.duration_since(began).as_secs_f32() / cedm::ui::ARRIVAL)
    }

    fn render(&mut self) -> anyhow::Result<()> {
        let now = Instant::now();
        // Advanced first, because it can retire itself: a menu that has
        // finished leaving stops existing before anything reads the stage it
        // was standing over.
        let leaving = matches!(self.stage, Stage::Departing { .. });
        let menu_live = self.menu_live();
        let menu = self.menu_progress(now).map(|progress| Menu {
            selected: self.menu_row,
            progress,
            interactive: menu_live && !leaving,
        });
        let wallpaper_time = self.wallpaper_clock.elapsed().as_secs_f32();
        let risen = self.arrival(now);
        let arrival = self.keyboard_arrival(now);
        // A frame drawn on no display at all is a black screen with a working
        // login behind it, which is worse than any answer about where the
        // displays are. There is no path here that leaves the list empty; this
        // is the one place where that mattering enough to check is cheap.
        let (width, height) = self.renderer.as_ref().context("renderer not ready")?.size();
        let whole = [cedm::displays::Display::whole(width as f32, height as f32)];
        let displays: &[cedm::displays::Display] = if self.displays.is_empty() {
            &whole
        } else {
            &self.displays
        };
        let departure = match &self.stage {
            Stage::Departing { started, .. } => {
                ease((now.duration_since(*started).as_secs_f32() / 0.3).clamp(0.0, 1.0))
            }
            _ => 0.0,
        };
        let phase = match &self.stage {
            Stage::Choose => Phase::Choose,
            Stage::Username { error } => Phase::Username {
                input: &self.input,
                error: *error,
            },
            Stage::Authenticating { prompt, secret } => Phase::Authenticating {
                prompt,
                secret: *secret,
                input: &self.input,
            },
            Stage::Busy(message) => Phase::Busy(message),
            Stage::Error(message) => Phase::Error(message),
            Stage::Departing { .. } => Phase::Departing(cedm::i18n::text().opening_session),
        };
        let (previous_phase, transition_progress) = self
            .stage_transition
            .as_ref()
            .map(|transition| {
                (
                    Some(transition.previous.phase()),
                    ease(
                        (now.duration_since(transition.started).as_secs_f32() / 0.28)
                            .clamp(0.0, 1.0),
                    ),
                )
            })
            .unwrap_or((None, 1.0));
        let board = self.keyboard_opened.as_ref().map(|_| &self.board);
        let output = cedm::ui::build(
            cedm::ui::View {
                users: &self.users,
                selected_user: self.selected_user,
                sessions: &self.sessions,
                selected_session: self.selected_session,
                focus: self.focus,
                phase,
                previous_phase,
                transition_progress,
                keyboard: board,
                keyboard_interactive: self.keyboard_closing.is_none(),
                keyboard_arrival: arrival,
                time: wallpaper_time,
                arrival: risen,
                departure,
                carousel_shift: self.carousel_shift(now),
                footer: &self.footer,
                now: self.now,
                clock: self.clock,
                session_menu: menu,
                legend: self.button_legend(),
            },
            displays,
        );
        self.hits = output.hits;
        let renderer = self.renderer.as_mut().context("renderer not ready")?;
        renderer.render(&output.scene, wallpaper_time)
    }

    /// Write the frame that was just drawn out as a PNG.
    fn save_shot(&mut self, path: &std::path::Path) -> anyhow::Result<()> {
        let (width, height, pixels) = self
            .renderer
            .as_mut()
            .context("renderer not ready")?
            .capture()?;
        let file = std::io::BufWriter::new(
            std::fs::File::create(path)
                .with_context(|| format!("could not create {}", path.display()))?,
        );
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()?
            .write_image_data(&pixels)
            .with_context(|| format!("could not write {}", path.display()))?;
        tracing::info!(path = %path.display(), width, height, "wrote a frame");
        Ok(())
    }
}

impl ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attributes =
            WindowAttributes::default().with_title("Console Experience Desktop Manager");
        attributes = match self.args.size {
            // Physical, because that is the unit the layout is worked out in
            // and the whole point of asking for a size is to choose it.
            Some((width, height)) => attributes.with_inner_size(PhysicalSize::new(width, height)),
            None => attributes.with_inner_size(LogicalSize::new(1280, 720)),
        };
        if !self.args.windowed {
            attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        match event_loop.create_window(attributes) {
            Ok(window) => {
                let window = Arc::new(window);
                window.set_ime_allowed(true);
                let faces = self.load_faces();
                match pollster::block_on(Renderer::new(window.clone(), &faces)) {
                    Ok(renderer) => {
                        self.renderer = Some(renderer);
                        self.window = Some(window);
                        self.refresh_displays();
                    }
                    Err(error) => {
                        self.fatal = Some(error);
                        event_loop.exit();
                    }
                }
            }
            Err(error) => {
                self.fatal = Some(error.into());
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                // A surface that changed size is, on this side of a compositor
                // that extends one across every output, a display having been
                // plugged in or unplugged.
                self.refresh_displays();
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.render() {
                    self.fatal = Some(error);
                    event_loop.exit();
                    return;
                }
                // After a frame rather than instead of one: the capture reads
                // back what `render` composed, so there has to have been one.
                if let Some(path) = self.args.shot.clone() {
                    if let Err(error) = self.save_shot(&path) {
                        self.fatal = Some(error);
                    }
                    event_loop.exit();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = position;
                if let Some(target) =
                    cedm::ui::target_at(&self.hits, self.pointer.x as f32, self.pointer.y as f32)
                {
                    self.hover(target);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(target) =
                    cedm::ui::target_at(&self.hits, self.pointer.x as f32, self.pointer.y as f32)
                {
                    self.click(target);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                // And a key is a hand on a keyboard. The board's own keys never
                // arrive here — they are pressed with whatever is driving the
                // column and reach `act_on_key` instead — so this is a real
                // key on a real keyboard every time.
                self.hands_on_pad = Some(false);
                if self.keyboard_opened.is_some() && input_stage(&self.stage) {
                    match &event.logical_key {
                        Key::Named(NamedKey::Backspace) | Key::Character(_) => {
                            self.hide_keyboard(Focus::Prompt)
                        }
                        _ => {}
                    }
                }
                match event.logical_key {
                    Key::Named(NamedKey::ArrowLeft) => self.apply_action(Action::Left),
                    Key::Named(NamedKey::ArrowRight) => self.apply_action(Action::Right),
                    Key::Named(NamedKey::ArrowUp) => self.apply_action(Action::Up),
                    Key::Named(NamedKey::ArrowDown) => self.apply_action(Action::Down),
                    Key::Named(NamedKey::Enter) => {
                        if input_stage(&self.stage) {
                            self.button_press(Self::submit)
                        } else {
                            self.apply_action(Action::Accept)
                        }
                    }
                    Key::Named(NamedKey::Escape) => self.button_press(Self::cancel),
                    Key::Named(NamedKey::Tab)
                        if input_stage(&self.stage) && self.keyboard_opened.is_some() =>
                    {
                        let focus = if self.modifiers.shift_key() {
                            Focus::Back
                        } else {
                            Focus::Continue
                        };
                        self.button_press(|application| {
                            application.hide_keyboard(focus);
                            application.focus = focus;
                        });
                    }
                    Key::Named(NamedKey::Tab)
                        if input_stage(&self.stage) || matches!(self.stage, Stage::Error(_)) =>
                    {
                        self.apply_action(tab_action(&self.stage, self.modifiers.shift_key()))
                    }
                    Key::Named(NamedKey::Tab) => {
                        self.apply_action(tab_action(&self.stage, self.modifiers.shift_key()))
                    }
                    Key::Named(NamedKey::Space) if !input_stage(&self.stage) => {
                        self.apply_action(Action::Accept)
                    }
                    Key::Named(NamedKey::Backspace) if input_stage(&self.stage) => self.pop_input(),
                    Key::Character(text) if input_stage(&self.stage) => {
                        for character in text.chars().filter(|character| !character.is_control()) {
                            self.push_input(character);
                        }
                    }
                    // A chord is a command, not the start of a password, and
                    // this greeter has no commands: it is only ever the tail
                    // of one the compositor did not take.
                    Key::Character(text)
                        if !self.modifiers.control_key()
                            && !self.modifiers.alt_key()
                            && !self.modifiers.super_key() =>
                    {
                        self.type_into_login(&text)
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.tick(event_loop);
    }
}

fn ease(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn ready_to_start_session(departure_elapsed: Duration, accent_settled: bool) -> bool {
    departure_elapsed >= Duration::from_millis(300) && accent_settled
}

fn input_stage(stage: &Stage) -> bool {
    matches!(stage, Stage::Username { .. } | Stage::Authenticating { .. })
}

fn tab_action(stage: &Stage, reverse: bool) -> Action {
    if input_stage(stage) || matches!(stage, Stage::Error(_)) {
        if reverse {
            Action::Left
        } else {
            Action::Right
        }
    } else if reverse {
        Action::Up
    } else {
        Action::Down
    }
}

/// What a failed attempt says under the "Try again" button.
///
/// A password PAM would not take is the one failure whose own words help
/// nobody: greetd sends it as `authentication error: AUTH_ERR`, which reads as
/// a fault in the machine rather than as a mistyped password, and sends the
/// person at the keyboard looking for a broken greeter instead of trying
/// again. It is also the one failure they can already explain themselves.
///
/// Every other failure is the opposite case. A socket that would not open or a
/// session that would not start is news, it is the machine's news, and
/// greetd's own sentence is the only description of it anybody has.
///
/// PAM does not say which half of a sign-in was wrong, and neither does this.
/// On the route where the account name was typed as well, naming the password
/// as the wrong half would be a guess, and a guess that quietly answers "does
/// this account exist" for anyone who cares to ask.
///
/// Both sentences are written to a line. The reserved line under the button is
/// one line tall at every size, and text laid out past it is cut off rather
/// than wrapped onto the column, so a longer sentence is not a longer sentence
/// — it is half a sentence. Measured in the shipped face at the tightest of
/// the sizes it is set at, these run to about nine tenths of that line.
fn refusal(failure: Failure, message: String, typed_account: bool) -> String {
    let text = cedm::i18n::text();
    match failure {
        Failure::Rejected if typed_account => text.incorrect_account_or_password.to_string(),
        Failure::Rejected => text.incorrect_password.to_string(),
        Failure::Service if message.trim().is_empty() => text.service_refused.to_string(),
        // Deliberately not translated. This is greetd's sentence about this
        // machine — a socket that would not open, a session that would not
        // start — and it is the only description of it anybody has; a login
        // screen that replaced it with a translated generality would be
        // throwing the news away to say something in the right language.
        Failure::Service => message,
    }
}

/// The question PAM asked, in the reader's language where it is one of the
/// questions PAM asks in English.
///
/// PAM localises its own conversation, but only where the process holding it
/// has a language to localise into — and that process is greetd, a system unit
/// whose environment is whatever the unit gives it. What comes back over the
/// socket on nearly every machine is therefore `Password:`, in the middle of a
/// column that is otherwise entirely in Polish.
///
/// Everything the greeter does not recognise is passed through exactly as it
/// arrived, and that is the more important half: an unrecognised prompt is a
/// PAM module with something specific to say — a hardware token, a one-time
/// code, an expiring password — and a login screen that guessed at those would
/// be answering a question nobody asked. See [`cedm::i18n::Strings::prompt`].
fn translated_prompt(message: String) -> String {
    match cedm::i18n::text().prompt(&message) {
        Some(translated) => translated.to_string(),
        None => message,
    }
}

/// Whether a refused attempt is one this screen makes a noise about.
///
/// PAM's refusal, and only PAM's. That is the user having got something wrong,
/// and the sound is there because of the interval it lands in: the answer went
/// away, PAM took its time over it, and by the time the refusal comes back the
/// person who typed it is very often looking somewhere else.
///
/// A failure of the login *service* — greetd gone, the socket, this greeter's
/// own worker — puts the same red screen up and is not that. Nothing the user
/// types will fix it, so a sound that said "try again" would be sending them
/// round a loop; and answering both the same way would teach the noise to mean
/// nothing in particular.
fn refusal_sounds(failure: Failure) -> bool {
    match failure {
        Failure::Rejected => true,
        Failure::Service => false,
    }
}

fn auth_event_allowed(stage: &Stage, event: &AuthEvent) -> bool {
    match event {
        AuthEvent::Prompt { .. } | AuthEvent::Status { .. } => {
            matches!(stage, Stage::Busy(_) | Stage::Authenticating { .. })
        }
        AuthEvent::Authenticated { .. } => matches!(stage, Stage::Busy(_)),
        AuthEvent::Started { .. } => matches!(stage, Stage::Departing { sent: true, .. }),
        AuthEvent::Failed { .. } => matches!(
            stage,
            Stage::Busy(_) | Stage::Authenticating { .. } | Stage::Departing { sent: true, .. }
        ),
        AuthEvent::Cancelled { .. } => true,
    }
}

fn carousel_delta(index: usize, selected: usize, count: usize) -> isize {
    if count <= 1 {
        return 0;
    }
    let forward = (index + count - selected) % count;
    if forward <= count / 2 {
        forward as isize
    } else {
        forward as isize - count as isize
    }
}

fn resolve_session_index<'a>(
    sessions: &[Session],
    preferred_ids: impl IntoIterator<Item = &'a str>,
) -> usize {
    preferred_ids
        .into_iter()
        .find_map(|id| sessions.iter().position(|session| session.id == id))
        .or_else(|| sessions.iter().position(|session| session.line_xin_bar))
        .or_else(|| {
            sessions.iter().position(|session| {
                session.id.eq_ignore_ascii_case("plasma")
                    || session.name.eq_ignore_ascii_case("plasma")
                    || session
                        .desktop_names
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case("KDE"))
            })
        })
        .unwrap_or(0)
}

/// Write the display configuration the greeter's compositor should come up in,
/// and say whose settings it came from.
///
/// The login screen is drawn before anybody has chosen an account, so there is
/// no user to ask — and a mode, a layout and high dynamic range are all
/// settled before a client exists to have an opinion. What it uses instead is
/// the account that signed in last, which is the same account whose accent the
/// greeter already opens in and, on all but a shared machine, the one about to
/// sign in again.
///
/// A machine nobody has signed into yet, an account that has published
/// nothing, a `preferences.toml` that has been cleared: all of them write a
/// configuration with no displays in it, which is a compositor that brings
/// every screen up the way it would have anyway. This never fails for want of
/// an answer, because the alternative to an answer is no login screen.
fn write_compositor_config(path: &Path, greeter_arguments: &[String]) -> anyhow::Result<Written> {
    let preferences = Preferences::load();
    let state = State::load();
    let last = preferences
        .last_user
        .as_deref()
        .or(state.last_user.as_deref());
    let look = last.and_then(|name| {
        let user = cedm::users::discover()
            .into_iter()
            .find(|user| user.name == name)?;
        // The published copy first, and the account's own settings only if
        // this greeter can somehow read them — a development machine, or a
        // home the administrator has opened up. Both are the same file at
        // one remove; see `cedm::look`.
        cedm::look::published(&user.name, user.uid)
            .or_else(|| Some(cedm::look::Look::read(&user.home, None)))
            .filter(|look| *look != cedm::look::Look::default())
            .map(|look| (user.name, look))
    });
    let (account, look) = match look {
        Some((account, look)) => (Some(account), look),
        None => (None, cedm::look::Look::default()),
    };
    // This program, by the path it was actually started from: the compositor
    // starts it again as its shell, and a greeter installed somewhere other
    // than /usr/bin must not start whatever is first on a PATH by that name.
    //
    // Quoted as a shell would quote it, because the far end splits this string
    // the way a shell would split it — an installation path with a space in it
    // is otherwise two arguments, neither of which exists.
    let executable = std::env::current_exe()
        .context("could not find this program's own path for the compositor to start")?;
    let executable = executable
        .to_str()
        .context("this program's own path is not usable as a command")?;
    let shell = shlex::try_join(
        std::iter::once(executable).chain(greeter_arguments.iter().map(String::as_str)),
    )
    .context("the greeter's own command line cannot be written as one")?;
    let document = look.compositor_config(&shell, cedm::clock::Now::read())?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    std::fs::write(path, document)
        .with_context(|| format!("could not write {}", path.display()))?;
    let accent = user_accent_for_the_login_screen(account.as_deref(), &look);
    let theme = user_theme_for_the_login_screen(account.as_deref(), &look);
    Ok(Written {
        // The wallpaper's half alone: what this record is for is a compositor
        // with no client yet, and what it draws is a wallpaper.
        wallpaper: greeter_wallpaper(&accent, &theme.wallpaper),
        accent: Some(accent),
        account,
    })
}

/// Warm the displays the way the machine warms them, if this compositor lets
/// a client do it at all.
///
/// From the last signed-in account's published look, which is the same source
/// the greeter's compositor is configured from and the same account the login
/// screen opens on. Set once, at start-up, and left alone afterwards.
///
/// Deliberately not per profile, unlike the accent. The accent belongs to
/// whoever is being looked at and travels as the carousel moves, which is the
/// point of it. Warmth belongs to the room at this hour: a filter that came
/// and went as somebody stepped between two accounts would be the panel
/// changing colour under their hand for a reason nothing on screen explains,
/// and on a shared machine it would announce which of the accounts keeps a
/// night light.
///
/// This is the path that runs under Cage, which is the compositor a default
/// installation has. Where LineXinBar's compositor holds the seat instead, it
/// has already warmed these displays from the configuration
/// [`write_compositor_config`] wrote for it, and it offers no protocol for a
/// client to warm them again — so this finds nothing to bind and does nothing.
/// Which speakers the login screen comes out of, and how loud.
///
/// From the last signed-in account's published look, the same source the
/// compositor is configured from and the same account the login screen opens on
/// — and for the same reason as all of it: the greeter runs as an account of its
/// own with no sound server, so this is not something it can find out. What it
/// gets here is what that user's own session was playing through, asked of the
/// server by the user themselves as their session started. See
/// [`cedm::audio`].
///
/// Deliberately not per profile, exactly like the night light. Which card the
/// machine is heard on is a fact about the room, not about whoever the carousel
/// is showing, and a login screen that moved between two accounts' speakers as
/// somebody paged past them would be answering the same button in two different
/// places.
///
/// Nothing published is an empty answer rather than a guess, and the guess is
/// then made further down where it can at least be logged. A machine nobody has
/// signed into yet has no account to have published anything.
fn sound_output(state: &State, preferences: &Preferences, users: &[User]) -> cedm::sound::Want {
    let Some(last) = preferences
        .last_user
        .as_deref()
        .or(state.last_user.as_deref())
    else {
        return cedm::sound::Want::default();
    };
    let Some(user) = users.iter().find(|user| user.name == last) else {
        return cedm::sound::Want::default();
    };
    let look = cedm::look::published(&user.name, user.uid)
        .or_else(|| Some(cedm::look::Look::read(&user.home, None)))
        .unwrap_or_default();
    cedm::sound::Want {
        card: look.sound_card.clone(),
        gain: look.sound_gain,
    }
}

fn night_light(
    state: &State,
    preferences: &Preferences,
    users: &[User],
) -> Option<cedm::gamma::NightLight> {
    let last = preferences
        .last_user
        .as_deref()
        .or(state.last_user.as_deref())?;
    let user = users.iter().find(|user| user.name == last)?;
    let look = cedm::look::published(&user.name, user.uid)
        .or_else(|| Some(cedm::look::Look::read(&user.home, None)))?;
    cedm::gamma::start(&look, cedm::clock::Now::read())
}

/// What [`write_compositor_config`] settled on.
struct Written {
    /// The account whose settings it came from, if any had any.
    account: Option<String>,
    /// The palette the login screen will open in.
    accent: Option<String>,
    /// The record that tells the greeter's compositor which wallpaper to draw
    /// until the greeter has a window, or `None` where there is no usable
    /// clock to anchor one to. See [`greeter_wallpaper`].
    wallpaper: Option<String>,
}

/// The wallpaper the greeter's own compositor should draw before the greeter
/// exists.
///
/// This is the same record a successful login hands the session, in the same
/// encoding, and it is here for the same reason: a compositor that has taken
/// the displays and has no client yet shows its clear colour, which is a black
/// screen. LineXinBar's compositor draws this wallpaper into that interval
/// instead — and, since it keeps drawing it whenever nothing else is on
/// screen, into the interval at the far end too, between the greeter fading
/// out and greetd starting the session.
///
/// Which is exactly why the accent has to be *this* accent rather than
/// whatever the compositor would have found for itself. It reads the shell
/// settings of the account it is running as, and the account it is running as
/// here is the greeter's own, which has no shell settings and no LineXinBar
/// and never will. Left to itself it would draw the default purple under a
/// login screen the user has set to blue, and put a purple flash at the very
/// handover this is meant to make invisible.
///
/// The scene carries on rather than starting at zero, because on every login
/// screen but the first of a boot something did come before it: the session
/// that just ended was drawing this same wallpaper from this same clock a
/// fraction of a second earlier. Only the first one begins the animation; the
/// rest continue it, and the greeter picks the clock up from here either way —
/// see [`cedm::handoff::SceneClock::of_this_boot`] and
/// [`cedm::handoff::SceneClock::resume`].
fn greeter_wallpaper(accent: &str, theme: &str) -> Option<String> {
    let clock = match cedm::state::wallpaper_clock_path() {
        Some(path) => cedm::handoff::SceneClock::of_this_boot(&path),
        // Nowhere to keep an anchor. The login screen still comes up and its
        // wallpaper still moves; it just starts the animation over.
        None => cedm::handoff::SceneClock::start(),
    };
    let record = clock.capture(accent, Some(theme))?;
    Some(record.encode())
}

/// The palette the login screen opens in, worked out the same way the greeter
/// itself works it out for the profile it opens on.
///
/// Kept beside [`write_compositor_config`] because the two answers have to be
/// the same one: the compositor is told which wallpaper to draw before the
/// greeter has started, and the greeter then draws its own over the top.
/// The material the login screen opens in, worked out the same way and for the
/// same reason as the palette beside it.
///
/// This one has a second job. The compositor that runs this greeter draws a
/// bridge frame before the greeter's window exists, and it runs as the greeter's
/// own account — it cannot read the settings of the person about to sign in. So
/// this answer is what goes into the handover record, and it is the only way that
/// compositor can know not to draw the water in front of a login screen coming
/// up in the plain material.
fn user_theme_for_the_login_screen(account: Option<&str>, look: &cedm::look::Look) -> Materials {
    let state = State::load();
    account
        .and_then(|name| {
            let user = cedm::users::discover()
                .into_iter()
                .find(|user| user.name == name)?;
            Some(user_theme(&state, &user))
        })
        .unwrap_or_else(|| Materials {
            wallpaper: named_or_default(look.theme(visual::theme::Part::Wallpaper)),
            icons: named_or_default(look.theme(visual::theme::Part::Icons)),
        })
}

fn named_or_default(named: Option<&'static str>) -> String {
    named.unwrap_or(cedm::accent::DEFAULT_THEME).to_string()
}

fn user_accent_for_the_login_screen(account: Option<&str>, look: &cedm::look::Look) -> String {
    let state = State::load();
    account
        .and_then(|name| {
            let user = cedm::users::discover()
                .into_iter()
                .find(|user| user.name == name)?;
            Some(user_accent(&state, &user))
        })
        .or_else(|| look.accent().map(str::to_string))
        .unwrap_or_else(|| cedm::accent::DEFAULT_ACCENT.to_string())
}

/// Prefer the user's current shell setting whenever it is readable, then the
/// copy they published on the way into their last session, then the broker's.
///
/// In that order because it is the order of freshness, and every step of it is
/// a step further from the account: the settings themselves are what the shell
/// is set to *now*, a published copy is what it was set to at the last sign-in
/// through this greeter, and broker state is whatever a privileged process was
/// told at some point. On an ordinary machine the first of those is
/// unreadable — a home directory is not the greeter's to walk into — and the
/// login screen stands or falls on the second.
/// The same three places, in the same order of freshness, for the two materials
/// the account's shell draws in.
///
/// Beside [`user_accent`] rather than folded into it: the accent is a colour this
/// screen is *tinted* with and the theme decides what it is *made of*, and only
/// one of them has ever been in the broker's state file. A machine whose broker
/// predates the setting simply falls through to the default, which is the shell's
/// own look — the right answer for a machine nobody has said is slow.
///
/// Both halves walk the same three places independently, and they have to: a
/// published look from the older shell answers both at once out of its one
/// `theme` key, and a broker that only knows the one map does the same, but a
/// current `shell.toml` can perfectly well name one half and leave the other.
fn user_theme(state: &State, user: &User) -> Materials {
    let of = |part| {
        cedm::accent::read_theme_for_home(&user.home, part)
            .or_else(|| {
                cedm::look::published(&user.name, user.uid)
                    .and_then(|look| look.theme(part))
                    .map(str::to_string)
            })
            .or_else(|| {
                match part {
                    visual::theme::Part::Wallpaper => state.theme_for(&user.name),
                    visual::theme::Part::Icons => state.icon_theme_for(&user.name),
                }
                .and_then(cedm::accent::canonical_theme)
                .map(str::to_string)
            })
            .unwrap_or_else(|| cedm::accent::DEFAULT_THEME.to_string())
    };
    Materials {
        wallpaper: of(visual::theme::Part::Wallpaper),
        icons: of(visual::theme::Part::Icons),
    }
}

/// The keyboard the on-screen board should be a picture of for `user`.
///
/// The account's own settings first and the copy they published second, which
/// is the same order of freshness [`user_accent`] walks — and then the
/// machine's own keyboard rather than the broker's state, which has never held
/// this and does not need to. A colour nobody has published has to be invented;
/// a keyboard does not, because the machine this greeter is running on has one.
///
/// The published copy is what answers on an ordinary machine, where a home
/// directory is not the greeter's to walk into. See [`cedm::look`].
fn user_keyboard(user: &User, machine: &(String, String)) -> (String, String) {
    cedm::accent::read_keyboard_for_home(&user.home)
        .or_else(|| cedm::look::published(&user.name, user.uid).and_then(|look| look.keyboard()))
        .unwrap_or_else(|| machine.clone())
}

/// Which clock an account writes a time on.
///
/// Out of the copy of `shell.toml` that account published on its way into its
/// last session, which is the one place a greeter can read it from: the
/// settings themselves are in a home directory this process has no business
/// reading, and there is nothing in broker state about a clock. An account
/// with no published look, or one written before the shell had the row, leaves
/// its language to answer — see [`cedm::clock::Clock::FromLanguage`].
fn user_clock(user: &User) -> cedm::clock::Clock {
    cedm::look::published(&user.name, user.uid)
        .map(|look| look.clock())
        .unwrap_or_default()
}

/// What an account's shell says about the row that explains its buttons.
///
/// Out of the copy of `shell.toml` that account published on its way into its
/// last session, on the clock's own terms and for the clock's own reason: the
/// settings themselves are in a home directory this process has no business
/// reading, and nothing in broker state has ever known about a button hint. An
/// account with no published look — or one written before the shell had the
/// row, which is every look on every machine today — gets the default, which
/// is a row, and a pad to draw it with.
fn user_legend(user: &User) -> cedm::ui::Legend {
    cedm::look::published(&user.name, user.uid)
        .map(|look| cedm::ui::Legend {
            shown: look.button_hints(),
            pad: look.pad_in_hand(),
        })
        .unwrap_or_default()
}

fn user_accent(state: &State, user: &User) -> String {
    cedm::accent::read_path(&cedm::accent::settings_path(&user.home))
        .or_else(|| {
            cedm::look::published(&user.name, user.uid)
                .and_then(|look| look.accent())
                .map(str::to_string)
        })
        .or_else(|| {
            state
                .accent_for(&user.name)
                .and_then(cedm::accent::canonical)
                .map(str::to_string)
        })
        .unwrap_or_else(|| cedm::accent::DEFAULT_ACCENT.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cedm::sessions::Kind;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn session(id: &str, line_xin_bar: bool) -> Session {
        Session {
            id: id.into(),
            name: id.into(),
            comment: None,
            command: vec![format!("/{id}")],
            desktop_names: vec![id.into()],
            kind: Kind::Wayland,
            source: PathBuf::from(format!("/{id}.desktop")),
            line_xin_bar,
        }
    }

    /// A greeter on the machine it is written for: a pad and nothing else.
    ///
    /// Said outright rather than left to whatever is plugged into the desk this
    /// is being run on, or the focus these tests follow would depend on it.
    fn preview_application(users: Vec<User>) -> Application {
        let mut application = console(users);
        application.keyboard_attached = false;
        application
    }

    /// And the same greeter on a desk.
    fn desk_application(users: Vec<User>) -> Application {
        let mut application = console(users);
        application.keyboard_attached = true;
        application
    }

    fn console(users: Vec<User>) -> Application {
        Application::new(
            Args {
                windowed: true,
                preview: true,
                preview_auth: false,
                list_sessions: false,
                publish_look: false,
                compositor_config: None,
                greeter_arguments: Vec::new(),
                no_gamepad: true,
                // No test opens the machine's sound device. What the sound
                // tests read is the tally of what was *asked* for, which a
                // silent greeter keeps exactly as a loud one does — see
                // [`cedm::sound::Spent`].
                no_sound: true,
                preview_menu: false,
                shot: None,
                size: None,
                displays: Vec::new(),
                language: None,
            },
            // The default policy rather than this machine's, so a desk with a
            // `/etc/cedm/config.toml` on it does not test something different
            // from a build machine without one.
            cedm::config::Config::default(),
            users,
            vec![session("lxb", true)],
        )
    }

    fn user(name: &str) -> User {
        User {
            avatar: None,
            name: name.to_lowercase(),
            display_name: name.to_string(),
            uid: 1000,
            home: PathBuf::from(format!("/home/{name}")),
            shell: PathBuf::from("/bin/sh"),
        }
    }

    /// The grid is built from what is on screen, so a screen that draws no
    /// keyboard button has no cell that could focus one. Walking every screen
    /// in every direction must never leave the focus on a control that screen
    /// does not have.
    #[test]
    fn focus_never_lands_on_a_control_the_screen_does_not_draw() {
        let mut application = preview_application(vec![user("Alex")]);
        for stage in [
            Stage::Choose,
            Stage::Username { error: None },
            Stage::Authenticating {
                prompt: "Password".into(),
                secret: true,
            },
            Stage::Busy("Checking".into()),
            Stage::Error("Denied".into()),
        ] {
            application.stage = stage;
            let cells = application
                .focus_grid()
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            assert!(!cells.is_empty());
            for action in [Action::Up, Action::Down, Action::Left, Action::Right] {
                for _ in 0..7 {
                    application.apply_action(action);
                    assert!(
                        cells.contains(&application.focus),
                        "{:?} is not on {:?}",
                        application.focus,
                        application.stage
                    );
                }
            }
        }
    }

    /// Down and back up returns to the control it left, rather than walking
    /// along the bottom row a step at a time.
    #[test]
    fn moving_between_rows_keeps_its_place_in_them() {
        let mut application = preview_application(vec![user("Alex")]);
        application.stage = Stage::Choose;
        // Through the pointer, which is how a focus arrives anywhere other
        // than by moving: it is what anchors the column to travel along.
        application.hover(Target::Continue);
        assert_eq!(application.focus, Focus::Continue);
        application.apply_action(Action::Down);
        assert_eq!(application.focus, Focus::Session, "passed a one-cell row");
        application.apply_action(Action::Up);
        assert_eq!(application.focus, Focus::Continue);
    }

    /// The bottom row is built once from policy, so a withdrawn action has no
    /// button, no focus cell and no target — not merely a refusal when pressed.
    #[test]
    fn a_withdrawn_power_action_is_absent_rather_than_refused() {
        let all = footer_items(&cedm::config::Config::default());
        assert_eq!(all.len(), 4);
        assert_eq!(all.last(), Some(&FooterItem::DifferentUser));

        let locked = footer_items(&cedm::config::Config {
            power: cedm::config::Power {
                sleep: false,
                restart: false,
                shut_down: false,
            },
            ..cedm::config::Config::default()
        });
        assert_eq!(locked, vec![FooterItem::DifferentUser]);

        let mut application = preview_application(vec![user("Alex")]);
        application.config.power.shut_down = false;
        assert!(!application.target_is_active(Target::Power(cedm::power::Action::ShutDown)));
        assert!(application.target_is_active(Target::Power(cedm::power::Action::Sleep)));
    }

    /// Nothing on the bottom row may act once greetd has been told to start a
    /// session: the handover is already under way and there is no longer an
    /// attempt to abandon.
    #[test]
    fn the_bottom_row_stops_acting_once_the_session_is_starting() {
        let mut application = preview_application(vec![user("Alex")]);
        application.stage = Stage::Departing {
            started: Instant::now(),
            sent: true,
        };
        for action in cedm::power::ALL {
            assert!(!application.target_is_active(Target::Power(action)));
        }
        assert!(!application.target_is_active(Target::DifferentUser));
        assert!(application.focus_grid().is_empty());
    }

    #[test]
    fn stored_session_ids_are_resolved_only_against_discovery() {
        let sessions = [session("gamescope", false), session("lxb", true)];
        assert_eq!(resolve_session_index(&sessions, ["gamescope"]), 0);
        assert_eq!(
            resolve_session_index(&sessions, ["removed", "gamescope"]),
            0
        );
        assert_eq!(resolve_session_index(&sessions, ["not-a-command"]), 1);
        assert_eq!(resolve_session_index(&sessions, []), 1);

        let sessions = [session("gamescope", false), session("plasma", false)];
        assert_eq!(resolve_session_index(&sessions, []), 1);
    }

    #[test]
    fn live_shell_accent_wins_over_stale_broker_cache() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home =
            std::env::temp_dir().join(format!("cedm-live-accent-{}-{unique}", std::process::id()));
        let settings = cedm::accent::settings_path(&home);
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(
            &settings,
            "accent = \"Blue\"\ntheme-wallpaper = \"Simple\"\n",
        )
        .unwrap();

        let user = User {
            avatar: None,
            name: "alex".into(),
            display_name: "Alex".into(),
            uid: 1000,
            home: home.clone(),
            shell: PathBuf::from("/bin/sh"),
        };
        let state = State {
            last_user: None,
            accents: [("alex".to_string(), "Red".to_string())]
                .into_iter()
                .collect(),
            // A broker from before the Theme setting was split: one map, saying
            // one thing about the whole shell.
            themes: [("alex".to_string(), "Default".to_string())]
                .into_iter()
                .collect(),
            icon_themes: Default::default(),
        };
        assert_eq!(user_accent(&state, &user), "Blue");
        assert_eq!(
            user_theme(&state, &user),
            Materials {
                // Named in the account's own file, which is the freshest of the
                // three places and outranks the broker.
                wallpaper: "Simple".to_string(),
                // Not named there at all, so it falls through to the broker's one
                // map — which is what a broker that has only ever known one
                // material says about both halves.
                icons: "Default".to_string(),
            }
        );

        std::fs::remove_file(settings).unwrap();
        std::fs::remove_dir(home.join(".config/lxb")).unwrap();
        std::fs::remove_dir(home.join(".config")).unwrap();
        std::fs::remove_dir(home).unwrap();
    }

    #[test]
    fn terminal_auth_events_cannot_reopen_or_skip_departure() {
        let prompt = AuthEvent::Prompt {
            attempt: 4,
            message: "Password".into(),
            secret: true,
        };
        let failed = AuthEvent::Failed {
            attempt: 4,
            failure: Failure::Service,
            message: "late failure".into(),
        };
        let started = AuthEvent::Started { attempt: 4 };
        let not_sent = Stage::Departing {
            started: Instant::now(),
            sent: false,
        };
        let sent = Stage::Departing {
            started: Instant::now(),
            sent: true,
        };
        assert!(!auth_event_allowed(&not_sent, &prompt));
        assert!(!auth_event_allowed(&not_sent, &failed));
        assert!(!auth_event_allowed(&not_sent, &started));
        assert!(auth_event_allowed(&sent, &failed));
        assert!(auth_event_allowed(&sent, &started));
    }

    #[test]
    fn a_fast_login_waits_for_both_departure_and_accent_settlement() {
        assert!(!ready_to_start_session(Duration::from_millis(299), true));
        assert!(!ready_to_start_session(Duration::from_millis(300), false));
        assert!(ready_to_start_session(Duration::from_millis(300), true));
    }

    #[test]
    fn carousel_retargets_through_the_shortest_wrapped_distance() {
        assert_eq!(carousel_delta(1, 0, 5), 1);
        assert_eq!(carousel_delta(4, 0, 5), -1);
        assert_eq!(carousel_delta(0, 4, 5), 1);
        assert_eq!(carousel_delta(0, 0, 1), 0);
    }

    #[test]
    fn shift_tab_reverses_each_screens_normal_focus_order() {
        assert_eq!(tab_action(&Stage::Choose, false), Action::Down);
        assert_eq!(tab_action(&Stage::Choose, true), Action::Up);
        let username = Stage::Username { error: None };
        assert_eq!(tab_action(&username, false), Action::Right);
        assert_eq!(tab_action(&username, true), Action::Left);
        assert_eq!(
            tab_action(&Stage::Error("Denied".into()), true),
            Action::Left
        );
    }

    /// A prompt raises the board by itself only where there is nothing else to
    /// type on. On a desk the field is simply focused and the user types.
    ///
    /// The button is not part of that bargain. Someone who has just pressed the
    /// button asking for a keyboard is not to be told they already have one —
    /// the detection is a heuristic about hardware and the press is a statement
    /// about what the user wants, and where they disagree the user is right.
    #[test]
    fn the_board_comes_up_by_itself_only_where_there_is_nothing_to_type_on() {
        let mut console = preview_application(Vec::new());
        console.begin_login();
        assert!(matches!(console.stage, Stage::Username { error: None }));
        assert_eq!(console.focus, Focus::Keyboard);
        assert!(console.keyboard_opened.is_some());

        let mut desk = desk_application(Vec::new());
        desk.begin_login();
        assert!(matches!(desk.stage, Stage::Username { error: None }));
        assert_eq!(desk.focus, Focus::Prompt, "the field, and nothing over it");
        assert!(desk.keyboard_opened.is_none());

        // Every automatic route agrees, including the one a pointer takes.
        desk.click(Target::Prompt);
        assert!(desk.keyboard_opened.is_none());

        // And the button overrules all of it.
        desk.click(Target::ShowKeyboard);
        assert!(desk.keyboard_opened.is_some());
        assert_eq!(desk.focus, Focus::Keyboard);

        // As does a key having been pressed, which is not the heuristic being
        // asked again but the thing it was guessing at, arriving.
        let mut typed = preview_application(Vec::new());
        typed.type_into_login("a");
        assert!(
            typed.keyboard_opened.is_none(),
            "a board over a field already being typed into"
        );
        assert_eq!(typed.focus, Focus::Prompt);
    }

    /// Start finishes the typing: Enter, and the board away with it.
    ///
    /// Two keys of the board at opposite ends of it, in the one press a
    /// console has already taught for exactly that. What it must not be is a
    /// second `A`, which is what it was: over a board it would have pressed
    /// whichever letter the cursor was standing on and left the keyboard up.
    #[test]
    fn start_sends_the_answer_and_takes_the_board_with_it() {
        let mut console = preview_application(Vec::new());
        console.begin_login();
        assert!(console.keyboard_opened.is_some());
        let keys = console.sounds.spent().key;

        // A letter, walked to and pressed the ordinary way.
        console.apply_action(Action::Right);
        console.apply_action(Action::Accept);
        assert_eq!(&*console.input, "s");

        // And then Start, from wherever the cursor happens to be standing.
        console.apply_action(Action::Submit);
        assert_eq!(
            console.sounds.spent().key,
            keys + 2,
            "Start is a key of the board like any other"
        );
        assert!(
            console.input.is_empty(),
            "Enter did not send what had been typed"
        );
        assert!(
            console.keyboard_closing.is_some(),
            "the board did not go with the answer"
        );
    }

    /// And everywhere there is no board, it is Accept — so that it is one
    /// button somebody can press without first working out where they are.
    #[test]
    fn start_is_accept_wherever_there_is_no_board() {
        let mut console = preview_application(vec![user("Alex")]);
        assert!(matches!(console.stage, Stage::Choose));
        console.focus = Focus::Continue;
        console.apply_action(Action::Submit);
        assert!(
            matches!(
                console.stage,
                Stage::Username { .. } | Stage::Authenticating { .. }
            ),
            "Start did not sign in with the profile that was selected"
        );

        // Including over the session menu, which owns every button while it is
        // up: Start takes the row it is standing on, as Accept does.
        let mut menu = preview_application(vec![user("Alex")]);
        menu.sessions = vec![session("lxb", true), session("plasma", false)];
        menu.user_sessions = vec![0];
        menu.open_session_menu();
        menu.apply_action(Action::Down);
        menu.apply_action(Action::Submit);
        assert!(!menu.menu_live(), "the menu did not answer Start");
        assert_eq!(menu.selected_session, 1);
    }

    /// A password is what a key press on the profile screen means. Nothing
    /// else on that screen takes typing, and the alternative — dropping the
    /// characters until "Sign in" is pressed — is a screen that silently eats
    /// the first few letters of every password typed at it.
    #[test]
    fn typing_on_the_profile_screen_opens_the_conversation_and_is_kept() {
        let mut console = preview_application(vec![user("Alex")]);
        assert!(matches!(console.stage, Stage::Choose));

        console.type_into_login("h");
        assert!(
            matches!(console.stage, Stage::Authenticating { .. }),
            "the key did not open the conversation"
        );
        console.type_into_login("unter2");
        assert_eq!(&*console.input, "hunter2");
        assert_eq!(console.focus, Focus::Prompt);

        // A refusal is the same screen for this purpose: what follows it is
        // another attempt, and typing is how one starts.
        console.transition_to(Stage::Error("That password was not accepted.".into()));
        console.type_into_login("s");
        assert!(matches!(console.stage, Stage::Authenticating { .. }));
        assert_eq!(&*console.input, "s");

        // The session menu owns every key while it is up, as it owns every
        // direction.
        let mut menu = preview_application(vec![user("Alex")]);
        menu.open_session_menu();
        menu.type_into_login("h");
        assert!(matches!(menu.stage, Stage::Choose));
    }

    /// Typed before greetd has asked for anything, and answered into the
    /// prompt when it does — but only into that same attempt's prompt.
    #[test]
    fn typing_ahead_of_the_prompt_is_held_for_the_attempt_it_was_typed_into() {
        let mut console = preview_application(vec![user("Alex")]);
        console.attempt = 3;
        console.stage = Stage::Busy("Starting authentication…".into());

        console.type_into_login("hunter2");
        assert!(console.input.is_empty(), "there is nowhere to put it yet");
        assert_eq!(
            console.take_typed_ahead().map(|held| held.to_string()),
            Some("hunter2".to_string())
        );

        console.stage = Stage::Busy("Starting authentication…".into());
        console.type_into_login("hunter2");
        console.attempt = 4;
        assert!(
            console.take_typed_ahead().is_none(),
            "an answer meant for one conversation was offered to another"
        );
    }

    /// PAM's own sentence for a wrong password is `authentication error:
    /// AUTH_ERR`, which reads as a broken greeter rather than a typo.
    #[test]
    fn a_refused_password_is_reported_as_one_and_not_as_pam_prose() {
        let pam = "authentication error: AUTH_ERR".to_string();
        assert_eq!(
            refusal(Failure::Rejected, pam.clone(), false),
            "Incorrect password."
        );
        // The typed route cannot say which half was wrong without answering
        // "does this account exist" for whoever asked.
        assert_eq!(
            refusal(Failure::Rejected, pam, true),
            "Incorrect account name or password."
        );
        // The machine's own failures keep the machine's own words.
        assert_eq!(
            refusal(
                Failure::Service,
                "GREETD_SOCK is not set".to_string(),
                false
            ),
            "GREETD_SOCK is not set"
        );
        assert!(!refusal(Failure::Service, "   ".to_string(), false)
            .trim()
            .is_empty());
    }

    #[test]
    fn empty_user_discovery_enters_a_bounded_directory_login_flow() {
        let mut application = preview_application(Vec::new());
        assert!(application.other_account_selected());
        assert_eq!(application.profile_count(), 1);

        application.begin_login();
        assert!(matches!(application.stage, Stage::Username { error: None }));
        assert_eq!(application.focus, Focus::Keyboard);

        for character in "two people".chars() {
            application.push_input(character);
        }
        application.submit();
        assert!(matches!(
            application.stage,
            Stage::Username { error: Some(_) }
        ));

        application.input.zeroize();
        for character in "person@example.test".chars() {
            application.push_input(character);
        }
        application.submit();
        assert!(matches!(application.stage, Stage::Authenticating { .. }));
        assert!(application.input.is_empty());
        assert!(matches!(
            application
                .stage_transition
                .as_ref()
                .map(|transition| &transition.previous),
            Some(VisualStage::Username { .. })
        ));

        application.cancel();
        assert!(matches!(application.stage, Stage::Choose));
        assert!(application.input.is_empty());
    }

    #[test]
    fn one_account_is_one_page_and_the_carousel_does_not_move() {
        let mut application = preview_application(vec![user("Alex")]);
        assert_eq!(application.profile_count(), 1);
        assert!(!application.other_account_selected());
        // The route for a non-enumerated account is on the bottom row, not at
        // the end of the ring, so paging a machine with one account is paging
        // through one account.
        for action in [Action::Right, Action::Right, Action::Left] {
            application.apply_action(action);
            assert!(!application.other_account_selected());
            assert_eq!(application.selected_user, 0);
        }
    }

    /// It is still a selection, just not one the carousel pages onto — and
    /// paging away from it comes back into the ring at the end the movement
    /// came from rather than one page inside it.
    #[test]
    fn the_bottom_rows_route_is_reachable_and_pages_back_out_cleanly() {
        let mut application = preview_application(vec![user("Alex"), user("Sam")]);
        application.click(Target::DifferentUser);
        assert!(application.other_account_selected());
        assert_eq!(application.focus, Focus::Users);

        application.apply_action(Action::Right);
        assert_eq!(application.selected_user, 0);

        application.click(Target::DifferentUser);
        application.apply_action(Action::Left);
        assert_eq!(application.selected_user, 1);
    }

    /// A machine with nothing to enumerate still has something selected, and
    /// that something is the only way in it has.
    #[test]
    fn no_enumerated_accounts_leaves_the_directory_route_selected() {
        let mut application = preview_application(Vec::new());
        assert_eq!(application.profile_count(), 1);
        assert!(application.other_account_selected());
        application.apply_action(Action::Right);
        assert!(application.other_account_selected());
    }

    /// The session badge opens a menu rather than stepping one session along:
    /// a machine with four desktops on it should not have to be pressed three
    /// times to be told what the fourth one is.
    #[test]
    fn the_session_badge_opens_a_menu_that_answers_and_dismisses() {
        let mut application = preview_application(vec![user("Alex")]);
        application.sessions = vec![session("lxb", true), session("plasma", false)];
        application.user_sessions = vec![0];
        application.selected_session = 0;

        application.click(Target::Session);
        assert!(application.menu_live());
        assert_eq!(application.menu_row, 0);

        // The menu owns every direction while it is up.
        application.apply_action(Action::Down);
        assert_eq!(application.menu_row, 1);
        assert_eq!(application.selected_session, 0, "aiming is not choosing");

        application.apply_action(Action::Accept);
        assert_eq!(application.selected_session, 1);
        assert!(!application.menu_live(), "answering closes it");
        // Remembered against the profile it was chosen for.
        assert_eq!(application.user_sessions[0], 1);

        application.click(Target::Session);
        application.apply_action(Action::Back);
        assert!(!application.menu_live());
        assert_eq!(
            application.selected_session, 1,
            "dismissing changes nothing"
        );
    }

    /// Every direction belongs to the menu while it is up, exactly as it does
    /// to the on-screen keyboard: a panel standing over the column is what the
    /// next press is about, and the focus underneath must not move behind it.
    #[test]
    fn an_open_menu_takes_every_direction_until_it_is_answered() {
        let mut application = preview_application(vec![user("Alex")]);
        application.sessions = vec![session("lxb", true), session("plasma", false)];
        application.user_sessions = vec![0];
        application.click(Target::Session);
        let focus = application.focus;

        for action in [Action::Up, Action::Down, Action::Left, Action::Right] {
            application.apply_action(action);
            assert!(application.menu_live(), "{action:?} closed the menu");
            assert_eq!(application.focus, focus, "{action:?} moved the column");
        }
        assert!(matches!(application.stage, Stage::Choose));
    }

    /// A direction clicks when it moved the highlight, and stays quiet when it
    /// moved nothing.
    ///
    /// Both halves are the point. A click is the half of the acknowledgement
    /// that reaches somebody who is looking at the pad in their hands rather
    /// than at the screen, so a direction that walked somewhere has to make
    /// one — and a direction that hit the end of a row that will not wrap has
    /// to not, or the noise stops meaning "you moved" and starts meaning "the
    /// button is not broken".
    #[test]
    fn a_direction_sounds_where_it_moved_something_and_nowhere_else() {
        let mut application = preview_application(vec![user("Alex"), user("Blair")]);

        // Down the column, and across the carousel: two different things a
        // direction means, one sound.
        application.apply_action(Action::Down);
        assert_eq!(application.sounds.spent().moved, 1);
        application.apply_action(Action::Up);
        assert_eq!(application.sounds.spent().moved, 2);
        assert_eq!(application.focus, Focus::Users);
        application.apply_action(Action::Right);
        assert_eq!(application.selected_user, 1, "the carousel did not turn");
        assert_eq!(application.sounds.spent().moved, 3);

        // Nothing else was spent on any of that.
        assert_eq!(
            application.sounds.spent(),
            cedm::sound::Spent {
                moved: 3,
                ..Default::default()
            }
        );

        // A carousel with one profile on it is the edge that does not wrap, and
        // it is silent however many times it is asked.
        let mut alone = preview_application(vec![user("Alex")]);
        for _ in 0..4 {
            alone.apply_action(Action::Right);
            alone.apply_action(Action::Left);
        }
        assert_eq!(alone.selected_user, 0);
        assert_eq!(
            alone.sounds.spent(),
            cedm::sound::Spent::default(),
            "a carousel that cannot turn answered as though it had"
        );
    }

    /// A press sounds when the screen acted on it, including the presses that
    /// go backwards, and not when the control did nothing.
    #[test]
    fn a_press_sounds_when_it_did_something() {
        let mut application = preview_application(vec![user("Alex")]);
        application.focus = Focus::Continue;

        // Opening the conversation.
        application.apply_action(Action::Accept);
        assert!(matches!(application.stage, Stage::Authenticating { .. }));
        assert_eq!(application.sounds.spent().selected, 1);

        // And leaving it again, which is a press somebody made and got.
        application.apply_action(Action::Back);
        assert!(matches!(application.stage, Stage::Choose));
        assert_eq!(application.sounds.spent().selected, 2);

        // Back on the first screen, with the highlight already where cancelling
        // puts it, is a control that does nothing.
        let before = application.sounds.spent();
        application.apply_action(Action::Back);
        assert_eq!(
            application.sounds.spent(),
            before,
            "a press that changed nothing was answered anyway"
        );

        // As are the shoulders on a machine with one session on it.
        application.apply_action(Action::Previous);
        application.apply_action(Action::Next);
        assert_eq!(application.sounds.spent(), before);
    }

    /// The on-screen keyboard answers in its own click — every key of it,
    /// including the ones that type nothing.
    ///
    /// Walking across the board is still walking, and takes the move sound like
    /// any other direction. What is the board's own is a key going *down*,
    /// which is the thing that only happens there.
    #[test]
    fn the_on_screen_keyboard_answers_in_its_own_click() {
        let mut application = preview_application(Vec::new());
        application.begin_login();
        assert!(application.keyboard_opened.is_some());
        let moved = application.sounds.spent().moved;

        // Across the keys: a move, not a key.
        application.apply_action(Action::Right);
        assert_eq!(application.sounds.spent().moved, moved + 1);
        assert_eq!(application.sounds.spent().key, 0);

        // A key going down.
        application.apply_action(Action::Accept);
        assert_eq!(application.sounds.spent().key, 1);
        assert_eq!(&*application.input, "s", "the fixture pressed no letter");

        // Including one that types nothing: Shift is a key the user pressed.
        application.board.select(4, 0);
        application.apply_action(Action::Accept);
        assert_eq!(application.sounds.spent().key, 2);

        // And the presses that are not keys keep the press sound: putting the
        // board away is something asked for and got.
        let selected = application.sounds.spent().selected;
        application.apply_action(Action::ToggleKeyboard);
        assert!(application.keyboard_closing.is_some());
        assert_eq!(application.sounds.spent().selected, selected + 1);
        assert_eq!(application.sounds.spent().key, 2);
    }

    /// The pointer is silent, and the pad doing the identical thing is not.
    ///
    /// A click answers a control somebody cannot see themselves operating. A
    /// mouse is the opposite: the highlight is following the pointer under
    /// their hand and they are watching it do so, and a sweep down the column
    /// would be a stream of clicks answering a question nobody asked.
    #[test]
    fn the_pointer_is_silent_where_the_pad_is_not() {
        let mut pointer = preview_application(vec![user("Alex"), user("Blair")]);
        pointer.sessions = vec![session("lxb", true), session("plasma", false)];
        pointer.user_sessions = vec![0, 0];

        // Every shape of pointer input there is: aiming at a control, taking
        // one, turning the carousel, opening a panel and answering it.
        pointer.hover(Target::Continue);
        pointer.hover(Target::Session);
        pointer.click(Target::NextUser);
        pointer.click(Target::Session);
        pointer.hover(Target::SessionOption(1));
        pointer.click(Target::SessionOption(1));
        // Which opens the conversation, and with it the board — this fixture is
        // a console, so there is nothing else to type on.
        pointer.click(Target::Continue);
        pointer.hover(Target::Key(3, 2));
        pointer.click(Target::Key(3, 2));
        assert_eq!(
            pointer.sounds.spent(),
            cedm::sound::Spent::default(),
            "the pointer made a noise"
        );
        // It did all of that, which is what makes the silence a rule about the
        // route rather than about a screen where nothing happened.
        assert_eq!(pointer.selected_user, 1);
        assert_eq!(pointer.selected_session, 1);
        assert!(pointer.keyboard_opened.is_some());
        assert!(!pointer.input.is_empty());

        // The pad reaching the same controls is answered throughout.
        let mut pad = preview_application(vec![user("Alex"), user("Blair")]);
        pad.apply_action(Action::Right);
        pad.apply_action(Action::Down);
        pad.apply_action(Action::Accept);
        let spent = pad.sounds.spent();
        assert!(spent.moved > 0 && spent.selected > 0, "{spent:?}");
    }

    /// The rise into view is smooth at both ends and is measured from the first
    /// frame.
    ///
    /// Symmetrical because it is a smoothstep: it leaves nothing and settles
    /// into place at the same rate, and the halfway point of the time is the
    /// halfway point of the picture. A linear fade arrives by *stopping*, which
    /// reads as a cut however long it is given, and an ease-out alone would
    /// lurch away from black.
    #[test]
    fn the_login_screen_rises_smoothly_and_from_its_first_frame() {
        let mut application = preview_application(vec![user("Alex")]);
        let began = Instant::now();

        // Nothing until there is a frame to be seen, and the clock starts at
        // that frame rather than when the process did: everything before it is
        // a GPU being opened and pictures being decoded, and a rise measured
        // from there would be over before anybody could see it.
        assert_eq!(application.first_frame, None);
        assert_eq!(application.arrival(began), 0.0);
        assert_eq!(application.first_frame, Some(began));

        let at = |application: &mut Application, seconds: f32| {
            application.arrival(began + Duration::from_secs_f32(seconds))
        };
        let half = at(&mut application, cedm::ui::ARRIVAL / 2.0);
        assert!((half - 0.5).abs() < 0.001, "{half}");
        assert_eq!(at(&mut application, cedm::ui::ARRIVAL), 1.0);
        assert_eq!(at(&mut application, cedm::ui::ARRIVAL * 4.0), 1.0);

        // Symmetrical: as much of the picture arrives in the quarter before the
        // middle as in the quarter after it.
        let quarter = at(&mut application, cedm::ui::ARRIVAL * 0.25);
        let three = at(&mut application, cedm::ui::ARRIVAL * 0.75);
        assert!(
            ((half - quarter) - (three - half)).abs() < 0.001,
            "{quarter} {half} {three}"
        );
        // And it eases at both ends rather than running at one rate.
        assert!(quarter < 0.25 && three > 0.75, "{quarter} {three}");

        // A single captured frame is arrived by definition: `--shot` draws one
        // frame and exits, and a picture of a login screen a fraction into its
        // own entrance is a picture of nothing much.
        let mut shot = preview_application(vec![user("Alex")]);
        shot.args.shot = Some(PathBuf::from("/dev/null"));
        assert_eq!(shot.arrival(Instant::now()), 1.0);
    }

    /// A refused password is the one thing on this screen somebody must not
    /// miss, and the one sound here that answers no press at all.
    #[test]
    fn only_pams_refusal_makes_the_error_noise() {
        assert!(refusal_sounds(Failure::Rejected));
        assert!(
            !refusal_sounds(Failure::Service),
            "a broken login service told the user to type it again"
        );
    }
}
