# Making a login one continuous picture

This is the long answer. [The README](../README.md) is the short one.
[`contrib/seamless/README.md`](../contrib/seamless/README.md) has the
measurements.

A login is two compositors and a gap between them. Three things used to break
the picture across it, and each has its own answer: the wallpaper's animation
restarting, the display going black while no process holds the DRM master, and
the login screen not knowing what colour the account signing in keeps.

## The wallpaper clock

CEDM launches LineXinBar with a public, one-shot record:

```text
LXB_BACKGROUND_HANDOFF=v=1;visual=lxb-wallpaper-v2;clock=linux-monotonic;boot=<boot-id>;sample-ns=<u64>;scene-ns=<u64>;accent=<name>
```

Both projects calculate the continuing scene clock as:

```text
scene-ns + (CLOCK_MONOTONIC_now_ns - sample-ns)
```

The record is strict, bounded, boot-specific and short-lived. A missing or
invalid record falls back to normal LineXinBar startup. The accent field is
diagnostic only: LineXinBar's own `shell.toml` remains authoritative.

CEDM derives both `sample-ns` and `scene-ns` from one raw monotonic-clock
origin. For an enumerated profile whose setting it could read, it waits for both
the 300 ms departure and that exact authored palette endpoint before sending
`StartSession`; this prevents a fast or passwordless login from handing
LineXinBar a half-blended colour. A compatible LineXinBar compositor removes the
record before creating XWayland or any other child and injects it only into
`lxb-desktop`.

**That origin is per boot, not per login screen.** It is written down under the
greeter account's own state directory
(`$XDG_STATE_HOME/console-experience-desktop-manager/wallpaper-clock`, one line
naming the boot it belongs to) and read back by every login screen after the
first, so a sign-out continues the animation the session it followed was drawing
instead of restarting it. An anchor from another boot, or one that cannot be
read, is no anchor: the animation begins again.

**None of this is a dependency.** CEDM discovers and launches any valid Wayland
session entry the same way, and a desktop that knows nothing about the record is
unaffected by it: the variable is only ever added to a session authoritatively
identified as LineXinBar, and every other session receives the plain allowlisted
environment. The continuity contract is an opt-in a cooperating desktop may
implement, not a condition of being launched.

### The visual contract

The vendored visual contract is named `lxb-wallpaper-v2`. It covers **both**
materials the shell can be set to: `Default` is the band of water and marks
beaded out of their own shape, and `Simple` is the plainer look a slow machine
asks for — the current as three fine glass-silk ribbons, and a mark as the flat
shape of itself in white with the accent breathed over it.

The shell's Theme setting is **two** keys, and this greeter reads both, because
it is both halves at once: it draws that wallpaper, and it draws the shell's own
marks in its clock, its arrows and its buttons.

```toml
theme-wallpaper = "Simple"
theme-icons = "Default"
```

They are read exactly where `accent` is read: the copy the account published on
the way into its last session, then the broker's state. So a machine set to
`Simple` is in `Simple` from the moment the login screen appears, and nothing
changes material in front of the user. A published look — or a broker — from
before the setting was split carries a single `theme` key, which said one thing
about the whole shell; it is still read, and both halves take it.

Only the **wallpaper's** half travels in the hand-over record, as its one
optional field, because the reader it is written for is a compositor drawing a
bridge frame in front of this greeter: it runs as the greeter's own account and
cannot read the settings of the person about to sign in, and what it draws is a
wallpaper and never a mark. That is also what kept the record byte-for-byte
unchanged across the split.

Any pixel-affecting shader or palette change must update both projects and bump
that identifier. The copied files carry an origin manifest in
[`vendor/linexinbar/ORIGIN.md`](../vendor/linexinbar/ORIGIN.md).

## Handing the displays over

greetd will not start the session until the greeter has exited, and the
session's compositor then needs the better part of a second to open the GPU and
set a mode. Measured on a real login here: **866 ms** — about 390 ms of greetd
and the session wrapper, about 425 ms of compositor start-up. Neither half is
worth shaving. What mattered is that it was black.

