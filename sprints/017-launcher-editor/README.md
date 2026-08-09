# Sprint 017 — the Launcher editor

Slice 4 of korg program **1143** ("Launcher — retire the Stream Deck"). Covers korg kvscf
**#1135**. Slices 1-3 shipped: the spike (#1132), the verb (#1133, sprint 016), and the kdeskdash
grid mode (#1134). Only slice 5 — the kwork/rpidash3 pairing — comes after this.

## Why this was built last

The instinct is to build the editor first so there is something to configure with. The plan
deliberately did the opposite: the editor's whole job is placing rectangles on a grid Ken looks at
from two feet away at ~21.6 mm per cell, and every design question about the picker is a guess
until that grid has been on the desk for a while. The gap was cheap to cover — buttons went in by
hand, and `--dump-launcher` / `--fire-button` made that checkable.

That patience paid off in one concrete way: slice 3 measured the panel's real behaviour, and this
slice mirrors it rather than inventing a preview.

## Its own window

The rail is 280 px wide and, docked, borderless and always-on-top. A nine-column grid picker
cannot live in it. So the editor is a **second eframe viewport** — a real, resizable OS window
opened from the Controls drawer. `show_viewport_immediate` keeps it an immediate-mode call from
`update()`, so there is no second app state to keep in sync.

## The picker

Fields above a grid that draws what is already there, each button in its real fill with its real
label. Click a free cell for 1×1; drag for a rectangle, clamped to 3×3.

**Bad placements are impossible to express, not reported.** `launcher::rect_is_free` gates what
the picker will commit, so `validate_layout` should never have anything to say about a button this
wrote — it stays the backstop for hand-edited registry entries, which is where overlaps actually
came from. A drag that would overlap paints red and simply does not commit.

One response for the whole grid rather than a widget per cell: a drag has to be read as a
*rectangle between two cells*, which per-cell widgets make awkward and this makes trivial.

## The dropdown that replaced regex

Ken's original ask was a regex field. The reason, when asked, was that his Edge windows are named
things like `🛠️ Pipes` and he did not want to type that into a config box — a data-entry problem,
not a matching problem. The target field is a dropdown of the named windows kvscf can already see,
plus "the current window". No pattern language, no escaping, and it shows him what is actually
open.

Two details that matter more than they look:

- A configured window that is **closed right now still appears** in the list, annotated. Dropping
  it because its window is shut would silently rewrite the button.
- Selecting "a named window" with nothing picked writes `target=current`, not `target=named` with
  an empty name — which `load` would otherwise warn about on every one-second reload.

## Colors, shown rather than described

`color` is the button's **background**, and the panel always draws labels in `MOON_INK`
(near-white). So the editor offers kdeskdash's palette as swatches **drawn with that same
near-white text on them** — legibility is previewed, not asserted. Its five text-role colors
(`MOON_INK`, `STEEL_MIST`, `FADED_DENIM`, `UTC_FROST`, `HOST_GREY`) are not offered at all; a unit
test guards the table against a careless paste. A raw hex field remains for anything else.

The table is copied from kdeskdash's `src/palette.h` rather than shared — different toolchains —
and that is safe by construction: an unrecognized name means "use the default" on the panel (§6),
so drift can only ever cost a color, never a button.

## The panel's limits are enforced here

kdeskdash's parser sizes `key`, `label` and `color` into C buffers, and treats them differently on
overflow: an over-long **key is rejected** (a clipped key would press the wrong button) while a
**label is truncated** (cosmetic). Mirrored as `KEY_MAX_BYTES` / `LABEL_MAX_BYTES` /
`COLOR_MAX_BYTES` — the key is a hard block, the label a warning. Without this the editor could
write a button that silently never appears on the panel, with nothing wrong in the registry.

Byte counts, not character counts: an emoji costs four.

## Keys

The label may carry emoji; the registry subkey cannot sensibly. `slugify_key` derives one
(`"🛠️ Pipes"` → `"pipes"`) and the key follows the label until Ken types in the key field, after
which it stops — an existing button must never be silently renamed out from under a panel that is
already pressing it.

Clash detection is **case-insensitive**, because registry subkey names are: writing `Pipes` beside
`pipes` would overwrite rather than add. A rename is written-then-old-key-removed, and if the
removal fails the status says the new button is live and the old key remains, rather than
reporting a failed save.

## Also built

- **Test** fires the form *as typed* — `launcher::fire` takes a button rather than looking one up,
  so a placement or a target can be tried before it is saved. No dashboard, no Redis.
- **Delete removes the subkey**, not its values. Blanking would leave a key that `load` warns about
  on every reload and that a later button of the same name would inherit stale fields from.
- A save re-scans immediately, so the picker shows what it just wrote instead of waiting out the
  1-second reload. Edit → panel is still about two seconds, with nothing restarted.

## Not in this slice

**Editing `rows`/`cols`.** The grid is displayed, read from the parent key as published, but not
editable: shrinking it orphans buttons, and Ken cannot change it today anyway, so leaving it out is
not a regression. Worth a follow-on when kwork's panel needs a different shape.

## Verification

Gate: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` on **both** feature sets, 63
tests. `-p kvscf-local` is a distinct gate here and not a formality — `label` and `color` were dead
in the local build until this slice read them, and sprint 016 found two real defects that way.

- **Headless full-frame draw.** A bare `egui::Context` embeds viewports, so `ctx.run` executes the
  real `show` path — form, swatches, button list, every painter call in the picker — and fails on a
  panic. This window is opened rarely, and a panic in it would take the rail down with it.
- **A live registry round-trip**, `#[ignore]`d so it stays out of the gate:
  `cargo test -p kvscf-app -- --ignored`. Writes a button with an emoji label, a named target and a
  palette color; reads it back through `scan`; deletes it; confirms the subkey is gone. It picks a
  cell `rect_is_free` reports empty, so it cannot displace a real button, and it cleans up even
  when an assertion fails. Passed on cleo.

**Driven by hand on cleo** by Ken before ship, from the release build (`cargo build --release`,
`kvscf.exe` copied to `C:\tools\bin`) — the agent's own instance could not, since the rail was
already running and kvscf is single-instance.
