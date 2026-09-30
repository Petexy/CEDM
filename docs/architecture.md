# How it is put together

This is the long answer. [The README](../README.md) is the short one.

## The layers

Each is started by the one above it:

```text
cedm.service                       systemd; the display-manager alias
  └── greetd                       root; PAM, seats, session ownership
        └── cedm-greeter-session   the unprivileged cedm-greeter account
              └── lxb              the seat-owning compositor
                    └── cedm       UI, input, PAM conversation only
```

**CEDM stands on its own.** `lxb` above is `lxb-compositor` — LineXinBar's
compositor packaged apart from that project's shell, precisely so a machine with
a login screen does not thereby have a desktop it never asked for. Nothing else
in that column comes from a desktop either, and installing `lxb-desktop` is
entirely the user's choice.

The greeter itself is unprivileged and holds nothing but the interface, the
input and its half of the PAM conversation:

```text
greetd (root; PAM and session ownership)
  └── CEDM greeter (unprivileged; UI, input, PAM conversation only)
        ├── exact vendored LineXinBar visual subset
        ├── local user and freedesktop session discovery
        └── selected session argv + a small allowlisted environment
```

## The greetd conversation

The client uses greetd's four-byte native-endian length-prefixed JSON protocol.
Requests and responses are bounded before allocation or use. Password input and
serialized request frames are held in zeroizing buffers, and secrets are never
logged. Authentication attempts carry monotonically changing IDs and immutable
user, session and accent context, so a queued prompt cannot revive an old screen
and a successful launch cannot be attributed to a later highlight. Back also
interrupts the active Unix socket; short I/O polling provides a cancellation
fallback where `shutdown(2)` is unavailable.

**A refused password ends the attempt here and does not end it at greetd.** The
daemon configures one session at a time and holds it until a `cancel_session`
arrives; it does not let go of it because a greeter went quiet or went away. A
refusal that is not cancelled is therefore not the end of one attempt but of
every attempt, because each later `create_session` is refused by the session
still sitting there — a machine that answers one mistyped password by never
accepting another.

So the conversation is cancelled where it ends, and **greetd is told before the
interface is**: the interface being told is exactly what lets a *Try again*
pressed on the same frame interrupt the connection mid-cancellation and leave
behind the session the cancellation was for. Because the state in question is
the daemon's rather than this process's, that is only half of it. A login that
*finds* a session already under configuration — left by a greeter killed at its
prompt, or by an older build of this one — clears it and opens its own, rather
than reporting to the person in front of it that the machine is already busy
signing somebody in. A session already *scheduled* is left alone: that one is
somebody's successful login on its way out of this greeter.

### What a refusal says

What a refusal says on screen is the greeter's sentence rather than PAM's.
greetd separates `auth_error` — "not a fatal error, and is likely caused by
incorrect credentials" — from every other failure, and only the other kind is
worth quoting. `authentication error: AUTH_ERR` under a *Try again* button reads
as a broken greeter rather than as a mistyped password, and sends somebody
looking for a fault in the machine instead of typing again; it is also the one
failure they can already explain to themselves.

A socket that would not open or a session that would not start is the opposite
case, and keeps greetd's own words, which are the only description of it anybody
has.

PAM does not say which half of a sign-in was wrong, and neither does this: on
the route where the account name was typed as well, naming the password as the
wrong half would be a guess, and a guess that quietly answers "does this account
exist" for anyone who cares to ask.

## Launching a session

Desktop-entry `Exec` values become direct argument vectors. **They are never
passed to `/bin/sh`.** LineXinBar is detected from an authoritative
`DesktopNames=LineXinBar` or an `lxb-session` executable; only that selected
session gets the handoff variable — see [the handover](handover.md).

The parser recognizes both Wayland and X11 entries, but the greeter currently
offers only Wayland. An X11 session also needs the display manager to provision
an X server and an authentication cookie; advertising it before that wrapper
exists would create a login choice that cannot work.

### Which session is offered first

Stored values are only IDs. CEDM tries each layer against freshly discovered,
validated sessions and never treats one as a command:

1. A valid per-user choice, remembered after a successful launch.
2. The last global choice.
3. The administrator's first-run default from `/etc/cedm/config.toml`.
4. LineXinBar where it is installed, then Plasma, then the first remaining
   validated session.

