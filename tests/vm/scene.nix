# The scene fixture: a PICTURE of zxr's composited scene in the sandboxed VM (research/78 §9 F14).
#
# The login fixtures run Monado's null compositor and prove the XR chain through journals and
# the control socket (devices/virtual-headset). This fixture puts Monado's MAIN compositor on
# its Wayland-window target (`XRT_COMPOSITOR_FORCE_WAYLAND`, monado comp_settings.c:29,203-208)
# inside cage, which owns the VM's virtio-gpu KMS output — so `machine.screenshot()` is the
# stereo view Monado composited from zxr's projection layer: the greeter card and the OSK.
# Same shape as upstream nixos/tests/cage.nix:17-24,40 [external, nixpkgs]: cage on `-device
# virtio-gpu-pci`, one child program, screenshot/OCR of its window.
#
# Why cage's child is monado-service (not zxr): cage scans out only what its child maps, and
# the window is Monado's. zxr is an OpenXR client of that Monado and never opens a window;
# it holds only the render node (its `Unable to become drm master` warning), so card0's master
# is free for cage. Why a plain root unit and not `services.cage`: that module is a tty1
# display manager under logind (Conflicts=getty@tty1, PAMName) and would take the seat the
# greetd chain holds; here cage has no input at all (`WLR_LIBINPUT_NO_DEVICES`), the builtin
# libseat backend (root opens card0 directly), and the pixman renderer (no GL on virtio-gpu).
# The VT typing path of the login fixtures is untouched because those fixtures do not import
# this. Test-only, never in a fixture image; the picture on hardware is the HMD panel.
#
# The picture fills the output because Monado's Wayland target honours cage's configure size
# (the fork's `wayland-resize` series, research/78 §9 F26; upstream Monado pinned its window to
# half the HMD screen and showed a 640x360 corner). What is asserted is the journals, the
# listing and the colour count; OCR, which upstream's cage test uses on an xterm, was tried at
# this size and reads only the space bars — not a key the test can rest on.
{ pkgs }:
let
  runtimeDir = "/run/mura-vm-scene";
  zxr = pkgs.mura.zxr;
