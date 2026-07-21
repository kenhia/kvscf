# Sprint 015 — docked-width chrome fix + config recovery

Ken's feedback on sprint 014's chrome, plus a data-loss incident diagnosed and repaired
(2026-07-20).

## The incident: apps + favorites gone

Symptom: after restarting on the new build, the Apps tab was empty and Code favorites were gone.
Two separate causes, neither in the new code:

1. **Registry hive rollback.** Application event log, 7/18 23:53: Event 1512/1517 — *"Windows
   cannot unload your registry file… Windows saved user …-1003 registry while an application or
   service…"* — followed by the 7/19 0:11 boot. The `HKCU\Software\kenhia\kvscf\apps` subkeys
   (seeded during sprint 007, exactly that evening) were lost in the rollback; the older sibling
   settings values survived. `KVSCF_TOKEN` was never in cleo's registry — the remote channel had
   been riding the `C:\tools\bin\.env` fallback all along, which is why it kept working.
2. **Favorites were likely never on disk.** `save_favorites` errors were swallowed (`let _ =`),
   and a kvscf launched early in boot can lack `%APPDATA%` — in that state every save fails
   silently and the list exists only in memory until the process exits. The review had noted the
   swallow as "acceptable"; it was exactly this failure.

**Recovery:** all six app entries rebuilt from sprint 007's verified matcher/launch table
(targets re-verified live first; `--dump-apps` resolves all six), and `KVSCF_TOKEN` promoted from
`.env` into the registry (the documented preferred source). Favorites are not reconstructable —
re-star as you go.

## Chrome fixes (Ken's design direction)

The sprint-014 one-row top bar clipped at docked width (~180px): the Apps tab, ⚙, and the
bottom buttons were unreachable — including the undock control itself. Changes:

- **Tabs look like tabs**: `Code | Edge | Apps` drawn as flat labels with a per-tab accent
  underline on the selected one (Code/Apps blue, Edge teal), no counts, no fills. Fits any
  width.
- **Bottom "Controls" drawer, collapsed by default**: everything that isn't a tab lives there
  vertically — Refresh, the three mode toggles, and (Code tab) Save set / Restore / Update
  Assist + status. Expandable and fully reachable at any rail width; force-opens while an
  Update Assist flow is mid-step so its buttons can't hide.

## Hardening

- `winset::appdata()` falls back to `%USERPROFILE%\AppData\Roaming` when `%APPDATA%` is absent
  (the early-boot case).
- Favorites save failures are surfaced (status line + stderr) instead of swallowed; Save set
  failures now include the error.

## Verification

fmt / clippy `-D warnings` (both feature sets) / 29 tests green; `--dump-apps` resolves all six
restored apps; `--build-info` probes pass on the release build.
