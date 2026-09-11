# Sprint 021 — dev-host windows in red, and favorites that come to the front

korg kvscf **#627** (task, S) and **#1311** (task, S), proposal **korg:2224** — slice 13 of the
backlog-drain push, program **korg:2233**. Ken oversaw this one directly.

Two unrelated S items that happen to share a theme: both are places where kvscf told the truth about
a window and then did nothing useful with it.

## #627 — the Extension Development Host reads as a workspace

Ken: *"When debugging extension, the window title is set to `[Extension Development Host] Visual
Studio Code...`. Let's add detection of that and color the entry red."*

He was writing this while building **korg-vs**, his first VS Code extension, so the dev host is now a
window he sees daily.

### Where the string comes from

Rather than guess the prefix, it came out of VS Code's own shipped nls bundle:

```
out/nls.metadata.json → vs/workbench/browser/parts/titlebar/windowTitle
  devExtensionWindowTitlePrefix = "[Extension Development Host]"
  userIsAdmin                   = "[Administrator]"
  userIsSudo                    = "[Superuser]"
```

Two things worth having from that:

- **The square brackets are part of the localized string**, not punctuation VS Code adds around it.
  So the constant is the whole `[Extension Development Host]`, and a localized VS Code will not
  match it — documented, with the one-line fix, in `docs/window-title-parsing.md`.
- **There are three decorations, not one**, and they are not in the same place. The dev host is a
  **prefix**; admin/superuser are **suffixes**. Only the prefix needed code: a suffix lands *after*
  the app name, so the existing `rfind(" - Visual Studio Code")` cut already removed it. Three tests
  now pin that down, including a dev host running elevated, which carries both at once.

### What it actually looked like

Captured live, by opening a dev host on korg-vs (`code-insiders --extensionDevelopmentPath`) and
dumping raw titles with the core CLI:

```
[Extension Development Host] Visual Studio Code - Insiders
```

**No folder name at all** — which is what F5 from an extension repo gives you, and exactly the shape
Ken pasted; his trailing `...` was `- Insiders`. That matters, because with no `rootName` there is no
`" - Visual Studio Code"` for the app-name cut to bite on. The same dev-host window, before and
after, through `kvscf-core -- list`:

| | workspace | active file |
|---|---|---|
| main | `[Extension Development Host] Visual Studio Code` | `Insiders` |
| this branch | `Extension Development Host` | — |

The "before" row is not reconstructed: it is main built in a throwaway worktree and run against the
same live window, while it was still open.

### The fix

`parse_title` gains two steps ahead of the app-name cut — strip the prefix, and if what remains is
*exactly* an app name, the window has no folder open. `ParsedTitle` and `Instance` carry an
`ext_dev_host` flag; `rows::code_row` paints those rows red (`Palette::ext_dev`, both modes) instead
of their build color, and adds a line to the hover tip.

**A judgment call for Ken, easily reversed:** a folderless dev host is *labelled*
`Extension Development Host` rather than left to parse as `Visual Studio Code`. Only the red would
have satisfied the WI as written, but a red row reading "Visual Studio Code" seemed worse than no
change. If you would rather see something else there, it is one constant (`EXT_DEV_LABEL`).

**Deliberately not changed:** an *ordinary* folderless window still parses to workspace
`Visual Studio Code`, as it always has. It is the same latent wart, but dropping those rows or
relabelling them changes what the rail shows for windows that have nothing to do with #627. Pinned
by a characterization test so the asymmetry is a decision on the record rather than an oversight —
see follow-ups.

## #1311 — a relaunched favorite opens behind you

Ken: *"thinking this is a timing issue and the 'fix' may be worse than the current symptom, but
let's investigate it and see if we can make it work. When I launch a closed favorited VS Code
[Insiders], it launches, but does not become foreground/focused."*

It was not a timing subtlety. The two launch paths sat four lines apart in `App::apply_actions`:

```rust
if let Some((spec, matcher)) = actions.launch {
    launch_and_focus(&spec, &matcher);     // Apps tab — polls, then foregrounds
}
if let Some(entry) = actions.fav_launch {
    let _ = winset::launch(&entry);        // favorites — fire and forget
}
```

The Apps tab has polled-and-focused since **sprint 007**, and `kvscf-core/src/app.rs` carries the
same symptom in its own comment — *"a launched app doesn't reliably come to the front on its own
(Kindle opened behind another window in testing)"*. Favorites never got it.

### The fix, and Ken's worry

`winset::launch_and_focus` launches on a background thread, then polls for the window and calls the
sprint-001 focus recipe. Used from both the rail click and a dashboard tap (`launch_favorite`).

Ken's instinct that the fix could be worse than the symptom is the right worry — a focus steal that
lands seconds late, after he has moved on, is worse than no focus. Two things bound it:

- **Only a window that was not open before the launch is ever focused.** The hwnds are snapshotted
  before spawning, so a late poll cannot grab something Ken switched to himself.
- **The poll stops at the first match**, so it focuses once and never again.

Matching is on build + workspace + host, not the folder URI — recovering a URI means re-reading
every `workspace.json` (see `resolve_open_set`), which is far too expensive at 500ms. That is safe
*because* of the snapshot: the candidate set is already only new windows. Six unit tests cover what
is eligible, including the two near-misses that matter — the same folder in the other build, and the
same folder name on another host (Ken has `klams` on two hosts; see sprint 020).

The poll runs 60 × 500ms = 30s, longer than the Apps tab's 20s: VS Code's cold start is slower than
a typical exe, and a **remote** favorite only takes its final title once the SSH connection is up.
Ten of Ken's eleven favorites are remote.

**`relaunch` (set restore) deliberately still does not focus.** Restoring a set opens several
windows, and foregrounding each in turn is a fight whose winner is whichever finished starting last.
A set restore has no single window the user asked for.

### Verified live

`a_relaunched_favorite_comes_to_the_front` is an `#[ignore]`d test in the repo's existing idiom (like
sprint 017's registry round-trip): it takes a folder-uri from the environment, so it is not tied to
one machine, and closes the window it opened even on failure.

