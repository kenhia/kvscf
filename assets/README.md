# kvscf icon assets

- **`kvscf.svg`** — the master art (focus-reticle concept "D2": green corner brackets on a
  dark-mode window with Windows-style controls / red close). Edit this to change the icon.
- **`kvscf.ico`** — multi-resolution icon (16/24/32/48/64/128/256) embedded into the `.exe` as a
  Win32 resource by each bin's `build.rs` (via `winresource`), so pinned taskbar shortcuts / Explorer
  show it. **Must be embedded** (not a sidecar) because the release exe is copied to `C:\tools\bin`.
- **`kvscf-256.png`** — 256×256 render used for the runtime window/taskbar icon
  (`ViewportBuilder::with_icon`, `include_bytes!` in `kvscf-app`).

## Regenerating `kvscf.ico` + `kvscf-256.png` from the SVG

No system SVG rasterizer is assumed. Regenerate with a tiny standalone Rust tool (kept out of the
workspace so it doesn't bloat the app's lockfile). Create it anywhere, point the paths at this dir:

`Cargo.toml`
```toml
[package]
name = "icongen"
version = "0.0.0"
edition = "2021"
[dependencies]
resvg = "0.44"
ico = "0.3"
png = "0.17"
```

`src/main.rs`
```rust
use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
use resvg::{tiny_skia, usvg};
use std::{fs::File, io::BufWriter};

const SVG: &str = r"D:\ClaudeWorks\kvscf\assets\kvscf.svg";
const OUT_ICO: &str = r"D:\ClaudeWorks\kvscf\assets\kvscf.ico";
const OUT_PNG: &str = r"D:\ClaudeWorks\kvscf\assets\kvscf-256.png";

fn straight_rgba(p: &tiny_skia::Pixmap) -> Vec<u8> {
    let mut out = Vec::with_capacity((p.width() * p.height() * 4) as usize);
    for px in p.pixels() { let c = px.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]); }
    out
}
fn render(t: &usvg::Tree, px: u32) -> Vec<u8> {
    let mut pm = tiny_skia::Pixmap::new(px, px).unwrap();
    let s = px as f32 / t.size().width();
    resvg::render(t, tiny_skia::Transform::from_scale(s, s), &mut pm.as_mut());
    straight_rgba(&pm)
}
fn main() {
    let data = std::fs::read(SVG).unwrap();
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).unwrap();
    let mut dir = IconDir::new(ResourceType::Icon);
    for px in [16u32, 24, 32, 48, 64, 128, 256] {
        dir.add_entry(IconDirEntry::encode(&IconImage::from_rgba_data(px, px, render(&tree, px))).unwrap());
    }
    dir.write(File::create(OUT_ICO).unwrap()).unwrap();
    let rgba = render(&tree, 256);
    let mut e = png::Encoder::new(BufWriter::new(File::create(OUT_PNG).unwrap()), 256, 256);
    e.set_color(png::ColorType::Rgba); e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&rgba).unwrap();
}
```

Then rebuild kvscf and the embedded icon updates.
