---
name: kvscf-window-title
description: Make kvscf's Code tab show the right project/host by getting the VS Code window title into a folder-first shape. Use when the Code tab shows filenames instead of project names, or when someone asks to set up / configure kvscf for their VS Code, or to adapt kvscf's title parser to a custom window.title.
---

# Set up kvscf's Code-tab window titles

kvscf reads each VS Code window's **workspace** and **remote host** from its **title bar text** — it
needs the **folder name first** in the title. VS Code's default `window.title` puts the active file
first, so kvscf shows filenames where the project name should be. There are two fixes; pick based on
what the user wants.

## Path 1 (default, easiest): set a folder-first `window.title`

Best when the user is fine changing their title bar. Add to their VS Code user `settings.json` (and to
**both** Stable and Insiders if they use both editions):

```jsonc
"window.title": "${dirty}${separator}${rootName}${separator}${activeEditorShort}${separator}${appName}${separator}${profileName}"
```

The only hard requirement is that **`${rootName}` is the first ` - `-separated piece**. Minimal forms
that also work: `${rootName}${separator}…`, or just `${rootName}`. The remote `[SSH: host]` tag is part
of `${rootName}` automatically; `${appName}` is optional (keeps the active-file label tidy). The VS
Code default (active file first) is exactly what does **not** work.

Verify: `cargo run -p kvscf-core --bin kvscf-core -- list` should show project names (and hosts), not
filenames.

## Path 2: keep their title, adapt the parser

Best when the user would rather not change `window.title`. The parser is one small, pure function
(`parse_title` in `crates/kvscf-core/src/parse.rs`) with unit tests — no UI or Windows APIs. Follow
**[docs/window-title-parsing.md](../../docs/window-title-parsing.md)**: capture real titles with the
CLI, add tests for the user's layout, adjust `parse_title` (separator / order / remote indicator), then
`cargo test -p kvscf-core` and check `… -- list` against live windows.

## Don't

- Don't touch the Edge parser or anything outside `parse.rs` — they're independent of `window.title`.
- Don't add settings-file reading or a regex engine; the project deliberately chose docs + this helper
  over those (see WI #491).
