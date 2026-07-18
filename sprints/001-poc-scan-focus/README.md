# Sprint 001 — POC: scan & focus

Status: **POC validated (PowerShell prototype) — all three mechanics proven; Rust `kvscf-core` build
pending.** Enumeration, title parsing, and the focus recipe are all confirmed against live windows on
Ken's box (see Findings). Remaining: port to the `kvscf-core` crate with unit tests + CI.

## Goal

Prove the three load-bearing mechanics before any UI: **enumerate** open VS Code / Insiders windows,
**parse** their titles into structured `Instance`s, and **foreground + focus** a chosen one — including
the hard case of focusing while another app holds the foreground. This is the `kvscf-core` walking
skeleton everything else attaches to.

## Scope

- Cargo **workspace** scaffold + `crates/kvscf-core`, CI (`windows-latest` build + test + clippy).
- Enumeration: `EnumWindows` → filter to visible, titled, top-level windows whose process is
  `Code.exe` / `Code - Insiders.exe` (Stable vs Insiders tagged from the process name).
- Title parser → `Instance { hwnd, app, workspace, remote, active_file, z_index }`:
  - extract `workspace` and remote tag (`[SSH: host]`, `[WSL: distro]`, `[Dev Container: ..]`, `[Codespaces]`),
  - strip the leading `${dirty}` indicator and any stray `${...}` tokens (config drift). **Dirty is not
    exposed** — see [../../PLAN.md](../../PLAN.md) §4.
- Focus path: `ShowWindow(SW_RESTORE)` + `SetForegroundWindow`, with the `AttachThreadInput` workaround
  and optional synthetic-ALT fallback (see [../../PLAN.md](../../PLAN.md) §5). Also exercise the
  **un-mitigated** call to characterize the taskbar-highlight fallback (§5).
- A tiny CLI over the core to exercise it:
  - `kvscf-core list` → prints the sorted, formatted instance list (with hwnds).
  - `kvscf-core focus <hwnd>` → foregrounds that window.

## Verification (the point of this sprint)

- [x] Open several VS Code windows (mix of local + at least one `[SSH: kai]` remote, Stable + Insiders)
      → `list` shows every one, correctly labeled `workspace (host)`. **Done** via PowerShell prototype:
      17 Insiders windows (local + SSH kai + SSH kubs0) all listed and labeled correctly. *Stable not
      exercised* (none open) — carry to Rust tests.
