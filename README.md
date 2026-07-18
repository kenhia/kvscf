# kvscf — Ken's VS Code Focuser

A small Windows helper that scans for open **VS Code** / **VS Code Insiders** windows, shows them as a
sorted, labeled list, and brings the one you click to the foreground and focuses it.

Two faces of the same tool:

1. **Local app** — a lightweight Windows tray/list app you drive directly.
2. **Remote mode** — the same instance list pushed to the desk dashboard
   [`kdeskdash`](ssh://kai/~/src/tools/kdeskdash) as a `vscode` mode, with taps sent back to focus the
   chosen window.

See [PLAN.md](PLAN.md) for design and decisions, and `sprints/` for sprint-by-sprint progress.

Status: **sprint 001 core built** — `kvscf-core` enumerates, parses, and focuses VS Code windows
(11 tests, `windows-latest` CI). Sprints 002 (app) and 003 (remote) not started.

## Build / run

```sh
cargo build
cargo test
cargo run -p kvscf-core --bin kvscf-core -- list        # list open VS Code windows
cargo run -p kvscf-core --bin kvscf-core -- focus <hwnd> # foreground+focus one
```

## Sprints

| # | Name | Goal |
|---|------|------|
| [001](sprints/001-poc-scan-focus/README.md) | POC — scan & focus | Prove we can enumerate VS Code windows, parse titles into structured instances, and foreground+focus a chosen one. |
| [002](sprints/002-windows-app/README.md) | Windows app | Tall/thin left-docked "nav rail" list over the core; click-to-focus; live refresh. |
| [003](sprints/003-remote-kdeskdash/README.md) | Remote plumbing + kdeskdash handoff | Push the instance list to kdeskdash and accept select-to-focus back; handoff spec for the `vscode` mode. |
