# Logging in without a black screen

A login crosses three boundaries, and each of them used to be visible. This
is what each one costs, what removes it, and what is still outstanding.

CEDM is a display manager, not a desktop: two of the three boundaries are on
the session's side of the handover and are removed by the session cooperating,
not by anything the greeter can do alone. Nothing here is required. A desktop
that implements none of it still launches correctly; it simply arrives the way
every other display manager arrives.

## 1. Greeter interface → session, with the animation intact

Owned by CEDM, and already done.

The greeter fades its interface out over 300 ms while the wallpaper keeps
running, and only then asks greetd to start the session. For a desktop that
implements the continuity contract it also hands over where the wallpaper's
clock had reached, so the session's first frame carries on from that exact
phase rather than restarting the animation. See "LineXinBar background
handoff" in the top-level README.

## 2. Session compositor → its shell

Owned by the session. LineXinBar does all of these; another desktop is free to
do none of them.

- **Do not put anything on the critical path that a first frame does not
  need.** LineXinBar used to start XWayland, wait for the X server *and* its
  window manager, and only then spawn the shell — several hundred milliseconds
  of nothing on screen for a handshake no Wayland frame depends on. The two now
  start together. The bridge frame below is on that path too, and is drawn on
  every core the machine has for the same reason.
- **Draw the wallpaper before the shell exists.** A compositor with no client
  yet shows its clear colour, which is a black screen. LineXinBar's compositor
  now draws the same analytic wallpaper itself, at the phase CEDM handed over,
  from its first frame until the shell has pixels of its own.
- **Keep drawing it after the shell has gone.** The same interval happens in
  reverse, and it is the one a greeter lands in: a shell's surfaces go away
  with the process that owned them, and its compositor outlives that by however
  long it takes to notice. LineXinBar keeps the bridge for the compositor's
  whole life and draws it whenever a frame contains no session at all, so both
  ends of a session are a wallpaper rather than a clear colour — and it notices
  within a frame rather than a fifth of a second, because that interval is on
  screen and is also dead time in every logout.
- **Keep it moving.** A bridge frame drawn once and held is a picture the shell
  jumps away from the moment it arrives, by however long the bridge lasted —
  and at the far end of a session, where the wallpaper is hours along, a frame
  made at login is not a jump but a different wallpaper. LineXinBar paints the
  bridge on a thread of its own for as long as it is the only thing on screen,
  which is the two-thirds of a second either side of a session and nothing
  else: the moment the shell has anything to show, the painting stops.

## 2a. Greeter compositor → the greeter

The same problem one boundary earlier, and only where
`CEDM_GREETER_COMPOSITOR=lxb`: that compositor takes the displays several
hundred milliseconds before the greeter has a window to draw in, and holds
them again after the greeter has closed one.

So it is handed a wallpaper record of its own — printed on standard output by
`--compositor-config`, and put into its environment by `cedm-greeter-session`.
That record has to be written rather than left to the compositor to work out,
because the compositor reads the shell settings of the account it runs as, and
that account is the greeter's own: it has no LineXinBar settings and never
will. Left to itself it would draw the default purple under a login screen the
user has set to blue, and the flash would land at exactly the handover the rest
of this exists to remove.

The greeter is handed the same record back and continues its clock from it, so
the wallpaper is one unbroken animation from the greeter compositor's first
frame, through the login screen, into the session shell's.

That clock is only zeroed once per boot. `--compositor-config` runs afresh at
every login screen, including the one greetd raises the instant a session ends,
and a clock started there would put a whole session's worth of animation into
the moment the user signs out. So the origin — one monotonic instant, and the
boot it belongs to — is written down under the greeter account's own state
directory and read back at every login screen after the first. Nothing has to
be sent back from the session for this: the session's clock came from this same
origin on the way in, so continuing from it on the way out lands on the frame
the session was showing. An anchor from another boot, or one that cannot be
read, is no anchor at all and the animation starts over.

## 3. Greeter compositor → session compositor

Solved by keeping the picture, not by shortening the gap.

Between the greeter's compositor exiting and the session's compositor setting a
mode, no process holds the DRM master. Measured on a real login: **866 ms**,
from the greeter's compositor saying goodbye to the session's first modeset, of
which about 390 ms is greetd and the session wrapper and about 425 ms is the
compositor's own start-up. Neither half is worth shaving. What mattered was
that those 866 ms were black rather than a picture.

