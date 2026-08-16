#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=packaging/lib.sh
source "$script_dir/../lib.sh"

output_dir="$PACKAGING_DIR/out/debian"
allow_foreign=false

usage() {
    cat <<'EOF'
Usage: packaging/debian/build.sh [--output-dir DIR] [--allow-foreign-host]

Builds a Debian binary package from the current working tree. A deployable
package must be built on Debian (or a Debian derivative) so its ABI and
generated shared-library dependencies match the target system.
EOF
}

while (($#)); do
    case "$1" in
        --output-dir)
            (($# >= 2)) || package_die "--output-dir requires a value"
            output_dir="$2"
            shift 2
            ;;
        --allow-foreign-host) allow_foreign=true; shift ;;
        -h|--help) usage; exit 0 ;;
        *) package_die "unknown Debian builder option: $1" ;;
    esac
done

if [[ "$allow_foreign" != true ]] && ! host_is_like debian; then
    package_die "build Debian packages on Debian/Ubuntu; use --allow-foreign-host only for metadata testing"
fi
if [[ "$output_dir" != /* ]]; then
    output_dir="$PWD/$output_dir"
fi

require_command dpkg-deb
require_command dpkg-shlibdeps
require_command dpkg
require_command md5sum
require_rust_version

target_dir="${CARGO_TARGET_DIR:-$PROJECT_ROOT/target}"
if [[ "$target_dir" != /* ]]; then
    target_dir="$PROJECT_ROOT/$target_dir"
fi
package_note "building the CEDM release binary"
RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$PROJECT_ROOT=/usr/src/$PACKAGE_NAME-$PACKAGE_VERSION" \
    CARGO_TARGET_DIR="$target_dir" \
    cargo build --manifest-path "$PROJECT_ROOT/Cargo.toml" --release --locked --bins

work="$(package_work_dir cedm-debian)"
cleanup() {
    if [[ -n "${work:-}" && "$work" == */cedm-debian.* && -d "$work" ]]; then
        rm -rf -- "$work"
    fi
}
trap cleanup EXIT

package_root="$work/root"
"$PACKAGING_DIR/install.sh" --destdir "$package_root" --target-dir "$target_dir"

document_dir="$package_root/usr/share/doc/$PACKAGE_NAME"
install -Dm0644 "$script_dir/copyright" "$document_dir/copyright"
install -Dm0644 "$PROJECT_ROOT/README.md" "$document_dir/README.md"
install -Dm0644 "$PROJECT_ROOT/contrib/config.toml.example" \
    "$document_dir/config.example.toml"
install -Dm0644 "$PROJECT_ROOT/contrib/seamless/README.md" \
    "$document_dir/seamless-login.md"
install -Dm0644 "$PACKAGING_DIR/files/polkit-power.rules.example" \
    "$document_dir/polkit-power.rules.example"
# The Open Font Licence the two bundled Noto subsets are under. This one is
# carried rather than pointed at: /usr/share/common-licenses holds the GPL and
# Apache texts that every Debian system already has, and OFL-1.1 is not one of
# them, so the copyright file has to have somewhere to send a reader.
install -Dm0644 "$PROJECT_ROOT/assets/fonts/LICENSE-NotoSans.txt" \
    "$document_dir/LICENSE-NotoSans.txt"
# No /usr/share/licenses here: that is the RPM and Arch convention. On Debian
# the copyright file above is the licence record, and it points at the GPL-3
# and Apache-2.0 texts every Debian system already carries in
# /usr/share/common-licenses.

if command -v strip >/dev/null 2>&1; then
    strip --strip-unneeded "$package_root/usr/bin/$PACKAGE_NAME"
fi

mkdir -p "$work/shlibs/debian"
install -m0644 "$script_dir/source-control" "$work/shlibs/debian/control"
shlib_output="$({
    cd "$work/shlibs"
    dpkg-shlibdeps -O -e"$package_root/usr/bin/$PACKAGE_NAME"
})"
[[ "$shlib_output" == shlibs:Depends=* ]] \
    || package_die "could not determine Debian shared-library dependencies"
shlib_depends="${shlib_output#shlibs:Depends=}"

architecture="$(dpkg --print-architecture)"
installed_size="$(du -sk "$package_root" | awk '{print $1}')"
mkdir -p "$package_root/DEBIAN"
awk \
    -v version="$PACKAGE_VERSION" \
    -v architecture="$architecture" \
    -v dependencies="$shlib_depends" \
    -v installed_size="$installed_size" \
    '{
        gsub(/@VERSION@/, version)
        gsub(/@ARCH@/, architecture)
        gsub(/@SHLIB_DEPENDS@/, dependencies)
        gsub(/@INSTALLED_SIZE@/, installed_size)
        print
    }' "$script_dir/control.in" > "$package_root/DEBIAN/control"

for script in postinst prerm postrm; do
    install -m0755 "$script_dir/$script" "$package_root/DEBIAN/$script"
done

# Everything the package puts under /etc is an administrator's to edit, so dpkg
# has to be told to ask before replacing it on upgrade rather than overwriting a
# machine's greeter configuration because a new version shipped a comment.
(
    cd "$package_root"
    find etc -type f -printf '/%p\n' | sort
) > "$package_root/DEBIAN/conffiles"
[[ -s "$package_root/DEBIAN/conffiles" ]] \
    || package_die "no conffiles were staged; greetd.toml should be one"

(
    cd "$package_root"
    find usr etc -type f -print0 | sort -z | xargs -0 md5sum
) > "$package_root/DEBIAN/md5sums"

mkdir -p "$output_dir"
artifact="$output_dir/${PACKAGE_NAME}_${PACKAGE_VERSION}-1_${architecture}.deb"
dpkg-deb --root-owner-group -Zxz --build "$package_root" "$artifact"
dpkg-deb --info "$artifact" >/dev/null
package_note "created $artifact"
