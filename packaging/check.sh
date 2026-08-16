#!/usr/bin/env bash

set -euo pipefail

packaging_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=packaging/lib.sh
source "$packaging_dir/lib.sh"

build=true
case "${1:-}" in
    --no-build) build=false ;;
    -h|--help)
        echo "Usage: packaging/check.sh [--no-build]"
        exit 0
        ;;
    "") ;;
    *) package_die "unknown check option: $1" ;;
esac

package_note "checking shell syntax"
while IFS= read -r -d '' script; do
    bash -n "$script"
done < <(find "$PACKAGING_DIR" -type f -name '*.sh' -print0)
bash -n "$PACKAGING_DIR/arch/PKGBUILD.in"
sh -n "$PACKAGING_DIR/files/cedm-greeter-session"
sh -n "$PACKAGING_DIR/files/cedm-session"

if command -v shellcheck >/dev/null 2>&1; then
    while IFS= read -r -d '' script; do
        shellcheck -x "$script"
    done < <(find "$PACKAGING_DIR" -type f -name '*.sh' -print0)
    shellcheck -s sh "$PACKAGING_DIR/files/cedm-greeter-session"
    shellcheck -s sh "$PACKAGING_DIR/files/cedm-session"
fi

package_note "checking the package version is consistent"
# The package and the application report the same version, so the crate is the
# other half of this check: `cedm --version`
# disagreeing with the package it was installed from is the kind of thing
# nobody notices until a bug report.
crate_version="$(awk '
    /^\[/ { section = $0; next }
    section == "[package]" \
        && match($0, /^version[[:space:]]*=[[:space:]]*"[^"]+"/) {
        line = substr($0, RSTART, RLENGTH)
        sub(/^version[[:space:]]*=[[:space:]]*"/, "", line)
        sub(/"$/, "", line)
        print line
        exit
    }' "$PROJECT_ROOT/Cargo.toml")"
[[ -n "$crate_version" ]] \
    || package_die "could not read [package] version from Cargo.toml"
[[ "$crate_version" == "$PACKAGE_VERSION" ]] \
    || package_die "packaging/VERSION says $PACKAGE_VERSION but the crate says $crate_version"
# The spec carries a literal version, so compare it against VERSION itself: a
# pattern spelling out the version would agree with a stale spec forever, and
# the mismatch would only surface as rpmbuild failing to find its Source0.
grep -Eq "^Version:[[:space:]]+${PACKAGE_VERSION//./\\.}\$" \
    "$PACKAGING_DIR/fedora/$PACKAGE_NAME.spec" \
    || package_die "fedora/$PACKAGE_NAME.spec does not declare version $PACKAGE_VERSION"
# The rest take the version from VERSION, so check that they still do.
grep -Fqx 'pkgver=@VERSION@' "$PACKAGING_DIR/arch/PKGBUILD.in" \
    || package_die "arch/PKGBUILD.in no longer reads its version from packaging/VERSION"
grep -Fq 'builtins.readFile ../VERSION' "$PACKAGING_DIR/nix/package.nix" \
    || package_die "nix/package.nix no longer reads its version from packaging/VERSION"
grep -Fq 'Version: @VERSION@-1' "$PACKAGING_DIR/debian/control.in" \
    || package_die "debian/control.in no longer reads its version from packaging/VERSION"

if command -v rpmspec >/dev/null 2>&1; then
    rpmspec --parse "$PACKAGING_DIR/fedora/$PACKAGE_NAME.spec" >/dev/null
fi
if command -v nix-instantiate >/dev/null 2>&1; then
    nix-instantiate --parse "$PROJECT_ROOT/flake.nix" >/dev/null
    nix-instantiate --parse "$PACKAGING_DIR/nix/package.nix" >/dev/null
    nix-instantiate --parse "$PACKAGING_DIR/nix/module.nix" >/dev/null
fi

# Each package definition declares the toolchain it needs to its own distro's
# dependency solver, which cannot read Cargo.toml. This is what keeps those
# declarations and the crate's `rust-version` the same number.
grep -Eq "^BuildRequires:[[:space:]]+cargo >= ${MINIMUM_RUST//./\\.}\$" \
    "$PACKAGING_DIR/fedora/$PACKAGE_NAME.spec" \
    || package_die "fedora/$PACKAGE_NAME.spec does not require Rust $MINIMUM_RUST"
grep -Fq "rust-version $MINIMUM_RUST" "$PACKAGING_DIR/arch/PKGBUILD.in" \
    || package_die "arch/PKGBUILD.in no longer documents Rust $MINIMUM_RUST"

require_command cargo
require_rust_version
if [[ "$build" == true ]]; then
    package_note "building the release binary"
    cargo build --manifest-path "$PROJECT_ROOT/Cargo.toml" --release --locked --bins
fi

work="$(package_work_dir cedm-check)"
cleanup() {
    if [[ -n "${work:-}" && "$work" == */cedm-check.* && -d "$work" ]]; then
        rm -rf -- "$work"
    fi
}
trap cleanup EXIT

stage="$work/stage"
# No --target-dir: install.sh already follows CARGO_TARGET_DIR, which is where
# the build above put the binary.
"$PACKAGING_DIR/install.sh" --destdir "$stage"

package_note "checking the staged display-manager payload"
for path in \
    "usr/bin/$PACKAGE_NAME" \
    "usr/bin/cedm-greeter-session" \
    "usr/bin/cedm-session" \
    "usr/lib/systemd/system/$PACKAGE_NAME.service" \
    "usr/lib/sysusers.d/$PACKAGE_NAME.conf" \
    "usr/lib/tmpfiles.d/$PACKAGE_NAME.conf" \
    "usr/lib/udev/rules.d/71-cedm-steam-controller.rules" \
    "etc/$PACKAGE_NAME/greetd.toml"
do
    [[ -f "$stage/$path" ]] || package_die "staged payload is missing: $path"
done
for path in "usr/bin/$PACKAGE_NAME" "usr/bin/cedm-greeter-session" \
    "usr/bin/cedm-session"
do
    [[ -x "$stage/$path" ]] || package_die "staged payload is not executable: $path"
done

unit="$stage/usr/lib/systemd/system/$PACKAGE_NAME.service"
# `Alias=display-manager.service` is the whole reason this unit exists rather
# than a README paragraph: it is what makes `systemctl enable` replace whichever
# login screen the machine had. Without it the package installs a service that
# nothing ever starts.
grep -qx 'Alias=display-manager.service' "$unit" \
    || package_die "the unit no longer aliases display-manager.service"
grep -qx 'Conflicts=getty@tty1.service' "$unit" \
    || package_die "the unit no longer conflicts with the getty on its VT"
grep -q "^ExecStart=.*/etc/$PACKAGE_NAME/greetd.toml\$" "$unit" \
    || package_die "the unit no longer starts greetd with CEDM's own configuration"

# The account is named in three files that are written independently. Two of
# them disagreeing is a display manager that starts as a user who does not
# exist, and the only symptom is greetd exiting at boot.
grep -qx "u $GREETER_ACCOUNT .*" "$stage/usr/lib/sysusers.d/$PACKAGE_NAME.conf" \
    || package_die "sysusers.d does not create $GREETER_ACCOUNT"
grep -q "^user = \"$GREETER_ACCOUNT\"\$" "$stage/etc/$PACKAGE_NAME/greetd.toml" \
    || package_die "greetd.toml does not run the greeter as $GREETER_ACCOUNT"
grep -q " $GREETER_ACCOUNT $GREETER_ACCOUNT " \
    "$stage/usr/lib/tmpfiles.d/$PACKAGE_NAME.conf" \
    || package_die "tmpfiles.d does not give $GREETER_ACCOUNT a home it owns"
# greetd starts this path directly, so a wrapper installed anywhere else is a
# greeter that cannot start.
grep -qx 'command = "/usr/bin/cedm-greeter-session"' \
    "$stage/etc/$PACKAGE_NAME/greetd.toml" \
    || package_die "greetd.toml does not start the packaged session wrapper"

# The greeter's own state directory is the one the code compiles in. If either
# side moves, the broker publishes accents nothing reads.
broker_state="$(sed -n 's/^pub const PATH: &str = "\(.*\)\/state.toml";$/\1/p' \
    "$PROJECT_ROOT/src/state.rs")"
[[ -n "$broker_state" ]] \
    || package_die "could not read the broker state path from src/state.rs"
grep -q "^d $broker_state " "$stage/usr/lib/tmpfiles.d/$PACKAGE_NAME.conf" \
    || package_die "tmpfiles.d does not create the broker state directory $broker_state"
config_path="$(sed -n 's/^pub const PATH: &str = "\(.*\)";$/\1/p' \
    "$PROJECT_ROOT/src/config.rs")"
[[ "$config_path" == "/etc/$PACKAGE_NAME/config.toml" ]] \
    || package_die "src/config.rs reads policy from $config_path, not /etc/$PACKAGE_NAME"

if command -v python3 >/dev/null 2>&1 && python3 -c 'import tomllib' 2>/dev/null; then
    for toml in "$stage/etc/$PACKAGE_NAME/greetd.toml" \
        "$PROJECT_ROOT/contrib/config.toml.example"
    do
        python3 -c 'import sys, tomllib; tomllib.load(open(sys.argv[1], "rb"))' "$toml" \
            || package_die "not valid TOML: $toml"
    done
fi

if command -v udevadm >/dev/null 2>&1 && udevadm verify --help >/dev/null 2>&1; then
    udevadm verify --no-summary \
        "$stage/usr/lib/udev/rules.d/71-cedm-steam-controller.rules"
fi
if command -v systemd-tmpfiles >/dev/null 2>&1; then
    # --dry-run needs the account to exist to resolve the ownership column, and
    # on a machine that has never installed CEDM it does not. Parsing the file
    # is what is being checked here, so an unknown user is not a failure.
    systemd-tmpfiles --dry-run --create \
        "$stage/usr/lib/tmpfiles.d/$PACKAGE_NAME.conf" >/dev/null 2>&1 || true
fi
# `systemd-analyze verify` resolves ExecStart, so it can only run where greetd
# is actually installed. Where it is, it is worth having: it catches a directive
# that silently does nothing far earlier than a machine that will not boot to a
# login screen does.
if command -v systemd-analyze >/dev/null 2>&1 && [[ -x /usr/bin/greetd ]]; then
    systemd-analyze verify "$unit"
fi

# Every face in the binary needs a licence text shipped with it, and every
# package format has to install both of them. The faces are compiled in with
# `include_bytes!`, so a missing *font* fails the build loudly; a missing
# licence fails nothing at all, and a package that drops one is a package that
# redistributes a font without its terms.
package_note "checking every bundled font has its licence, in every package"
for licence in LICENSE.txt LICENSE-NotoSans.txt; do
    [[ -f "$PROJECT_ROOT/assets/fonts/$licence" ]] \
        || package_die "assets/fonts/$licence is missing"
    # The three formats that keep a licence directory of their own.
    for packager in "$PACKAGING_DIR/arch/PKGBUILD.in" "$PACKAGING_DIR/nix/package.nix" \
        "$PACKAGING_DIR/fedora/$PACKAGE_NAME.spec"
    do
        grep -Fq "assets/fonts/$licence" "$packager" \
            || package_die "${packager#"$PACKAGING_DIR"/} does not install assets/fonts/$licence"
    done
done
# Debian is the exception, and deliberately: its licence record is the
# copyright file, which points at the Apache text every Debian system already
# carries in /usr/share/common-licenses. OFL-1.1 is not one of those, so that
# text has to travel with the package or the copyright file sends a reader
# nowhere.
grep -Fq "assets/fonts/LICENSE-NotoSans.txt" "$PACKAGING_DIR/debian/build.sh" \
    || package_die "debian/build.sh does not ship the Open Font Licence text"
for pattern in "assets/fonts/Roboto-" "assets/fonts/NotoSans"; do
    grep -Fq "$pattern" "$PACKAGING_DIR/debian/copyright" \
        || package_die "debian/copyright does not account for $pattern*"
done
# And a face nothing loads is a face nobody checked the licence of.
while IFS= read -r -d '' face; do
    grep -Fq "assets/fonts/${face##*/}" "$PROJECT_ROOT/src/visual/mod.rs" \
        || package_die "assets/fonts/${face##*/} is shipped but never loaded"
done < <(find "$PROJECT_ROOT/assets/fonts" -name '*.ttf' -print0)

package_note "checking every package enables the unit the same way"
# Three package formats, three scriptlet languages, one behaviour: enable
# cedm.service on a first install, refuse to take display-manager.service from
# another display manager that already holds it, and never start anything. A
# format that quietly stopped doing one of those would be a machine that
# installs a login screen and does not boot to it — or one that takes over a
# working GDM without saying so.
arch_install="$PACKAGING_DIR/arch/cedm.install"
[[ -f "$arch_install" ]] || package_die "arch/cedm.install is missing"
grep -Fqx "install=cedm.install" "$PACKAGING_DIR/arch/PKGBUILD.in" \
    || package_die "arch/PKGBUILD.in no longer names cedm.install as its scriptlet"
grep -Fq 'install -m0644 "$script_dir/cedm.install"' "$PACKAGING_DIR/arch/build.sh" \
    || package_die "arch/build.sh no longer renders cedm.install beside the PKGBUILD"

for scriptlet in "$arch_install" "$PACKAGING_DIR/debian/postinst" \
    "$PACKAGING_DIR/fedora/cedm.spec"; do
    name="${scriptlet#"$PACKAGING_DIR"/}"
    # The redirection is what distinguishes the call from the same words
    # printed in the "to do it by hand" message beside it. Without it this
    # matches the advice and passes over a scriptlet that enables nothing.
    grep -Fq "systemctl enable cedm.service >/dev/null" "$scriptlet" \
        || package_die "$name does not enable cedm.service on install"
    grep -Fq "/etc/systemd/system/display-manager.service" "$scriptlet" \
        || package_die "$name does not check for an existing display manager"
    grep -Fq "systemctl get-default" "$scriptlet" \
        || package_die "$name does not check the machine's default boot target"
    # `systemctl start` or `--now` here would end the session of whoever ran
    # the install: the running greeter owns that seat.
    if grep -Eq "systemctl (start|enable --now|--now enable) cedm" "$scriptlet"; then
        package_die "$name starts cedm.service; installing must never take the running seat"
    fi
done

# And then actually run the two scriptlets that are runnable, because a grep
# over a shell script proves the words are present and nothing about what it
# does with them. `systemctl` is a stub on PATH that records its arguments, so
# this touches no unit on the machine running the check.
harness="$work/enable"
mkdir -p "$harness/bin" "$harness/units"
cat > "$harness/bin/systemctl" <<'STUB'
#!/bin/sh
echo "$*" >> "$SYSTEMCTL_LOG"
[ "$1" = get-default ] && echo graphical.target
exit 0
STUB
chmod +x "$harness/bin/systemctl"
# The scriptlets skip everything when systemd is not running the machine, which
# is exactly the case in a build container. Nothing below can be checked there.
if [ -d /run/systemd/system ]; then
    ln -sf /usr/lib/systemd/system/some-other-dm.service "$harness/units/taken"

    enable_attempted() {
        # $1 is a shell command that invokes the scriptlet; $2 the link path.
        SYSTEMCTL_LOG="$harness/log" : > "$harness/log"
        env PATH="$harness/bin:$PATH" SYSTEMCTL_LOG="$harness/log" \
            CEDM_DISPLAY_MANAGER_LINK="$2" _cedm_display_manager_link="$2" \
            sh -c "$1" >/dev/null 2>&1 || true
        grep -q "^enable cedm.service$" "$harness/log"
    }

    for probe in \
        "sh '$PACKAGING_DIR/debian/postinst' configure" \
        ". '$arch_install'; post_install"; do
        enable_attempted "$probe" "$harness/units/absent" \
            || package_die "scriptlet did not enable cedm.service on a machine with no display manager: $probe"
        ! enable_attempted "$probe" "$harness/units/taken" \
            || package_die "scriptlet claimed display-manager.service from another display manager: $probe"
    done

    # An upgrade must not re-enable a unit the administrator turned off.
    ! enable_attempted "sh '$PACKAGING_DIR/debian/postinst' configure 0.0.1" \
        "$harness/units/absent" \
        || package_die "debian/postinst re-enables cedm.service on upgrade"
    ! enable_attempted ". '$arch_install'; post_upgrade" "$harness/units/absent" \
        || package_die "arch/cedm.install re-enables cedm.service on upgrade"
fi

# The removal path, which is what stops a machine booting to a unit whose
# binary has just been uninstalled.
grep -Fq "systemctl --quiet disable cedm.service" "$PACKAGING_DIR/debian/prerm" \
    || package_die "debian/prerm no longer disables cedm.service on removal"
grep -Fq "systemctl --quiet disable cedm.service" "$arch_install" \
    || package_die "arch/cedm.install no longer disables cedm.service on removal"

package_note "package definitions and staged payload are valid (version $PACKAGE_VERSION)"
