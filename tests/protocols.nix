# Protocol XML validation: every file in protocols/ must be well-formed XML and
# must survive wayland-scanner code generation (client + server headers, glue).
# A protocol that does not scan does not merge (specification-program wave 0).
{ nixpkgs, system }:
let
  pkgs = nixpkgs.legacyPackages.${system};
in
pkgs.runCommand "spatial-protocols-check"
{
  nativeBuildInputs = [ pkgs.libxml2 pkgs.wayland-scanner ];
  src = ../protocols;
} ''
  fail=0
  for f in "$src"/*.xml; do
    echo "checking $(basename "$f")"
    xmllint --noout "$f" || fail=1
    wayland-scanner client-header "$f" /dev/null || fail=1
    wayland-scanner server-header "$f" /dev/null || fail=1
    wayland-scanner private-code "$f" /dev/null || fail=1
  done
  [ "$fail" = 0 ] && echo ok > $out
''