A stale layer falls through to the next. An administrator can disable either
category of remembering; unknown keys or invalid syntax are rejected and fail
closed with history disabled.

After greetd has accepted a launch, the greeter atomically stores a visible
local account name and desktop-file ID in
`$XDG_STATE_HOME/console-experience-desktop-manager/preferences.toml`. Its
directory is mode `0700`, its file `0600`, reads are bounded, and a corrupt or
unknown-version document is ignored. **Preview and failed authentication runs
never write preferences.**

A name typed through *Different user* is sent only to the live greetd
conversation: it is not displayed after submission and never becomes a saved
profile, a `last_user`, or a per-user preference key. Only its non-identifying
global session choice may be remembered. That privacy boundary also means CEDM
cannot pre-read a hidden or directory account's palette without leaking account
existence, so that route uses the fallback palette and LineXinBar may correct
the colour on its first frame.

The unprivileged greeter never writes `/var/lib`.

It also turns off its own core dumps and its own ptrace-ability as it starts —
`RLIMIT_CORE` at nothing, `PR_SET_DUMPABLE` at zero — because for the length of
one login it is a process with a password in it, and by default any process
running as the same account may read another's memory. It is not alone under
`cedm-greeter`: the compositor that gives it a seat runs there too, and so does
the session bus beside it. Neither limit reaches the session that follows, which
greetd starts itself, as root, after this process has exited.

This is also why CEDM's service unit is **not** sandboxed and must not be. That
unit is greetd, not the greeter, and greetd forks every user session on the
machine out of itself: `NoNewPrivileges=yes` or `PrivateTmp=yes` there would be
a restriction on somebody's whole desktop rather than on their login screen. A
limit meant for the thirty seconds somebody spends typing must not become a
limit on the eight hours they spend working. GDM's unit contains none of them
either. The confinement belongs one level down, on the process that is actually
the login screen, and that is where it is.

## What a login screen can know about an account

**Nothing it has to walk into a home directory for.** Homes are `0700` on most
distributions and `0710` on some, and `cedm-greeter` is in nobody's group, so an
account's own settings are usually unreadable anyway — but the boundary is that
the greeter does not open them, not that it would fail if it tried. A home an
administrator has opened up, or a development machine, must not be a machine
where an account decides what the login screen reads. [`src/faces.rs`](../src/faces.rs)
has always said so about avatars: the greeter reads the copy `accounts-daemon`
published under `/var/lib/AccountsService/icons` and never `~/.face`. As of the
security pass this is true of every other setting too — the accent, both halves
of the theme, the keyboard, the displays, the sound device — each of which reads
the published copy and nothing else. GDM draws the same line.

**And it opens what it does read through one door.**
[`src/reading.rs`](../src/reading.rs) is the only way the greeter opens a file
somebody else can write, and it refuses three things: a symbolic link, anything
that is not a plain file, and a plain file the expected account does not own. It
refuses the second of those *without waiting*, which is the part that is not
obvious — a name can be a named pipe, and opening one for reading blocks until
a writer arrives. Nothing on this side of a login may be able to wait forever.

**What it decodes is bounded before it allocates.** An avatar is a PNG from
outside this program, and a PNG's header names its own size: fifty-seven bytes
can claim 32768 by 32768, which is a four-gigabyte allocation asked for before
there is any image data to contradict it. The picture's dimensions and its
decoded size are checked against a fixed budget first, and the decoder is given
a budget of its own.

