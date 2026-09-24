# modules/os/persist.nix — per-unit persistent state (implementation-path §2 (i): B1a, F1).
#
# Owns everything under /persist that is not the family's business of *mounting* the
# `syspersist` partition (families/uefi-rauc) or the VM's stand-in disk
# (devices/virtual-headset/vm-persist.nix): the state-class skeleton, the persisted /etc
# overlay (account database, machine-id, network profiles), the Bluetooth pairing class,
# and the F1 per-task-marker pattern. Design: docs/architecture/first-run-onboarding.md
# §2–§3, multi-user.md §1.1.
#
# /persist layout (first-run-onboarding.md §2 — factory reset treats each class
# differently, never the tree as one blob):
#   mura/factory/    factory calibration — survives factory reset
#   mura/identity/   device keys (incl. identity/ssh host keys) — survive reset
#   mura/enrollment/ per-user calibration + the non-secret numeric-credential hint — wiped
#   mura/pairing/    BlueZ state (/var/lib/bluetooth: link keys) — wiped
#   mura/state/      F1 per-task markers (state/provisioning/), update/migration state — per policy
#   etc-rw/          the writable upper layer of the /etc overlay: passwd/shadow/group
#                    (userborn, hybrid mode), machine-id, NetworkManager profiles, anything
#                    else written into /etc at runtime — wiped (declared accounts
#                    re-materialise, machine-id rotates, network profiles are gone)
#
# Why an /etc overlay and not symlinks into /persist (D1 finding): shadow-utils (`passwd`,
# `useradd`, `chpasswd`) write `/etc/shadow+` and rename(2) it over `/etc/shadow`. A symlink
# there is *replaced* by a slot-local regular file (verified in the VM: the new hash landed
# in /etc, /persist never saw it); a bind-mounted file makes the rename fail with EBUSY.
# NixOS's mechanism for exactly this — image-based systems with mutable /etc state — is
# `system.etc.overlay` with a writable upperdir; we put that upperdir on /persist. This is
# also what upstream's userborn tests pair userborn with.
{ lib, config, options, utils, ... }:
let
  cfg = config.mura;
  persist = "/persist";
  root = "${persist}/mura";
  etcRw = "${persist}/etc-rw";

  # NixOS mounts the /etc overlay in the initrd with upperdir=/sysroot/.rw-etc/upper; we
  # bind /.rw-etc from /persist before that. The mount unit name for a stage-1 bind.
  rwEtcMountUnit = utils.escapeSystemdPath "/sysroot/.rw-etc" + ".mount";

  # Mounts this module owns. Emitted twice below: as ordinary `fileSystems` for real images,
  # and as `virtualisation.fileSystems` where the qemu-vm module is present (VM builds and
  # tests), because that module overrides `fileSystems` wholesale (mkVMOverride).
  mounts = {
    # /var/lib/mura is the contract-visible path; it binds into /persist so it survives
    # A/B slot switches. The bind *pulls in* the setup service (x-systemd.requires —
    # ordering alone is not a dependency); the service creates the skeleton, which is what
    # makes this work after a factory reset (an image-seeded directory would not).
    "/var/lib/mura" = {
      device = root;
      fsType = "none";
      options = [
        "bind"
        "x-systemd.requires=mura-persist-setup.service"
        "x-systemd.after=mura-persist-setup.service"
      ];
    };
    # The /etc overlay's writable layer, on /persist. Stage 1: the overlay itself is a
    # stage-1 mount. The source directory is created in the initrd (below).
    "/.rw-etc" = {
      device = etcRw;
      fsType = "none";
      options = [ "bind" ];
      neededForBoot = true;
    };
  } // lib.optionalAttrs cfg.hardware.input.bluetooth {
    # pairing class: BlueZ's link keys are shared secrets with every paired controller/
    # keyboard/phone — persisted across updates, wiped by factory reset.
    "/var/lib/bluetooth" = {
      device = "${root}/pairing";
      fsType = "none";
      options = [
        "bind"
        "x-systemd.requires=mura-persist-setup.service"
        "x-systemd.after=mura-persist-setup.service"
      ];
    };
  };

  inVm = options ? virtualisation && options.virtualisation ? fileSystems;
