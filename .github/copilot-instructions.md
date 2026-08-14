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

**kvscf — Ken's VS Code Focuser.** Windows-only Rust workspace: enumerate the open VS Code /
Insiders windows, list them as `workspace (host)`, foreground the one you pick. The full build also
publishes the list to a Redis feed rendered by a kdeskdash touch panel and takes taps back as focus
commands. Public repo, shipped through sprint 018.

### Build, run, test

`just check` is the gate and mirrors `.github/workflows/ci.yml` — **two passes**; change one and
change the other in the same commit. `just check-default` is pass 1 (default members, remote on),
`just check-local` is pass 2 (`kvscf-local` alone, remote off, plus the `--build-info` assertion).
`just` needs Git Bash's `sh`; plain `bash` on this machine is the WSL launcher and `python` is the
MSIX stub. Run with `cargo run -p kvscf` / `-p kvscf-local`; headless CLI is
`cargo run -p kvscf-core --bin kvscf-core -- list`.

### Read these first

`docs/architecture.md` (crates + mechanics), `PLAN.md` (original design rationale),
`crates/kvscf-core/src/parse.rs` (title parsing), `sprints/` (`NNN-slug/README.md` per sprint,
plan in `sprints/planning/roadmap.md`), `docs/kdeskdash-vscode-mode.md` (wire contract).

### Gotchas

- **Build `kvscf-local` in isolation** (`-p kvscf-local`). It is excluded from `default-members`
  deliberately — a whole-workspace build unifies features and turns `remote` back on for the shared
  `kvscf-app`. Don't remove the exclusion; `--build-info` → `remote=false` is the proof.
- **Config is in `HKCU\Software\kenhia\kvscf`** (`.env` is the fallback), reached via
  `RegOpenCurrentUser` (`crates/kvscf-app/src/userreg.rs`) — an early-boot process otherwise binds
  `HKEY_CURRENT_USER` to the empty `HKU\.DEFAULT` hive for good.
- **Title parsing needs a folder-first `window.title`** (`${rootName}` first); the VS Code default
  puts the active file first and misreads as the workspace.
- **Focus uses the `AttachThreadInput` recipe** and never un-maximizes a maximized window.
- Branches are `sprint/NNN-slug`, matching the sprint dir name.

