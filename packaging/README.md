# CEDM packaging

These definitions build one early-development package named
`cedm`. The package version is **0.9.0**, which is
the crate's own version: what a package claims and what
`cedm --version` reports are the same number, and
`packaging/build.sh check` refuses to let the two drift apart. Bumping a release
means editing both `packaging/VERSION` and `[package]` in `Cargo.toml`.

## What a package installs

Writing `cedm` for `cedm`, which is what every one
of these files is actually called:

```text
usr/bin/cedm            the greeter
usr/bin/cedm-greeter-session                          the wrapper greetd starts it with
usr/lib/systemd/system/cedm.service                   the display-manager unit
usr/lib/sysusers.d/cedm.conf                          the cedm-greeter account
usr/lib/tmpfiles.d/cedm.conf                          the state directory and the account's home
usr/lib/udev/rules.d/71-cedm-steam-controller.rules   the Steam Controller's hidraw node
etc/cedm/greetd.toml                                  greetd's CEDM configuration
```

Nothing under `/etc` is written except that one file, and it is a conffile on
every format that has the concept. **No administrator policy file is installed.**
CEDM runs correctly without one and its compiled-in defaults are the permissive
ones; the annotated example goes to `/usr/share/doc` so that a machine which
wants a policy writes exactly the policy it wants.

## What makes it a display manager

The unit carries `Alias=display-manager.service`. That single line is the
difference between a program somebody can run and the thing the machine boots
to, and every package enables it on a first install:

```text
packaging/arch/cedm.install     post_install
packaging/debian/postinst       configure, with no previous version
packaging/fedora/cedm.spec      %post, $1 -eq 1
```

All three do the same three things, and a change to one belongs in all of them.
They refuse to claim `display-manager.service` from another display manager
that already holds it, printing how to switch instead. They never start the
unit, because starting a display manager takes the seat the person running the
install is signed in on. And they warn when the default boot target is not
`graphical.target`, which is what pulls `display-manager.service` in — an
enabled display manager on a `multi-user.target` machine still never runs.

Enabling on install is a deliberate departure from Arch's packaging standards,
which say a package must not enable the services it ships. A display manager is
the exception every distribution ends up making.

The unit also declares `Conflicts=greetd.service`, because CEDM *is* greetd with
a different configuration and two of them would fight over VT 1.

No package enables it for you. Installing a display manager and taking the
machine's login screen over are two different decisions, and only one of them is
implied by `apt install`.

### The layers under it

```text
cedm.service                       ← systemd; the display-manager alias
  └── greetd                       ← root; PAM, seats, session ownership
        └── cedm-greeter-session   ← the cedm-greeter account
              └── lxb              ← the seat-owning compositor
                    └── cedm       ← UI, input, PAM conversation only
```

greetd and lxb-compositor are hard dependencies rather than suggestions: the
unit starts one by absolute path and the wrapper execs the other. Neither is
optional, and without them the package installs a login screen that cannot come
up. lxb-compositor is LineXinBar's compositor packaged apart from that
project's shell, so depending on it brings in a Wayland session and not a
desktop.

The greeter itself never gains a privilege at any layer. `cedm-greeter` is a
system account with no login shell whose home holds one file of remembered
account and session choices; authentication stays in greetd and PAM, where the
answers go straight from the socket into a zeroizing buffer and never touch
disk.

### The one thing that is a real grant

`71-cedm-steam-controller.rules`. The second-generation Steam Controller has no
kernel gamepad driver, so CEDM reads its report from hidraw — and hidraw nodes
are `0600 root:root` with nothing in systemd's own uaccess rules tagging them.
The rule tags Valve's devices `uaccess`, which grants them to whoever holds the
*active session on the seat*: the greeter while the greeter is up, the user once
they have signed in, and at no point every account on the machine. That is why
`cedm-greeter` is not in the `input` group, and must not be put in it — that
group is read access to every evdev node, which is a keylogger's worth of
privilege for a program that takes its input through Wayland.

## Where the build happens, and why not /tmp

Every builder works under `packaging/out/build/`, on whatever filesystem the
checkout is on. Not `${TMPDIR:-/tmp}`, which is the obvious choice and the wrong
one: on a systemd machine /tmp is a tmpfs sized at a fraction of RAM, so
building there means building in memory. This dependency graph — wgpu, naga,
winit, x11rb — writes about 1 GiB compiling in the release profile, and the
`cargo test` that makepkg's `check()` and rpmbuild's `%check` run builds the
whole of it again in the dev profile for roughly 4.5 GiB more. A 16 GiB tmpfs
that is already three-quarters full runs out somewhere around crate nine
hundred, and reports it as `No space left on device` — or, where the tmpfs
carries quotas, as `Disk quota exceeded (os error 122)`.

Send it elsewhere with `--work-dir DIR` on the Arch and Fedora builders, or
`CEDM_WORK_DIR` for all of them:

```sh
./packaging/build.sh arch --work-dir /var/tmp/cedm
CEDM_WORK_DIR=/var/tmp/cedm ./packaging/build.sh fedora
```

A work directory inside the checkout is refused unless it is under
`packaging/out`, because `snapshot_source` picks up untracked files and a build
tree anywhere else would end up inside the source archive built from it.

