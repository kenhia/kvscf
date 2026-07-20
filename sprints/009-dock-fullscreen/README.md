# Sprint 009 — Dock yields to fullscreen apps (WI #481)

Status: **built; live-verified for WoW fullscreen, awaiting Ken's checks on the rest.** A docked
kvscf sat on top of fullscreen apps — Ken saw the rail over WoW. The taskbar drops behind fullscreen
apps; now kvscf does too (which also covers browser/app **F11**).

## The find

Windows *does* have a notification for exactly this: **`ABN_FULLSCREENAPP`** — "when a full-screen
application is opening, an appbar must drop to the bottom of the z-order... when it is closing, the
appbar should restore its z-order position"
([docs](https://learn.microsoft.com/en-us/windows/win32/shell/abn-fullscreenapp)).

And `dock.rs` **already registers the callback message it arrives on** (`uCallbackMessage` =
`WM_USER+0x101`) — then deliberately ignores it, because the module chose a ~1s re-assert timer over
subclassing winit's HWND. So the notification for this exact case was being delivered and dropped.

## What we did instead (and why)

Rather than pull message-loop interception into an app that has avoided it, we **poll on the dock
tick we already run**. `dock::fullscreen_app_present()`:

- `GetForegroundWindow()` → ignore null, our own HWND, and the shell (`Progman`/`WorkerW`, which
  cover the monitor but aren't fullscreen apps).
- `MonitorFromWindow` on both us and the foreground window — **only react on our monitor**, so a game
  fullscreened on a second display leaves the dock alone.
- Compare `GetWindowRect(fg)` against `MONITORINFO.rcMonitor` — the **full** monitor bounds, *not*
  `rcWork`.

**Why the rect test is the right line:** it catches exclusive fullscreen, borderless-windowed, and
F11 uniformly (all cover `rcMonitor`), while a merely **maximized** window cannot trigger it —
maximized respects `rcWork`, which already excludes our reserved band. That's the distinction we
want, and it's why this is a rect test rather than a window-style/caption test.

`update_fullscreen_yield` then flips `WindowLevel` (`AlwaysOnTop` ↔ `Normal`) **only on a state
change**, so we're not pushing a viewport command every tick. `apply_mode` clears the flag on any
dock/undock so the yield can't get stuck.

**The AppBar reservation stays registered throughout.** Only the topmost flag needs to move —
fullscreen apps use full monitor bounds and ignore the work area anyway (the taskbar keeps its band
too). Position re-assert uses `SWP_NOZORDER`, so it won't fight the yield.

## Verification

`--probe-fullscreen` samples the foreground window every 500ms for 20s and prints the verdict plus
*why* (class, window rect vs monitor rect). It exists precisely because the research flagged
borderless-windowed behavior as unconfirmed — this settles it empirically instead of by hope.

**Live result (2026-07-20), WoW running fullscreen on the 3440x1440 primary:**

```
  0  fullscreen=true   class=waApplication Window   win=(0,0)-(3440,1440)  mon=(0,0)-(3440,1440)  World of Warcraft
```

Window rect matches monitor bounds exactly → detected, no ambiguity about which fullscreen mode WoW
was in. Gate: `fmt --check` clean, `clippy -D warnings` clean on remote/local/core, 5 app tests pass.

### Still to confirm (Ken, docked on the primary)
- [ ] Rail is actually **hidden behind** WoW (detection is proven; the z-order effect is not yet).
- [ ] **Alt-tab out of WoW → rail returns** (the restore half; the probe never caught a transition
      because WoW held foreground for all 20 samples).
- [ ] Browser **F11** → hidden; Esc → returns.
- [ ] A merely **maximized** window → rail **still visible** ← the main regression risk.
- [ ] Fullscreen app on a **secondary** monitor → docked rail on primary stays visible.
- [ ] Undock/redock and exit while fullscreen is running → no stuck state, AppBar removed cleanly.

## Follow-up

Handling `ABN_FULLSCREENAPP` properly (subclass the HWND, `HWND_BOTTOM` on TRUE / restore on FALSE)
stays available as a refinement — it would react instantly instead of within ~1s. Only worth it if
the poll's latency is noticeable in practice.
