fn main() {
    // tauri-build compiles icons/icon.ico into the exe resources (also the
    // runtime window and taskbar icon on Windows) but does not ask Cargo to
    // rerun when the icon changes. With a cached target directory an
    // icon-only change would otherwise ship the previous icon.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
