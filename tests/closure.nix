# tests/closure.nix — the interpreter proof and the Python fence (AGENTS.md rule 6; D4 rev 3).
#
# Two assertions over built closures (pkgs.closureInfo; no daemon access needed at check time):
#
# 1. THE PROOF — the closure of every Mura program (mura-session, mura-preflight, mura-setup,
#    mura-authd, mura-recovery, mura-settingsd) plus what execs them on the login path (greetd, systemd, util-linux's waitpid)
#    contains no interpreter runtime: no python*, no perl, no uwsm. This is the statement the
#    three code rungs (D3/D4/D5/D6 ports) could not each make alone: the boot, login and
#    session-start path is interpreter-free end to end.
#
# 2. THE FENCE — the two x86_64 toplevels (virtual-headset, virtual-headset-multiuser):
#    (a) no uwsm path anywhere; (b) no python*/perl on the system PATH (sw/bin);
#    (c) the python3*-named store paths in the closure equal a pinned allowlist, every entry
#        carrying its reason and the action that removes it. The check fails on additions AND
#        on stale entries, so the list can only shrink. This is a regression fence around
#        nixpkgs-side residue, not a "no Python on the device" claim — the ruling is about Mura's
#        own usage (AGENTS.md rule 6), and mesa/gstreamer/flatpak retain python3 references
#        that are nixpkgs' to fix.
#
# The Steam Frame (aarch64) is not checked here: `nix flake check` runs on x86_64 (ADR 0004);
# `nix flake check --all-systems` on an aarch64 builder is the path for it.
{ pkgs, configurations }:
let
  inherit (pkgs) lib;

  # Names (hash stripped) that count as an interpreter runtime on the login path.
  interpreterRe = "^(python[23]?(\\.[0-9]+)?-|python3\\.[0-9]+-|perl-[0-9]|perl-.*-env|uwsm-)";

  loginPathRoots = [
    pkgs.mura.session
    pkgs.mura.preflight
    pkgs.mura.setup
    pkgs.mura.authd
    pkgs.mura.greeter
    pkgs.mura.osk
    pkgs.mura.recovery
    pkgs.mura.settingsd
    pkgs.greetd
    pkgs.systemd
    pkgs.util-linux
  ];
  loginPathInfo = pkgs.closureInfo { rootPaths = loginPathRoots; };

  # Residual python3* store paths in the toplevel closures, by name with the version stripped
  # (python3.14-pyxdg-0.28 -> python3.X-pyxdg; python3-3.14.7 -> python3). Each entry: why it is
  # there and what removes it.
  pythonAllowlist = {
    "python3" = "the interpreter itself, pulled by the entries below and by mesa/gstreamer/flatpak retained references; leaves when they do";
    "python3.X-pyxdg" = "speech-dispatcher depends on pyxdg; nixpkgs' graphical-desktop.nix enables services.speechd whenever a display manager is on (greetd's module sets services.displayManager.enable) — it did not leave with the sway/gtkgreet stand-ins (G2/G3, 2026-09-28); turning an accessibility service off is an owner item, not a default to flip here";
  };
  allowlistFile = pkgs.writeText "python-allowlist" (lib.concatStringsSep "\n" (lib.attrNames pythonAllowlist) + "\n");
  allowlistReasons = lib.concatStringsSep "\n" (lib.mapAttrsToList (n: r: "  ${n}: ${r}") pythonAllowlist);

  toplevels = {
    virtual-headset = configurations.virtual-headset.config.system.build.toplevel;
    virtual-headset-multiuser = configurations.virtual-headset-multiuser.config.system.build.toplevel;
  };
  toplevelInfos = lib.mapAttrs (_: t: pkgs.closureInfo { rootPaths = [ t ]; }) toplevels;

  fenceScript = name: toplevel: info: ''
    echo "== fence: ${name}"
    names=$(sed 's|^/nix/store/[a-z0-9]*-||' ${info}/store-paths | sort -u)
    # (a) no uwsm anywhere in the closure
    if echo "$names" | grep -E '^uwsm-' ; then echo "FAIL ${name}: uwsm in the closure"; exit 1; fi
    # (b) no interpreter on the system PATH (NixOS's environment.defaultPackages — perl rsync
    #     strace — is emptied in modules/os/default.nix; research/56 §10, ruled)
    if ls ${toplevel}/sw/bin | grep -E '^(python|perl)' ; then echo "FAIL ${name}: interpreter on PATH"; exit 1; fi
    # (d) the stand-ins are gone (implementation-path §3 G2/G3 exit criteria): gtkgreet and cage
    #     left with the greeter swap, squeekboard with mura-osk
    if echo "$names" | grep -E '^(gtkgreet|cage|squeekboard|sway)-[0-9]' ; then echo "FAIL ${name}: a stand-in is still in the closure"; exit 1; fi
    # (c) python3*-named paths == allowlist (normalised: minor version -> X, trailing version dropped)
    echo "$names" \
      | grep -E '^python3(\.[0-9]+)?(-|$)' \
      | sed -E 's/^python3\.[0-9]+/python3.X/; s/-[0-9][^-]*$//' \
      | sort -u > found
    sort -u ${allowlistFile} > allowed
    if ! diff -u allowed found; then
      echo "FAIL ${name}: python3 paths differ from tests/closure.nix's allowlist (+ new, - stale)."
      echo "Allowlisted, with reasons:"
      echo "${allowlistReasons}"
      exit 1
    fi
    echo "ok ${name}: $(wc -l < found) allowlisted python3 path(s), no uwsm, clean PATH"
  '';
in
pkgs.runCommand "mura-closure-check" { } ''
  echo "== proof: login-path closure has no interpreter"
  echo "roots: ${lib.concatMapStringsSep " " (p: p.name or (baseNameOf p)) loginPathRoots}"
  offenders=$(sed 's|^/nix/store/[a-z0-9]*-||' ${loginPathInfo}/store-paths | grep -E '${interpreterRe}' || true)
  if [ -n "$offenders" ]; then
    echo "FAIL: interpreter(s) in the login-path closure:"; echo "$offenders"; exit 1
  fi
  echo "ok: $(wc -l < ${loginPathInfo}/store-paths) paths, none an interpreter"

  ${lib.concatStringsSep "\n" (lib.mapAttrsToList (n: t: fenceScript n t toplevelInfos.${n}) toplevels)}
  touch $out
''