**It is not the kernel restoring a text console.** With
`CONFIG_FRAMEBUFFER_CONSOLE_DEFERRED_TAKEOVER` and a quiet boot, the frame
buffer console never takes over: on the machine this was measured on only the
dummy console is bound. Check `/sys/class/vtconsole/*/name` before believing
otherwise.

**It is the kernel, though.** Two things happen when the last handle on a DRM
device closes, and neither is anything a compositor asks for:

* `drm_fb_release` destroys every framebuffer that file created, and removing
  one that a plane is still scanning out disables that plane — and, for a
  primary plane, the CRTC behind it.
* `drm_lastclose` then restores the frame buffer console's own mode on top,
  which on a machine that never took the console over is a cleared buffer.

So a compositor blanks the display by exiting, however carefully it shut down.
Two earlier versions of this file blamed something else each time — first the
console, then wlroots restoring its saved CRTC state on Cage's way out. The
second is real, but it is not what was left: LineXinBar's compositor restores
no CRTC and the screen still went black.

Both are keyed to the *file description* rather than the process. So what
removes them is to not let it close. On the way out the compositor forks a
child that holds the descriptor open and does nothing else with it, until the
next compositor has committed a frame of its own or ten seconds have passed.
The framebuffers stay alive, the CRTC goes on scanning one out, and the display
keeps showing the last frame of the outgoing session — the wallpaper of §2, at
the phase the next one is about to carry on from.

That is the same thing every other display manager gets from the other end:
SDDM's greeter and GDM's both stay alive across the switch, so the outgoing
framebuffer is still there to be replaced. Holding the descriptor buys it
without a second process tree.

The incoming half matters just as much, and is the reason the greeter runs on
LineXinBar's compositor wherever it is installed. A compositor normally
disables every connector and clears every plane when it opens the GPU, so that
no earlier compositor's state can make its own commits fail — which is a black
screen for as long as it takes to reach a first frame, laid straight over the
picture the last one was still showing. Told that a hand-over is happening, it
inherits that state instead: the connector-to-CRTC mapping is recovered as it
was left, so the first commit is a plane update on a live display rather than a
modeset, and a modeset that does not change the mode does not blank. If what
was inherited turns out to be unusable, the device is reset and rescanned once
— the flicker comes back rather than the display staying dark.

Both halves are asked for with one variable, `LXB_HOLD_DISPLAY=1`, which
`cedm-greeter-session` and `cedm-session` export on their respective sides. It
is documented in LineXinBar's `docs/configuration.md`; any compositor is free
to implement it, and one that does not simply behaves the way it always did.

The colour pipeline is left alone across the same boundary, for the same
reason: undoing HDR makes the panel re-sync, which is a black screen of the
display's own making arriving exactly where one is being removed. The night
light is kept for that reason too — handing the display back cold and letting
the next compositor warm it again is three states where the user asked for one,
and it is visible the moment the black stops hiding it. The cost is that a
compositor handed the displays this way must set its own colour state; this
project's greeter does, within a frame of taking over.

Two things still matter on the machine's side, and one of them is what makes
the frame stay up at all.

**Keep the session on the greeter's VT.** Configure greetd's `[terminal]` so
the session starts on the terminal the greeter was already on. A VT switch is
a mode set of its own and hands the console back on the way through, which
undoes everything above. See `../greetd/config.toml.example`.

**Keep the console off the display.** On the kernel command line:

```text
quiet loglevel=3 vt.global_cursor_default=0
```

This is not cosmetic any more. With deferred takeover the frame buffer console
stays out of the way only until something writes to it; once it has taken over,
every hand-over from then on repaints a console over the picture. A quiet boot
is what keeps the last frame on the screen.

**Not needed, and no longer recommended: unbinding the framebuffer console.**
Earlier versions of this file suggested `echo 0 > /sys/class/vtconsole/vtcon1/bind`
to stop the console being restored. On a machine that boots quietly there is
nothing to stop, and the cost — no text console anywhere, so a session that
fails to start leaves a screen with nothing on it and no way to read why — was
never worth paying. Check `/sys/class/vtconsole/*/name` before concluding the
console is your problem: if the only bound one is the dummy device, it is not.
