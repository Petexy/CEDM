# Console Experience Desktop Manager

A controller-first graphical greeter for LineXinBar and ordinary Linux desktop sessions.

This repository is an early, runnable vertical slice. It already renders LineXinBar's exact analytic wallpaper, palette and Liquid Glass material; presents local and directory-user routes in a compact XMB-style carousel; discovers and directly launches installed Wayland session entries without invoking a shell; conducts PAM conversations through greetd; supports controller, mouse and physical-keyboard navigation; embeds the same ANSI on-screen keyboard geometry; draws a whole login screen on every display rather than one across all of them; remembers successful public account/session choices; speaks nine languages, taken from whichever file this distribution keeps the machine's locale in; and hands the wallpaper clock to LineXinBar after authentication.

## The screen

One glass column on the left of the wallpaper, and the time on the right of it.

```text
┌───────────────────────────┬────────────────────────────┐
│  (avatar)  Good evening   │                            │
│            alex           │           20:38            │
│            • ○            │                            │
│                           │           Mon 17           │
│            [ Password  ][→]│                           │
│                           │                            │
│  (session) Plasma  ▸menu  │      analytic wallpaper    │
│                           │                            │
│  ⏸    ⟳    ⏻    →👤       │                            │
│ Sleep Restart Shut  Diff.  │                            │
└───────────────────────────┴────────────────────────────┘
```

The column is the whole interface: who is signing in, what into, the answer PAM is waiting for, and what the machine can be asked to do instead. Everything on every screen stands in the same place — the field, the sign-in button and a failure's "Try again" are one rectangle with different contents — so a screen change is a change of content rather than a rearrangement. Only that middle band cross-fades; the column, the identity and the bottom row belong to the greeter, not to any one screen, and do not move when PAM asks another question.

The wallpaper behind it is full-bleed and untouched, which is what keeps the handover seamless: at the end of a login the column and the clock fade out and the last frame the greeter draws is a wallpaper frame the session's compositor then carries on drawing at the same phase. See below.

The column is cut from LineXinBar's guide sidebar rather than from its modal-panel recipe: a shallower, clearer slab with its own light under it — a broad glow at the head, a quieter one at the foot, and a hairline rim — so the wallpaper's current stays visible through the glass while the controls on top stand proud as the frostier objects. It floats clear of the display's edges for the reason the shell gives: glass only reads as a layer when what it is laid over runs past it. Those numbers are vendored from `lxb-desktop`'s `sidebar_surface`; if they change there they have to change here, or the login screen and the first shell frame are two different materials.

The marks on it are LineXinBar's, in both senses. Ten of the thirteen are the shell's own files: the four arrow caps, the two controller hints and the keyboard's close key are byte-identical, and the power symbol, the restart cycle and the session badge's screen are its `shutdown.svg`, `refresh.svg` and `setting-display.svg` unchanged from `<svg` on, under names that say what they do here. The greeter and the shell it hands over to must not put two different power symbols in front of the same user four seconds apart.

The three it has to draw for itself — suspend, the route to an unlisted account, and the button that raises the on-screen board — are drawn in the same material: moulded white plastic under one lamp above it, gloss laid on as its own shape so its lower edge can follow the object's swell, a rim lit at the top and again along the bottom by the bounce, and a shadow underneath so the thing has somewhere to be. That material is the shell's answer to why its power symbol stopped being two strokes: everything around it is a moulded object under one lamp, and a flat mark among them reads as the one control that has not finished loading. Key caps and controller hints stay line art, in both projects, because a cap on a key is not an object on a shelf.

`lxb-desktop`'s own guard over that set is ported with them: every glyph must rasterise, cover between 5% and 60% of its cell, keep its channels within one of each other — the atlas multiplies the quad's colour into the texel, so a glyph with a hue of its own could only come out muddier than the label beside it — and never go darker than mid-grey, because a shadow on a white shell is a dimmer white and one dark enough to read as a hole has stopped being shading. No two may rasterise alike, which is what stops four arrow caps pointing the wrong way three times out of four.

Each account is shown with its own picture where the system has published one. `accounts-daemon` keeps a copy of every user's chosen avatar at `/var/lib/AccountsService/icons/<name>`, world-readable on purpose: that directory exists so a login screen — which runs as nobody in particular — can show a face without being handed a way into anybody's home. GDM reads it and so does every other display manager that shows one. It is not a privilege the greeter has; it is a copy the system published, and the greeter reads it exactly as it is entitled to.

Which is why nothing goes near `~/.face`. That is the *source* the daemon copied from, it sits inside a home directory the greeter has no business in — unreadable anyway on a good many distributions — and reaching for it would cross precisely the boundary this project draws around per-user configuration everywhere else. An avatar that never reached AccountsService is an account with no picture here, and the initial stands in for it, as it does for an account that never set one.

The picture is centre-cropped to a square and box-filtered down into a cell of the atlas, once, as the window is made: cropped rather than squeezed because the shape it is going into is a circle and portraits are framed on the middle; box-filtered because this only ever shrinks, and taking the nearest source pixel instead is how an avatar comes out looking photographed through a screen door. Only PNG is decoded — the daemon does not transcode what it copies, so an avatar set from a JPEG stays a JPEG, and that account keeps its initial rather than the greeter growing a second image decoder to run over a file before anyone has logged in. Cells are bounded at seventeen accounts and the read at 8 MB, because everything on this side of a login is.

The profile carousel is still a carousel — left and right move between accounts — but it shows one profile at a time, sliding, because the column cannot clip what it holds. The dots under the avatar count *enumerated accounts*, so a machine with one account has no dots: the route for an account the greeter cannot enumerate is a button on the bottom row, not a page of the ring, and counting it would tell a single-user machine it has two profiles.

Choosing a session opens a menu in the same language the shell's own context menus use: beside the badge it is about rather than over it, grown out of it, dimming what it stands in front of, with the session already chosen keeping its mark whichever row the cursor is on. It owns every direction while it is up, exactly as the on-screen keyboard does, and a press anywhere outside it is an answer of "not this". The shoulder buttons still step straight between sessions without opening anything.

Its panel is cut from the column's own glass — the same four quads, the same depth, gloss and face curve — because the shell cuts its context menus from the guide sidebar for the same reason: a menu is a quiet pane with things to press laid on it, not a question that takes the screen over. The modal recipe, which is the near-opaque slab the on-screen keyboard rests under, would have hidden the one thing this panel is standing on. The one number it parts on is the frost, which is cut deeper: the shell's reason for keeping the sidebar clear is a reason about size — a surface two thirds of the display tall, frosted hard, turns almost all of its face into one flat field — and a pane of three rows has no face to lose and a job the column has not got, which is to be read against the column itself, a few pixels behind it and in the same violet.

