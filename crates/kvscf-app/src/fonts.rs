//! Font setup: the bundled monochrome emoji fallback and the system bold face used for row
//! names. Moved here from lib.rs (WI #496).

use std::sync::Arc;

use eframe::egui::{self, FontFamily, FontId};

/// Family name for the system bold face rows use for names.
pub const BOLD_FAMILY: &str = "kvscf-bold";

/// Family name for our bundled monochrome emoji fallback.
const EMOJI_FAMILY: &str = "kvscf-emoji";

/// The row-name font: the bold family when available, plain proportional otherwise.
pub fn name_font(has_bold: bool) -> FontId {
    if has_bold {
        FontId::new(14.5, FontFamily::Name(Arc::from(BOLD_FAMILY)))
    } else {
        FontId::proportional(14.5)
    }
}

/// The muted host suffix font.
pub fn host_font() -> FontId {
    FontId::proportional(13.0)
}

/// Install fonts: a bundled monochrome emoji fallback (so emoji in window names render instead of
/// `?`, WI #489) plus Segoe UI Bold registered as [`BOLD_FAMILY`]. egui/epaint renders **monochrome
/// glyphs only** — no color emoji — so emoji appear as black-and-white silhouettes.
///
/// Returns whether the bold face was available (if not, rows fall back to regular proportional, but
/// the emoji fallback is still installed).
pub fn install_bold_font(ctx: &egui::Context) -> bool {
    let mut fonts = egui::FontDefinitions::default();

    // Bundled emoji font (instanced+subset Noto Emoji, OFL — see assets/NotoEmoji-kvscf.README.md).
    // Broader coverage than egui's built-in subset, which lacks e.g. U+1F980 🦀 / U+1F3D7 🏗.
    // Insert right after each stock family's primary face so text stays in the primary font and
    // only emoji fall through to it.
    fonts.font_data.insert(
        EMOJI_FAMILY.to_owned(),
        egui::FontData::from_static(include_bytes!("../../../assets/NotoEmoji-kvscf.ttf")),
    );
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        let v = fonts.families.entry(fam).or_default();
        v.insert(1.min(v.len()), EMOJI_FAMILY.to_owned());
    }

    // Bold face from the system (WI #465 styling), as its own family with the emoji fallback.
    let candidates = [
        r"C:\Windows\Fonts\segoeuib.ttf", // Segoe UI Bold
        r"C:\Windows\Fonts\seguisb.ttf",  // Segoe UI Semibold
        r"C:\Windows\Fonts\calibrib.ttf", // Calibri Bold
        r"C:\Windows\Fonts\arialbd.ttf",  // Arial Bold
    ];
    let has_bold = if let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts
            .font_data
            .insert(BOLD_FAMILY.to_owned(), egui::FontData::from_owned(bytes));
        let v = fonts
            .families
            .entry(FontFamily::Name(Arc::from(BOLD_FAMILY)))
            .or_default();
        v.extend([BOLD_FAMILY.to_owned(), EMOJI_FAMILY.to_owned()]);
        // Also chain egui's bundled emoji/symbol fonts (already in font_data) for the union of
        // coverage — e.g. symbols our subset omits.
        for extra in ["NotoEmoji-Regular", "emoji-icon-font"] {
            if fonts.font_data.contains_key(extra) {
                v.push(extra.to_owned());
            }
        }
        true
    } else {
        false
    };

    ctx.set_fonts(fonts);
    has_bold
}
