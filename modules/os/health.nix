# modules/os/health.nix — the XR preflight, the crash-loop ladder and the session-ready gate
# (implementation-path §3a-bis B1b and §3a B9; D6). Device-independent; the uefi-rauc family
# adds the slot side (`+N` boot counting, mark-good) on top of `boot-complete.target`.
#
#   mura-preflight.service : P1–P7 before the greeter/autologin; exit 0 / 1 soft / 2 hard;
#                            /run/mura/preflight.json. greetd Requires= it, so a hard failure
#                            leaves this boot without a greeter or session.
#   mura-crashloop.service : OnFailure= of the preflight — increments the durable counter in
#                            state/health/crashloop (the probe itself never writes persistent
#                            state); at mura.health.crashLoopThreshold it takes the step
#                            systemd's boot counting cannot (a persistent state fault in an
#                            already-good slot boots dark forever — research/56 §3): reboot into
#                            the Mura recovery environment (recovery.nix; the family's recovery
#                            entry), or, where no entry exists yet, mura-recovery.target.
#   mura-recovery.target   : the failure-feedback state — sshd, gadget and hotspot up, nothing
#                            graphical; identical to what a single hard failure produces, kept
#                            as the fallback step. A runtime or driver failure never leaves a
#                            dark headset without a way in; the first hard failure already puts
#                            what failed and how to reach the device on the panels (recovery.nix,
#                            plymouth).
#   mura-readiness.service : the G3-minimum blessing tier — the compositor unit (appliance) or
#                            greetd + its greeter (multi-user) active and /persist/mura writable:
#                            a target reached, the shape every shipping system blesses on
#                            (systemd boot-complete.target, RAUC mark-good after multi-user,
#                            mobile-nixos boot-control; research/56 §4, ruled 2026-09-25 — the
#                            D6 stability window had no comparable). A crash after blessing is a
#                            session matter (the compositor unit's StartLimit), not a boot
#                            failure. Then the counter is reset; RequiredBy= boot-complete.target,
#                            so systemd-bless-boot (and the family's mark-good) wait for it.
{ lib, config, pkgs, ... }:
let
  cfg = config.mura;
  health = "/var/lib/mura/state/health";
  counter = "${health}/crashloop";

  # What the probe needs from the contract, as data — no Nix logic in the program.
  preflightConfig = pkgs.writeText "mura-preflight.json" (builtins.toJSON {
    codename = cfg.device.codename;
    calibrationPaths = cfg.xr.calibration.paths;
    displayBackend = cfg.xr.compositor.backend;
    trackingSimulated = (cfg.xr.environment.SIMULATED_ENABLE or "") == "true";
    selectKey = cfg.hardware.input.hmdButtons.${cfg.hardware.input.selectRole} or null;
    deviceWaitSeconds = cfg.health.deviceWaitSeconds;
    monadoRuntime = cfg.xr.runtime == "monado";
    persistClasses = [ "factory" "identity" "enrollment" "pairing" "state" ];
    vulkaninfo = "${pkgs.vulkan-tools}/bin/vulkaninfo";
    monadoCli = "${pkgs.monado}/bin/monado-cli";
  });

  # The probe itself: pkgs/mura-preflight (Rust; no interpreter on the boot path). It reads
  # the JSON above and nothing else; the two helpers it executes come in as paths.
  preflight = pkgs.mura.preflight;

  # Blessing tier (§3a): profile-specific target — the autologin user's compositor unit on
  # the appliance profile, the greeter on the multi-user profile. Never waits for a login.
  readiness = pkgs.writeShellApplication {
    name = "mura-readiness";
    runtimeInputs = [ pkgs.systemd pkgs.coreutils pkgs.procps ];
    text = ''
      deadline=$(( $(date +%s) + 300 ))
      while :; do
        now=$(date +%s)
        [ "$now" -lt "$deadline" ] || { echo "readiness: target not reached within 300 s"; exit 1; }
        ok=1
        # /persist writable
        touch /var/lib/mura/state/health/.readiness-probe 2>/dev/null && rm -f /var/lib/mura/state/health/.readiness-probe || ok=0
        ${if cfg.xr.session.autoLogin != null then ''
          # appliance: the autologin user's compositor unit is active (session.nix, D4 rev 3)
          systemctl --user -M ${cfg.xr.session.autoLogin}@ is-active --quiet mura-compositor.service 2>/dev/null || ok=0
        '' else ''
          # multi-user: greetd is up and its greeter process is alive
          systemctl is-active --quiet greetd.service || ok=0
          pgrep -u greeter -f . >/dev/null 2>&1 || ok=0
        ''}
        if [ "$ok" = 1 ]; then
          echo "readiness: blessing tier reached (session target active, /persist writable)"
          echo 0 > ${counter}.tmp && mv ${counter}.tmp ${counter}
          exit 0
        fi
        sleep 2
      done
    '';
  };
