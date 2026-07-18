# Sprint 005 — Edge second-tab research (WI #472)

Status: **research done — feasible, and simpler than VS Code.** This sprint is a scout + write-up; the
actual Edge tab is a follow-on (sprint 006). Recorded so future-us and others have the map.

## Question

Add an in-app tab strip `[ Code | Edge ]`; the Edge tab lists/focuses Microsoft **Edge windows** the same
way the Code tab does VS Code windows. The rows should be the **window names** — the user-set name from
Edge's *right-click title bar → "Name window…"* — with named windows first, a separator, then the
unnamed ones. The open questions were: can we get that name, and can we tell a named window apart from
one whose title is just its active tab?

## Method

Enumerated top-level `msedge.exe` windows with `EnumWindows` (same as sprint 001 for VS Code) and dumped
titles. Ground truth: **27 live Edge windows** on `cleo`.

## Findings

**1. Enumeration is identical to VS Code.** Edge windows are `msedge.exe`, class `Chrome_WidgetWin_1`
(Chromium — same as VS Code, so class is not a filter; process image is). The existing
`kvscf-core` enumerate path generalizes by just widening the process filter.

**2. The window name IS the title — no storage lookup needed.** Two clean shapes:

| kind | title shape | examples (real) |
|---|---|---|
| **Named** (user "Name window…") | **just the name**, nothing appended | `korg`, `Homelab`, `Claude`, `GitHub`, `Wowhead-Main`, `AI-2 Computer Purchase` |
| **Unnamed** | `<active tab title>[ and N more pages] - <Profile> - Microsoft Edge` | `hvsim — galaxy - Personal - Microsoft​ Edge`, `Home - Dashboards - Grafana and 1 more page - Personal - Microsoft​ Edge` |

This is *simpler* than VS Code, which needed `workspaceStorage` to recover folder paths. Edge puts the
name right in the window title.

**3. Named vs unnamed is trivially distinguishable — and robust.**
> **Rule:** normalize the title (drop `U+200B`), then **a title ending in ` - Microsoft Edge` is
> unnamed** (tab-derived); anything else is a **named** window (label = the whole title).

Gotcha: the branding string is literally `Microsoft​ Edge` — there's a **zero-width space
(U+200B)** between "Microsoft" and "Edge". Strip `U+200B` before the suffix check.

**4. Label extraction.**
- **Named:** label = title verbatim.
- **Unnamed:** strip the trailing ` - <Profile> - Microsoft Edge`, and a trailing ` and N more page(s)`,
  leaving the active tab title (e.g. `Home - Dashboards - Grafana`). The `<Profile>` is the Edge profile
  (`Personal` here) — variable, so match it as "the segment before ` - Microsoft Edge`", don't hardcode.

**5. Validated on all 27 live windows** → 9 named, 18 unnamed, labels extracted cleanly:

```
NAMED (9):  AI-2 Computer Purchase, AI-Models, Claude, GettingClaude, GitHub,
            Homelab, korg, Spice, Wowhead-Main
UNNAMED (18): hvsim — galaxy · Home - Dashboards - Grafana · ch3.ipynb (3) - JupyterLab · … (tab titles)
```

## Feasibility

**Yes — and lighter than VS Code.** No storage archaeology, no path resolution. Focus and close reuse
the existing `kvscf-core` primitives unchanged (`SetForegroundWindow`/`WM_CLOSE` on the HWND).

## Implementation sketch (sprint 006)

- **Generalize `kvscf-core` enumeration:** parameterize the process filter. Introduce a `Kind`
  (`VsCode { app }` | `Edge`) and per-kind title parsing → a common `Window { hwnd, kind, label,
  named: bool, z_index, … }`. Keep the VS Code parser; add an Edge parser (the rule above).
- **App: `[ Code | Edge ]` tab strip** at the top; each tab renders its kind's list. Edge tab: **named
  windows first (sorted by name), a separator, then unnamed (by tab title)**.
- Focus/close: unchanged. Save/restore + Update Assist stay VS-Code-only (Edge windows aren't
  folder-URI relaunchable the same way).
- Row styling: reuse the bold/colored treatment; pick an Edge accent.

## Open questions / edge cases (for 006)

- **Profiles:** multi-profile users show the profile in unnamed titles (`Personal` here). Could surface
  it; named windows don't carry a profile. Low priority.
- **Edge channels:** Dev/Beta/Canary — confirm their process image (likely still `msedge.exe` in a
  different install dir) if we want to tag them. Ken uses stable.
- **Coincidental name:** a window literally named `Something - Microsoft Edge` would misclassify as
  unnamed. Vanishingly rare; accept.
- **Tab count:** the ` and N more pages` is a free signal — could show a tab-count badge on unnamed rows.
- **No remote/kdeskdash for Edge** in scope (the dashboard mode is VS-Code-specific for now).

## Reference

Scout script: `scratchpad/scan-edge.ps1` (throwaway). Findings captured here.
