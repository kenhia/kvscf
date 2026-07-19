---
name: kvscf-add-app
description: Add an app to the kvscf Apps tab — discover how to detect its window and how to launch it when it's not running, verify both live, then write the registry entry under HKCU\Software\kenhia\kvscf\apps. Use when Ken says "add <app> to kvscf apps", "kvscf add-app", or wants a new app on the Apps tab.
---

# Add an app to the kvscf Apps tab

The Apps tab (sprint 007) shows a configured set of apps Ken switches to a lot: **focus if running,
launch if not**. Each app is one registry subkey under `HKCU\Software\kenhia\kvscf\apps\<key>`. Your
job, given an app name: figure out **how to detect its window** and **how to launch it**, *verify both
live*, then write the entry. The running kvscf app reloads config every refresh (~1s), so a new entry
appears on the Apps tab without a restart.

Build the CLI once if needed: `cargo build --release -p kvscf-core` → `target/release/kvscf-core.exe`.
Below it's written as `kvscf-core`.

## The two things every entry needs

1. **A matcher** — how to recognize the app's *window* among all open windows. A window matches when
   every set field matches; **at least one of `process` / `class` must be set** (title alone is
   ambiguous — 7 open windows can be titled "Claude"; only one is Claude Desktop).
   - `process` — process image basename, e.g. `claude.exe` (case-insensitive).
   - `class` — exact window class, e.g. `EVERYTHING`. Use this when the process image is unavailable
     (elevated/tray apps) or the process name is random (see Battle.net).
   - `match` — optional title substring to disambiguate a multi-window app (e.g. exclude "Friends").
2. **A launch spec** — how to start it when not running:
   - `launch_kind = exe`, `launch = <full exe path>` — normal Win32 apps.
   - `launch_kind = aumid`, `launch = <AppUserModelID>` — Store/packaged apps, whose install paths
     are versioned (so a fixed exe path would break on update). kvscf launches these via
     `explorer.exe shell:AppsFolder\<AUMID>`.

## Procedure

### 1. Discover the matcher — dump windows while the app is running

Ask Ken to make sure the app is open, then:

```
kvscf-core windows <name-substring>
```

This prints every visible window as `hwnd  image  class  title`. Find the app's real row and read off
its **image** and **class**. Pick the narrowest reliable matcher:
- Prefer `process=<image>` when the image is a stable, app-specific basename.
- Use `class=<class>` when the image is blocked (elevated) or generic. `Chrome_WidgetWin_1` is
  **not** specific (Electron/Chromium apps all share it) — pair it with `title` or prefer `process`.
- Add `match=<title substring>` only if the app has multiple windows and you must pick one.

### 2. Discover the launch spec

- **Store/packaged app** → get its AUMID:
  ```
  powershell -NoProfile -Command "Get-StartApps | Where-Object { $_.Name -like '*<name>*' }"
  ```
  Use the `AppID` column as `launch` with `launch_kind=aumid`.
- **Win32 app** → find the exe path (from the running process, or the Start-menu shortcut target):
  ```
  powershell -NoProfile -Command "(Get-Process <procname> -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Path)"
  ```
  Use that full path as `launch` with `launch_kind=exe`. Prefer AUMID over a versioned
  `...\WindowsApps\<pkg-version>\...` path (it changes on update).

### 3. Verify — both halves, live

- **Detect:** `kvscf-core find proc=<image> class=<class> title=<sub>` (pass only the fields you set).
  It must print `found hwnd=…`. If it says `no window matched`, the matcher is wrong — go back to step 1.
- **Launch:** only if safe to do so, and ideally with the app closed, confirm the launch target works.
  For `exe`, that the path exists and runs; for `aumid`, that
  `explorer.exe shell:AppsFolder\<AUMID>` opens the app. Don't force-close Ken's running app just to
  test launch unless he's fine with it — a correct AUMID from `Get-StartApps` / exe path from the live
  process is usually enough.

### 4. Write the registry entry

Choose a short lowercase `<key>` (the stable id used by the remote `{app:<key>}` command). Then:

```powershell
$k = "HKCU:\Software\kenhia\kvscf\apps\<key>"
New-Item -Path $k -Force | Out-Null
Set-ItemProperty -Path $k -Name label       -Value "<Display Name>"
# set whichever matcher fields you chose:
Set-ItemProperty -Path $k -Name process     -Value "<image.exe>"     # if using process
Set-ItemProperty -Path $k -Name class       -Value "<CLASS>"         # if using class
Set-ItemProperty -Path $k -Name match       -Value "<title sub>"     # only if disambiguating
Set-ItemProperty -Path $k -Name launch_kind -Value "exe"             # or "aumid"
Set-ItemProperty -Path $k -Name launch      -Value "<path-or-AUMID>"
Set-ItemProperty -Path $k -Name order       -Value <n> -Type DWord   # optional sort index
```

Omit matcher fields you didn't set (don't write empty strings — kvscf treats empty as unset, which is
fine, but cleaner to omit). Then confirm the whole pipeline:

```
kvscf-core.exe   # (or the kvscf app) — but the quickest check:
<kvscf app dir>\target\release\kvscf.exe --dump-apps
```

`--dump-apps` prints each configured app with its resolved running state — the new app should appear,
`running` if its window is open. Do them **one at a time**, verifying each before the next.

## Verified reference entries (already scouted live, 2026-07-18)

| key | label | matcher | launch_kind | launch |
|---|---|---|---|---|
| claude | Claude | `process=claude.exe` | aumid | `Claude_pzs8sxrjxfjjc!Claude` |
| copilot | Copilot | `process=mscopilot.exe` | aumid | `Microsoft.Copilot_8wekyb3d8bbwe!App` |
| everything | Everything | `class=EVERYTHING` | exe | `C:\Program Files\Everything\Everything.exe` |
| battlenet | Battle.net | `class=Chrome_WidgetWin_0` + `match=Battle.net` | exe | `C:\Program Files (x86)\Battle.net\Battle.net.exe` |
| terminal | Terminal | `process=WindowsTerminal.exe` | aumid | `Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe!App` |
| kindle | Kindle | `process=Kindle.exe` | exe | `C:\Users\kenhi\AppData\Local\Amazon\Kindle\application\Kindle.exe` |

### Gotchas learned live
- **Battle.net can't match by process** — its `QueryFullProcessImageNameW` basename is a random
  `temp_…`, so `process=` never matches. Use `class=Chrome_WidgetWin_0` + `match=Battle.net` (its
  class is `Chrome_WidgetWin_0`, the second window "Friends" is a different class/title). It runs two
  windows (main + Friends); the `match` picks the main one.
- **Everything** is often elevated/tray → `MainWindowHandle`=0 and the process image path may be
  blocked, so `process=` fails. Match by **`class=EVERYTHING`** (needs no process access).
- **Apps don't auto-foreground on launch** — kvscf's `launch_and_focus` launches, then polls ~20s for
  the window and foregrounds it. Cold-launch (e.g. Kindle) can take several seconds; that's expected.
- **Electron/Chromium apps** (Claude, Copilot, Battle.net) all use `Chrome_WidgetWin_*` classes, so
  class alone is not unique across them — prefer `process=` when the image is app-specific.

When odd apps turn up that these rules don't cover, tweak this skill with what you learned.
