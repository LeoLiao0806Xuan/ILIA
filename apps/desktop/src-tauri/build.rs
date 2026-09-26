fn main() {
    println!("cargo:rustc-check-cfg=cfg(mobile)");
    // Windows resource compilation is only required for distributable binaries. Skipping it for
    // debug/test/check builds keeps CI and local validation independent of windres/linker quirks;
    // release builds still execute the complete Tauri bundling metadata path.
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        tauri_build::build();
    } else {
        println!("cargo:rerun-if-changed=tauri.conf.json");
    }
}
