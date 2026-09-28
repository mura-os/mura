fn main() {
    // The scene carries no images: nothing to embed, no decoder in the closure, and the
    // compiler's accessibility pass stays on (it is switched off for `EmbedForSoftwareRenderer`,
    // `i-slint-compiler/lib.rs:342-346` — research/78 §7a).
    let config = slint_build::CompilerConfiguration::new().embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/greeter.slint", config).expect("ui/greeter.slint compiles");
}
