# Donor pipeline builders: acquire -> identify -> parse -> extract -> qualify.
#
# Per docs/architecture/donor-pipeline.md, each stage is a separate derivation for
# granular caching and independent review. This scaffold provides the manifest
# evaluator entry point and stage signatures; the derivations themselves are
# implemented against the first real donor (Lynx R1 is the target first port).
{ lib, pkgs }:
rec {
  # Evaluate a donor manifest attrset into the stage derivation graph. Returns an
  # attrset of stage outputs; flashable consumers only ever see them once `qualify`
  # has a reviewed contract (null-propagation gating, brick appliance pattern).
  evalDonor = manifest:
    assert lib.isAttrs manifest;
    throw ''
      lib/donor.evalDonor is a scaffold stub. Implement the five stages against the
      first real donor (see docs/architecture/donor-pipeline.md §stages):
        acquire (fetchurl | requireFile | on-device path)
        identify (magic-byte detection, assert device+buildId, fail-closed)
        parse   (verbatim partition blobs + parse-report.json)
        extract (allowlisted artifact sets + metadata.json, unprivileged debugfs/fsck.erofs)
        qualify (reviewed, hash-bound contract; ->40-char reviewNotes)
      Enforce licensing.redistributable=false -> allowSubstitutes=false on any
      donor-containing output.
    '';

  # Placeholder for the manifest schema validator (typed check of the section-5.6
  # attrset). Wired into checks once the first manifest exists.
  validateManifest = manifest: manifest;
}
