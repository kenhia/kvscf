# Sprint 012 — review follow-up: core DRY + hygiene

From the [2026-07-20 deep review](../review/2026-07-20-review.md). The small, mechanical,
test-backed half of the findings — WI #494, #495, #497, #498, #499, #500.

## What changed

- **WI #494 — string conversions centralized in `kvscf-core`.** `App::key()` / `App::from_key()` /
  `Remote::kind()` replace the duplicated `app_str` (remote.rs) / `app_key` + `app_from_key`
  (winset.rs) / `remote_kind` (remote.rs). The Edge named-first sort comparator, previously written
  twice (app refresh + CLI `edge`), is now `kvscf_core::sort_edge_windows`. The ~50 lines of
  per-item `#[cfg(not(windows))]` stubs in core's lib.rs are grouped into one `stubs` module.
- **WI #495 — `SetEntry` carries `workspace` + `host`.** Set at resolve time from the `Instance`;
  re-derived from the URI on load (label fallback). `remote.rs::split_label` — which reverse-parsed
  `"workspace (host)"` and would mis-split a workspace named `foo (bar)` — is deleted. Persisted
  JSON shape (`{app, uri, label}`) is unchanged; old favorites/sets files load as before.
- **WI #497 — `remote.rs` blanket `#![allow(dead_code)]` removed.** It was masking a genuinely dead
  `Channel.host` field + `host()` method (never called); both deleted.
- **WI #498 — winset tests.** `parse_uri` (ssh, ssh+user@, encoded drive letters, trailing slash,
  encoded spaces, degenerate inputs), `percent_decode` (hex escapes, malformed pass-through), and a
  write/read roundtrip asserting workspace/host re-derivation. 8 new tests, all portable.
- **WI #499 — no more double window scans.** `resolve_open_set` now takes the caller's already
  scanned `&[Instance]` — the app's 1s refresh path no longer runs a second `EnumWindows` +
  per-window `OpenProcess` pass when a new window appears. One-shot callers (Save set, Close
  Extras, `--dump-set`) use `resolve_open_set_now()`, which still scans fresh.
- **WI #500 — `[profile.release]`.** `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`.
  kvscf 6.8 → 6.6 MB, kvscf-local 6.2 → 6.1 MB (most of the weight is the bundled fonts + egui);
  the real win is symbol stripping for the exes that get copied around. `--build-info` verified on
  both builds.

## Verification

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (default and `-p kvscf-local`),
`cargo test` (29 tests: 16 parse + 13 app incl. the new winset suite), release builds of both bins
with the `--build-info` probe — all green.
