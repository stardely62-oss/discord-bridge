use sha2::{Digest, Sha256};

fn main() {
    // tauri-build compiles icons/icon.ico into the exe resources (also the
    // runtime window and taskbar icon on Windows) but does not ask Cargo to
    // rerun when the icon changes. With a cached target directory an
    // icon-only change would otherwise ship the previous icon.
    println!("cargo:rerun-if-changed=icons");

    // Telegram WS Proxy engine (built by telegram-proxy/build.ps1 before the
    // app): its SHA-256 is compiled into Discord Bridge, which starts the
    // bundled executable only if the file still has exactly this hash.
    // Without the engine (e.g. a local build that skipped it) the module is
    // reported as not installed.
    let engine = "../resources/tg-ws-proxy/discord-bridge-tg-ws-proxy.exe";
    println!("cargo:rerun-if-changed={engine}");
    if let Ok(bytes) = std::fs::read(engine) {
        println!(
            "cargo:rustc-env=DB_TGPROXY_SHA256={}",
            hex::encode(Sha256::digest(&bytes))
        );
    }
    tauri_build::build()
}
