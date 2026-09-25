# Mura device contract: typed option surface.
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

  cfg = config.mura;

  # Human accounts declared in the image (ADR 0017 rev 2). Read from NixOS's own
  # `users.users` when the contract is evaluated inside a NixOS configuration; empty when
  # evaluated standalone (lib/eval-device.nix, tests) with no `users` option declared.
  declaredHumanAccounts =
    lib.attrNames
      (lib.filterAttrs (_: u: (u.isNormalUser or false))
        (lib.attrByPath [ "users" "users" ] { } config));
in
{
  options.mura = {

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
      ipd = {
        source = mkOption {
          type = types.enum [ "fixed" "manual" "manual-sensed" "stored" "motorized-auto" ];
          default = "fixed";
          description = ''
            Source of the rendering-IPD value (ADR 0011):
            - fixed: hardcoded default (defaultMeters).
            - manual: unsensed mechanical adjustment; user-entered/stored value.
            - manual-sensed: device reports the mechanism position (Quest 1 / Lynx R1 class).
            - stored: per-user software value on fixed optics.
            - motorized-auto: eye-tracked servo (Galaxy XR / PFDM class); requires
              mura.adaptation.eyes.
          '';
        };
        defaultMeters = mkOption {
          type = types.float;
          default = 0.063;
          description = "Safe default IPD used pre-auth (greeter/lock, ADR 0007) and when no measured/stored value exists.";
        };
      };

      ## Input facts (docs/research/42, first-run-onboarding.md §4.4) -------
      # What the headset can accept as input before anything is configured. The
      # input floor every pre-login and welcome scene must be operable at is IMU
      # head-aim + the HMD's own buttons (dwell where a button is unusable).
      input = {
        hmdButtons = mkOption {
          type = types.attrsOf types.str;
          default = { power = "KEY_POWER"; volumeUp = "KEY_VOLUMEUP"; volumeDown = "KEY_VOLUMEDOWN"; };
          description = ''
            Buttons on the HMD body as evdev key names, keyed by role. Every target has
            power + volume; declare a dedicated `select` where one exists (Steam Frame
            "Aux" = KEY_SELECT, Quest 3S action button, Lynx "R"). The compositor reads
            them through libinput as ordinary key events; logind's power-key handling is
            set to ignore or inhibited so the compositor owns the key (research/42 §4.3).
          '';
          example = literalExpression ''{ power = "KEY_POWER"; volumeUp = "KEY_VOLUMEUP"; volumeDown = "KEY_VOLUMEDOWN"; select = "KEY_SELECT"; }'';
        };
        selectRole = mkOption {
          type = types.str;
          default = "volumeUp";
          description = ''
            Which `hmdButtons` role acts as "select" at the input floor. The default follows
            the Android-side convention (Meta's head-gaze fallback and PICO's Head Control
            Mode click with the volume keys; Android Switch Access defaults Vol+ = Select);
            the Steam Frame has a dedicated Aux (`KEY_SELECT`); on the Galaxy XR the Top
            button *is* the PMIC power key, so `select` and `power` may legitimately share a
            code — the compositor disambiguates short press (select) from long press (power
            menu). Must name a key of `hmdButtons` — asserted. research/42 §4.3a.
          '';
        };
        backRole = mkOption {
          type = types.nullOr types.str;
          default = "volumeDown";
          description = ''
            Which `hmdButtons` role acts as "back/cancel" at the input floor (Android Switch
            Access: Vol- = Next; PICO: Vol- = Home). null = no back button; scenes must then
            expose an on-scene cancel target. Recenter is a long press of the select role by
            convention (PICO Vol- hold, Play For Dream dial hold). Must name a key of
            `hmdButtons` when set — asserted.
          '';
        };
        controllers = mkOption {
          type = types.enum [ "none" "imu-3dof" "optical-6dof" ];
          default = "none";
          description = ''
            Controller class available *before cameras are up*: none; imu-3dof (buttons +
            orientation-only pose from the controller's IMU — the WMR/Rift S/Index-dongle
            class, research/42 §4.2); optical-6dof (needs the perception plane, so counts as
            imu-3dof pre-login). Informational for the greeter's input ladder.
          '';
        };
        bluetooth = mkOption {
          type = types.bool;
          default = true;
          description = "The device has a Bluetooth adapter (pre-login pairing agent + `pairing/` state class exist only when true).";
        };
        concurrentApSta = mkOption {
          type = types.nullOr types.bool;
          default = null;
          description = ''
            Whether the Wi-Fi chip supports a hotspot and a station link at the same time
            (research/42 §6.1). null = unknown; the provisioning hotspot's handoff behaviour
            depends on it (first-run-onboarding.md §5).
          '';
        };
        proximitySource = mkOption {
          type = types.enum [ "none" "iio" "hid" "ssc" ];
          default = "none";
          description = "Where the wear (don/doff) sensor is read from: IIO proximity (Steam Frame vcnl4040, Lynx), a vendor HID field, or the Qualcomm SSC (Galaxy XR class). research/42 §4.4.";
        };
        usbGadget = mkOption {
          type = types.bool;
          default = true;
          description = ''
            The device's USB port has a device-capable controller (UDC), so the USB Ethernet
            gadget of first-run-onboarding.md §5.4 can be presented from the initramfs (the
            postmarketOS pattern). false on targets whose port is host-only. The virtual
            headset provides a UDC through `dummy_hcd`. D3.
          '';
        };
      };
    };

    ## Health: preflight, crash-loop ladder, readiness (implementation-path §3a/§3a-bis; D6) --
    health = {
      crashLoopThreshold = mkOption {
        type = types.ints.positive;
        default = 3;
        description = ''
          Consecutive boots on which the XR preflight (`mura-preflight`, implementation-path
          §3a-bis) failed a hard check before the boot enters `mura-recovery.target` (the
          diagnostic target: sshd + serial, no greeter). Reset by a blessed boot. Schema value
          (constraint 9). The count is the A/B ecosystem's convention (systemd boot counting,
          RAUC, U-Boot, Barebox all default to 3); the ladder itself has no in-tree comparable
          and is before the owner (research/56 §3, Q1).
        '';
      };
      deviceWaitSeconds = mkOption {
        type = types.ints.positive;
        default = 10;
        description = ''
          How long the preflight waits for the tracking device nodes (P5) and the Monado probe
          (P6) before reporting them failed. Both are *soft* checks: the greeter starts either
          way and the result is in `/run/mura/preflight.json` — the shape of everything that
          waits for a hardware class at boot (GDM waits 10 s for a primary GPU, then "Proceeding
          with any GPU"; postmarketOS waits 10 s for a framebuffer, then continues; systemd's
          guidance is "warn or report failure after a timeout … tailored to the hardware type").
          Ruled 2026-09-25 from research/56 §5; per-device contracts tailor the value.
        '';
      };
    };

    ## Out-of-band access (first-run-onboarding.md §5; modules/os/oob.nix, D3) --------
    oob.hotspot = {
      idleTimeoutMinutes = mkOption {
        type = types.ints.positive;
        default = 10;
        description = ''
          Minutes with no station associated after which the provisioning hotspot's radio is
          taken down for this boot (it returns at the next boot while setup is unfinished;
          first-run-onboarding.md §5). Never counts down while a phone is connected. A schema
          value, never compiled in (constraint 9). The default is Android's soft-AP shutdown
          timeout (600000 ms, `config_wifiFrameworkSoftApShutDownTimeoutMilliseconds`) — the
          only battery-powered comparable that tears an empty AP down; the mains-powered
          provisioning portals never do (research/56 §6).
        '';
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
      source = mkOption {
        type = types.nullOr types.attrs;
        default = null;
        description = ''
          Pinned kernel source (vendor tag or mainline rev + hash), fetcher-args attrs.
          null = the device uses the default nixpkgs kernel (VM/dev targets).
          (device-contract.md §kernel; doc-only until now, registry §10.2.)
        '';
      };
      structuredExtraConfig = mkOption {
        type = types.attrs;
        default = { };
        description = ''
          Structured kconfig overrides with per-option provenance comments (Jovian style).
          Applied on top of configFile when both are set; the realization-time contract
          check verifies the merged result (Mobile NixOS validator model).
        '';
      };
      configFile = mkOption {
        type = types.nullOr types.path;
        default = null;
        description = "Literal .config as the source of truth (Mobile NixOS style).";
      };
      dtbs = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "DTB name templates, resolved per device.";
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
      eyes = mkOption {
        type = types.submodule {
          options.backend = mkOption {
            type = types.enum [ "none" "native" "android-backed" "device-specific" ];
            default = "none";
            description = ''
              Eye-tracking backend (ADR 0011): the session-scoped Monado-side eye-camera
              service (gaze via XR_EXT_eye_gaze_interaction; rotation-center IPD into
              eye_relation). 'none' = no eye-tracking hardware (most targets).
              'android-backed' = the donor's vendor ET service closure (no target documents
              V4L2 eye cameras); 'native' = our pipeline on directly accessible cameras;
              'device-specific' = bespoke DSP/vendor protocol.
            '';
          };
        };
        default = { };
        description = "Eye-tracking subsystem backend.";
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
      monado = {
        rev = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = ''
            Pinned Monado revision for this device's XR driver (monado-rev + patch
            series, the WiVRn pinning pattern; Monado has no stable out-of-tree
            driver ABI). null = the default packaged Monado.
          '';
        };
        patches = mkOption {
          type = types.listOf types.path;
          default = [ ];
          description = "Per-device Monado patch series (patches/monado/<device>/...).";
        };
        drivers = mkOption {
          type = types.attrsOf (types.submodule {
            options.enable = mkOption { type = types.bool; default = false; };
          });
          default = { };
          description = "Per-driver toggles mapped to XRT_BUILD_DRIVER_* for a minimal per-device runtime.";
        };
      };
      tracking.slam.package = mkOption {
        type = types.nullOr types.package;
        default = null;
        description = ''
          VIT tracker package providing libbasalt.so (or compatible); sets
          VIT_SYSTEM_LIBRARY_PATH in the Monado unit (ADR 0009 layer A).
          null = no SLAM (3DoF-only or VM).
        '';
      };
      calibration.paths = mkOption {
        type = types.attrsOf types.str;
        default = { };
        description = ''
          Per-device calibration data locations (per-unit SYSTEM state, never $HOME —
          ADR 0007). Keys are calibration kinds (camera, distortion, ipd, imu...),
          values absolute paths (typically under /var/lib/mura or a vendor persist
          mount listed in deployment.protectedPartitions).
        '';
      };
      shell = mkOption {
        type = types.enum [ "zxr" "stardust" "wayvr" "none" ];
        default = "none";
        description = ''
          The XR shell/compositor session run above the OpenXR runtime (ADR 0006).
          - zxr: the Mura compositor (Wayland-native, continues the wxrc zxr lineage
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

      ## Spatial mapping / anchors / world understanding (ADR 0009) --------
      mapping = {
        enable = mkOption {
          type = types.bool;
          default = false;
          description = "Enable the mapping+anchor service (layers B/C: keyframes, loop closure, relocalization, persistent anchors). See docs/architecture/spatial-mapping.md.";
        };
        depthAssist = mkOption {
          type = types.enum [ "none" "flood-ir" "active-ir-pattern" "tof-sensor" "android-backed" ];
          default = "none";
          description = ''
            The device's depth-ASSIST hardware for mapping (docs/research/22 §6; named "assist"
            because 'none' still means passive RGB stereo, not "no depth" — REVIEW-mapping M-17).
            A *policy* axis — it sets the inferred-state budget and illuminator duty cycle for
            dense geometry — not a backend selector (that stays passthrough.depthBackend).
            'none' = passive stereo RGB only (blank-wall/low-light geometry stays 'unknown');
            'flood-ir' improves SNR but not texture (Steam Frame); 'active-ir-pattern' gives
            night-capable active stereo (Quest 3); 'tof-sensor'/'android-backed' are direct-depth
            paths (Galaxy XR class).
          '';
        };
        persistence = mkOption {
          type = types.bool;
          default = false;
          description = "Enable the encrypted on-device map/anchor store and boot relocalization (anchors survive reboots). Requires mapping.enable.";
        };
        boundary = mkOption {
          type = types.bool;
          default = false;
          description = "Enable the compositor-owned boundary system (floor + play volume + keep-out; breach forces passthrough without client cooperation).";
        };
      };

      ## Expression/gaze sensing facts (adr/0010-avatar-control-space-and-driver.md)
      # Declared per-device capabilities for the avatar driver's degraded-mode
      # ladder. Initial population: the verified matrix in docs/research/25 §3.
      # These are facts about what the device's runtime path exposes on Linux,
      # not feature toggles; every value must survive the S-1 sensing kill-gate.
      sensing = {
        gaze = mkOption {
          type = types.enum [ "none" "combined" "per-eye" ];
          default = "none";
          description = "Eye-gaze exposure: combined pose (XR_EXT_eye_gaze_interaction) or per-eye poses. No verified Linux per-eye path exists today (docs/research/25 §2).";
        };
        eyelid = mkOption {
          type = types.enum [ "none" "weights" "openness" ];
          default = "none";
          description = "Eyelid signal: 'weights' = closure blendshapes inside the face-weight set (FB2/ANDROID indices 12/13); 'openness' = a dedicated per-eye openness channel (add-on trackers).";
        };
        faceWeights = mkOption {
          type = types.enum [ "none" "fb2-visual" "fb2-audio" "android" "htc" ];
          default = "none";
          description = "Face expression-weight source/schema served through Monado's face-device role (docs/research/25 §1-2). fb2-audio requires a microphone.";
        };
        mouthCamera = mkOption {
          type = types.enum [ "none" "internal" "addon" ];
          default = "none";
          description = "Optical mouth view: 'internal' = built-in face cameras feed the runtime's visual tracking; 'addon' = expansion-port/USB mouth camera (Baballonia path).";
        };
        micChannels = mkOption {
          type = types.ints.unsigned;
          default = 0;
          description = "Microphone channels available to the audio-inferred rung (and fb2-audio).";
        };
      };

      ## Persona avatar feature (adr/0010-avatar-control-space-and-driver.md) ----
      avatar = {
        enable = mkOption {
          type = types.bool;
          default = false;
          description = ''
            Enable the Persona avatar driver service + runtime (docs/architecture/avatar-persona.md).
            The driver consumes Monado face/gaze devices per mura.xr.sensing.* and emits the
            versioned semantic control stream; the runtime renders assets as a zxr client.
            Gated per device on the S-1 sensing and R-1 render kill-gates.
          '';
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
          example = "mura";
        };
        greeter = mkOption {
          type = types.enum [ "none" "zxr-greeter" ];
          default = "none";
          description = ''
            Multi-user profile greeter run via greetd `default_session`.
            - zxr-greeter: the zxr compositor in restricted --greeter mode as the `greeter`
              user (Monado + IMU-only tracking, built-in auth scene, sessions from
              `mura.xr.shell`), per docs/research/11.
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
              mura-authd). Not ext-session-lock-v1 (that is exposed only for the dev
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
        readinessTimeoutSeconds = mkOption {
          type = types.ints.positive;
          default = 30;
          description = ''
            Seconds the session wrapper waits for the compositor to signal readiness
            (`WAYLAND_DISPLAY` published, `READY=1`) before the login is torn down and the
            greeter returns (specs/session-bootstrap.md §4 step 4; uwsm's
            `mura-compositor.service` `TimeoutStartSec`). A schema value, never compiled in
            (constraint 9).
          '';
        };
        faillock = {
          deny = mkOption {
            type = types.ints.positive;
            default = 5;
            description = ''
              Consecutive authentication failures before pam_faillock locks the account
              (greeter, lock, SSH). Counters persist on /persist so a reboot does not reset the
              ladder (multi-user.md §3). A schema value, never compiled in (constraint 9).
            '';
          };
          unlockSeconds = mkOption {
            type = types.ints.positive;
            default = 300;
            description = "Seconds after which a faillock lockout clears (pam_faillock unlock_time).";
          };
        };
        allowNoDeclaredAccount = mkOption {
          type = types.bool;
          default = false;
          description = ''
            Escape hatch for the declared-account assertions (ADR 0017 rev 2,
            first-run-onboarding.md §1). The image is the installation: a greeter profile
            must declare at least one human account (`users.users.<n>.isNormalUser`), and
            an appliance profile's `autoLogin` user must be declared — otherwise nobody can
            ever log in and no runtime bootstrap screen exists to fix it. Set this to true
            only if you really want to build such an image (the NixOS
            `users.allowNoPasswordLogin` pattern); recovery is then root over TTY/SSH.
          '';
        };
        multiUser = {
          enable = mkOption {
            type = types.bool;
            default = false;
            description = ''
              Standard Linux multi-user on the greeter profile (ADR 0018 rev 3,
              multi-user.md): selects the userborn wiring that persists the account
              database across A/B slots (/persist/userdb). Accounts are ordinary Unix
              accounts managed by standard tools (useradd over SSH works); the in-headset
              settings UI is a polkit-gated convenience path. No account cap exists.
            '';
          };
          uidRange = mkOption {
            type = types.submodule {
              options = {
                min = mkOption {
                  type = types.ints.unsigned;
                  default = 1000;
                  description = "Lowest UID the greeter picker enumerates (login.defs UID_MIN convention).";
                };
                max = mkOption {
                  type = types.ints.unsigned;
                  default = 60000;
                  description = "Highest UID the greeter picker enumerates (login.defs UID_MAX convention).";
                };
              };
            };
            default = { };
            description = ''
              The UID window the greeter picker enumerates (NSS iteration; the SDDM/
              tuigreet login.defs-shaped pattern — multi-user.md §2). Enumeration only:
              free-text username entry is always available beside the picker, and the
              window never limits how many accounts exist.
            '';
          };
        };
        guest = {
          enable = mkOption {
            type = types.bool;
            default = false;
            description = ''
              Guest mode (ADR 0018, multi-user.md §4): an ephemeral per-session account
              (provisiond add/remove around session lifecycle, LightDM contract), tmpfs or
              wiped home, autologin-class PAM service `mura-guest`, transient
              calibration, owner-granted greeter tile. Requires multiUser.enable (the
              appliance profile has no greeter surface to grant it from).
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
      bootTries = mkOption {
        type = types.ints.positive;
        default = 3;
        description = ''
          Boot attempts a freshly installed slot gets before systemd-boot falls back to the
          previous one (the `+N` BLS counter armed by `mura-bootconf set-primary`;
          implementation-path §3a, images-and-updates.md health-gated success). A blessed boot
          (mura-readiness → boot-complete.target → systemd-bless-boot) clears it. Schema value;
          the default is the A/B ecosystem's convention (systemd's walkthrough, RAUC, U-Boot and
          Barebox all use 3; none explain it — research/56 §3).
        '';
      };
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
      readinessCheck = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = ''
          Name of the XR-readiness health check an installed update must pass before
          being marked successful (images-and-updates.md §health-gated success). The
          update agent runs it post-boot; failure keeps the previous slot bootable.
          Required for tiers above 'booting'.
        '';
      };
    };
  };

  ## Cross-field assertions -------------------------------------------------
  config.assertions = [
    {
      assertion = cfg.device.supportTier == "booting" || cfg.device.maintainers != [ ];
      message = "mura.device.supportTier '${cfg.device.supportTier}' requires at least one entry in mura.device.maintainers.";
    }
    {
      assertion = cfg.device.supportTier == "booting" || cfg.qualification.readinessCheck != null;
      message = "mura.device.supportTier '${cfg.device.supportTier}' requires mura.qualification.readinessCheck (the xr-functional tier is defined by a passing readiness check; device-contract.md §tiers, images-and-updates.md §health-gated success).";
    }
    {
      assertion = cfg.deployment.bootScheme != "android-bootimg" || cfg.kernel.bootimg.headerVersion != null;
      message = "android-bootimg boot scheme requires mura.kernel.bootimg.headerVersion (derive it from the donor with unpack_bootimg; do not assume a legacy header).";
    }
    {
      # Any android-backed subsystem needs a donor to extract blobs from.
      assertion =
        let backends = with cfg.adaptation; [ display.backend gpu.backend camera.backend sensors.backend audio.backend wifiBt.backend tracking.backend eyes.backend ];
        in !(lib.any (b: b == "android-backed") backends) || cfg.donor != null;
      message = "An 'android-backed' adaptation subsystem requires mura.donor to be set (blobs are extracted from the pinned donor).";
    }
    {
      # ADR 0011: motorized auto-IPD is an eye-tracked servo; it needs the eyes subsystem.
      assertion = cfg.hardware.ipd.source != "motorized-auto" || cfg.adaptation.eyes.backend != "none";
      message = "mura.hardware.ipd.source = \"motorized-auto\" requires mura.adaptation.eyes.backend != \"none\" (the servo is driven by eye tracking; ADR 0011).";
    }
    {
      # ADR 0007: a device with an XR shell session must select exactly one profile.
      # Appliance = autoLogin (no greeter); multi-user = greeter (no autoLogin).
      # Headless/bring-up images (shell = "none") are exempt.
      assertion = cfg.xr.shell == "none"
        || ((cfg.xr.session.autoLogin != null) != (cfg.xr.session.greeter != "none"));
      message = "mura.xr.session must select exactly one profile when mura.xr.shell is set: session.autoLogin (appliance) OR session.greeter != \"none\" (multi-user), not both and not neither (ADR 0007).";
    }
    {
      # ADR 0017 rev 2: the image is the installation. A greeter image must declare at
      # least one human account — there is no runtime account-bootstrap screen, and GDM's
      # zero-users fallback is not translated (our OEM-preinstall equivalent is the default
      # image). Mirrors NixOS's users.allowNoPasswordLogin check, which is silent for the
      # multi-user profile because it runs mutableUsers = true under userborn.
      assertion = cfg.xr.session.greeter == "none"
        || cfg.xr.session.allowNoDeclaredAccount
        || declaredHumanAccounts != [ ];
      message = "mura.xr.session.greeter requires at least one declared human account (users.users.<name>.isNormalUser = true); the image is the installation and no runtime bootstrap screen exists (ADR 0017 rev 2 / first-run-onboarding.md §1). Declare one, or set mura.xr.session.allowNoDeclaredAccount = true if you really want an image nobody can log in to.";
    }
    {
      # research/42 §7 / first-run-onboarding §4.4: the input floor needs a "select" button
      # that actually exists on the HMD.
      assertion = builtins.hasAttr cfg.hardware.input.selectRole cfg.hardware.input.hmdButtons;
      message = "mura.hardware.input.selectRole = \"${cfg.hardware.input.selectRole}\" must name a key of mura.hardware.input.hmdButtons (the input-floor select button; first-run-onboarding.md §4.4).";
    }
    {
      assertion = cfg.hardware.input.backRole == null
        || builtins.hasAttr cfg.hardware.input.backRole cfg.hardware.input.hmdButtons;
      message = "mura.hardware.input.backRole = \"${toString cfg.hardware.input.backRole}\" must name a key of mura.hardware.input.hmdButtons or be null (first-run-onboarding.md §4.4).";
    }
    {
      # ADR 0017 rev 2: the appliance profile's autologin user must exist in the image.
      assertion = cfg.xr.session.autoLogin == null
        || cfg.xr.session.allowNoDeclaredAccount
        || builtins.elem cfg.xr.session.autoLogin declaredHumanAccounts;
      message = "mura.xr.session.autoLogin = \"${toString cfg.xr.session.autoLogin}\" names a user that is not declared as a human account (users.users.<name>.isNormalUser = true); the default image declares `mura` (ADR 0017 rev 2). Declare the user, or set mura.xr.session.allowNoDeclaredAccount = true.";
    }
    {
      # ADR 0018: multi-account rides the greeter (picker + per-account PIN); the
      # appliance profile is single-owner by design.
      assertion = !cfg.xr.session.multiUser.enable || cfg.xr.session.greeter != "none";
      message = "mura.xr.session.multiUser.enable requires the multi-user profile (session.greeter != \"none\"); the appliance profile is single-owner (ADR 0017/0018).";
    }
    {
      # ADR 0018: the guest tile lives in the greeter scene and is owner-granted there.
      assertion = !cfg.xr.session.guest.enable || cfg.xr.session.multiUser.enable;
      message = "mura.xr.session.guest.enable requires mura.xr.session.multiUser.enable (the guest tile is a greeter-scene affordance; ADR 0018 / multi-user.md §4).";
    }
    {
      # multi-user.md §2: the picker enumeration window must be well-formed and start at
      # or above the human-account floor. It bounds enumeration only — never account count.
      assertion =
        !cfg.xr.session.multiUser.enable
        || (
          cfg.xr.session.multiUser.uidRange.min <= cfg.xr.session.multiUser.uidRange.max
          && cfg.xr.session.multiUser.uidRange.min >= 1000
        );
      message = "mura.xr.session.multiUser.uidRange must satisfy min <= max with min >= 1000 (the login.defs human-account floor; multi-user.md §2).";
    }
    {
      # A lockable session needs a runtime to compose the lock scene over.
      assertion = cfg.xr.shell == "none" || !cfg.xr.session.lock.enable || cfg.xr.runtime != "none";
      message = "mura.xr.session.lock.enable requires mura.xr.runtime != \"none\" (the lock scene composes over the runtime; ADR 0007).";
    }
    {
      # adr/0010 (avatar): audio-derived face weights need a microphone.
      assertion = cfg.xr.sensing.faceWeights != "fb2-audio" || cfg.xr.sensing.micChannels > 0;
      message = "mura.xr.sensing.faceWeights = \"fb2-audio\" requires mura.xr.sensing.micChannels > 0 (adr/0010-avatar-control-space-and-driver.md).";
    }
    {
      # adr/0010 (avatar): the avatar driver consumes Monado devices; the runtime renders via the zxr shell.
      assertion = !cfg.xr.avatar.enable || (cfg.xr.runtime != "none" && cfg.xr.shell == "zxr");
      message = "mura.xr.avatar.enable requires mura.xr.runtime != \"none\" and mura.xr.shell = \"zxr\" (driver consumes Monado face/gaze devices; runtime is a zxr client; adr/0010-avatar-control-space-and-driver.md).";
    }
  ];
}
