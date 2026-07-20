# NotoEmoji-kvscf.ttf

A monochrome emoji font bundled so emoji in Edge/VS Code window names render in the
nav rail instead of `?` (WI #489). egui/epaint renders **monochrome glyphs only** —
it cannot do color emoji — so these show as black-and-white silhouettes.

## Provenance (derivative of Noto Emoji, OFL)

- Source: `NotoEmoji[wght].ttf` (variable) from google/fonts `ofl/notoemoji`.
- Instanced to a static Regular: `fonttools varLib.instancer … wght=400`.
- Subset to emoji Unicode ranges (misc symbols/dingbats, emoji blocks, regional
  indicators, ZWJ U+200D, variation selectors U+FE00–FE0F, keycap U+20E3).
- Result: static, 1811 glyphs, ~0.87 MB.

License: SIL Open Font License 1.1 — see `NotoEmoji-OFL.txt`. No Reserved Font Name
(the copyright line carries no RFN clause), so the name is retained.
