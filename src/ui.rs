//! Login screen composition in LineXinBar's visual language.
//!
//! One glass column standing on the left of the wallpaper, and the time on the
//! right of it. The column is the whole of the interface: who is signing in,
//! what they are signing in to, the answer PAM is waiting for, and what the
//! machine can be asked to do instead of signing in at all.
//!
//! The wallpaper behind it is deliberately untouched and full-bleed. It is the
//! one thing that survives into the session — see [`crate::handoff`] — so the
//! column and the clock are the only things that fade out at the end of a
//! login, leaving a wallpaper frame that the desktop's compositor then carries
//! on drawing at the same phase. Anything painted edge to edge here, or any
//! backdrop that were not the analytic wallpaper, would put a seam back into
//! the handover.

use crate::displays::Display;
use crate::i18n;
use crate::keyboard::{self, Arrow, Board, Key, Latch};
use crate::power;
use crate::sessions::Session;
use crate::users::User;
use crate::visual::theme;
use crate::visual::{self, Quad, Scene, Text, TextAlign};

const REFERENCE_WIDTH: f32 = 1280.0;
const REFERENCE_HEIGHT: f32 = 720.0;
const OSK_REFERENCE_HEIGHT: f32 = 1080.0;
const PANEL_RADIUS: f32 = 30.0;
const CONTROL_RADIUS: f32 = 16.0;
const DEPTH_PANEL: f32 = 22.0;
const DEPTH_CONTROL: f32 = 9.0;
const FROST_PANEL: f32 = 0.95;
const FROST_CONTROL: f32 = 0.20;
const GLOSS_FULL: f32 = 1.0;
const GLOSS_QUIET: f32 = 0.45;
const PULSE_PERIOD: f32 = 1.8;

/// The sign-in column is cut from LineXinBar's guide sidebar, not from its
/// modal-panel recipe: a shallower, clearer slab that the wallpaper's current
/// stays visible through, with its own light under it. Vendored from
/// `lxb-desktop`'s `ui.rs` alongside the wallpaper and the palette.
const DEPTH_SIDEBAR: f32 = 15.0;
const FROST_SIDEBAR: f32 = 0.46;
const GLOSS_SIDEBAR: f32 = 0.66;
const CURVE_SIDEBAR: f32 = 1.0;
const SIDEBAR_STAIN: f32 = 0.38;
const SIDEBAR_HEADER_LIGHT: f32 = 0.075;
const SIDEBAR_FOOT_LIGHT: f32 = 0.04;
const SIDEBAR_RIM: f32 = 0.10;
/// And the session menu's, which is the same glass cut deeper.
///
/// The shell's reason for keeping the sidebar clear is a reason about *size*:
/// a surface two thirds of the display tall, frosted hard, turns almost all of
/// its face into one flat field, and the point of the column is that the
/// wallpaper's current still runs through it. A pane of three rows has no face
/// to lose. What it has instead is a job the column does not have — to be read
/// against whatever it happens to land in front of, which here is the column
/// itself, at a few pixels' remove and in the same violet.
///
/// Short of the modal cut, which is a surface that has taken the screen over
/// rather than one standing on it.
const FROST_MENU: f32 = 0.80;
/// How far the sidebar floats clear of the display's edges. Glass reads as a
/// slab laid *over* the wallpaper, which needs the wallpaper to run past it;
/// pinned to the edge it is only a differently coloured region of the screen.
const PANEL_INSET: f32 = 14.0;
const MIN_ACTION: f32 = 44.0;

/// How much of the display the sign-in column takes when there is room for the
/// clock beside it.
const COLUMN_SHARE: f32 = 0.40;
/// Below this the clock is dropped and the column becomes the whole screen: a
/// split layout on a narrow display gives both halves too little to be worth
/// having, and the column is the half that does the work.
const SPLIT_WIDTH: f32 = 900.0;

/// How far one step of the profile carousel travels inside the column.
///
/// A fraction of the column rather than a fixed distance, and small: the
/// identity block is nearly as wide as the column, and the panel cannot clip,
/// so a profile has to have faded out entirely before it would reach the
/// glass's edge. See [`build_identity`].
const CAROUSEL_TRAVEL: f32 = 0.34;

/// The session menu, in LineXinBar's context-menu language: a panel that opens
/// *beside* what it is about rather than over it, grows out of it, and dims
/// what it stands in front of.
///
/// Beside, because the anchor is the whole reason the panel is where it is — it
/// says which control these choices belong to — so covering it would throw away
/// the only context the panel has.
const MENU_WIDTH: f32 = 340.0;
const MENU_ROW: f32 = 48.0;
const MENU_MARGIN: f32 = 16.0;
const MENU_ROW_PADDING: f32 = 4.0;
/// The air between the anchor and the panel that hangs off it.
const MENU_GAP: f32 = 12.0;
/// How far the rest of the screen is dimmed behind it. Light: this is a note
/// pinned to something the user can still see, not a question about the machine.
const MENU_DIM: f32 = 0.42;
/// And how far back it steps while the menu is up. The two together are the
/// difference between a screen that has been turned down and one that has been
/// stepped back from.
///
/// Enough to be seen sitting there. This is a distance the screen *holds* for
/// as long as the user takes to read a list, not a flourish under something
/// about to cover the display: a push that only reads while it is moving has
/// stopped saying anything by the time it matters.
///
/// And no further. The menu is *about* the badge it came out of, and the column
/// still has to be read past the panel while it is up; far enough back and the
/// step stops being the screen receding and starts being the screen shrinking,
/// which is a thing happening to the subject rather than to the ground behind
/// it. `lxb-desktop`'s `CONTEXT_DEPTH`.
const MENU_DEPTH: f32 = 0.1;
/// How long it takes to unfold out of its anchor, in seconds.
pub const MENU_UNFOLD: f32 = 0.32;

/// How long the login screen takes to rise into view, in seconds.
///
/// Longer than every other movement here — the menu unfolding, a screen
/// changing, the board coming up — because it is the only one that is not an
/// answer to something the user did. Those are as quick as they can be without
/// snapping, because somebody is waiting on them. This one happens while
/// nobody is waiting on anything: the machine has just come up, and the point
/// of it is that the screen was *arriving* rather than that it is now here.
///
/// Still well under a second. A login screen that made somebody wait to type
/// would have got this wrong in the other direction, which is why the arrival
/// dims the picture and never the keyboard: see [`View::arrival`].
pub const ARRIVAL: f32 = 0.6;

const KEY_UNIT: f32 = 62.0;
const KEY_HEIGHT: f32 = 56.0;
const KEY_GAP: f32 = 8.0;
const KEY_RADIUS: f32 = 13.0;
const KEY_CAP_FUNCTION: f32 = 14.0;
const BOARD_PADDING: f32 = 20.0;
const BOARD_MARGIN: f32 = 34.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Users,
    Session,
    Continue,
    Prompt,
    Back,
    KeyboardToggle,
    /// One of the actions along the bottom of the column, by position in
    /// [`View::footer`]. An index rather than a named action because which
    /// actions exist is the administrator's decision, and a focus that named
    /// one the machine does not offer would be a focus on nothing.
    Footer(usize),
    Keyboard,
}

/// An action on the bottom row of the column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FooterItem {
    Power(power::Action),
    /// The route for an account the greeter cannot enumerate. Last, because it
    /// is the only one of the four that continues the login rather than
    /// abandoning it.
    DifferentUser,
}

