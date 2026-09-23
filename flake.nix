{
  description = "spatial-os — a Nix-built, NixOS-based, Wayland XR distribution for standalone VR headsets";

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
      spatialSystem = import ./lib/eval-device.nix { inherit nixpkgs; };
    in
    {
      # The spatial-os module set — importable into any NixOS configuration.
      nixosModules.default = { imports = import ./modules; };

      # Helper re-exported so downstream flakes can build their own devices.
      lib.spatialSystem = spatialSystem;

      # Device evaluations.
      nixosConfigurations.virtual-headset = spatialSystem {
        device = ./devices/virtual-headset;
        system = "x86_64-linux";
        extraModules = [{ nixpkgs.overlays = [ nixpkgs-xr.overlays.default (import ./pkgs) ]; }];
      };

      # Valve Steam Frame (deckard): the first real device target. aarch64 artifacts
      # build on remote aarch64 builders (ADR 0004; nixbuild.net) and are excluded
      # from `nix flake check`.
      nixosConfigurations.valve-steam-frame = spatialSystem {
        device = ./devices/valve-steam-frame;
        system = "aarch64-linux";
        extraModules = [{ nixpkgs.overlays = [ nixpkgs-xr.overlays.default (import ./pkgs) ]; }];
      };

      # Named, discoverable outputs (no untyped grab-bag).
      packages = forAll (system:
        nixpkgs.lib.optionalAttrs (system == "x86_64-linux")
          {
            # The dev-vm smoke target: a bootable NixOS VM running the common userspace.
            virtual-headset-vm = self.nixosConfigurations.virtual-headset.config.system.build.vm;

            # QEMU runner + smoke checks for the Frame image (runs the aarch64 disk
            # image via qemu-system-aarch64 full-system emulation on the dev host).
            frame-vm-run = (pkgsFor system).callPackage ./pkgs/frame-vm-run { };

            # Rung-1 dev loop: nested session window + simulated-HMD Monado.
            dev-session = (pkgsFor system).callPackage ./pkgs/dev-session { };
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
          # protocols/*.xml: well-formed + wayland-scanner generates cleanly.
          protocols = import ./tests/protocols.nix { inherit nixpkgs system; };
          # End-to-end smoke check: the virtual-headset VM builds.
          virtual-headset-vm = self.packages.${system}.virtual-headset-vm;
          # The rung-1 dev-loop harness builds (script-level shellcheck via writeShellApplication).
          dev-session = self.packages.${system}.dev-session;
        };
    };
}
