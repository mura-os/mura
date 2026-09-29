# virtual-headset — the x86_64 VM smoke target.
#
# Proves the whole module stack (contract + os + xr + adaptation) evaluates and builds
# end-to-end without hardware: a NixOS VM running the common userspace under Wayland
# with Monado's simulated driver. This is the "prove the module stack" half of the
# Phase 1 implementation order (docs/research/00-synthesis.md §7 item 9), not a real port.
{ lib, pkgs, config, ... }:
{
  # The dev profile (SSH, serial) is part of the VM device; the *login* profile is
  # composed by the flake so both fixtures share this file.
  imports = [ ../../soc/virtual ../../profiles/dev.nix ];

  mura.device = {
    codename = "virtual-headset";
    vendor = "mura";
    name = "Virtual Headset (VM smoke target)";
    arch = "x86_64";
    supportTier = "booting";
    maintainers = [ ];
  };

  mura.hardware = {
    displays = 1;
    panel = { width = 1920; height = 1080; refresh = 60; };
    input.bluetooth = false; # no adapter in the VM: no pairing/ bind, no pre-login agent
    # The VM's Wi-Fi is one mac80211_hwsim radio (below); it cannot run AP and STA at once.
    input.concurrentApSta = false;
    # Its USB port is a dummy_hcd UDC (below): the gadget path runs for real, host side too.
    input.usbGadget = true;
  };

  # Virtual hardware for the out-of-band path (modules/os/oob.nix, D3): dummy_hcd gives a
  # device-side USB controller whose host side is this same kernel (so the gadget's DHCP
  # lease and web page are reachable in-VM); mac80211_hwsim gives two radios — one is the
  # headset's Wi-Fi, the second stands in for the phone in tests/vm/oob.nix.
  boot.kernelModules = [ "dummy_hcd" "mac80211_hwsim" ];
  # virtio_gpu: the DRM device plymouth draws on in stage 1 (recovery.nix). i8042/atkbd: QEMU's
  # PS/2 keyboard stands in for the HMD's buttons in stage 1 — the recovery panel's keyboard
  # fallback codes (specs/recovery-menu.md §4.5) are what the VM test drives.
  boot.initrd.kernelModules = [ "dummy_hcd" "virtio_gpu" "i8042" "atkbd" ];
  boot.extraModprobeConfig = "options mac80211_hwsim radios=2";

  # No donor: this is a from-source VM, so donor stays null and no flashable image
  # outputs are produced (null-propagation gating).
  mura.donor = null;

  # Native everything; simulated tracking (Monado's SIMULATED driver).
  mura.adaptation = {
    display.backend = "native";
    gpu.backend = "native";
    camera.backend = "native";
    sensors.backend = "native";
    audio.backend = "native";
    wifiBt.backend = "native";
    tracking.backend = "device-specific"; # simulated, provided by the runtime itself
  };

  mura.xr = {
    runtime = "monado";
    # The XR shell this device runs (ADR 0006). Selecting it is what turns the login chain
    # on (modules/os/session.nix); until M1 the session body is the sway stand-in.
    shell = "zxr";
    compositor.backend = "window"; # windowed compositor inside the VM, not vk-display
    environment = {
      # Monado's simulated-HMD setup for a VM without real hardware: the simulated
      # system builder is excluded from auto-discovery unless explicitly enabled
      # (monado target_builder_simulated.c).
      SIMULATED_ENABLE = "true";
      # The login chain's Monado has no display path to scan out to: lavapipe (the only Vulkan
      # driver on virtio-gpu) has no VK_KHR_display, and there is no window system under the
      # greeter or the session — zxr *is* the compositor. Monado's null compositor
      # (`XRT_COMPOSITOR_NULL`, `monado/src/xrt/targets/common/target_instance.c:43,111-117`;
      # compiled into nixpkgs' monado) accepts sessions, swapchains and layers and displays
      # nothing, so the XR chain runs end to end in the VM and is proven through the compositor's
      # control socket, the journal and AT-SPI. The picture of the scene in the VM is
      # tests/vm/scene.nix's: a test-only cage owning the virtio-gpu KMS output with a second
      # `monado-service` (main compositor, Wayland-window target) as its child and a zxr against it
      # (research/78 §9 F25, closing F14) — not this `monado.service`, which serves the login chain
      # that holds the seat.
      #
      # Also measured (research/78 §9 F18): Monado's MAIN compositor runs here without a window
      # too, on its off-screen `debug_image` target, when this is "false" AND
      # `XRT_COMPOSITOR_DISABLE_DEFERRED = "true"` (otherwise Monado picks its deferred XCB target
      # and the first session fails), at ~10x Monado's CPU and +90 MB in the VM with no picture.
      # Null vs main for the login chain's Monado is the owner's decision.
      XRT_COMPOSITOR_NULL = "true";
    };
  };

  mura.kernel.contract = [ "systemd" "container" ];

  mura.deployment = {
    bootScheme = "vm";
    flashMethod = "none";
    imageVariants = [ "dev-vm" ];
  };

  # Standard NixOS bits that make the VM boot.
  # (Kept at the top level: mixing these with an explicit `config` block is rejected
  # by the module system when top-level `mura.*` options are also set.)
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = false;
  fileSystems."/" = lib.mkDefault { device = "/dev/disk/by-label/nixos"; fsType = "ext4"; };

  # The login chain comes from the contract (modules/os/session.nix: greetd autologin or
  # the stand-in greeter) and the accounts from the profile the flake composes with this
  # device (profiles/default.nix or profiles/multi-user.nix + a fixture user). No getty
  # hack, no declared user here, and no permanent video/input group membership — device
  # access is logind's seat ACLs (implementation-path B1a).

  # Rung-2 dev-loop tuning (VM builds only; docs: README §Development). The VM
  # shares the host /nix/store, so iteration never builds an image: edit modules,
  # `nix run .#virtual-headset-vm`, and the QEMU window boots into the session (default
  # image) or the stand-in greeter (multi-user fixture).
  virtualisation.vmVariant = {
    imports = [ ./vm-persist.nix ]; # /persist on a second virtual disk (syspersist stand-in)
    # qemu-vm.nix assumes no radio and disables wpa_supplicant; this VM has a hwsim radio.
    networking.wireless.enable = lib.mkOverride 5 true; # beats qemu-vm.nix's mkVMOverride (10)
    virtualisation = {
      memorySize = 8192;
      cores = 4;
      # virgl: real GL inside the guest (wlroots/Monado want more than llvmpipe).
      qemu.options = [
        "-device virtio-gpu-gl-pci"
        "-display gtk,gl=on,show-cursor=on"
      ];
      forwardPorts = [
        { from = "host"; host.port = 2221; guest.port = 22; }
      ];
    };
  };

  system.stateVersion = lib.mkDefault "25.05";
}
