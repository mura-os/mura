# XR runtime and session wiring.
#
# Monado as the system OpenXR runtime, out-of-process, socket-activated, with
# /etc/xdg/openxr/1/active_runtime.json declared here — never symlink-flipped at
# runtime (docs/architecture/device-contract.md §xr, docs/research/05-xr-userspace.md
# §9 item 1). The services.monado module is upstream nixpkgs'; this module drives it
# from the mura.* contract and layers per-device config. (The pre-D0 "unavailable in
# this nixpkgs" stub is gone: the pinned nixpkgs provides the module.)
#
# The Monado the module runs is `pkgs.monado` after Mura's overlay (pkgs/monado/default.nix):
# nixpkgs-xr's package with its `src` swapped for the pinned `mura-os/monado` fork — upstream
# `main` plus Mura's upstream-shaped series, where the C-track lands (ADR 0006 amendment 4 D13:
# the spatial-container pair, the controller seam, the depth policy, the dmabuf-import
# swapchain; specs/composition.md). `services.monado.package` is the override point if a
# profile needs a different build; nothing here is per-device (that is `mura.xr.monado.*`).
{ lib, config, pkgs, ... }:
let
  cfg = config.mura.xr;
in
{
  config = lib.mkMerge [
    (lib.mkIf (cfg.runtime == "monado") {
      services.monado = {
        enable = true;
        defaultRuntime = true; # materializes /etc/xdg/openxr/1/active_runtime.json
      };

      # C0 — admission classes and the controller lease (specs/composition.md §5.3, ADR 0006
      # amendment 5). The fork's service listens on two sockets and stamps each connection
      # with its class at accept: `monado_comp_ipc` → app, `monado_comp_ipc_control` →
      # controller (the fork's monado-control.in.socket; nixpkgs' module mirrors upstream's
      # units in Nix rather than shipping the files, so the second unit is mirrored here the
      # same way). systemd hands both fds to the service by name (FileDescriptorName=app /
      # control, sd_listen_fds_with_names). The same $XDG_RUNTIME_DIR holds both; who may
      # reach the control socket is the ordinary Unix question (0700 runtime dir; a sandbox
      # that does not bind-mount it) — not a credential scheme (§5.3.1, 5.3.6).
      systemd.user.services.monado = {
        requires = [ "monado-control.socket" ];
        environment = cfg.environment // {
          # Mura's shell owns the session, so the control verbs are the lease holder's from
          # first boot; upstream's default (false) keeps them open while no controller is
          # connected for unmodified monado-ctl users (§5.3.3). A profile may still override.
          IPC_REQUIRE_CONTROLLER = lib.mkDefault "true";
        };
      };
      systemd.user.sockets.monado.socketConfig.FileDescriptorName = "app";
      systemd.user.sockets.monado-control = {
        description = "Monado XR service module control socket";
        conflicts = [ "monado-dev.service" ];
        unitConfig.ConditionUser = "!root";
        socketConfig = {
          ListenStream = "%t/monado_comp_ipc_control";
          FileDescriptorName = "control";
          Service = "monado.service";
          RemoveOnStop = true;
          FlushPending = true;
        };
        restartTriggers = [ config.services.monado.package ];
        wantedBy = [ "sockets.target" ];
      };
    })

    (lib.mkIf (cfg.runtime == "wivrn") {
      warnings = [ "mura.xr.runtime = wivrn: WiVRn server wiring is not yet implemented in the scaffold." ];
    })
  ];
}
