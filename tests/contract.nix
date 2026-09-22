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

  # A device with an XR shell + a valid appliance session profile (ADR 0007).
  applianceSession = {
    imports = [ validDevice ];
    config = {
      spatial.xr.shell = "zxr";
      spatial.xr.session.autoLogin = "owner";
    };
  };

  # A device with an XR shell + a valid multi-user greeter profile.
  greeterSession = {
    imports = [ validDevice ];
    config = {
      spatial.xr.shell = "zxr";
      spatial.xr.session.greeter = "zxr-greeter";
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
    # Lock triggers default sensibly.
    lockDefaultsOn = eval.config.spatial.xr.session.lock.enable == true;
  };

  failures = lib.filterAttrs (_: v: v != true) results;
in
if failures == { }
then nixpkgs.legacyPackages.${system}.runCommand "spatial-contract-tests-pass" { } "echo ok > $out"
else throw "spatial contract tests failed: ${builtins.toJSON (builtins.attrNames failures)}"
