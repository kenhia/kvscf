# Sprint 019 — onto the kprojects harness

korg kvscf **#1238**, proposal **korg:1243** — batch 2 of the kprojects rollout (korg #737), and
the **first migration run on Windows**. A chore sprint: no behaviour changes to the app.

## What was here first

Less than the word "migration" suggests. kvscf carried **no old harness at all** — no `CLAUDE.md`,
no `.github/copilot-instructions.md`, no Spec-Kit / ATV / Phoenix footprints. What it did have was
most of the layout already, arrived at independently: `sprints/NNN-slug/` records, `sprints/review/`,
`docs/`, `.scratch/`. So the installer only added `sprints/planning/`, a `justfile`, and the managed
instruction blocks.

`stack : rust (detected)` from the root `Cargo.toml`, as expected — `--stack` was omitted.
Unaffected by kprojects #1254 (that fix only fires on a repo that *already* has a justfile lacking
`check`; kvscf had none, so the rust template was seeded whole).

`.claude/skills/` — `kvscf-add-app`, `kvscf-window-title` — survived untouched.

**`PLAN.md` stays at the root.** It is genuine design content, not harness machinery: README,
`docs/architecture.md` and sprint 001 all link to it. Moving it under `sprints/planning/` would have
bought a tidier tree for three broken links and a rewrite of a public repo's front page.

## The gate is the work

The seeded rust `check` already ran `cargo clippy --all-targets -- -D warnings` — so the thing this
sprint was originally scoped around was ticked by the template before it started. The real gap was
the *other half* of `ci.yml`, which does **two passes** on `windows-latest`:

| CI step | seeded `check` |
|---|---|
| `cargo fmt --all --check` | ✗ — seed omits `--all` |
| `cargo clippy --all-targets -- -D warnings` | ✓ |
| `cargo build --all-targets` | ✗ |
| `cargo test` | ✓ |
| `cargo clippy -p kvscf-local --all-targets -- -D warnings` | ✗ |
| `cargo build -p kvscf-local` | ✗ |
| `cargo run -q -p kvscf-local -- --build-info` asserting `remote=false` | ✗ |

That second pass is not redundancy. `kvscf-local` is excluded from `default-members` because feature
unification in a whole-workspace build would turn `remote` **on** for the shared `kvscf-app` — so a
gate that stops after the seeded three lines stays green while the no-comms path rots. The same
"a gate that passes by not looking" failure as a bare `clippy` without `--all-targets`, one level
down. And `cargo fmt --check` without `--all` formats one package of four.

So `check` became an aggregator over two named recipes that mirror the workflow one-for-one:

```
check: check-default check-local
```

`ci.yml` stays authoritative; the justfile is the local mirror. Change one, change the other.

## Decisions

- **Split into `check-default` / `check-local`** rather than one flat list, because the two passes
  mean different things and are worth running separately while working on one of them.
- **Kept `sh` as `just`'s shell.** A pristine Windows PATH has no `sh` — Git ships it in
  `usr/bin`, which is only on PATH inside Git Bash, so `just check` from a bare PowerShell fails
  with *"could not find the shell `sh`"*. Fixing that here would mean `set windows-shell` and a
  justfile that diverges from every other kproject repo, and PowerShell's exit-code propagation
  through `-Command` is exactly the kind of thing that produces a gate that lies. It is a kprojects
  question; noted in the roadmap, not forked here.
- **Left the workspace and its exclusion alone**, per the WI. The exclusion is load-bearing.
- `PLAN.md` left in place (above).

## Verification

`rustup update` first — the workflow floats on `dtolnay/rust-toolchain@stable`; already on
rustc 1.97.1, unchanged. Then `just check` end to end: **exit 0**, 22 + 45 tests, and
`kvscf-local (remote=false)` printed by the assertion.

The assertion was also checked in the direction that matters — inverted to grep for `remote=true`,
it exits 1 with the failure message. A `grep -q` assertion that has only ever been seen passing is
an assertion nobody has tested.

## Follow-ups

- `just` / `sh` on a bare Windows PATH → kprojects (roadmap, Later).
- The installer appended `target/` to a `.gitignore` that already had `/target`. Harmless duplicate,
  left alone rather than hand-edited so re-running the installer stays a no-op.
