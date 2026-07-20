# Sprint 011 — window.title docs + parser-adapt helper (WI #491)

Status: **built.** Docs-only. The Code-tab parser is tuned for a *folder-first* window title (the
author's `window.title`); anyone on VS Code's default (active-file first) would see filenames where the
project name belongs. Generalize via **documentation**, not code.

## Why docs, not code

Two directions considered and rejected:
- **Read both editions' `settings.json` and parse from the configured `window.title`** — too
  complicated (settings can live locally, per-workspace, and on remotes; expanding VS Code's title
  grammar to reverse it is a lot for little).
- **A user-configurable regex** — doable, but heavier than warranted for the payoff.

Chosen, "right for this tool in this era": a README pointer + a small **agent-followable** helper.
Users either set a folder-first `window.title` (easy) or have any coding agent — even a free tier —
adapt the tiny pure parser to their own title.

## Confirmed in code (so the README claims are exact)

Parser: `crates/kvscf-core/src/parse.rs::parse_title`. Author's actual setting (both editions'
`settings.json`):

```
${dirty}${separator}${rootName}${separator}${activeEditorShort}${separator}${appName}${separator}${profileName}
```

Hard requirement: **`${rootName}` is the first ` - `-separated segment.** Findings:
- The remote tag `[SSH: host]` is baked into `${rootName}` by VS Code in remote windows (live titles
  show `kvllm [SSH: kai]` as one segment) — no separate remote variable.
- `strip_leading_dirty` removes `●`/`•`/`*` **and** a leading `- `, so a leading `${dirty}${separator}`
  collapses away.
- `${appName}` is cut at ` - Visual Studio Code` only to keep the active-file label clean; not required
  for workspace + remote.

How-much-is-needed (verified against the parser):

| starts with | works |
|---|---|
| `${dirty}${separator}${rootName}${separator}…` | ✅ |
| `${rootName}${separator}…` | ✅ |
| `${rootName}` alone | ✅ (no active-file label) |
| VS Code default (`${activeEditorShort}` first) | ❌ (filename read as workspace) |

## Deliverables

- **README.md** — new "Code tab: window titles" section: why folder-first is needed, the recommended
  `window.title` (verbatim), the how-much table above, and a link to the adapt-the-parser helper.
- **docs/window-title-parsing.md** — agent-followable walkthrough for the customize path: capture real
  titles via the CLI, add tests first, adjust `parse_title` (separator / order / remote indicator),
  verify. Written for any agent/tier.
- **.claude/skills/kvscf-window-title/** — thin repo skill routing a Claude agent to the two paths
  (mirrors the existing `kvscf-add-app` skill).

No parser logic changed — `parse.rs` is untouched.

## Verification

Docs-only, but the full CI workflow was run locally anyway (fmt, clippy `--all-targets` default +
`kvscf-local`, build, `cargo test` — 16 core + 5 app still green) since a sprint ships through CI.