- [x] `focus <hwnd>` raises the right window (easy case) — covered by the mitigated recipe (test #2).
- [~] **Hard case:** mitigated recipe raised the window while another app (`Claude`) held foreground —
      **but** the PS caller was a child of that foreground app, so the grant was friendly. Truly-hostile
      case (idle background, unrelated foreground) deferred to sprint-003 verification from the real app.
- [x] **Un-mitigated characterization:** bare `SetForegroundWindow` did **nothing visible** for a
      minimized window (no raise, no taskbar flash) despite returning `True` — the fallback hypothesis
      was wrong; the un-mitigated path is unusable. Recorded above.
- [ ] Confirm no phantom entries (helper/renderer/cloaked windows) leak into the list.

## Out of scope (later sprints)

- Any GUI / tray (sprint 002).
- Live refresh / change events (sprint 002).
- Remote channel / kdeskdash (sprint 003).

## Tasks

- [ ] Workspace `Cargo.toml` + `crates/kvscf-core` scaffold, deps (`windows` crate feature set for
      `Win32_UI_WindowsAndMessaging`, `Win32_System_Threading`, `Win32_Foundation`, ...).
- [ ] `enumerate.rs`: `EnumWindows` collect → filter → `(hwnd, pid, title, process_name)`.
- [ ] `parse.rs`: title → `Instance`; remote-tag extraction + `${dirty}`/`${...}` stripping; unit tests
      over real title samples (locals, SSH, WSL, Insiders, Default profile).
- [ ] `focus.rs`: `SW_RESTORE` + `AttachThreadInput` + `SetForegroundWindow` (+ ALT fallback); plus a
      no-mitigation variant for the taskbar-highlight characterization.
- [ ] `sort.rs` / format: default group-by-host, plus recency-by-`z_index`; display formatter
      (`workspace (host)`, `*` dirty).
- [ ] `main.rs`: `list` / `focus <hwnd>` verbs.
- [ ] CI workflow on `windows-latest`.
- [ ] Run the four verification checks above; record real title samples in `parse.rs` tests.

## Findings (live sanity check, 2026-07-17)

Validated enumeration + parse against the real VS Code windows on Ken's box via a throwaway PowerShell
`EnumWindows` + parse prototype (scratchpad) before writing any Rust. Results:

- **Enumeration confirmed.** 17 VS Code Insiders windows, **all sharing one process** (single Insiders
  pid), each a distinct top-level `HWND`, window class `Chrome_WidgetWin_1`. Confirms `Get-Process`
  (one `MainWindowHandle` per process) is **insufficient** — `EnumWindows` is mandatory. The
  visible + non-empty-title + process-image filter returned **exactly** the 17 real editor windows out
  of 72 system-wide top-level windows — **zero phantoms**, no helper/renderer leakage.
- **Separator gotcha (important).** The title separator is `" - "`, but the appName
  `"Visual Studio Code - Insiders"` *itself contains* `" - "`. A naive split on `" - "` over-splits.
  Parser must strip the **known appName suffix first** — and match **Insiders before Stable**, since the
  Stable pattern `" - Visual Studio Code"` is a prefix of the Insiders one.
- **Class is not a filter.** `Chrome_WidgetWin_1` is generic Electron/Chromium (Slack, Chrome, etc.) —
  filter on process image name, not class.
- **Remote tag** confirmed as `<workspace> [SSH: <host>]` (brackets, not separator-delimited). Hosts
  seen: `kai`, `kubs0`. Regex `^(.*?)\s*\[(SSH|WSL|Dev Container|Codespaces):\s*([^\]]+)\]\s*$`.
- **Default profile collapses cleanly** — empty `${profileName}` leaves no trailing separator or literal
  token (confirms the `${separator}` typo fix; nothing stray to strip in current titles).
- **Middle segment is arbitrary**, not always a filename: observed real filenames (`leaderboard.md`,
  `ch3.ipynb`) *and* Claude session tab titles (`Start sprint korg:437`, `Create WI and sprint pro…`),
  sometimes truncated with `…`. Split `rootName` on the **first** `" - "` and treat the rest as an
  opaque `active_file` label. (Edge risk: a workspace folder name containing `" - "` — rare, accept.)

Parse output matched the target look on all 17 (`kvscf`, `kvllm (kai)`, `kpidash (kubs0)`, ...).

### Focus test #1 — un-mitigated (2026-07-17)

Target: `kdeskdash (kai)`, **minimized**, from a PowerShell child of the foreground Claude app.

- **API lied.** Bare `SetForegroundWindow` returned `True` and `GetForegroundWindow` afterward reported
  the *target* hwnd — but **visually nothing happened**: the window stayed minimized, and there was
  **no taskbar highlight/flash** either. The hoped-for "at least it flashes the taskbar" fallback did
  **not** materialize for a minimized window. Conclusion: **the un-mitigated path is unusable** — don't
  trust the return value or `GetForegroundWindow` as evidence of a visible result.
- **`ForegroundLockTimeout` = 200000 (default, lock ENABLED)** — so the (nominal) success was *not* a
  disabled-lock box. Likely granted because the PS caller was **started by the foreground process**
  (Claude) and/or the foreground app is a normal Win32/Electron app, not UWP. Either way this harness is
  **optimistic** vs. the real sprint-003 case (kvscf idle in the background, not a child of the
  foreground app, no recent input) — the genuinely hostile case still needs the `AttachThreadInput`
  mitigation, and must ultimately be verified from the real backgrounded app, not a Claude-spawned shell.
- **Minimized windows need `ShowWindow(SW_RESTORE)`** explicitly — `SetForegroundWindow` alone never
  un-minimizes. This is now a hard requirement in the focus path, not optional.

### Focus test #2 — mitigated (2026-07-17) ✅ WORKS

Same target (`kdeskdash (kai)`, minimized), Claude still the foreground app. Sequence:

```
tgtThread = GetWindowThreadProcessId(currentForegroundWindow)   // attach to whoever holds foreground
AttachThreadInput(ourThread, tgtThread, TRUE)
ShowWindow(target, SW_RESTORE)
SetForegroundWindow(target)
BringWindowToTop(target)
AttachThreadInput(ourThread, tgtThread, FALSE)
```

- **Visually confirmed by Ken: the window restored and came to the foreground.** `iconic` flipped
  True→False, foreground = target. This is the working recipe.
- No need for the synthetic-ALT variant (test #3) — the `AttachThreadInput` + `SW_RESTORE` combo was
  sufficient here.
- **Standing caveat:** the PS caller was still a child of the foreground Claude app, so the foreground
  grant was friendly. The genuinely hostile case (kvscf idle in the background, unrelated foreground app,
  no recent input) is **only truly verifiable from the real backgrounded `kvscf` app** — carried to
  sprint 003 as the remote-select verification. Recipe is proven; hostile-case robustness is the one
  open question.

### The focus recipe (locked for `kvscf-core`)

Always: attach to the current foreground window's thread → `SW_RESTORE` → `SetForegroundWindow` →
`BringWindowToTop` → detach. Keep the synthetic-ALT blip in reserve as a fallback if the hostile case
proves flaky in sprint 003.

### Canonical title samples (for `parse.rs` unit tests)

```
kvscf - README.md - Visual Studio Code - Insiders
    -> app=Insiders workspace=kvscf remote=Local  active=README.md
kvllm [SSH: kai] - leaderboard.md - Visual Studio Code - Insiders
    -> app=Insiders workspace=kvllm remote=Ssh(kai) active=leaderboard.md
kpidash [SSH: kubs0] - kpidash-cards (7bb4b67) (kpidash-cards (7bb4b67)) - Visual Studio Code - Insiders
    -> app=Insiders workspace=kpidash remote=Ssh(kubs0) active="kpidash-cards (7bb4b67) (kpidash-cards (7bb4b67))"
kyac [SSH: kai] - Start sprint korg:437 - Visual Studio Code - Insiders
    -> app=Insiders workspace=kyac remote=Ssh(kai) active="Start sprint korg:437"
```

TODO for the Rust tests: also capture a **Stable** sample (`... - Visual Studio Code`, no Insiders) and
a **WSL** (`[WSL: <distro>]`) sample — none were open during this check.

## Notes / open questions

- Parser strips leading `${dirty}` and any stray `${...}` tokens defensively (robust to per-machine
  config drift). Dirty is intentionally not surfaced (PLAN §4).
- **Focus tests still pending** (easy case, hard background case, un-mitigated characterization) — these
  foreground real windows on Ken's live desktop, so run them when Ken is watching.
