# Sprint 003 — Remote plumbing + kdeskdash handoff

Status: **done.** Redis channel to the claude-feed instance (publish `kvscf:instances:*` + subscribe
`kvscf:focus:*`), token auth, feature-gated so `kvscf-local` (kwork) has no comms code. Verified live
end-to-end, including background-thread focus and full outage recovery (rpidash2 power-cycle). Reverse
handoff + architecture docs delivered; kdeskdash `vscode` mode unblocked. Includes **WI #471**
(feature-gate; `kvscf-local`), pulled in from the start.

## Goal

Make the instance list drivable from [`kdeskdash`](ssh://kai/~/src/tools/kdeskdash) on kai: the Windows
side **pushes** the list and **accepts** select-to-focus back, and we hand off a spec for the kdeskdash
`vscode` mode. Deliberately split — this sprint owns the **Windows channel + protocol + handoff doc**;
the kdeskdash mode itself is implemented on kai as a follow-on (its own sprint, possibly driven from here).

## WI #471 — feature-gate the comms (do this first)

- Every bit of this sprint's channel lives behind a Cargo feature `remote` (**default on**);
  comms deps (ws/tokio/etc.) are **optional** so a no-`remote` build doesn't even compile them.
- Two artifacts, no build-flag footgun:
  - `kvscf` — default features (remote included).
  - `kvscf-local` — remote compiled out (for `kwork`).
  - Leaning: refactor the app into a lib (`kvscf-app`) + two thin bin crates so each artifact is
    self-describing; CI builds **both** feature sets so the no-comms build can't rot.
- **Slice 1 = this restructure with an empty `remote` module** (compiles both artifacts, no channel
  yet), then subsequent slices fill in the WebSocket client inside the feature.

### Slice 1 — DONE (2026-07-18)

Restructured into a lib + two thin bins: `kvscf-app` (lib, `remote` feature default-on, stub
`remote` module) → `kvscf` (full) and `kvscf-local` (no-comms). Verified: `kvscf --build-info` →
`remote=true`, `kvscf-local --build-info` → `remote=false`.

**Feature-unification gotcha (important):** in a whole-workspace `cargo build`, the shared `kvscf-app`
is compiled once with the *union* of features, so a workspace-built `kvscf-local` would have `remote`
**on**. Fixes: (a) `kvscf-local` is excluded from `default-members`, so a bare `cargo build` never
produces a unified one; (b) the comms-free artifact is built in **isolation** —
`cargo build --release -p kvscf-local`; (c) CI builds/lints it isolated and asserts
`--build-info` reports `remote=false`. Ship command for `kwork`: **`cargo build --release -p kvscf-local`**.

## Architecture (REVISED — Redis, not WebSocket)

The kdeskdash side already speaks **Redis**, so the transport is the shared **claude-feed Redis**
(`192.168.1.144:6380`, rpidash2; LAN, no Redis auth, ephemeral: 32mb / allkeys-lru / no persistence) —
**not** a WebSocket over Tailscale. (Contract driven by
`ken@kai:.../kdeskdash/.scratch/kvscf-kdeskdash-redis-handoff.md` + klams decision `019f7699…`.)

- kvscf **publishes** the instance list to Redis; kdeskdash reads & renders.
- kdeskdash **publishes** a focus command back; kvscf's subscriber consumes it → `focus_with` (the
  background-thread foreground case, from a subscriber thread — leans on the 001 `AttachThreadInput`
  recipe).
- The channel lives in `crates/kvscf-app` (`remote` module, behind the `remote` feature). **kvscf must
  be running** for the dashboard to drive it — no headless agent, no service.

## Contract (finalized — see [../../docs/kdeskdash-vscode-mode.md](../../docs/kdeskdash-vscode-mode.md))

- **Instance list:** `kvscf:instances:<host>` (String=JSON, TTL 10s, republished ~1s); discover via
  `SCAN kvscf:instances:*`. Per instance: `id` (HWND string = focus token), `label`, `workspace`,
  `remote`, `remote_host`, `app`, `active_file`, `z_index`.
- **Focus command:** pub/sub channel `kvscf:focus:<host>`, payload `{token, id, maximize}`.

## Auth: Option A (locked) — token in payload

Redis is unauthenticated (trusted LAN), so the preshared **`KVSCF_TOKEN`** (`kvscf-<64 hex>`, in `.env`
on both boxes, gitignored) gates the **focus command** — the only action. kvscf validates
`token == KVSCF_TOKEN` before foregrounding. Not riding krcmd's SSHSIG path.

## Slice 2 — DONE (2026-07-18)

`remote` module (`crates/kvscf-app/src/remote.rs`): publisher thread (`SET kvscf:instances:cleo` per
refresh, TTL 10s, backlog-collapsed, reconnecting) + subscriber thread (`SUBSCRIBE kvscf:focus:cleo` →
token-check → `focus_with`). Config from env / `.env` (cwd or exe dir). Deps (`redis`, `serde_json`,
`dotenvy`) are optional under `remote`, absent from `kvscf-local`.

**Verified live against the real claude-feed Redis:** `kvscf:instances:cleo` holds the correct JSON
(Stable + Insiders, ssh/local); `PUBLISH kvscf:focus:cleo` with a valid token foregrounded the tapped
window on cleo (confirmed by Ken) — including the background-thread case. Wrong token ⇒ ignored.

## Deliverables

- [x] Redis publisher + focus subscriber (reconnect/backoff), token-gated.
- [x] Token from gitignored `.env` (`KVSCF_TOKEN`); `.env` added to `.gitignore`.
- [x] **Reverse handoff** `docs/kdeskdash-vscode-mode.md` (finalized contract) + `docs/architecture.md`
      + klams memory `019f76c3…` so the kdeskdash agent can build the mode.

## Verification

- [x] kvscf publishes; the list appears in Redis and updates.
- [x] A focus command from Redis foregrounds the correct window **from a background subscriber thread**
      (the real hard-case test — passed).
- [x] Reconnect across a full Redis outage — verified via a real rpidash2 power-cycle (2026-07-18):
      both threads recovered with **no app restart** (publisher republished with a fresh ts; a focus
      command foregrounded a window again).

## Out of scope

- The actual kdeskdash `vscode` mode implementation on kai (follow-on, per the handoff doc).

## Notes / open questions

- Multi-Windows-box future: keys/channel are per-`<host>` and payload carries `host`, so kdeskdash can
  `SCAN` and namespace by box.
- kwork gets `kvscf-local` (no `remote`) — it never publishes, so no config/token needed there.