**It was black because of the kernel, not because of the gap.** Closing the last
handle on a DRM device destroys the framebuffers that file created, and removing
one that a plane is still scanning out disables the plane and the CRTC behind
it; `drm_lastclose` then puts the frame buffer console's cleared buffer on top.
A compositor blanks the display by exiting, however carefully it shut down.
Meanwhile the compositor starting up disables every connector and clears every
plane to get to a known state, which blanks it again before there is anything to
show.

Both ends are told not to, with one variable:

```text
LXB_HOLD_DISPLAY=1
```

`cedm-greeter-session` exports it for the greeter's compositor and `cedm-session`
for whatever session the user picked — it is a statement about what happens
next, not about which desktop is starting. A compositor that honours it inherits
the display configuration it was handed instead of resetting it, leaves the
colour pipeline alone on the way out, and forks a child that holds the DRM
descriptor open until the next compositor has committed a frame or ten seconds
have passed. Nothing is drawn by that child and nothing is held open beyond it.

**That last clause is a requirement, not a description of tidiness.** This
display manager runs the greeter and the session on the same VT — see the
comment on `vt = 1` in
[`packaging/files/greetd.toml`](../packaging/files/greetd.toml) — and logind
holds a terminal on behalf of whichever session has a *controller*, which is the
compositor's own connection to it. A keeper that inherited that connection along
with the descriptors would leave the outgoing session owning VT 1 for the whole
hand-over, and logind would restore the terminal at the moment that keeper
finally exited: a second late, after the incoming compositor had already
configured it for itself. What comes back is a kernel translating keys under a
running desktop, where Ctrl+C is a `SIGINT` to the session's process group
rather than a copy — the session closes — and where a VT switch no longer goes
through the compositor at all. So the keeper lets go of everything it is not
holding before it settles down to watch, and the outgoing session gives the
terminal up while greetd is still starting the next one. LineXinBar's
`handover::let_go` is where that is done and why.

The variable is LineXinBar's, documented in its `docs/configuration.md`, and any
compositor may implement it. One that does not behaves exactly as it did before
— an unknown variable in the environment, and a login that looks like every
other display manager's. This is why the greeter runs on LineXinBar's compositor
wherever it is installed: it is the same shape SDDM and GDM arrived at, where
the greeter and the session are the same compositor and the outgoing framebuffer
is still there to be replaced.

**The trade** is that a compositor handed the displays this way must set its own
colour state, because the one before it deliberately did not undo HDR or the
night light — undoing them makes the panel re-sync, which is a black screen of
the display's own making arriving exactly where one is being removed.

The third interval — the one in which no process holds the DRM master at all —
is still open, and is step 8 of the roadmap.

## What an account publishes about itself

A greeter cannot read a home directory, so an account that wants to be greeted
in its own colour has to leave a copy somewhere a greeter may look. As its
session starts, `cedm-session` runs
`console-experience-desktop-manager --publish-look`, which reads that account's
LineXinBar settings and writes the parts a login screen has a use for to
`/var/lib/console-experience-desktop-manager/published/<account>.toml`:

```toml
accent = "Red"
button-hints = true
controller-in-hand = true
sound-card = "PCH"
sound-gain = 0.027

[display.DP-1]
hdr = true
hdr-sdr-brightness = 250
```

The directory is `1733`: every account may create its own file, none may list,
remove or overwrite another's, and the greeter — which needs only to open a name
it already knows — has search access and no more.

A published file is opened under [`src/reading.rs`](../src/reading.rs), which is
three refusals rather than one. It does not follow a symbolic link, so a link
planted under somebody else's name leads nowhere. It refuses anything `fstat`
says is not a plain file — and it makes that refusal without ever waiting on
what it was pointed at, because a name in this directory can be a *named pipe*,
and opening one of those for reading waits for a writer who never comes. That
one mattered more than it looks: the greeter reads the published look of every
account it enumerates as it comes up, not just the selected one, so a single
pipe left under any name used to be a login screen that never appeared, for
everybody. And it is believed only while it belongs to the account it is named
for. What is left to a squatter is denying that account its colour, which is
where every login screen was before any of this existed; forging one was never
possible.

