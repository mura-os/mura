# pkgs/mura-plymouth-theme — the boot / failure / recovery screen, composited per device.
#
# One master illustration (assets/branding/recovery-mode.png: square, 8 bpc RGBA, black
# background, no text) becomes a plymouth `two-step` theme whose watermark is
# a full-output canvas: the illustration scaled to `artHeightFraction` of the eye height, centred
# per eye, duplicated left/right when one DRM output spans both panels. Text (the failure message,
# the recovery menu) is plymouth's — rendered at runtime under the illustration, sized to the
# panel — never baked into the asset. research/57 §5 item 5: the one shipping headset comparable
# (Quest) presents recovery as flat text per eye; pmOS puts the failure and a URL on the plymouth
# splash. The theme is built at image-build time from the device contract; no runtime scaling.
{ lib
, runCommand
, imagemagick
, artwork ? ../../assets/branding/recovery-mode.png
, panelWidth
, panelHeight
, displays ? 2
  # Whether one DRM output carries both eyes side by side (the Frame's donor exposes one
  # 4320x2160 output); a single-panel device (the VM) has displays = 1.
, sideBySide ? displays > 1
, artHeightFraction ? 0.85
}:
let
  eyes = if sideBySide then displays else 1;
  outW = panelWidth * eyes;
  outH = panelHeight;
  artPx = builtins.floor (panelHeight * artHeightFraction);
  # Text size follows the panel: ~1/60 of its height in points (Frame 36, VM 18).
  fontPt = lib.max 12 (builtins.floor (panelHeight / 60));
  titlePt = fontPt * 2;
in
runCommand "mura-plymouth-theme"
{
  nativeBuildInputs = [ imagemagick ];
  passthru = { inherit outW outH eyes artPx fontPt; };
} ''
    set -eu
    theme=$out/share/plymouth/themes/mura
    mkdir -p "$theme"

    # One eye: the illustration scaled to the eye height fraction, centred, black around it.
    magick ${artwork} -resize ${toString artPx}x${toString artPx} \
      -background black -gravity center -extent ${toString panelWidth}x${toString outH} eye.png
    # The output: the eye duplicated side by side when one output spans both panels.
    if [ ${toString eyes} -gt 1 ]; then
      args=""; for _ in $(seq 1 ${toString eyes}); do args="$args eye.png"; done
      # shellcheck disable=SC2086
      magick $args +append "$theme/watermark.png"
    else
      cp eye.png "$theme/watermark.png"
    fi

    cat > "$theme/mura.plymouth" <<EOF
  [Plymouth Theme]
  Name=Mura
  Description=Mura boot, failure-feedback and recovery screen (per-eye canvas from the device contract)
  ModuleName=two-step

  [two-step]
  Font=DejaVu Sans ${toString fontPt}
  TitleFont=DejaVu Sans Light ${toString titlePt}
  ImageDir=$theme
  # The watermark is the whole canvas; everything else is text under it.
  WatermarkHorizontalAlignment=.5
  WatermarkVerticalAlignment=.5
  HorizontalAlignment=.5
  VerticalAlignment=.80
  DialogHorizontalAlignment=.5
  DialogVerticalAlignment=.80
  TitleHorizontalAlignment=.5
  TitleVerticalAlignment=.80
  Transition=none
  TransitionDuration=0.0
  BackgroundStartColor=0x000000
  BackgroundEndColor=0x000000
  ProgressBarBackgroundColor=0x303030
  ProgressBarForegroundColor=0xffffff
  MessageBelowAnimation=true

  [boot-up]
  UseEndAnimation=false

  [shutdown]
  UseEndAnimation=false

  [reboot]
  UseEndAnimation=false

  [system-reset]
  SuppressMessages=false
  UseProgressBar=true
  Title=Resetting this headset
  SubTitle=Do not power off
  EOF
''