The accent had no such copy, and the result was a login screen that drew every
account in the default purple however its shell was set. So each account
publishes one for itself, and [the handover](handover.md#what-an-account-publishes-about-itself)
is where that is written down.

Nothing here requires LineXinBar to be installed. An account that has never run
it publishes nothing, and a login screen with nothing published is the login
screen this project always had.

## Machine actions

The bottom row asks the machine to sleep, restart or shut down. **The greeter
performs none of them**: it runs `systemctl` as a fixed argument vector with one
of three compile-time verbs, and logind and polkit decide whether that is
allowed. There are two independent gates and a request has to pass both —
[`contrib/config.toml.example`](../contrib/config.toml.example) decides whether
the button exists, polkit decides whether pressing it does anything, and a
refusal is reported rather than swallowed. Choosing one abandons any
conversation in progress first, so a machine going down is never left holding an
open PAM attempt.

The fourth item is the route for an account the greeter cannot enumerate. It is
not an administrator's to withdraw: on a machine whose accounts live in a
directory it is the only way in.

## Left alone

A handheld switched on and put down spends its night on the login screen if
nobody signs in, so the login screen dims, goes dark and asks the machine to
sleep on the same waits the session keeps — [`src/idle.rs`](../src/idle.rs).
They are LineXinBar's Settings > Power, which is **the machine's** setting and
not an account's, and LineXinBar keeps it where a greeter can read it:
`/etc/lxb/power.toml`, believed only when root owns it and read through the same
door as everything else here. A machine with no file has LineXinBar's defaults:
dim after two minutes, dark after five, sleep after a quarter of an hour on the
battery and an hour plugged in. Whether the machine is on its battery is read
out of `/sys/class/power_supply`, leaving out a mouse's or a pad's own.

Anything this screen hears counts as somebody being there — a key, the pointer,
a button on a controller — and the press that lights a dark screen does only
that. Waking from sleep starts every wait over: the boot clock, which counts
sleep, is compared with the monotonic one, which does not. Sleep is asked of
`systemctl --no-ask-password` like the bottom row's, so whatever the system
holds, logind refuses and the login screen asks again a minute later. A login
screen inside another desktop (LineXinBar's compositor says which backend it
draws through), a preview and a windowed run never ask.

**How the displays are dimmed and switched off** is
[`src/display_power.rs`](../src/display_power.rs): the greeter is its
compositor's shell, and LineXinBar's compositor offers its shell
`lxb_shell_v1`, whose `set_output_power` draws the dimming sheet or fades a
display to black and switches it off at the connector — the session's own
sheet and fades. The XML is LineXinBar's, copied into `protocols/`. While the
screens are dark nothing is drawn and the loop wakes ten times a second, which
took the login screen from about 5% of a processor core to about 0.1% on the
desk this was written at. Under any other compositor the global is not there
and the screen stays lit, as it always did.

**The power button** is logind's: LineXinBar writes the machine's answer as
`HandlePowerKey` (`/etc/systemd/logind.conf.d/60-lxb-power.conf`), so a press at
the login screen sleeps, hibernates or turns the machine off as the session's
would. The one answer logind has no word for, **Power menu**, the login screen
gives itself: the compositor sends the button to its shell as
`lxb_shell_v1.power_button`, and the focus moves to the bottom row's power
buttons.

## Low-end hardware mode

LineXinBar's Settings > System > Low-end hardware mode is for a machine that
cannot keep up with the water, and the login screen is the first thing such a
machine draws. It is on here where the last account to sign in published it
(`low-end-mode` in its look) and, where it chose nothing, whenever this screen
is drawn on the processor rather than a graphics chip. It holds the wallpaper
on one moment (LineXinBar's own, so the session that follows stands on the
same picture), draws the plain material without sparkles, and draws a frame
once a second while nothing moves. While something moves it draws each frame
as the display shows the last one, which is the display's own refresh; a
device that misses more than a fifth of them is drawn at every other refresh
instead, on the display's beat, and is given the full rate back once it has
shown for long enough that it can. That pace is `src/cadence.rs`, LineXinBar's
own (see `vendor/linexinbar/ORIGIN.md`): each frame asks, with
`pre_present_notify`, to be told when it has been shown, and the redraw asked
for as it goes out arrives with that answer. The lit control's pulse runs on
the wallpaper's clock, so it holds still too. On llvmpipe that took a still
login screen from about seven processor cores to a quarter of one.

## The udev rule

The udev rule the package installs is the only thing any of this grants. The
second-generation Steam Controller has no kernel gamepad driver, so CEDM reads
its report from hidraw — and hidraw nodes are `0600 root:root`, with nothing in
systemd's own uaccess rules tagging them. The rule tags Valve's devices
`uaccess`, which hands them to whoever holds the *active session on the seat*:
the greeter while the greeter is up, the user once they have signed in, and at
no point every account on the machine.

That is also why `cedm-greeter` is **not** in the `input` group and must not be
put in it — that group is read access to every evdev node on the machine, which
is a keylogger's worth of privilege for a program that takes its input through
Wayland.

## Input contract

- **Controller.** D-pad and left stick move; South and Start accept; East backs
  out in one press; North opens or hides the keyboard; the shoulders switch
  sessions. Polling, stick hysteresis and repeat timings match LineXinBar.
  Standard and Steam-controller axes are merged fresh every poll, so a centred
  or disconnected device cannot leave navigation stuck.
- **Mouse.** Every user, session selector and arrow, Back, Continue, Retry, the
  prompt, the closed-board keyboard affordance and every on-screen key has an
  explicit hit target. Hover and controller navigation update the same focus
  state.
- **Keyboard.** Arrows and Tab move forward, Shift+Tab reverses, Enter accepts,
  Escape cancels, and physical typing edits the greeter-owned bounded PAM buffer
  directly. Typing hides a visible on-screen keyboard without dropping the first
  key.
- **The session menu and the on-screen keyboard each take every direction while
  they are up.** Nothing underneath moves behind them.
- **Navigation is a grid built per frame from what the screen actually draws**,
  rather than a table of transitions written out per screen, so focus can never
  land on a control this phase does not have. Rows move with up and down keeping
  the column where they can; left and right move within a row, except on the
  identity row, where they are the profile carousel.

**Typing on the profile screen is the password starting.** A printable key opens
the conversation exactly as pressing *Sign in* does, and the character goes
where it was always going. Nothing else on that screen takes typing, so there is
nothing else it could have meant. What is typed before greetd has asked for
anything is held and handed to the first prompt of that same attempt rather than
dropped — a login screen that quietly eats the first few letters of every
password typed at it is one whose users find that out the hard way, one password
at a time, with nothing on the screen to say why the password was wrong. A chord
— Ctrl, Alt or Super held — is not typing and does not start anything.

## When the on-screen keyboard comes up by itself

**Only where there is nothing else to type on**, so a controller alone is
sufficient without the board getting in the way of a desk. A board that covers
half the screen to offer a worse copy of the keys already under the user's
hands, at the exact moment they were about to start typing, is worse than no
board at all. Its dedicated close key hides only the board; the persistent Back
action cancels authentication.

What counts as a keyboard is decided once at start from
`/proc/bus/input/devices`, which is world-readable and needs no device opened
and no privilege the greeter would otherwise want — the greeter takes its own
input through Wayland and never touches evdev, so this is a question about the
machine rather than about the seat.

**A Steam Controller is a controller, by name, before anything is measured.**
Lizard mode makes the pad enumerate a full keyboard so it works in a program
that has never heard of one, and four of them appear with a base station
attached. None of that makes the machine a machine with a keyboard on it — it
makes it the console this greeter was written for, and letting the pad withhold
the on-screen board takes the board away from exactly the room it exists for. It
is a named exception rather than something the tests below happen to get right,
because those are heuristics and this is a fact: Valve makes controllers, not
keyboards.

For everything else, three tests, each of which something ordinary fails while
passing the others:

- **udev's own `ID_INPUT_KEYBOARD` range**, `KEY_ESC` through `KEY_D`.
- **Lock lamps**, because Num, Caps and Scroll are a physical keyboard's own
  lights and one synthesised by a remote control or a power button has none.
- **No relative or absolute axes**, because a gaming mouse carries the whole key
  bitmap *and* lighting, and only its movement gives it away.

If the file cannot be read the answer is "no keyboard", so the board is offered
where it might not have been needed rather than withheld from someone with no
other way to sign in.

None of that binds the button beside the field, which always raises the board:
the detection is a heuristic about hardware and a press is a statement about what
the user wants, and where they disagree the user is right. The button carries
the picture of a keyboard *rising*, not the one of it folding away — it used to
wear the close glyph, which told the user the board was already up and they were
about to dismiss it. A picture that reads instantly is worse than a word when it
reads instantly wrong.

## The gamma module

[`src/gamma.rs`](../src/gamma.rs) is a second way to warm the login screen, from
the client side through `zwlr_gamma_control_v1`. **It is not the one that runs**:
LineXinBar's compositor deliberately implements no such protocol, so under the
packaged configuration that module finds nothing and stands down, and the warmth
comes from the compositor configuration instead — see
[the handover](handover.md#what-the-greeter-cannot-apply-itself). It survives
for a greeter started by hand under some other wlroots compositor, and for no
other case.