Everything in it is bounded and range-checked on the way in and on the way out;
a value that is not a setting is dropped rather than carried. Two settings get
more than that:

- **`enabled` is never passed on.** It is the one published setting that decides
  whether there is a login screen rather than what it looks like, and the
  greeter cannot sanity-check it — the compositor configuration is written
  before any DRM device is open, so "at least one display is left on" is not a
  question this side can answer about connectors it has not seen. An account
  that turned off every screen it has, or a file left behind by a desk that has
  since been rearranged, would otherwise be a machine whose next login screen is
  on no screen at all. The session's own compositor still honours it a second
  later, where it belongs and where whoever set it can undo it.
- **`night-light-latitude` and `night-light-longitude` are rounded** to a tenth
  of a degree on the way out — about eleven kilometres, which moves sunset by
  under a minute and is the difference between publishing a town and publishing
  a street. This file has to be world-readable, since the greeter reads it as
  nobody in particular, and these two numbers are the only thing in it that is
  about a person rather than about a desktop.

**It is current rather than one login old, and it is the shell that keeps it
that way.** Writing the copy once at sign-in would be right until the first time
somebody changed a setting: change the accent to red at lunchtime, sign out in
the evening, and the login screen would still be the purple it was that morning,
with nothing on that screen to explain why. So LineXinBar runs `--publish-look`
at the end of the same function that writes `shell.toml`, and this reads the file
that has just been written.

**Told rather than watched for**, deliberately. A watcher on this side would
have to decide when a rewrite had finished and then race the logout that may
follow it, and the one person it would get wrong is somebody who changes a
setting and signs straight out — which is exactly the person about to look at
the login screen. It also means nothing of CEDM stays resident inside anybody's
session. The shell tells it only when the half of its settings a login screen
shows has changed, so a held volume key, which rewrites that file on every step,
starts nothing.

CEDM does not require any of that to be there. A desktop that says nothing
leaves the copy as it was at sign-in, which `cedm-session` still writes — never
staler than the session's own beginning — and an account that has never run the
shell has nothing to publish at all.

### The greeter's own cache

For each visible local account, CEDM reads the top-level `accent` value out of
the copy that account published, bounded to 256 KiB and restricted to
LineXinBar's canonical twelve palette names. It smoothly previews that user's
whole palette when selection moves.

It used to read `.config/lxb/shell.toml` first "where it can", on the grounds
that the settings themselves are fresher than any copy of them. They are, and it
was still the wrong place to look. On an ordinary machine a home is `0700` and
the read simply failed; on a machine where it succeeds — a development box, a
home an administrator has opened up — it is an account deciding what the login
screen opens, waits on and allocates for, before anybody has signed in. GDM has
never read a home directory and neither does this, now in the code as well as in
this document. Nothing is lost by it: LineXinBar publishes as it saves, so the
copy is not the stale one of the pair.

The future privileged seat broker may supply cached accent data in
`/var/lib/console-experience-desktop-manager/state.toml`:

```toml
last_user = "alex"

[accents]
alex = "Blue"
```

## What the greeter cannot apply itself

Two of those settings are not a client's to apply. A mode, an output's place in
a layout, whether a screen is lit at all and whether a connector is driven in
high dynamic range are all settled by whoever holds the DRM master, before there
is a surface to draw on — so a Wayland client cannot ask for any of them. That
is the first reason the greeter runs on a compositor it can configure rather
than on a kiosk compositor it cannot: under one of those the login screen is SDR
at whatever mode each display offered first, whatever the account's settings
say.

What CEDM does instead is hand its own compositor the same settings.
`--compositor-config PATH` writes the last signed-in account's published look as
a LineXinBar compositor configuration — connector by connector, with the shell's
own inheritance already resolved — and `cedm-greeter-session` starts that
compositor with it.

### The night light

Worked out here rather than passed on, because a compositor has no clock and no
time zone: it is given a plain on-or-off, and something with a clock has to
decide.

- `all-day` and `hours` are the machine's local time against the switch and the
  two hours the account kept.
