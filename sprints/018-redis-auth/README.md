# Sprint 018 — Redis AUTH for the publisher

Slice 4b of korg program **1143** ("Launcher — retire the Stream Deck"), covering korg kvscf
**#1147**. Unplanned: added 2026-08-09, between slices 4 and 5, and the only thing slice 5 (#1137)
was waiting on.

## Why it exists

Slice 5's security paragraph said the endpoint would get a `requirepass` and that "kdeskdash's
`kvscf_redis_init` already takes an `auth` argument and rpi53's instance already does exactly this,
so both sides are proven". True of the **reader** and of the **server**. The **publisher** — kvscf,
the side that actually has to present a password — could not: `Config` carried only host and port,
and `url()` emitted a bare `redis://{host}:{port}`.

Found by the kdeskdash session picking up slice 5, before writing code, by reading kvscf instead of
trusting the WI.

Worth stating plainly: this was not a gap, it was a **stated assumption**. `remote.rs` said so in
its own module doc — *"Redis itself is unauthenticated (trusted LAN), so `KVSCF_TOKEN` is the
app-level gate"*. This sprint revises that premise, so the doc changes with the code, here and in
`docs/architecture.md` and §Endpoint of the contract doc.

## Two gates, and they are not interchangeable

- **`KVSCF_TOKEN`** — app-level, gates the focus command, **mandatory**. No token, no channel.
- **`KVSCF_REDIS_PASSWORD`** — transport-level, **optional**. No password, no AUTH — *not* no
  channel.

That asymmetry is the whole design. cleo publishes to rpidash2:6380, which is deliberately open on
the trusted home LAN, and had to keep working untouched. Same resolution order for both
(`HKCU\Software\kenhia\kvscf` first, then env / `.env`), since the registry is what survives a
pinned launch from `C:\tools\bin` with no cwd or exe-dir `.env` — the existing reason the token
prefers it.

## A struct, not a URL

The obvious implementation is `redis://:{pw}@{host}:{port}`. It was rejected for two independent
reasons, either sufficient on its own:

1. **The endpoint string is logged.** `Channel::start` prints it to stderr when the channel comes
   up. A password in the URL is a password in the log — and this is a desktop app whose stderr
   nobody thinks of as sensitive.
2. **Escaping.** `@`, `:`, `/`, `#` and `%` are all structural in a URL and all plausible in a
   generated password. Getting it wrong presents the *wrong credentials* silently, or fails to
   parse and surfaces as an ordinary reconnect loop — two of the least debuggable outcomes
   available.

`redis::Client::open` takes `impl IntoConnectionInfo`, so none of that is necessary: the password
goes into `RedisConnectionInfo` (which derives `Default`) and never becomes part of a string
anything prints. `endpoint()` remains, display-only, with auth reported beside it as a boolean —
worth a word, because an endpoint that grows a `requirepass` while kvscf has none fails as a silent
reconnect loop, and that log line is what separates it from an unreachable host.

Also rejected, considered and named so it does not come back: setting
`KVSCF_REDIS_HOST=":<pw>@192.168.1.73"` so the existing `url()` happens to emit an authenticated
URL. It ships the same day and needs no code — and stores a secret in a field named `host`, which
leaks into that same stderr line, breaks the moment anyone validates the field or splits
`host:port`, and would have krot registering a password whose location is an env var named after
something else.

## Verification

Full CI workflow locally on the matching toolchain (rustc 1.97.1 — see sprint 017 for why that is
now checked): fmt, `clippy --all-targets -D warnings` for both feature sets, both builds, tests, and
the `--build-info` remote=false assertion. 67 tests.

Four new, all on `Config`:

- No password configured → `connection_info().redis.password` is `None`, address unchanged. The
  regression test for "cleo must keep working".
- A configured password reaches the connection.
- `endpoint()` never contains the password — the regression test for reason 1 above.
- A password of `p@ss:w/rd#%` survives intact — the case a hand-built URL fails, and the argument
  for the struct in one assertion.

Not verified live against a `requirepass` endpoint: there isn't one yet. rpidash3's Redis gets its
password in slice 5, and that is where this gets its first real AUTH — see the handoff on
`korg:1142`.
