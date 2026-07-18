# Sprint 002 — The Windows app

Status: **planned** (depends on 001).

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

- [ ] `crates/kvscf` scaffold + egui/eframe + `tray-icon`.
- [ ] Left-docked borderless always-on-top window: fixed width, full height, saved position; minimize
      to tray + restore.
- [ ] List view + formatter (reuse `kvscf-core` display helpers); sort toggle.
- [ ] Refresh strategy (poll vs `SetWinEventHook`) + debounce.
- [ ] Click → `focus`.
- [ ] Global restore hotkey.
- [ ] Single-instance guard; clean shutdown.

## Notes / open questions

- Decide poll vs. `SetWinEventHook` after measuring churn/cost in practice.
- AppBar reserved-edge space vs. plain always-on-top overlay — start with overlay; upgrade to AppBar
  only if window overlap actually annoys.
