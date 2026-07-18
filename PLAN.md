# kvscf — Planning Doc (draft, 2026-07-17)

Status: **draft for review** — nothing implemented yet.

## 1. Goal

A Windows helper that surfaces every open **VS Code** / **VS Code Insiders** window as a sorted,
readable list, and — on click (local) or tap (remote, via kdeskdash) — brings that specific window to
the foreground and focuses it.

Two consumption surfaces over one shared core:

- **Local app** — tray + popup list, driven directly on the Windows box.
- **Remote mode** — the instance list streamed to [`kdeskdash`](ssh://kai/~/src/tools/kdeskdash) on
  kai as a `vscode` mode; a tap there sends a select command back to focus the window.

Sibling in spirit to [`krcmd`](../krcmd) (which *launches* editors from kai/kubs into Windows) — kvscf
*surfaces and focuses already-open* windows. Same "control my Windows editors from the homelab" theme.

## 2. Stack decision: Rust + `windows` crate

Consistent with the sibling Windows tools in this tree (`krcmd`, `kpidashclient-win`): Rust workspace,
the `windows` crate for Win32 FFI, no C shims. Everything the OS integration needs is reachable:

| Concern | How |
|---|---|
| Enumerate top-level windows | `EnumWindows` + `GetWindowThreadProcessId` (`windows` crate `Win32::UI::WindowsAndMessaging`) |
| Window title / class | `GetWindowTextW`, `GetClassNameW` |
| Process name for a window | `OpenProcess` + `QueryFullProcessImageNameW` (or `GetModuleBaseNameW`) |
| Visibility / real editor windows | `IsWindowVisible`, non-empty title, exclude tool/cloaked windows |
| Foreground + focus | `ShowWindow(SW_RESTORE)` + `SetForegroundWindow`, with the `AttachThreadInput` workaround (§5) |
| Z-order for recency proxy | `EnumWindows` returns top-to-bottom Z-order |

Proposed crate layout (mirrors `krcmd`'s multi-crate workspace):

- `crates/kvscf-core` — enumeration, **title parsing**, focus logic. Pure library, the reusable heart.
- `crates/kvscf` — the local Windows app (left-docked list UI) → sprint 002.
- Remote channel (sprint 003) folds into `kvscf` as an outbound client task — **kvscf must be running
  for the dashboard to drive it** (locked; no separate headless agent).

A throwaway **PowerShell one-liner** is a fine 15-minute sanity check of enumerate+focus before the
Rust core, but sprint 001 builds the real `kvscf-core` walking skeleton so nothing is thrown away.

## 3. Identifying VS Code windows

Each VS Code editor window is exactly one top-level `HWND`. Filter enumeration to:

- Process image basename in `{ Code.exe, Code - Insiders.exe }` (→ also tags Stable vs Insiders).
- `IsWindowVisible` true and title non-empty (drops hidden helper/renderer/shared-process windows).
- Optionally exclude cloaked windows (`DwmGetWindowAttribute` / `DWMWA_CLOAKED`) to skip virtual-desktop
  ghosts — evaluate if it turns out to be a problem.

`HWND` is stable for the life of the window (not across restarts) — good enough: the live list always
carries current handles, and the focus path re-validates a handle before acting on it.

## 4. Title parsing → structured instance

The window title is the pragmatic source of workspace/remote/dirty info. The configured `window.title`
on the target machine is:

```
${dirty}${separator}${rootName}${separator}${activeEditorShort}${separator}${appName}${separator}${profileName}
```

(The `${dirty}` indicator is present in the title but **we deliberately do not expose dirtiness** — see
below.) We do **not** hard-code the exact format — titles are user-configurable and vary per machine, so
the parser strips any leading `${dirty}` indicator, strips stray `${...}` tokens defensively (config
drift / typos), and extracts semantics regardless:

- **`rootName`** carries the workspace name plus, for remote windows, a bracket tag:
  `[SSH: <host>]`, `[WSL: <distro>]`, `[Dev Container: <name>]`, `[Codespaces]`. Parse:
  - `workspace` = leading name (e.g. `kvllm`).
  - `remote_kind` ∈ `{ Local, Ssh, Wsl, DevContainer, Codespaces }`.
  - `remote_host` = the captured host/distro/name (e.g. `kai`).
- **`app`** — from `${appName}` → Stable vs Insiders (also confirmable via process name, §3).
- **`active_file`** — from `${activeEditorShort}`, optional secondary label.

**Dirty is out of scope (decided).** VS Code's `${dirty}` reflects only the *active editor*, not "any
unsaved tab in the window," so it would mislabel windows that have unsaved changes in a non-active tab.
Rather than show a misleading indicator we drop it entirely; revisit only if a reliable "any unsaved"
signal becomes available.

Resulting model:

```rust
struct Instance {
    hwnd: u64,
    app: App,                 // Stable | Insiders
    workspace: String,        // "kvllm"
    remote: Remote,           // Local | Ssh("kai") | Wsl("Ubuntu") | DevContainer(..) | ...
    active_file: Option<String>,
    z_index: usize,           // enumeration order = recency proxy
}
```

### Display

Per the target look:

- `kvllm (kai)` — host (`kai`) rendered in a distinct color; local windows omit the `(...)` suffix.
- Stable vs Insiders distinguished by icon/color accent.

### Sorting

Default: group by remote host (locals first, then by host), then workspace name A–Z. Alternative
sort = **recency** via Z-order (`z_index`) so the last-active window floats to the top. Make it a toggle;
Z-order is the cheap recency signal we get for free from `EnumWindows`.

## 5. The one real gotcha: foreground from the background

Windows blocks background processes from stealing foreground — a naive background `SetForegroundWindow`
just flashes the taskbar button instead of raising the window. Standard mitigation:

```
ShowWindow(hwnd, SW_RESTORE);         // un-minimize if needed
// AttachThreadInput(ourThread, targetThread, TRUE);
SetForegroundWindow(hwnd);
// AttachThreadInput(ourThread, targetThread, FALSE);
```

plus, if needed, a synthetic `ALT` key blip (`keybd_event`) to satisfy the foreground-lock heuristic.
This matters differently per surface:

- **Local click** — the app *has* foreground at click time and can hand it off cleanly. Nearly always
  works with just `SW_RESTORE` + `SetForegroundWindow`.
- **Remote select** (sprint 003) — the trigger arrives over the network while the app is in the
  background: exactly the restricted case. The `AttachThreadInput` path handles it ~reliably. This is
  the highest-risk item and gets a dedicated verification in 001 (test focusing while another app holds
  foreground), not just "click the app and it works."

**Also characterize the *un-mitigated* path.** 001 will run a naive `SetForegroundWindow` (no
`AttachThreadInput`) from the background and record what actually happens. Hypothesis: instead of fully
raising, Windows **highlights/flashes the target's taskbar button + thumbnail** — which is *already
useful* as a fallback (it points Ken at the right window without hunting thumbnails). We keep the full
mitigation as the primary path, but knowing the un-mitigated fallback behavior is worth banking.

## 6. Remote channel (sprint 003 sketch)

- **kvscf must be running** for the dashboard to drive it (locked). The channel is an outbound client
  task inside the `kvscf` app — no separate headless agent, no service.
- The app **opens an outbound WebSocket to kdeskdash** over the Tailscale tailnet
  (`encke-wahoo.ts.net`). Outbound-only = no inbound firewall/reachability into Windows; kai never
  dials in.
- It **pushes the instance list** on change (debounced): `[{hwnd, app, workspace, remote, ...}]`.
- kdeskdash `vscode` mode **renders the list** and, on tap, sends `{ "select": <hwnd> }` back down the
  same socket → the app re-validates the handle and runs the §5 focus path.
- **Auth: Option A (locked)** — Tailscale-only + a preshared token in the frame. The tailnet provides
  the network boundary; the token gates the `select` action specifically (the only frame that takes
  action). Not riding krcmd's SSHSIG path.
- **kdeskdash side** is a separate build on kai (its multi-mode model already has `dev` mode etc.; a
  `vscode` mode is the same shape). Sprint 003 produces the Windows channel **plus a handoff spec doc**
  for that mode — implemented on kai as its own follow-on.

## 7. Non-goals (for now)

- Cross-platform (Linux/Mac) window control — Windows only.
- Launching / closing editors (that's `krcmd`'s job).
- Persisting window history across VS Code restarts.
- Per-tab dirtiness beyond what the title exposes.
