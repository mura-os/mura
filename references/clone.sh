#!/usr/bin/env bash
# Clone the spatial-os reference set (shallow) and pin it in MANIFEST.json.
# Re-running is idempotent: existing checkouts are kept, missing ones cloned.
set -u

cd "$(dirname "$0")"

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
} > MANIFEST.json

echo
echo "Manifest written: $(pwd)/MANIFEST.json"
if [ "${#failed[@]}" -gt 0 ]; then
  echo "Failed clones: ${failed[*]}"
  exit 1
fi
