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
    # Minimal stand-in for NixOS's `users.users` so the declared-account assertions
    # (ADR 0017 rev 2) can be exercised standalone.
    options.users.users = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule {
        options.isNormalUser = lib.mkOption { type = lib.types.bool; default = false; };
      });
      default = { };
    };
  };

  # The default image's declared user (first-run-onboarding.md §1).
  declaredMura = { users.users.mura.isNormalUser = true; };

  # Evaluate a device module against the contract alone — we only want the mura.*
  # options and assertions, not a full NixOS toplevel.
  evalContract = mod: (lib.evalModules {
    modules = [ ../lib/contract assertionsOption mod ];
    specialArgs = { inherit lib; };
  });

  # A minimal valid device declaration.
  validDevice = { ... }: {
    mura.device = {
      codename = "t";
      vendor = "v";
      name = "T";
      arch = "aarch64";
      supportTier = "booting";
      maintainers = [ ];
    };
    mura.hardware.soc = "sm8250";
    mura.hardware.panel = { width = 1600; height = 1600; refresh = 90; };
    mura.deployment.bootScheme = "android-bootimg";
    mura.kernel.bootimg.headerVersion = 2;
  };

  eval = evalContract validDevice;

  # Tier above 'booting' with and without a readiness check (registry §10.2 catch-up).
  xrFunctionalWithReadiness = {
    imports = [ validDevice ];
    config.mura.device.supportTier = lib.mkForce "xr-functional";
    config.mura.device.maintainers = lib.mkForce [ "j" ];
    config.mura.qualification.readinessCheck = "xr-smoke";
  };
  xrFunctionalNoReadiness = {
    imports = [ validDevice ];
    config.mura.device.supportTier = lib.mkForce "xr-functional";
    config.mura.device.maintainers = lib.mkForce [ "j" ];
  };

  # A device with an XR shell + a valid appliance session profile (ADR 0007): the default
  # image shape — declared `mura`, autologin (ADR 0017 rev 2).
  applianceSession = {
    imports = [ validDevice declaredMura ];
    config = {
      mura.xr.shell = "zxr";
      mura.xr.session.autoLogin = "mura";
    };
  };

  # ADR 0017 rev 2: autologin naming an undeclared user must fail...
  applianceUndeclaredUser = {
    imports = [ validDevice ];
    config = {
      mura.xr.shell = "zxr";
      mura.xr.session.autoLogin = "nobody-here";
    };
  };

  # ...unless the escape hatch is set.
  applianceUndeclaredUserEscape = {
    imports = [ validDevice ];
    config = {
      mura.xr.shell = "zxr";
      mura.xr.session.autoLogin = "nobody-here";
      mura.xr.session.allowNoDeclaredAccount = true;
    };
  };

  # A device with an XR shell + a valid multi-user greeter profile.
  # ADR 0017 rev 2: the image is the installation — a greeter image declares its first account.
  greeterSession = {
    imports = [ validDevice declaredMura ];
    config = {
      mura.xr.shell = "zxr";
      mura.xr.session.greeter = "zxr-greeter";
    };
  };

  # ADR 0017 rev 2: a greeter WITHOUT any declared human account must fail (no runtime
  # bootstrap screen exists to rescue it)...
  greeterNoAccount = {
    imports = [ validDevice ];
    config = {
      mura.xr.shell = "zxr";
      mura.xr.session.greeter = "zxr-greeter";
    };
  };

  # ...unless the administrator insists (the users.allowNoPasswordLogin pattern).
  greeterNoAccountEscape = {
    imports = [ validDevice ];
    config = {
      mura.xr.shell = "zxr";
      mura.xr.session.greeter = "zxr-greeter";
      mura.xr.session.allowNoDeclaredAccount = true;
    };
  };

  # A system account (isNormalUser = false) does not satisfy the declared-account rule.
  greeterOnlySystemAccount = {
    imports = [ validDevice ];
    config = {
      mura.xr.shell = "zxr";
      mura.xr.session.greeter = "zxr-greeter";
      users.users.svc.isNormalUser = false;
    };
  };

  # ADR 0018: multi-account + guest on the multi-user profile passes.
  multiUserWithGuest = {
    imports = [ validDevice declaredMura ];
    config = {
      mura.xr.shell = "zxr";
      mura.xr.session.greeter = "zxr-greeter";
      mura.xr.session.multiUser.enable = true;
      mura.xr.session.guest.enable = true;
    };
  };

  # ADR 0018: multi-account on the appliance profile must fail (single-owner by design).
  multiUserOnAppliance = {
    imports = [ validDevice declaredMura ];
    config = {
      mura.xr.shell = "zxr";
      mura.xr.session.autoLogin = "mura";
      mura.xr.session.multiUser.enable = true;
    };
  };

  # multi-user.md §1.1: a malformed uid window must fail (min > max).
  multiUserBadUidRange = {
    imports = [ validDevice declaredMura ];
    config = {
      mura.xr.shell = "zxr";
      mura.xr.session.greeter = "zxr-greeter";
      mura.xr.session.multiUser.enable = true;
      mura.xr.session.multiUser.uidRange = {
        min = 1099;
        max = 1000;
      };
    };
  };

  # ADR 0018: guest without multiUser must fail (greeter-scene affordance).
  guestWithoutMultiUser = {
    imports = [ validDevice declaredMura ];
    config = {
      mura.xr.shell = "zxr";
      mura.xr.session.greeter = "zxr-greeter";
      mura.xr.session.guest.enable = true;
    };
  };

  # Invalid: shell set but NEITHER session profile chosen.
  noProfile = {
    imports = [ validDevice ];
    config = {
      mura.xr.shell = "zxr";
    };
  };

  # Invalid: shell set but BOTH profiles chosen.
  bothProfiles = {
    imports = [ validDevice declaredMura ];
    config = {
      mura.xr.shell = "zxr";
      mura.xr.session.autoLogin = "mura";
      mura.xr.session.greeter = "zxr-greeter";
    };
  };

  # ADR 0011: motorized auto-IPD with an eyes backend passes...
  motorizedIpdWithEyes = {
    imports = [ validDevice ];
    config = {
      mura.hardware.ipd.source = "motorized-auto";
      mura.adaptation.eyes.backend = "device-specific";
    };
  };

  # ...and without one fails.
  motorizedIpdNoEyes = {
    imports = [ validDevice ];
    config = {
      mura.hardware.ipd.source = "motorized-auto";
    };
  };

  # research/42: a dedicated select button (Steam Frame Aux) passes...
  inputSelectAux = {
    imports = [ validDevice ];
    config.mura.hardware.input = {
      hmdButtons = { power = "KEY_POWER"; volumeUp = "KEY_VOLUMEUP"; volumeDown = "KEY_VOLUMEDOWN"; select = "KEY_SELECT"; };
      selectRole = "select";
    };
  };

  # ...and a select role naming a button the HMD does not have must fail.
  inputSelectMissing = {
    imports = [ validDevice ];
    config.mura.hardware.input.selectRole = "select";
  };

  # Assertion helpers.
  assertsPass = e: builtins.all (a: a.assertion) e.config.assertions;

  results = {
    # The valid device evaluates and passes all contract assertions.
    validDevicePasses = assertsPass eval;
    # codename is threaded through.
    codenameSet = eval.config.mura.device.codename == "t";
    # default backend is native.
    defaultBackendNative = eval.config.mura.adaptation.gpu.backend == "native";
    # tracking defaults to device-specific.
    trackingDeviceSpecific = eval.config.mura.adaptation.tracking.backend == "device-specific";
    # ADR 0007: headless bring-up (shell = none) needs no session profile.
    headlessExemptFromProfile = assertsPass eval;
    # ADR 0007: appliance and greeter profiles each pass.
    applianceProfilePasses = assertsPass (evalContract applianceSession);
    greeterProfilePasses = assertsPass (evalContract greeterSession);
    # ADR 0007: neither / both profiles must fail the exactly-one assertion.
    noProfileFails = !assertsPass (evalContract noProfile);
    bothProfilesFail = !assertsPass (evalContract bothProfiles);
    # ADR 0017 rev 2: the image is the installation — declared-account assertions.
    greeterNoAccountFails = !assertsPass (evalContract greeterNoAccount);
    greeterNoAccountEscapePasses = assertsPass (evalContract greeterNoAccountEscape);
    greeterOnlySystemAccountFails = !assertsPass (evalContract greeterOnlySystemAccount);
    applianceUndeclaredUserFails = !assertsPass (evalContract applianceUndeclaredUser);
    applianceUndeclaredUserEscapePasses = assertsPass (evalContract applianceUndeclaredUserEscape);
    allowNoDeclaredAccountDefaultOff =
      eval.config.mura.xr.session.allowNoDeclaredAccount == false;
    # ADR 0017 rev 2: no onboarding-placement or provisioning-marker options exist any more
    # (no pre-login wizard, no dispatcher, no marker gating UI).
    noProvisioningOptions = !(eval.options.mura.xr.session ? provisioning);
    # ADR 0018: multi-account/guest profile coupling + defaults.
    multiUserWithGuestPasses = assertsPass (evalContract multiUserWithGuest);
    multiUserOnApplianceFails = !assertsPass (evalContract multiUserOnAppliance);
    guestWithoutMultiUserFails = !assertsPass (evalContract guestWithoutMultiUser);
    multiUserDefaultOff = eval.config.mura.xr.session.multiUser.enable == false;
    # ADR 0018 rev 3: no account cap exists anywhere in the contract.
    multiUserNoCapOption = !(eval.options.mura.xr.session.multiUser ? maxAccounts);
    multiUserWindowLoginDefs =
      eval.config.mura.xr.session.multiUser.uidRange.max == 60000;
    multiUserBadUidRangeFails = !assertsPass (evalContract multiUserBadUidRange);
    # research/42 §7 / first-run-onboarding §4.4: input-floor facts default to power +
    # volume with volumeUp as select (the PICO Head-Control-Mode shape); a dedicated select
    # passes; a select role the HMD lacks fails.
    inputDefaultSelectVolumeUp = eval.config.mura.hardware.input.selectRole == "volumeUp";
    inputDefaultControllersNone = eval.config.mura.hardware.input.controllers == "none";
    inputDefaultApStaUnknown = eval.config.mura.hardware.input.concurrentApSta == null;
    inputSelectAuxPasses = assertsPass (evalContract inputSelectAux);
    inputSelectMissingFails = !assertsPass (evalContract inputSelectMissing);
    # Lock triggers default sensibly.
    lockDefaultsOn = eval.config.mura.xr.session.lock.enable == true;
    # ADR 0011: eyes defaults to none; ipd defaults to fixed @ 63mm.
    eyesDefaultNone = eval.config.mura.adaptation.eyes.backend == "none";
    ipdDefaultFixed = eval.config.mura.hardware.ipd.source == "fixed";
    # ADR 0011: motorized-auto requires an eyes backend.
    motorizedIpdWithEyesPasses = assertsPass (evalContract motorizedIpdWithEyes);
    motorizedIpdNoEyesFails = !assertsPass (evalContract motorizedIpdNoEyes);
    # Registry §10.2 catch-up: doc-listed options now exist with sane defaults.
    kernelDtbsDefaultEmpty = eval.config.mura.kernel.dtbs == [ ];
    kernelSourceDefaultNull = eval.config.mura.kernel.source == null;
    monadoRevDefaultNull = eval.config.mura.xr.monado.rev == null;
    slamPackageDefaultNull = eval.config.mura.xr.tracking.slam.package == null;
    calibrationPathsDefaultEmpty = eval.config.mura.xr.calibration.paths == { };
    # Tier gate: above 'booting' requires a readiness check.
    xrFunctionalWithReadinessPasses = assertsPass (evalContract xrFunctionalWithReadiness);
    xrFunctionalNoReadinessFails = !assertsPass (evalContract xrFunctionalNoReadiness);
  };

  failures = lib.filterAttrs (_: v: v != true) results;
in
if failures == { }
then nixpkgs.legacyPackages.${system}.runCommand "mura-contract-tests-pass" { } "echo ok > $out"
else throw "mura contract tests failed: ${builtins.toJSON (builtins.attrNames failures)}"
