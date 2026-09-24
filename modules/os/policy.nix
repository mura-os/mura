# modules/os/policy.nix — PAM, sshd, polkit and logind posture (implementation-path §2 (iii),
# D2). The table in docs/architecture/multi-user.md §3.1 and the static passwordless posture
# of first-run-onboarding.md §5.3, as code. Everything here is static configuration: nothing
# detects "the account has no password" at runtime.
#
#   greeter / autologin / lock : nullok (a passwordless account logs in), faillock
#   sudo, polkit                : STANDARD — administration requires a password; passwd is the gate
#   sshd                        : key-only everywhere except the USB-gadget subnet, where password
#                                 auth is allowed — the cable is TTY trust (never PermitEmptyPasswords)
#   polkit                      : exactly one Mura rule (greeter may add system Wi-Fi profiles)
#   logind                      : the compositor owns the power key (HandlePowerKey=ignore)
#   faillock                    : real lockout (preauth/authfail/account), counters on /persist
{ lib, config, ... }:
let
  cfg = config.mura.xr.session;
  pam = config.security.pam.package;
  faillockSo = "${pam}/lib/security/pam_faillock.so";

  # The USB Ethernet gadget's subnet (first-run-onboarding.md §5.4; D3 owns the link itself).
  usbGadgetSubnet = "172.16.42.0/24";

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
      sshd = {
        # Empty passwords are NOT accepted over SSH (see the sshd section) — no nullok here.
        allowNullPassword = false;
        # NixOS drops pam_unix from sshd's stack when the *global* PasswordAuthentication is
        # off; the scoping lives in sshd's Match block, so the PAM stack must keep pam_unix.
        unixAuth = lib.mkForce true;
        rules = faillockRules "sshd";
      };
      # sudo and polkit-1 are deliberately NOT configured: a passwordless account cannot
      # administer until it sets a password (first-run §5.3). `mura-lock` (authd) joins at D5,
      # `cockpit` at D3.
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

    ## sshd -------------------------------------------------------------------------------
    # Key-only on every interface — both PasswordAuthentication and KbdInteractiveAuthentication
    # (with UsePAM, keyboard-interactive IS PAM password auth) — except the USB-gadget subnet,
    # where the cable is the authorisation and a password is accepted. Static: it also keeps a
    # short numeric password off the LAN.
    #
    # NOT `PermitEmptyPasswords` (D2 finding, first-run §5.3): with it, sshd's initial `none`
    # method runs a real PAM authenticate with an empty password in the parent process; the
    # actual authentication then runs in a *forked* helper, so the parent's PAM handle keeps
    # the failed probe as its cached chain and `pam_setcred` replays the failure — every SSH
    # password login breaks the moment the account HAS a password. A passwordless account's
    # first contact over the cable is Cockpit (PAM nullok, no such probe) or the session;
    # SSH follows `passwd` — or an authorized key, which is what a self-builder declares.
    services.openssh.settings = {
      PasswordAuthentication = false;
      KbdInteractiveAuthentication = false;
      PermitEmptyPasswords = false;
    };
    services.openssh.extraConfig = ''
      Match Address ${usbGadgetSubnet}
        PasswordAuthentication yes
        KbdInteractiveAuthentication yes
    '';

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
