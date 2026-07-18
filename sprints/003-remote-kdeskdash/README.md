# Sprint 003 — Remote plumbing + kdeskdash handoff

Status: **planned** (depends on 001; independent of 002's UI).

## Goal

Make the instance list drivable from [`kdeskdash`](ssh://kai/~/src/tools/kdeskdash) on kai: the Windows
side **pushes** the list and **accepts** select-to-focus back, and we hand off a spec for the kdeskdash
`vscode` mode. Deliberately split — this sprint owns the **Windows channel + protocol + handoff doc**;
the kdeskdash mode itself is implemented on kai as a follow-on (its own sprint, possibly driven from here).

## Architecture (see PLAN §6)

- Windows agent opens an **outbound WebSocket to kdeskdash** over the Tailscale tailnet
  (`encke-wahoo.ts.net`) — outbound-only, so nothing dials into Windows.
- Push the instance list on change (debounced).
- Receive `{ "select": <hwnd> }` → re-validate handle → run the `kvscf-core` focus path (the **hard**
  background-foreground case; leans on the 001 `AttachThreadInput` verification).
- The channel lives in `crates/kvscf` as a background client task. **kvscf must be running** for the
  dashboard to drive it (locked) — no headless agent, no service.

## Protocol (draft — pin down in the handoff doc)

- Transport: WebSocket, JSON frames, over Tailscale.
- **Up** (Windows→kdeskdash): `{ "type": "instances", "items": [ { "hwnd", "app", "workspace",
  "remote": {"kind","host"}, "active_file", "z_index" } ], "host": "<win-box>" }`.
- **Down** (kdeskdash→Windows): `{ "type": "select", "hwnd": <u64> }`.
- Handles are per-session; kdeskdash treats them as opaque tokens and echoes them back.

## Auth: Option A (locked)

**Tailscale-only + preshared token.** The tailnet (`encke-wahoo.ts.net`) is the network boundary; a
preshared token in the frame gates the `select` action specifically (the only frame that takes action).
Not riding krcmd's SSHSIG path. Token stored in local config (gitignored), never in the repo.

## Deliverables

- [ ] Windows-side WebSocket client: connect (with reconnect/backoff), push-on-change, handle `select`.
- [ ] Option A auth: preshared-token gate on `select`, token from gitignored local config.
- [ ] **Handoff doc** `docs/kdeskdash-vscode-mode.md`: protocol, frame schemas, connection/lifecycle,
      auth, and a UI sketch for the `vscode` mode (list of `workspace (host)` chips, host-colored, dirty
      marker; tap → `select`). Enough for the kdeskdash build to proceed without this conversation.

## Verification

- [ ] Windows agent connects to a stub/echo kdeskdash endpoint over Tailscale; list appears and updates.
- [ ] A `select` frame from the stub foregrounds the correct window **while the Windows app is
      backgrounded** (the real test — reuses 001's hard-case path).
- [ ] Reconnect survives kdeskdash restart and Windows sleep/wake.

## Out of scope

- The actual kdeskdash `vscode` mode implementation on kai (follow-on, per the handoff doc).

## Notes / open questions

- Multi-Windows-box future: `host` field is already in the up-frame so kdeskdash can namespace by box.