The builders check free space before extracting anything, so a machine without
the room is told immediately rather than forty minutes in. That check reads
`df`, which cannot see a quota — the default location is what actually solves
the quota case. One thing it cannot route around either: `makepkg.conf` wins
over the environment, so a machine that sets `BUILDDIR` builds there whatever
`--work-dir` said. The Arch builder notices and says so.

## Validate the shared payload

```sh
./packaging/build.sh check
```

This checks shell and package syntax, confirms every package definition still
takes its version and its minimum Rust from the crate rather than from a copy,
builds the release binary, stages the payload, and then checks the things that
only fail at boot: that the unit still aliases `display-manager.service`, that
the account named by `sysusers.d`, by `tmpfiles.d` and by `greetd.toml` is one
account rather than three, that the wrapper greetd starts is the path the wrapper
is installed to, and that the state and policy directories the package creates
are the ones `src/state.rs` and `src/config.rs` compile in. Where the host has
them it also runs `udevadm verify`, `rpmspec --parse`, `nix-instantiate --parse`
and a TOML parse over the shipped greetd configuration.

Use `--no-build` only when a current release binary already exists in
`target/release` (or in `$CARGO_TARGET_DIR/release`, which the staging step
follows).

## Debian

Build on Debian, Ubuntu, or another Debian-derived system. The builder checks
everything it needs before compiling and names whatever is missing in one
`apt install` line — Rust among it as `rustup`, because Debian 13's own is
older than the locked graph allows. A distrobox or toolbox container on a plain
`debian` image is enough:

```sh
./packaging/build.sh debian
```

The builder runs `dpkg-shlibdeps` over the locally linked binary, stages a
policy-shaped binary package with `conffiles`, `md5sums` and maintainer scripts,
and writes it to `packaging/out/debian/`. Wayland, EGL and X11 libraries that
the greeter opens dynamically are declared explicitly because ELF dependency
scanning cannot see them.

It compiles into `target/debian` rather than `target/` (or into
`$CARGO_TARGET_DIR` when that is set), so a build in a container that shares
the checkout never replaces the host's own binaries.

`postinst` runs `systemd-sysusers` and then `systemd-tmpfiles` — in that order,
because the home directory's ownership cannot be resolved before the account
exists — reloads udev rules, and stops there. `prerm` disables the unit on
removal but deliberately does not stop it: this unit owns the seat the signed-in
user's session is running on, and stopping it during an unattended upgrade would
end that session and everything in it.

The package deliberately does not `Provides: x-display-manager`. That virtual
package is a promise to take part in Debian's `/etc/X11/default-display-manager`
mechanism and its shared debconf question, and CEDM does neither: it is selected
through the systemd alias, like greetd's own Debian package, and claiming an
interface it does not implement would leave a machine with two answers to the
question of which login screen it has.

Note that `control.in` becomes `DEBIAN/control` verbatim, and binary control
files cannot carry comments — anything that needs explaining is explained here.

`--allow-foreign-host` exists for package-structure testing only. A `.deb` built
against another distribution's libc must not be deployed on Debian.

## Fedora

Build on Fedora with the RPM tools and what the spec asks for, which
`dnf builddep` reads from the spec itself:

```sh
sudo dnf install rpm-build dnf5-plugins git-core
sudo dnf builddep packaging/fedora/cedm.spec
./packaging/build.sh fedora
```

The builder snapshots the tree, vendors the locked Cargo dependencies so
`rpmbuild` runs offline, and produces both a source RPM and a binary RPM in
`packaging/out/fedora/`.

`%postun` is plain `%systemd_postun`, not `%systemd_postun_with_restart`, for
the same reason `prerm` does not stop the unit on Debian.

## Arch

```sh
./packaging/build.sh arch
```

The builder snapshots the tree, writes a checksummed `PKGBUILD` from
`arch/PKGBUILD.in`, and runs `makepkg --cleanbuild`. Artifacts land in
`packaging/out/arch/` — collected from `makepkg --packagelist` rather than from
beside the `PKGBUILD`, so a `makepkg.conf` that sets `PKGDEST` still works.

No `.install` file: Arch's own ALPM hooks already run `systemd-sysusers`,
`systemd-tmpfiles`, `udevadm control --reload` and `systemctl daemon-reload`
after any package that ships those fragments.

`--allow-foreign-host` permits a build on an Arch derivative.

## Nix

```sh
./packaging/build.sh nix
```

NixOS is the one target that does not install the unit, and should not. There
the login screen is `services.greetd`, whose module already owns the service, the
VT and an unprivileged `greeter` account; writing CEDM's own unit into `/etc`
alongside it would give the machine two display managers competing for one
terminal, which is the failure `Conflicts=` prevents everywhere else. The module
configures greetd to start CEDM's wrapper instead, and the greetd configuration
is installed as an example rather than as configuration.

```nix
{
  inputs.cedm.url = "github:petexy/ConsoleExperienceDesktopManager";

  # in configuration.nix
  imports = [ inputs.cedm.nixosModules.default ];
  services.cedm = {
    enable = true;
    settings.default_session = "lxb";
  };
}
```

## Before deploying any of this

CEDM is an early vertical slice, and the seat/VT handoff it is missing is
described in the main README's roadmap. Install and test it from a spare VT, or
on a disposable machine, before it becomes the only way into one you need.
