Name:           cedm
Version:        0.1.0
Release:        1%{?dist}
Summary:        Controller-first graphical display manager for console and desktop sessions

# CEDM, its embedded Roboto, and the locked statically linked Rust dependency
# graph for Linux.
License:        GPL-3.0-only AND Apache-2.0 AND OFL-1.1 AND MIT AND BSD-2-Clause AND BSD-3-Clause AND ISC AND MPL-2.0 AND Unicode-3.0 AND Zlib
URL:            https://github.com/petexy/ConsoleExperienceDesktopManager
Source0:        %{name}-%{version}.tar.gz

ExclusiveArch:  x86_64 aarch64

# Cargo's release profile emits no DWARF, so find-debuginfo would produce an
# empty debugsourcefiles.list and rpmbuild would fail on it after the whole
# build. An archive submission wants real debuginfo instead: drop this and
# build with `-Cdebuginfo=2 -Cstrip=none` under Fedora's own remapping.
%global debug_package %{nil}

%global greeter_account cedm-greeter

BuildRequires:  cargo >= 1.85
BuildRequires:  rust >= 1.85
BuildRequires:  gcc
BuildRequires:  pkgconfig
BuildRequires:  pkgconfig(libudev)
# The four sounds the login screen answers a button with: rodio's CPAL backend
# links libasound.so.2.
BuildRequires:  pkgconfig(alsa)
BuildRequires:  pkgconfig(wayland-client)
BuildRequires:  pkgconfig(xkbcommon)
BuildRequires:  pkgconfig(x11)
BuildRequires:  pkgconfig(xcb)
BuildRequires:  pkgconfig(xcursor)
BuildRequires:  pkgconfig(xi)
BuildRequires:  systemd-rpm-macros
%{?sysusers_requires_compat}

# greetd owns PAM and the sessions it starts; CEDM is its greeter and its
# configuration, and is not a display manager without it.
# lxb-compositor gives that greeter a seat to draw on — it is
# LineXinBar's compositor packaged apart from that project's shell, so this
# brings in a Wayland session and not a desktop. Both are started by name at
# boot, so neither of them is a recommendation.
Requires:       greetd
Requires:       lxb-compositor
Requires:       dbus-daemon
Requires:       dbus-tools
Requires:       systemd
%{?systemd_requires}

# Wayland, EGL/Vulkan and the X libraries are loaded dynamically, so RPM's ELF
# dependency generator cannot discover them.
Requires:       libwayland-client
Requires:       libglvnd-egl
Requires:       mesa-libEGL
Requires:       libX11
Requires:       libX11-xcb
Requires:       libxcb
Requires:       libXcursor
Requires:       libXi
Requires:       libxkbcommon
Recommends:     mesa-vulkan-drivers
Recommends:     accountsservice
Recommends:     polkit
# Where LineXinBar's compositor is installed the greeter runs under it instead
# of cage, which is what lets the login screen set a mode, place its outputs
# and drive a display in HDR. Suggested rather than required: without it the
# greeter runs exactly as before, under the cage dependency above.
Suggests:       lxb-desktop

%description
Console Experience Desktop Manager is a controller-first graphical greeter for
LineXinBar and ordinary Linux desktop sessions. It renders a console-style
profile carousel over an analytic wallpaper, discovers installed Wayland session
entries, and conducts PAM conversations through greetd without ever holding a
privilege of its own.

This package installs the greeter, a display-manager systemd unit, greetd's
CEDM configuration, and the unprivileged account the greeter runs as. Enabling
the unit makes CEDM the machine's login screen.

%prep
%autosetup -n %{name}-%{version}

%build
export CARGO_TARGET_DIR=target
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=%{_builddir}=/usr/src/debug/%{name}-%{version}"
cargo build --frozen --release --bins

%check
export CARGO_TARGET_DIR=target
# `cargo test` already builds in the dev profile. Nothing here may pin an
# optimisation level: RUSTFLAGS is appended after the profile's own flags and
# wins, so a level named here would silently override the profile.
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=%{_builddir}=/usr/src/debug/%{name}-%{version}"
cargo test --frozen --lib --bins

