# Reverse handoff: kvscf → kdeskdash `vscode` mode contract

**Direction:** kvscf side → kdeskdash side. This is the reply to
`ken@kai:/home/ken/src/tools/kdeskdash/.scratch/kvscf-kdeskdash-redis-handoff.md`. It defines the two
contracts the kdeskdash `vscode` mode implements. The kvscf side (publisher + focus subscriber) is
**built and verified live** against the claude-feed Redis.

## Endpoint (as specified by the kdeskdash handoff)

| | |
|---|---|
| Host (from cleo) | `192.168.1.144` (rpidash2) — kdeskdash reads it as `127.0.0.1:6380` on the Pi |
| Port | `6380` |
| Auth | none (Redis-level) — trusted LAN |
| Instance | ephemeral: `maxmemory 32mb`, `allkeys-lru`, no persistence |

Because Redis is unauthenticated, the **`KVSCF_TOKEN`** preshared token authenticates the *focus
command* (the only action). It lives in `.env` on both boxes (`ken@kai:.../kdeskdash/.env` and
`D:\ClaudeWorks\kvscf\.env`), as `KVSCF_TOKEN=kvscf-<64 hex>`.

## 1. Instance-list contract (kvscf publishes → kdeskdash reads)

- **Key:** `kvscf:instances:<host>` — one per publishing machine. Today only `kvscf:instances:cleo`.
- **Discovery:** `SCAN 0 MATCH kvscf:instances:* COUNT 100` (robust to host names / multiple boxes).
  Each value carries its own `host`, so you don't need to assume `cleo`.
- **Type:** Redis **String** containing a JSON object.
- **TTL:** 10s, republished every ~1s. A missing key ⇒ that machine's kvscf isn't running (or died) —
  render nothing for it; it self-expires.

**Payload:**

```json
{
  "host": "cleo",
  "ts": 1752863400,
  "instances": [
    {
      "id": "684134154",
      "label": "korg (kai)",
      "workspace": "korg",
      "remote": "ssh",
      "remote_host": "kai",
      "app": "insiders",
      "active_file": "Plan sprint with WI 260 …",
      "z_index": 3
    }
  ]
}
```

Field notes:
- `id` — **the focus token**: the Win32 HWND as a decimal string. Opaque to kdeskdash; echo it back
  verbatim in the focus command. Valid only while that window lives (stable within a kvscf run).
- `label` — ready-to-render row label (`workspace (remote_host)`, or just `workspace` when local).
- `workspace`, `remote_host` — the parts of `label`, if you want to style them separately (e.g. host in
  a different color, as the kvscf app does).
- `remote` — one of `local` | `ssh` | `wsl` | `devcontainer` | `codespaces`.
- `app` — `stable` | `insiders` | `exploration` | `unknown` (accent Insiders vs Stable differently).
- `active_file` — active editor / tab label; may be truncated with `…` or `null`.
- `z_index` — enumeration order (0 = most-recently-active); optional sort signal. kvscf sorts by name.

## 2. Focus-command contract (kdeskdash publishes → kvscf consumes)

- **Channel (pub/sub):** `kvscf:focus:<host>` — use the `host` from the instance payload
  (e.g. `kvscf:focus:cleo`).
- **Delivery:** pub/sub, fire-and-forget (not durable — matches the ephemeral instance). No reply.
- **Payload:**

```json
{ "token": "kvscf-<64 hex>", "id": "684134154", "maximize": false }
```

- `token` — must equal `KVSCF_TOKEN`; kvscf ignores the message otherwise.
- `id` — the `id` from the tapped instance row (the HWND string).
- `maximize` — optional (default `false`). `true` ⇒ kvscf maximizes the window as it focuses it.

kvscf validates the token, then foregrounds that HWND. Verified live: tapping publishes → the window
comes to the foreground on cleo.

## 3. Edge windows (WI #474) — extend Remote Mode