While it is up the whole screen steps back from the viewer — the column, the clock, the wallpaper's furniture, all of it scaled down about the badge the panel came out of, which is the one thing that does not move. Shrinking towards the middle of the display instead would slide that badge out from under the very panel growing off it, which reads as the column sliding rather than as the column receding. Glass depth is a length like any other and goes back with everything else: a slab left at full thickness on a screen that had moved away would be a bevel that grew as the screen shrank. It rides the panel's own arrival, so the screen goes back over exactly the span the panel comes forward in and returns on the way out, and it is a distance the screen *holds* rather than a flourish — a push that only reads while it is moving has stopped saying anything by the time the user is reading the list. That, with the dimming, is the difference between a screen that has been turned down and one that has been stepped back from.

Frost alone cannot make it readable, though, because text is one pass after every quad: a label is not covered by a panel drawn over it, it is drawn *afterwards*, whatever the panel is made of. So the panel takes the writing it covers away — one run in, up to three out, the pieces either side of it kept where they were and the piece under it faded out over the panel's arrival. That is the shell's `cut_text_behind`, and it is what stops the session's own name reading straight through the rows naming sessions. The cut is measured against the line rather than against the box a run was laid out in, so a label standing plainly above the panel is not taken away because the empty bottom of its box dips under an edge.

Glass is handed the greeter's own drawing as its backdrop, and nothing else. The wallpaper is rendered into its own target and the interface into a second one over nothing, so a pane that looks through itself finds the panes it is resting on and, wherever there are none, an emptiness — which the shader answers by *evaluating* the wallpaper at whatever softness the pane's frost asks for, rather than by blurring a picture of it. That distinction is the whole visible difference: the wallpaper is an analytic function of smooth, wide gradients, and blurring a picture of a smooth gradient returns the gradient. Asking the function for its softened self is what LineXinBar's two Wayland surfaces do, and it is why the column reads as frosted glass rather than as a tinted rectangle. The two targets meet once, in a compositing pass; the text is drawn onto the joined frame after it, where its own blending is correct.

## Every display is a display

A machine with two monitors on it has two screens, not one wide one, and the greeter draws a whole login screen on each: its own wallpaper, its own column at its own scale, its own clock, its own board. Either screen can be signed in on, and both are the same conversation — there is one PAM exchange behind them, so a profile chosen with the pointer on the right-hand monitor is the profile shown on the left one.

That has to be arranged for, because it is not what the greeter is handed. CEDM is a Wayland client with a single surface, and the compositor that gives it that surface extends it across the whole output layout: one buffer as wide as every monitor put together, with the seam between two of them somewhere in the middle. Laid out as one screen — which is what a client that never asks does — that is a wallpaper stretched over two panels of different shapes, a column pinned to the outer edge of the left-hand one, a clock in the gap between them, and every length in the interface scaled to a screen nobody owns.

So the surface is cut back into the displays it was made of and each one is composed on its own, in its own pixels, from its own corner. The compositor's layout is believed only when it accounts for the surface exactly — every output a rectangle of its own, none overlapping another, their bounding box the size of the surface. An output the layout names more than once is one output: winit reports every screen twice, and counted twice a screen stands on itself, which is a mirrored pair by every rule that looks only at geometry and was answered by drawing one login screen across the whole desk. A nested development window on a desktop, a mirrored pair, an output whose mode has been announced but not applied: each of those is answered with the whole surface as one display, which is what the greeter did before it could count displays and is never wrong on screen, only wide. A strip of surface no monitor is behind — the space under a shorter screen standing beside a taller one — is drawn on by nobody and stays black, because that is what is in front of the user there.

The wallpaper in particular has to be per display, and not only because a stretched gradient looks stretched. LineXinBar gives every output its own layer surface and evaluates the wallpaper against that output's own size, so a greeter that evaluated one across all of them would be handing over a picture the shell is about to replace with a different one on every screen. The seamless frame is only seamless per display. Glass is asked the same question the same way: a pane carries the display it stands on, and where it finds nothing drawn behind it, the wallpaper it falls through to is that display's. It is also cut to that display's bounds — the light under an avatar is drawn wider than the avatar, and on a single screen the edge of the framebuffer takes care of the overhang, while on a row of monitors what is out there is the next screen along.

