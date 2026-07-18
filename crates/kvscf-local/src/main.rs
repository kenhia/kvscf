//! `kvscf-local` — no-comms build for `kwork`. Depends on kvscf-app with
//! `default-features = false`, so the `remote` feature (the kdeskdash channel) is not
//! compiled in at all — the communication code is simply absent from this binary.

// No console window in release builds; keep it in debug for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(e) = kvscf_app::run() {
        eprintln!("kvscf-local: {e}");
        std::process::exit(1);
    }
}