kvscf also publishes open **Microsoft Edge** windows, so kdeskdash can add an Edge mode (or a
Code/Edge toggle). Same shape as the instance list; the **focus command is identical** (the `id` is
just an HWND, kind-agnostic — publish to `kvscf:focus:<host>` exactly as for VS Code).

- **Key:** `kvscf:edge:<host>` (e.g. `kvscf:edge:cleo`), Redis **String** = JSON, **TTL 10s**,
  republished ~1s. Discover via `SCAN kvscf:edge:*`.
- **Payload:**

```json
{
  "host": "cleo",
  "ts": 1784416199,
  "windows": [
    { "id": "133434", "label": "AI-2 Computer Purchase", "named": true,  "tab_count": null, "z_index": 64 },
    { "id": "657812", "label": "Dashboard | Claude Platform", "named": false, "tab_count": 9, "z_index": 34 }
  ]
}
```

Field notes:
- `id` — the HWND string (the focus token; echo it back in the focus command, same as VS Code).
- `label` — ready-to-render: the user-set window name for **named** windows, else the active tab title.
- `named` — `true` = a user "Name window…" window, `false` = tab-title-derived. **Suggested UI: render
  named windows first (sorted), a separator, then unnamed** — that's what the kvscf app does.
- `tab_count` — best-effort tab count for unnamed windows (`null` for named). Optional badge.
- `z_index` — enumeration order (0 = most-recently-active); optional sort signal.

The kvscf app renders named windows in an Edge-teal accent; unnamed muted. Match if you like.

## 4. Apps (sprint 007) — configured apps, focus-if-running-**else-launch**

kvscf also publishes a set of **configured apps** (Ken's frequently-switched-to apps, e.g. Claude,
Everything, Terminal, Battle.net, Kindle). Unlike Code/Edge these are *configured*, not
auto-discovered, and each one may or may not be running. The key twist for the dashboard: a
**non-running app has no HWND** — it's **launched**, not focused. So the command carries the app's
**stable `key`** (not an HWND), and kvscf does *focus-if-running-else-launch* on its side.

- **Key:** `kvscf:apps:<host>` (e.g. `kvscf:apps:cleo`), Redis **String** = JSON, **TTL 10s**,
  republished ~1s. Discover via `SCAN kvscf:apps:*`.
- **Payload:**

```json
{
  "host": "cleo",
  "ts": 1784416199,
  "apps": [
    { "key": "claude",     "label": "Claude",     "running": true,  "id": "9176544", "order": 0 },
    { "key": "everything", "label": "Everything", "running": true,  "id": "182783712", "order": 1 },
    { "key": "kindle",     "label": "Kindle",     "running": false, "id": null, "order": 5 }
  ]
}
```

Field notes:
- `key` — the **stable app id** (its registry subkey). **This is what the command echoes back**, not
  an HWND — because a non-running app has no HWND.
- `label` — ready-to-render display name.
- `running` — `true` = a matching window is open. **Suggested UI: render non-running apps greyed
  out**; a tap on either state sends the same command.
- `id` — the HWND string **when running**, else `null`. Informational (kvscf resolves the target
  itself from `key`); you don't need it for the command.
- `order` — configured sort index (kvscf sorts by it, then label). Optional.

**Command (kdeskdash publishes → kvscf consumes):** same channel `kvscf:focus:<host>`, but keyed by
`app` instead of `id`:

```json
{ "token": "kvscf-<64 hex>", "app": "kindle" }
```

kvscf validates the token, then **focuses the app if a matching window is open, else launches it**
(exe or Store AUMID) and foregrounds it once its window appears (cold launch can take several
seconds). `app` takes precedence over `id` if both are present. The HWND-based
`{ "id": … }` command (sections 2–3) is unchanged and still used for Code/Edge rows.

## Reference

- kvscf publisher/subscriber: `crates/kvscf-app/src/remote.rs` in this repo.
- Overall mechanics: [architecture.md](architecture.md).
- klams memory (this contract): search `kvscf kdeskdash redis contract`.
