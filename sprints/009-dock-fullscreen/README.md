# Sprint 009 — Dock yields to fullscreen apps (WI #481)

Status: **done — verified live with WoW and VS Code Insiders F11.** A docked
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

`update_fullscreen_yield` acts **only on a state change**, so we're not hammering `SetWindowPos`
every tick. `apply_mode` clears the flag on any dock/undock so the yield can't get stuck.

### The bug worth remembering: un-topmost ≠ behind

The first implementation "dropped always-on-top" via `ViewportCommand::WindowLevel(Normal)` — and it
**didn't work**, while looking like it should. Instrumenting the live window proved detection and the
toggle were both fine:

```
29  railTOPMOST=True   coversMonitor=True    <- fullscreen detected
30  railTOPMOST=False  coversMonitor=True    <- topmost cleared, correctly, within ~1s
39  railTOPMOST=True   coversMonitor=False   <- restored on exit
```

…yet the rail still sat on top. The reason: **`HWND_NOTOPMOST` parks a window at the *top of the
non-topmost band*** — above every ordinary window, including the fullscreen app. Clearing the topmost
style is not the same as going behind. That's why `ABN_FULLSCREENAPP` says an appbar must drop to the
**bottom of the z-order**; it's a literal instruction, not a description.

So `yield_z_order` does both: `HWND_NOTOPMOST` to leave the topmost band, then `HWND_BOTTOM` to sink
below everything. `restore_z_order` puts back `HWND_TOPMOST`.

Z-order is driven **straight through Win32** rather than `ViewportCommand::WindowLevel`, because
viewport commands are applied asynchronously on the next frame — which would race the ordering of the
two `SetWindowPos` calls the yield depends on.

**The AppBar reservation stays registered throughout** — fullscreen apps use full monitor bounds and
ignore the work area anyway (the taskbar keeps its band too). The 1s position re-assert uses
`SWP_NOZORDER`, so it won't undo the sink.

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

### Confirmed live by Ken (docked on the primary)
- [x] **WoW** → rail hidden behind it; returns on exit.
- [x] **VS Code Insiders F11** → rail hidden; returns on toggle back.

### Not explicitly re-tested after the fix
- [ ] A merely **maximized** window → rail should stay visible. The rect test makes this structurally
      impossible to false-trigger (maximized respects `rcWork`, which excludes our band), and it held
      through all the instrumented sampling — but it wasn't deliberately exercised post-fix.
- [ ] Fullscreen app on a **secondary** monitor → docked rail on primary unaffected (the
      same-monitor guard covers it; not exercised).

## Follow-up

Handling `ABN_FULLSCREENAPP` properly (subclass the HWND, `HWND_BOTTOM` on TRUE / restore on FALSE)
stays available as a refinement — it would react instantly instead of within ~1s. Ken reported the
poll's latency as unnoticeable in practice, so this is not currently worth the message-loop
interception it would cost.
