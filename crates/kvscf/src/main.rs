//! `kvscf` — full build (includes the kdeskdash remote channel via kvscf-app's default
//! `remote` feature). See `kvscf-local` for the no-comms build.

// No console window in release builds; keep it in debug for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(e) = kvscf_app::run() {
        eprintln!("kvscf: {e}");
        std::process::exit(1);
    }
}
