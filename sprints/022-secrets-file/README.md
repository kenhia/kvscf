# Sprint 022 — the Redis password from the per-host secrets file

korg kvscf **WI 2412** (chore, S), proposal **korg:2434** — a consumer slice of korg program
**2440**, "Simplify homelab secrets". Overseen: the ship waits on the overseer's green light.

## Goal

kvscf stops treating `HKCU\Software\kenhia\kvscf` as the home of its Redis password. It reads the
fleet's one per-host copy, `%ProgramData%\khomelab\secrets.env`, using the same rules every other
consumer in the program uses (CD-19, kdashdata handoff korg:2494):

1. the environment,
2. `%ProgramData%\khomelab\secrets.env` — `ProgramData` read at run time, the rung skipped when it is
   unset, never assumed to be `C:\ProgramData`,
3. the old registry value, deprecated, still answering, with a warning.

The pairing token (`KVSCF_TOKEN`) does **not** move: whose secret it is is korg WI 2479, still
unanswered, and the proposal said to leave it on HKCU rather than invent a key name.

## Premise check — the key name was wrong for cleo

The proposal gave kvscf's key as `KVSCF_REDISCLI_AUTH`. Fingerprints (sha256, first 12 hex — no
value was read into the session) say otherwise:

| copy | fingerprint |
|---|---|
| cleo, HKCU `KVSCF_REDIS_PASSWORD` | `daa601b6368a` |
| rpidash2, per-host file, `CLAUDE_REDISCLI_AUTH` | `daa601b6368a` |
| rpidash3, per-host file, `KVSCF_REDISCLI_AUTH` | `d3f6d2c72545` |

cleo publishes to **rpidash2**, whose instance (`redis-claude`, shared with the Claude feed's
neighbour) has the fleet name `CLAUDE_REDISCLI_AUTH`. `KVSCF_REDISCLI_AUTH` is **rpidash3's**, the
kwork desk. One kvscf binary talks to two endpoints, and every password has one fleet-wide name — so a
kvscf that only knew `KVSCF_REDISCLI_AUTH` would never find cleo's password in cleo's own file.

Also measured, and put on WI 2479 as evidence: cleo's `KVSCF_TOKEN` (`af4fcf90e571`) is the same
value as rpidash2's panel copy, which reads as one pairing token per desk pair rather than a stale
copy.

## Decisions

- **The key follows the endpoint, by an explicit setting** (Ken, 2026-09-13). New non-secret
  `KVSCF_REDIS_AUTH_KEY`, read from env / `.env` exactly like `KVSCF_REDIS_HOST`. Default
  `CLAUDE_REDISCLI_AUTH`, paired with the default endpoint. kwork's `.env` gains
  `KVSCF_REDIS_AUTH_KEY=KVSCF_REDISCLI_AUTH` beside its host line.
  - Rejected: **a lookup order over both names.** No config change anywhere, but a host whose file
    ever held both would silently present the wrong password — the least debuggable failure there is.
  - Rejected: **renaming rpidash2's key.** A k-homelab + kdeskdash contract change, not this repo's.
- **kwork is not reachable from cleo** — ping answers, ssh times out on port 22 (probed from cleo). Its
  deploy and live check move to the Windows changeover, WI 2404, which is Ken's at kwork. Nothing
  breaks there meanwhile: kwork's registry value is still a rung. **Ordering condition** (overseer):
  kwork's `.env` line goes in *before* its registry password is deleted, or kwork's kvscf finds no
  password at all and loses its desk. Written onto WI 2404; pinned by a unit test
  (`without_the_setting_a_kwork_file_falls_through_to_the_registry`).
- **Resolved on every connect, not at startup** — same reason as kpidash-client-win (korg:2429): a
  rotation rewrites the file and the next reconnect reads it, no relaunch. `Config` no longer holds a
  password at all; `connection_info` takes the value resolved for that connect.
- **Name the source, never the value.** The first connect prints
  `kvscf: redis password from <source>`, and again only when the source changes. A release build has
  no console, so the observable form is a probe: **`kvscf --probe-redis-auth`** prints endpoint, key
  and source, then AUTHs, writes `kvscf:probe:<host>` (TTL 10s) and deletes it; exit 2 unless the write
  landed. A write rather than a PING because an endpoint that answers unauthenticated commands would
  pass a PING. WI 2404 uses this to prove the file became the source.
- **Transliterated, not rewritten.** `redis_auth.rs` is kpidash-client-win's `auth.rs` with the key a
  parameter and the registry as the last rungs — no third parser in the fleet.

Checked and **not** needed: the URL-escaping repair the proposal asked about. kvscf has put the
password in a `redis::ConnectionInfo` since sprint 018.

## What changed

