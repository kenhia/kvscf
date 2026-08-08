# Sprint 016 — the Launcher verb

Slice 2 of korg program **1143** ("Launcher — retire the Stream Deck", spanning kvscf +
kdeskdash). Covers korg kvscf **#1133**; the gating spike was **#1132**, slice 1.

## What Ken is replacing

A 32-key Stream Deck that opens URLs but cannot choose *which* Edge window they open in, and
cannot bring that window forward. Every use costs 3-5 seconds of hunting, 10-50 times a day.
kvscf already knows every Edge window, named and unnamed, with Z-order. The gap was one verb.

## The verb

`focus_with(hwnd)` → settle ~100ms → spawn `msedge.exe <url>`. Chromium opens the tab in the
last-active window of the matching profile, and foregrounding *is* what makes it last-active.

The spike (#1132) proved this on cleo — 8/8 trials, no new windows ever created, works from a
minimized target, and **0ms delay already passed every trial**. The 100ms is deliberate
insurance for a busier machine than the idle box the spike ran on. The synthesized `Ctrl+T`
fallback was the contingency if this failed; it did not, so it is **not built**.

Resolution order, in `kvscf_core::pick_edge_target` (pure, six unit tests):

1. Named target → the **named** Edge window whose label matches, case-insensitive exact.
2. Not found, **or** "use current" → the top-Z Edge window.
3. Nothing open → cold-start Edge with the URL.

**Step 2 is one path serving two cases on purpose.** "Use current" and "your preferred window
went away" are the same behaviour, so the fallback is exercised on every use instead of being
rare code that first runs on the day a window got renamed.

## Why there is no regex

Ken asked for regex matching, then said why: his window names contain emoji, and he trusted
himself to type `.*Pipes` but not `🛠️ Pipes`. That is a **data-entry** problem, and slice 4's
editor solves it directly with a dropdown of live named windows. Matching happens inside kvscf,
so if patterns are ever genuinely wanted it is a one-field change with **no contract impact**.

## Config

`HKCU\Software\kenhia\kvscf\launcher\<key>` — one subkey per button, same shape and `UserRoot`
handling as `apps\<key>`, reloaded every refresh so an edit reaches the panel in ~2s with no
restart. `rows`/`cols` on the parent key (default 3x9).

Values: `label`, `url`, `target` (`current`|`named`), `target_name`, `color`, `row`, `col`,
`w`, `h`. A named target stores the window's **name**, never its HWND — handles die with the
window, names survive it.

`validate_layout` drops buttons that cannot be drawn — unusable size, off-grid, or overlapping —
with a warning, earlier-wins on a contested cell. The slice-4 editor will make bad placements
impossible to express; until then Ken edits the registry by hand, which is exactly when overlaps
happen.

## Contract

Published as `kvscf:launcher:<host>` and documented as **§6 of
[docs/kdeskdash-vscode-mode.md](../../docs/kdeskdash-vscode-mode.md)** — frozen before the
kdeskdash side starts, the same discipline that let the Edge mode land with zero round-trips.

Command is `{token, button:<key>}` on the existing `kvscf:focus:<host>` channel, precedence
`button` > `app` > `id`. The §2-§5 forms are untouched.

**The payload carries no `url` and no `target`.** The dashboard draws buttons; it never learns
where they go. On kwork those URLs are an employer's business and stay on the employer's machine.

## Verification

fmt / clippy `--all-targets -D warnings` on **both** feature sets / 43 tests green (21 core,
22 app).

Live on cleo against 15 real Edge windows, via two new probes:

- `--dump-launcher` resolves every button against the **current** window list, so a button that
  silently degrades to the fallback is visible rather than inferred. All three rejection paths
  (no url / overlap / off-grid) reported and skipped correctly.
- `--fire-button <key>` ran the whole verb: the `GitHub` window went **z=3 → z=0** and took the
  tab. No dashboard, no Redis round trip.

Test registry entries were removed afterwards; `apps` and settings untouched.

## Notes for later

- **Edge window titles carry a profile segment** (`… - Personal - Microsoft Edge`, visible in
  `parse.rs`'s Edge fixtures). Not used, and not needed on cleo — which has exactly one profile,
  so `--profile-directory` is moot there. It is the obvious seam **if kwork turns out to have a
  work profile**, which slice 5 must check rather than assume.
- Named Edge windows still publish `tab_count: null` (the open #474 follow-up). That is why
  verifying tab placement uses the unnamed-window title-change trick from #1132 rather than a
  tab count.
- `crates/kvscf-core/src/bin/spike_url.rs` is kept deliberately, not deleted — slice 5 re-runs
  it on kwork.
