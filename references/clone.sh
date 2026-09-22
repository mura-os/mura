#!/usr/bin/env bash
# Clone the spatial-os reference set (shallow) and pin it in MANIFEST.json.
# Re-running is idempotent: existing checkouts are kept, missing ones cloned.
set -u

cd "$(dirname "$0")"

# Never pull LFS payloads (research corpus is code-only; avoids multi-GB checkpoint
# downloads and the smudge hang seen with LiteAnyStereo).
export GIT_LFS_SKIP_SMUDGE=1

# name|url|branch (empty branch = default)
repos=(
  'mobile-nixos|https://github.com/mobile-nixos/mobile-nixos.git|'
  'robotnix|https://github.com/nix-community/robotnix.git|'
  'nixos-generators|https://github.com/nix-community/nixos-generators.git|'
  'jovian-nixos|https://github.com/Jovian-Experiments/Jovian-NixOS.git|'
  'pmbootstrap|https://gitlab.postmarketos.org/postmarketOS/pmbootstrap.git|'
  'pmaports|https://gitlab.postmarketos.org/postmarketOS/pmaports.git|'
  'halium-generic-adaptation-build-tools|https://gitlab.com/ubports/porting/community-ports/halium-generic-adaptation-build-tools.git|'
  'halium-docs|https://github.com/Halium/docs.git|'
  'halium-boot|https://github.com/Halium/halium-boot.git|'
  'droidian|https://github.com/droidian/droidian.git|'
  'droid-hal-device|https://github.com/mer-hybris/droid-hal-device.git|'
  'libhybris|https://github.com/libhybris/libhybris.git|'
  'waydroid|https://github.com/waydroid/waydroid.git|'
  'monado|https://gitlab.freedesktop.org/monado/monado.git|'
  'stardustxr-server|https://github.com/StardustXR/server.git|'
  'nixpkgs-xr|https://github.com/nix-community/nixpkgs-xr.git|'
  'wivrn|https://github.com/WiVRn/WiVRn.git|'
  'envision|https://gitlab.com/gabmus/envision.git|'
  'nixos-apple-silicon|https://github.com/tpwrules/nixos-apple-silicon.git|'
  'tow-boot|https://github.com/Tow-Boot/Tow-Boot.git|'
  'meta-qcom|https://github.com/qualcomm-linux/meta-qcom.git|'
  'mkosi|https://github.com/systemd/mkosi.git|'
  'freexr|https://github.com/FreeXR/FreeXR.git|init'
  # --- wxrc compositor lineage (spatial-os compositor research) ---
  'motorcar|https://github.com/evil0sheep/motorcar.git|stable'
  'motorcar-thesis|https://github.com/evil0sheep/MastersThesis.git|'
  'wxrc|https://git.sr.ht/~sircmpwn/wxrc|'
  'wxrc-mirror|https://github.com/patchedsoul/wxrc.git|'
  'wxrd|https://gitlab.freedesktop.org/xrdesktop/wxrd.git|'
  'xrdesktop|https://gitlab.freedesktop.org/xrdesktop/xrdesktop.git|'
  'gxr|https://gitlab.freedesktop.org/xrdesktop/gxr.git|'
  'zwin|https://github.com/zwin-project/zwin.git|'
  'zen|https://github.com/zwin-project/zen.git|'
  'wayvr|https://github.com/wayvr-org/wayvr.git|'
  # --- session / greeter / lock stack (XR login research) ---
  'greetd|https://git.sr.ht/~kennylevinsen/greetd|'
  'gtkgreet|https://git.sr.ht/~kennylevinsen/gtkgreet|'
  'seatd|https://git.sr.ht/~kennylevinsen/seatd|'
  'tuigreet|https://github.com/apognu/tuigreet.git|'
  'regreet|https://github.com/rharish101/ReGreet.git|'
  'cage|https://github.com/cage-kiosk/cage.git|'
  'sddm|https://github.com/sddm/sddm.git|'
  'gdm|https://gitlab.gnome.org/GNOME/gdm.git|'
  'lightdm|https://github.com/canonical/lightdm.git|'
  'swaylock|https://github.com/swaywm/swaylock.git|'
  'hyprlock|https://github.com/hyprwm/hyprlock.git|'
  'kscreenlocker|https://invent.kde.org/plasma/kscreenlocker.git|'
  'wayland-protocols|https://gitlab.freedesktop.org/wayland/wayland-protocols.git|'
  # --- eye tracking / auto-IPD (read-only study; never build) ---
  'pupil|https://github.com/pupil-labs/pupil.git|'
  'pye3d|https://github.com/pupil-labs/pye3d-detector.git|'
  'eyetrackvr|https://github.com/EyeTrackVR/EyeTrackVR.git|'
  'ritnet|https://github.com/AayushKrChaudhary/RITnet.git|'
  'ellseg|https://github.com/RSKothari/EllSeg.git|'
  'deepvog|https://github.com/pydsgz/DeepVOG.git|'
  'eyerectoo|https://github.com/tcsantini/EyeRecToo.git|'
  'alvr|https://github.com/alvr-org/ALVR.git|'
  # --- perception: passthrough / depth / hands (Tier 1-3) ---
  'openxr-steamvr-passthrough|https://github.com/Rectus/openxr-steamvr-passthrough.git|'
  'viewcorrection|https://github.com/puzzlepaint/viewcorrection.git|'
  'neuralpassthrough|https://github.com/facebookresearch/NeuralPassthrough.git|'
  'tc-stereo|https://github.com/jiaxiZeng/Temporally-Consistent-Stereo-Matching.git|'
  'xr-stereo|https://github.com/za-cheng/XR-Stereo.git|'
  'openstereo|https://github.com/XiandaGuo/OpenStereo.git|'
  'raft-stereo|https://github.com/princeton-vl/RAFT-Stereo.git|'
  'ego2hands|https://github.com/AlextheEngineer/Ego2Hands.git|'
  'egohos|https://github.com/owenzlz/EgoHOS.git|'
  'robust-video-matting|https://github.com/PeterL1n/RobustVideoMatting.git|'
  'lightweight-hand-segmentation|https://github.com/itap-robotica-medica/lightweight-hand-segmentation.git|'
  # Tier 2 depth extras confirmed by docs/research/14 (both MIT). Non-commercial
  # ones (Fast-FoundationStereo, OpenStereo_DoItOnce) tracked-not-cloned per §Part 3.
  'liteanystereo|https://github.com/TomTomTommi/LiteAnyStereo.git|'
  'banet|https://github.com/gangweix/BANet.git|'
  # --- spatial sharing: capture/consent/input stack (docs/research/17) ---
  'xdg-desktop-portal|https://github.com/flatpak/xdg-desktop-portal.git|'
  'xdg-desktop-portal-wlr|https://github.com/emersion/xdg-desktop-portal-wlr.git|'
  'gnome-remote-desktop|https://gitlab.gnome.org/GNOME/gnome-remote-desktop.git|'
  'wayvnc|https://github.com/any1/wayvnc.git|'
  'neatvnc|https://github.com/any1/neatvnc.git|'
  'obs-vkcapture|https://github.com/nowrep/obs-vkcapture.git|'
  'libei|https://gitlab.freedesktop.org/libinput/libei.git|'
  'pipewire|https://gitlab.freedesktop.org/pipewire/pipewire.git|'
  # --- spatial sharing: streaming engines (docs/research/18) ---
  'alvr|https://github.com/alvr-org/ALVR.git|'
  'sunshine|https://github.com/LizardByte/Sunshine.git|'
  'wolf|https://github.com/games-on-whales/wolf.git|'
  # --- spatial sharing: wayland proxying / virtio (docs/research/19) ---
  'waypipe|https://gitlab.freedesktop.org/mstoeckl/waypipe.git|'
  'wprs|https://github.com/wayland-transpositor/wprs.git|'
  'wayland-proxy-virtwl|https://github.com/talex5/wayland-proxy-virtwl.git|'
  'crosvm|https://github.com/google/crosvm.git|'
  'spectrum|https://spectrum-os.org/git/spectrum|'
  # sommelier lives in chromiumos platform2 (vm_tools/sommelier); large repo, shallow
  'platform2|https://chromium.googlesource.com/chromiumos/platform2|'
  # --- spatial sharing: workspace replication datapoint ---
  'overte|https://github.com/overte-org/overte.git|'
  # --- spatial mapping: SLAM / anchors / dense geometry (Tier 4, docs/research/20-23) ---
  'basalt-monado|https://gitlab.freedesktop.org/mateosss/basalt.git|'
  'vit|https://gitlab.freedesktop.org/monado/utilities/vit.git|'
  'orbslam3|https://github.com/UZ-SLAMLab/ORB_SLAM3.git|'
  'orbslam3-monado|https://gitlab.freedesktop.org/mateosss/ORB_SLAM3.git|'
  'open-vins|https://github.com/rpng/open_vins.git|'
  'ov-plane|https://github.com/rpng/ov_plane.git|'
  'kimera-vio|https://github.com/MIT-SPARK/Kimera-VIO.git|'
  'kimera-rpgo|https://github.com/MIT-SPARK/Kimera-RPGO.git|'
  'kimera-semantics|https://github.com/MIT-SPARK/Kimera-Semantics.git|'
  'rtabmap|https://github.com/introlab/rtabmap.git|'
  'voxblox|https://github.com/ethz-asl/voxblox.git|'
  'vdbfusion|https://github.com/PRBonn/vdbfusion.git|'
  'supereight2|https://github.com/smartroboticslab/supereight2.git|'
  'nvblox|https://github.com/nvidia-isaac/nvblox.git|'
  'hloc|https://github.com/cvg/Hierarchical-Localization.git|'
  'lightglue|https://github.com/cvg/LightGlue.git|'
  'mast3r-slam|https://github.com/rmurai0610/MASt3R-SLAM.git|'
  'kalibr|https://github.com/ethz-asl/kalibr.git|'
  'openxr-docs|https://github.com/KhronosGroup/OpenXR-Docs.git|'
  'illixr|https://github.com/ILLIXR/ILLIXR.git|'
  # --- avatar / persona: representation, driving, runtime (docs/research/24-27) ---
  # Read-only study corpus. No checkpoints/datasets; LFS smudge globally skipped below.
  # Tracked-not-cloned (no public code or weights-only): GAF, URAvatar, FiCA, SqueezeMe,
  # Apple HeadsUp, HRM2Avatar, LAM main (only Audio2Expression needed).
  'rgbavatar|https://github.com/gapszju/RGBAvatar.git|'
  'gaussianavatars|https://github.com/ShenhanQian/GaussianAvatars.git|'
  'match|https://github.com/malteprinzler/match.git|'
  'flexavatar|https://github.com/tobias-kirschstein/flexavatar.git|'
  'metrical-tracker|https://github.com/Zielon/metrical-tracker.git|'
  'ava-256|https://github.com/facebookresearch/ava-256.git|'
  'goliath|https://github.com/facebookresearch/goliath.git|'
  'baballonia|https://github.com/Project-Babble/Baballonia.git|'
  'eyetrackvr|https://github.com/EyeTrackVR/EyeTrackVR.git|'
  'vrcfacetracking|https://github.com/benaclejames/VRCFaceTracking.git|'
  'ofera|https://github.com/ysshwan147/OFERA.git|'
  'lam-audio2expression|https://github.com/aigc3d/LAM_Audio2Expression.git|'
  '3dgs-cpp|https://github.com/shg8/3DGS.cpp.git|'
  'vkgs|https://github.com/jaesung-cs/vkgs.git|'
  # --- desktop environment: XDG specs + DE implementation comparison (doc 30 addendum) ---
  # xdg-specs = the freedesktop Cross-Desktop-Group specification sources (desktop-entry,
  # basedir, autostart, icon-theme, menu, trash, notifications, status-notifier/SNI) —
  # distinct from the xdg_* Wayland protocol namespace and from xdg-desktop-portal.
  'xdg-specs|https://gitlab.freedesktop.org/xdg/xdg-specs.git|'
  'kwin|https://invent.kde.org/plasma/kwin.git|'
  'plasma-workspace|https://invent.kde.org/plasma/plasma-workspace.git|'
  'mutter|https://gitlab.gnome.org/GNOME/mutter.git|'
  'gnome-shell|https://gitlab.gnome.org/GNOME/gnome-shell.git|'
  'cosmic-comp|https://github.com/pop-os/cosmic-comp.git|'
  'cosmic-panel|https://github.com/pop-os/cosmic-panel.git|'
  'cosmic-protocols|https://github.com/pop-os/cosmic-protocols.git|'
  'xdg-desktop-portal-cosmic|https://github.com/pop-os/xdg-desktop-portal-cosmic.git|'
)

