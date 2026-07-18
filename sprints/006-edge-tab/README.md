# Sprint 006 — Edge tab (WI #474)

Status: **built + verified.** Implements the `[ Code | Edge ]` in-app tab from the sprint-005 research.

## What shipped

- **`kvscf-core` generalized:** one enumeration pass (`raw_windows`) feeds `scan()` (VS Code),
  `scan_edge()` (Edge), and `scan_all()` (both). New `EdgeWindow { hwnd, label, named, tab_count,
  z_index }` and `parse_edge_title` (the sprint-005 rule: drop U+200B, ` - Microsoft Edge` suffix ⇒
  unnamed/tab-derived; else named = title verbatim; strip profile + ` and N more pages`). 5 unit tests
  over real titles. CLI gained `kvscf-core edge`.
- **App `[ Code | Edge ]` tab strip:** Edge tab renders **named windows first (Edge-teal, alphabetical)
  → separator → unnamed (muted, tab titles)**; click focuses (reuses `focus_with`). Save/Restore +
  Update Assist hide on the Edge tab (VS-Code-only). Focus/close unchanged.
- **Remote extended:** publisher also SETs `kvscf:edge:<host>` (JSON, TTL 10s). The **focus channel is
  unchanged** — `id` is just an HWND, so tapping an Edge window on kdeskdash already works. Handoff for
  the kdeskdash Edge mode: `docs/kdeskdash-vscode-mode.md` §3 + klams `019f777f`.

## Verified

- `kvscf-core edge` on live windows → 9 named (alphabetical) + 18 unnamed with tab counts.
- `kvscf:edge:cleo` publishes 27 windows (9 named) with the documented payload.
- 16 core unit tests pass; clippy/fmt clean on both builds; `kvscf-local` still comms-free.

## Notes / not done

- Edge windows aren't folder-URI relaunchable → no save/restore or Update Assist for Edge (by design).
- Edge Dev/Beta/Canary channels untested (Ken uses stable). Profile-strip assumes a profile is shown in
  unnamed titles (true for signed-in Edge); see sprint-005 doc for the edge case.
