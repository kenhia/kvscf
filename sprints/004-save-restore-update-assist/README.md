# Sprint 004 — Save/restore window set + Insiders Update Assist

Status: **in progress.** WIs **#469** (save & restore the open window set) and **#470** (Insiders
Update Assist) — done as one sprint since #470 is built directly on #469's snapshot + relaunch.

## Key findings (research, 2026-07-18)

- **Path resolution (basename → full folder URI):** VS Code records every opened folder's full URI in
  `%APPDATA%\<Code|Code - Insiders>\User\workspaceStorage\*\workspace.json` (`{"folder": "<uri>"}`).
  Match an open window (basename + remote host + build) against these, most-recent `mtime` wins → the
  **exact URI to relaunch verbatim**. Two authority forms occur: `ssh-remote+kai` and
  `ssh-remote+ken@kai` — reuse whatever is stored (don't reconstruct).
- **No krcmd needed:** krcmd runs *remote→cleo*; kvscf is already on cleo, so relaunch is a local
  `code`/`code-insiders --folder-uri <uri>` — same command krcmd-host would run.
- **Close:** `PostMessage(hwnd, WM_CLOSE)` — normal close (respects unsaved-changes prompts). Update
  scenario assumes saved.

## Slice 1 — #469 save/restore (foundation)

- `kvscf-core::close_window(hwnd)` (`WM_CLOSE`).
- `kvscf-app::winset`: resolve open windows → folder URIs (via `workspaceStorage`), snapshot type
  `{variant, uri}[]`, save/load JSON, and `launch(uri, variant)` + staggered `relaunch(set)`.
- `--dump-set` CLI arg to verify resolution against the live windows.
- `serde_json` becomes a normal dep (winset needs it in both builds; not remote-only).

## Slice 2 — #470 Update Assist (UI, per Ken's notes)

Bottom **"Update Assist"** → **"Close Extras" / "Cancel"** → close all but one per **(host × build)** →
button becomes **"Relaunch"** ("Cancel" stays) + hint "Your turn, start the update(s) then click
'Relaunch'" → Ken runs updates → **Relaunch** → staggered `code`/`code-insiders` launches of the closed
set (~1–2 s apart). **Live test** with the real pending Code + Insiders updates.

## Progress

- **Slice 1 built + resolution proven:** `--dump-set` resolved all 17 live windows → exact folder URIs
  (locals + remote kai/kubs0, mixed `+host`/`+user@host` authorities). `close_window` (WM_CLOSE) added
  to `kvscf-core`.
- **Slice 2 built:** bottom-panel Update Assist state machine (Idle → ConfirmClose → ReadyRelaunch),
  Close Extras (keep one per remote host×build, close rest, locals untouched, survivor = lowest
  z_index), staggered Relaunch (~1.5s). Plus **Save set / Restore** (persisted "last" set under
  `%APPDATA%\kvscf\sets\`). Clippy-clean both builds.
- **Pending:** the live Update Assist test with the real Code + Insiders updates.

## Notes

- Available in **both** builds (kvscf + kvscf-local) — it's a local feature, not comms.
- Grouping key for "keep one": (remote host, build). Locals: left alone (not closed).
