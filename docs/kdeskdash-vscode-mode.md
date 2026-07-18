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

## Reference

- kvscf publisher/subscriber: `crates/kvscf-app/src/remote.rs` in this repo.
- Overall mechanics: [architecture.md](architecture.md).
- klams memory (this contract): search `kvscf kdeskdash redis contract`.