```sh
KVSCF_FOCUS_TEST_URI=file:///d%3A/ClaudeWorks/korg-vs \
  cargo test -p kvscf-app -- --ignored --nocapture comes_to_the_front
```

Run against korg-vs: window appeared, came to the front, passed.

**What that does not prove**, recorded in the test too: a new VS Code window *sometimes* comes to the
front on its own — unreliably, which is the whole of #1311 — so a pass means the path works, not that
the focus call was load-bearing on that run. Making it decisive would mean forcing the window to open
behind, which is the one bit of timing nobody controls. The unit tests cover the part that is
deterministic.

## Gate

`just check` green — both passes, on **stable 1.98.1**. Worth noting: the local toolchain was 1.97.1
(2026-07-14) and stable had moved on 2026-09-01, so `rustup update` came first. CI pins
`dtolnay/rust-toolchain@stable`, which floats, so the old toolchain would have been a green local run
and a clippy surprise in CI.

A baseline `just check` was run on 1.98.1 *before* any edit, so new lints could not be mistaken for
this sprint's.

One small correction on the record: the proposal's notes said the Linux-side checks go through the
`build-clones` skill on kai. They did not, and should not — `.github/workflows/ci.yml` runs on
`windows-latest`, and nearly all of this crate is `#[cfg(windows)]`. Running it on kai would have
exercised the non-Windows stubs. cleo *is* the CI platform; the real gate ran here.

## Cross-slice state (korg:2230, krot, same host)

- **kvscf was not restarted — zero times.** The running instance is still pid 8448, started
  2026-09-10 23:36:50, which is slice korg:2231's restart, not this sprint's. The fix therefore is
  **not live** on cleo; `C:\tools\bin\kvscf.exe` is untouched and this repo has no `just deploy`.
- **`HKCU\Software\kenhia\kvscf` was not written.** Confirmed after the work that
  `KVSCF_REDIS_PASSWORD` is still there, by value *name* only, from an `ssh cleo` session rather than
  from inside the MSIX-packaged app.
- **`userreg.rs` was not touched** — as the cross-slice notice predicted, neither item needed it.

## Follow-ups

- **Ordinary folderless windows** still read as workspace `Visual Studio Code` (characterization
  test `an_ordinary_folderless_window_keeps_its_historical_parse`). Decide whether those should be
  relabelled or dropped from the rail.
- **The dashboard does not learn about dev hosts.** `ext_dev_host` is not on the
  `kvscf:instances:<host>` wire, so a dev host renders on the kdeskdash panel as an ordinary window.
  Adding it is a kdeskdash-side change as well as a wire-contract change, which is its own slice, not
  a quiet addition here.
- **Stable VS Code on cleo has no folder-first `window.title`** — its window reads `settings.json`
  where the project name should be (visible in the CLI dumps above). That is the documented Path-1
  setup step from the `kvscf-window-title` skill, not a bug, but it is live on Ken's machine.
- **The roadmap's "First live AUTH run" item is overtaken.** It expected the first real
  `KVSCF_REDIS_PASSWORD` run on rpidash3 via korg program 1143; slice korg:2231 did it on **rpidash2**
  on 2026-09-10. Whether that run actually authenticated was not checked here — it needs the Redis
  side, which belongs to the krot/kdeskdash slices, not this one.
