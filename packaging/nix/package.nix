{
  lib,
  rustPlatform,
  pkg-config,
  makeWrapper,
  patchelf,
  addDriverRunpath,
  wayland,
  libxkbcommon,
  libglvnd,
  vulkan-loader,
  systemd,
  xorg,
  dbus,
  lxb-compositor,
  src ? ../..,
}:

let
  sourceRoot = toString src;
  cleanSrc = lib.cleanSourceWith {
    inherit src;
    filter = path: type:
      let
        relative = lib.removePrefix "${sourceRoot}/" (toString path);
      in
      !(relative == ".git"
        || lib.hasPrefix ".git/" relative
        || relative == "target"
        || lib.hasPrefix "target/" relative
        || relative == "packaging/out"
        || lib.hasPrefix "packaging/out/" relative
        || relative == "result"
        || lib.hasPrefix "result-" relative);
  };
  runtimeLibraries = [
    wayland
    libxkbcommon
    libglvnd
    vulkan-loader
    systemd
    xorg.libX11
    xorg.libxcb
    xorg.libXcursor
    xorg.libXi
  ];
  # The greeter is started by name from a service, with none of a login shell's
  # environment. Everything the wrapper reaches for has to be on the PATH the
  # wrapper itself carries.
  # LineXinBar's compositor, and only the compositor: `lxb-compositor`
  # is that project's compositor packaged apart from its shell, so a display
  # manager that needs a seat to draw a login screen on does not thereby depend
  # on a desktop. The wrapper prefixes rather than replaces the PATH, so a
  # machine whose system profile has its own `lxb` is found first.
  runtimePrograms = [
    dbus
    lxb-compositor
    systemd
  ];
in
rustPlatform.buildRustPackage {
  pname = "cedm";
  version = lib.removeSuffix "\n" (builtins.readFile ../VERSION);
  src = cleanSrc;

  cargoLock.lockFile = "${cleanSrc}/Cargo.lock";

  strictDeps = true;
  nativeBuildInputs = [
    pkg-config
    makeWrapper
    patchelf
    addDriverRunpath
  ];
  buildInputs = [
    wayland
    libxkbcommon
    libglvnd
    vulkan-loader
    systemd
    xorg.libX11
    xorg.libxcb
    xorg.libXcursor
    xorg.libXi
  ];

  postInstall = ''
    install -Dm0755 packaging/files/cedm-greeter-session \
      "$out/bin/cedm-greeter-session"

    # The authenticated session's wrapper, beside the greeter's because the
    # greeter's resolves it relative to its own path rather than to /usr/bin.
    install -Dm0755 packaging/files/cedm-session "$out/bin/cedm-session"

    # The udev rule is the one piece that cannot live in the store alone: it is
    # picked up by services.udev.packages, which the module beside this file
    # sets. Without it the Steam Controller's hidraw node stays root-only and
    # the one pad the greeter was written for is the one it cannot read.
    install -Dm0644 packaging/files/71-cedm-steam-controller.rules \
      "$out/lib/udev/rules.d/71-cedm-steam-controller.rules"

    # No unit and no sysusers fragment: on NixOS the login screen is
    # services.greetd, and the account, the VT and the service come from the
    # module rather than from files copied into /etc. The greetd configuration
    # is installed as an example for the same reason.
    install -Dm0644 packaging/files/greetd.toml \
      "$out/share/doc/cedm/greetd.example.toml"
    install -Dm0644 packaging/files/polkit-power.rules.example \
      "$out/share/doc/cedm/polkit-power.rules.example"

    install -Dm0644 LICENSE \
      "$out/share/licenses/cedm/GPL-3.0-only.txt"
    install -Dm0644 assets/fonts/LICENSE.txt \
      "$out/share/licenses/cedm/Roboto-Apache-2.0.txt"
    install -Dm0644 README.md \
      "$out/share/doc/cedm/README.md"
    install -Dm0644 contrib/config.toml.example \
      "$out/share/doc/cedm/config.example.toml"
    install -Dm0644 contrib/seamless/README.md \
      "$out/share/doc/cedm/seamless-login.md"

    patchShebangs "$out/bin/cedm-greeter-session" "$out/bin/cedm-session"
  '';

  postFixup = ''
    addDriverRunpath "$out/bin/cedm"
    patchelf --add-rpath "${lib.makeLibraryPath runtimeLibraries}" \
      "$out/bin/cedm"
    wrapProgram "$out/bin/cedm-greeter-session" \
      --prefix PATH : "$out/bin:${lib.makeBinPath runtimePrograms}"
    wrapProgram "$out/bin/cedm-session" \
      --prefix PATH : "$out/bin:${lib.makeBinPath runtimePrograms}"
  '';

  meta = {
    description = "Controller-first graphical display manager for console and desktop sessions";
    homepage = "https://github.com/petexy/ConsoleExperienceDesktopManager";
    license = with lib.licenses; [ gpl3Only asl20 ];
    mainProgram = "cedm";
    platforms = lib.platforms.linux;
  };
}
