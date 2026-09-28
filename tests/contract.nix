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
    config.mura.device.maintainers = lib.mkForce [ "someone" ];
    config.mura.qualification.readinessCheck = "xr-smoke";
  };
  xrFunctionalNoReadiness = {
    imports = [ validDevice ];
    config.mura.device.supportTier = lib.mkForce "xr-functional";
    config.mura.device.maintainers = lib.mkForce [ "someone" ];
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

  # Galaxy XR shape: the Top button is the PMIC power key — select and power share a code.
  inputSelectSharesPower = {
    imports = [ validDevice ];
    config.mura.hardware.input = {
      hmdButtons = { power = "KEY_POWER"; top = "KEY_POWER"; volumeUp = "KEY_VOLUMEUP"; volumeDown = "KEY_VOLUMEDOWN"; };
      selectRole = "top";
    };
  };

  # No back button is allowed (scenes expose an on-scene cancel)...
  inputNoBack = {
    imports = [ validDevice ];
    config.mura.hardware.input.backRole = null;
  };

  # ...but a back role naming a missing button fails.
  # native-openxr-apps §6: the reserved system control may share the select button (Steam
  # Frame Aux) ...
  inputSystemSharesSelect = {
    imports = [ validDevice ];
    config.mura.hardware.input = {
      hmdButtons = { power = "KEY_POWER"; volumeUp = "KEY_VOLUMEUP"; volumeDown = "KEY_VOLUMEDOWN"; select = "KEY_SELECT"; };
      selectRole = "select";
      systemRole = "select";
    };
  };

  # ...and a system role naming a missing button fails.
  inputSystemMissing = {
    imports = [ validDevice ];
    config.mura.hardware.input.systemRole = "aux";
  };

  inputBackMissing = {
    imports = [ validDevice ];
    config.mura.hardware.input.backRole = "aux";
  };

  # D2/D4: a device may tighten the faillock ladder and lengthen the readiness bound.
  faillockStrict = {
    imports = [ validDevice ];
    config.mura.xr.session = {
      faillock = { deny = 3; unlockSeconds = 900; };
      readinessTimeoutSeconds = 45;
    };
    config.mura.oob.hotspot.idleTimeoutMinutes = 25;
    config.mura.health.crashLoopThreshold = 5;
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
    # NixOS's own UID_MAX (nixbld starts at 30000; research/78 §9 F9 — Debian's 60000 broke the login)
    multiUserWindowLoginDefs =
      eval.config.mura.xr.session.multiUser.uidRange.max == 29999;
    multiUserBadUidRangeFails = !assertsPass (evalContract multiUserBadUidRange);
    # research/42 §7 / first-run-onboarding §4.4: input-floor facts default to power +
    # volume with volumeUp as select (the PICO Head-Control-Mode shape); a dedicated select
    # passes; a select role the HMD lacks fails.
    inputDefaultSelectVolumeUp = eval.config.mura.hardware.input.selectRole == "volumeUp";
    inputDefaultControllersNone = eval.config.mura.hardware.input.controllers == "none";
    inputDefaultApStaUnknown = eval.config.mura.hardware.input.concurrentApSta == null;
    inputSelectAuxPasses = assertsPass (evalContract inputSelectAux);
    inputSelectMissingFails = !assertsPass (evalContract inputSelectMissing);
    inputSelectSharesPowerPasses = assertsPass (evalContract inputSelectSharesPower);
    inputDefaultBackVolumeDown = eval.config.mura.hardware.input.backRole == "volumeDown";
    inputNoBackPasses = assertsPass (evalContract inputNoBack);
    inputBackMissingFails = !assertsPass (evalContract inputBackMissing);
    inputDefaultNoSystemRole = eval.config.mura.hardware.input.systemRole == null;
    inputSystemSharesSelectPasses = assertsPass (evalContract inputSystemSharesSelect);
    inputSystemMissingFails = !assertsPass (evalContract inputSystemMissing);
    # Lock triggers default sensibly.
    lockDefaultsOn = eval.config.mura.xr.session.lock.enable == true;
    # multi-user.md §3 / D2: the faillock ladder is a schema value (constraint 9), never
    # compiled in; defaults are 5 failures, 5 minutes.
    faillockDefaultDeny = eval.config.mura.xr.session.faillock.deny == 5;
    faillockDefaultUnlock = eval.config.mura.xr.session.faillock.unlockSeconds == 300;
    faillockOverridable =
      (evalContract faillockStrict).config.mura.xr.session.faillock.deny == 3;
    # specs/session-bootstrap.md §4 step 4 / D4: the readiness bound is a schema value.
    readinessTimeoutDefault = eval.config.mura.xr.session.readinessTimeoutSeconds == 30;
    # first-run §5.4 / D3: the gadget is a hardware fact, the hotspot idle timeout a schema value.
    usbGadgetDefaultOn = eval.config.mura.hardware.input.usbGadget == true;
    # implementation-path §3a/§3a-bis / D6: health values are schema values.
    crashLoopDefault = eval.config.mura.health.crashLoopThreshold == 3;
    deviceWaitDefault = eval.config.mura.health.deviceWaitSeconds == 10;
    bootTriesDefault = eval.config.mura.deployment.bootTries == 3;
    crashLoopOverridable = (evalContract faillockStrict).config.mura.health.crashLoopThreshold == 5;
    hotspotIdleDefault = eval.config.mura.oob.hotspot.idleTimeoutMinutes == 10;
    hotspotIdleOverridable =
      (evalContract faillockStrict).config.mura.oob.hotspot.idleTimeoutMinutes == 25;
    readinessTimeoutOverridable =
      (evalContract faillockStrict).config.mura.xr.session.readinessTimeoutSeconds == 45;
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
