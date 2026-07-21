# Sprint 014 — review follow-up: theme tokens + design uplift

From the [2026-07-20 deep review](../review/2026-07-20-review.md) — WI #501, #502. The visual
sprint: same behavior, cleaner chrome, and a design layer that's editable in one file.

## What changed

- **WI #501 — theme tokens.** The eight ad-hoc color functions became one `Palette` struct with
  named tokens (`insiders`, `exploration`, `stable`, `edge`, `edge_unnamed`, `host`, `fav_star`,
  `dim`), resolved per dark/light by `theme::palette(dark)`, with `Palette::app(build)` and
  `Palette::dimmed(accent)` (the blend-halfway-to-gray used by not-open favorites). All spacing
  magic numbers moved to `theme::dims` (`ROW_PAD`, `ROW_MIN_H`, `GUTTER`, `ROW_GAP`,
  `PANEL_PAD`, `SECTION_GAP`, `ACTIVE_BAR_W`). Colors are unchanged — same RGB values, now named.
- **WI #502 — chrome uplift.**
  - **One-row top bar**: tabs (with their live counts) + right-aligned ⟳ and a new **⚙ settings
    popup** holding the three mode checkboxes. The old second header panel — three always-visible
    checkboxes plus a "N window(s)" line that duplicated the tab counts — is gone; the list gets
    that vertical space back.
  - **Active-window indicator**: the row whose window currently holds the foreground gets a thin
    rounded accent bar (its own accent color) at the left edge — on all three tabs. New
    `kvscf_core::foreground_hwnd()` supports it.
  - **Update Assist / sets panel**: one compact button row — `Save set` `Restore` … `Update…`
    (right-aligned) — instead of two stacked rows; status line unchanged beneath.
  - Tab buttons get slightly larger padding for a more deliberate segmented look.

## Verification

`cargo fmt --check`, clippy `-D warnings` (default + `-p kvscf-local`), 29 tests, and a smoke
launch (exited cleanly via the single-instance mutex — Ken's live kvscf was running). The visual
result needs Ken's eyeball after restarting kvscf on the new build; every change is a small,
revertable commit-level decision if anything doesn't land.