in
(import ./lib.nix { inherit pkgs; }) {
  name = "mura-vm-scene";
  profileModules = [ ../../profiles/multi-user.nix ./fixture-user.nix ];
  extraModules = [
    ({ pkgs, ... }: {
      # cage → monado-service. Monado's env is inherited by the child; cage exports
      # WAYLAND_DISPLAY to it. HOME: monado-service's static initialisers read it
      # (steamvr_lh.cpp:91). XRT_NO_STDIN: the IPC mainloop otherwise epolls a stdin a unit
      # does not have (ipc_server_mainloop_linux.c:212-220; dev-session does the same).
      systemd.services.mura-vm-scene-display = {
        description = "TEST-ONLY cage on virtio-gpu KMS with Monado's main compositor as its child";
        wantedBy = [ "multi-user.target" ];
        after = [ "systemd-logind.service" ];
        environment = {
          WLR_BACKENDS = "drm";
          WLR_RENDERER = "pixman";
          WLR_DRM_NO_MODIFIERS = "1";
          WLR_LIBINPUT_NO_DEVICES = "1";
          LIBSEAT_BACKEND = "builtin";
          XDG_RUNTIME_DIR = runtimeDir;
          HOME = "/root";
          XRT_NO_STDIN = "true";
          XRT_COMPOSITOR_NULL = "false";
          XRT_COMPOSITOR_DISABLE_DEFERRED = "true";
          XRT_COMPOSITOR_FORCE_WAYLAND = "true";
          SIMULATED_ENABLE = "true";
        };
        serviceConfig = {
          ExecStartPre = "${pkgs.coreutils}/bin/install -d -m 0700 ${runtimeDir}";
          ExecStart = "${pkgs.cage}/bin/cage -- ${pkgs.monado}/bin/monado-service";
          Restart = "no";
        };
      };
      # zxr as the greeter compositor against that Monado (the IPC socket lives in the shared
      # runtime dir; the loader is pointed at Monado's manifest as dev-session does). No
      # libinput: this fixture has no seat; input would come from the injector (`zxr ctl`).
      systemd.services.mura-vm-scene-zxr = {
        description = "TEST-ONLY zxr greeter scene rendered through Monado's main compositor";
        wantedBy = [ "multi-user.target" ];
        after = [ "mura-vm-scene-display.service" ];
        requires = [ "mura-vm-scene-display.service" ];
        environment = {
          XDG_RUNTIME_DIR = runtimeDir;
          XR_RUNTIME_JSON = "${pkgs.monado}/share/openxr/1/openxr_monado.json";
          ZXR_NO_LIBINPUT = "1";
        };
        path = [ pkgs.mura.greeter pkgs.mura.osk ];
        serviceConfig = {
          ExecStart = "${zxr}/bin/zxr --greeter --trusted mura-greeter --osk mura-osk";
          Restart = "on-failure";
          RestartSec = 2;
        };
      };
    })
  ];
  testScript = ''
    import os, subprocess

    machine.start()
    machine.wait_for_unit("multi-user.target")

    with subtest("cage owns the KMS output and Monado's main compositor presents into it"):
        machine.wait_until_succeeds("journalctl -u mura-vm-scene-display --no-pager | grep -q 'Modesetting with'")
        machine.wait_until_succeeds("journalctl -u mura-vm-scene-display --no-pager | grep -q 'comp_target_swapchain_create_images'", timeout=120)

    with subtest("zxr runs a focused session and composes the greeter and the OSK"):
        machine.wait_until_succeeds("journalctl -u mura-vm-scene-zxr --no-pager | grep -q 'session state state=FOCUSED'", timeout=120)
        machine.wait_until_succeeds("journalctl -u mura-vm-scene-zxr --no-pager | grep -q 'layer surface created member=.* namespace=mura-greeter'", timeout=60)
        machine.wait_until_succeeds("journalctl -u mura-vm-scene-zxr --no-pager | grep -c 'first frame committed' | grep -qE '^[2-9]'", timeout=60)
        sock = machine.succeed("ls ${runtimeDir}/zxr-*.sock").strip().split()[0]
        listing = machine.succeed(f"${zxr}/bin/zxr ctl {sock} list")
        print(listing)
        assert "ns=mura-greeter" in listing and "ns=osk" in listing, "both trusted layers are in the scene"
        assert "layers_submitted=0 " not in listing, "zxr has submitted projection layers"

    # C0 — admission classes and the controller lease (composition §5.3 rev 1, §7.7). The fork's
    # service binds both sockets itself here (cage's child, no socket units); zxr connects to
    # the control socket through libmonado and holds the lease. This Monado runs with upstream's
    # default (IPC_REQUIRE_CONTROLLER unset), so (iii) is the legacy-open behaviour; the Mura
    # session units set it true (modules/xr) and tests/vm/default-image.nix proves those units.
    ctl = "XDG_RUNTIME_DIR=${runtimeDir} ${pkgs.monado}/bin/monado-ctl "
    with subtest("C0 (ii): zxr holds the controller lease; a second controller is pending"):
        machine.wait_until_succeeds(f"${zxr}/bin/zxr ctl {sock} list | grep -q 'controller=holder'", timeout=30)
        status = machine.succeed(ctl + "--socket=control")   # monado-ctl on the control socket, behind zxr
        print(status)
        assert "Controller: pending" in status, "a second controller queues behind the holder"
        assert "role: controller" in status and "role: app" in status, "classes are stamped at accept"

    with subtest("C0 (i): a control verb from the application socket is refused while zxr holds the lease"):
        out = machine.fail(ctl + "--socket=app -f 1 2>&1")
        print(out)
        assert "does not hold the controller lease" in out, "XRT_ERROR_IPC_NOT_CONTROLLER reaches the client"
        assert "Controller: none" in machine.succeed(ctl + "--socket=app"), "an app-socket connection is never a controller"

    with subtest("C0 (ii): the holder's restart releases the lease and the new zxr takes it"):
        machine.succeed("systemctl restart mura-vm-scene-zxr")
        machine.wait_until_succeeds("journalctl -u mura-vm-scene-zxr --no-pager | grep -c 'controller=holder' | grep -qE '^[2-9]'", timeout=120)
        sock = machine.succeed("ls ${runtimeDir}/zxr-*.sock").strip().split()[0]
        machine.wait_until_succeeds(f"${zxr}/bin/zxr ctl {sock} list | grep -q 'controller=holder'", timeout=30)

    with subtest("C0 (iii): with no controller and the upstream default, the legacy verbs are open and the next controller is the holder"):
        machine.succeed("systemctl stop mura-vm-scene-zxr")
        machine.wait_until_succeeds(ctl + "--socket=app | grep -q 'Controller: none'", timeout=30)
        machine.succeed(ctl + "--socket=app -f 1")   # set_focused: legacy-open while no holder
        assert "Controller: holder" in machine.succeed(ctl + "--socket=control"), "monado-ctl on the control socket becomes the holder when nobody holds it"
        machine.succeed("systemctl start mura-vm-scene-zxr")
        machine.wait_until_succeeds("journalctl -u mura-vm-scene-zxr --no-pager | grep -q 'session state state=FOCUSED'", timeout=120)
        machine.wait_until_succeeds("journalctl -u mura-vm-scene-zxr --no-pager | grep -c 'first frame committed' | grep -qE '^[2-9]'", timeout=60)

    with subtest("the screenshot is the scene, not a console or a blank output"):
        import time; time.sleep(5)  # let a few composited frames land on the scanout
        png = os.path.join(machine.out_dir, "scene.png")
        machine.screenshot(png)
        colours = int(subprocess.check_output(f"pngtopnm '{png}' | ppmhist -noheader | wc -l", shell=True).decode().strip())
        print(f"distinct colours on the scanout: {colours}")
        assert colours > 200, f"a VT or blank output has few colours; the composited scene has over a thousand (got {colours})"
        # (OCR — upstream's cage-test assertion — reads only "space" off this picture; see the header.)
  '';
}
