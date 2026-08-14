# Roadmap

> The general plan for this project. Keep it current; detail lives in the
> sprint records. korg (project `kvscf`) is the authoritative queue — this file
> is the readable shape of it.

## Now

- **019 — the kprojects harness** (korg #1238). Agent instructions, `sprints/planning/`,
  and a `just check` that mirrors both CI passes instead of only the first.

## Next

- **Extension development support** (korg #627, S). Handle VS Code extension-development
  windows, which today parse as an ordinary workspace.
- **First live AUTH run.** Sprint 018 gave the publisher `KVSCF_REDIS_PASSWORD`, but nothing has
  presented it yet — rpidash3's Redis gets its `requirepass` in slice 5 of korg program 1143, on
  the kdeskdash side (#1137). Expect the first real end-to-end check there, not here.

## Later / Ideas

- Revisit `just` needing Git Bash's `sh` on Windows. A kprojects-wide question (every seeded
  justfile has it), not a kvscf one — raise it there rather than forking this repo's shell.
- Remaining threads from `sprints/review/2026-07-20-review.md` that were filed but not taken.
