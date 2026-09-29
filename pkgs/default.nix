# Mura package overlay.
#
# XR components come from nixpkgs-xr (pulled as a flake input, per ADR 0005) rather
# than being repackaged here. This overlay is for Mura-specific packages:
# kernels and tooling; Monado itself comes from the mura-os/monado fork through the sibling
# overlay pkgs/monado (ADR 0006 amendment 4 D13), applied after this one in flake.nix.
# Empty in the scaffold beyond a marker attribute.
final: prev: {
  mura = (prev.mura or { }) // {
    # Marker so `pkgs.mura ? scaffold` is a cheap "overlay applied" check.
    scaffold = true;
    # The lock-path PAM helper + its conformance harness (specs/session-auth.md; D5).
    authd = final.callPackage ./mura-authd { };
    # TEST-ONLY PAM module for the harness (session-auth §6 items 2 and 7); never shipped.
    pamTestModule = final.callPackage ./mura-authd/test { };
    # The session wrapper greetd execs (specs/session-bootstrap.md §4; modules/os/session.nix; D4).
    session = final.callPackage ./mura-session { };
    # The XR preflight probe (implementation-path §3a-bis; modules/os/health.nix; D6).
    preflight = final.callPackage ./mura-preflight { };
    # The setup program's system instance — D3 stub (first-run §5.1; modules/os/oob.nix).
    setup = final.callPackage ./mura-setup { };
    # The recovery environment's one program: actions + menu, panel/shell frontends
    # (specs/recovery-menu.md; modules/os/recovery.nix).
    recovery = final.callPackage ./mura-recovery { };
    # The perception→compositor intake protocol library + its §8 conformance harness
    # (specs/perception-intake.md; tests/vm/perception-intake.nix). The harness bins are test-only.
    perceptionIntake = final.callPackage ./mura-perception-intake { };
    # The settings daemon + CLI (specs/settings-schema.md, specs/settings-daemon.md;
    # modules/os/settings.nix; D7).
    settingsd = final.callPackage ./mura-settingsd { };
    # The compositor (specs/zxr-core.md; ADR 0006): one OpenXR client of Monado, one Wayland
    # compositor. R0 bring-up — runs nested in the dev-session slot (`dev-session --zxr`).
    zxr = final.callPackage ./zxr { };
    # The greeter and lock program (specs/session-auth.md rev 6 §5; shell-plane §3.1; research/78):
    # Slint on the sctk platform; greetd's kiosk child in greeter mode, an ext-session-lock client
    # under a user unit in lock mode.
    greeter = final.callPackage ./mura-greeter { };
    # The on-screen keyboard (shell-plane §3.2; research/75 §3.2): squeekboard's shape on the same
    # Slint platform — zxr's `--osk` child in every mode.
    osk = final.callPackage ./mura-osk { };
    # The plymouth theme (boot / failure feedback / recovery screen) is device-specific — it is
    # composited from assets/branding with the panel geometry — so modules/os/recovery.nix calls
    # pkgs/mura-plymouth-theme directly with the contract's values; no fixed overlay attribute.
  };
}
