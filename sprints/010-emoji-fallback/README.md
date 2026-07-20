# Sprint 010 — Emoji in window names (WI #489)

Status: **built; awaiting Ken's visual confirm.** Emoji Ken adds to Edge window names showed as `?` /
`??`. Now they render — monochrome (egui can't do color emoji).

## Root cause (from WI #489, proved live)

Not data loss: `GetWindowTextW` + `from_utf16_lossy` reads the emoji intact (dumped
`ClaudeOnTheWeb 🦀` → `… 0020 D83E DD80`, the correct surrogate pair for U+1F980). It's an egui
font-coverage gap, on **named** rows specifically: egui's default proportional family already falls
back to a bundled monochrome emoji subset, but kvscf's custom **bold** family (used by named rows) had
no fallback → missing glyph → `?`. `?` vs `??` is one replacement per missing Unicode *scalar*
(single-scalar 🦀 → `?`; multi-scalar 🛠️ = U+1F6E0 + U+FE0F → `??`).

## What shipped

Started as **A1** (reuse egui's bundled emoji as the bold family's fallback — near-free). A
`--probe-glyphs` headless check (uses `Fonts::has_glyph` per `FontId`) showed A1 was **insufficient**:
egui's subset has 🛠 but **not** 🦀 or 🏗 — two of Ken's three real glyphs. So, per the pre-authorized
escalation, went to **A2**: bundle a monochrome emoji font.

- **Font:** `assets/NotoEmoji-kvscf.ttf` — the OFL **Noto Emoji** variable font, `instancer`'d to a
  static Regular (`wght=400`) and subset to the emoji Unicode ranges. Static, 1811 glyphs, ~0.87 MB.
  License + provenance in `assets/NotoEmoji-OFL.txt` and `assets/NotoEmoji-kvscf.README.md`. No
  Reserved Font Name, so the name is kept.
- **Wiring** (`install_bold_font`): `include_bytes!` the font, register it as `kvscf-emoji`, and put
  it in every family's fallback chain (right after each primary face). The bold family also chains
  egui's bundled emoji/symbols after ours for the union of coverage. Compiled in, so `kvscf-local` on
  kwork needs no extra file.

## Verification

`--probe-glyphs` — every target glyph now covered on the **bold** family (the one named rows use):

```
U+1F980 🦀  bold=true   (crab, cleo)
U+1F6E0 🛠   bold=true   (hammer-wrench, kwork)
U+1F3D7 🏗   bold=true   (building, kwork)
U+0FE0F     bold=true   (VS16 — present, so no stray trailing ?)
```

Full CI workflow run locally (fmt, `clippy --all-targets` for default and `kvscf-local`, `build
--all-targets`, `cargo test`, `--build-info` → `remote=false`): green. Binary size: kvscf 6.08→7.06 MB,
kvscf-local 5.5→6.48 MB (+~0.87 MB, the font).

### Awaiting Ken
- [ ] `ClaudeOnTheWeb 🦀` (cleo) shows the crab silhouette, not `?`.
- [ ] `Pipes🛠️` and `🏗️ BUILD TEST` (kwork) show their glyphs, no `??`.

Reminder: **monochrome** — egui/epaint stores glyphs in grayscale and can't render color emoji, so
these are black-and-white silhouettes, not the color glyphs.