- `crates/kvscf-app/src/redis_auth.rs` — new: key setting, parser, resolver with injected environment
  and registry, source announcement. 19 tests, including the kwork cases and "the registry is not
  read when a better rung answers".
- `remote.rs` — `Config` carries `auth_key` instead of a password; both loops resolve through
  `Config::client()` on every connect; `--probe-redis-auth`.
- `probes.rs` — the probe flag (full build only), in `--help`.
- Docs: `architecture.md`, `README.md`, `kdeskdash-vscode-mode.md`, the roadmap.

## Verification

Gate: `just check` green on stable 1.98.1 — both passes, 88 tests in `kvscf-app`, `--build-info`
remote=false. A baseline run on `main` went first, so no lint could be mistaken for this sprint's.

### The file rung, proven with a control

cleo has no real per-host file yet (WI 2404 creates it), and a hand-made one in `%ProgramData%\khomelab`
would be WI 2404's to own. So, as kpidash-client-win did: scratch directories under the user profile
(inheritance broken; SYSTEM, Administrators, `kenhi`), `ProgramData` pointed at each in turn, the
**release** build's `--probe-redis-auth` run from an `ssh cleo` session. The "real" file was written on
cleo straight from the HKCU value — same host, never through the session — and its fingerprint
checked (`daa601b6368a`). It also carried decoy `REDISCLI_AUTH` and `KVSCF_REDISCLI_AUTH` lines. The
HKCU value stayed present throughout, so the file had to *outrank* a working password to be seen:

| `ProgramData` → | source reported | result |
|---|---|---|
| `pass` — real value | per-host secrets file (`…\pass\khomelab\secrets.env`, `CLAUDE_REDISCLI_AUTH`) | `write: ok`, exit 0 |
| `control` — wrong value | per-host secrets file (`…\control\…`) | `connect: REFUSED - Password authentication failed`, exit 2 |
| `empty` — no file | HKCU `KVSCF_REDIS_PASSWORD` (deprecated) | `write: ok` |
| unset | HKCU (deprecated) — the rung skipped, not guessed | `write: ok` |

The control is what makes the pass mean something: had the file not been read, the HKCU value would
have answered and the control would have passed too.

### The deployed app, proven with a control

The probe shares the resolver and `ConnectionInfo` with the running app but is not the running app. So
the app got its own check, read from rpidash2 (`ttl kvscf:<feed>:cleo`, all four feeds):

| state | rpidash2 |
|---|---|
| old build, before | all four TTL 10 |
| unauthenticated `SET` from cleo | `-NOAUTH Authentication required.` — the endpoint really does refuse |
| new build deployed, relaunched | all four TTL 9–10 |
| wrong `CLAUDE_REDISCLI_AUTH` in `C:\tools\bin\.env`, relaunched | all four **-2** (gone) |
| that `.env` removed, relaunched | all four TTL 9 |

The middle row is the decisive one: the old build did not know `CLAUDE_REDISCLI_AUTH` existed and would
have kept publishing through it. Only a build resolving through the new key goes dark.

`C:\tools\bin\kvscf.exe` = built artifact, sha256 `A6DB55E8C9856FE2…` (was `AE121435…`, sprint 021).
Restarted four times through `kvscf-relaunch`, with Ken's go; left running the new build. The scratch
directories — one held a real copy — and the control `.env` were deleted, and their absence confirmed.

**What is not live-proven:** resolution on reconnect picking up a *rotated* file without a relaunch.
It is structural (the password is resolved inside the connect loop and `Config` holds none), but a
live demonstration needs a rotation, which is the proof pass's (korg:2439), not this slice's.

Probed from **cleo** for everything above — the host that runs kvscf. rpidash2's side was read on
rpidash2.

## Repaired in passing

- **Stale docs that said rpidash2's Redis has no password.** It has had one since korg:2231
  (2026-09-10): `architecture.md`, the contract's Auth row, and `remote.rs`'s module doc all said "no
  auth, trusted LAN". Corrected alongside the password section they sat in.
- **The roadmap's "Tell the dashboard about dev hosts"** said `ext_dev_host` was not on the wire; sprint
  021 put it there. Now says only kdeskdash's half (korg #2365) remains. The "first live AUTH run" item
  is gone — sprint 021 measured `AUTH +OK` on rpidash2.
- **`lib.rs` said `kvscf-local` is "for kwork"**; kwork runs the full build since the kwork/rpidash3
  pairing.

## Left for others

- **WI 2404** (k-homelab, Windows changeover): create cleo's file with `CLAUDE_REDISCLI_AUTH`; on kwork,
  install this build, add the `.env` line, create the file with `KVSCF_REDISCLI_AUTH`, check, *then*
  delete each host's HKCU password. Until then the registry value is kvscf's only source on both.
- **WI 2479**: `KVSCF_TOKEN`.
