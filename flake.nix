{
  description = "Mura — a Nix-built, NixOS-based, Wayland XR distribution for standalone VR headsets";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    # XR stack (Monado, WiVRn, ...) with its own nvfetcher pin architecture and cache.
    # Reused rather than repackaged (ADR 0005). Follows our nixpkgs to avoid divergence.
    nixpkgs-xr = {
      url = "github:nix-community/nixpkgs-xr";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, nixpkgs-xr, treefmt-nix, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: import nixpkgs {
        inherit system;
        overlays = [ nixpkgs-xr.overlays.default (import ./pkgs) ];
      };

      treefmtEval = forAll (system:
        treefmt-nix.lib.evalModule (pkgsFor system) {
          projectRootFile = "flake.nix";
          programs.nixpkgs-fmt.enable = true;
          programs.shfmt.enable = true;
        });

      # The single integration path (ADR 0005): evaluate a device into a NixOS system.
      muraSystem = import ./lib/eval-device.nix { inherit nixpkgs; };
    in
    {
      # The Mura module set — importable into any NixOS configuration.
      nixosModules.default = { imports = import ./modules; };

      # Helper re-exported so downstream flakes can build their own devices.
      lib.muraSystem = muraSystem;

      # Device evaluations. A device is composed with a *profile* (profiles/ — the declared
      # configuration an installer would have produced; docs/architecture/repo-structure.md).
      # The VM device exists in two fixtures so every D-track rung is verified on both
      # login profiles (implementation-path §3c):
      #   virtual-headset            — the default-image shape: `mura`, no password, autologin
      #   virtual-headset-multiuser  — the greeter shape: a declared account + the stand-in greeter
      nixosConfigurations.virtual-headset = muraSystem {
        device = ./devices/virtual-headset;
        system = "x86_64-linux";
        extraModules = [
          { nixpkgs.overlays = [ nixpkgs-xr.overlays.default (import ./pkgs) ]; }
          ./profiles/default.nix
        ];
      };
      nixosConfigurations.virtual-headset-multiuser = muraSystem {
        device = ./devices/virtual-headset;
        system = "x86_64-linux";
        extraModules = [
          { nixpkgs.overlays = [ nixpkgs-xr.overlays.default (import ./pkgs) ]; }
          ./profiles/multi-user.nix
          # VM FIXTURE ONLY: the declared account (mura / mura) — see the file's header.
          ./tests/vm/fixture-user.nix
        ];
      };

      # Valve Steam Frame (deckard): the first real device target. aarch64 artifacts
      # build on remote aarch64 builders (ADR 0004; nixbuild.net) and are excluded
      # from `nix flake check`.
      nixosConfigurations.valve-steam-frame = muraSystem {
        device = ./devices/valve-steam-frame;
        system = "aarch64-linux";
        extraModules = [{ nixpkgs.overlays = [ nixpkgs-xr.overlays.default (import ./pkgs) ]; }];
      };

      # Named, discoverable outputs (no untyped grab-bag).
      packages = forAll (system:
        {
          # Mura's own programs (all Rust; AGENTS.md rule 6). Built on both architectures;
          # the x86_64 ones are also `checks`, and tests/closure.nix proves their closure
          # carries no interpreter.
          mura-authd = (pkgsFor system).mura.authd; # the lock-path PAM helper (D5)
          mura-session = (pkgsFor system).mura.session; # the session wrapper greetd execs (D4 rev 3)
          mura-preflight = (pkgsFor system).mura.preflight; # the XR preflight probe (D6)
          mura-setup = (pkgsFor system).mura.setup; # the setup program's system instance, D3 stub
        }
        // nixpkgs.lib.optionalAttrs (system == "x86_64-linux")
          {
            # The dev-vm smoke targets: bootable NixOS VMs running the common userspace —
            # the default-image fixture (autologin) and the multi-user fixture (greeter).
            virtual-headset-vm = self.nixosConfigurations.virtual-headset.config.system.build.vm;
            virtual-headset-multiuser-vm = self.nixosConfigurations.virtual-headset-multiuser.config.system.build.vm;

            # QEMU runner + smoke checks for the Frame image (runs the aarch64 disk
            # image via qemu-system-aarch64 full-system emulation on the dev host).
            frame-vm-run = (pkgsFor system).callPackage ./pkgs/frame-vm-run { };

            # Rung-1 dev loop: nested session window + simulated-HMD Monado.
            dev-session = (pkgsFor system).callPackage ./pkgs/dev-session { };

            # D-track VM tests (implementation-path §3c) — on demand, NOT in `nix flake check`
            # (each boots a VM and takes minutes): `nix build .#vm-test-default-image`.
            vm-test-default-image = import ./tests/vm/default-image.nix { pkgs = pkgsFor system; };
            vm-test-multi-user = import ./tests/vm/multi-user.nix { pkgs = pkgsFor system; };
            vm-test-oob = import ./tests/vm/oob.nix { pkgs = pkgsFor system; };
            vm-test-health = import ./tests/vm/health.nix { pkgs = pkgsFor system; };
            vm-test-recovery = import ./tests/vm/recovery.nix { pkgs = pkgsFor system; };
          }
        // nixpkgs.lib.optionalAttrs (system == "aarch64-linux") {
          # Steam Frame uefi-rauc artifacts (build via remote aarch64 builder).
          frame-image = self.nixosConfigurations.valve-steam-frame.config.system.build.image;
          frame-bundle = self.nixosConfigurations.valve-steam-frame.config.system.build.raucBundle;
        });

      apps = nixpkgs.lib.genAttrs [ "x86_64-linux" ] (system: {
        dev-session = {
          type = "app";
          program = "${self.packages.${system}.dev-session}/bin/dev-session";
          meta.description = "Rung-1 dev loop: nested spatial session + simulated-HMD Monado";
        };
      });

      formatter = forAll (system: treefmtEval.${system}.config.build.wrapper);

      devShells = forAll (system:
        let pkgs = pkgsFor system; in {
          default = pkgs.mkShell {
            packages = with pkgs; [
              nixpkgs-fmt
              # Donor-pipeline tooling (docs/architecture/donor-pipeline.md format zoo).
              android-tools
              payload-dumper-go
              erofs-utils
              squashfs-tools
              rauc
              desync
            ];
          };
        });

      # Checks run on the x86_64 CI/dev arch. aarch64 device artifacts are built on
      # native aarch64 builders (ADR 0004), not during `nix flake check` here.
      checks.x86_64-linux =
        let system = "x86_64-linux"; in
        {
          # Formatting (treefmt: nixpkgs-fmt + shfmt).
          formatting = treefmtEval.${system}.config.build.check self;
          # Device-contract typing + cross-field assertions (pure eval).
          contract = import ./tests/contract.nix { inherit nixpkgs system; };
          # Persistent-state layout on the flashable image and the VM stand-in (pure eval —
          # the aarch64 Frame configuration is checked here without building it).
          persist = import ./tests/persist.nix { inherit nixpkgs system; configurations = self.nixosConfigurations; };
          # protocols/*.xml: well-formed + wayland-scanner generates cleanly.
          protocols = import ./tests/protocols.nix { inherit nixpkgs system; };
          # End-to-end smoke checks: both VM fixtures build (D-track, implementation-path §3c).
          virtual-headset-vm = self.packages.${system}.virtual-headset-vm;
          virtual-headset-multiuser-vm = self.packages.${system}.virtual-headset-multiuser-vm;
          # Mura's programs build (Rust; AGENTS.md rule 6).
          mura-authd = self.packages.${system}.mura-authd;
          mura-session = self.packages.${system}.mura-session;
          mura-preflight = self.packages.${system}.mura-preflight;
          mura-setup = self.packages.${system}.mura-setup;
          # The interpreter proof and the Python fence (tests/closure.nix): the closure of every
          # Mura program plus greetd carries no interpreter; the toplevels' residual nixpkgs
          # Python is a pinned, shrinking allowlist.
          closure = import ./tests/closure.nix {
            pkgs = pkgsFor system;
            configurations = self.nixosConfigurations;
          };
          # The rung-1 dev-loop harness builds (script-level shellcheck via writeShellApplication).
          dev-session = self.packages.${system}.dev-session;
        };
    };
}
