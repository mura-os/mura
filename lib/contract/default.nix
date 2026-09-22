# spatial-os device contract: typed option surface.
#
# This is the NixOS-module option set a device declares, per
# docs/architecture/device-contract.md. It is a plain module (importable into any
# NixOS configuration or evaluated standalone by lib/eval-device.nix). Options are
# grouped exactly as the contract document specifies; assertions reject inconsistent
# combinations before anything builds.
{ lib, config, ... }:
let
  inherit (lib) mkOption types mkEnableOption literalExpression;

  # A per-subsystem adaptation backend selector. Each hardware subsystem picks one
  # of native | android-backed | device-specific (ADR 0003).
  backendModule = types.submodule {
    options.backend = mkOption {
      type = types.enum [ "native" "android-backed" "device-specific" ];
      default = "native";
      description = ''
        Which adaptation backend provides this subsystem.
        - native: mainline Linux driver stack (default posture).
        - android-backed: donor HAL/vendor library via libhybris/libgbinder or a
          late, optional LXC unit. Never a local-fs.target prerequisite.
        - device-specific: a dedicated implementation (e.g. DSP tracking).
      '';
    };
  };

  cfg = config.spatial;
in
{
  options.spatial = {

    ## Identity and support (mandatory minimum) -----------------------------
    device = {
      codename = mkOption {
        type = types.str;
        description = "Short device codename; also the devices/<codename>/ directory name.";
        example = "lynx-r1";
      };
      vendor = mkOption {
        type = types.str;
        description = "Device vendor.";
        example = "lynx";
      };
      name = mkOption {
        type = types.str;
        description = "Human-readable device name.";
        example = "Lynx R1";
      };
      arch = mkOption {
        type = types.enum [ "aarch64" "x86_64" ];
        default = "aarch64";
        description = "Target CPU architecture.";
      };
      supportTier = mkOption {
        type = types.enum [ "booting" "xr-functional" "release-supported" ];
        default = "booting";
        description = ''
          Support tier. Gates which qualification checks are mandatory. CI asserts
          the declared tier is consistent with the checks the device actually passes.
        '';
      };
      maintainers = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Named maintainers. Empty list caps the device at the 'booting' tier.";
      };
      skuConstraints = mkOption {
        type = types.attrsOf types.str;
        default = { };
        description = "Hardware revision / SKU constraints this port is valid for.";
      };
    };

    ## Hardware geometry (declared facts) -----------------------------------
    hardware = {
      soc = mkOption {
        type = types.enum [ "msm8998" "sm8250" "sm8550" "sm8650" "virtual" ];
        description = "SoC family. Normally set by the family module, not the device.";
      };
      displays = mkOption {
        type = types.ints.positive;
        default = 2;
        description = "Number of physical display panels.";
      };
      panel = mkOption {
        type = types.submodule {
          options = {
            width = mkOption { type = types.ints.positive; description = "Panel width in pixels."; };
            height = mkOption { type = types.ints.positive; description = "Panel height in pixels."; };
            refresh = mkOption { type = types.ints.positive; default = 90; description = "Refresh rate in Hz."; };
          };
        };
        description = "Per-eye panel geometry.";
      };
    };

    ## Donor manifest (see lib/donor + docs/architecture/donor-pipeline.md) --
    donor = mkOption {
      type = types.nullOr (types.attrsOf types.anything);
      default = null;
      description = ''
        The donor manifest attrset (schema in docs/architecture/donor-pipeline.md
        §manifest). null means no donor pinned yet; flashable image outputs remain
        absent until a donor and its reviewed contract exist (null-propagation gating).
      '';
    };

    ## Kernel build + contract ----------------------------------------------
    kernel = {
      contract = mkOption {
        type = types.listOf types.str;
        default = [ "systemd" "container" ];
        description = ''
          kconfig contract category aliases this kernel must satisfy (checked against
          the built .config). Composed per subsystem-backend and per tier. This is the
          gate that lets NixOS be the default runtime safely (ADR 0002).
        '';
      };
      bootimg = {
        headerVersion = mkOption {
          type = types.nullOr (types.ints.between 0 4);
          default = null;
          description = "Android boot image header version, derived from the donor via unpack_bootimg. Never assumed.";
        };
        hasVendorBoot = mkOption { type = types.bool; default = false; description = "Donor has a separate vendor_boot partition."; };
        hasInitBoot = mkOption { type = types.bool; default = false; description = "Donor has a separate init_boot partition (Android 13+ launch)."; };
        hasDtbo = mkOption { type = types.bool; default = false; description = "Donor has a dtbo partition."; };
      };
    };

    ## Per-subsystem adaptation backends (ADR 0003) -------------------------
    adaptation = {
      display = mkOption { type = backendModule; default = { }; description = "Display / KMS backend."; };
      gpu = mkOption { type = backendModule; default = { }; description = "GPU / rendering backend."; };
      camera = mkOption { type = backendModule; default = { }; description = "Tracking/passthrough camera backend."; };
      sensors = mkOption { type = backendModule; default = { }; description = "IMU / sensor backend."; };
      audio = mkOption { type = backendModule; default = { }; description = "Audio backend."; };
      wifiBt = mkOption { type = backendModule; default = { }; description = "Wi-Fi / Bluetooth backend."; };
      tracking = mkOption {
        type = backendModule;
        default = { backend = "device-specific"; };
        description = "6DoF tracking backend. Usually device-specific; no ecosystem compat precedent exists.";
      };
    };

    ## XR runtime and device driver -----------------------------------------
    xr = {
      runtime = mkOption {
        type = types.enum [ "monado" "wivrn" "none" ];
        default = "monado";
        description = "System OpenXR runtime. 'none' for a headless/bring-up image.";
      };
      compositor.backend = mkOption {
        type = types.enum [ "vk-display" "wayland-direct" "window" ];
        default = "vk-display";
        description = ''
          Monado compositor windowing backend. vk-display (VK_KHR_display, Monado owns
          DRM directly) is the appliance default and the first feasibility test per device.
          'window' is for the virtual-headset VM.
        '';
      };
      environment = mkOption {
        type = types.attrsOf types.str;
        default = { };
        description = "Environment variables for the Monado service unit (the proven config channel).";
      };
    };

    ## Deployment: partitions, images, flashing -----------------------------
    deployment = {
      bootScheme = mkOption {
        type = types.enum [ "android-bootimg" "uefi-rauc" "abl-uboot" "vm" ];
        description = "Boot scheme; selects the image family and update backend.";
      };
      abSlots = mkOption { type = types.bool; default = false; description = "Device uses A/B slots."; };
      flashMethod = mkOption {
        type = types.enum [ "fastboot" "heimdall" "edl-qdl" "rauc" "none" ];
        default = "none";
        description = "Flashing protocol for the reference-free bundle's flash script.";
      };
      protectedPartitions = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Per-unit calibration/identity/NV partitions the installer must never touch without a separately reviewed operation.";
      };
      imageVariants = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Which lib/images/ variants to build for this device.";
      };
    };

    ## Qualification --------------------------------------------------------
    qualification = {
      acceptanceTests = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Names of automated/manual acceptance tests, gated by support tier.";
      };
    };
  };

  ## Cross-field assertions -------------------------------------------------
  config.assertions = [
    {
      assertion = cfg.device.supportTier == "booting" || cfg.device.maintainers != [ ];
      message = "spatial.device.supportTier '${cfg.device.supportTier}' requires at least one entry in spatial.device.maintainers.";
    }
    {
      assertion = cfg.deployment.bootScheme != "android-bootimg" || cfg.kernel.bootimg.headerVersion != null;
      message = "android-bootimg boot scheme requires spatial.kernel.bootimg.headerVersion (derive it from the donor with unpack_bootimg; do not assume a legacy header).";
    }
    {
      # Any android-backed subsystem needs a donor to extract blobs from.
      assertion =
        let backends = with cfg.adaptation; [ display.backend gpu.backend camera.backend sensors.backend audio.backend wifiBt.backend tracking.backend ];
        in !(lib.any (b: b == "android-backed") backends) || cfg.donor != null;
      message = "An 'android-backed' adaptation subsystem requires spatial.donor to be set (blobs are extracted from the pinned donor).";
    }
  ];
}
