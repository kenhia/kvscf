// Embed the app icon as a Win32 resource so pinned taskbar shortcuts / Explorer show it.
fn main() {
    #[cfg(windows)]
    {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let ico = std::path::Path::new(&manifest)
            .join("..")
            .join("..")
            .join("assets")
            .join("kvscf.ico");
        println!("cargo:rerun-if-changed={}", ico.display());
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico.to_str().unwrap());
        res.compile().expect("embed icon resource");
    }
}