It is not ready to replace a production display manager yet. In particular, the native seat-owning compositor/broker, NSS/AccountsService enumeration beyond the existing local parser, the accessibility pass, and real-VT integration tests are still work in progress. (Localisation is done — see [What it says, and in which language](#what-it-says-and-in-which-language).)

## Try the safe preview

Preview mode never contacts greetd and never starts a session:

```sh
cargo run --release -- --windowed --preview
```

Start the safe preview directly on a secret prompt to inspect the built-in keyboard. This one forces the board up whatever is plugged in, because reviewing it is the point of the flag:

```sh
cargo run --release -- --windowed --preview --preview-auth
```

Look at a translation without changing what the machine is set to. It takes a locale name or a language tag, and one this greeter is not written in is ignored rather than fatal:

```sh
cargo run --release -- --windowed --preview --language pl
cargo run --release -- --preview --preview-auth --shot /tmp/cedm-de.png --language de_DE.UTF-8
```

List the sessions CEDM would offer:

```sh
cargo run -- --list-sessions
```

Write one composed frame out as a PNG, for reviewing what the greeter draws without giving it a seat to draw on. It implies `--preview`, so it can never open a PAM conversation:

```sh
cargo run --release -- --shot /tmp/cedm.png
cargo run --release -- --preview --preview-menu --shot /tmp/cedm-menu.png
```

Review the multi-display composition on a machine with one monitor. A developer's window is nested inside a desktop and has no output layout of its own to be cut up, so the displays are given on the command line instead and replace what the compositor says; each is `WIDTHxHEIGHT+X+Y` in physical pixels, and the window is asked to be big enough to hold them:

```sh
cargo run --release -- --preview --size 2400x800 \
    --displays 1280x800+0+0,1120x700+1280+0 --shot /tmp/cedm-two-screens.png
```

The real greeter expects `GREETD_SOCK`, exactly as a greetd default-session client receives it. This milestone is a Wayland client, so it also needs a seat-owning compositor — `lxb-compositor`, which the package depends on; [`contrib/greetd/config.toml.example`](contrib/greetd/config.toml.example) is a reference configuration. Do not replace an active display manager until the complete VT/seat handoff has been tested on that machine.

## Installing it as the display manager

Build a package for the machine and install it:

```sh
./packaging/build.sh arch      # or: debian | fedora | nix
```

Packages build under `packaging/out/build/` rather than in `/tmp`, which on most machines is a tmpfs: this dependency graph needs about 1 GiB to compile and roughly 4.5 GiB more for the second, dev-profile build that the package's test phase runs, and a tmpfs that size is a build that ends in `No space left on device`. `--work-dir DIR` sends it somewhere else.

Installing it is the whole of becoming a display manager. The unit carries `Alias=display-manager.service`, and every package enables it on a first install — that is what display managers do, and one that installs without being enabled has done nothing at all.

It will not take the login screen away from something else. Only one unit can hold that alias, so where another display manager already has it the install says so and leaves it alone:

```text
cedm: gdm.service is already this machine's display manager, so
cedm: cedm.service has been left disabled. To switch to it:
cedm:     systemctl disable gdm.service
cedm:     systemctl enable cedm.service
```

Nothing is ever started by the install, because starting a display manager takes the seat whoever ran it is signed in on; the change lands at the next boot. An upgrade never re-enables the unit either, since an administrator who disabled it meant it. And a machine that does not boot to `graphical.target` is told so, because that target is what pulls `display-manager.service` in.

Underneath it, the layers are the ones the architecture below describes, each started by the one above it:

```text
cedm.service                   systemd; the display-manager alias
  └── greetd                   root; PAM, seats, session ownership
        └── cedm-greeter-session   the unprivileged cedm-greeter account
              └── lxb          the seat-owning compositor
                    └── cedm   UI, input, PAM conversation only
```

CEDM stands on its own. `lxb` above is `lxb-compositor`, a dependency of this package — LineXinBar's compositor packaged apart from that project's shell, precisely so a machine with a login screen does not thereby have a desktop it never asked for. Nothing else in that column comes from a desktop either, and installing `lxb-desktop` is entirely the user's choice.

A package installs the greeter, that unit, greetd's CEDM configuration at `/etc/cedm/greetd.toml`, the `cedm-greeter` account and the two directories it needs, and one udev rule. It does **not** install an administrator policy file: CEDM runs correctly without one and its defaults are the permissive ones, so [`contrib/config.toml.example`](contrib/config.toml.example) is installed as documentation and a machine that wants a policy writes exactly the policy it wants at `/etc/cedm/config.toml`.

The udev rule is the only thing any of this grants. The second-generation Steam Controller has no kernel gamepad driver, so CEDM reads its report from hidraw — and hidraw nodes are `0600 root:root`, with nothing in systemd's own uaccess rules tagging them. The rule tags Valve's devices `uaccess`, which hands them to whoever holds the *active session on the seat*: the greeter while the greeter is up, the user once they have signed in, and at no point every account on the machine. That is also why `cedm-greeter` is not in the `input` group and must not be put in it — that group is read access to every evdev node on the machine, which is a keylogger's worth of privilege for a program that takes its input through Wayland.

Everything the packaging decides, and why, is in [`packaging/README.md`](packaging/README.md). `./packaging/build.sh check` validates the payload without building a package, including the failures that would otherwise only appear at boot: a unit that no longer aliases `display-manager.service`, or an account that `sysusers.d`, `tmpfiles.d` and `greetd.toml` have stopped agreeing on.

Install and test from a spare VT, or on a disposable machine, before this becomes the only way into one you need. The gap it has not closed yet is the one with no DRM master in it; [`contrib/seamless/README.md`](contrib/seamless/README.md) says what that looks like and what reduces it.

## Architecture now

```text
greetd (root; PAM and session ownership)
  └── CEDM greeter (unprivileged; UI, input, PAM conversation only)
        ├── exact vendored LineXinBar visual subset
        ├── local user and freedesktop session discovery
        └── selected session argv + a small allowlisted environment
```

The client uses greetd's four-byte native-endian length-prefixed JSON protocol. Requests and responses are bounded before allocation/use. Password input and serialized request frames are held in zeroizing buffers, and secrets are never logged. Authentication attempts carry monotonically changing IDs and immutable user/session/accent context, so a queued prompt cannot revive an old screen and a successful launch cannot be attributed to a later highlight. Back also interrupts the active Unix socket; short I/O polling provides a cancellation fallback if `shutdown(2)` is unavailable.

A refused password ends the attempt here and does not end it at greetd. The daemon configures one session at a time and holds it until a `cancel_session` arrives; it does not let go of it because a greeter went quiet or went away. A refusal that is not cancelled is therefore not the end of one attempt but of every attempt, because each later `create_session` is refused by the session still sitting there — a machine that answers one mistyped password by never accepting another. So the conversation is cancelled where it ends, and greetd is told before the interface is: the interface being told is exactly what lets a "Try again" pressed on the same frame interrupt the connection mid-cancellation and leave behind the session the cancellation was for. Because the state in question is the daemon's rather than this process's, that is only half of it. A login that *finds* a session already under configuration — left by a greeter killed at its prompt, or by an older build of this one — clears it and opens its own, rather than reporting to the person in front of it that the machine is already busy signing somebody in. A session already *scheduled* is left alone: that one is somebody's successful login on its way out of this greeter.

What a refusal says on screen is the greeter's sentence rather than PAM's. greetd separates `auth_error` — "not a fatal error, and is likely caused by incorrect credentials" — from every other failure, and only the other kind is worth quoting. `authentication error: AUTH_ERR` under a "Try again" button reads as a broken greeter rather than as a mistyped password, and sends somebody looking for a fault in the machine instead of typing again; it is also the one failure they can already explain to themselves. A socket that would not open or a session that would not start is the opposite case, and keeps greetd's own words, which are the only description of it anybody has. PAM does not say which half of a sign-in was wrong, and neither does this: on the route where the account name was typed as well, naming the password as the wrong half would be a guess, and a guess that quietly answers "does this account exist" for anyone who cares to ask.

Desktop-entry `Exec` values become direct argument vectors. They are never passed to `/bin/sh`. LineXinBar is detected from authoritative `DesktopNames=LineXinBar` or an `lxb-session` executable; only that selected session gets the handoff variable.

The parser recognizes both Wayland and X11 entries, but the greeter currently offers only Wayland. An X11 session also needs the display manager to provision an X server and authentication cookie; advertising it before that wrapper exists would create a login choice that cannot work.

## LineXinBar background handoff

CEDM launches LineXinBar with a public, one-shot record:

```text
LXB_BACKGROUND_HANDOFF=v=1;visual=lxb-wallpaper-v1;clock=linux-monotonic;boot=<boot-id>;sample-ns=<u64>;scene-ns=<u64>;accent=<name>
```

Both projects calculate the continuing scene clock as:

```text
scene-ns + (CLOCK_MONOTONIC_now_ns - sample-ns)
```

The record is strict, bounded, boot-specific and short-lived. A missing or invalid record falls back to normal LineXinBar startup. The accent field is diagnostic only: LineXinBar's own `shell.toml` remains authoritative.

CEDM derives both `sample-ns` and `scene-ns` from one raw monotonic-clock origin. For an enumerated profile whose setting it could read, it waits for both the 300 ms departure and that exact authored palette endpoint before sending `StartSession`; this prevents a fast or passwordless login from handing LineXinBar a half-blended colour. A compatible LineXinBar compositor removes the record before creating XWayland or any other child and injects it only into `lxb-desktop`.

That origin is per boot, not per login screen. It is written down under the greeter account's own state directory (`$XDG_STATE_HOME/console-experience-desktop-manager/wallpaper-clock`, one line naming the boot it belongs to) and read back by every login screen after the first, so a sign-out continues the animation the session it followed was drawing instead of restarting it. An anchor from another boot, or one that cannot be read, is no anchor: the animation begins again.

This preserves animation phase on LineXinBar's first wallpaper frame. Two intervals that used to be black on the session's side of the handover have since been removed there, both without changing this record: LineXinBar now starts its shell beside XWayland instead of behind it, and its compositor draws the same analytic wallpaper at the handed-over phase from its own first frame — and goes on drawing it, frame by frame, for as long as it is the only thing on screen, so the seconds before the shell has pixels are the continuing wallpaper rather than a clear colour or a still of one.

The third interval — the one in which no process holds the DRM master at all, between the greeter's compositor exiting and the session's compositor setting a mode — is covered by "Handing the displays over" below. [`contrib/seamless/README.md`](contrib/seamless/README.md) accounts for all three boundaries and shows the measurements.

None of this is a dependency. CEDM discovers and launches any valid Wayland session entry the same way, and a desktop that knows nothing about the record is unaffected by it: the variable is only ever added to a session authoritatively identified as LineXinBar, and every other session receives the plain allowlisted environment. The continuity contract is an opt-in a cooperating desktop may implement, not a condition of being launched.

The vendored visual contract is named `lxb-wallpaper-v1`. Any pixel-affecting shader/palette change must update both projects and bump that identifier. The copied files carry an origin manifest in [`vendor/line-xinbar/ORIGIN.md`](vendor/line-xinbar/ORIGIN.md).

## Handing the displays over

A login is two compositors and a gap between them: greetd will not start the session until the greeter has exited, and the session's compositor then needs the better part of a second to open the GPU and set a mode. Measured on a real login here, **866 ms** — about 390 ms of greetd and the session wrapper, about 425 ms of compositor start-up. Neither half is worth shaving. What mattered is that it was black.

It was black because of the kernel, not because of the gap. Closing the last handle on a DRM device destroys the framebuffers that file created, and removing one that a plane is still scanning out disables the plane and the CRTC behind it; `drm_lastclose` then puts the frame buffer console's cleared buffer on top. A compositor blanks the display by exiting, however carefully it shut down. Meanwhile the compositor starting up disables every connector and clears every plane to get to a known state, which blanks it again before there is anything to show.

Both ends are told not to, with one variable:

```text
LXB_HOLD_DISPLAY=1
```

`cedm-greeter-session` exports it for the greeter's compositor and `cedm-session` for whatever session the user picked — it is a statement about what happens next, not about which desktop is starting. A compositor that honours it inherits the display configuration it was handed instead of resetting it, leaves the colour pipeline alone on the way out, and forks a child that holds the DRM descriptor open until the next compositor has committed a frame or ten seconds have passed. Nothing is drawn by that child and nothing is held open beyond it.

That last clause is a requirement, not a description of tidiness. This display manager runs the greeter and the session on the same VT — see the comment on `vt = 1` in [`packaging/files/greetd.toml`](packaging/files/greetd.toml) — and logind holds a terminal on behalf of whichever session has a *controller*, which is the compositor's own connection to it. A keeper that inherited that connection along with the descriptors would leave the outgoing session owning VT 1 for the whole hand-over, and logind would restore the terminal at the moment that keeper finally exited: a second late, after the incoming compositor had already configured it for itself. What comes back is a kernel translating keys under a running desktop, where Ctrl+C is a `SIGINT` to the session's process group rather than a copy — the session closes — and where a VT switch no longer goes through the compositor at all. So the keeper lets go of everything it is not holding before it settles down to watch, and the outgoing session gives the terminal up while greetd is still starting the next one. LineXinBar's `handover::let_go` is where that is done and why.

The variable is LineXinBar's, documented in its `docs/configuration.md`, and any compositor may implement it. One that does not behaves exactly as it did before — an unknown variable in the environment, and a login that looks like every other display manager's. This is why the greeter runs on LineXinBar's compositor wherever it is installed: it is the same shape SDDM and GDM arrived at, where the greeter and the session are the same compositor and the outgoing framebuffer is still there to be replaced.

The trade is that a compositor handed the displays this way must set its own colour state, because the one before it deliberately did not undo HDR or the night light — undoing them makes the panel re-sync, which is a black screen of the display's own making arriving exactly where one is being removed.

## What a login screen can know about an account

Nothing it has to walk into a home directory for. Homes are `0700` on most distributions and `0710` on some, and `cedm-greeter` is in nobody's group, so an account's own settings are simply unreadable — which is the boundary, not an obstacle to it. [`src/faces.rs`](src/faces.rs) has always said so about avatars: the greeter reads the copy `accounts-daemon` published under `/var/lib/AccountsService/icons` and never `~/.face`.

The accent had no such copy, and the result was a login screen that drew every account in the default purple however its shell was set. So each account publishes one for itself. As its session starts, `cedm-session` runs `console-experience-desktop-manager --publish-look`, which reads that account's LineXinBar settings and writes the parts a login screen has a use for to `/var/lib/console-experience-desktop-manager/published/<account>.toml`:

```toml
accent = "Red"

[display.DP-1]
hdr = true
hdr-sdr-brightness = 250
```

The directory is `1733`: every account may create its own file, none may list, remove or overwrite another's, and the greeter — which needs only to open a name it already knows — has search access and no more. A published file is opened without following symlinks and is believed only while it belongs to the account it is named for, so squatting on a name that has never been published can deny that account its colour and can never forge one. Everything in it is bounded and range-checked on the way in and on the way out; a value that is not a setting is dropped rather than carried.

It is current rather than one login old, and it is the shell that keeps it that way. Writing the copy once at sign-in would be right until the first time somebody changed a setting: change the accent to red at lunchtime, sign out in the evening, and the login screen would still be the purple it was that morning, with nothing on that screen to explain why. So LineXinBar runs `--publish-look` at the end of the same function that writes `shell.toml`, and this reads the file that has just been written.

Told rather than watched for, deliberately. A watcher on this side would have to decide when a rewrite had finished and then race the logout that may follow it, and the one person it would get wrong is somebody who changes a setting and signs straight out — which is exactly the person about to look at the login screen. It also means nothing of CEDM stays resident inside anybody's session. The shell tells it only when the half of its settings a login screen shows has changed, so a held volume key, which rewrites that file on every step, starts nothing.

CEDM does not require any of that to be there. A desktop that says nothing leaves the copy as it was at sign-in, which `cedm-session` still writes — never staler than the session's own beginning — and an account that has never run the shell has nothing to publish at all.

Nothing here requires LineXinBar to be installed. An account that has never run it publishes nothing, and a login screen with nothing published is the login screen this project always had.

### What the greeter cannot do with it

Two of those settings are not a client's to apply. A mode, an output's place in a layout, whether a screen is lit at all and whether a connector is driven in high dynamic range are all settled by whoever holds the DRM master, before there is a surface to draw on — so a Wayland client cannot ask for any of them. That is the first reason the greeter runs on a compositor it can configure rather than on a kiosk compositor it cannot: under one of those the login screen is SDR at whatever mode each display offered first, whatever the account's settings say.

What CEDM does instead is hand its own compositor the same settings. `--compositor-config PATH` writes the last signed-in account's published look as a LineXinBar compositor configuration — connector by connector, with the shell's own inheritance already resolved — and `cedm-greeter-session` starts that compositor with it.

The night light is worked out here rather than passed on, because a compositor has no clock and no time zone: it is given a plain on-or-off, and something with a clock has to decide. `all-day` and `hours` are the machine's local time against the switch and the two hours the account kept. `sunset-to-sunrise` is the sun where the machine is, worked out from the same place the shell works it out from — `/etc/localtime` says which zone it is in, `zone1970.tab` says where that zone is, and the NOAA equations say what the sun does there today. Nobody has to publish any of it: it is system data, world-readable, and as available to a greeter as to a session. A settings file that names coordinates by hand outranks the table, on both sides, because the shell reads that key too.

It is the shell's own arithmetic, transcribed, and it has to stay that way. A login screen that disagreed with the session about whether it is night would warm a display and then let the shell cool it a second later, in front of somebody who had asked for neither. So the fallbacks are the shell's as well, down to which way each of them errs: hours that meet are no window rather than a whole day, a machine that cannot say where it is burns rather than staying cold, and a day at a latitude where the sun does not come up is a day that is night.

The second reason, and the one that settled it, is the hand-over. Closing the last handle on a DRM device is itself what blanks a display — the kernel destroys that file's framebuffers and disables the plane still scanning one out — so whichever compositor the greeter ran on, the screen went black for the whole of greetd opening a session and the session's compositor setting a mode. Measured here: 866 ms. Removing that needs the outgoing compositor to hold the descriptor open and the incoming one to inherit the configuration rather than reset it, which is something a compositor has to implement. See [Handing the displays over](#handing-the-displays-over).

The trade that used to argue for a kiosk compositor is real and is now the smaller one. An ordinary compositor fullscreens a client on one output, so on a machine with two monitors the login column is drawn on one of them and the other shows the wallpaper. That is worth having over a black screen at every login on both, and on a single-display machine — a handheld, a console, a television — there is no trade at all. What removes it everywhere is the greeter opening a surface per output instead of one, which is the next piece of work in [Every display is a display](#every-display-is-a-display).

[`src/gamma.rs`](src/gamma.rs) is a second way to warm the login screen, from the client side through `zwlr_gamma_control_v1`. It is not the one that runs: LineXinBar's compositor deliberately implements no such protocol, so under the packaged configuration that module finds nothing and stands down, and the warmth comes from the compositor configuration above. It survives for a greeter started by hand under some other wlroots compositor, and for no other case.

## Accent and successful-session preferences

For each visible local account, CEDM reads the same top-level `accent` value from `.config/lxb/shell.toml` when it can, then the copy that account published, bounded to 256 KiB and restricted to LineXinBar's canonical five palette names. It smoothly previews that user's whole palette when selection moves.

The future privileged seat broker may supply cached accent data in `/var/lib/console-experience-desktop-manager/state.toml`:

```toml
last_user = "alex"

[accents]
alex = "Blue"
```

The unprivileged greeter never writes `/var/lib`. After greetd has accepted a session launch, it atomically stores a visible local account name and desktop-file ID in `$XDG_STATE_HOME/console-experience-desktop-manager/preferences.toml` (or the corresponding path below `$HOME`). Its directory is mode `0700`, its file is mode `0600`, reads are bounded, and a corrupt or unknown-version document is ignored. Preview and failed authentication runs never write preferences. A name typed through “Other account” is sent only to the live greetd conversation: it is not displayed after submission and never becomes a saved profile, `last_user`, or per-user preference key. Only its non-identifying global session choice may be remembered. That privacy boundary also means CEDM cannot pre-read a hidden/directory account's palette without leaking account existence; this route uses the fallback palette, and LineXinBar may correct the colour on its first frame if the authenticated account chose another one. Post-authentication accent metadata from the future trusted broker is required to remove that remaining snap safely.

An administrator can set a first-run desktop-file ID and disable either category of remembering in `/etc/cedm/config.toml`; see [`contrib/config.toml.example`](contrib/config.toml.example). Unknown keys or invalid syntax are rejected and fail closed with history disabled. Stored values are only IDs: CEDM tries each layer against freshly discovered, validated sessions and never treats one as a command. A valid per-user choice wins over the last global choice, then the administrator default; a stale layer falls through to the next. With no valid preference, LineXinBar is preferred when installed, followed by Plasma, then the first remaining validated session.

## Machine actions

The bottom row asks the machine to sleep, restart or shut down. The greeter performs none of them: it runs `systemctl` as a fixed argument vector with one of three compile-time verbs, and `logind` and polkit decide whether that is allowed. There are two independent gates and a request has to pass both — [`contrib/config.toml.example`](contrib/config.toml.example) decides whether the button exists, polkit decides whether pressing it does anything, and a refusal is reported rather than swallowed. Choosing one abandons any conversation in progress first, so a machine going down is never left holding an open PAM attempt.

The fourth item is the route for an account the greeter cannot enumerate. It is not an administrator's to withdraw: on a machine whose accounts live in a directory it is the only way in.

## Input contract

- Controller: D-pad/left stick moves; South/Start accepts; East backs out in one press; North opens or hides the keyboard; shoulder buttons switch sessions. Polling, stick hysteresis and repeat timings match LineXinBar. Standard and Steam-controller axes are merged fresh every poll so a centred or disconnected device cannot leave navigation stuck.
- The session menu and the on-screen keyboard each take every direction while they are up. Nothing underneath moves behind them.
- Navigation is a grid built per frame from what the screen actually draws, rather than a table of transitions written out per screen, so focus can never land on a control this phase does not have. Rows move with up/down keeping the column where they can; left/right moves within a row, except on the identity row, where it is the profile carousel.
- Mouse: every user, session selector/arrow, Back, Continue, Retry, prompt, closed-board keyboard affordance and on-screen key has an explicit hit target. Hover and controller navigation update the same focus state.
- Keyboard: arrows/Tab move forward, Shift+Tab reverses, Enter accepts, Escape cancels, and physical typing edits the greeter-owned bounded PAM buffer directly. Typing hides a visible on-screen keyboard without dropping the first key.
- **Typing on the profile screen is the password starting.** A printable key opens the conversation exactly as pressing "Sign in" does, and the character goes where it was always going. Nothing else on that screen takes typing, so there is nothing else it could have meant. What is typed before greetd has asked for anything is held and handed to the first prompt of that same attempt rather than dropped — a login screen that quietly eats the first few letters of every password typed at it is one whose users find that out the hard way, one password at a time, with nothing on the screen to say why the password was wrong. A key press is also the answer to the question the board detection below is a heuristic about, so nothing rises over a field that is already being answered, and a chord — Ctrl, Alt or Super held — is not typing and does not start anything.

Secret prompts open the built-in on-screen keyboard by themselves **only where there is nothing else to type on**, so a controller alone is sufficient without the board getting in the way of a desk. A board that covers half the screen to offer a worse copy of the keys already under the user's hands, at the exact moment they were about to start typing, is worse than no board at all. Its dedicated close key hides only the board; the persistent Back action cancels authentication. There is no redundant external “Hide keyboard” button.

What counts as a keyboard is decided once at start from `/proc/bus/input/devices`, which is world-readable and needs no device opened and no privilege the greeter would otherwise want — the greeter takes its own input through Wayland and never touches evdev, so this is a question about the machine rather than about the seat.

**A Steam Controller is a controller, by name, before anything is measured.** Lizard mode makes the pad enumerate a full keyboard so it works in a program that has never heard of one, and four of them appear with a base station attached. None of that makes the machine a machine with a keyboard on it — it makes it the console this greeter was written for, and letting the pad withhold the on-screen board takes the board away from exactly the room it exists for. It is a named exception rather than something the tests below happen to get right, because those are heuristics and this is a fact: Valve makes controllers, not keyboards. A firmware update that gives the puck lock lamps changes what the heuristics answer and changes nothing here. The rest of the greeter already works this way — `steam_hid` reads the pad over hidraw precisely because its keyboard is a duplicate to be dropped rather than an input to be believed.

For everything else, three tests, each of which something ordinary fails while passing the others: udev's own `ID_INPUT_KEYBOARD` range, `KEY_ESC` through `KEY_D`; **lock lamps**, because Num, Caps and Scroll are a physical keyboard's own lights and one synthesised by a remote control or a power button has none; and **no relative or absolute axes**, because a gaming mouse carries the whole key bitmap *and* lighting, and only its movement gives it away. If the file cannot be read the answer is "no keyboard", so the board is offered where it might not have been needed rather than withheld from someone with no other way to sign in.

None of that binds the button beside the field, which always raises the board: the detection is a heuristic about hardware and a press is a statement about what the user wants, and where they disagree the user is right. The button carries the picture of a keyboard *rising*, not the one of it folding away — it used to wear the close glyph, which told the user the board was already up and they were about to dismiss it. A picture that reads instantly is worse than a word when it reads instantly wrong.

Profile changes slide beneath a stationary selection light and can be retargeted while moving. Screen changes keep the outgoing and incoming compositions together for a 280 ms eased handoff instead of replacing one whole screen in a single frame; the OSK keeps its separate solid rise/lower motion.

The login screen itself **rises into view over 600 ms** from its first frame, on a smoothstep — symmetrical, so it leaves nothing and settles at the same rate, and the halfway point of the time is the halfway point of the picture. A linear fade arrives by stopping, which reads as a cut however long it is given.

What rises is everything the greeter draws, and **nothing of the wallpaper**. That is the only shape this can take. The wallpaper is on the screen before this program has a window — the compositor draws it, at the phase a scene clock kept across the hand-over — so a rise that began from black would have to lay black over a picture that is already there, and the display would drop to black and come back rather than arrive. What was missing was never the picture; it was everything in front of it. Measured across the rise, the wallpaper moves by at most one 8-bit level, which is rounding.

It is longer than every other movement here because it is the only one that is not an answer to something the user did — nobody is waiting on it. For the same reason it dims the picture and never the controls: a screen that is still arriving is a screen that works, and somebody who starts typing their password into the first half-second of it has every character. Only a departure takes the controls away. `--shot` captures a fully arrived frame, since a picture of a login screen a fraction into its own entrance is a picture of nothing much.

## What a button sounds like

Four recordings, and they are LineXinBar's own — copied into `assets/sounds/`, recorded in [`vendor/line-xinbar/ORIGIN.md`](vendor/line-xinbar/ORIGIN.md), and shipped in the binary for the same reason the fonts and the glyphs are: a login screen runs before any desktop does, and there may be no theme of sounds on the machine to borrow one from. Signing in and using the shell that follows are meant to be one instrument.

| When | Clip |
| --- | --- |
| the highlight arrives somewhere new | `press-guide.ogg` |
| a press this screen acts on | `press-selected.ogg` |
| a key of the on-screen keyboard going down | `keyboard-click.ogg` |
| a password refused | `error.ogg` |

Three rules decide the rest of it.

**Only a button.** The pad and the keys that drive the column make these noises; a pointer makes none of them. That is what the sounds are for — a click is the half of the acknowledgement that reaches somebody looking at the pad in their hands rather than at the screen — and a mouse is a control the user is watching the whole time, with the highlight following it under their hand. A swept pointer would otherwise be a stream of clicks answering a question nobody asked. It is structural rather than remembered: every one of them is spent around `apply_action`, and `click`/`hover` do not reach the sound module at all.

**Only what happened.** A direction that moved nothing is silent — the end of a row that will not wrap, a carousel holding one profile — and so is a press on a control that did nothing. A click for either would stop meaning "you moved" and start meaning "the button is not broken". A press that goes *backwards* is not one of those: leaving a prompt and putting the board away are things somebody asked for and got.

**The refusal is the exception**, and it is why it is here at all. It answers no press — it arrives from PAM, whole seconds after the answer went away, very often at somebody who typed a password they know by heart and then looked somewhere else. Only PAM's refusal: a failure of the login service puts the same red screen up, but nothing the user types will fix that one, and answering both the same way would teach the noise to mean nothing. Physical typing into the field is silent too; `keyboard-click.ogg` belongs to the on-screen board, which is its own instrument.

An administrator turns all of it off with `sound = false` in `/etc/cedm/config.toml`, for a ward or a shared office or a room somebody sleeps in; an unreadable policy file silences it along with everything else there. `--no-sound` is the same switch for somebody running the preview on their own desktop. The greeter reaches the sound card the way it reaches every other device on its seat — logind's ACL, not the `audio` group — and one whose seat was never granted it logs `no audio output` once and goes on in silence, which is a working login screen.

### Which speakers, and how loud

The same ones as the desktop, at the same level, and the greeter cannot work either out for itself. It runs as an account of its own with no sound server: no default sink, no stored volume, and an ALSA `default` that is a plugin waiting to connect to a server that is not there. Left to guess it guesses badly. A machine with several cards in it — a graphics card's HDMI sockets, an onboard codec, whatever is plugged into USB — offers a long list of outputs that all open successfully, and only one of them is the one somebody is listening to. Opening a display socket with no cable in it looks exactly like success.

So it is not guessed. The account publishes it, exactly as it publishes the accent and the display settings: `cedm --publish-look` asks the session's own sound server through `pactl --format=json` and writes two more keys into `/var/lib/console-experience-desktop-manager/published/NAME.toml`.

- `sound-card` is ALSA's own id for the card the default sink is on, which is the `CARD=` in the device names the greeter chooses between. Not the sink's name: that one belongs to PipeWire, which is precisely what will not be running.
- `sound-gain` is a plain multiplier taken from the sink's decibels. The session's volume is applied by the sound server in software, so a greeter that opened the card directly and played at full scale would answer a button many times louder than the desktop either side of it. A muted machine publishes `0.0` and gets a silent login screen, which is what muting it meant.

Given a published card the greeter uses **that card**, choosing among its devices by the rules below; without one it falls back to them across every card. Either way it prefers a card's own default and its plain stereo output over a raw device or one particular multichannel arrangement, ignores ALSA's plugin entries entirely (`jack`, `oss`, `pulse`, `pipewire`, the rate converters — those are ways of *reaching* an output, and several open happily where nothing is listening), and for a socket on a graphics card requires a live ELD in `/proc/asound/CARD/eld#codec.pin`, so a port with no cable in it is never chosen. What it settled on is in the journal beside what it was asked for:

```
INFO cedm::sound: login screen audio ready device="<card>, <output>" wanted="<published card>" gain=0.027
```

### Who publishes it, and when

Three moments, because none of this is written to a file anywhere: which device the machine plays through and how loud belong to the sound server, and a desktop that kept its own copy would be a second opinion about them at every login.

- **`cedm-session`, once the sound server answers.** This is the one that works on *any* desktop, and it is why the greeter's sound does not depend on which one is installed. The wrapper cannot ask before it starts the session — there is no sound server yet — so it leaves a short-lived helper behind that waits for one, publishes, and exits. Bounded: it gives up quietly rather than becoming a process resident in somebody's session.
- **LineXinBar, when the output device is changed** in Settings > Sounds. Nothing is written to disk on that path, so the shell says so directly rather than the copy falling out of a file being saved.
- **LineXinBar, on its way out** of Exit or Shut down, which is the moment that catches the volume — nothing marks the moment a volume changes, and holding a volume key is a hundred changes. That publish waits for its child, unlike every other one: a session that exits takes its children with it.

The last two are refinements and neither is required. A machine running Plasma, or a LineXinBar too old to know about any of this, is covered by the first.

The publish that happens *before* the session starts cannot answer any of it, so it carries the last known answer forward rather than erasing it. Everything else in a published look is the whole state every time; this is the one exception, and without it every login would wipe the one fact the login screen cannot recover.

## What it says, and in which language

Nine: English, French, German, Hindi, Polish, Portuguese (Brazil), Russian, Spanish and Chinese (Simplified). Every word the greeter writes is in `src/i18n.rs`, once per language, compiled into the binary — there is no catalogue under `/usr/share/locale` to be missing, and no message looked up by a name that could fail to match. A language is a struct, so one that forgot a sentence does not build.

### Where the language comes from

From the machine, because nobody has signed in yet to have a preference. That is one question with a different answer on nearly every distribution, so it is asked in order and the first answer wins:

1. `--language`, for reviewing a translation by hand.
2. `language` in `/etc/cedm/config.toml`, which is the administrator saying so outright — for the machine whose login screen should *not* be in the machine's language, such as a shared terminal in a building where the desks were set up in one language and the people signing in read another.
3. `LC_ALL`, `LC_MESSAGES`, `LANG` in the greeter's own environment, in POSIX's order of precedence.
4. The machine's locale file, in this order, first value found: `/etc/locale.conf` (systemd — Arch, Fedora, openSUSE), `/etc/default/locale` (Debian, Ubuntu), `/etc/sysconfig/i18n` (older Red Hat and SUSE), `/etc/env.d/02locale` and `/etc/conf.d/locale` (Gentoo), `/etc/environment` (PAM's environment file, which some installers put `LANG` into as well). All six are `KEY=value` lines, which is why one parser serves them; they are read, never run.
5. English.

The first file that *answers* wins rather than the first that exists, because a machine can carry two of them: an empty `/etc/locale.conf` beside a populated `/etc/default/locale` is an ordinary Debian, and stopping at the empty one would read the machine as having no language at all.

An environment that says `C` or `POSIX` is treated as **nothing said** rather than as a choice of English. That is what those names mean — no locale has been selected — and a greeter started by a system unit inherits them routinely; reading them as an answer is how a login screen ends up ignoring the very file the machine's language is written in. It is verifiable from the journal, which names both the language and where the answer came from:

```text
the login screen speaks the language this machine is set to
    language=pl name=polski locale=pl_PL.UTF-8 from=/etc/default/locale
```

### What is not translated, and why

Two kinds of writing, both for the same reason: they are somebody else's words.

- **A session's name** is the desktop's own, and comes from that desktop entry's own `Name[xx]` keys — nothing here could know that GNOME's Polish entry says "GNOME na Xorgu". The country-qualified spelling is preferred over the bare language, so `Name[pt_BR]` beats `Name[pt]`, and an entry with no translation for the reader's language falls back to its unqualified `Name`.
- **A failure the login service reports.** A socket that would not open or a session that would not start is greetd's news about this machine, and its sentence is the only description of it anybody has; replacing it with a translated generality would throw the news away in order to say something in the right language. The greeter's own refusals — a wrong password, an action polkit declined — are translated, because those are the greeter's to write.

PAM sits between the two. It localises its own conversation, but only where the process holding it has a language to localise into, and that process is greetd — a system unit whose environment is whatever the unit gives it. What comes back over the socket on nearly every machine is therefore `Password:`, in the middle of a column that is otherwise entirely in Polish, so that one question and the two or three beside it are translated here. Anything else PAM asks is passed through exactly as it arrived: an unrecognised prompt is a module with something specific to say — a hardware token, a one-time code, an expiring password — and a greeter that guessed at those would be answering a question nobody asked.

### The keyboard, the clock and the fonts

The on-screen board's character keys are not translated and will not be. It is a picture of an ANSI keyboard and the letters printed on it are the letters it types; a board whose caps said one thing and typed another would be worse in every language. What the machine's language does decide is the handful of caps that are *words*, and the rule there is that a legend stays Latin wherever that language's own keyboards carry Latin legends — a Russian keyboard has `Shift` written on it, so the board does too, while French, German, Spanish and Portuguese boards say `Maj`, `Umschalt`, `Mayús` and `Shift`.

The clock's second line is a pattern rather than a weekday with a number after it, because the languages disagree about the order and about whether the number is marked: `Mon 17`, `Mo 17.`, `пн 17`, `17日 周一`.

Roboto carries Latin, Latin Extended, Greek and Cyrillic — eight of the nine — and no Devanagari and no Han at all, so two Noto faces are bundled beside it in `assets/fonts/` and compiled into the binary with everything else. Without them the Hindi and Chinese columns rasterise to rows of empty boxes, and nothing else in the build would say a word about it. Devanagari is subset to the whole script, so an account named in it draws too; the Han face is subset to the characters this program's own words are made of, because the whole of Noto Sans CJK is twenty megabytes. An account or a session named in Han falls back to whatever the machine has installed — which on a machine with a Chinese desktop is a full CJK face, and on one without is a machine with no Han names to draw.

### Two checks, because a translation is not a string

`cargo test` shapes every sentence of every language in the faces the greeter ships, loaded into an **empty** font database — the machine's own fonts are deliberately kept out of it, because the question is what happens on a machine that has just been installed and has none. A character no shipped face can draw fails the build.

Then it lays every screen out, in every language, on displays from 1280×720 to 4K, and measures each run against the box it was given. Nothing here wraps onto the column: the reserved line under the button is one line tall at every size, the caps of the on-screen board are the width of the keys under them, and text laid out past its rectangle is *cut*. A sentence that does not fit is not a longer sentence — it is half a sentence, and the half that goes missing is the end of it. Both checks prove they have teeth by asserting that a script nothing carries, and a sentence far too long for the line, are reported rather than passed over.

That check is what several of the shorter sentences here are: the reserved line holds about forty Latin characters at the tightest size the greeter is drawn at, and where a natural translation ran past it, the sentence was written shorter rather than the line made longer. It caught an English one too.

## Validation

```sh
cargo fmt -- --check
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
```

The test suite includes an in-process fake greetd exchange which checks the actual requests on the wire: a selected LineXinBar entry receives exactly one canonical handoff and a Plasma entry receives none; a refused password is followed by a `cancel_session` on that same connection before the next attempt's `create_session`; and a login that meets a session already under configuration sends the `cancel_session` and the second `create_session` that clear it. Unix-socket tests may need to run outside syscall-restricted build sandboxes.

Previewing is deliberately separate from installing/configuring the system greeter. A production milestone must additionally test authentication failure/retry, logout, session cleanup, Plasma and LineXinBar launches, multiple monitors, controller hardware, and VT switching on a disposable test machine.

## Roadmap

1. Build the root seat/session broker plus a minimal Smithay greeter compositor, keeping secrets in the unprivileged greeter/greetd conversation.
2. Add AccountsService/NSS enumeration while retaining the bounded, privacy-safe “Other account” route for LDAP/NIS and hidden users.
3. Move multi-seat preference coordination and broker-owned accent refresh into the seat service.
4. Give the greeter a surface per output, so that it draws on every screen under an ordinary compositor and not only under a kiosk one. That is what makes `CEDM_GREETER_COMPOSITOR=lxb` — and with it the login screen's modes, layout and high dynamic range — the default rather than an opt-in for single-display machines; see [What the greeter cannot do with it](#what-the-greeter-cannot-do-with-it).
5. Add screen reader/a11y semantics. (Localisation is done, in nine languages taken from the machine's own locale: see [What it says, and in which language](#what-it-says-and-in-which-language). Multi-output layout is done too: see [Every display is a display](#every-display-is-a-display). What is left of that is per-output scale factors, which need the layout in logical coordinates the compositor's own broker will publish.)
6. Add the isolated X11 server/auth wrapper, then expose parsed X11 entries.
7. Add nested GPU golden frames and real-VT end-to-end tests for LineXinBar and Plasma.
8. Close the last black interval — the one with no DRM master in it — as part of step 1, and add golden-frame coverage of the whole handover.
