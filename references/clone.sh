#!/usr/bin/env bash
# Clone the Mura reference set (shallow) and pin it in MANIFEST.json.
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
  # --- device unlock / installation state machines (docs/research/71-72) ---
  # Source-level mechanism evidence only. Firmware archives, engineering loaders,
  # unlock tokens and per-unit backups are never cloned into the research corpus.
  'grapheneos-org|https://github.com/GrapheneOS/grapheneos.org.git|'
  'grapheneos-fastboot-js|https://github.com/GrapheneOS/fastboot.js.git|'
  'grapheneos-flasher|https://github.com/264nm/grapheneos-flasher.git|'
  'aosp-system-core|https://android.googlesource.com/platform/system/core|main'
  'avb|https://android.googlesource.com/platform/external/avb|main'
  'ubports-installer|https://github.com/ubports/ubports-installer.git|'
  'ubports-installer-configs|https://github.com/ubports/installer-configs.git|'
  'bootloader-unlock-wall-of-shame|https://github.com/zenfyrdev/bootloader-unlock-wall-of-shame.git|'
  'fuguquest|https://github.com/Henry1887/fuguquest.git|'
  'more-picohaxx-tool|https://github.com/chaixshot/more-picohaxx-tool.git|'
  'pico-documentation|https://github.com/thoricelli/PICO-documentation.git|'
  'pico4-downgrade-guide|https://github.com/Spalishe/Pico4-Downgrade-Guide.git|'
  'queststack|https://github.com/starseed12345/QuestStack.git|'
  'quest1-bootloader-unlocker-web|https://github.com/darknight1050/quest1-bootloader-unlocker-web.git|'
  'quest-bootloader-unlocker|https://github.com/darknight1050/quest-bootloader-unlocker.git|'
  # --- wxrc compositor lineage (Mura compositor research) ---
  'motorcar|https://github.com/evil0sheep/motorcar.git|stable'
  'motorcar-thesis|https://github.com/evil0sheep/MastersThesis.git|'
  'wxrc|https://git.sr.ht/~sircmpwn/wxrc|'
  'wxrc-mirror|https://github.com/patchedsoul/wxrc.git|'
  'wxrd|https://gitlab.freedesktop.org/xrdesktop/wxrd.git|'
  'xrdesktop|https://gitlab.freedesktop.org/xrdesktop/xrdesktop.git|'
  'gxr|https://gitlab.freedesktop.org/xrdesktop/gxr.git|'
  'g3k|https://gitlab.freedesktop.org/xrdesktop/g3k.git|'
  'zwin|https://github.com/zwin-project/zwin.git|'
  'zen|https://github.com/zwin-project/zen.git|'
  'wayvr|https://github.com/wayvr-org/wayvr.git|'
  # --- session / greeter / lock stack (XR login research) ---
  # accountsservice = the freedesktop user-enumeration D-Bus daemon GDM/SDDM
  # pickers consume (docs/research/41, multi-user design).
  'accountsservice|https://gitlab.freedesktop.org/accountsservice/accountsservice.git|'
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
  # krdp = KDE's RDP server: the proven KWin-adjacent buffer-consumption + libei input
  # plumbing precedent cited by ADR 0014 M-B (producer-spec workstream, doc 40).
  'krdp|https://invent.kde.org/plasma/krdp.git|'
  'mutter|https://gitlab.gnome.org/GNOME/mutter.git|'
  'gnome-shell|https://gitlab.gnome.org/GNOME/gnome-shell.git|'
  'cosmic-comp|https://github.com/pop-os/cosmic-comp.git|'
  'cosmic-panel|https://github.com/pop-os/cosmic-panel.git|'
  'cosmic-protocols|https://github.com/pop-os/cosmic-protocols.git|'
  'xdg-desktop-portal-cosmic|https://github.com/pop-os/xdg-desktop-portal-cosmic.git|'
  # --- KWin VR study (docs/research/31, ADR 0013): the lightofmysoul fork family ---
  # kwin-vr = the VR-plugin fork branch behind KWin MR !8671 (upstream kwin master is
  # already pinned above for diffing); vr-patches = required Qt/XWayland patch series;
  # monado-galaxyxr = Galaxy XR bring-up branch; xrinfo = the author's OpenXR probe tool.
  'kwin-vr|https://invent.kde.org/lightofmysoul/kwin.git|vr'
  'kwin-vr-patches|https://invent.kde.org/lightofmysoul/vr-patches.git|'
  'monado-galaxyxr|https://gitlab.freedesktop.org/lightofmysoul/monado.git|galaxyxr'
  'xrinfo|https://gitlab.freedesktop.org/lightofmysoul/xrinfo.git|'
  # --- VR shell interaction-pattern study (docs/research/36) ---
  'simula|https://github.com/SimulaVR/Simula.git|'
  'breezy-desktop|https://github.com/wheaney/breezy-desktop.git|'
  'xr-linux-driver|https://github.com/wheaney/XRLinuxDriver.git|'
  # --- compositor base (ADR 0006 ratification study, docs/research/39) ---
  # smithay = the ratified base (smallvil/anvil examples in-tree); wlroots = the
  # recorded fallback + Vulkan-renderer study (current freedesktop upstream — the
  # archived swaywm/wlroots GitHub mirror is a known trap); niri = the pattern
  # reference for smithay state/calloop/damage structure; openxrs = the Rust
  # OpenXR bindings (the ash/OpenXR boundary zxr owns); waynest = StardustXR's
  # Wayland wire layer (datapoint, not a candidate); weston/louvre/mir cloned
  # only for evidence-cited disposition.
  'smithay|https://github.com/Smithay/smithay.git|'
  'wlroots|https://gitlab.freedesktop.org/wlroots/wlroots.git|'
  'niri|https://github.com/YaLTeR/niri.git|'
  'openxrs|https://github.com/Ralith/openxrs.git|'
  'waynest|https://github.com/verdiwm/waynest.git|'
  'weston|https://gitlab.freedesktop.org/wayland/weston.git|'
  'louvre|https://github.com/CuarzoSoftware/Louvre.git|'
  'mir|https://github.com/canonical/mir.git|'
  # --- input bootstrap + out-of-band provisioning study (docs/research/42) ---
  # What a headset can accept as input before anything is configured, and how a
  # person reaches it from a device they already hold. bluez = pre-login pairing
  # agents + PAN; libinput = HID hotplug into the greeter seat; systemd = logind seat
  # ACLs + sshd/gadget ordering; NetworkManager = AP/shared mode (hotspot), keyfile
  # secrets, inactive-session D-Bus policy; gnome-initial-setup = the desktop wizard
  # we deliberately do NOT have (mechanism evidence only); squeekboard/wvkbd = the two
  # Wayland on-screen-keyboard lineages (input-method-v2 vs virtual-keyboard-v1);
  # cockpit/wifi-connect/comitup/raspap/luci = the web-provisioning tool candidates
  # scored in doc 42 §6; unudhcpd = postmarketOS's first-boot USB-network DHCP server.
  'bluez|https://github.com/bluez/bluez.git|'
  'libinput|https://gitlab.freedesktop.org/libinput/libinput.git|'
  'systemd|https://github.com/systemd/systemd.git|'
  'networkmanager|https://gitlab.freedesktop.org/NetworkManager/NetworkManager.git|'
  'gnome-initial-setup|https://gitlab.gnome.org/GNOME/gnome-initial-setup.git|'
  'squeekboard|https://gitlab.gnome.org/World/Phosh/squeekboard.git|'
  'wvkbd|https://github.com/jjsullivan5196/wvkbd.git|'
  'cockpit|https://github.com/cockpit-project/cockpit.git|'
  'wifi-connect|https://github.com/balena-os/wifi-connect.git|'
  'comitup|https://github.com/davesteele/comitup.git|'
  # --- first-run authority study (docs/research/48) ---
  # How shipping Linux first-run flows let the FIRST user set system state (time zone,
  # hostname, network, accounts) before any password exists. jupiter-hw-support = SteamOS's
  # holo-polkit-helpers (pkexec helpers + org.valve.holo.policy for the passwordless deck
  # user); plasma-welcome / elementary initial-setup / phosh-tour / lomiri-system-settings =
  # the desktop and mobile welcome/wizard lineages; calamares = installer-side first-boot
  # hooks; gnome-initial-setup (above) = the two-mode reference. rauc = the update client
  # whose slot-state/mark-good behaviour D6 wires (was cited unpinned).
  'jupiter-hw-support|https://github.com/Jovian-Experiments/jupiter-hw-support.git|'
  'plasma-welcome|https://invent.kde.org/plasma/plasma-welcome.git|'
  'elementary-initial-setup|https://github.com/elementary/initial-setup.git|'
  'phosh-tour|https://gitlab.gnome.org/World/Phosh/phosh-tour.git|'
  'phosh-mobile-settings|https://gitlab.gnome.org/World/Phosh/phosh-mobile-settings.git|'
  'lomiri-system-settings|https://gitlab.com/ubports/development/core/lomiri-system-settings.git|'
  'calamares|https://github.com/calamares/calamares.git|'
  'rauc|https://github.com/rauc/rauc.git|'
  'raspap|https://github.com/RaspAP/raspap-webgui.git|'
  'luci|https://github.com/openwrt/luci.git|'
  'unudhcpd|https://gitlab.postmarketos.org/postmarketOS/unudhcpd.git|'
  # --- settings stores (docs/research/58, specs/settings-schema.md, D7) ---
  # The desktop settings stores themselves, not their clients: dconf (single-writer
  # session service behind GSettings), glib (GSettings schemas/backends/lockdown),
  # kconfig (KConfig + KConfigXT + kconf_update), libcosmic (cosmic-config: the Rust
  # file-per-key store), cosmic-settings-daemon (its notification concentrator),
  # gsettings-desktop-schemas (the canonical desktop keys + org.gnome.desktop.lockdown).
  'dconf|https://gitlab.gnome.org/GNOME/dconf.git|'
  'glib|https://gitlab.gnome.org/GNOME/glib.git|'
  'kconfig|https://invent.kde.org/frameworks/kconfig.git|'
  'libcosmic|https://github.com/pop-os/libcosmic.git|'
  'cosmic-settings-daemon|https://github.com/pop-os/cosmic-settings-daemon.git|'
  'gsettings-desktop-schemas|https://gitlab.gnome.org/GNOME/gsettings-desktop-schemas.git|'
  # Appliance-OS system configuration stores (research/58 §12): snapd's `snap set system`
  # (configcore validators + apply handlers, Change/Task status), OpenWrt UCI (+ /var/state
  # runtime layer) and procd (reload triggers), SteamOS's steamos-manager (Rust system D-Bus
  # daemon with polkit). LuCI (apply/rollback) and platform2 login_manager (owner-signed device
  # settings) are already pinned above.
  'snapd|https://github.com/canonical/snapd.git|'
  'uci|https://github.com/openwrt/uci.git|'
  'procd|https://github.com/openwrt/procd.git|'
  'steamos-manager|https://gitlab.steamos.cloud/holo/steamos-manager.git|v26.4.1'
  # --- zxr architecture study (docs/research/59, /60; specs/zxr-core.md) ---
  # gamescope = SteamOS's compositor: a Wayland compositor that is itself a Vulkan app with its
  # own pacing and an OpenVR presentation backend; wlx-overlay-s = the OpenXR overlay loop WayVR
  # grew in; river = the layout-CLIENT precedent for pluggable window-management policy;
  # mako/dunst = layer-shell notification daemons; phosh = the mobile shell's lock/OSD/keyboard
  # seams; the GNOME/KDE portal backends = the consent and capture seam as the desktops ship it.
  'gamescope|https://github.com/ValveSoftware/gamescope.git|'
  'wlx-overlay-s|https://github.com/galister/wlx-overlay-s.git|'
  'river|https://codeberg.org/river/river.git|'
  'mako|https://github.com/emersion/mako.git|'
  'dunst|https://github.com/dunst-project/dunst.git|'
  'phosh|https://gitlab.gnome.org/World/Phosh/phosh.git|'
  'xdg-desktop-portal-gnome|https://gitlab.gnome.org/GNOME/xdg-desktop-portal-gnome.git|'
  'xdg-desktop-portal-kde|https://invent.kde.org/plasma/xdg-desktop-portal-kde.git|'
  # xwayland-satellite = the out-of-process X11 WM niri and wayvr use (research/59 §9): what it
  # requires of the compositor, what it maps, and where it stops.
  'xwayland-satellite|https://github.com/Supreeeme/xwayland-satellite.git|'
  # --- XR input / focus / selection study (docs/research/63) ---
  # mrtk3 = the open implementation of the HoloLens gaze-pinch / hand-ray / poke model with its
  # rationale in code; stereokit = a small C hands+gaze UI input model (focus/active, hysteresis);
  # godot-xr-tools = the Godot community's converged pointer/poke patterns.
  'mrtk3|https://github.com/MixedRealityToolkit/MixedRealityToolkit-Unity.git|'
  'stereokit|https://github.com/StereoKit/StereoKit.git|'
  'godot-xr-tools|https://github.com/GodotVR/godot-xr-tools.git|'
  # --- window / workspace management study (docs/research/64) ---
  # flatland = StardustXR's 2D-window manager as a *client* of a server that owns only the graph;
  # hyprland = the most-used pluggable WM surface today (dispatchers, plugin API);
  # paperwm = scrollable tiling replacing GNOME's policy as an extension, compositor untouched.
  'flatland|https://github.com/StardustXR/flatland.git|'
  'hyprland|https://github.com/hyprwm/Hyprland.git|'
  'paperwm|https://github.com/paperwm/PaperWM.git|'
  # --- native OpenXR applications beside zxr (docs/research/66) ---
  # xrizer = the Rust OpenVR-on-OpenXR layer Envision launches SteamVR titles through;
  # opencomposite = the older C++ implementation xrizer's README defers to as the mature one.
  'xrizer|https://github.com/Supreeeme/xrizer.git|'
  'opencomposite|https://gitlab.com/znixian/OpenOVR.git|'
  # --- input architecture (docs/research/68) ---
  # aosp-frameworks-native = InputFlinger/InputDispatcher: the one shipping design with the input
  # dispatcher as a service separate from the compositor (SurfaceFlinger); Horizon OS / Android XR
  # inherit it. openvr = IVROverlay's runtime-side mouse-event synthesis (xrizer/opencomposite implement it).
  'aosp-frameworks-native|https://android.googlesource.com/platform/frameworks/native|'
  'openvr|https://github.com/ValveSoftware/openvr.git|'
  # --- the shell plane as shipped (docs/research/75) ---
  # The COSMIC shell components beside the already-pinned cosmic-panel/libcosmic: greeter,
  # launcher, OSD, notifications, applets — one toolkit (libcosmic/iced), one process each,
  # started by cosmic-session. waybar/fuzzel = the wlroots ecosystem's panel and launcher
  # (per-tool GTK/cairo); maliit-keyboard = Plasma Mobile's OSK (Qt); slint = the toolkit
  # candidate for Mura's own shell components (docs/research/75 §toolkit); accesskit = the
  # cross-toolkit accessibility bridge (AT-SPI over zbus) Slint and iced both integrate.
  'cosmic-greeter|https://github.com/pop-os/cosmic-greeter.git|'
  'cosmic-launcher|https://github.com/pop-os/cosmic-launcher.git|'
  'cosmic-osd|https://github.com/pop-os/cosmic-osd.git|'
  'cosmic-notifications|https://github.com/pop-os/cosmic-notifications.git|'
  'cosmic-applets|https://github.com/pop-os/cosmic-applets.git|'
  'cosmic-session|https://github.com/pop-os/cosmic-session.git|'
  'waybar|https://github.com/Alexays/Waybar.git|'
  'fuzzel|https://codeberg.org/dnkl/fuzzel.git|'
  'maliit-keyboard|https://github.com/maliit/keyboard.git|'
  'slint|https://github.com/slint-ui/slint.git|v1.18.0'
  'accesskit|https://github.com/AccessKit/accesskit.git|'
  # client-toolkit (sctk) = the Rust Wayland client toolkit Mura's Slint platform stands on: layer-shell
  # and session-lock roles, seat, shm (docs/research/78 §9; shell-plane.md §4 rev 0.3). Pinned at the
  # release the platform builds against.
  'client-toolkit|https://github.com/Smithay/client-toolkit.git|v0.20.0'
  # --- the shell layer's server-side mechanics (docs/research/76) ---
  # sway = wlroots' reference layer-shell arrangement and keyboard-interactivity focus rules;
  # phoc = phosh's compositor (the pinned mobile shell's other half: squeekboard/phosh-lockscreen
  # against a wlroots server); gtk-layer-shell / layer-shell-qt = the two client libraries whose
  # assumptions (pixel output geometry, configure round-trip, popup parenting) must map on zxr
  # unmodified.
  'sway|https://github.com/swaywm/sway.git|'
  'phoc|https://gitlab.gnome.org/World/Phosh/phoc.git|'
  'gtk-layer-shell|https://github.com/wmww/gtk-layer-shell.git|'
  'layer-shell-qt|https://invent.kde.org/plasma/layer-shell-qt.git|'
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
