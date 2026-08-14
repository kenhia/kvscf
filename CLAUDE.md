<!-- kproject:begin — managed by kprojects; do not edit inside this block -->
## kproject conventions

This project uses the kproject minimal harness
(<https://github.com/kenhia/kprojects>). Keep context small; prefer doing
over ceremony.

### Layout

- `sprints/` — the project's evolution, one record per PR-sized unit of
  work (a "sprint")
  - `planning/` — planning docs; at minimum `roadmap.md` (the general plan)
  - `review/` — more formal reviews as the project matures
  - sprint records: `###-<short-name>.md` for small projects, or a
    `###-<short-name>/` directory of files for larger/more formal ones
  - a sprint record is one informal narrative: goal, decisions, what
    shipped, follow-ups — written during the sprint, not after
- `docs/` — project documentation, architecture, usage
- `.scratch/` — git-ignored scratch space for user or agent ephemera;
  use it instead of /tmp
- `justfile` — dev recipes; default recipe is `@just --list`; `just check`
  runs the CI gates; `just deploy` (or variants) if the project deploys
- `.env` — git-ignored; tokens and environment vars

### Workflow

- One sprint ≈ one PR. Sprint proposals and work items are managed in
  `korg`; durable cross-project knowledge goes in `klams`.
- If the korg or klams MCP tools are unavailable in your session, say so
  up front — don't silently work around missing infrastructure.
- TDD preferred: write the failing test first when practical.

### Tooling preferences

- Rust managed by `cargo`; format with `cargo fmt`, lint with
  `cargo clippy --all-targets` (test targets included deliberately — a gate
  that skips them is a gate that lies)
- Mirror `rust-toolchain.toml`, `rustfmt.toml` and `clippy.toml` from a
  sibling homelab repo rather than generating them
- License is MIT unless specifically directed otherwise
<!-- kproject:end -->

## Project

**kvscf — Ken's VS Code Focuser.** A Windows-only Rust workspace that enumerates every open
VS Code / VS Code Insiders window, lists them as `workspace (host)`, and foregrounds the one you
pick. The full build also publishes that list to a Redis feed a
[kdeskdash](https://github.com/kenhia/kdeskdash) touch panel renders, and takes taps back as focus
commands. Public repo; shipped through sprint 018 and in daily use on cleo.

### Build, run, test

`just check` is the gate and it is a **faithful mirror of `.github/workflows/ci.yml`** — two passes,
not one. Change one and change the other in the same commit.

```
just check          # both passes (what CI runs)
just check-default  # pass 1: default members, remote ON
just check-local    # pass 2: kvscf-local alone, remote OFF, + the --build-info assertion
just fmt            # cargo fmt --all
```

`just` shells out through `sh`, which on this machine lives only in Git Bash's `usr/bin` — run it
from Git Bash, not a bare PowerShell. Note also that plain `bash` on PATH here is the **WSL**
launcher, and `python` is the MSIX Store stub.

Run the app with `cargo run -p kvscf` (full) or `cargo run -p kvscf-local` (no comms);
`cargo run -p kvscf-core --bin kvscf-core -- list|focus <hwnd>` is the headless CLI.

### Read these first

- `docs/architecture.md` — crates, core mechanics, the remote contract
- `PLAN.md` — the original design rationale (§4 title parsing, §5 the foreground gotcha)
- `crates/kvscf-core/src/parse.rs` — title → `Instance`, the part most likely to need changing
- `sprints/` — one dir per sprint, `NNN-slug/README.md`; `sprints/planning/roadmap.md` is the plan
- `docs/kdeskdash-vscode-mode.md` — the wire contract with the Pi panel
- `.claude/skills/` — two project skills: `kvscf-add-app`, `kvscf-window-title`

### Gotchas

- **`kvscf-local` must be built in isolation** (`-p kvscf-local`). It is excluded from
  `default-members` on purpose: a whole-workspace build unifies Cargo features and would switch the
  `remote` feature back on for the shared `kvscf-app`. Do not "tidy" that exclusion away — it is
  load-bearing, and `--build-info` printing `remote=false` is what proves the artifact is comms-free.
- **Config lives in the registry** under `HKCU\Software\kenhia\kvscf` (`.env` is the fallback).
  Always reach it via `RegOpenCurrentUser` — see `crates/kvscf-app/src/userreg.rs`; a process started
  early in the boot binds `HKEY_CURRENT_USER` to the empty `HKU\.DEFAULT` hive permanently.
- **Title parsing needs a folder-first `window.title`** (`${rootName}` as the first ` - ` piece).
  The default VS Code title puts the active file first and reads as the workspace name.
- **Focus needs the `AttachThreadInput` recipe**, and never un-maximizes a maximized window (WI #465).
- Branches are `sprint/NNN-slug`, matching the sprint dir name.

