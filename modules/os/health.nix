# modules/os/health.nix — the XR preflight, the crash-loop ladder and the session-ready gate
# (implementation-path §3a-bis B1b and §3a B9; D6). Device-independent; the uefi-rauc family
# adds the slot side (`+N` boot counting, mark-good) on top of `boot-complete.target`.
#
#   mura-preflight.service : P1–P7 before the greeter/autologin; exit 0 / 1 soft / 2 hard;
#                            /run/mura/preflight.json. greetd Requires= it, so a hard failure
#                            leaves this boot without a greeter or session.
#   mura-crashloop.service : OnFailure= of the preflight — increments the durable counter in
#                            state/health/crashloop (the probe itself never writes persistent
#                            state); at mura.health.crashLoopThreshold it starts
#                            mura-recovery.target.
#   mura-recovery.target   : the diagnostic target — sshd + the serial getty stay, nothing
#                            graphical. A runtime or driver failure never leaves a dark headset
#                            without a way in.
#   mura-readiness.service : the G3-minimum blessing tier — compositor (appliance) or greeter
#                            (multi-user) stable for readinessStabilitySeconds, /persist/mura
#                            writable, then the counter is reset; RequiredBy= boot-complete.target,
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
  });

  preflight = pkgs.writers.writePython3Bin "mura-preflight" { flakeIgnore = [ "E501" ]; } ''
    """mura-preflight — implementation-path §3a-bis. Exit 0 all pass, 1 a soft check failed,
    2 a hard check failed. Writes /run/mura/preflight.json. Never writes persistent state."""
    import glob
    import json
    import os
    import subprocess
    import sys
    import time

    CFG = json.load(open("${preflightConfig}"))
    results = []


    def check(name, ok, hard, detail=""):
        results.append({"check": name, "pass": bool(ok), "class": "hard" if hard else "soft", "detail": detail})


    # P1 persist: writable, class dirs present
    try:
        probe = "/var/lib/mura/.preflight-probe"
        with open(probe, "w") as f:
            f.write("ok")
        os.unlink(probe)
        missing = [c for c in CFG["persistClasses"] if not os.path.isdir(f"/var/lib/mura/{c}")]
        check("P1 persist", not missing, True, f"missing: {missing}" if missing else "writable; classes present")
    except OSError as e:
        check("P1 persist", False, True, str(e))

    # P2 factory calibration: every declared path exists and is non-empty
    paths = CFG["calibrationPaths"]
    if paths:
        bad = {k: p for k, p in paths.items() if not (os.path.exists(p) and os.path.getsize(p) > 0)}
        check("P2 factory calibration", not bad, True, f"missing/empty: {bad}" if bad else f"{len(paths)} file(s) present; version check is the runtime's")
    else:
        check("P2 factory calibration", True, True, "none declared (calibration.paths empty)")

    # P3 display path: a connected DRM connector (vk-display) or any DRM card (window backend)
    connected = [c for c in glob.glob("/sys/class/drm/card*-*/status") if open(c).read().strip() == "connected"]
    cards = glob.glob("/sys/class/drm/card[0-9]*")
    if CFG["displayBackend"] == "window":
        check("P3 display path", bool(cards), True, f"{len(cards)} DRM card(s); window backend")
    else:
        check("P3 display path", bool(connected), True, f"connected connectors: {[c.split('/')[-2] for c in connected]}")

    # P4 Vulkan: a physical device the runtime can create
    try:
        out = subprocess.run(["${pkgs.vulkan-tools}/bin/vulkaninfo", "--summary"], capture_output=True, text=True, timeout=30)
        devs = [ln.strip() for ln in out.stdout.splitlines() if "deviceName" in ln]
        check("P4 vulkan", out.returncode == 0 and devs, True, "; ".join(devs) or out.stderr[-200:])
    except Exception as e:  # noqa: BLE001
        check("P4 vulkan", False, True, str(e))

    # P5 tracking nodes: an IIO accel+gyro within the device wait, unless the runtime simulates
    if CFG["trackingSimulated"]:
        check("P5 tracking nodes", True, True, "simulated tracking (SIMULATED_ENABLE)")
    else:
        deadline = time.monotonic() + CFG["deviceWaitSeconds"]
        found = []
        while time.monotonic() < deadline and not found:
            found = [d for d in glob.glob("/sys/bus/iio/devices/iio:device*") if glob.glob(d + "/in_accel_*_raw") and glob.glob(d + "/in_anglvel_*_raw")]
            if not found:
                time.sleep(1)
        check("P5 tracking nodes", bool(found), True, f"iio: {[os.path.basename(d) for d in found]}")

    # P6 Monado probe: drivers initialise within the wait (first-frame is the blessing tier's).
    # monado reads HOME/XDG_* for its config and dies on a NULL env (found at D6): give it a
    # runtime-only home.
    if CFG["monadoRuntime"]:
        os.makedirs("/run/mura/preflight", mode=0o700, exist_ok=True)
        try:
            out = subprocess.run(["${pkgs.monado}/bin/monado-cli", "probe"], capture_output=True, text=True, timeout=CFG["deviceWaitSeconds"], env={**os.environ, "SIMULATED_ENABLE": "true" if CFG["trackingSimulated"] else "false", "XRT_NO_STDIN": "1", "HOME": "/run/mura/preflight", "XDG_CONFIG_HOME": "/run/mura/preflight", "XDG_CACHE_HOME": "/run/mura/preflight", "XDG_RUNTIME_DIR": "/run/mura/preflight"})
            check("P6 monado probe", out.returncode == 0, True, (out.stdout + out.stderr).strip().splitlines()[-1:][0] if (out.stdout + out.stderr).strip() else f"rc={out.returncode}")
        except Exception as e:  # noqa: BLE001
            check("P6 monado probe", False, True, str(e))
    else:
        check("P6 monado probe", True, True, "runtime is not monado")

    # P7 input floor: an evdev device exposing the select key, or a keyboard (soft)
    KEYCODES = {"KEY_POWER": 116, "KEY_VOLUMEUP": 115, "KEY_VOLUMEDOWN": 114, "KEY_SELECT": 353, "KEY_ENTER": 28}
    want = KEYCODES.get(CFG["selectKey"] or "", None)
    have = False
    try:
        for dev in open("/proc/bus/input/devices").read().split("\n\n"):
            for line in dev.splitlines():
                if line.startswith("B: KEY="):
                    words = line[7:].split()
                    bits = int("".join(w.zfill(16) for w in words), 16)
                    if (want is not None and (bits >> want) & 1) or (bits >> KEYCODES["KEY_ENTER"]) & 1:
                        have = True
    except OSError:
        pass
    check("P7 input floor", have, False, f"select={CFG['selectKey']} or a keyboard")

    os.makedirs("/run/mura", exist_ok=True)
    hard_fail = any(not r["pass"] and r["class"] == "hard" for r in results)
    soft_fail = any(not r["pass"] and r["class"] == "soft" for r in results)
    rc = 2 if hard_fail else 1 if soft_fail else 0
    with open("/run/mura/preflight.json.tmp", "w") as f:
        json.dump({"codename": CFG["codename"], "result": rc, "checks": results}, f, indent=1)
    os.replace("/run/mura/preflight.json.tmp", "/run/mura/preflight.json")
    for r in results:
        print(("PASS " if r["pass"] else ("FAIL " if r["class"] == "hard" else "WARN ")) + r["check"] + ": " + r["detail"])
    sys.exit(rc)
  '';

  # Blessing tier (§3a): profile-specific stability — the autologin user's compositor unit on
  # the appliance profile, the greeter on the multi-user profile. Never waits for a login.
  readiness = pkgs.writeShellApplication {
    name = "mura-readiness";
    runtimeInputs = [ pkgs.systemd pkgs.coreutils pkgs.procps ];
    text = ''
      stability=${toString cfg.health.readinessStabilitySeconds}
      deadline=$(( $(date +%s) + 300 ))
      stable_since=""
      while :; do
        now=$(date +%s)
        [ "$now" -lt "$deadline" ] || { echo "readiness: not stable within 300 s"; exit 1; }
        ok=1
        # /persist writable
        touch /var/lib/mura/state/health/.readiness-probe 2>/dev/null && rm -f /var/lib/mura/state/health/.readiness-probe || ok=0
        ${if cfg.xr.session.autoLogin != null then ''
          # appliance: the autologin user's compositor unit is active (uwsm's wayland-wm@<id>)
          systemctl --user -M ${cfg.xr.session.autoLogin}@ is-active --quiet 'wayland-wm@*.service' 2>/dev/null || ok=0
        '' else ''
          # multi-user: greetd is up and its greeter process is alive
          systemctl is-active --quiet greetd.service || ok=0
          pgrep -u greeter -f . >/dev/null 2>&1 || ok=0
        ''}
        if [ "$ok" = 1 ]; then
          [ -n "$stable_since" ] || stable_since=$now
          if [ $(( now - stable_since )) -ge "$stability" ]; then
            echo "readiness: stable for $stability s; blessing tier reached"
            echo 0 > ${counter}.tmp && mv ${counter}.tmp ${counter}
            exit 0
          fi
        else
          stable_since=""
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
        ExecStart = lib.getExe preflight;
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