mkdir -p .logs
pids=()
names=()

for entry in "${repos[@]}"; do
  IFS='|' read -r name url branch <<<"$entry"
  if [ -d "$name/.git" ]; then
    echo "skip  $name (exists)"
    continue
  fi
  args=(clone --depth 1)
  [ -n "$branch" ] && args+=(--branch "$branch")
  git "${args[@]}" "$url" "$name" >".logs/$name.log" 2>&1 &
  pids+=($!)
  names+=("$name")
done

failed=()
for i in "${!pids[@]}"; do
  if wait "${pids[$i]}"; then
    echo "ok    ${names[$i]}"
  else
    echo "FAIL  ${names[$i]} (see .logs/${names[$i]}.log)"
    failed+=("${names[$i]}")
  fi
done

# Ava-256 study branches: the unmerged headset-encoder / expression-code PRs
# (docs/research/26). Fetched shallow into local branches; failures are non-fatal.
if [ -d "ava-256/.git" ]; then
  for pr in 1 7 19; do
    git -C ava-256 rev-parse --verify "study/pr-$pr" >/dev/null 2>&1 && continue
    git -C ava-256 fetch --depth 1 origin "pull/$pr/head:study/pr-$pr" \
      >>".logs/ava-256.log" 2>&1 &&
      echo "ok    ava-256 study/pr-$pr" ||
      echo "warn  ava-256 PR $pr fetch failed (see .logs/ava-256.log)"
  done
fi

# Pin the manifest from what actually exists on disk.
{
  echo '{'
  first=1
  for entry in "${repos[@]}"; do
    IFS='|' read -r name url branch <<<"$entry"
    [ -d "$name/.git" ] || continue
    commit=$(git -C "$name" rev-parse HEAD)
    ref=$(git -C "$name" rev-parse --abbrev-ref HEAD)
    [ $first -eq 1 ] || echo ','
    first=0
    printf '  "%s": {"url": "%s", "commit": "%s", "ref": "%s", "cloned": "%s"}' \
      "$name" "$url" "$commit" "$ref" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  done
  echo
  echo '}'
} >MANIFEST.json

echo
echo "Manifest written: $(pwd)/MANIFEST.json"
if [ "${#failed[@]}" -gt 0 ]; then
  echo "Failed clones: ${failed[*]}"
  exit 1
fi