in
{
  config = lib.mkIf (cfg.xr.shell != "none") {
    # The counter's home is state/health/ in the persist skeleton (modules/os/persist.nix).

    ## Preflight ------------------------------------------------------------------------
    systemd.services.mura-preflight = {
      description = "Mura XR preflight (implementation-path §3a-bis: P1–P7 before the greeter)";
      wantedBy = [ "graphical.target" ];
      after = [ "local-fs.target" "systemd-udev-settle.service" "mura-persist-setup.service" "systemd-tmpfiles-setup.service" ];
      wants = [ "systemd-udev-settle.service" ];
      before = [ "greetd.service" ];
      onFailure = [ "mura-crashloop.service" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${lib.getExe preflight} ${preflightConfig}";
        SuccessExitStatus = "1"; # a soft failure starts the greeter and is exposed in the a11y menu
        TimeoutStartSec = "${toString (cfg.health.deviceWaitSeconds * 3 + 60)}s";
      };
    };
    # A hard failure (exit 2) fails the unit; greetd requires it, so this boot has no greeter.
    systemd.services.greetd = {
      requires = [ "mura-preflight.service" ];
      after = [ "mura-preflight.service" ];
    };

    ## Crash-loop ladder ---------------------------------------------------------------
    systemd.services.mura-crashloop = {
      description = "Mura crash-loop counter (increments on a hard preflight failure)";
      serviceConfig.Type = "oneshot";
      script = ''
        n=0; [ -f ${counter} ] && n=$(cat ${counter} 2>/dev/null || echo 0)
        n=$(( n + 1 ))
        echo "$n" > ${counter}.tmp && mv ${counter}.tmp ${counter}
        echo "preflight hard failure; consecutive count now $n (threshold ${toString cfg.health.crashLoopThreshold})"
        if [ "$n" -ge ${toString cfg.health.crashLoopThreshold} ]; then
          echo "threshold reached: entering mura-recovery.target"
          systemctl start --no-block mura-recovery.target
        fi
      '';
    };
    systemd.targets.mura-recovery = {
      description = "Mura recovery: diagnostic target — SSH and serial stay up, nothing graphical";
      wants = [ "sshd.service" ]; # the serial getty is the device's own unit and stays up regardless
      conflicts = [ "greetd.service" ];
      unitConfig.AllowIsolate = true;
    };

    ## Readiness → boot-complete.target ---------------------------------------------------
    systemd.services.mura-readiness = {
      description = "Mura session-ready gate (blessing tier → boot-complete.target)";
      wantedBy = [ "multi-user.target" ];
      after = [ "greetd.service" "mura-preflight.service" ];
      requires = [ "mura-preflight.service" ];
      before = [ "boot-complete.target" ];
      requiredBy = [ "boot-complete.target" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = lib.getExe readiness;
        TimeoutStartSec = "360s";
      };
    };
    # boot-complete.target is upstream's extension point: systemd-bless-boot.service (and the
    # family's mark-good) order after it. Pull it in so it is reached on every profile, with
    # or without boot counting on the ESP.
    systemd.targets.boot-complete.wantedBy = [ "multi-user.target" ];

    environment.systemPackages = [ preflight ];
  };
}
