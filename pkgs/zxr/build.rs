//! Compile the two GLSL shaders to SPIR-V at build time with glslc (shaderc), the path baked by
//! the Nix build through GLSLC; `include_bytes!` picks them up from OUT_DIR. Also names the
//! protocol XML the seam's bindings are generated from (policy/seam.rs, the test manager), so
//! an edit to it rebuilds — wayland-scanner's macros read it but do not register it.
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../../protocols/zxr-window-management-v1.xml");
    let glslc = std::env::var("GLSLC").unwrap_or_else(|_| "glslc".into());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for (src, stage) in [("shaders/plane.vert", "vert"), ("shaders/plane.frag", "frag")] {
        println!("cargo:rerun-if-changed={src}");
        let dst = out.join(format!("plane.{stage}.spv"));
        let st = Command::new(&glslc)
            .args(["-O", "--target-env=vulkan1.2", "-o"])
            .arg(&dst)
            .arg(src)
            .status()
            .unwrap_or_else(|e| panic!("cannot run {glslc}: {e}"));
        assert!(st.success(), "glslc failed on {src}");
    }
}
