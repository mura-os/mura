# Evaluation-time tests for the device contract.
#
# These are pure evaluations (no build) that assert the contract's typing and
# cross-field assertions behave as intended. Run via `nix flake check` / the
# checks.<system>.contract output.
{ nixpkgs, system }:
let
  lib = nixpkgs.lib;

  # Minimal declaration of `assertions` so the contract module (which sets
  # config.assertions) can be evaluated standalone, outside a full NixOS toplevel.
  assertionsOption = { lib, ... }: {
    options.assertions = lib.mkOption {
      type = lib.types.listOf (lib.types.submodule {
        options.assertion = lib.mkOption { type = lib.types.bool; };
        options.message = lib.mkOption { type = lib.types.str; };
      });
      default = [ ];
    };
    options.warnings = lib.mkOption { type = lib.types.listOf lib.types.str; default = [ ]; };
  };

  # Evaluate a device module against the contract alone — we only want the spatial.*
  # options and assertions, not a full NixOS toplevel.
  evalContract = mod: (lib.evalModules {
    modules = [ ../lib/contract assertionsOption mod ];
    specialArgs = { inherit lib; };
  });

  # A minimal valid device declaration.
  validDevice = { ... }: {
    spatial.device = {
      codename = "t";
      vendor = "v";
      name = "T";
      arch = "aarch64";
      supportTier = "booting";
      maintainers = [ ];
    };
    spatial.hardware.soc = "sm8250";
    spatial.hardware.panel = { width = 1600; height = 1600; refresh = 90; };
    spatial.deployment.bootScheme = "android-bootimg";
    spatial.kernel.bootimg.headerVersion = 2;
  };

  eval = evalContract validDevice;

  # Tier above 'booting' with and without a readiness check (registry §10.2 catch-up).
  xrFunctionalWithReadiness = {
    imports = [ validDevice ];
    config.spatial.device.supportTier = lib.mkForce "xr-functional";
    config.spatial.device.maintainers = lib.mkForce [ "j" ];
    config.spatial.qualification.readinessCheck = "xr-smoke";
  };
  xrFunctionalNoReadiness = {
    imports = [ validDevice ];
    config.spatial.device.supportTier = lib.mkForce "xr-functional";
    config.spatial.device.maintainers = lib.mkForce [ "j" ];
  };

  # A device with an XR shell + a valid appliance session profile (ADR 0007).
  applianceSession = {
    imports = [ validDevice ];
    config = {
      spatial.xr.shell = "zxr";
      spatial.xr.session.autoLogin = "owner";
    };
  };

  # A device with an XR shell + a valid multi-user greeter profile.
  # ADR 0017: a greeter requires onboarding placement (greeter-gated dispatcher).
  greeterSession = {
    imports = [ validDevice ];
    config = {
      spatial.xr.shell = "zxr";
      spatial.xr.session.greeter = "zxr-greeter";
      spatial.xr.session.provisioning.mode = "greeter-gated";
    };
  };

  # ADR 0017: a greeter WITHOUT onboarding placement must fail.
  greeterNoProvisioning = {
    imports = [ validDevice ];
    config = {
      spatial.xr.shell = "zxr";
      spatial.xr.session.greeter = "zxr-greeter";
    };
  };

  # ADR 0018: multi-account + guest on the multi-user profile passes.
  multiUserWithGuest = {
    imports = [ validDevice ];
    config = {
      spatial.xr.shell = "zxr";
      spatial.xr.session.greeter = "zxr-greeter";
      spatial.xr.session.provisioning.mode = "greeter-gated";
      spatial.xr.session.multiUser.enable = true;
      spatial.xr.session.guest.enable = true;
    };
  };

  # ADR 0018: multi-account on the appliance profile must fail (single-owner by design).
  multiUserOnAppliance = {
    imports = [ validDevice ];
    config = {
      spatial.xr.shell = "zxr";
      spatial.xr.session.autoLogin = "owner";
      spatial.xr.session.multiUser.enable = true;
    };
  };

  # multi-user.md §1.1: a malformed uid window must fail (min > max).
  multiUserBadUidRange = {
    imports = [ validDevice ];
    config = {
      spatial.xr.shell = "zxr";
      spatial.xr.session.greeter = "zxr-greeter";
      spatial.xr.session.provisioning.mode = "greeter-gated";
      spatial.xr.session.multiUser.enable = true;
      spatial.xr.session.multiUser.uidRange = {
        min = 1099;
        max = 1000;
      };
    };
  };

  # ADR 0018: guest without multiUser must fail (greeter-scene affordance).
  guestWithoutMultiUser = {
    imports = [ validDevice ];
    config = {
      spatial.xr.shell = "zxr";
      spatial.xr.session.greeter = "zxr-greeter";
      spatial.xr.session.provisioning.mode = "greeter-gated";
      spatial.xr.session.guest.enable = true;
    };
  };

  # Invalid: shell set but NEITHER session profile chosen.
  noProfile = {
    imports = [ validDevice ];
    config = {
      spatial.xr.shell = "zxr";
    };
  };

  # Invalid: shell set but BOTH profiles chosen.
  bothProfiles = {
    imports = [ validDevice ];
    config = {
      spatial.xr.shell = "zxr";
      spatial.xr.session.autoLogin = "owner";
      spatial.xr.session.greeter = "zxr-greeter";
    };
  };

  # ADR 0011: motorized auto-IPD with an eyes backend passes...
  motorizedIpdWithEyes = {
    imports = [ validDevice ];
    config = {
      spatial.hardware.ipd.source = "motorized-auto";
      spatial.adaptation.eyes.backend = "device-specific";
    };
  };

  # ...and without one fails.
  motorizedIpdNoEyes = {
    imports = [ validDevice ];
    config = {
      spatial.hardware.ipd.source = "motorized-auto";
    };
  };

  # Assertion helpers.
  assertsPass = e: builtins.all (a: a.assertion) e.config.assertions;

  results = {
    # The valid device evaluates and passes all contract assertions.
    validDevicePasses = assertsPass eval;
    # codename is threaded through.
    codenameSet = eval.config.spatial.device.codename == "t";
    # default backend is native.
    defaultBackendNative = eval.config.spatial.adaptation.gpu.backend == "native";
    # tracking defaults to device-specific.
    trackingDeviceSpecific = eval.config.spatial.adaptation.tracking.backend == "device-specific";
    # ADR 0007: headless bring-up (shell = none) needs no session profile.
    headlessExemptFromProfile = assertsPass eval;
    # ADR 0007: appliance and greeter profiles each pass.
    applianceProfilePasses = assertsPass (evalContract applianceSession);
    greeterProfilePasses = assertsPass (evalContract greeterSession);
    # ADR 0007: neither / both profiles must fail the exactly-one assertion.
    noProfileFails = !assertsPass (evalContract noProfile);
    bothProfilesFail = !assertsPass (evalContract bothProfiles);
    # ADR 0017: greeter without onboarding placement fails; provisioning defaults off;
    # the marker default lives in the enrollment state class.
    greeterNoProvisioningFails = !assertsPass (evalContract greeterNoProvisioning);
    provisioningDefaultNone = eval.config.spatial.xr.session.provisioning.mode == "none";
    provisioningMarkerInEnrollment =
      eval.config.spatial.xr.session.provisioning.markerPath
      == "/var/lib/spatial/enrollment/provisioned";
    # ADR 0018: multi-account/guest profile coupling + defaults.
    multiUserWithGuestPasses = assertsPass (evalContract multiUserWithGuest);
    multiUserOnApplianceFails = !assertsPass (evalContract multiUserOnAppliance);
    guestWithoutMultiUserFails = !assertsPass (evalContract guestWithoutMultiUser);
    multiUserDefaultOff = eval.config.spatial.xr.session.multiUser.enable == false;
    # ADR 0018 rev 3: no account cap exists anywhere in the contract.
    multiUserNoCapOption = !(eval.options.spatial.xr.session.multiUser ? maxAccounts);
    multiUserWindowLoginDefs =
      eval.config.spatial.xr.session.multiUser.uidRange.max == 60000;
    multiUserBadUidRangeFails = !assertsPass (evalContract multiUserBadUidRange);
    # Lock triggers default sensibly.
    lockDefaultsOn = eval.config.spatial.xr.session.lock.enable == true;
    # ADR 0011: eyes defaults to none; ipd defaults to fixed @ 63mm.
    eyesDefaultNone = eval.config.spatial.adaptation.eyes.backend == "none";
    ipdDefaultFixed = eval.config.spatial.hardware.ipd.source == "fixed";
    # ADR 0011: motorized-auto requires an eyes backend.
    motorizedIpdWithEyesPasses = assertsPass (evalContract motorizedIpdWithEyes);
    motorizedIpdNoEyesFails = !assertsPass (evalContract motorizedIpdNoEyes);
    # Registry §10.2 catch-up: doc-listed options now exist with sane defaults.
    kernelDtbsDefaultEmpty = eval.config.spatial.kernel.dtbs == [ ];
    kernelSourceDefaultNull = eval.config.spatial.kernel.source == null;
    monadoRevDefaultNull = eval.config.spatial.xr.monado.rev == null;
    slamPackageDefaultNull = eval.config.spatial.xr.tracking.slam.package == null;
    calibrationPathsDefaultEmpty = eval.config.spatial.xr.calibration.paths == { };
    # Tier gate: above 'booting' requires a readiness check.
    xrFunctionalWithReadinessPasses = assertsPass (evalContract xrFunctionalWithReadiness);
    xrFunctionalNoReadinessFails = !assertsPass (evalContract xrFunctionalNoReadiness);
  };

  failures = lib.filterAttrs (_: v: v != true) results;
in
if failures == { }
then nixpkgs.legacyPackages.${system}.runCommand "spatial-contract-tests-pass" { } "echo ok > $out"
else throw "spatial contract tests failed: ${builtins.toJSON (builtins.attrNames failures)}"