%install
export CARGO_TARGET_DIR=target
./packaging/install.sh \
    --destdir %{buildroot} \
    --prefix %{_prefix} \
    --sysconfdir %{_sysconfdir} \
    --target-dir target

install -Dm0644 packaging/files/polkit-power.rules.example \
    %{buildroot}%{_docdir}/%{name}/polkit-power.rules.example
# Installed under a name of its own rather than listed with %%doc: that macro
# copies basenames, and contrib/seamless/README.md would land on top of the
# project's own README.md.
install -Dm0644 contrib/seamless/README.md \
    %{buildroot}%{_docdir}/%{name}/seamless-login.md

%pre
%sysusers_create_compat %{_sysusersdir}/%{name}.conf

%post
%systemd_post %{name}.service
%tmpfiles_create_package %{name} %{_tmpfilesdir}/%{name}.conf

# On a first install only ($1 is 1 there and 2 on an upgrade), make this the
# machine's display manager. That is what every distribution's display managers
# do, and it is the one kind of package where it is right: one that installs
# without being enabled has done nothing at all, and the machine still boots to
# whatever it booted to before.
#
# It will not take the login screen away from something else. Only one unit can
# hold `display-manager.service`, which is what that alias means, so where
# another display manager already has it this says so and leaves it alone.
# Nothing is ever started: that would take the seat the signed-in user is on.
if [ $1 -eq 1 ] && [ -d /run/systemd/system ]; then
    cedm_link=/etc/systemd/system/display-manager.service
    cedm_claim=1
    if [ -L "$cedm_link" ]; then
        cedm_current=$(readlink -f "$cedm_link" 2>/dev/null || true)
        case "$cedm_current" in
            */cedm.service) cedm_claim=0 ;;
            '') ;;
            *)
                cedm_claim=0
                cat >&2 <<EOF
cedm: ${cedm_current##*/} is already this machine's display manager, so
cedm: cedm.service has been left disabled. To switch to it:
cedm:     systemctl disable ${cedm_current##*/}
cedm:     systemctl enable cedm.service
EOF
                ;;
        esac
    fi
    if [ "$cedm_claim" -eq 1 ]; then
        if systemctl enable cedm.service >/dev/null 2>&1; then
            echo "cedm: enabled cedm.service; it is this machine's login screen from the next boot." >&2
            cedm_target=$(systemctl get-default 2>/dev/null || true)
            if [ -n "$cedm_target" ] && [ "$cedm_target" != graphical.target ]; then
                cat >&2 <<EOF
cedm: this machine boots to $cedm_target, which starts no display manager.
cedm:     systemctl set-default graphical.target
EOF
            fi
        else
            cat >&2 <<'EOF'
cedm: could not enable cedm.service automatically. To do it by hand:
cedm:     systemctl enable cedm.service
EOF
        fi
    fi
fi

%preun
%systemd_preun %{name}.service

%postun
# Deliberately not `_with_restart`: restarting a display manager takes the
# login screen — and on a machine where somebody is signed in, their session's
# seat — out from under whoever is using it. A new greeter arrives at the next
# boot, or when an administrator asks for it.
%systemd_postun %{name}.service

%files
%license LICENSE assets/fonts/LICENSE.txt assets/fonts/LICENSE-NotoSans.txt
%doc README.md contrib/config.toml.example
%{_docdir}/%{name}/polkit-power.rules.example
%{_docdir}/%{name}/seamless-login.md
%{_bindir}/%{name}
%{_bindir}/cedm-greeter-session
%{_bindir}/cedm-session
%{_unitdir}/%{name}.service
%{_sysusersdir}/%{name}.conf
%{_tmpfilesdir}/%{name}.conf
%{_udevrulesdir}/71-cedm-steam-controller.rules
%dir %{_sysconfdir}/%{name}
%config(noreplace) %{_sysconfdir}/%{name}/greetd.toml

%changelog
* Fri Aug 14 2026 Piotr Lewandowski <piotr.petexiness@gmail.com> - 0.1.0-1
- Initial early-development package
