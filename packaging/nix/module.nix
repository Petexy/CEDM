{ config, lib, pkgs, ... }:

let
  cfg = config.services.cedm;
in
{
  options.services.cedm = {
    enable = lib.mkEnableOption
      "Console Experience Desktop Manager as the machine's login screen";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { src = ../..; };
      defaultText = lib.literalExpression
        "pkgs.callPackage ./packaging/nix/package.nix { src = ./.; }";
      description = "CEDM package to install and run as the greeter.";
    };

    vt = lib.mkOption {
      type = lib.types.int;
      default = 1;
      description = ''
        The virtual terminal the greeter and the session it starts both use.
        Keeping them on one terminal is what removes a VT switch — and its mode
        set, and its console frame — from the middle of every login.
      '';
    };

    settings = lib.mkOption {
      type = lib.types.attrsOf lib.types.anything;
      default = { };
      example = lib.literalExpression ''
        {
          default_session = "lxb";
          power.shut_down = false;
        }
      '';
      description = ''
        Administrator policy, written to
        /etc/cedm/config.toml. Preferences only:
        a session named here still has to be discovered and validated locally
        before the greeter will offer it. Leave empty to accept the defaults,
        which remember successful choices and offer all three machine actions.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];

    # On NixOS the login screen is greetd, and its module already owns the
    # unit, the VT and the unprivileged `greeter` account. Writing CEDM's own
    # unit into /etc here would give the machine two display managers competing
    # for one terminal, which is the failure the unit's Conflicts= exists to
    # prevent everywhere else.
    services.greetd = {
      enable = true;
      vt = cfg.vt;
      settings.default_session = {
        command = "${cfg.package}/bin/cedm-greeter-session";
      };
    };

    environment.etc."cedm/config.toml" =
      lib.mkIf (cfg.settings != { }) {
        source = (pkgs.formats.toml { }).generate "cedm-config.toml" cfg.settings;
      };

    services.udev.packages = [ cfg.package ];

    # The greeter reads /var/lib/cedm/state.toml
    # and must not be able to write it: accent data the privileged broker
    # publishes would otherwise be forgeable by the unprivileged process that
    # displays it.
    #
    # `published/` is the other direction and the other permissions: each
    # account writes its own accent there as its session starts, because a
    # greeter cannot read a home directory. Search without read, and sticky, so
    # that a file can be created and never removed or overwritten by anybody
    # but its owner. See packaging/files/tmpfiles.conf for the whole of it.
    systemd.tmpfiles.rules = [
      "d /var/lib/cedm 0755 root root -"
      "d /var/lib/cedm/published 1733 root root -"
    ];

    services.dbus.enable = true;
    security.polkit.enable = lib.mkDefault true;
    hardware.graphics.enable = lib.mkDefault true;

    # Account pictures. The greeter reads the copies accounts-daemon publishes
    # under /var/lib/AccountsService/icons and never goes near a home directory.
    services.accounts-daemon.enable = lib.mkDefault true;
  };
}
