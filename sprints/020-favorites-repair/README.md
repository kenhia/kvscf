# Sprint 020 — favorites: right folder, and a way to fix it

korg kvscf **#1682** (bug) and **#1683** (feature), proposal **korg:1684**.

Ken: *"Launching the 'klams' favorite was launching the /ai/klams. Unfavoriting that 'klams' entry
then refavoriting with only the ~/src/ai/klams open does not fix it."* And the guess that came with
it — *"there is a lookup by the 'displayed' word so two identical ones ends up in this state"* — was
right, including the part that made it unfixable.

## The bug

A VS Code window title carries the workspace's **leaf folder name** and nothing else. To relaunch a
favorite, kvscf has to recover the full folder URI, which it does by scanning VS Code's own
`workspaceStorage` for a folder with that leaf name on that host. Ken has two:

| folder on kubs0 | what it is |
|---|---|
| `/home/ken/src/ai/klams` | the klams repo |
| `/ai/klams` | klams's **runtime data** — `config/`, `data/` |

Both real, both legitimately opened, both leaf-named `klams`. So the filter matches two, and a
tie-break decides. The tie-break was:

```rust
.max_by_key(|u| u.mtime)   // mtime of workspace.json
```

`workspace.json` is written **once**, when VS Code first creates the storage folder, and never
touched again. Its mtime means *first opened*, not *last used*. On cleo:

| storage folder | `workspace.json` | `state.vscdb` |
|---|---|---|
| `~/src/ai/klams` | 2026-05-16 | 2026-08-27 17:51:27 |
| `/ai/klams` | **2026-05-24** ← won | 2026-08-27 17:51:07 |

Ken happened to open the data dir eight days after the repo, in May. That ordering is frozen
forever, so the open `klams (kubs0)` window resolved to `/ai/klams` every time — and re-favoriting
re-ran the same resolution and stored the same wrong URI, which is why unfavorite/refavorite could
never fix it. Nothing was corrupt; the answer was stably wrong.

## The fix

Tie-break on `state.vscdb`'s mtime instead, falling back to `workspace.json` when it's absent (a
folder recorded but never opened). `state.vscdb` is rewritten as a window is *used*, so it orders
folders by recency of use rather than of creation.

Two functions came out of `resolve_open_set` to make the decision testable rather than reachable
only through `%APPDATA%`: `last_used(storage_dir)` and `best_match(uris, workspace, host)`.

**One claim corrected mid-sprint.** The first draft of the comment said `state.vscdb` is written
*continuously while the window lives*. Measured, that's false: both klams windows were open and
both files had sat unflushed for 129 minutes, 20 seconds apart. It is flushed on use, not on a
heartbeat. That's still enough — the comment now says what was actually observed. A doc comment
asserting a cadence nobody measured is how the next person inherits this same bug.

**Known limit, deliberately not solved.** With both same-named windows open at once — which is
Ken's state right now — the two rows collapse to whichever flushed last. `--dump-set` shows it
honestly:

```
klams (kubs0)   Insiders   vscode-remote://ssh-remote%2Bkubs0/home/ken/src/ai/klams
klams (kubs0)   Insiders   vscode-remote://ssh-remote%2Bkubs0/home/ken/src/ai/klams
```

Nothing in a window title separates them, so fixing that needs an identity the window doesn't
carry. What it costs is bounded: a favorite made from the wrong one of the pair — repairable by
hand, which is the other half of this sprint.

## The favorite editor

A favorite is a *resolved* thing: starring a window stores what kvscf worked out, not anything Ken
typed. When the resolution was wrong the entry was also opaque **and** unfixable from inside the
app — the only recourse was hand-editing `%APPDATA%\kvscf\favorites.json`. So `favedit.rs` is a
debugging surface first and an editor second.

- Its own viewport window, like the Launcher editor (`editor.rs`), for the same reason: a 280 px
  rail, often docked and borderless, cannot hold a form with a full URI in it.
- Shows the **decoded** URI beside the stored one. `ssh-remote%2Bkubs0/ai/klams` versus
  `ssh-remote%2Bkubs0/home/ken/src/ai/klams` is the comparison you actually have to make, and
  percent-encoding is exactly the wrong font for making it.
- Shows the derived **workspace** and **host** live, because those — not the label — are what
  matching, the dimmed list and relaunch run on.
- Edits label, build and URI; Save replaces **in place** so a repaired favorite keeps its position
  in the rail. Retargeting onto a folder another favorite already owns is refused (two entries
  matching one window means one is permanently dimmed and neither looks wrong).
- An unparseable URI warns but still saves — `read_entries` already tolerates one, and an escape
  hatch that refuses odd input isn't one.

Entry points: **✎ Edit favorite…** on a favorite's right-click menu (both the starred running row
and the dimmed not-open row), plus a **Favorite editor…** button in the Controls drawer. It does
not create favorites; starring a window is still how one comes into being.

## Verification

- `just check` — both passes, exit 0, 60 tests.
- The regression the bug deserved: `duplicate_leaf_names_resolve_to_the_recently_used_one` fails on
  the old tie-break and passes on the new one; `last_used_reads_state_vscdb_not_workspace_json` is
  built on the exact shape that caused it (workspace.json newer, state.vscdb older).
- `saving_a_repair_replaces_the_entry_in_place` — the end of the #1682 story, asserted with
  `same_target` rather than a suffix match, because the correct URI *also* ends `/ai/klams`. The
  first draft of that assertion got it wrong, which is the bug in miniature.
- Both editor windows draw headlessly for two frames without panicking — this one opens over
  whatever Ken is doing and a panic in it would take the rail down.
- **Live on cleo:** `--dump-set` against the real `workspaceStorage` now resolves `klams (kubs0)`
  to `~/src/ai/klams`. Release build launched and ran clean.

Not verified by clicking: the GUI wiring (context menu → editor → Save). `computer-use` can't
resolve `kvscf` — it has no Start-menu entry, so a dev build is unreachable to it. The headless
tests drive the real widget tree, but the menu item itself is eyeball-only.

## Follow-ups

- **The poisoned entry is still in `favorites.json`** (`klams (kubs0)` → `ssh-remote%2Bkubs0/ai/klams`).
  Left for Ken to fix with the new editor rather than hand-edited behind his back — repairing it is
  the tool's first real job. Backup at `.scratch/favorites.backup.json`.
- Proposal korg:1684 suggested deleting the `/ai/klams` workspaceStorage dir as optional cleanup.
  **Don't** — the folder exists and is klams's live runtime data, not a stale leftover. That was a
  guess written before checking kubs0.
- Deploying: kvscf has no `just deploy`; the installed binary is `C:\tools\bin\kvscf.exe` and gets
  a manual copy.
