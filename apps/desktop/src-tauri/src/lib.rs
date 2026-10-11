//! NanoNexus desktop shell: thin Tauri commands over the `nexus-*` crates.

pub mod runtime_paths;

/// Build and run the Tauri application.
pub fn run() {
    let result = tauri::Builder::default().run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("error while running NanoNexus: {e}");
        std::process::exit(1);
    }
}