impl FooterItem {
    pub fn label(self) -> &'static str {
        match self {
            Self::Power(action) => action.label(),
            Self::DifferentUser => i18n::text().different_user_action,
        }
    }

    fn slot(self) -> u32 {
        match self {
            Self::Power(power::Action::Sleep) => visual::SLEEP_SLOT,
            Self::Power(power::Action::Restart) => visual::RESTART_SLOT,
            Self::Power(power::Action::ShutDown) => visual::POWER_SLOT,
            Self::DifferentUser => visual::USER_SWITCH_SLOT,
        }
    }

    /// Its own target rather than the carousel's `OtherAccount`.
    ///
    /// They do the same thing — select the profile for an account the greeter
    /// cannot enumerate — but they are in different places, and a pointer
    /// resting on this button must not light up the avatar at the top of the
    /// column instead of the button under the pointer.
    fn target(self) -> Target {
        match self {
            Self::Power(action) => Target::Power(action),
            Self::DifferentUser => Target::DifferentUser,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    User(usize),
    OtherAccount,
    PreviousUser,
    NextUser,
    PreviousSession,
    NextSession,
    Session,
    Continue,
    Retry,
    Back,
    Prompt,
    ShowKeyboard,
    Power(power::Action),
    DifferentUser,
    /// A row of the open session menu.
    SessionOption(usize),
    /// Anywhere outside the open session menu: the way to dismiss it without
    /// choosing, which every menu on every desktop offers.
    DismissMenu,
    Key(usize, usize),
}

#[derive(Debug, Clone, Copy)]
pub struct Hit {
    pub rect: [f32; 4],
    pub target: Target,
}

#[derive(Debug, Default)]
pub struct Output {
    pub scene: Scene,
    pub hits: Vec<Hit>,
}

#[derive(Debug, Clone, Copy)]
pub enum Phase<'a> {
    Choose,
    Username {
        input: &'a str,
        error: Option<&'a str>,
    },
    Authenticating {
        prompt: &'a str,
        secret: bool,
        input: &'a str,
    },
    Busy(&'a str),
    Departing(&'a str),
    Error(&'a str),
}

impl Phase<'_> {
    /// Whether this screen has something to go back out of.
    fn cancellable(&self) -> bool {
        matches!(
            self,
            Self::Username { .. } | Self::Authenticating { .. } | Self::Busy(_) | Self::Error(_)
        )
    }

    /// Whether the session may still be changed. Once an attempt has been
    /// opened the session has already been named to greetd, so the selector
    /// stays on screen — the user needs to see what they are signing in to —
    /// but stops being something that can be moved.
    fn choosing(&self) -> bool {
        matches!(self, Self::Choose)
    }
}

pub struct View<'a> {
    pub users: &'a [User],
    pub selected_user: usize,
    pub sessions: &'a [Session],
    pub selected_session: usize,
    pub focus: Focus,
    pub phase: Phase<'a>,
    pub previous_phase: Option<Phase<'a>>,
    pub transition_progress: f32,
    pub keyboard: Option<&'a Board>,
    /// Whether the visible board may currently receive pointer presses.
    ///
    /// The board remains in the scene while it travels off-screen. Keeping
    /// this separate from `keyboard` prevents its old key rectangles from
    /// accepting clicks during that departure (and during the login handoff).
    pub keyboard_interactive: bool,
    pub keyboard_arrival: f32,
    pub time: f32,
    /// How far this login screen has risen into view: 0 the instant it has a
    /// surface to draw on, 1 once it is fully here. See [`ARRIVAL`].
    ///
    /// It dims everything the greeter itself draws and **nothing** of the
    /// wallpaper, which is the only shape this can take. The wallpaper is
    /// already on the screen before this program has a window — the compositor
    /// draws it, at the phase a scene clock kept across the hand-over — so a
    /// rise that started from black would have to put black over a picture that
    /// is already there, and the login screen would drop to black and come back
    /// rather than arrive. What was missing was never the picture; it was
    /// everything in front of it, and that is what rises.
    ///
    /// The mirror of [`View::departure`], and the two multiply: a screen that
    /// somehow left while it was still arriving fades from where it had got to
    /// rather than jumping to full first.
    pub arrival: f32,
    pub departure: f32,
    /// Continuous carousel offset in item spacings. It eases to zero while
    /// the fixed selection light stays centred.
    pub carousel_shift: f32,
    /// The bottom row, in order. Empty is a legitimate configuration.
    pub footer: &'a [FooterItem],
    /// The wall clock, or `None` where the machine could not be asked what
    /// time it is. The greeting comes from the same reading, so a display with
    /// no clock also greets nobody by the hour.
    pub now: Option<crate::clock::Now>,
    /// The open session menu, if one is open.
    pub session_menu: Option<Menu>,
}

/// The session menu's state, as far as drawing it is concerned.
#[derive(Debug, Clone, Copy)]
pub struct Menu {
    /// Which row the next press will take. Not the selected session: the menu
    /// is a question, and nothing is chosen until it is answered.
    pub selected: usize,
    /// How far out of its anchor it is, 0 to 1. Both directions — a menu on its
    /// way out runs the same number backwards.
    pub progress: f32,
    /// Whether it may still be pressed. A menu that is leaving is still drawn
    /// and must not still be answerable.
    pub interactive: bool,
}

/// Compose the login screen on every display the greeter has been given.
///
/// Each of them is a whole screen and is drawn as one: its own wallpaper, its
/// own column at its own scale, its own clock, its own board. A display is not
/// a region of a larger composition — the greeter is handed one surface across
/// all of them and the temptation is to lay out in it, which is what puts a
/// column on the outer edge of the left-hand panel and the time in the gap
/// between two monitors.
///
/// On every display rather than on one of them, for the reason LineXinBar puts
/// its bar on every output: the machine does not know which screen the person
/// in front of it is looking at, and a console plugged into a television beside
/// a monitor has no primary. It is also what the handover needs — the shell's
/// first frame is drawn on all of them, so the greeter's last one has to be.
///
/// The composition itself knows nothing about any of this. It is built in the
/// display's own pixels, from its own corner, and then moved onto the surface
/// where that display is; a display is where it is, and the interface on it is
/// what it would have been if it were the only one.
pub fn build(view: View<'_>, displays: &[Display]) -> Output {
    let mut output = Output::default();
    for display in displays {
        let mut one = Output::default();
        compose(&mut one, &view, display.width(), display.height());
        move_output(&mut one, display.rect[0], display.rect[1]);
        // Which display each pane is standing on, carried to the shader: glass
        // that finds nothing drawn behind it answers by evaluating the
        // wallpaper, and the wallpaper it has to evaluate is this display's.
        for quad in &mut one.scene.quads {
            quad.display = display.rect;
        }
        append_output(&mut output, one);
    }
    output.scene.displays = displays.iter().map(|display| display.rect).collect();
    output
}

fn compose(output: &mut Output, view: &View<'_>, width: f32, height: f32) {
    let metrics = Metrics::new(width, height);
    let layout = Layout::new(metrics, view);
    let pulse = 0.5 + 0.5 * (view.time * std::f32::consts::TAU / PULSE_PERIOD).sin();
    let fade = (view.arrival * (1.0 - view.departure)).clamp(0.0, 1.0);
    // Deliberately not `fade`. A screen still rising is a screen that works:
    // somebody who starts typing their password into the first half-second of
    // it must have every character, and this program already argues at length
    // that a login screen which eats the beginning of a password is one whose
    // users find out the hard way. Only the departure takes the controls away,
    // because by then the session is already starting.
    let interactive = view.departure <= f32::EPSILON;

    // The clock and the column are the same on every screen, so they are drawn
    // once and are not part of the cross-fade below. A panel that slid out and
    // back in whenever PAM asked another question would be the interface
    // reacting to something the user did not do.
    build_clock(output, view, layout, fade);
    build_column(output, layout, metrics, fade);
    build_identity(output, view, layout, metrics, pulse, fade, interactive);
    build_session(output, view, layout, metrics, pulse, fade, interactive);
    build_footer(output, view, layout, metrics, pulse, fade, interactive);
    build_back(output, view, layout, metrics, pulse, fade, interactive);

    // Only the middle of the column changes between screens, so only the
    // middle of the column travels.
    if let Some(previous) = view
        .previous_phase
        .filter(|_| view.transition_progress < 1.0)
    {
        let progress = view.transition_progress.clamp(0.0, 1.0);
        let travel = 18.0 * metrics.scale.max(0.72);
        let mut outgoing = Output::default();
        build_centre(
            &mut outgoing,
            view,
            layout,
            metrics,
            previous,
            pulse,
            fade * (1.0 - progress),
            false,
        );
        shift_output(&mut outgoing, -travel * progress);
        append_output(output, outgoing);

        let mut incoming = Output::default();
        build_centre(
            &mut incoming,
            view,
            layout,
            metrics,
            view.phase,
            pulse,
            fade * progress,
            interactive && progress >= 0.45,
        );
        shift_output(&mut incoming, travel * (1.0 - progress));
        append_output(output, incoming);
    } else {
        build_centre(
            output,
            view,
            layout,
            metrics,
            view.phase,
            pulse,
            fade,
            interactive,
        );
    }

    if let Some(menu) = view.session_menu {
        let progress = menu.progress.clamp(0.0, 1.0);
        // Back first, so everything below measures the screen where it now is.
        // The panel itself is drawn last and at full size: it is the thing in
        // front, and the badge it grew out of has not moved.
        recede_into_depth(output, layout.session, progress);
        // The scrim below dims the wallpaper and every quad drawn so far, but
        // text is composed in its own pass after all of them and would stay at
        // full strength over the top of it. Dimming it here rather than adding
        // a second text pass keeps one rule — what is behind the menu is
        // dimmed — true of the whole picture.
        dim_text(output, 1.0 - MENU_DIM * progress);
        // And what the panel actually covers is taken away rather than dimmed,
        // for the same reason and more of it: no amount of frost can scatter a
        // label that is printed after the pane is drawn. Left dimmed, the
        // session's own name reads straight through the rows naming sessions,
        // which is the one place in the interface where two runs of the same
        // words at two brightnesses is worse than one.
        cut_text_behind(
            output,
            menu_bounds(
                menu_rect(layout, metrics, view.sessions.len()).0,
                layout.session,
                progress,
            ),
            progress,
        );
        build_menu(output, view, layout, metrics, menu, pulse, fade);
    }

    if let Some(board) = view.keyboard {
        build_keyboard(
            output,
            board,
            width,
            height,
            view.keyboard_arrival,
            view.time,
            view.keyboard_interactive && interactive,
        );
    }
}

/// Step everything drawn so far back from the viewer, around `anchor`.
///
/// It goes back *around the badge the panel came out of*, which is the one
/// thing that does not move. Shrinking towards the middle of the display
/// instead would slide that badge out from under the very panel growing off it,
/// which reads as the column sliding rather than as the column receding — and a
/// menu coming out of nothing.
///
/// Every quad is scaled, glass included: a pane's depth is a length like any
/// other, and a slab left at full thickness on a screen that has moved away
/// would be a bevel that grew as the screen shrank. The hit rectangles travel
/// with it too, so what can be pressed stays what can be seen — moot while the
/// menu takes every press, and one less thing that is only accidentally true.
///
/// `depth` is 0 flat against the glass and 1 fully stepped away; it runs on the
/// panel's own arrival, so the screen goes back over exactly the span the panel
/// comes forward in. The mirror of the lean the shell gives an application
/// opening off the bar: something standing over the screen pushes it back,
/// something coming out of it pulls it forward. `lxb-desktop`'s
/// `recede_into_depth`.
fn recede_into_depth(output: &mut Output, anchor: [f32; 4], depth: f32) {
    let depth = depth.clamp(0.0, 1.0);
    if depth <= 0.0 {
        return;
    }
    let factor = lerp(1.0, 1.0 - MENU_DEPTH, depth);
    let [ax, ay, aw, ah] = anchor;
    // Scaling about a point rather than about the origin: everything keeps its
    // distance from the badge in the same proportion, and the badge itself
    // stays exactly where it was.
    let (cx, cy) = (ax + aw * 0.5, ay + ah * 0.5);
    let moved = |[x, y, w, h]: [f32; 4]| {
        [
            cx + (x - cx) * factor,
            cy + (y - cy) * factor,
            w * factor,
            h * factor,
        ]
    };
    for quad in &mut output.scene.quads {
        quad.rect = moved(quad.rect);
        quad.radius *= factor;
        quad.border *= factor;
        quad.thickness *= factor;
    }
    for text in &mut output.scene.texts {
        text.rect = moved(text.rect);
        text.size *= factor;
        // Whatever is standing over the run travels with it: this is the same
        // screen seen from further off, not a different one.
        text.clip = text.clip.map(moved);
    }
    for hit in &mut output.hits {
        hit.rect = moved(hit.rect);
    }
}

/// Take everything already written down to `share` of its opacity.
fn dim_text(output: &mut Output, share: f32) {
    for text in &mut output.scene.texts {
        text.color[3] *= share;
    }
}

/// Whether a panel at `rect` stands in front of this run's writing.
///
/// Measured against the writing rather than against the box the run was laid
/// out in. A box is what the words are *allowed* to fill, not what they do
/// fill: it is drawn generously and the bottom of it is empty descender space.
/// The em below its top is where the letters are, and a panel grazing the air
/// under them is not standing in front of them — measured by the box, a prompt
/// sitting plainly above the panel loses its words to a three-pixel overlap of
/// nothing.
fn writing_behind(text: &Text, [x, y, w, h]: [f32; 4]) -> bool {
    let [tx, ty, tw, _] = text.rect;
    !(tx >= x + w || tx + tw <= x || ty >= y + h || ty + text.size <= y)
}

/// Put the writing behind `rect`: one run in, up to three out — what is left of
/// it on either side of the panel, and, while the panel is still arriving, what
/// is under it, faded by how far arrived it is.
///
/// `covered` is how much of the panel there is, so at 1 the middle piece is
/// gone. It is not dropped outright at the first frame because the panel
/// travels as one whole rectangle rather than growing into one: a panel a
/// fifth of a second out of the badge already covers, on paper, everything it
/// will ever cover, while being very nearly invisible. Taking the words away
/// then would read as the column losing them rather than as a panel arriving
/// over them.
///
/// Horizontal only. Vertically a run is one line inside its own box, and a
/// panel that overlaps that line at all overlaps the whole of it — a label
/// sliced through the middle by a panel edge would be worse than either answer.
///
/// Cutting rather than laying out again: each piece keeps the run's own box and
/// alignment and differs only in its scissor, so no glyph moves. LineXinBar's
/// `cut_text_behind`, which its context menus need for exactly this reason.
fn cut_text_behind(output: &mut Output, [x, y, w, h]: [f32; 4], covered: f32) {
    let covered = covered.clamp(0.0, 1.0);
    if w <= 0.0 || h <= 0.0 || covered <= 0.0 {
        return;
    }
    let mut kept = Vec::with_capacity(output.scene.texts.len());
    for text in std::mem::take(&mut output.scene.texts) {
        let [tx, ty, tw, th] = text.rect;
        if !writing_behind(&text, [x, y, w, h]) {
            kept.push(text);
            continue;
        }
        // A piece of the run in its own *box* rather than in its ink: where the
        // ink actually fell is only known once the run has been shaped, which
        // happens in the renderer, and a scissor wider than the words costs
        // nothing — there are no glyphs out there to cut.
        //
        // Narrower than one em and there is nothing to keep: a piece that
        // cannot hold a whole character is not a word surviving beside the
        // panel, it is the edge of a letter, and one stray stroke poking out
        // reads as a fault rather than as something standing behind glass.
        let piece = |from: f32, width: f32| {
            let box_of_it = [from, ty, width, th];
            let cut = match text.clip {
                Some(clip) => visual::intersection(clip, box_of_it),
                None => box_of_it,
            };
            (cut[2] >= text.size && cut[3] > 0.0).then_some(cut)
        };
        for clip in [piece(tx, x - tx), piece(x + w, tx + tw - (x + w))]
            .into_iter()
            .flatten()
        {
            kept.push(Text {
                clip: Some(clip),
                ..text.clone()
            });
        }
        if covered < 1.0 {
            if let Some(clip) = piece(x, w) {
                let mut under = text;
                under.color[3] *= 1.0 - covered;
                under.clip = Some(clip);
                kept.push(under);
            }
        }
    }
    output.scene.texts = kept;
}

/// Move a finished composition onto the display it belongs to.
///
/// Everything the display drew, in one place: the panes, the writing, whatever
/// a panel cut that writing to, and what the pointer may press. A hit rectangle
/// that stayed behind would be a control that answers to a press on the display
/// to the left of the one it is drawn on.
fn move_output(output: &mut Output, x: f32, y: f32) {
    if x == 0.0 && y == 0.0 {
        return;
    }
    let moved = |rect: [f32; 4]| [rect[0] + x, rect[1] + y, rect[2], rect[3]];
    for quad in &mut output.scene.quads {
        quad.rect = moved(quad.rect);
    }
    for text in &mut output.scene.texts {
        text.rect = moved(text.rect);
        text.clip = text.clip.map(moved);
    }
    for hit in &mut output.hits {
        hit.rect = moved(hit.rect);
    }
}

fn shift_output(output: &mut Output, y: f32) {
    for quad in &mut output.scene.quads {
        quad.rect[1] += y;
    }
    for text in &mut output.scene.texts {
        text.rect[1] += y;
    }
    for hit in &mut output.hits {
        hit.rect[1] += y;
    }
}

fn append_output(output: &mut Output, mut addition: Output) {
    output.scene.quads.append(&mut addition.scene.quads);
    output.scene.texts.append(&mut addition.scene.texts);
    output.hits.append(&mut addition.hits);
}

#[derive(Debug, Clone, Copy)]
struct Metrics {
    width: f32,
    height: f32,
    scale: f32,
    compact: bool,
    safe: f32,
}

impl Metrics {
    fn new(width: f32, height: f32) -> Self {
        let scale = (width / REFERENCE_WIDTH)
            .min(height / REFERENCE_HEIGHT)
            .clamp(0.35, 2.5);
        let compact = width < 760.0 || height < 520.0 || scale < 0.72;
        Self {
            width,
            height,
            scale,
            compact,
            safe: if compact { 16.0 } else { 40.0 * scale },
        }
    }

    /// A length from the 1280×720 canvas the interface is drawn against, in
    /// the device pixels this frame is actually made of.
    ///
    /// `build` is handed the surface's size, which is physical: on a 4K panel
    /// it is 3840×2160, and every length here has to be multiplied up to suit
    /// it. That makes a bare constant standing beside a scaled one a bug and
    /// not a shortcut — and the bound of a `clamp` is a length like any other.
    /// A ceiling left unmultiplied stops growing at some resolution while
    /// everything measured against it carries on, and because the field's
    /// width is what is *left* of the column after the furniture beside it,
    /// what that costs lands on the field first: a column pinned at 620 while
    /// its own margins, avatar and button grow to two and a half times leaves
    /// the prompt an 80-pixel sliver on the panel it is furthest wrong on. The
    /// developer's preview window is 1280×720, where the scale is exactly 1
    /// and every one of these is a no-op, so none of it shows up there.
    fn px(self, reference: f32) -> f32 {
        reference * self.scale
    }
}

/// Where everything in the column is, worked out once per frame.
///
/// Every screen shares this. The centre band — the field, or the status, or
/// the sign-in button — is one rectangle whatever is standing in it, so the
/// screens cross-fade in place rather than each arranging themselves.
#[derive(Debug, Clone, Copy)]
struct Layout {
    column: [f32; 4],
    content_x: f32,
    content_w: f32,
    avatar: [f32; 4],
    greeting: [f32; 4],
    name: [f32; 4],
    pips: [f32; 4],
    /// The band the phase draws into: a prompt and its button, or a status.
    field: [f32; 4],
    submit: [f32; 4],
    message: [f32; 4],
    session: [f32; 4],
    keyboard_toggle: [f32; 4],
    footer_row: [f32; 4],
    footer_mark: f32,
    back: [f32; 4],
    clock: [f32; 4],
    date: [f32; 4],
    /// Whether there is room beside the column for the clock.
    split: bool,
    /// Whether the on-screen keyboard has taken the bottom of the display.
    /// The footer goes first, because it is the row nearest the board and the
    /// one nothing in an answer depends on.
    footer_visible: bool,
}

impl Layout {
    fn new(metrics: Metrics, view: &View<'_>) -> Self {
        let split = metrics.width >= SPLIT_WIDTH;
        let column_w = if split {
            // Both bounds are canvas lengths: they read as "no narrower than a
            // column that can hold the avatar and its two lines, no wider than
            // a comfortable measure", and neither of those is a count of
            // device pixels. Left raw, the ceiling is the whole bug — it pins
            // the column at 620 physical pixels from 1550 across upwards,
            // which is 16% of a 4K display, while the margins and controls
            // inside it go on scaling.
            (metrics.width * COLUMN_SHARE).clamp(metrics.px(360.0), metrics.px(620.0))
        } else {
            metrics.width
        };
        // Floating clear of the display's edges, as the shell's own sidebar
        // does, because glass only reads as a layer when what it is laid over
        // runs past it — and because a rim hairline on a slab pinned to three
        // edges is a hairline the display has cut off on three sides.
        let float = PANEL_INSET * metrics.scale;
        let column = [
            float,
            float,
            (column_w - float * 2.0).max(200.0),
            (metrics.height - float * 2.0).max(200.0),
        ];

        let inset = if metrics.compact {
            18.0
        } else {
            34.0 * metrics.scale
        };
        let content_x = column[0] + inset;
        let content_w = (column[2] - inset * 2.0).max(120.0);

        let board_top = view
            .keyboard
            .map(|_| keyboard_panel_rect(metrics.width, metrics.height)[1])
            .unwrap_or(metrics.height);
        let footer_h = if metrics.compact {
            64.0
        } else {
            76.0 * metrics.scale
        };
        let footer_y = column[1] + column[3] - metrics.safe - footer_h;
        let footer_visible = !view.footer.is_empty() && board_top > footer_y + footer_h * 0.35;

        // The identity sits above the middle rather than on it: the column is
        // read top to bottom, and what is being answered has to be above the
        // answer.
        let usable_bottom = if footer_visible {
            footer_y
        } else {
            board_top.min(metrics.height)
        };
        // Three bands, stacked and then centred as a whole: who, the answer,
        // and what the answer is for. Laid out with a running cursor rather
        // than from fixed offsets, because every one of these heights depends
        // on the display and two of them overlapping is not a rounding error
        // the user forgives.
        let avatar_size = if metrics.compact {
            72.0
        } else {
            (118.0 * metrics.scale).clamp(metrics.px(72.0), metrics.px(150.0))
        };
        let field_h = if metrics.compact {
            46.0
        } else {
            (52.0 * metrics.scale).max(MIN_ACTION)
        };
        let badge = if metrics.compact {
            40.0
        } else {
            (46.0 * metrics.scale).max(MIN_ACTION * 0.9)
        };
        // Enough under the avatar for the profile dots to stand clear of it.
        let after_avatar = if metrics.compact {
            30.0
        } else {
            38.0 * metrics.scale
        };
        // Room for a line of validation text under the field whether or not
        // there is one to show, so the row below does not move when a name is
        // rejected. The session row follows immediately under that reserved
        // line rather than a further gap down: it answers for the field above
        // it, and a row that belongs to something reads as belonging to it by
        // being nearer to it than to anything else.
        let message_h = (18.0 * metrics.scale).max(16.0);
        let after_field = message_h
            + if metrics.compact {
                4.0
            } else {
                metrics.px(6.0)
            };
        let back_side = MIN_ACTION.max(42.0 * metrics.scale);
        let back = [content_x, column[1] + metrics.safe, back_side, back_side];

        let text_x = content_x
            + avatar_size
            + (if metrics.compact {
                14.0
            } else {
                22.0 * metrics.scale
            });
        let text_w = (content_x + content_w - text_x).max(80.0);
        let greeting_size = if metrics.compact {
            20.0
        } else {
            (26.0 * metrics.scale).clamp(metrics.px(20.0), metrics.px(38.0))
        };
        let lines_h = greeting_size * 2.54;
        // Between the name and the band that answers for it.
        let after_lines = if metrics.compact {
            14.0
        } else {
            metrics.px(20.0)
        };

        // The two lines and the band under them are one stack, and the avatar
        // is set against the middle of the whole of it — not against the two
        // lines alone. Both are the same subject: this account, and the answer
        // it is being asked for. Centred on the greeting by itself the face
        // rides at the group's shoulder and the field hangs off nothing,
        // which is the picture of a field that belongs to whatever is under
        // it rather than to the person named beside it.
        let stack_h = lines_h + after_lines + field_h;
        let head_h = stack_h.max(avatar_size);
        let pip_h = metrics.px(8.0);
        // What each side needs under that head, measured from one top: the
        // dots on the left, the reserved line and the session row on the
        // right. The two do not share an x range, so the block is the taller
        // of them rather than the sum.
        let left_h = (head_h - avatar_size) * 0.5 + avatar_size + after_avatar;
        let right_h = (head_h - stack_h) * 0.5 + stack_h + after_field + badge;
        let block_h = left_h.max(right_h);
        // Centred in what is left of the column, but never above the way out:
        // an open keyboard shortens the column enough that the two would
        // otherwise be laid over each other.
        let block_top = ((usable_bottom - block_h) * 0.5).max(
            back[1]
                + back[3]
                + if metrics.compact {
                    12.0
                } else {
                    18.0 * metrics.scale
                },
        );

        let avatar_y = block_top + (head_h - avatar_size) * 0.5;
        let stack_top = block_top + (head_h - stack_h) * 0.5;
        let avatar = [content_x, avatar_y, avatar_size, avatar_size];
        let greeting = [text_x, stack_top, text_w, greeting_size * 1.3];
        let name = [
            text_x,
            greeting[1] + greeting_size * 1.24,
            text_w,
            greeting_size * 1.3,
        ];
        let pips = [
            content_x,
            avatar_y + avatar_size + (after_avatar - pip_h) * 0.5,
            avatar_size,
            pip_h,
        ];

        let field_y = stack_top + lines_h + after_lines;
        let submit_w = field_h;
        let gap = if metrics.compact {
            8.0
        } else {
            12.0 * metrics.scale
        };
        let field = [
            text_x,
            field_y,
            (content_x + content_w - text_x - submit_w - gap).max(80.0),
            field_h,
        ];
        let submit = [field[0] + field[2] + gap, field_y, submit_w, field_h];

        // On the field's own axis, starting exactly where the field starts.
        // The session is what the answer above it will be spent on, so the row
        // is read as part of that band and has to line up with it: set under
        // the avatar instead it starts on the left-hand column's edge, which
        // puts a control that answers for the field on the same line as the
        // dots that do not.
        let options_y = field_y + field_h + after_field;
        let session = [text_x, options_y, badge, badge];
        // At the far end of the row, under the button it belongs to: the board
        // is opened to answer the field, and the field's own button is there.
        let keyboard_toggle = [
            submit[0] + (submit_w - badge) * 0.5,
            options_y,
            badge,
            badge,
        ];
        let message = [
            text_x,
            field_y + field_h + metrics.px(5.0),
            (content_x + content_w - text_x).max(80.0),
            message_h,
        ];

        let clock_w = (metrics.width - column_w).max(0.0);
        let clock_x = column_w;
        let clock_size = if metrics.compact {
            64.0
        } else {
            (132.0 * metrics.scale).clamp(metrics.px(64.0), metrics.px(200.0))
        };
        let clock_y = metrics.height * 0.40 - clock_size * 0.62;
        let clock = [clock_x, clock_y, clock_w, clock_size * 1.22];
        let date = [
            clock_x,
            clock[1] + clock[3] + metrics.px(4.0),
            clock_w,
            (clock_size * 0.24).max(18.0),
        ];

        Self {
            column,
            content_x,
            content_w,
            avatar,
            greeting,
            name,
            pips,
            field,
            submit,
            message,
            session,
            keyboard_toggle,
            footer_row: [content_x, footer_y, content_w, footer_h],
            footer_mark: if metrics.compact {
                34.0
            } else {
                (38.0 * metrics.scale).clamp(metrics.px(30.0), metrics.px(48.0))
            },
            back,
            clock,
            date,
            split,
            footer_visible,
        }
    }
}

/// A pane of LineXinBar's guide-sidebar glass, at any size — the four quads of
/// `lxb-desktop`'s `sidebar_surface`, in that order and with its numbers.
///
/// Not one uniformly frosted block. The two glows are painted first and wholly
/// inside the panel, so the pane scatters and bends them together with the
/// animated wallpaper behind it: that makes them part of the material rather
/// than colour sprayed on its face. The hairline last restores a precise
/// silhouette without pretending the whole circumference catches one highlight.
///
/// Its cut is shallower and clearer than a modal panel's, for the reason the
/// shell gives: a surface this tall would otherwise turn almost all of its face
/// into one flat field, and the point of it is that the wallpaper's current
/// stays visible through the glass while the controls on top stand proud as the
/// frostier objects. `face_curve` is what makes it read as one continuous
/// sheet: the room reflected in it slides across the whole face rather than
/// living only in the rim.
///
/// Both glows are measured against the pane's own height, so the recipe holds
/// from a column two thirds of the display tall down to a menu of three rows —
/// which is exactly how the shell reuses it, and why this is one function here
/// too. If the numbers change in `lxb-desktop` they have to change here, or the
/// login screen and the first shell frame are two different materials.
fn sidebar_surface([x, y, w, h]: [f32; 4], scale: f32, frost: f32, fade: f32) -> [Quad; 4] {
    let palette = theme::theme();
    let header_h = (300.0 * scale).min(h * 0.36);
    let foot_h = (360.0 * scale).min(h * 0.38);
    let vertical_inset = 2.0 * scale;

    [
        Quad {
            rect: [x + w * 0.06, y + vertical_inset, w * 0.88, header_h],
            slot: visual::GLOW_SLOT,
            color: palette.accent_soft.a(SIDEBAR_HEADER_LIGHT),
            fade,
            ..Quad::default()
        },
        Quad {
            rect: [
                x + w * 0.10,
                y + h - foot_h - vertical_inset,
                w * 0.80,
                foot_h,
            ],
            slot: visual::GLOW_SLOT,
            color: palette.accent.a(SIDEBAR_FOOT_LIGHT),
            fade,
            ..Quad::default()
        },
        Quad {
            rect: [x, y, w, h],
            color: palette.glass.a(SIDEBAR_STAIN),
            radius: PANEL_RADIUS * scale,
            thickness: DEPTH_SIDEBAR * scale,
            frost,
            gloss: GLOSS_SIDEBAR,
            face_curve: CURVE_SIDEBAR,
            fade,
            ..Quad::default()
        },
        Quad {
            rect: [x, y, w, h],
            color: palette.accent_soft.a(SIDEBAR_RIM),
            radius: PANEL_RADIUS * scale,
            border: (1.0 * scale).max(1.0),
            fade,
            ..Quad::default()
        },
    ]
}

/// The glass the column is made of.
fn build_column(output: &mut Output, layout: Layout, metrics: Metrics, fade: f32) {
    output.scene.quads.extend(sidebar_surface(
        layout.column,
        metrics.scale,
        FROST_SIDEBAR,
        fade,
    ));
}

/// The time, large, on the half of the display the column does not cover.
fn build_clock(output: &mut Output, view: &View<'_>, layout: Layout, fade: f32) {
    let palette = theme::theme();
    let Some(now) = view.now.filter(|_| layout.split) else {
        return;
    };
    text(
        &mut output.scene,
        &now.time(),
        layout.clock,
        layout.clock[3] / 1.22,
        palette.text.a(0.97 * fade),
        true,
        TextAlign::Center,
    );
    text(
        &mut output.scene,
        &now.date(),
        layout.date,
        layout.date[3],
        palette.text.a(0.88 * fade),
        false,
        TextAlign::Center,
    );
}

/// Who is signing in: the avatar, the greeting, and the name.
///
/// Every profile within one step of the selected one is drawn, offset and
/// faded by how far away it is, which is what makes moving between them a
/// carousel rather than a redraw. A neighbour is fully transparent by the time
/// it is one step out, so nothing is ever visible past the glass — this panel
/// cannot clip what it holds.
#[allow(clippy::too_many_arguments)]
fn build_identity(
    output: &mut Output,
    view: &View<'_>,
    layout: Layout,
    metrics: Metrics,
    pulse: f32,
    fade: f32,
    interactive: bool,
) {
    let palette = theme::theme();
    // The enumerated accounts are the carousel. The route for an account the
    // greeter cannot enumerate is not a page of it — it has its own button on
    // the bottom row — so when it is the selection it is drawn alone, with no
    // ring around it and nothing to page between.
    let pages = view.users.len();
    let other = view.selected_user >= pages;
    let selected = if other { 0 } else { view.selected_user };
    let focused = view.focus == Focus::Users && view.phase.choosing();
    let travel = layout.content_w * CAROUSEL_TRAVEL;
    let greeting = view
        .now
        .map(|now| now.greeting())
        .unwrap_or(i18n::text().welcome_back);

    let drawn: &[usize] = &if other {
        vec![usize::MAX]
    } else {
        (0..pages).collect::<Vec<_>>()
    };
    for &index in drawn {
        let offset = if other {
            0.0
        } else {
            carousel_offset(index, selected, pages) as f32 + view.carousel_shift
        };
        let presence = (1.0 - offset.abs()).clamp(0.0, 1.0);
        if presence <= 0.0 {
            continue;
        }
        let alpha = presence * fade;
        let dx = offset * travel;
        let avatar = shift_x(layout.avatar, dx);
        let (name, initial, face) = match view.users.get(index) {
            Some(user) => (
                user.display_name.as_str(),
                user.display_name
                    .chars()
                    .find(|character| character.is_alphanumeric())
                    .map(|character| character.to_uppercase().to_string())
                    .unwrap_or_else(|| "?".to_string()),
                // `avatar` is only set for an account whose picture is on the
                // GPU — see the greeter's `load_faces` — so this is a cell that
                // has certainly been written.
                user.avatar.as_ref().and_then(|_| visual::face_slot(index)),
            ),
            // The account nobody enumerated has no picture to have published.
            None => (i18n::text().different_user_profile, "+".to_string(), None),
        };
        avatar_surface(
            &mut output.scene,
            avatar,
            presence,
            focused && presence > 0.5,
            pulse,
            metrics.scale,
            alpha,
        );
        // The account's own picture where there is one, and its initial where
        // there is not. Inset, so the disc's lit rim and the selection ring
        // stay the outermost thing: a photograph run to the very edge of the
        // circle would cover the one part of the control that says it is glass.
        //
        // No depth and no gloss on this quad, which is what keeps it out of the
        // shader's glass branch — it is a picture printed on the face of the
        // disc, not a second pane over it — and a circular corner at half its
        // height, which is what cuts it to the disc.
        match face.filter(|_| presence > 0.0) {
            Some(slot) => {
                let inset = avatar[3] * 0.07;
                let disc = [
                    avatar[0] + inset,
                    avatar[1] + inset,
                    avatar[2] - inset * 2.0,
                    avatar[3] - inset * 2.0,
                ];
                output.scene.quads.push(Quad {
                    rect: disc,
                    slot,
                    // White, so the picture arrives in its own colours: the
                    // atlas multiplies this into the texel, and a portrait is
                    // the one thing here that is not the greeter's to tint.
                    color: [1.0, 1.0, 1.0, alpha],
                    radius: disc[3] * 0.5,
                    corner: visual::CIRCULAR_CORNER,
                    ..Quad::default()
                });
            }
            None => text(
                &mut output.scene,
                &initial,
                [
                    avatar[0],
                    avatar[1] + avatar[3] * 0.14,
                    avatar[2],
                    avatar[3] * 0.72,
                ],
                avatar[3] * 0.44,
                palette.text.a(alpha),
                true,
                TextAlign::Center,
            ),
        }
        // The same weight and size as the name below it. They are one address
        // — "Good evening, Alex" set on two lines — and setting the first line
        // quieter would make it a caption on the second.
        text(
            &mut output.scene,
            greeting,
            shift_x(layout.greeting, dx),
            layout.greeting[3] / 1.3,
            palette.text.a(0.96 * alpha),
            true,
            TextAlign::Left,
        );
        text(
            &mut output.scene,
            name,
            shift_x(layout.name, dx),
            layout.name[3] / 1.3,
            palette.text.a(alpha),
            true,
            TextAlign::Left,
        );
        if interactive && presence > 0.5 && view.phase.choosing() {
            output.hits.push(Hit {
                rect: [
                    layout.content_x,
                    layout.avatar[1],
                    layout.content_w,
                    layout.avatar[3],
                ],
                target: if other {
                    Target::OtherAccount
                } else {
                    Target::User(index)
                },
            });
        }
    }

    // How many accounts there are and which one this is — and nothing at all
    // when there is one account, because a row of dots under the only profile
    // on the machine is an invitation to page through something that is not
    // there. Absent too while the non-enumerated route is selected: that is not
    // one of the pages, so no page of it is the current one.
    if pages > 1 && !other && view.phase.choosing() {
        let dot = 6.0 * metrics.scale.max(0.8);
        let gap = dot * 1.9;
        let run = dot + gap * (pages - 1) as f32;
        let start = layout.pips[0] + layout.pips[2] * 0.5 - run * 0.5;
        for index in 0..pages {
            output.scene.quads.push(Quad {
                rect: [start + gap * index as f32, layout.pips[1], dot, dot],
                color: if index == selected {
                    palette.accent.a(0.95 * fade)
                } else {
                    palette.text_soft.a(0.34 * fade)
                },
                radius: dot * 0.5,
                fade,
                ..Quad::default()
            });
        }
    }
}

/// Which desktop the sign-in will start.
///
/// A mark and a name rather than a spinner between two arrows: the arrows are
/// still there for a pointer, but the shoulder buttons and this row's own
/// left/right are what a controller uses, and neither of those needs a target
/// drawn for it.
#[allow(clippy::too_many_arguments)]
fn build_session(
    output: &mut Output,
    view: &View<'_>,
    layout: Layout,
    metrics: Metrics,
    pulse: f32,
    fade: f32,
    interactive: bool,
) {
    let palette = theme::theme();
    let live = view.phase.choosing();
    // Still shown once an attempt is open — the user has to be able to see
    // what they are signing in to — but no longer something that can move.
    let alpha = fade * if live { 1.0 } else { 0.55 };
    let focused = live && view.focus == Focus::Session;
    circular_control(
        &mut output.scene,
        layout.session,
        focused,
        pulse,
        metrics.scale,
        alpha,
    );
    glyph(
        &mut output.scene,
        visual::SESSION_SLOT,
        layout.session,
        0.48,
        palette.text.a(alpha),
    );
    let name = view
        .sessions
        .get(view.selected_session)
        .map(|session| session.name.as_str())
        .unwrap_or(i18n::text().no_sessions);
    let label_x = layout.session[0] + layout.session[2] + 12.0 * metrics.scale.max(0.8);
    text(
        &mut output.scene,
        name,
        [
            label_x,
            layout.session[1] + layout.session[3] * 0.28,
            (layout.content_x + layout.content_w - label_x).max(60.0),
            layout.session[3] * 0.5,
        ],
        if metrics.compact {
            14.0
        } else {
            (15.0 * metrics.scale).clamp(metrics.px(13.0), metrics.px(20.0))
        },
        palette.text_soft.a(0.92 * alpha),
        focused,
        TextAlign::Left,
    );
    if interactive && live {
        output.hits.push(Hit {
            rect: layout.session,
            target: Target::Session,
        });
    }
}

/// Sleep, restart, shut down, and the way in for an account not on the list.
#[allow(clippy::too_many_arguments)]
fn build_footer(
    output: &mut Output,
    view: &View<'_>,
    layout: Layout,
    metrics: Metrics,
    pulse: f32,
    fade: f32,
    interactive: bool,
) {
    if !layout.footer_visible {
        return;
    }
    let palette = theme::theme();
    let count = view.footer.len();
    let [row_x, row_y, row_w, row_h] = layout.footer_row;
    let pitch = row_w / count as f32;
    let mark = layout.footer_mark;
    let label_size = if metrics.compact {
        11.0
    } else {
        (12.5 * metrics.scale).clamp(metrics.px(10.5), metrics.px(16.0))
    };
    for (index, item) in view.footer.iter().enumerate() {
        let centre = row_x + pitch * (index as f32 + 0.5);
        let button = [centre - mark * 0.5, row_y, mark, mark];
        let focused = view.focus == Focus::Footer(index);
        circular_control(
            &mut output.scene,
            button,
            focused,
            pulse,
            metrics.scale,
            fade,
        );
        glyph(
            &mut output.scene,
            item.slot(),
            button,
            0.52,
            palette.text.a(if focused { fade } else { 0.86 * fade }),
        );
        text(
            &mut output.scene,
            item.label(),
            [
                centre - pitch * 0.5,
                row_y + mark + 6.0,
                pitch,
                label_size * 2.4,
            ],
            label_size,
            palette
                .text_soft
                .a(if focused { 0.98 * fade } else { 0.76 * fade }),
            focused,
            TextAlign::Center,
        );
        if interactive {
            output.hits.push(Hit {
                // The label is part of the target: at this size the mark alone
                // is a small thing to hit with a pointer, and the words under
                // it are plainly the same button.
                rect: [centre - pitch * 0.5, row_y, pitch, row_h],
                target: item.target(),
            });
        }
    }
}

/// The way out of an attempt that has already been opened.
#[allow(clippy::too_many_arguments)]
fn build_back(
    output: &mut Output,
    view: &View<'_>,
    layout: Layout,
    metrics: Metrics,
    pulse: f32,
    fade: f32,
    interactive: bool,
) {
    // Eased with the screen change rather than appearing on the frame the
    // phase does, so it arrives with everything else that arrives.
    let shown = |phase: &Phase<'_>| if phase.cancellable() { 1.0 } else { 0.0 };
    let presence = match view
        .previous_phase
        .filter(|_| view.transition_progress < 1.0)
    {
        Some(previous) => {
            let progress = view.transition_progress.clamp(0.0, 1.0);
            shown(&previous) + (shown(&view.phase) - shown(&previous)) * progress
        }
        None => shown(&view.phase),
    };
    if presence <= 0.0 {
        return;
    }
    let palette = theme::theme();
    let alpha = presence * fade;
    circular_control(
        &mut output.scene,
        layout.back,
        view.focus == Focus::Back,
        pulse,
        metrics.scale,
        alpha,
    );
    glyph(
        &mut output.scene,
        visual::ARROW_LEFT_SLOT,
        layout.back,
        0.42,
        palette.text.a(alpha),
    );
    if interactive && presence > 0.5 {
        output.hits.push(Hit {
            rect: layout.back,
            target: Target::Back,
        });
    }
}

/// The one band of the column that differs between screens.
#[allow(clippy::too_many_arguments)]
fn build_centre(
    output: &mut Output,
    view: &View<'_>,
    layout: Layout,
    metrics: Metrics,
    phase: Phase<'_>,
    pulse: f32,
    fade: f32,
    interactive: bool,
) {
    match phase {
        Phase::Choose => build_sign_in(output, view, layout, metrics, pulse, fade, interactive),
        Phase::Username { input, error } => build_field(
            output,
            view,
            layout,
            metrics,
            i18n::text().account_name,
            false,
            input,
            error,
            pulse,
            fade,
            interactive,
        ),
        Phase::Authenticating {
            prompt,
            secret,
            input,
        } => build_field(
            output,
            view,
            layout,
            metrics,
            prompt,
            secret,
            input,
            None,
            pulse,
            fade,
            interactive,
        ),
        Phase::Busy(message) | Phase::Departing(message) => build_status(
            output, view, layout, metrics, message, false, pulse, fade, false,
        ),
        Phase::Error(message) => build_status(
            output,
            view,
            layout,
            metrics,
            message,
            true,
            pulse,
            fade,
            interactive,
        ),
    }
}

/// Where writing goes inside the centre band.
///
/// The three things that stand in that band — "Sign in", a live PAM prompt and
/// a failure's "Try again" — are one rectangle with different contents, so
/// they inset it identically or they are three rectangles wearing the same
/// paint. They are set at their own sizes, which is a difference in voice
/// rather than in furniture: the answer the user is typing is larger than the
/// label that stood in for it.
///
/// The inset is a canvas length. Left at 18 device pixels while the band
/// around it grew with the display, it would put a 4K prompt hard against the
/// glass.
fn field_text_box(layout: Layout, metrics: Metrics) -> [f32; 4] {
    let pad = metrics.px(18.0);
    [
        layout.field[0] + pad,
        layout.field[1] + layout.field[3] * 0.24,
        (layout.field[2] - pad - metrics.px(12.0)).max(metrics.px(20.0)),
        layout.field[3] * 0.6,
    ]
}

/// Before PAM has been asked anything: the field the answer will go in,
/// standing where it will stand, with what it is waiting for written in it.
///
/// It is not a live field yet and does not pretend to be — there is no
/// conversation to type into until greetd has opened one. Pressing it opens
/// that conversation, and the prompt PAM sends back lands in this same
/// rectangle a moment later, which is why it is drawn here at all: the
/// alternative is a screen whose furniture moves as soon as it is touched.
#[allow(clippy::too_many_arguments)]
fn build_sign_in(
    output: &mut Output,
    view: &View<'_>,
    layout: Layout,
    metrics: Metrics,
    pulse: f32,
    fade: f32,
    interactive: bool,
) {
    let palette = theme::theme();
    selectable(
        &mut output.scene,
        layout.field,
        view.focus == Focus::Prompt,
        pulse,
        metrics.scale,
        fade,
    );
    text(
        &mut output.scene,
        i18n::text().sign_in,
        field_text_box(layout, metrics),
        if metrics.compact {
            16.0
        } else {
            (17.0 * metrics.scale).clamp(metrics.px(15.0), metrics.px(22.0))
        },
        palette.text.a(0.92 * fade),
        false,
        TextAlign::Left,
    );
    build_submit(output, view, layout, metrics, pulse, fade);
    if interactive {
        output.hits.push(Hit {
            rect: layout.field,
            target: Target::Continue,
        });
        output.hits.push(Hit {
            rect: layout.submit,
            target: Target::Continue,
        });
    }
}

/// A live PAM question and the answer being typed into it.
#[allow(clippy::too_many_arguments)]
fn build_field(
    output: &mut Output,
    view: &View<'_>,
    layout: Layout,
    metrics: Metrics,
    prompt: &str,
    secret: bool,
    input: &str,
    validation_error: Option<&str>,
    pulse: f32,
    fade: f32,
    interactive: bool,
) {
    let palette = theme::theme();
    selectable(
        &mut output.scene,
        layout.field,
        view.focus == Focus::Prompt,
        pulse,
        metrics.scale,
        fade,
    );
    let characters = input.chars().count();
    // The prompt itself stands in the empty field. PAM's wording is the label,
    // and a label over an empty box says the same thing twice.
    let (shown, colour) = if characters == 0 {
        (prompt.to_string(), palette.text_soft.a(0.72 * fade))
    } else if secret {
        ("•".repeat(characters.min(32)), palette.text.a(fade))
    } else {
        (bounded_tail(input, 32), palette.text.a(fade))
    };
    text(
        &mut output.scene,
        &shown,
        field_text_box(layout, metrics),
        if metrics.compact {
            17.0
        } else {
            (19.0 * metrics.scale).clamp(metrics.px(16.0), metrics.px(24.0))
        },
        colour,
        false,
        TextAlign::Left,
    );
    build_submit(output, view, layout, metrics, pulse, fade);

    // The board's own key hides it again; this only ever opens it, and carries
    // the picture of it rising rather than the picture of it folding away. A
    // button says what pressing it will do.
    let toggle_hidden = view.keyboard.is_some();
    if !toggle_hidden {
        circular_control(
            &mut output.scene,
            layout.keyboard_toggle,
            view.focus == Focus::KeyboardToggle,
            pulse,
            metrics.scale,
            fade,
        );
        glyph(
            &mut output.scene,
            visual::KEYBOARD_SHOW_SLOT,
            layout.keyboard_toggle,
            0.5,
            palette.text.a(0.9 * fade),
        );
    }
    if let Some(message) = validation_error {
        text(
            &mut output.scene,
            message,
            layout.message,
            (layout.message[3] * 0.82).clamp(metrics.px(12.0), metrics.px(17.0)),
            palette.danger.a(fade),
            false,
            TextAlign::Left,
        );
    }
    if interactive {
        output.hits.push(Hit {
            rect: layout.field,
            target: Target::Prompt,
        });
        output.hits.push(Hit {
            rect: layout.submit,
            target: Target::Continue,
        });
        if !toggle_hidden {
            output.hits.push(Hit {
                rect: layout.keyboard_toggle,
                target: Target::ShowKeyboard,
            });
        }
    }
}

/// The button at the end of the field: an arrow, as in the design, because a
/// word there would have to be a different word on every screen.
fn build_submit(
    output: &mut Output,
    view: &View<'_>,
    layout: Layout,
    metrics: Metrics,
    pulse: f32,
    fade: f32,
) {
    let palette = theme::theme();
    selectable(
        &mut output.scene,
        layout.submit,
        view.focus == Focus::Continue,
        pulse,
        metrics.scale,
        fade,
    );
    glyph(
        &mut output.scene,
        visual::ARROW_RIGHT_SLOT,
        layout.submit,
        0.40,
        palette.text.a(fade),
    );
}

/// What the machine is doing, or what went wrong, standing where the field
/// stands on every other screen.
///
/// The button keeps the field's shape and the field's place, so a refusal does
/// not rearrange the column around the thing the user is about to press again.
#[allow(clippy::too_many_arguments)]
fn build_status(
    output: &mut Output,
    view: &View<'_>,
    layout: Layout,
    metrics: Metrics,
    message: &str,
    error: bool,
    pulse: f32,
    fade: f32,
    interactive: bool,
) {
    let palette = theme::theme();
    let label_size = if metrics.compact {
        16.0
    } else {
        (17.0 * metrics.scale).clamp(metrics.px(15.0), metrics.px(22.0))
    };
    if error {
        selectable(
            &mut output.scene,
            layout.field,
            view.focus == Focus::Continue,
            pulse,
            metrics.scale,
            fade,
        );
        text(
            &mut output.scene,
            i18n::text().try_again,
            field_text_box(layout, metrics),
            label_size,
            palette.text.a(fade),
            false,
            TextAlign::Left,
        );
        build_submit(output, view, layout, metrics, pulse, fade);
        // Under the button rather than in it: what went wrong is PAM's
        // sentence, and it does not fit on a button.
        text(
            &mut output.scene,
            message,
            layout.message,
            (layout.message[3] * 0.82).clamp(metrics.px(12.0), metrics.px(17.0)),
            palette.danger.a(0.95 * fade),
            false,
            TextAlign::Left,
        );
        if interactive {
            output.hits.push(Hit {
                rect: layout.field,
                target: Target::Retry,
            });
            output.hits.push(Hit {
                rect: layout.submit,
                target: Target::Retry,
            });
        }
        return;
    }

    // Nothing to press. The accent breathing under the words is the whole of
    // the progress indication: this state lasts as long as PAM takes, which is
    // not a quantity anything here could honestly draw a bar for.
    let glow = layout.field[3] * 2.2;
    output.scene.quads.push(Quad {
        rect: [
            layout.field[0] + layout.field[2] * 0.5 - glow * 0.5,
            layout.field[1] + layout.field[3] * 0.5 - glow * 0.5,
            glow,
            glow,
        ],
        slot: visual::GLOW_SLOT,
        color: palette.accent.a((0.18 + pulse * 0.10) * fade),
        ..Quad::default()
    });
    text(
        &mut output.scene,
        message,
        [
            layout.field[0],
            layout.field[1] + layout.field[3] * 0.24,
            layout.field[2] + layout.submit[2],
            layout.field[3] * 0.6,
        ],
        label_size,
        palette.text.a(0.95 * fade),
        false,
        TextAlign::Left,
    );
}

/// Where the session menu settles: beside the badge it is about, on whichever
/// side of it there is room for, and never off the display.
///
/// The rows are the sessions, so the panel is exactly as tall as it needs to be
/// until that would run past the display, at which point it stops growing and
/// scrolls instead.
fn menu_rect(layout: Layout, metrics: Metrics, count: usize) -> ([f32; 4], usize, usize, usize) {
    let scale = metrics.scale;
    let inset = PANEL_INSET * scale;
    let margin = MENU_MARGIN * scale;
    let row = (MENU_ROW * scale).max(MIN_ACTION);
    let width = (MENU_WIDTH * scale).min((metrics.width - inset * 2.0).max(120.0));

    let room = (metrics.height - inset * 2.0 - margin * 2.0).max(row);
    let fits = ((room / row) as usize).max(1);
    let visible = count.clamp(1, fits);
    let height = visible as f32 * row + margin * 2.0;

    let [ax, ay, aw, ah] = layout.session;
    // To the right by preference: the column is on the left of the display, so
    // that is the side with room on it. Flipped when it would run off the edge,
    // which is what makes one rule serve a narrow display and a wide one.
    let right = ax + aw + MENU_GAP * scale;
    let x = if right + width <= metrics.width - inset {
        right
    } else {
        (ax - MENU_GAP * scale - width).max(inset)
    };
    // Centred on the anchor, so the panel reads as hanging off it rather than
    // as having been dropped beside it.
    let y =
        (ay + ah * 0.5 - height * 0.5).clamp(inset, (metrics.height - inset - height).max(inset));
    (
        [
            x.min((metrics.width - inset - width).max(inset)),
            y,
            width,
            height,
        ],
        visible,
        fits,
        count,
    )
}

/// Which row the list starts at, so the selection is always on screen.
fn menu_first_visible(selected: usize, visible: usize, count: usize) -> usize {
    if count <= visible {
        return 0;
    }
    selected.saturating_sub(visible - 1).min(count - visible)
}

/// The chip for row `index`, or `None` when it is scrolled out of the panel.
fn menu_row_rect(
    layout: Layout,
    metrics: Metrics,
    count: usize,
    selected: usize,
    index: usize,
) -> Option<[f32; 4]> {
    let ([px, py, pw, _], visible, _, _) = menu_rect(layout, metrics, count);
    let first = menu_first_visible(selected, visible, count);
    if index < first || index >= first + visible {
        return None;
    }
    let scale = metrics.scale;
    let margin = MENU_MARGIN * scale;
    let padding = MENU_ROW_PADDING * scale;
    let row = (MENU_ROW * scale).max(MIN_ACTION);
    Some([
        px + margin,
        py + margin + (index - first) as f32 * row + padding,
        pw - margin * 2.0,
        row - padding * 2.0,
    ])
}

/// Where the panel is when it is `progress` of the way out of its anchor.
///
/// It leaves as one shape rather than growing into its proportions, so the
/// contents ride out on the very same factor and nothing inside the panel moves
/// relative to anything else on the way.
fn menu_bounds(rect: [f32; 4], anchor: [f32; 4], progress: f32) -> [f32; 4] {
    let [px, py, pw, ph] = rect;
    let [ax, ay, aw, ah] = anchor;
    if pw <= 0.0 {
        return rect;
    }
    let factor = lerp((aw / pw).min(1.0), 1.0, progress);
    let cx = lerp(ax + aw * 0.5, px + pw * 0.5, progress);
    let cy = lerp(ay + ah * 0.5, py + ph * 0.5, progress);
    [
        cx - pw * factor * 0.5,
        cy - ph * factor * 0.5,
        pw * factor,
        ph * factor,
    ]
}

fn lerp(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

/// The session menu: which desktop this sign-in will start.
#[allow(clippy::too_many_arguments)]
fn build_menu(
    output: &mut Output,
    view: &View<'_>,
    layout: Layout,
    metrics: Metrics,
    menu: Menu,
    pulse: f32,
    fade: f32,
) {
    let palette = theme::theme();
    let count = view.sessions.len();
    if count == 0 {
        return;
    }
    let progress = menu.progress.clamp(0.0, 1.0);
    let (rect, visible, _, _) = menu_rect(layout, metrics, count);
    let bounds = menu_bounds(rect, layout.session, progress);
    let scale = metrics.scale;

    // What the panel stands in front of, dimmed. It is also the target that
    // dismisses the menu, so pressing anywhere else is an answer of "not this".
    output.scene.quads.push(Quad {
        rect: [0.0, 0.0, metrics.width, metrics.height],
        color: [0.0, 0.0, 0.0, MENU_DIM * progress * fade],
        ..Quad::default()
    });
    if menu.interactive {
        output.hits.push(Hit {
            rect: [0.0, 0.0, metrics.width, metrics.height],
            target: Target::DismissMenu,
        });
    }

    // Cut from the column's own glass, through [`sidebar_surface`], because it
    // is the same kind of object: a quiet pane with things to press laid on it.
    // The modal recipe — the near-opaque, wholly frosted slab the on-screen
    // keyboard rests under — is right for a surface that takes the screen over
    // and wrong for a note attached to a badge. It would also have hidden the
    // one thing this panel is standing on: the column it belongs to.
    output
        .scene
        .quads
        .extend(sidebar_surface(bounds, scale, FROST_MENU, fade * progress));

    // The contents ride the same flight as the panel: settled rectangles moved
    // and scaled by exactly what moved and scaled the slab.
    let factor = if rect[2] > 0.0 {
        bounds[2] / rect[2]
    } else {
        1.0
    };
    let ride = |settled: [f32; 4]| {
        [
            bounds[0] + (settled[0] - rect[0]) * factor,
            bounds[1] + (settled[1] - rect[1]) * factor,
            settled[2] * factor,
            settled[3] * factor,
        ]
    };
    // Nothing legible until the panel is most of the way out, for the reason
    // the shell gives about its own panels: a list drawn at the size of a badge
    // reads as a shrunken list being enlarged rather than as one opening.
    let ink = smoothstep(0.45, 1.0, progress) * fade;
    if ink <= 0.0 {
        return;
    }

    let first = menu_first_visible(menu.selected, visible, count);
    for index in first..(first + visible).min(count) {
        let Some(settled) = menu_row_rect(layout, metrics, count, menu.selected, index) else {
            continue;
        };
        let row = ride(settled);
        let chosen = index == view.selected_session;
        let focused = index == menu.selected;
        selectable(&mut output.scene, row, focused, pulse, scale, ink);
        let label_x = row[0] + 14.0 * scale.max(0.8);
        let tick_w = row[3] * 0.7;
        text(
            &mut output.scene,
            &view.sessions[index].name,
            [
                label_x,
                row[1] + row[3] * 0.26,
                (row[2] - (label_x - row[0]) - tick_w).max(20.0),
                row[3] * 0.55,
            ],
            (15.0 * scale).clamp(metrics.px(13.0), metrics.px(20.0)),
            palette.text.a(if focused { ink } else { 0.92 * ink }),
            focused,
            TextAlign::Left,
        );
        // The one already chosen keeps its mark whichever row the cursor is on,
        // because "where I am" and "what is set" are two different questions.
        if chosen {
            glyph(
                &mut output.scene,
                visual::SESSION_SLOT,
                [row[0] + row[2] - tick_w, row[1], tick_w, row[3]],
                0.44,
                palette.accent_soft.a(ink),
            );
        }
        if menu.interactive && progress >= 0.999 {
            output.hits.push(Hit {
                rect: row,
                target: Target::SessionOption(index),
            });
        }
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn shift_x(rect: [f32; 4], dx: f32) -> [f32; 4] {
    [rect[0] + dx, rect[1], rect[2], rect[3]]
}

fn carousel_offset(index: usize, selected: usize, count: usize) -> isize {
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

/// A round glass button: the shape every mark in this design sits in.
fn circular_control(
    scene: &mut Scene,
    rect: [f32; 4],
    focused: bool,
    pulse: f32,
    scale: f32,
    fade: f32,
) {
    let palette = theme::theme();
    if focused {
        let glow = rect[3] * 2.1;
        scene.quads.push(Quad {
            rect: [
                rect[0] + rect[2] * 0.5 - glow * 0.5,
                rect[1] + rect[3] * 0.5 - glow * 0.5,
                glow,
                glow,
            ],
            slot: visual::GLOW_SLOT,
            color: palette.accent.a((0.30 + pulse * 0.08) * fade),
            ..Quad::default()
        });
    }
    scene.quads.push(Quad {
        rect,
        color: if focused {
            palette.accent.a(0.46 * fade)
        } else {
            palette.glass_raised.a(0.13 * fade)
        },
        radius: rect[3] * 0.5,
        // A true circle rather than the squircle the rectangular controls use.
        // At a radius of exactly half the side the superellipse the rest of
        // the interface is drawn with is visibly not round, and a row of not
        // quite round buttons under a round avatar reads as a mistake.
        corner: visual::CIRCULAR_CORNER,
        thickness: DEPTH_CONTROL * scale.max(0.72),
        frost: FROST_CONTROL,
        gloss: if focused { GLOSS_FULL } else { GLOSS_QUIET },
        fade,
        ..Quad::default()
    });
}

fn selectable(
    scene: &mut Scene,
    rect: [f32; 4],
    selected: bool,
    pulse: f32,
    scale: f32,
    fade: f32,
) {
    let palette = theme::theme();
    if selected {
        let glow = rect[3] * 2.0;
        scene.quads.push(Quad {
            rect: [
                rect[0] + rect[2] * 0.5 - glow * 0.5,
                rect[1] + rect[3] * 0.5 - glow * 0.5,
                glow,
                glow,
            ],
            slot: visual::GLOW_SLOT,
            color: palette.accent.a((0.28 + pulse * 0.08) * fade),
            ..Quad::default()
        });
    }
    scene.quads.push(Quad {
        rect,
        color: if selected {
            palette.accent.a(0.47 * fade)
        } else {
            palette.glass_raised.a(0.12 * fade)
        },
        radius: CONTROL_RADIUS * scale,
        corner: visual::SQUIRCLE_CORNER,
        thickness: DEPTH_CONTROL * scale,
        frost: FROST_CONTROL,
        gloss: if selected { GLOSS_FULL } else { GLOSS_QUIET },
        fade,
        ..Quad::default()
    });
}

fn text(
    scene: &mut Scene,
    content: &str,
    rect: [f32; 4],
    size: f32,
    color: [f32; 4],
    bold: bool,
    align: TextAlign,
) {
    scene.texts.push(Text {
        content: content.to_string(),
        rect,
        size,
        color,
        bold,
        align,
        clip: None,
    });
}

pub fn target_at(hits: &[Hit], x: f32, y: f32) -> Option<Target> {
    hits.iter()
        .rev()
        .find(|hit| contains(hit.rect, x, y))
        .map(|hit| hit.target)
}

fn contains([x, y, w, h]: [f32; 4], px: f32, py: f32) -> bool {
    px >= x && px < x + w && py >= y && py < y + h
}

fn avatar_surface(
    scene: &mut Scene,
    rect: [f32; 4],
    selection: f32,
    focused: bool,
    pulse: f32,
    scale: f32,
    fade: f32,
) {
    let palette = theme::theme();
    if focused {
        let glow = rect[3] * 1.85;
        scene.quads.push(Quad {
            rect: [
                rect[0] + rect[2] * 0.5 - glow * 0.5,
                rect[1] + rect[3] * 0.5 - glow * 0.5,
                glow,
                glow,
            ],
            slot: visual::GLOW_SLOT,
            color: palette.accent.a((0.28 + pulse * 0.08) * fade),
            ..Quad::default()
        });
    }
    scene.quads.push(Quad {
        rect,
        color: palette.glass_raised.a(0.12 * fade),
        radius: rect[3] * 0.5,
        corner: visual::CIRCULAR_CORNER,
        thickness: DEPTH_CONTROL * scale.max(0.72),
        frost: FROST_CONTROL,
        gloss: if selection > 0.5 {
            GLOSS_FULL
        } else {
            GLOSS_QUIET
        },
        fade,
        ..Quad::default()
    });
    if selection > 0.0 {
        scene.quads.push(Quad {
            rect,
            color: palette
                .accent
                .a((0.18 + 0.22 * selection + if focused { 0.07 } else { 0.0 }) * fade),
            radius: rect[3] * 0.5,
            corner: visual::CIRCULAR_CORNER,
            fade,
            ..Quad::default()
        });
    }
}

fn glyph(scene: &mut Scene, slot: u32, rect: [f32; 4], share: f32, color: [f32; 4]) {
    let mark = rect[3] * share;
    scene.quads.push(Quad {
        rect: [
            rect[0] + rect[2] * 0.5 - mark * 0.5,
            rect[1] + rect[3] * 0.5 - mark * 0.5,
            mark,
            mark,
        ],
        slot,
        color,
        ..Quad::default()
    });
}

fn bounded_tail(input: &str, limit: usize) -> String {
    let characters = input.chars().collect::<Vec<_>>();
    characters[characters.len().saturating_sub(limit)..]
        .iter()
        .collect()
}

fn build_keyboard(
    output: &mut Output,
    board: &Board,
    width: f32,
    height: f32,
    arrived: f32,
    time: f32,
    interactive: bool,
) {
    let palette = theme::theme();
    let scale = board_scale(width, height);
    let pulse = 0.5 + 0.5 * (time * std::f32::consts::TAU / PULSE_PERIOD).sin();
    let [panel_x, panel_y, panel_w, panel_h] = keyboard_panel_rect(width, height);
    let lift = (1.0 - arrived.clamp(0.0, 1.0)) * (panel_h + BOARD_MARGIN * scale);
    output.scene.quads.push(Quad {
        rect: [panel_x, panel_y + lift, panel_w, panel_h],
        color: palette.glass.a(0.52),
        radius: PANEL_RADIUS * scale,
        thickness: DEPTH_PANEL * scale,
        frost: FROST_PANEL,
        gloss: GLOSS_FULL,
        ..Quad::default()
    });
    let selected = board.selected();
    for row in 0..keyboard::ROW_COUNT {
        for (column, (key, _)) in keyboard::row_spans(row).into_iter().enumerate() {
            let [x, y, w, h] = keyboard_key_rect(row, column, width, height);
            let rect = [x, y + lift, w, h];
            let focused = (row, column) == selected;
            let held = board.latched(key) != Latch::Off;
            if focused {
                let glow = h * 2.2;
                output.scene.quads.push(Quad {
                    rect: [
                        x + w * 0.5 - glow * 0.5,
                        y + lift + h * 0.5 - glow * 0.5,
                        glow,
                        glow,
                    ],
                    slot: visual::GLOW_SLOT,
                    color: palette.accent.a(0.30 + 0.08 * pulse),
                    ..Quad::default()
                });
            }
            output.scene.quads.push(Quad {
                rect,
                color: if focused {
                    palette.accent.a(0.52 + 0.05 * pulse)
                } else if held {
                    palette.accent.a(if board.latched(key) == Latch::Locked {
                        0.50
                    } else {
                        0.32
                    })
                } else if matches!(key, Key::Char(..)) {
                    palette.glass_raised.a(0.10)
                } else {
                    palette.glass_raised.a(0.17)
                },
                radius: KEY_RADIUS * scale,
                corner: visual::SQUIRCLE_CORNER,
                thickness: DEPTH_CONTROL * scale,
                frost: FROST_CONTROL,
                gloss: if focused { GLOSS_FULL } else { GLOSS_QUIET },
                ..Quad::default()
            });
            if key.is_close() {
                output.scene.quads.push(Quad {
                    rect,
                    color: palette.accent_soft.a(if focused { 0.55 } else { 0.34 }),
                    radius: KEY_RADIUS * scale,
                    corner: visual::SQUIRCLE_CORNER,
                    border: 1.5 * scale,
                    ..Quad::default()
                });
            }
            if let Some(slot) = glyph_slot(key) {
                let mark = h * 0.46;
                output.scene.quads.push(Quad {
                    rect: [
                        x + w * 0.5 - mark * 0.5,
                        y + lift + h * 0.5 - mark * 0.5,
                        mark,
                        mark,
                    ],
                    slot,
                    color: palette.text.a(if focused { 1.0 } else { 0.82 }),
                    ..Quad::default()
                });
            } else {
                let label = key.cap(board.shifted());
                let size = if row == 0 {
                    KEY_CAP_FUNCTION * scale
                } else if matches!(key, Key::Char(..)) {
                    26.0 * scale
                } else {
                    17.0 * scale
                };
                text(
                    &mut output.scene,
                    &label,
                    [x, y + lift + h * 0.5 - size * 0.66, w, size * 1.35],
                    size,
                    palette.text.a(if focused || held { 1.0 } else { 0.82 }),
                    focused,
                    TextAlign::Center,
                );
            }
            if interactive {
                output.hits.push(Hit {
                    rect,
                    target: Target::Key(row, column),
                });
            }
        }
    }
}

fn glyph_slot(key: Key) -> Option<u32> {
    match key {
        Key::Arrow(Arrow::Left) => Some(visual::ARROW_LEFT_SLOT),
        Key::Arrow(Arrow::Down) => Some(visual::ARROW_DOWN_SLOT),
        Key::Arrow(Arrow::Up) => Some(visual::ARROW_UP_SLOT),
        Key::Arrow(Arrow::Right) => Some(visual::ARROW_RIGHT_SLOT),
        Key::Close => Some(visual::KEYBOARD_HIDE_SLOT),
        _ => None,
    }
}

fn board_scale(width: f32, height: f32) -> f32 {
    let base = (height / OSK_REFERENCE_HEIGHT).clamp(0.6, 2.5);
    let natural = (KEY_UNIT * keyboard::COLUMNS + (BOARD_PADDING + BOARD_MARGIN) * 2.0) * base;
    if width > 0.0 && natural > width {
        base * width / natural
    } else {
        base
    }
}

fn row_band(row: usize) -> (f32, f32) {
    let top = (0..row)
        .map(|above| keyboard::row_scale(above) * KEY_HEIGHT + KEY_GAP)
        .sum();
    (top, keyboard::row_scale(row) * KEY_HEIGHT)
}

fn keys_height() -> f32 {
    let (top, height) = row_band(keyboard::ROW_COUNT - 1);
    top + height
}

pub fn keyboard_panel_rect(width: f32, height: f32) -> [f32; 4] {
    let scale = board_scale(width, height);
    let w = (KEY_UNIT * keyboard::COLUMNS + BOARD_PADDING * 2.0) * scale;
    let h = (keys_height() + BOARD_PADDING * 2.0) * scale;
    [(width - w) * 0.5, height - BOARD_MARGIN * scale - h, w, h]
}

pub fn keyboard_key_rect(row: usize, column: usize, width: f32, height: f32) -> [f32; 4] {
    let scale = board_scale(width, height);
    let [panel_x, panel_y, _, _] = keyboard_panel_rect(width, height);
    let unit = KEY_UNIT * scale;
    let gap = KEY_GAP * scale;
    let padding = BOARD_PADDING * scale;
    let (start, span) = keyboard::row_layout(row)
        .get(column)
        .copied()
        .unwrap_or((0.0, 1.0));
    let (top, tall) = row_band(row);
    [
        panel_x + padding + start * unit + gap * 0.5,
        panel_y + padding + top * scale,
        span * unit - gap,
        tall * scale,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::Kind;
    use std::path::PathBuf;

    /// Every real display the greeter will be put on, as physical pixels —
    /// which is what `build` is handed, and the reason none of this showed up
    /// in the 1280×720 preview window, where the scale is exactly 1.
    const DISPLAYS: [(f32, f32); 5] = [
        (1280.0, 720.0),
        (1600.0, 900.0),
        (1920.0, 1080.0),
        (2560.0, 1440.0),
        (3840.0, 2160.0),
    ];

    fn layout_at(width: f32, height: f32) -> Layout {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        Layout::new(
            Metrics::new(width, height),
            &view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Users,
                FOOTER.as_slice(),
            ),
        )
    }

    /// The band a password is typed into keeps its share of the display.
    ///
    /// It is measured as what is *left* of the column once the avatar, the
    /// margins and the button beside it have taken theirs, so it is the first
    /// thing to be spent when a length stops scaling. A column pinned to 620
    /// physical pixels while its own furniture grew to two and a half times
    /// used to leave the prompt an 80-pixel sliver — narrower than its own
    /// submit button, and taller than it was wide — on exactly the 4K panel a
    /// console greeter is most likely to be plugged into.
    #[test]
    fn the_prompt_keeps_its_share_of_every_display() {
        for (width, height) in DISPLAYS {
            let layout = layout_at(width, height);
            let share = layout.field[2] / width;
            assert!(
                share >= 0.12,
                "the field is {:.0}px on a {width}x{height} display, {:.1}% of it",
                layout.field[2],
                share * 100.0
            );
            assert!(
                layout.field[2] > layout.submit[2] * 2.0,
                "the field ({:.0}px) is not clearly wider than its own button \
                 ({:.0}px) at {width}x{height}",
                layout.field[2],
                layout.submit[2]
            );
            assert!(
                layout.field[2] > layout.field[3],
                "the field is taller than it is wide at {width}x{height}"
            );
        }
    }

    /// Nothing in the column stops growing while the rest of it carries on.
    ///
    /// The interface is drawn against a 1280×720 canvas and multiplied up to
    /// the surface it is given, so the composition at 4K has to be the
    /// composition at 720p. Anything left in raw device pixels — the bound of
    /// a `clamp` as much as a length — shows up here as a proportion that
    /// drifts with the display.
    #[test]
    fn the_column_is_the_same_composition_at_every_resolution() {
        let reference = layout_at(1280.0, 720.0);
        let share = |layout: &Layout, of: f32| {
            [
                layout.column[2] / of,
                layout.avatar[2] / of,
                layout.field[2] / of,
                layout.field[3] / of,
            ]
        };
        let want = share(&reference, 1280.0);
        for (width, height) in DISPLAYS {
            // Above 3200×1800 the scale deliberately stops at 2.5, so the
            // canvas is no longer tracked exactly; every display below that
            // has to match the reference composition.
            if Metrics::new(width, height).scale >= 2.5 {
                continue;
            }
            let got = share(&layout_at(width, height), width);
            for (got, want) in got.iter().zip(want.iter()) {
                assert!(
                    (got - want).abs() < 0.01,
                    "{width}x{height} draws {got:.3} of the width where 1280x720 \
                     draws {want:.3}"
                );
            }
        }
    }

    /// The face is set against the middle of the group it belongs to — the
    /// greeting, the name and the band that answers for them — rather than
    /// against the two lines alone, which left it riding at the shoulder of a
    /// field that appeared to hang off nothing.
    #[test]
    fn the_avatar_is_centred_on_the_identity_and_its_field() {
        for (width, height) in DISPLAYS {
            let layout = layout_at(width, height);
            let avatar = layout.avatar[1] + layout.avatar[3] * 0.5;
            let group = (layout.greeting[1] + layout.field[1] + layout.field[3]) * 0.5;
            assert!(
                (avatar - group).abs() < 1.0,
                "at {width}x{height} the avatar's middle is {avatar:.1} and the \
                 group it stands against runs to {group:.1}"
            );
        }
    }

    /// The session row answers for the field, so it lines up with the field
    /// and follows directly under the line reserved for validation — not on
    /// the left-hand column's edge, which is where the profile dots live.
    #[test]
    fn the_session_row_lines_up_under_the_field_it_answers_for() {
        for (width, height) in DISPLAYS {
            let layout = layout_at(width, height);
            assert_eq!(
                layout.session[0], layout.field[0],
                "the session badge does not start where the field starts at \
                 {width}x{height}"
            );
            let message_bottom = layout.message[1] + layout.message[3];
            let gap = layout.session[1] - message_bottom;
            assert!(
                (0.0..=Metrics::new(width, height).px(4.0)).contains(&gap),
                "the session row sits {gap:.1}px under the reserved line at \
                 {width}x{height}"
            );
            assert!(
                layout.session[1] > layout.field[1] + layout.field[3],
                "the session row overlaps the field at {width}x{height}"
            );
        }
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

    fn session(name: &str) -> Session {
        Session {
            id: name.to_lowercase(),
            name: name.to_string(),
            comment: None,
            command: vec![name.to_lowercase()],
            desktop_names: vec![name.to_string()],
            kind: Kind::Wayland,
            source: PathBuf::from(format!("/usr/share/wayland-sessions/{name}.desktop")),
            line_xin_bar: name == "LineXinBar",
        }
    }

    const FOOTER: [FooterItem; 4] = [
        FooterItem::Power(power::Action::Sleep),
        FooterItem::Power(power::Action::Restart),
        FooterItem::Power(power::Action::ShutDown),
        FooterItem::DifferentUser,
    ];

    fn view<'a>(
        users: &'a [User],
        sessions: &'a [Session],
        phase: Phase<'a>,
        focus: Focus,
        footer: &'a [FooterItem],
    ) -> View<'a> {
        View {
            users,
            selected_user: 0,
            sessions,
            selected_session: 0,
            focus,
            phase,
            previous_phase: None,
            transition_progress: 1.0,
            keyboard: None,
            keyboard_interactive: false,
            keyboard_arrival: 0.0,
            time: 0.0,
            // Fully arrived, which is where every test below wants the screen:
            // what is being checked is the composition, not its entrance.
            arrival: 1.0,
            departure: 0.0,
            carousel_shift: 0.0,
            footer,
            now: crate::clock::Now::read(),
            session_menu: None,
        }
    }

    /// The one-display composition, which is what every test here is about:
    /// a display is laid out in its own pixels from its own corner, so what is
    /// true of the only display is true of each of a row of them.
    fn one_display(view: View<'_>, width: f32, height: f32) -> Output {
        build(view, &[Display::whole(width, height)])
    }

    /// Nine languages, and every one of them has to fit where it is written.
    ///
    /// This is the check that a translation is a translation rather than a
    /// longer sentence in the same box. Nothing here wraps onto the column:
    /// the reserved line under the button is one line tall at every size, the
    /// caps of the on-screen keyboard are the width of the keys under them,
    /// and text laid out past its rectangle is *cut* — so a sentence that does
    /// not fit is not a longer sentence, it is half a sentence, and the half
    /// that is missing is the end of it.
    ///
    /// Measured by shaping each run exactly as the renderer will, in the faces
    /// this greeter ships, on the displays it will be put on. A word that has
    /// to be shortened is shortened in [`crate::i18n`], which is the only
    /// place any of them are written.
    #[test]
    fn no_translation_overflows_the_place_it_is_written() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        let board = Board::default();
        for language in i18n::ALL {
            i18n::with_language(language, || {
                let strings = language.strings();
                // Every sentence that can stand on the reserved line under the
                // button, including the longest of the power refusals: those
                // are the greeter's own words wrapped around a translated
                // action, so they are the longest thing this screen ever says.
                let refusals = [
                    power::Action::ShutDown.refusal(),
                    i18n::fill(strings.power_unavailable, power::Action::Restart.label()),
                    i18n::fill(strings.power_not_in_preview, power::Action::Restart.label()),
                    strings.incorrect_account_or_password.to_string(),
                    strings.worker_unavailable.to_string(),
                    strings.attempt_lost.to_string(),
                    strings.service_refused.to_string(),
                ];
                let mut phases = vec![
                    Phase::Choose,
                    Phase::Username {
                        input: "",
                        error: Some(strings.name_whitespace),
                    },
                    Phase::Username {
                        input: "",
                        error: Some(strings.name_control),
                    },
                    Phase::Authenticating {
                        prompt: strings.password,
                        secret: true,
                        input: "",
                    },
                    Phase::Busy(strings.starting_authentication),
                    Phase::Busy(strings.checking),
                    Phase::Departing(strings.opening_session),
                ];
                phases.extend(refusals.iter().map(|message| Phase::Error(message)));

                for (width, height) in [(1280.0, 720.0), (1920.0, 1080.0), (3840.0, 2160.0)] {
                    for phase in &phases {
                        // Once with the board down and once with it up, because
                        // the caps are only in the scene while it is up.
                        for board in [None, Some(&board)] {
                            let mut view =
                                view(&users, &sessions, *phase, Focus::Prompt, FOOTER.as_slice());
                            view.keyboard = board;
                            view.keyboard_arrival = 1.0;
                            let output = one_display(view, width, height);
                            for text in &output.scene.texts {
                                let (right, lines_over) = crate::visual::tests::overflow(text);
                                assert!(
                                    right <= 0.5 && lines_over == 0,
                                    "{}: {:?} runs {right:.1}px past the right of its \
                                     {:.0}x{:.0} box and wraps onto {lines_over} line(s) \
                                     it has no room for, at {width}x{height}",
                                    language.endonym(),
                                    text.content,
                                    text.rect[2],
                                    text.rect[3],
                                );
                            }
                        }
                    }
                }
            });
        }

        // And the check has teeth. The reserved line under the button is one
        // line at every size, so a sentence half again as long as the longest
        // one shipped has to be reported as running off it — a check that let
        // everything above through by measuring nothing would look identical
        // from here.
        let sprawling = "Ta nazwa konta zawiera nieprawidłowy znak, a hasło do niej \
                         nie zostało przyjęte przez usługę logowania."
            .to_string();
        let output = one_display(
            view(
                &users,
                &sessions,
                Phase::Error(&sprawling),
                Focus::Continue,
                FOOTER.as_slice(),
            ),
            1280.0,
            720.0,
        );
        assert!(
            output
                .scene
                .texts
                .iter()
                .filter(|text| text.content == sprawling)
                .any(|text| {
                    let (right, lines_over) = crate::visual::tests::overflow(text);
                    right > 0.5 || lines_over > 0
                }),
            "a sentence too long for the reserved line was not reported as one"
        );
    }

    fn hit(output: &Output, target: Target) -> [f32; 4] {
        output
            .hits
            .iter()
            .find(|hit| hit.target == target)
            .unwrap_or_else(|| panic!("missing hit target {target:?}"))
            .rect
    }

    fn has(output: &Output, target: Target) -> bool {
        output.hits.iter().any(|hit| hit.target == target)
    }

    #[test]
    fn every_key_can_be_hit_at_its_center() {
        for row in 0..keyboard::ROW_COUNT {
            for column in 0..keyboard::row_spans(row).len() {
                let rect = keyboard_key_rect(row, column, 1920.0, 1080.0);
                let hit = Hit {
                    rect,
                    target: Target::Key(row, column),
                };
                assert_eq!(
                    target_at(&[hit], rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5),
                    Some(Target::Key(row, column))
                );
            }
        }
    }

    /// The column is a column: everything the user can press is inside it,
    /// and the half of the display the clock is on stays wallpaper.
    #[test]
    fn every_control_stays_inside_the_sign_in_column() {
        let users = [user("Alex"), user("Sam")];
        let sessions = [session("LineXinBar")];
        for (width, height) in [(1920.0, 1080.0), (1366.0, 768.0), (1280.0, 720.0)] {
            let output = one_display(
                view(
                    &users,
                    &sessions,
                    Phase::Choose,
                    Focus::Users,
                    FOOTER.as_slice(),
                ),
                width,
                height,
            );
            // Asked of the layout rather than worked out again here: a test
            // that recomputes the column's width is a test that agrees with a
            // copy of the formula instead of with the screen.
            let metrics = Metrics::new(width, height);
            let column = Layout::new(
                metrics,
                &view(
                    &users,
                    &sessions,
                    Phase::Choose,
                    Focus::Users,
                    FOOTER.as_slice(),
                ),
            )
            .column;
            let column_width = column[0] + column[2];
            for hit in &output.hits {
                assert!(
                    hit.rect[0] >= 0.0 && hit.rect[0] + hit.rect[2] <= column_width + 1.0,
                    "{:?} at {:?} escapes a {column_width}-wide column",
                    hit.target,
                    hit.rect
                );
            }
        }
    }

    /// A monitor beside another monitor is a screen, not a half of one.
    ///
    /// The greeter is handed one surface across the whole row of them, and the
    /// composition it draws into that surface has to be one login screen per
    /// display: everything the user can press inside the display it was drawn
    /// on, and nothing spanning the seam between two monitors.
    #[test]
    fn every_display_gets_a_login_screen_of_its_own() {
        let users = [user("Alex"), user("Sam")];
        let sessions = [session("LineXinBar")];
        let displays = [
            Display {
                rect: [0.0, 0.0, 2560.0, 1440.0],
            },
            Display {
                rect: [2560.0, 0.0, 1920.0, 1080.0],
            },
        ];
        let output = build(
            view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Users,
                FOOTER.as_slice(),
            ),
            &displays,
        );

        // The wallpaper is named once per display, in that display's own shape.
        assert_eq!(
            output.scene.displays,
            displays
                .iter()
                .map(|display| display.rect)
                .collect::<Vec<_>>()
        );

        // Everything drawn says which display it stands on, and stands on the
        // one it is drawn over. A pane that named the wrong display would be
        // cut to a screen it is not on, and would transmit that screen's
        // wallpaper wherever nothing was drawn behind it.
        for quad in &output.scene.quads {
            let middle = [
                quad.rect[0] + quad.rect[2] * 0.5,
                quad.rect[1] + quad.rect[3] * 0.5,
            ];
            let [dx, dy, dw, dh] = quad.display;
            assert!(
                displays.iter().any(|display| display.rect == quad.display),
                "a quad at {:?} stands on no display",
                quad.rect
            );
            assert!(
                (dx..dx + dw).contains(&middle[0]) && (dy..dy + dh).contains(&middle[1]),
                "a quad at {:?} is not on the display it says it is on, {:?}",
                quad.rect,
                quad.display
            );
        }

        // And every control exists once on each of them, in that display's own
        // pixels. The same target twice is the point: either screen can be
        // pressed, and both answer the one conversation behind them.
        for target in [
            Target::Continue,
            Target::Session,
            Target::User(0),
            Target::DifferentUser,
        ] {
            let mut counted = Vec::new();
            for display in &displays {
                let [dx, dy, dw, dh] = display.rect;
                counted.push(
                    output
                        .hits
                        .iter()
                        .filter(|hit| hit.target == target)
                        .filter(|hit| {
                            let [x, y, w, h] = hit.rect;
                            x >= dx && y >= dy && x + w <= dx + dw && y + h <= dy + dh
                        })
                        .count(),
                );
            }
            assert!(
                counted.iter().all(|count| *count > 0),
                "{target:?} is missing from a display: {counted:?}"
            );
            assert!(
                counted.iter().all(|count| *count == counted[0]),
                "{target:?} is drawn a different number of times per display: \
                 {counted:?}"
            );
            assert_eq!(
                counted.iter().sum::<usize>(),
                output
                    .hits
                    .iter()
                    .filter(|hit| hit.target == target)
                    .count(),
                "a {target:?} lies across the seam between two displays"
            );
        }
    }

    /// Each display is laid out at its own scale, against its own corner.
    ///
    /// The composition on a display is what it would have been if that display
    /// were the only one: measured against the surface instead, a 1280-wide
    /// monitor beside a 2560-wide one is handed the scale of a 3840-wide screen
    /// and gets a column that does not fit on it.
    #[test]
    fn a_display_is_composed_as_though_it_were_the_only_one() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        let make = |displays: &[Display]| {
            build(
                view(
                    &users,
                    &sessions,
                    Phase::Choose,
                    Focus::Users,
                    FOOTER.as_slice(),
                ),
                displays,
            )
        };
        let alone = make(&[Display::whole(1280.0, 1024.0)]);
        let beside = make(&[
            Display {
                rect: [0.0, 0.0, 2560.0, 1440.0],
            },
            Display {
                rect: [2560.0, 0.0, 1280.0, 1024.0],
            },
        ]);

        let submit = |output: &Output, offset: f32| {
            let rect = output
                .hits
                .iter()
                .rfind(|hit| hit.target == Target::Continue)
                .expect("the sign-in button")
                .rect;
            [rect[0] - offset, rect[1], rect[2], rect[3]]
        };
        assert_eq!(
            submit(&alone, 0.0),
            submit(&beside, 2560.0),
            "the second display's column is not the column that display would \
             have had on its own"
        );
    }

    /// The clock is the half of the design that has room to be dropped. A
    /// narrow display gives the whole width to the column instead of splitting
    /// it into two halves that are each too small to read.
    #[test]
    fn a_narrow_display_gives_the_whole_width_to_the_column_and_no_clock() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        let wide = one_display(
            view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Users,
                FOOTER.as_slice(),
            ),
            1600.0,
            900.0,
        );
        let narrow = one_display(
            view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Users,
                FOOTER.as_slice(),
            ),
            720.0,
            900.0,
        );
        let clock = crate::clock::Now::read().expect("local time").time();
        assert!(wide.scene.texts.iter().any(|text| text.content == clock));
        assert!(!narrow.scene.texts.iter().any(|text| text.content == clock));
    }

    /// Every action the administrator left on is reachable with a pointer, and
    /// the one that is not an administrator's to withdraw is always there.
    #[test]
    fn the_bottom_row_offers_exactly_what_it_was_given() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        let output = one_display(
            view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Users,
                FOOTER.as_slice(),
            ),
            1600.0,
            900.0,
        );
        for action in power::ALL {
            assert!(has(&output, Target::Power(action)));
        }
        assert!(has(&output, Target::DifferentUser));

        let restricted = [FooterItem::DifferentUser];
        let locked = one_display(
            view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Users,
                restricted.as_slice(),
            ),
            1600.0,
            900.0,
        );
        for action in power::ALL {
            assert!(!has(&locked, Target::Power(action)));
        }
        assert!(has(&locked, Target::DifferentUser));
    }

    /// Turning the machine off is not part of signing in, so it does not go
    /// away while somebody is signing in — including after a refusal, which is
    /// exactly when giving up on the login is the thing being reached for.
    #[test]
    fn the_bottom_row_survives_every_screen() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        for phase in [
            Phase::Choose,
            Phase::Authenticating {
                prompt: "Password",
                secret: true,
                input: "",
            },
            Phase::Busy("Checking"),
            Phase::Error("Denied"),
        ] {
            let output = one_display(
                view(&users, &sessions, phase, Focus::Prompt, FOOTER.as_slice()),
                1600.0,
                900.0,
            );
            assert!(
                has(&output, Target::Power(power::Action::ShutDown)),
                "{phase:?} dropped the bottom row"
            );
        }
    }

    /// A board covering the bottom of the display takes the row nearest to it
    /// rather than drawing buttons underneath itself that cannot be pressed.
    #[test]
    fn an_open_keyboard_takes_the_bottom_row_rather_than_hiding_under_it() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        let board = Board::default();
        let mut open = view(
            &users,
            &sessions,
            Phase::Authenticating {
                prompt: "Password",
                secret: true,
                input: "",
            },
            Focus::Prompt,
            FOOTER.as_slice(),
        );
        open.keyboard = Some(&board);
        open.keyboard_interactive = true;
        open.keyboard_arrival = 1.0;
        let output = one_display(open, 1280.0, 720.0);
        assert!(!has(&output, Target::Power(power::Action::ShutDown)));
        let panel_top = keyboard_panel_rect(1280.0, 720.0)[1];
        for hit in &output.hits {
            if matches!(hit.target, Target::Key(..)) {
                continue;
            }
            assert!(
                hit.rect[1] < panel_top,
                "{:?} is buried under the keyboard",
                hit.target
            );
        }
    }

    #[test]
    fn no_enumerated_users_still_offers_a_complete_route_in() {
        let sessions = [session("LineXinBar")];
        let output = one_display(
            view(
                &[],
                &sessions,
                Phase::Choose,
                Focus::Users,
                FOOTER.as_slice(),
            ),
            1600.0,
            900.0,
        );
        assert!(has(&output, Target::OtherAccount));
        assert!(has(&output, Target::DifferentUser));
        assert!(has(&output, Target::Continue));
    }

    #[test]
    fn auth_busy_and_error_always_offer_a_mouse_back_target() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        for phase in [
            Phase::Username {
                input: "",
                error: None,
            },
            Phase::Authenticating {
                prompt: "Password",
                secret: true,
                input: "",
            },
            Phase::Busy("Checking"),
            Phase::Error("Denied"),
        ] {
            let output = one_display(
                view(&users, &sessions, phase, Focus::Back, FOOTER.as_slice()),
                1600.0,
                900.0,
            );
            assert!(has(&output, Target::Back), "{phase:?} has no way back");
        }
        let choose = one_display(
            view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Users,
                FOOTER.as_slice(),
            ),
            1600.0,
            900.0,
        );
        assert!(
            !has(&choose, Target::Back),
            "there is nothing to back out of before an attempt is opened"
        );
    }

    /// The field, the sign-in button and the status all stand in one place, so
    /// a screen change is a change of content and not of furniture.
    #[test]
    fn every_screen_puts_its_centre_in_the_same_rectangle() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        let choose = hit(
            &one_display(
                view(
                    &users,
                    &sessions,
                    Phase::Choose,
                    Focus::Prompt,
                    FOOTER.as_slice(),
                ),
                1600.0,
                900.0,
            ),
            Target::Continue,
        );
        let authenticating = hit(
            &one_display(
                view(
                    &users,
                    &sessions,
                    Phase::Authenticating {
                        prompt: "Password",
                        secret: true,
                        input: "",
                    },
                    Focus::Prompt,
                    FOOTER.as_slice(),
                ),
                1600.0,
                900.0,
            ),
            Target::Prompt,
        );
        assert_eq!(choose[1], authenticating[1]);
        assert_eq!(choose[3], authenticating[3]);
    }

    #[test]
    fn a_secret_is_never_drawn_and_a_visible_answer_is_bounded() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        let secret = one_display(
            view(
                &users,
                &sessions,
                Phase::Authenticating {
                    prompt: "Password",
                    secret: true,
                    input: "hunter2",
                },
                Focus::Prompt,
                FOOTER.as_slice(),
            ),
            1600.0,
            900.0,
        );
        assert!(secret
            .scene
            .texts
            .iter()
            .all(|text| !text.content.contains("hunter2")));
        assert!(secret
            .scene
            .texts
            .iter()
            .any(|text| text.content == "•".repeat(7)));

        let long = "n".repeat(400);
        let visible = one_display(
            view(
                &users,
                &sessions,
                Phase::Username {
                    input: &long,
                    error: None,
                },
                Focus::Prompt,
                FOOTER.as_slice(),
            ),
            1600.0,
            900.0,
        );
        assert!(visible
            .scene
            .texts
            .iter()
            .any(|text| text.content.chars().count() == 32));
    }

    /// The column, the identity and the bottom row belong to the greeter and
    /// not to any one screen, so a screen change must not draw two of them
    /// sliding past each other.
    #[test]
    fn a_screen_change_moves_only_the_centre_of_the_column() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        let mut moving = view(
            &users,
            &sessions,
            Phase::Authenticating {
                prompt: "Password",
                secret: true,
                input: "",
            },
            Focus::Prompt,
            FOOTER.as_slice(),
        );
        moving.previous_phase = Some(Phase::Choose);
        moving.transition_progress = 0.5;
        let output = one_display(moving, 1600.0, 900.0);

        let shut_down = output
            .hits
            .iter()
            .filter(|hit| hit.target == Target::Power(power::Action::ShutDown))
            .count();
        assert_eq!(shut_down, 1, "the bottom row was drawn twice");
        let names = output
            .scene
            .texts
            .iter()
            .filter(|text| text.content == "Alex")
            .count();
        assert_eq!(names, 1, "the identity was drawn twice");
        // Both centres are present, which is what makes it a cross-fade.
        assert!(output
            .scene
            .texts
            .iter()
            .any(|text| text.content == "Sign in"));
        assert!(output
            .scene
            .texts
            .iter()
            .any(|text| text.content == "Password"));
    }

    /// Nothing accepts a press once the session is being handed over, or the
    /// last frame of the login could start a second one.
    #[test]
    fn a_departing_screen_offers_nothing_to_press() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        let mut leaving = view(
            &users,
            &sessions,
            Phase::Departing("Starting LineXinBar"),
            Focus::Continue,
            FOOTER.as_slice(),
        );
        leaving.departure = 0.5;
        let output = one_display(leaving, 1600.0, 900.0);
        assert!(output.hits.is_empty());
    }

    /// The wallpaper is what survives into the session, so a departing greeter
    /// has to be getting out of its way rather than dimming it with a slab.
    #[test]
    fn the_column_fades_out_completely_at_the_end_of_a_login() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        let mut leaving = view(
            &users,
            &sessions,
            Phase::Departing("Starting LineXinBar"),
            Focus::Continue,
            FOOTER.as_slice(),
        );
        leaving.departure = 1.0;
        let output = one_display(leaving, 1600.0, 900.0);
        for quad in &output.scene.quads {
            // What actually reaches the display, which is the tint's own alpha
            // times the pane's opacity — the shader's last line. A pane may
            // carry its colour at full strength and be drawn at none of it.
            assert!(
                quad.color[3] * quad.fade <= f32::EPSILON,
                "{quad:?} outlives the login"
            );
        }
        for text in &output.scene.texts {
            assert!(text.color[3] <= f32::EPSILON, "{text:?} outlives the login");
        }
    }

    /// The login screen rises *into* the wallpaper, and never out of black.
    ///
    /// The mirror of the departure above, and it has to be the same shape for
    /// the same reason. The wallpaper is on the screen before this program has
    /// a window — the compositor draws it, at the phase a scene clock kept
    /// across the hand-over — so the only thing an arrival may do is add the
    /// greeter to it. A slab laid over the picture to be faded away would drop
    /// the whole display to black and bring it back, which is a flash that is
    /// not there today and would be a worse entrance than no entrance at all.
    ///
    /// So at nothing arrived there is nothing drawn: not a dark quad, not a
    /// dimmed one. The frame is the wallpaper, exactly as the compositor left
    /// it.
    #[test]
    fn the_login_screen_rises_into_the_wallpaper_rather_than_out_of_black() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar")];
        let arriving = |arrival: f32| {
            let mut view = view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Users,
                FOOTER.as_slice(),
            );
            view.arrival = arrival;
            one_display(view, 1600.0, 900.0)
        };

        let nothing = arriving(0.0);
        for quad in &nothing.scene.quads {
            assert!(
                quad.color[3] * quad.fade <= f32::EPSILON,
                "{quad:?} is on the wallpaper before the screen has arrived"
            );
        }
        for text in &nothing.scene.texts {
            assert!(text.color[3] <= f32::EPSILON, "{text:?} arrived early");
        }

        // Halfway is halfway there rather than either end of it.
        let half = arriving(0.5);
        assert!(
            half.scene
                .quads
                .iter()
                .any(|quad| quad.color[3] * quad.fade > f32::EPSILON),
            "nothing at all is drawn halfway through the arrival"
        );
        let full = arriving(1.0);
        let brightest = |output: &Output| {
            output
                .scene
                .quads
                .iter()
                .map(|quad| quad.color[3] * quad.fade)
                .fold(0.0_f32, f32::max)
        };
        assert!(
            brightest(&half) < brightest(&full),
            "the screen is as bright halfway through its arrival as it is once here"
        );

        // And it can be used the whole way in. Somebody typing their password
        // into the first half-second of a login screen has to have every
        // character of it; only a departure takes the controls away.
        assert!(
            !half.hits.is_empty() && !nothing.hits.is_empty(),
            "a login screen that is still arriving cannot be answered"
        );
    }

    /// The session menu is cut from the column's own glass, and the shell cuts
    /// its context menu from the guide sidebar's — the same move, because it is
    /// the same kind of object: a quiet pane with things to press laid on it.
    ///
    /// What this is really guarding is the material. Reaching for the modal
    /// recipe instead — the near-opaque, wholly frosted slab under the on-screen
    /// keyboard — would give a note attached to a badge the weight of a question
    /// that takes the screen over, and would hide the one thing the panel is
    /// standing on. Every number the pane is made of is compared, not just its
    /// depth, because it is the *combination* that is the material.
    #[test]
    fn the_session_menu_is_cut_from_the_columns_glass() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar"), session("Plasma")];
        let mut open = view(
            &users,
            &sessions,
            Phase::Choose,
            Focus::Session,
            FOOTER.as_slice(),
        );
        open.session_menu = Some(Menu {
            selected: 0,
            progress: 1.0,
            interactive: true,
        });
        let output = one_display(open, 1600.0, 900.0);

        // The column is compared as it stands with no menu over it, because
        // with one over it the whole screen has stepped back and its glass has
        // gone back with it — thinner slab, smaller radius. That is the depth
        // and not the recipe, and it is the recipe under test here.
        let closed = one_display(
            view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Session,
                FOOTER.as_slice(),
            ),
            1600.0,
            900.0,
        );
        let pane = |output: &Output| {
            output
                .scene
                .quads
                .iter()
                .filter(|quad| quad.thickness > 0.0 && quad.face_curve > 0.0)
                .copied()
                .collect::<Vec<_>>()
        };
        let columns = pane(&closed);
        assert_eq!(columns.len(), 1, "one column and nothing else like it");
        let panes = pane(&output);
        assert_eq!(panes.len(), 2, "the column and the menu, and nothing else");
        let (column, menu) = (columns[0], panes[1]);
        assert_eq!(column.color, menu.color);
        assert_eq!(column.thickness, menu.thickness);
        assert_eq!(column.gloss, menu.gloss);
        assert_eq!(column.face_curve, menu.face_curve);
        assert_eq!(column.corner, menu.corner);
        assert_eq!(column.radius, menu.radius);
        // The one number they part on, and only in one direction: the menu is
        // the same glass cut deeper, never shallower, and never as deep as the
        // modal slab — which is a surface that has taken the screen over rather
        // than one standing on it.
        assert!(menu.frost > column.frost);
        assert!(menu.frost < FROST_PANEL);
    }

    /// An open menu pushes the whole screen back — and it goes back *around the
    /// badge the menu came out of*, which is the one thing that does not move.
    /// The panel grows off that badge for a fifth of a second; a badge that slid
    /// away underneath it would be a menu coming out of nothing.
    #[test]
    fn the_screen_steps_back_around_the_badge_the_menu_opens_from() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar"), session("Plasma")];
        let flat = one_display(
            view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Session,
                FOOTER.as_slice(),
            ),
            1600.0,
            900.0,
        );
        let mut back = one_display(
            view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Session,
                FOOTER.as_slice(),
            ),
            1600.0,
            900.0,
        );
        let layout = Layout::new(
            Metrics::new(1600.0, 900.0),
            &view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Session,
                FOOTER.as_slice(),
            ),
        );
        recede_into_depth(&mut back, layout.session, 1.0);

        let [ax, ay, aw, ah] = layout.session;
        let (cx, cy) = (ax + aw * 0.5, ay + ah * 0.5);
        let factor = 1.0 - MENU_DEPTH;
        assert_eq!(
            back.scene.quads.len(),
            flat.scene.quads.len(),
            "stepping back drops nothing"
        );
        let mut moved = 0;
        for (pushed, still) in back.scene.quads.iter().zip(&flat.scene.quads) {
            assert!((pushed.rect[2] - still.rect[2] * factor).abs() < 1e-3);
            assert!((pushed.rect[3] - still.rect[3] * factor).abs() < 1e-3);
            // Glass depth is a length like any other. A slab that kept its
            // thickness on a screen that moved away would be a bevel that grew
            // as the screen shrank.
            assert!((pushed.thickness - still.thickness * factor).abs() < 1e-3);
            assert!((pushed.rect[0] - (cx + (still.rect[0] - cx) * factor)).abs() < 1e-3);
            assert!((pushed.rect[1] - (cy + (still.rect[1] - cy) * factor)).abs() < 1e-3);
            if (pushed.rect[0] - still.rect[0]).abs() > 1.0 {
                moved += 1;
            }
        }
        assert!(moved > 0, "the screen should have something to push back");
        for (pushed, still) in back.scene.texts.iter().zip(&flat.scene.texts) {
            assert!((pushed.size - still.size * factor).abs() < 1e-3);
            assert!((pushed.rect[0] - (cx + (still.rect[0] - cx) * factor)).abs() < 1e-3);
        }
        // And the badge it all draws towards is really there: the control the
        // menu hangs off, which has not moved a pixel.
        let anchor = flat
            .hits
            .iter()
            .find(|hit| hit.target == Target::Session)
            .expect("the badge is a thing that can be pressed");
        assert!((anchor.rect[0] + anchor.rect[2] * 0.5 - cx).abs() < 1.0);
        assert!((anchor.rect[1] + anchor.rect[3] * 0.5 - cy).abs() < 1.0);
        let pushed = back
            .hits
            .iter()
            .find(|hit| hit.target == Target::Session)
            .expect("and it is still there once the screen has gone back");
        assert!((pushed.rect[0] + pushed.rect[2] * 0.5 - cx).abs() < 1e-3);
        assert!((pushed.rect[1] + pushed.rect[3] * 0.5 - cy).abs() < 1e-3);
    }

    /// Nothing prints through the open menu.
    ///
    /// Text is one pass after every quad, so a panel does not cover a label by
    /// being drawn over it — the label is drawn afterwards, whatever the panel
    /// is made of. No amount of frost reaches this: the session's own name,
    /// left where it is, reads straight through the rows naming sessions.
    ///
    /// What must survive is the writing the panel does *not* cover, including
    /// the run whose box dips under its edge while its words stand clear above
    /// it — the field's prompt, one line up.
    #[test]
    fn the_open_menu_takes_away_the_writing_it_covers_and_no_other() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar"), session("Plasma")];
        let mut open = view(
            &users,
            &sessions,
            Phase::Choose,
            Focus::Session,
            FOOTER.as_slice(),
        );
        open.session_menu = Some(Menu {
            selected: 0,
            progress: 1.0,
            interactive: true,
        });
        let output = one_display(open, 1600.0, 900.0);
        let metrics = Metrics::new(1600.0, 900.0);
        let layout = Layout::new(
            metrics,
            &view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Session,
                FOOTER.as_slice(),
            ),
        );
        let panel = menu_bounds(
            menu_rect(layout, metrics, sessions.len()).0,
            layout.session,
            1.0,
        );

        // Everything the column writes, judged against the same screen with the
        // menu shut: a run is the column's own if it was there before the menu
        // was raised, which is what tells it apart from the rows the menu draws
        // inside the very rectangle under test. Stepped back by hand, because
        // that is where those runs now are — and comparing rectangle for
        // rectangle is what makes this a test of the cut rather than of the
        // step.
        let mut closed = one_display(
            view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Session,
                FOOTER.as_slice(),
            ),
            1600.0,
            900.0,
        );
        recede_into_depth(&mut closed, layout.session, 1.0);
        let mut covered = 0;
        let mut clear = 0;
        for before in &closed.scene.texts {
            let under = writing_behind(before, panel);
            // Whatever became of that run: every piece it was cut into, and what
            // of each piece actually reaches the display.
            let showing = output
                .scene
                .texts
                .iter()
                .filter(|after| after.rect == before.rect && after.content == before.content)
                .filter(|after| after.color[3] > 0.0)
                .map(|after| {
                    after
                        .clip
                        .map_or(after.rect, |clip| visual::intersection(after.rect, clip))
                })
                .filter(|shown| shown[2] > 0.5 && shown[3] > 0.5)
                .collect::<Vec<_>>();
            if under {
                covered += 1;
                for shown in showing {
                    let over = visual::intersection(shown, panel);
                    assert!(
                        over[2] <= 0.5 || over[3] <= 0.5,
                        "{before:?} still prints through the panel"
                    );
                }
            } else {
                clear += 1;
                assert!(
                    !showing.is_empty(),
                    "{before:?} stands clear of the panel and was taken anyway"
                );
            }
        }
        // The badge's own label is under it, and the field's prompt one line
        // above it is not — though the prompt's box reaches under its edge.
        assert!(covered > 0, "nothing was under the panel to take away");
        assert!(clear > 0, "nothing stood clear of it to be kept");
        assert!(
            output
                .scene
                .texts
                .iter()
                .any(|text| text.content == "Sign in" && text.color[3] > 0.0),
            "the prompt above the panel was taken too"
        );
    }

    /// The menu is a panel standing over the column, so a press that is not on
    /// one of its rows is a press on the menu — dismissing it — and never on
    /// whatever it happens to be covering.
    #[test]
    fn nothing_in_the_column_can_be_pressed_through_an_open_menu() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar"), session("Plasma")];
        let mut open = view(
            &users,
            &sessions,
            Phase::Choose,
            Focus::Session,
            FOOTER.as_slice(),
        );
        open.session_menu = Some(Menu {
            selected: 0,
            progress: 1.0,
            interactive: true,
        });
        let output = one_display(open, 1600.0, 900.0);

        // Every row is answerable...
        for index in 0..sessions.len() {
            let rect = hit(&output, Target::SessionOption(index));
            assert_eq!(
                target_at(
                    &output.hits,
                    rect[0] + rect[2] * 0.5,
                    rect[1] + rect[3] * 0.5
                ),
                Some(Target::SessionOption(index))
            );
        }
        // ...and everything else on the display dismisses it, including the
        // controls the panel is standing in front of.
        let closed = one_display(
            view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Session,
                FOOTER.as_slice(),
            ),
            1600.0,
            900.0,
        );
        let covered = hit(&closed, Target::Power(power::Action::ShutDown));
        assert_eq!(
            target_at(
                &output.hits,
                covered[0] + covered[2] * 0.5,
                covered[1] + covered[3] * 0.5
            ),
            Some(Target::DismissMenu)
        );
    }

    /// It opens beside the badge it is about, never over it and never off the
    /// display: the anchor is the only context the panel has.
    #[test]
    fn the_menu_hangs_off_its_anchor_without_covering_it() {
        let users = [user("Alex")];
        let sessions = [session("LineXinBar"), session("Plasma")];
        for (width, height) in [(1600.0, 900.0), (1280.0, 720.0), (720.0, 900.0)] {
            let mut open = view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Session,
                FOOTER.as_slice(),
            );
            open.session_menu = Some(Menu {
                selected: 0,
                progress: 1.0,
                interactive: true,
            });
            let metrics = Metrics::new(width, height);
            let layout = Layout::new(metrics, &open);
            let (rect, _, _, _) = menu_rect(layout, metrics, sessions.len());
            let [ax, ay, aw, ah] = layout.session;
            assert!(
                rect[0] >= ax + aw || rect[0] + rect[2] <= ax,
                "the menu covers its own anchor at {width}x{height}"
            );
            assert!(
                rect[0] >= 0.0
                    && rect[1] >= 0.0
                    && rect[0] + rect[2] <= width
                    && rect[1] + rect[3] <= height,
                "the menu runs off a {width}x{height} display"
            );
            let _ = (ay, ah);
        }
    }

    /// A machine with more desktops than fit stops growing the panel and
    /// scrolls it instead, and the row being aimed at is always on screen.
    #[test]
    fn a_long_session_list_scrolls_rather_than_running_off_the_display() {
        let users = [user("Alex")];
        let sessions = (0..12)
            .map(|n| session(&format!("S{n}")))
            .collect::<Vec<_>>();
        let mut open = view(
            &users,
            &sessions,
            Phase::Choose,
            Focus::Session,
            FOOTER.as_slice(),
        );
        let metrics = Metrics::new(1280.0, 720.0);
        for selected in 0..sessions.len() {
            open.session_menu = Some(Menu {
                selected,
                progress: 1.0,
                interactive: true,
            });
            let layout = Layout::new(metrics, &open);
            let (rect, visible, _, _) = menu_rect(layout, metrics, sessions.len());
            assert!(visible <= sessions.len());
            assert!(rect[1] >= 0.0 && rect[1] + rect[3] <= 720.0);
            let row = menu_row_rect(layout, metrics, sessions.len(), selected, selected)
                .expect("the aimed-at row is always drawn");
            assert!(
                row[1] >= rect[1] && row[1] + row[3] <= rect[1] + rect[3],
                "row {selected} is outside its own panel"
            );
        }
    }

    /// An account with a published picture is drawn with it, and one without
    /// keeps its initial. Both, on the same screen, because a machine where one
    /// user has set an avatar and another has not is the ordinary case and the
    /// two have to sit beside each other.
    #[test]
    fn an_account_with_a_picture_is_drawn_with_it_and_one_without_keeps_its_initial() {
        let mut users = [user("Alex"), user("Sam")];
        users[1].avatar = Some(PathBuf::from("/var/lib/AccountsService/icons/sam"));
        let sessions = [session("LineXinBar")];

        for (index, has_face) in [(0, false), (1, true)] {
            let mut showing = view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Users,
                FOOTER.as_slice(),
            );
            showing.selected_user = index;
            let output = one_display(showing, 1600.0, 900.0);

            let slot = visual::face_slot(index).expect("two accounts fit in the atlas");
            let face = output
                .scene
                .quads
                .iter()
                .find(|quad| quad.slot == slot)
                .copied();
            let initial = output
                .scene
                .texts
                .iter()
                .any(|text| text.content == users[index].display_name[..1]);
            assert_eq!(face.is_some(), has_face, "account {index}");
            assert_eq!(initial, !has_face, "account {index} drew both or neither");

            let Some(face) = face else { continue };
            // A picture printed on the face of the disc, not a second pane over
            // it: no depth and no gloss is what keeps it out of the shader's
            // glass branch, and a circular corner at half its height is what
            // cuts it to the disc.
            assert_eq!(face.thickness, 0.0);
            assert_eq!(face.gloss, 0.0);
            assert_eq!(face.corner, visual::CIRCULAR_CORNER);
            assert!((face.radius - face.rect[3] * 0.5).abs() < 1e-3);
            // White, because the atlas multiplies this into the texel and a
            // portrait is the one thing here that is not the greeter's to tint.
            assert_eq!(&face.color[..3], &[1.0, 1.0, 1.0]);
            // And inside the disc, so the lit rim stays the outermost thing.
            let avatar = Layout::new(
                Metrics::new(1600.0, 900.0),
                &view(
                    &users,
                    &sessions,
                    Phase::Choose,
                    Focus::Users,
                    FOOTER.as_slice(),
                ),
            )
            .avatar;
            assert!(face.rect[0] > avatar[0] && face.rect[1] > avatar[1]);
            assert!(face.rect[2] < avatar[2] && face.rect[3] < avatar[3]);
        }
    }

    /// The route for an account nobody enumerated has no picture to have
    /// published, and must not borrow the cell of the account whose index it
    /// happens to share.
    #[test]
    fn the_unlisted_account_route_draws_no_face() {
        let mut users = [user("Alex")];
        users[0].avatar = Some(PathBuf::from("/var/lib/AccountsService/icons/alex"));
        let sessions = [session("LineXinBar")];
        let mut other = view(
            &users,
            &sessions,
            Phase::Choose,
            Focus::Users,
            FOOTER.as_slice(),
        );
        other.selected_user = users.len();
        let output = one_display(other, 1600.0, 900.0);
        assert!(!output
            .scene
            .quads
            .iter()
            .any(|quad| quad.slot >= visual::FACE_SLOT));
        assert!(output.scene.texts.iter().any(|text| text.content == "+"));
    }

    #[test]
    fn carousel_motion_slides_the_identity_without_leaving_the_column() {
        let users = [user("Alex"), user("Sam")];
        let sessions = [session("LineXinBar")];
        let mut moving = view(
            &users,
            &sessions,
            Phase::Choose,
            Focus::Users,
            FOOTER.as_slice(),
        );
        moving.carousel_shift = 0.5;
        let output = one_display(moving, 1600.0, 900.0);
        let metrics = Metrics::new(1600.0, 900.0);
        let settled = Layout::new(
            metrics,
            &view(
                &users,
                &sessions,
                Phase::Choose,
                Focus::Users,
                FOOTER.as_slice(),
            ),
        );
        // The boundary between the column's half of the display and the
        // wallpaper's, which is where the clock begins. Not the glass rect:
        // the clock is meant to be outside the glass, and only the identity
        // sliding past this line would be the carousel leaking.
        let column_width = settled.clock[0];
        for text in &output.scene.texts {
            // Anything mid-slide is half faded; nothing at full strength may
            // be outside the glass.
            if text.color[3] > 0.9 {
                assert!(
                    text.rect[0] >= 0.0 && text.rect[0] <= column_width,
                    "{text:?} slid out of the column"
                );
            }
        }
    }
}
