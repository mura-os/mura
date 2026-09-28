fn main() {
    // No images: nothing to embed, no decoder in the closure, the accessibility pass stays on
    // (the same reasoning as mura-greeter's build.rs).
    let config = slint_build::CompilerConfiguration::new().embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/osk.slint", config).expect("ui/osk.slint compiles");
}
