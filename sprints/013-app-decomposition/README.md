# Sprint 013 — review follow-up: app decomposition

From the [2026-07-20 deep review](../review/2026-07-20-review.md) — WI #493, #496. Pure
restructuring; the one intentional visual tweak is noted below.

## What changed

- **WI #493 — one row painter.** The four near-identical painters (`draw_row`, `draw_fav_row`,
  `draw_edge_row`, `draw_app_row`, ~300 lines) are now thin specs over a single `rows::draw`
  taking a `RowSpec` (optional gutter marker, truncating main text, never-truncated trailing
  host). Small deliberate improvement: the Apps rows' ●/○ dot moved from inline text into the
  same reserved gutter the Code tab uses, so app labels align with each other (and with Code
  rows) whether running or not.
- **WI #496 — `lib.rs` decomposed** from 1,358 to ~780 lines. New modules:
  - `rows` — the row painter + per-kind specs
  - `theme` — the color functions (restructured into tokens next sprint, WI #501)
  - `fonts` — emoji fallback + bold-family install, `name_font`/`host_font`
  - `settings` — registry persistence (cfg split inside)
  - `probes` — the five headless flags, previously five separate `env::args()` scans in
    `run()`, now one pass — plus a new `--help` that lists them
  - `single_instance` — the named-mutex guard
  - `update()` shed its ~370-line body into `ui_tabs` / `ui_header` / `ui_update_assist` /
    `ui_code_tab` / `ui_edge_tab` / `ui_apps_tab`, with clicks collected in an `Actions` struct
    and applied after the borrows release.

## Verification

`cargo fmt --check`, clippy `-D warnings` (default + `-p kvscf-local`), 29 tests, `--build-info`
and `--help` probes — all green.
