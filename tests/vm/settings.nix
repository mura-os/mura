# The settings daemon (modules/os/settings.nix, pkgs/mura-settingsd; specs/settings-schema.md §9,
# specs/settings-daemon.md §9) on the default-image fixture: `mura` autologins into the sway
# stand-in, so a user manager and a session bus exist. Proves the contract's conformance items
# 1–9 (item 9 at the unit level: the migration table is empty at rev 1) and the daemon's 10–13:
# D-Bus activation on first use, the generation hook through a NixOS specialisation switch, the
# closure fence, and the measured footprint.
# (Nix indented string: never write two consecutive single quotes inside the script.)
{ pkgs }:
(import ./lib.nix { inherit pkgs; }) {
  name = "mura-vm-settings";
  profileModules = [ ../../profiles/default.nix ];
  extraModules = [
    ({ lib, ... }: {
      # TEST-ONLY: one locked key (§7, item 8) and a specialisation whose default for a runtime
      # key moves (§4 rule 1, item 1) — the shape of a rebuild that changes an image default.
      mura.settings.locks = [ "xr.passthrough.upperLimbVisibility" ];
      specialisation.moved.configuration = {
        mura.xr.passthrough.latencyMode = lib.mkForce "high-quality";
      };
    })
  ];

  testScript = ''
    import re

    # transient user units get the manager's PATH, not the login shell's: absolute paths
    S = "${pkgs.mura.settingsd}/bin/mura-settings"
    LIAR = "${pkgs.mura.settingsd}/bin/mura-settingsd-liar"
    SH = "${pkgs.runtimeShell}"
    BUSCTL = "${pkgs.systemd}/bin/busctl"

    machine.start()
    machine.wait_for_unit("multi-user.target")
    userctl = "systemctl --user -M mura@ "
    machine.wait_until_succeeds(userctl + "is-active graphical-session.target", timeout=90)

    def user(cmd, check=True):
        # run in mura's session (its manager environment: session bus, XDG dirs)
        full = f"systemd-run --quiet --user -M mura@ --pipe --wait --collect {cmd} 2>&1"
        return machine.succeed(full) if check else machine.execute(full)

    def get(key):
        out = user(f"{S} get {key}").rstrip("\n")
        value, prov = out.split("\t")
        return value, prov

    with subtest("settings: the artifact is in the closure, root-owned, and the daemon is not running until first use (item 10)"):
        art = machine.succeed("readlink -f /etc/mura/settings-schema.json").strip()
        assert art.startswith("/nix/store/"), art
        machine.succeed("test $(stat -c %U /etc/mura/settings-schema.json) = root")
        keys = machine.succeed("mura-settings --direct list").strip()
        assert "xr.passthrough.latencyMode\tlow-latency\tdefault" in keys, keys
        machine.fail(userctl + "is-active mura-settingsd.service")
        assert get("xr.passthrough.latencyMode") == ("low-latency", "default")
        machine.succeed(userctl + "is-active mura-settingsd.service")   # D-Bus activated it

    with subtest("settings: Set equal to the default creates the override (item 2); Reset returns to following (§3)"):
        user(f"{S} set xr.passthrough.latencyMode low-latency")
        assert get("xr.passthrough.latencyMode") == ("low-latency", "user")
        store = machine.succeed("cat /home/mura/.config/mura/settings/xr.passthrough.json")
        assert '"latencyMode": "low-latency"' in store and '"schemaVersion": 1' in store, store
        assert "upperLimbVisibility" not in store   # sparse: only explicit overrides
        user(f"{S} reset xr.passthrough.latencyMode")
        assert get("xr.passthrough.latencyMode") == ("low-latency", "default")
        machine.fail("grep -q latencyMode /home/mura/.config/mura/settings/xr.passthrough.json")

    with subtest("settings: research/73 keys — a preference round-trips, a layered key's default is the contract's, a build fact is locked"):
        keys = machine.succeed("mura-settings --direct list").strip().splitlines()
        assert len(keys) >= 90, len(keys)                                         # 3 before research/73
        assert get("input.cursor.ray") == ("both", "default")
        user(f"{S} set input.cursor.ray image")
        assert get("input.cursor.ray") == ("image", "user")
        user(f"{S} reset input.cursor.ray")
        assert get("input.cursor.ray") == ("both", "default")
        assert get("input.hand.pinch.close") == ("0.75", "default")               # layered on hardware.input.hand.pinch.close
        user(f"{S} set input.hand.pinch.close 0.8")
        assert get("input.hand.pinch.close") == ("0.8", "user")
        assert get("hardware.input.hand.pinch.close") == ("0.75", "locked")       # the calibration itself is a locked build fact
        rc, out = user(f"{S} set hardware.input.hand.pinch.close 0.9", check=False)
        assert rc == 1 and ("Locked" in out or "Immutable" in out), (rc, out)
        rc, out = user(f"{S} set input.cursor.angle_deg 9", check=False)
        assert rc == 1 and "Range" in out, (rc, out)
        user(f"{S} reset input.hand.pinch.close")

    with subtest("settings: immutable and locked writes are refused and touch nothing (items 3, §7); range and type too"):
        rc, out = user(f"{S} set hardware.ipd.meters 0.064", check=False)
        assert rc == 1 and "Immutable" in out, (rc, out)
        machine.fail("test -e /home/mura/.config/mura/settings/hardware.ipd.json")
        rc, out = user(f"{S} set xr.passthrough.upperLimbVisibility hidden", check=False)
        assert rc == 1 and "Locked" in out, (rc, out)
        assert get("xr.passthrough.upperLimbVisibility") == ("automatic", "locked")
        rc, out = user(f"{S} set xr.passthrough.latencyMode medium", check=False)
        assert rc == 1 and "Range" in out, (rc, out)
        rc, out = user(f"{S} get nope.key", check=False)
        assert rc == 2, (rc, out)

    with subtest("settings: template instances — create by Set, enumerate, delete exactly one (item 6)"):
        assert user(f"{S} instances places.entry").strip() == ""
        user(f"{S} set places.entry:desk.enabled false")
        user(f"{S} set places.entry:sofa.launch firefox")
        assert user(f"{S} instances places.entry").split() == ["desk", "sofa"]
        assert get("places.entry:desk.launch") == ("", "default")
        user(f"{S} delete-instance places.entry desk")
        assert user(f"{S} instances places.entry").split() == ["sofa"]
        machine.fail("test -e /home/mura/.config/mura/settings/places.entry:desk.json")

    with subtest("settings: every accepted change is one Changed with value and provenance (§8)"):
        machine.succeed("rm -f /tmp/mon.txt")
        machine.succeed(f"systemd-run --quiet --user -M mura@ --unit=mon --collect {SH} -c '{BUSCTL} --user monitor --match interface=org.mura.Settings1 > /tmp/mon.txt 2>&1'")
        machine.succeed("sleep 1")
        user(f"{S} set xr.passthrough.latencyMode high-quality")
        user(f"{S} set xr.passthrough.latencyMode high-quality")   # same value, same provenance: silent
        user(f"{S} reset xr.passthrough.latencyMode")
        machine.wait_until_succeeds("grep -c Member=Changed /tmp/mon.txt | grep -qx 2", timeout=10)
        mon = machine.succeed("cat /tmp/mon.txt")
        assert 'STRING "high-quality"' in mon and 'STRING "user"' in mon and 'STRING "default"' in mon, mon
        machine.succeed(userctl + "stop mon")

    with subtest("settings: an invalid stored value resolves to the default, is reported, and the file is untouched (item 5)"):
        machine.succeed(userctl + "stop mura-settingsd.service")
        machine.succeed("""cat > /home/mura/.config/mura/settings/xr.passthrough.json <<EOF
    {"schema":"xr.passthrough","instance":null,"schemaVersion":1,"generation":"old","values":{"latencyMode":"turbo"}}
    EOF
    chown mura:users /home/mura/.config/mura/settings/xr.passthrough.json""")
        before = machine.succeed("sha256sum /home/mura/.config/mura/settings/xr.passthrough.json")
        assert get("xr.passthrough.latencyMode") == ("low-latency", "invalid")
        after = machine.succeed("sha256sum /home/mura/.config/mura/settings/xr.passthrough.json")
        assert before == after
        user(f"{S} reset xr.passthrough.latencyMode")   # the user clears it explicitly
        assert get("xr.passthrough.latencyMode") == ("low-latency", "default")

    with subtest("settings: kill -9 mid-burst — no torn file, identical resolution on restart (item 4)"):
        machine.succeed(f"systemd-run --quiet --user -M mura@ --unit=burst --collect {SH} -c 'for i in $(seq 1 200); do {S} set places.entry:sofa.launch app$i >/dev/null 2>&1; done'")
        machine.succeed("sleep 0.3; pkill -9 -u mura -x mura-settingsd")
        machine.wait_until_fails(userctl + "is-active burst.service", timeout=60)
        machine.fail("ls /home/mura/.config/mura/settings/*.tmp 2>/dev/null")
        direct = machine.succeed("cd /home/mura && XDG_CONFIG_HOME=/home/mura/.config XDG_STATE_HOME=/home/mura/.local/state mura-settings --direct get places.entry:sofa.launch").strip()
        assert re.match(r"^app\d+\tuser$", direct), direct
        bus = user(f"{S} get places.entry:sofa.launch").strip()   # restarted by the bus
        assert bus == direct, (bus, direct)

    with subtest("settings: a locked-key consumer reads the artifact, not the bus — even when the bus lies (item 8)"):
        machine.succeed(userctl + "stop mura-settingsd.service")
        machine.succeed(f"systemd-run --quiet --user -M mura@ --unit=liar --collect {LIAR}")
        machine.wait_until_succeeds("busctl --user -M mura@ status org.mura.Settings1 >/dev/null 2>&1", timeout=10)
        lied = user(f"{S} get xr.passthrough.upperLimbVisibility").strip()
        assert lied == "999\tuser", lied                       # a bus-trusting consumer is fooled
        truth = machine.succeed("cd /home/mura && XDG_CONFIG_HOME=/home/mura/.config mura-settings --direct get xr.passthrough.upperLimbVisibility").strip()
        assert truth == "automatic\tlocked", truth              # §7: the artifact is the authority
        machine.succeed(userctl + "stop liar")
        assert get("xr.passthrough.latencyMode") == ("low-latency", "default")   # the real daemon is back

    with subtest("settings: generation switch — a moved default advances, a pinned value survives, the session is told (items 1, 11)"):
        user(f"{S} set places.entry:sofa.launch pinned")
        gen1 = user(f"{S} generation").strip()
        machine.succeed("rm -f /tmp/mon.txt")
        machine.succeed(f"systemd-run --quiet --user -M mura@ --unit=mon2 --collect {SH} -c '{BUSCTL} --user monitor --match interface=org.mura.Settings1 > /tmp/mon.txt 2>&1'")
        machine.succeed("sleep 1")
        machine.succeed("/run/current-system/specialisation/moved/bin/switch-to-configuration test 2>&1 | tail -5")
        gen2 = user(f"{S} generation").strip()
        assert gen1 != gen2, (gen1, gen2)
        assert get("xr.passthrough.latencyMode") == ("high-quality", "default")   # untouched key follows the new default
        assert get("places.entry:sofa.launch") == ("pinned", "user")             # explicit value survives
        machine.wait_until_succeeds("grep -q Member=GenerationChanged /tmp/mon.txt", timeout=30)
        mon = machine.succeed("cat /tmp/mon.txt")
        assert "xr.passthrough.latencyMode" in mon and 'STRING "high-quality"' in mon, mon
        machine.succeed(userctl + "stop mon2")

    with subtest("settings: footprint (item 13) and no interpreter in the daemon (item 12: tests/closure.nix fences it)"):
        pid = machine.succeed("pgrep -u mura -x mura-settingsd").strip().split()[0]
        rss = int(machine.succeed(f"awk '/VmRSS/ {{print $2}}' /proc/{pid}/status").strip())
        threads = int(machine.succeed(f"ls /proc/{pid}/task | wc -l").strip())
        print(f"mura-settingsd: RSS {rss} kB, {threads} threads")
        assert rss < 12000, rss          # research/58 §11 measured ~3.5 MB; a wide fence
        assert threads <= 6, threads     # async-io, not a multi-thread runtime
  '';
}
