# modules/os/policy.nix — PAM, polkit and logind posture (implementation-path §2 (iii), D2).
# The table in docs/architecture/multi-user.md §3.1 and the static passwordless posture of
# first-run-onboarding.md §5.3, as code. Everything here is static configuration: nothing
# detects "the account has no password" at runtime. The headset is a regular Linux PC: where
# upstream already has a default, this file does not override it.
#
#   greeter / autologin / lock : nullok (a passwordless account logs in), faillock
#   sudo, polkit                : STANDARD — administration requires a password; passwd is the gate
#   sshd                        : UPSTREAM DEFAULTS, on every profile (modules/os/default.nix):
#                                 password auth on every interface, PermitEmptyPasswords no —
#                                 so a passwordless account gets SSH after `passwd` or with a
#                                 declared key (profiles/dev.nix). Only faillock is added here.
#   polkit                      : exactly one Mura rule (greeter may add system Wi-Fi profiles)
#   logind                      : the compositor owns the power key (HandlePowerKey=ignore)
#   faillock                    : real lockout (preauth/authfail/account), counters on /persist
#
# NOT here, and why (first-run §5.3 rev 2.5, ADR 0017 rev 2.4 alternatives): no key-only /
# `Match Address` scoping of sshd (over-hardening beyond upstream; faillock guards every
# password); no `PermitEmptyPasswords` (OpenSSH's `none` probe then authenticates with an
# empty password in the parent while the real attempt runs in a forked helper, and
# `pam_setcred` replays the cached failure — every password login breaks once the account has
# a password; measured in the D2 VM test).
{ lib, config, ... }:
let
  cfg = config.mura.xr.session;
  pam = config.security.pam.package;
  faillockSo = "${pam}/lib/security/pam_faillock.so";

  # pam_faillock done properly: NixOS's `logFailures` adds a single argument-less line (the
  # authfail action alone), which records failures but never blocks a correct password.
  # Real lockout needs preauth before pam_unix, authfail after it, and the account step to
  # reset the counter on success. `unix` is `sufficient` in NixOS's auth stack, so a success
  # never reaches authfail; a failure falls through to it and dies.
  #
  # `conf=`: nixpkgs builds Linux-PAM with sysconfdir inside the store, so pam_faillock never
  # looks at /etc/security/faillock.conf on its own — without this argument the compiled
  # defaults (deny=3, 10 min, /run/faillock) silently apply.
  faillockConf = "/etc/security/faillock.conf";
  faillockDir = "/var/lib/mura/state/faillock"; # persist skeleton, modules/os/persist.nix
  faillockRules = service: {
    auth = {
      faillock-preauth = {
        control = "required";
        modulePath = faillockSo;
        args = [ "preauth" "conf=${faillockConf}" ];
        order = config.security.pam.services.${service}.rules.auth.unix.order - 10;
      };
      faillock-authfail = {
        control = "[default=die]";
        modulePath = faillockSo;
        args = [ "authfail" "conf=${faillockConf}" ];
        order = config.security.pam.services.${service}.rules.auth.unix.order + 10;
      };
    };
    account.faillock = {
      control = "required";
      modulePath = faillockSo;
      args = [ "conf=${faillockConf}" ];
      order = config.security.pam.services.${service}.rules.account.unix.order - 10;
    };
  };
in
{
  config = {
    ## PAM ---------------------------------------------------------------------------------
    security.pam.services = {
      # greetd substacks `login` in this nixpkgs (older revisions set allowNullPassword on
      # greetd itself); pin the posture where it takes effect. TTY login shares it.
      login = {
        allowNullPassword = true;
        rules = faillockRules "login";
      };
      # sshd keeps NixOS's stack (pam_unix, no nullok — OpenSSH refuses empty passwords anyway)
      # and gains the same faillock ladder as the greeter.
      sshd.rules = faillockRules "sshd";
      # sudo and polkit-1 are deliberately NOT configured: a passwordless account cannot
      # administer until it sets a password (first-run §5.3). `mura-lock` (authd) joins at D5.
    };

    # Counters on /persist so a reboot does not reset the ladder (multi-user.md §3); the
    # directory is part of the persist skeleton (modules/os/persist.nix).
    environment.etc."security/faillock.conf".text = ''
      dir = ${faillockDir}
      deny = ${toString cfg.faillock.deny}
      unlock_time = ${toString cfg.faillock.unlockSeconds}
      silent
    '';
    # The faillock CLI has only `--dir` (it reads no conf file either); point it at the real
    # tally directory so `faillock --user X --reset` does what an administrator expects.
    environment.shellAliases.faillock = "faillock --dir ${faillockDir}";

    ## polkit -----------------------------------------------------------------------------
    # The one Mura polkit rule (multi-user.md §3.1): GDM parity — a Wi-Fi network joined at
    # the greeter becomes a *system* connection the person who then logs in can use. Without
    # it NetworkManager scopes the profile to the greeter user and it is useless.
    security.polkit.extraConfig = lib.mkIf (cfg.greeter != "none") ''
      /* Mura: the greeter may add system-wide network connections (GDM's polkit-gdm.rules). */
      polkit.addRule(function(action, subject) {
        if (action.id == "org.freedesktop.NetworkManager.settings.modify.system" &&
            subject.user == "greeter" && subject.local && subject.active) {
          return polkit.Result.YES;
        }
      });
    '';

    ## logind -----------------------------------------------------------------------------
    # The compositor owns the power key through libinput (first-run §4.4; the Steam Deck's
    # powerbuttond arrangement, and what SteamOS ships on the Steam Frame). Volume keys were
    # never logind's.
    services.logind.settings.Login = {
      HandlePowerKey = "ignore";
      HandlePowerKeyLongPress = "ignore";
    };
  };
}