in
{
  config = lib.mkMerge [
    { fileSystems = mounts; }
    (lib.optionalAttrs inVm { virtualisation.fileSystems = mounts; })

    {
      # Mura's boot is systemd stage 1 (the family already requires it; the persist classes
      # and the /etc overlay depend on initrd-time mounts).
      boot.initrd.systemd.enable = true;

      # /etc is an overlay: the generated configuration below, runtime state above, the
      # upper layer on /persist. Mutable so that passwd/useradd/NetworkManager/systemd can
      # write into it like on any Linux machine.
      system.etc.overlay = {
        enable = true;
        mutable = true;
      };

      # Stage 1: the skeleton directories stage-1 mounts need, created after /sysroot/persist
      # is mounted and before the /.rw-etc bind (and hence before NixOS's rw-etc service and
      # the /etc overlay mount).
      boot.initrd.systemd.services.mura-persist-stage1 = {
        description = "Create /persist skeleton directories needed by stage-1 mounts";
        unitConfig = {
          DefaultDependencies = false;
          RequiresMountsFor = [ "/sysroot${persist}" ];
        };
        before = [ rwEtcMountUnit "initrd-fs.target" ];
        requiredBy = [ rwEtcMountUnit ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        script = ''
          mkdir -p -m 0755 /sysroot${root}
          mkdir -p -m 0755 /sysroot${etcRw}
        '';
      };
      # NixOS's rw-etc service mkdirs the upper/work dirs and the overlay mount uses them;
      # both must see the bind already in place.
      boot.initrd.systemd.services.rw-etc = {
        after = [ rwEtcMountUnit ];
        requires = [ rwEtcMountUnit ];
      };

      # Stage 2: the class skeleton. Idempotent; runs before anything binds into it. It is
      # pulled in by the /var/lib/mura mount, which belongs to local-fs.target — so it must
      # not carry default dependencies (After=sysinit.target would be an ordering cycle;
      # the family's old `nofail` on the bind had been hiding exactly that).
      systemd.services.mura-persist-setup = {
        description = "Create the /persist/mura state skeleton";
        unitConfig = {
          DefaultDependencies = false;
          RequiresMountsFor = persist;
        };
        before = [ "local-fs.target" "shutdown.target" ];
        conflicts = [ "shutdown.target" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        script = ''
          # The tree is traversable (0755): users reach their own enrollment/<user>/ and the
          # shared credential-hint directory; each class protects itself with its own mode.
          install -d -m 0755 ${root}
          install -d -m 0750 ${root}/factory
          install -d -m 0700 ${root}/identity
          install -d -m 0755 ${root}/identity/ssh
          install -d -m 0755 ${root}/enrollment
          install -d -m 0700 ${root}/pairing
          install -d -m 0755 ${root}/state
          install -d -m 0750 ${root}/state/provisioning
          # faillock counters (modules/os/policy.nix): the directory is traversable so an
          # unprivileged caller (mura-authd, the lock) can read and update the user's own 0660
          # tally — Linux-PAM's design for screen lockers; only root creates tallies (D5)
          install -d -m 0755 ${root}/state/faillock
          # credential hint (multi-user.md §3): the /tmp shape — every user writes their own
          # <user> file (0644); the greeter/lock trust a file only if its owner is that user.
          install -d -m 1777 ${root}/state/credential-hint
          # setup-complete marker (first-run §3, §5): the same /tmp shape — the session user
          # (welcome surface) or mura-setup (web app) creates the file; only owner/root remove it.
          install -d -m 1777 ${root}/state/setup
          # health (modules/os/health.nix): the crash-loop counter, RAUC's status file (family)
          install -d -m 0755 ${root}/state/health
        '';
      };

      # The account database is /etc/{passwd,shadow,group} in the persisted overlay
      # (multi-user.md §1.1 rev 3.3): userborn in hybrid mode (users.mutableUsers = true, set
      # by the profiles) preserves administrator-created rows and runtime password changes;
      # declared accounts are re-materialised from the image every boot. On every profile —
      # a password the wearer sets with `passwd` must survive reboots and slot switches.
      services.userborn.enable = true;

      # F1 (first-run-onboarding.md §3) — reference implementations of the two shapes:
      #
      # (a) A per-unit identity that must survive a slot switch: the SSH host keys, generated
      #     idempotently by sshd's own keygen unit into identity/ssh/. No marker needed — the
      #     key's existence is the state.
      services.openssh.hostKeys = lib.mkIf config.services.openssh.enable (lib.mkForce [
        {
          path = "/var/lib/mura/identity/ssh/ssh_host_ed25519_key";
          type = "ed25519";
        }
      ]);
      systemd.services.sshd-keygen = lib.mkIf config.services.openssh.enable {
        unitConfig.RequiresMountsFor = [ "/var/lib/mura" ];
        after = [ "mura-persist-setup.service" ];
      };

      # (b) The marker-gated one-shot pattern every later F1 task copies. Gated on its OWN
      #     durable marker under state/provisioning/ (never ConditionFirstBoot — a fresh A/B
      #     root slot looks like first boot to that); body idempotent; marker committed by
      #     rename(2) after the work. This unit does no real work yet — it is the pattern.
      systemd.services.mura-f1-seed-state = {
        description = "F1: seed per-unit state (marker-gated pattern)";
        wantedBy = [ "multi-user.target" ];
        after = [ "mura-persist-setup.service" ];
        unitConfig = {
          RequiresMountsFor = [ "/var/lib/mura" ];
          ConditionPathExists = "!/var/lib/mura/state/provisioning/seed-state";
        };
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        script = ''
          marker=/var/lib/mura/state/provisioning/seed-state
          # ... idempotent work goes here ...
          tmp="$(mktemp "$(dirname "$marker")/.seed-state.XXXXXX")"
          date -u +%Y-%m-%dT%H:%M:%SZ > "$tmp"
          sync -f "$tmp"
          mv -f "$tmp" "$marker"
        '';
      };
    }
  ];
}
