# Sprint 008 — Code favorites (WI TBD)

Status: **proposed / design.** Mark VS Code sessions as **favorites**; a favorite that isn't
currently open shows **dimmed** and **relaunches on click** — the Apps-tab pattern
(focus-if-running-else-launch) applied to VS Code, with the "launch" being the Update-Assist relaunch.

## Motivation

VS Code (both editions) is RAM-heavy per instance and Ken is on 32 GB (RAM prices have stalled the
upgrade he'd like). He wants to **close sessions he isn't actively using** without losing them from his
workflow — favorites make a closed session one click from being back. Favorite it → close it → it
parks in the dimmed section, ready to reopen. Nothing is lost, and the RAM comes back.

## Concept — reuse, don't reinvent

Three pieces we already have carry almost all of this:
1. **Apps tab (sprint 007):** running/dimmed rows + focus-if-running-else-launch UI, and the
   "no-HWND row, kvscf figures out focus-vs-launch" remote command trick.
2. **winset (WI #469/#470):** resolve an open window → its folder URI, and relaunch via
   `code --folder-uri` (locals and ssh remotes — Update Assist already relaunches remotes this way).
3. **Edge tab (WI #474):** the named/separator/unnamed two-section list layout.

## What a favorite is

A favorite = a **`winset::SetEntry`** — `{app, folder-uri, label}` — exactly what
`winset::resolve_open_set()` already produces and `save_set`/`load_set` already persist. Store the set
as `%APPDATA%\kvscf\favorites.json` (same shape as the named sets), add/remove one entry at a time.

## UI (Code tab)

- **Running windows:** unchanged — normal rows, top, click to focus.
- **Favorites not currently open:** **dimmed**, below a **separator** (same visual as Apps
  running/not-running and the Edge named/unnamed split). Ken considered a bottom-growing-up layout and
  rejected it — not worth the resize edge-cases. Separator it is.
- **Left-click a dimmed favorite → `winset::launch(entry)`** (reuses the relaunch; remote `ssh kai`
  windows relaunch exactly as Update Assist already does).
- **Right-click** (egui `response.context_menu`):
  - running row → **"★ Mark as favorite"** / **"☆ Unfavorite"** (toggle by whether it's already one)
  - running favorite → **"Close (keep favorite)"** — closes the window (frees the RAM) and it drops
    straight to the dimmed section. This is the move that closes the loop on the RAM goal.
  - dimmed row → **"☆ Unfavorite"** (removes it from the list)

## Matching favorite ↔ running window

Match by **folder URI** for precision (not workspace basename — two folders both named `src` on the
same host would false-match). Don't re-resolve every 1 s: **cache URI per HWND** (a window's URI never
changes; resolve each new window once). The dimmed list = favorites whose URI isn't among the open
windows.

## Edge cases / decisions

- **Unresolvable windows:** `resolve_open_set` has an "unresolved" bucket (untitled, odd multi-root).
  "Mark as favorite" is disabled/greyed for those — we can't relaunch what we can't resolve to a URI.
- **Just-launched gap:** a relaunched favorite takes a few seconds to appear; it stays dimmed until the
  next scan sees its window (same as the Apps tab launch). Acceptable.
- **Remote relaunch** reuses the Update-Assist path (reachable host + `code` connects). No new risk.
- **Label:** stored at favorite-time (`workspace (host)`); stable enough for display.

## Remote / kdeskdash contract (docs §5)

kdeskdash orders VS Code rows alphabetically by label. To render favorites it needs two things:
**ordering** (dimmed favorites after running) and **dimming**. So:

- Add `running: bool` and `favorite: bool` to each instance row in `kvscf:instances:<host>`.
- Publish **not-running favorites as extra `running:false` rows**, with a **synthetic `id` = the
  folder-URI** (a string that can't parse as an HWND int) plus `label`, `favorite:true`.
- **Command stays uniform:** kdeskdash sends the normal `{token, id}` for whatever row was tapped.
  kvscf routes it: `id` parses to a live HWND → focus (today's path); `id` doesn't resolve to a live
  window → look it up in favorites → **relaunch**. Only kvscf knows the difference — kdeskdash needs no
  special-casing beyond sort-running-first + dim-where-`running:false`.

This mirrors the Apps `{app:<key>}` split, just carried in the shared `id` field. Document it as §5 in
[docs/kdeskdash-vscode-mode.md](../../docs/kdeskdash-vscode-mode.md).

## Plan

1. **Favorites store + toggle** — `favorites.json` (reuse winset save/load); add/remove one entry;
   "is this favorite open?" via the URI-per-HWND cache. *(local-only, shippable)*
2. **Dimmed-section UI + context menus** — separator + dimmed favorite rows; right-click
   mark/unmark; left-click dimmed → relaunch. *(local-only, shippable)*
3. **"Close (keep favorite)"** — right-click a running favorite → `close_window` (already exists from
   Update Assist), stays in the list, drops to dimmed. *(local-only, shippable)*
4. **Remote + kdeskdash** — add `running`/`favorite` fields; publish not-running favorites; route
   non-HWND `id` → relaunch; write docs §5. kdeskdash rendering side is a **follow-up in the
   kdeskdash project** (like the Apps §4 handoff — documented here, built there).

Steps 1–3 are local-only and each independently shippable; step 4 mirrors the Apps remote work.

## Open questions

- kdeskdash dims via the `running:false` flag, or does it want a dedicated `dimmed` field? (Prefer
  reusing `running` — one fewer field.)
- Do favorites and named window-sets (WI #469) eventually merge? For now keep separate; favorites are a
  living per-entry list, sets are named snapshots.
