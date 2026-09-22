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
      shell = mkOption {
        type = types.enum [ "zxr" "stardust" "wayvr" "none" ];
        default = "none";
        description = ''
          The XR shell/compositor session run above the OpenXR runtime (ADR 0006).
          - zxr: the spatial-os compositor (Wayland-native, continues the wxrc zxr lineage
            as zxr-shell-v2; xdg-shell 2D apps + zxr-shell-v2 3D apps in one depth-tested space).
            Ships the 2D tier first, then the 3D-native tier (docs/research/10).
          - stardust: StardustXR as a packaged alternative session (not the backbone).
          - wayvr: WayVR as a packaged 2D-panels-in-XR overlay session.
          - none: headless/bring-up (the virtual-headset VM default until the compositor exists).
        '';
      };
      environment = mkOption {
        type = types.attrsOf types.str;
        default = { };
        description = "Environment variables for the Monado service unit (the proven config channel).";
      };

      ## Camera passthrough + hand cutout (ADR 0008) ----------------------
      passthrough = {
        enable = mkOption {
          type = types.bool;
          default = false;
          description = "Enable video see-through passthrough (the compositor environment layer). See docs/architecture/perception-passthrough-hands.md.";
        };
        latencyMode = mkOption {
          type = types.enum [ "low-latency" "high-quality" ];
          default = "low-latency";
          description = "Passthrough quality/latency tradeoff. Default favours latency (latency beats cleanliness); the quality knob lives on the geometry pipeline, never the display path.";
        };
        depthBackend = mkOption {
          type = types.enum [ "classical" "vk-qcom" "adreno-dfs" "hexagon" "none" ];
          default = "classical";
          description = ''
            The pluggable stereo-depth backend feeding passthrough (ADR 0008).
            'classical' (standard Vulkan compute / SGBM) is the BSP-independent baseline; the others
            are gated on per-device BSP inspection (docs/research/14-perception-claims-audit.md).
          '';
        };
        handCutout = {
          enable = mkOption {
            type = types.bool;
            default = false;
            description = "Enable egocentric hand/upper-limb cutout as a compositor top layer.";
          };
          upperLimbVisibility = mkOption {
            type = types.enum [ "visible" "hidden" "automatic" ];
            default = "automatic";
            description = "Shell default upper-limb composition policy (per-client overridable), mirroring the visionOS contract.";
          };
        };
      };

      ## Session / greeter / lock model (ADR 0007) -------------------------
      session = {
        autoLogin = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = ''
            Appliance profile: the owner username to auto-login straight into the XR session
            (greetd `initial_session`, no greeter UI). null selects the multi-user profile,
            which requires `session.greeter != "none"`. See ADR 0007.
          '';
          example = "owner";
        };
        greeter = mkOption {
          type = types.enum [ "none" "zxr-greeter" ];
          default = "none";
          description = ''
            Multi-user profile greeter run via greetd `default_session`.
            - zxr-greeter: the zxr compositor in restricted --greeter mode as the `greeter`
              user (Monado + IMU-only tracking, built-in auth scene, sessions from
              `spatial.xr.shell`), per docs/research/11.
            - none: appliance profile (requires `session.autoLogin`).
          '';
        };
        lock = {
          enable = mkOption {
            type = types.bool;
            default = true;
            description = ''
              Compositor-integrated lock (ADR 0007): an internal composition-policy state
              (compose only the lock scene, route input only to it, PAM via out-of-process
              spatial-authd). Not ext-session-lock-v1 (that is exposed only for the dev
              profile / third-party lockers).
            '';
          };
          triggers = mkOption {
            type = types.listOf (types.enum [ "boot" "doff" "idle" "suspend" "explicit" ]);
            default = [ "boot" "suspend" "explicit" ];
            description = ''
              Events that lock the session and require re-auth. `doff`/`idle` honor the
              grace window (docs/research/12 §6.2). `boot` locks the session on start
              whenever a credential is enrolled (Quest power-on-lock model).
            '';
          };
          doffGraceSeconds = mkOption {
            type = types.ints.unsigned;
            default = 45;
            description = ''
              Grace window after doff/idle during which don/activity resumes the session
              without re-auth. 0 = lock immediately (security-sensitive deployments).
            '';
          };
        };
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
    {
      # ADR 0007: a device with an XR shell session must select exactly one profile.
      # Appliance = autoLogin (no greeter); multi-user = greeter (no autoLogin).
      # Headless/bring-up images (shell = "none") are exempt.
      assertion = cfg.xr.shell == "none"
        || ((cfg.xr.session.autoLogin != null) != (cfg.xr.session.greeter != "none"));
      message = "spatial.xr.session must select exactly one profile when spatial.xr.shell is set: session.autoLogin (appliance) OR session.greeter != \"none\" (multi-user), not both and not neither (ADR 0007).";
    }
    {
      # A lockable session needs a runtime to compose the lock scene over.
      assertion = cfg.xr.shell == "none" || !cfg.xr.session.lock.enable || cfg.xr.runtime != "none";
      message = "spatial.xr.session.lock.enable requires spatial.xr.runtime != \"none\" (the lock scene composes over the runtime; ADR 0007).";
    }
  ];
}
