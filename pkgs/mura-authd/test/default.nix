# TEST-ONLY PAM module used by the mura-authd conformance harness (session-auth §6 items 2, 7).
{ stdenv, pam }:
stdenv.mkDerivation {
  pname = "pam_mura_test";
  version = "0.1.0";
  src = ./.;
  buildInputs = [ pam ];
  buildPhase = ''
    $CC -shared -fPIC -o pam_mura_test.so pam_mura_test.c -lpam
  '';
  installPhase = ''
    install -D -m 0644 pam_mura_test.so $out/lib/security/pam_mura_test.so
  '';
}
