# Sprint 007 — Apps tab (WI TBD)

Status: **research done, feasibility proven; building.** A third tab of arbitrary apps Ken switches to
a lot — focus if running, **launch if not** — configured via registry, populated by an agent skill.

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
2. `kvscf-core`: `find_app_window(matcher)` + `launch_app(spec)` (exe / shell-AUMID).
3. App config load from registry → `AppEntry { key, label, running, hwnd?, launch }`.
4. Apps tab UI (running normal + focus; not-running dimmed + launch).
5. The `kvscf-add-app` skill; add Claude/Copilot/Everything/Battle.net/Terminal/Kindle.
6. Remote: publish `kvscf:apps:<host>` `{key,label,running,hwnd?}`; command `{token, app:<key>}` →
   focus-if-running-else-launch (subscriber routes numeric `id`→HWND, string `app`→launch/focus).
   Handoff to kdeskdash (greys out non-running).