- `sunset-to-sunrise` is the sun where the machine is, worked out from the same
  place the shell works it out from — `/etc/localtime` says which zone it is in,
  `zone1970.tab` says where that zone is, and the NOAA equations say what the
  sun does there today.

Nobody has to publish any of it: it is system data, world-readable, and as
available to a greeter as to a session. A settings file that names coordinates
by hand outranks the table, on both sides, because the shell reads that key too.

It is the shell's own arithmetic, transcribed, and it has to stay that way. A
login screen that disagreed with the session about whether it is night would
warm a display and then let the shell cool it a second later, in front of
somebody who had asked for neither. So the fallbacks are the shell's as well,
down to which way each of them errs: hours that meet are no window rather than a
whole day, a machine that cannot say where it is burns rather than staying cold,
and a day at a latitude where the sun does not come up is a day that is night.

## Sound

The greeter cannot work out which speakers, or how loud, for itself. It runs as
an account of its own with no sound server: no default sink, no stored volume,
and an ALSA `default` that is a plugin waiting to connect to a server that is
not there. Left to guess it guesses badly. A machine with several cards in it —
a graphics card's HDMI sockets, an onboard codec, whatever is plugged into USB —
offers a long list of outputs that all open successfully, and only one of them
is the one somebody is listening to. Opening a display socket with no cable in
it looks exactly like success.

So it is not guessed. `--publish-look` asks the session's own sound server
through `pactl --format=json` and writes two more keys:

- **`sound-card`** is ALSA's own id for the card the default sink is on, which
  is the `CARD=` in the device names the greeter chooses between. Not the sink's
  name: that one belongs to PipeWire, which is precisely what will not be
  running.
- **`sound-gain`** is a plain multiplier taken from the sink's decibels. The
  session's volume is applied by the sound server in software, so a greeter that
  opened the card directly and played at full scale would answer a button many
  times louder than the desktop either side of it. A muted machine publishes
  `0.0` and gets a silent login screen, which is what muting it meant.

Given a published card the greeter uses **that card**, choosing among its
devices by the rules below; without one it falls back to them across every card.
Either way it prefers a card's own default and its plain stereo output over a
raw device or one particular multichannel arrangement, ignores ALSA's plugin
entries entirely (`jack`, `oss`, `pulse`, `pipewire`, the rate converters —
those are ways of *reaching* an output, and several open happily where nothing
is listening), and for a socket on a graphics card requires a live ELD in
`/proc/asound/CARD/eld#codec.pin`, so a port with no cable in it is never
chosen. What it settled on is in the journal beside what it was asked for:

```text
INFO cedm::sound: login screen audio ready device="<card>, <output>" wanted="<published card>" gain=0.027
```

### Who publishes it, and when

Three moments, because none of this is written to a file anywhere: which device
the machine plays through and how loud belong to the sound server, and a desktop
that kept its own copy would be a second opinion about them at every login.

- **`cedm-session`, once the sound server answers.** This is the one that works
  on *any* desktop, and it is why the greeter's sound does not depend on which
  one is installed. The wrapper cannot ask before it starts the session — there
  is no sound server yet — so it leaves a short-lived helper behind that waits
  for one, publishes, and exits. Bounded: it gives up quietly rather than
  becoming a process resident in somebody's session.
- **LineXinBar, when the output device is changed** in Settings ▸ Sounds.
  Nothing is written to disk on that path, so the shell says so directly rather
  than the copy falling out of a file being saved.
- **LineXinBar, on its way out** of Exit or Shut down, which is the moment that
  catches the volume — nothing marks the moment a volume changes, and holding a
  volume key is a hundred changes. That publish waits for its child, unlike
  every other one: a session that exits takes its children with it.

The last two are refinements and neither is required. A machine running Plasma,
or a LineXinBar too old to know about any of this, is covered by the first.

The publish that happens *before* the session starts cannot answer any of it, so
it carries the last known answer forward rather than erasing it. Everything else
in a published look is the whole state every time; this is the one exception,
and without it every login would wipe the one fact the login screen cannot
recover.
