//! `static_ui` embeds `apps/desktop/dist-web`. If the web UI has not been built yet, create a
//! placeholder so the crate still compiles and the browser shows "web UI not built".

use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/dist-web");
    if !dir.join("index.html").exists() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join("index.html"),
            "<!doctype html><meta charset=utf-8><title>NanoNexus</title>\
             <p style=\"font:16px system-ui;margin:2rem\">The NanoNexus web UI is not built yet. \
             Run <code>pnpm build:web</code> and rebuild the app.</p>",
        );
    }
    println!("cargo:rerun-if-changed=../../apps/desktop/dist-web");
    println!("cargo:rerun-if-changed=build.rs");
}
