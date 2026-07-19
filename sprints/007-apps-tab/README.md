# Sprint 007 — Apps tab (WI TBD)

Status: **shipped.** A third tab of arbitrary apps Ken switches to a lot — focus if running,
**launch if not** — configured via registry, populated by an agent skill. All six apps seeded and
verified running.

## Twists (vs. Code/Edge tabs)

- Apps are **configured**, not auto-discovered. Each configured app shows whether or not it's running.
- **Launch-if-not-running:** clicking a non-running app launches it (foreground follows on launch). On
  the **dashboard**, non-running apps render **greyed out**; a tap launches them.
- An app is identified by a **key**, not an HWND (a non-running app has none).

## Research findings (scouted live, 2026-07-18)

Detection is by **process image** (`QueryFullProcessImageNameW` basename) and/or **window class** —
never title alone (an Edge window is titled "Claude"; VS Code windows say "…Copilot…"). Multi-window
apps: take the first (optionally a title filter).

| App | detect | launch | notes |
|---|---|---|---|
| Claude Desktop | image `claude.exe` | AUMID `Claude_pzs8sxrjxfjjc!Claude` | Store (versioned WindowsApps path) — use AUMID |
| Copilot | image `mscopilot.exe` | AUMID `Microsoft.Copilot_8wekyb3d8bbwe!App` | single window |
| Everything | class `EVERYTHING` | exe `C:\Program Files\Everything\Everything.exe` | tray/elevated → `MainWindowHandle`=0, image path may be blocked; **match by class** |
| Battle.net | image `Battle.net.exe` | exe `…\Battle.net\Battle.net.exe` | `ProcessName` is a random `temp_…`; path basename is `Battle.net.exe`. 2 windows (main + "Friends") |
| Terminal | image `WindowsTerminal.exe` | AUMID `Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe!App` (or `wt.exe`) | 3 windows → first |
| Kindle | image `Kindle.exe` | exe `…\Local\Amazon\Kindle\application\Kindle.exe` | class `Qt5QWindowIcon`; well-behaved; cold-launch verified |

**Two launch flavors:** direct **exe** (Win32) and **`explorer.exe shell:AppsFolder\<AUMID>`** (Store —
their versioned paths change on update). AUMIDs come from `Get-StartApps`.

**Feasibility proven live:** focused Everything (arbitrary app) via `kvscf-core::focus`; cold-launched
Kindle (not running → visible window) via its exe path.

## Config schema (registry — no `.env` fallback)

`HKCU\Software\kenhia\kvscf\apps\<key>` (subkey per app), values:
- `label` — display name.
- `process` — image basename to match (optional if `class` is set).
- `class` — window class to match (optional; for elevated/odd apps like Everything).
- `match` — optional title substring to disambiguate multi-window (e.g. exclude "Friends").
- `launch_kind` — `exe` | `aumid`.
- `launch` — exe path, or the AUMID for `explorer shell:AppsFolder\<AUMID>`.
- `order` — optional sort index.

## The skill

`.claude/skills/kvscf-add-app/` — given an app name, an agent: finds a running window (image via
`QueryFullProcessImageNameW`, class via `GetClassName`), looks up the AUMID (`Get-StartApps`) or exe
path, **verifies** detect+launch, then writes the registry entry. Add apps one at a time, tweaking the
skill as odd apps (Battle.net, elevated apps) turn up.

## Plan

1. ✅ Prove feasibility (Everything focus + Kindle cold launch).
2. ✅ `kvscf-core`: `find_app_window(matcher)` + `launch_app(spec)` (exe / shell-AUMID), plus
   `resolve_apps(matchers)` (batch, single enum pass) and `list_windows()` (skill discovery).
3. ✅ App config load from registry → `AppEntry { key, label, matcher, launch, order, running, hwnd? }`
   (`crates/kvscf-app/src/apps.rs`), reloaded each ~1s refresh so skill-added apps appear live.
4. ✅ Apps tab UI (`Tab::Apps`): running → full-color ● + `focus_with`; not-running → dimmed ○ +
   `launch_and_focus`. Headless probe `kvscf.exe --dump-apps`.
5. ✅ The `kvscf-add-app` skill (`.claude/skills/kvscf-add-app/`) + CLI `kvscf-core windows [filter]`
   discovery command; all six added (claude/everything/copilot/terminal/battlenet/kindle) and
   verified running via `--dump-apps`.
6. ✅ Remote: publish `kvscf:apps:<host>` `{key,label,running,id?,order}`; command `{token, app:<key>}`
   → `apps::activate` (focus-if-running-else-launch). Subscriber routes `id`→HWND focus, `app`→app.
   kdeskdash contract documented in [docs/kdeskdash-vscode-mode.md](../../docs/kdeskdash-vscode-mode.md)
   §4. 3 unit tests lock the command/JSON contract.

## Verified matchers + launch (post-build, use these for the seed config / skill)

All confirmed live via `kvscf-core find …`:

| key | matcher | launch_kind | launch target |
|---|---|---|---|
| claude | `proc=claude.exe` | aumid | `Claude_pzs8sxrjxfjjc!Claude` |
| copilot | `proc=mscopilot.exe` | aumid | `Microsoft.Copilot_8wekyb3d8bbwe!App` |
| everything | `class=EVERYTHING` | exe | `C:\Program Files\Everything\Everything.exe` |
| battlenet | `class=Chrome_WidgetWin_0` + `title=Battle.net` | exe | `C:\Program Files (x86)\Battle.net\Battle.net.exe` |
| terminal | `proc=WindowsTerminal.exe` | aumid | `Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe!App` |
| kindle | `proc=Kindle.exe` | exe | `C:\Users\kenhi\AppData\Local\Amazon\Kindle\application\Kindle.exe` |

Note (verified): **Battle.net can't match by process** — its `QueryFullProcessImageNameW` basename is a
random `temp_…`; use class+title. **Everything** is tray/elevated (`MainWindowHandle`=0, image path may
be blocked) → class. **Apps don't auto-foreground on launch** → `launch_and_focus` launches then polls
~20s for the window and foregrounds it.

## Done (sprint complete)

Everything above shipped. Verification:
- `cargo test -p kvscf-core` (16) + `cargo test -p kvscf-app --features remote` (3) green; clippy clean
  on the remote, isolated-local, and core builds.
- `kvscf.exe --dump-apps` resolves all six configured apps to `running` with their HWNDs.
- Matchers each verified live via `kvscf-core find …`; launch specs verified (AUMIDs from
  `Get-StartApps`, exe paths exist).

**Not done / follow-ups:**
- No korg WI was ever filed for the Apps tab (the whole sprint ran WI-less). If we want it tracked,
  file one retroactively.
- kdeskdash side (rendering `kvscf:apps:*`, greying out non-running, sending `{app:<key>}`) is the
  other half — contract is in docs §4, not yet implemented on kdeskdash.
- Launch-spec *launch* paths weren't cold-tested this session (all six were already running); detection
  was fully verified. Cold-launch was proven in feasibility (Kindle) and reuses `launch_and_focus`.
