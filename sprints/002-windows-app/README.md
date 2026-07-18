# Sprint 002 — The Windows app

Status: **done.** Floating + docked (AppBar) modes, bold/colored live list, click-to-focus, settings
persistence, single-instance guard — all verified live. Includes **WI #465** (focus un-maximize fix +
"maximize on focus") and **WI #468** (AppBar dock), both resolved. Tray + global hotkey intentionally
**deferred** (a docked bar likely doesn't want a tray; revisit if wanted). Next: sprint 003 (remote),
with **WI #471** (feature-gate remote comms / `kvscf-local`) pulled in from the start.

## Goal

A lightweight, foreground Windows app over `kvscf-core`: a **tall, narrow "left-nav" strip** docked to
the left edge of the screen, showing a live vertical list of open VS Code windows; click an entry to
foreground+focus it. Minimizable, and lockable in place at the left edge. Normal desktop helper, **not**
a service (contrast `kpidashclient-win`).

## Form factor

- **Tall and thin**, vertical list — like a left navigation rail. Ken locks it against the left edge of
  a widescreen monitor and is fine ceding a few pixels of width for it.
- **Locked/docked** to the left edge when not minimized; **minimizable** (tray icon to restore).
- Docking approach — start simple with a **left-docked, always-on-top, borderless window** at a fixed
  width + saved position. If we want maximized windows to *not* overlap it (true reserved edge space,
  like the taskbar), the proper mechanism is the Windows **AppBar API** (`SHAppBarMessage`,
  `ABM_NEW`/`ABM_SETPOS`) — heavier; treat as an optional upgrade, not the MVP.

## Stack

- **`crates/kvscf`** — the app, depending on `kvscf-core`.
- UI: **egui / eframe** (pure Rust, quick, no C toolchain) for the rail, plus the **`tray-icon`** crate
  for a tray entry (restore from minimized). Revisit only if egui proves awkward for a
  borderless/always-on-top docked window.

## Scope

- Left-docked tall/thin window: fixed width, full (or configurable) height, borderless, always-on-top,
  saved position; minimize-to-tray and restore.
- List rendering per PLAN §4: vertical rows of `workspace (host)` with host in a distinct color and a
  Stable/Insiders accent; grouped by host with an A–Z / recency sort toggle. (No dirty marker.)
- **Click-to-focus** wired to `kvscf-core::focus` (the easy foreground case — app has focus at click).
- **WI #465 — focus/maximize behavior:**
  - *Core fix:* `kvscf-core::focus` must only `SW_RESTORE` when the target is minimized (`IsIconic`) —
    an unconditional `SW_RESTORE` un-maximizes an already-maximized window (observed in 001).
  - *App option:* a **"maximize on focus" checkbox**; when checked, focusing also `SW_MAXIMIZE`s the
    target (via `kvscf-core::focus_with(hwnd, maximize)`).
- **Live refresh**: light poll (e.g. 1–2 s) or on foreground-window change; debounce so the list doesn't
  churn. (`SetWinEventHook` for `EVENT_SYSTEM_FOREGROUND` is the tidy event-driven option — evaluate vs.
  a simple poll.)
- Empty state ("no VS Code windows open") and a manual refresh affordance.
- A global **show/hide (restore) hotkey** (nice-to-have; configurable or a sane default).

## Verification

- [ ] Rail docks tall/thin at the left edge, stays put, survives minimize/restore, and remembers width.
- [ ] With several editors open, the list shows them correctly and updates within ~2 s as windows
      open/close or switch workspace.
- [ ] Clicking any entry reliably foregrounds that exact window.
- [ ] Tray/behavior sane (single instance, clean exit, no lingering process).

## Out of scope

- Remote / kdeskdash channel (sprint 003).
- Installer / packaging polish (fold in later if wanted; a plain exe is fine to start).

## Tasks

- [x] **WI #465 core fix**: `kvscf-core::focus` conditional `SW_RESTORE` (only when `IsIconic`); added
      `focus_with(hwnd, maximize)`.
- [x] `crates/kvscf` scaffold + egui/eframe.
- [x] List view + formatter: left-aligned rows, name build-colored + **real bold** (Segoe UI Bold loaded
      from system fonts, graceful fallback), host italic/muted, **name truncated but host always kept**
      (`generative_ai_w… kai`). Sort = lowercased name (hosts interleaved); toggle dropped as unneeded.
- [x] Click → `focus_with(hwnd, maximize_on_focus)`.
- [x] **WI #465 app option**: "maximize on focus" checkbox wired to `focus_with`.
- [x] Refresh strategy: 1s poll + immediate refresh on `⟳`. (`SetWinEventHook` not needed at this cadence.)
- [x] Window behavior (revised): **normal, non-always-on-top, resizable**, remembers geometry
      (`persist_window`). "Auto-hide after focus" self-minimizes ~2s after a click (default off).
- [x] Settings persistence: `maximize_on_focus` + `auto_hide` → `HKCU\Software\kenhia\kvscf` (`winreg`),
      written on change.
- [x] No console in release (`windows_subsystem` gated to release).
- [x] **AppBar "docked" mode (WI #468)**: `dock.rs` reserves the **primary monitor left edge**
      (`SHAppBarMessage` ABM_NEW/QUERYPOS/SETPOS/REMOVE); "Dock (primary left)" toggle flips
      decorations + always-on-top and (de)registers the appbar; ~1s re-assert; `on_exit` removes it;
      `docked` persisted. HWND via eframe `HasWindowHandle`. **Verified live** — maximized windows respect
      the reserved band, undock releases it, persists across runs.
- [ ] Minimize to tray + restore (`tray-icon`) — deferred; less critical now the window is a normal,
      non-top window (and a docked bar likely doesn't want a tray).
- [ ] Global restore hotkey — deferred with tray.
- [x] Single-instance guard: named mutex (`Local\kvscf-single-instance`); a 2nd launch exits
      immediately (verified: ~162 ms, code 0, no window). Prevents two docked bars fighting the edge.

## Verified with Ken (2026-07-18)

Screenshot review + live use: rows/colors/left-align good; **bold** made scanning dramatically faster
(**~0.5–1.5 s** to find a target vs **5–12 s** hunting taskbar thumbnails); truncation "perfect";
settings persist on subsequent runs. Dropped the **Mono** option (bold alone won). The first-run
"didn't persist" was an artifact of watching it together; later runs persist fine.

**AppBar dock (#468) verified:** "everything works exactly as expected" — docks to primary left,
maximized windows respect the reserved band, undock releases it cleanly, persists across runs. (Same
benign first-run persistence quirk as above.)

## Notes / open questions

- **Docking modes:** the requested "docking" is the Windows **AppBar** API (reserve a screen edge like
  the taskbar) — tracked as **WI #468**, the next slice. Current default is the floating window +
  auto-hide fallback.
- Poll (1s) is fine; revisit `SetWinEventHook` only if churn/cost warrants.
- Tray + global hotkey deferred — reconsider once AppBar mode lands (a docked bar may not want a tray).
