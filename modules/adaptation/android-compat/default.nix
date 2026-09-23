# Android compatibility building blocks (ADR 0003) — optional, per-subsystem,
# late-starting. Designed in but proven last, on one device, per subsystem.
#
# This is a placeholder for the libhybris / android-headers / late-LXC machinery
# described in docs/research/03-android-compat.md §9. It is intentionally empty of
# config in the scaffold: no device selects an android-backed subsystem yet, and the
# hard prerequisites (per-subsystem blob closure from the donor, android-headers
# derivation, DSP-tracking spike) need hardware work before implementation.
{ lib, ... }:
{
  # Intentionally no options/config yet. When the first android-backed subsystem is
  # implemented, add:
  #   - mura.adaptation.androidCompat.androidGeneration
  #   - the android-headers-<gen>-<device> derivation wiring
  #   - a late, optional systemd LXC unit (never a local-fs.target prerequisite)
  #   - per-subsystem donor blob-closure selection
}
