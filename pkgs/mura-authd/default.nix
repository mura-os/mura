# pkgs/mura-authd — the lock-path PAM helper (specs/session-auth.md §2) and its conformance
# harness (§6). The first Rust in the tree: zxr is Rust (ADR 0006), authd handles secrets, and
# the two will share a repository shape. PAM is reached through a hand-written FFI against the
# Linux-PAM ABI (no binding crate); the only crates are libc, serde and serde_json.
{ lib, rustPlatform, pam }:
rustPlatform.buildRustPackage {
  pname = "mura-authd";
  version = "0.1.0";
  src = lib.cleanSource ./.;
  cargoLock.lockFile = ./Cargo.lock;
  buildInputs = [ pam ];
  # the harness is test-only; it ships in the same output for the VM tests and nothing else
  meta = {
    description = "Mura lock-path PAM helper (per-conversation, SOCK_SEQPACKET JSON) + conformance harness";
    license = lib.licenses.gpl3Plus;
    mainProgram = "mura-authd";
    platforms = lib.platforms.linux;
  };
}
