fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    let mut attributes = tauri_build::Attributes::new();
    if target_os == "windows" && target_env == "msvc" {
        // tauri-build only embeds the app manifest into the main binary. Test
        // binaries need it too (common-controls v6), otherwise they fail to
        // start with STATUS_ENTRYPOINT_NOT_FOUND. Embed it for every target.
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg=/WX");
    }
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
