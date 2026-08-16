#!/usr/bin/env bash

# Stage the runtime payload shared by every distro package. This deliberately
# does not install distro-specific documentation or license metadata.

set -euo pipefail

packaging_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=packaging/lib.sh
source "$packaging_dir/lib.sh"

destdir=""
prefix="/usr"
sysconfdir="/etc"
target_dir="${CARGO_TARGET_DIR:-$PROJECT_ROOT/target}"

usage() {
    cat <<'EOF'
Usage: packaging/install.sh --destdir DIR [--prefix PREFIX]
                            [--sysconfdir DIR] [--target-dir DIR]

Stages the greeter binary, the session wrapper greetd starts it with, the
display-manager unit, the sysusers/tmpfiles fragments that provision the
greeter account and its state directories, the Steam Controller udev rule, and
greetd's CEDM configuration. PREFIX defaults to /usr and SYSCONFDIR to /etc.

No administrator policy file is staged. CEDM runs correctly without one, and
its defaults are the permissive ones; the annotated example is installed as
documentation by each package instead.
EOF
}

while (($#)); do
    case "$1" in
        --destdir)
            (($# >= 2)) || package_die "--destdir requires a value"
            destdir="$2"
            shift 2
            ;;
        --prefix)
            (($# >= 2)) || package_die "--prefix requires a value"
            prefix="$2"
            shift 2
            ;;
        --sysconfdir)
            (($# >= 2)) || package_die "--sysconfdir requires a value"
            sysconfdir="$2"
            shift 2
            ;;
        --target-dir)
            (($# >= 2)) || package_die "--target-dir requires a value"
            target_dir="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *) package_die "unknown install option: $1" ;;
    esac
done

[[ -n "$destdir" ]] || package_die "--destdir is required"
[[ "$destdir" == /* ]] || package_die "--destdir must be absolute"
[[ -z "$prefix" || "$prefix" == /* ]] || package_die "--prefix must be empty or absolute"
[[ "$sysconfdir" == /* ]] || package_die "--sysconfdir must be absolute"

if [[ "$target_dir" != /* ]]; then
    target_dir="$PROJECT_ROOT/$target_dir"
fi
prefix="${prefix%/}"
[[ "$prefix" != "/" ]] || prefix=""
install_root="${destdir}${prefix}"
config_root="${destdir}${sysconfdir%/}/$PACKAGE_NAME"

[[ -x "$target_dir/release/$PACKAGE_NAME" ]] \
    || package_die "missing release binary: $target_dir/release/$PACKAGE_NAME"
install -Dm0755 "$target_dir/release/$PACKAGE_NAME" "$install_root/bin/$PACKAGE_NAME"

# The wrapper greetd runs, rather than a command line spelled out in a config
# file: what the greeter needs around it — a compositor holding the seat, a
# cleared display environment, a state directory — is the packager's business
# and changes with the packaging, not the administrator's.
install -Dm0755 "$PACKAGING_DIR/files/cedm-greeter-session" \
    "$install_root/bin/cedm-greeter-session"

# The wrapper the *authenticated* session runs behind, installed beside the
# greeter's because that is where the greeter's wrapper looks for it. It is
# what keeps a session's output — and the profile it sources — off the login
# VT, the way SDDM's session log file and GDM's journal redirection do.
install -Dm0755 "$PACKAGING_DIR/files/cedm-session" \
    "$install_root/bin/cedm-session"

# The unit that makes this a display manager rather than a program someone can
# run. `Alias=display-manager.service` in it is what `systemctl enable` acts on.
install -Dm0644 "$PACKAGING_DIR/files/$PACKAGE_NAME.service" \
    "$install_root/lib/systemd/system/$PACKAGE_NAME.service"

# The greeter account and the two directories it needs. Declarative rather than
# a maintainer script, so all four package formats provision one identical
# machine and `systemd-sysusers`/`systemd-tmpfiles` can be re-run to repair it.
install -Dm0644 "$PACKAGING_DIR/files/sysusers.conf" \
    "$install_root/lib/sysusers.d/$PACKAGE_NAME.conf"
install -Dm0644 "$PACKAGING_DIR/files/tmpfiles.conf" \
    "$install_root/lib/tmpfiles.d/$PACKAGE_NAME.conf"

# hidraw is not covered by systemd's uaccess rules, so without this the one
# controller the greeter was written for is the one it cannot read.
install -Dm0644 "$PACKAGING_DIR/files/71-cedm-steam-controller.rules" \
    "$install_root/lib/udev/rules.d/71-cedm-steam-controller.rules"

# greetd's configuration, under CEDM's own directory rather than /etc/greetd:
# a machine may already run greetd for something else, and enabling CEDM must
# not rewrite that machine's other greeter.
install -Dm0644 "$PACKAGING_DIR/files/greetd.toml" "$config_root/greetd.toml"
