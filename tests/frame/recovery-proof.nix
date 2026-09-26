# Deckard-image fixture for implementation-path §4's recovery-entry proof.
#
# P2 is forced hard by naming a factory-calibration file that cannot exist in the fresh image.
# The production ladder deliberately stays on the first failed boot so a user can inspect it;
# this fixture alone cycles that first boot.  The second failure reaches the production
# `systemctl reboot --boot-loader-entry=recovery.conf` step, and the third boot must enter the
# dedicated recovery partition's initrd at mura-recovery.target. The counter, boot command, BLS
# entry and recovery target are otherwise production code.
{ lib, pkgs, ... }:
let
  proofReport = pkgs.writeShellScript "mura-recovery-proof-report" ''
    i=0
    while [ "$i" -lt 120 ]; do
      if ${pkgs.systemd}/bin/systemctl is-active --quiet \
          mura-recovery.target \
          mura-recovery-identity.service \
          mura-recovery-sshd.service \
          mura-recovery-panel.service \
          mura-setup-recovery.service; then
        cmdline=$(cat /proc/cmdline)
        case "$cmdline" in
          *'rd.systemd.unit=mura-recovery.target'*) ;;
          *) sleep 1; i=$((i + 1)); continue ;;
        esac
        selected=$(
          ${pkgs.systemd}/bin/bootctl status --no-pager 2>/dev/null \
            | ${pkgs.gnused}/bin/sed -n 's/^[[:space:]]*Current Entry:[[:space:]]*//p'
        )
        [ "$selected" = recovery.conf ] || { sleep 1; i=$((i + 1)); continue; }
        {
          echo RECOVERY_PROOF_OK
          ${pkgs.systemd}/bin/systemctl is-active \
            mura-recovery.target \
            mura-recovery-identity.service \
            mura-recovery-sshd.service \
            mura-recovery-panel.service \
            mura-setup-recovery.service
          echo "selected=$selected"
          cat /proc/cmdline
        } > /dev/ttyAMA0
        exit 0
      fi
      i=$((i + 1))
      sleep 1
    done
    {
      echo RECOVERY_PROOF_TIMEOUT
      echo "selected=$(${pkgs.systemd}/bin/bootctl status --no-pager 2>/dev/null | ${pkgs.gnused}/bin/sed -n 's/^[[:space:]]*Current Entry:[[:space:]]*//p')"
      cat /proc/cmdline
      ${pkgs.systemd}/bin/systemctl list-jobs --no-pager
      ${pkgs.systemd}/bin/systemctl --failed --no-pager
      ${pkgs.systemd}/bin/systemctl status --no-pager \
        mura-recovery.target \
        mura-recovery-identity.service \
        mura-recovery-sshd.service \
        mura-recovery-panel.service \
        mura-setup-recovery.service
    } > /dev/ttyAMA0 2>&1
    exit 1
  '';
in
{
  mura.xr.calibration.paths.factory =
    "/persist/mura/factory/forced-missing-for-recovery-proof.json";
  mura.health.crashLoopThreshold = lib.mkForce 2;

  # QEMU's display for the panel frontend; real targets supply their own DRM driver.
  boot.initrd.kernelModules = [ "virtio_gpu" ];

  # TEST ONLY: make the manual QEMU proof machine-readable on the Frame's serial console. The
  # service is Type=simple, so the target does not wait for its loop; it reports only after the
  # production target and all required frontends are active.
  boot.initrd.systemd.services.mura-recovery-proof-report = {
    description = "TEST ONLY: report recovery target completion on ttyAMA0";
    wantedBy = [ "mura-recovery.target" ];
    serviceConfig = {
      Type = "simple";
      ExecStart = proofReport;
    };
  };
  boot.initrd.systemd.storePaths = [ proofReport ];

  systemd.services.mura-crashloop.unitConfig.OnSuccess =
    "mura-recovery-proof-cycle.service";
  systemd.services.mura-crashloop.serviceConfig = {
    StandardOutput = "journal+console";
    StandardError = "journal+console";
  };
  systemd.services.mura-recovery-proof-cycle = {
    description = "TEST ONLY: cycle the first failed boot for the recovery-entry proof";
    after = [ "mura-crashloop.service" ];
    serviceConfig = {
      Type = "oneshot";
      StandardOutput = "journal+console";
      StandardError = "journal+console";
    };
    script = ''
      n=$(cat /var/lib/mura/state/health/crashloop)
      if [ "$n" -lt 2 ]; then
        echo "recovery proof: first hard failure recorded; cycling the fixture"
        ${pkgs.systemd}/bin/systemctl reboot
      fi
    '';
  };
}
