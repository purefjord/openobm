# Playtest guide — what to poke at, and why

The port has ~70 pixel-parity frames and ~28 per-beat drive gates, all byte-exact
against the real jar. What it has **never had is a person playing it.** Those are
different instruments: byte-parity only catches divergence at points someone
thought to gate. It structurally cannot catch a softlock, a progression
dead-end, or "this is wrong and no fixture looks here."

That is the remaining 1.0 risk, and it is the one thing automation can't cover
(see `m14-phase0-transition-audit.md` §6c for why the credits-run milestone was
retired).

## Running it

```sh
cargo run -p game --features interactive --bin play
```

Optional args: a `/x.scr` to jump straight into a level (save persistence is
**disabled** for jump sessions, so it can't clobber a real playthrough), and
`wide` / `wide10` for the non-canonical 16:9 viewer.

Controls follow the original's quick-key table. `Esc` quits.

## What a real bug looks like

Two failure shapes are worth distinguishing when you report something:

- **An honest boundary.** If the port hits an opcode or state it hasn't
  reached, it stops ticking gameplay and paints an *overlay* over the last
  live frame instead of crashing. If you see that, it's a known-unknown —
  tell me what you were doing.
- **A panic** (the window dies) is always a bug worth reporting, with what
  you were doing immediately before.

Anything that looks visually wrong is worth a note even if the game continues —
several real finds (the parchment fill colour, the class carousel order) were
things no gate happened to cover.

## Ranked list — least-tested first

**1. Save / load across a real relaunch.** The highest-value item, and it has
an explicitly outstanding manual pass from Item 2's todo. Do:
play → checkpoint menu → Save Game → quit the window → relaunch → "Load Game"
should be in the menu and should restore your player. Then check your key
bindings survived too. The headless test covers the wiring; nobody has done it
by hand. The record lives at `port/playdata/eso.bin` — if something goes wrong,
that file plus a description is exactly what I need.

**2. Free combat.** Explicitly *not* byte-gated — it's RNG-driven, and the
loop-#28 precedent documented it as unreachable for deterministic gating
(l12's Great Gate Xivilai is the standing example). All the combat gates are
seeded single swings. So real fights — multi-enemy, fleeing, dying, respawning,
levelling up — are the largest genuinely unvalidated surface in the game.

**3. The prison (l01_1) played properly.** Directly relevant to what we just
found: the exit is walled until a five-link death-trigger chain completes
(kill actors 4 → 3 → 1 → 5 → 6, each arming the next). No drive has ever
completed it. Does the barrier actually open? Does the exit region work when
you walk into it and press fire?

**4. The action menu (mode 2).** Six byte-identical shots cover the *paint*,
but the equip/use paths are where state changes: equipping armour into each of
the 8 slots, swapping weapons, using potions, the potion-group radio rule,
spell cross-marking. Try to get the inventory into a weird state.

**5. The shop (mode 1).** One drive covers it. Buy until broke, sell your last
item (there's a cursor-fix for removing the last node), sell equipped gear,
buy something your class can't use.

**6. The l06_a maze.** Procedurally generated per seed. Two floors, a captive
to rescue, and a documented rescue path (`l06_6b`) that no trace found until
loop #36 — so that branch is young. Try both: rescue, and leave early.

**7. Custom Controls (modes 5/20).** Rebinding commits `f ← g` and fires a
real save write. Digits 2/4/5/6/8 should be unbindable (reserved). Check a
rebind survives a relaunch, and that rebinding doesn't eat your saved game.

**8. Death and respawn.** Die on purpose. The death screen's YES resumes with
a respawned player at the level's anchor; NO returns to the menu. Worth
confirming the respawn anchor is sane in a few different levels.

**9. Level transitions in real play.** Nobody has walked from one quest into
the next. Each transition re-spawns you at the destination's scripted
coordinates, so watch for arriving somewhere wrong, stuck, or inside geometry.

**10. The long tail.** Help/About text pages, the overview stat tables, the
interrupt screen (try alt-tabbing away mid-game), the HUD, quick keys.

## Things that are known and not bugs

- **There is no sound.** The original game is silent — no audio API in any
  class, zero audio assets. Not a port gap.
- **Wide mode is non-canonical.** `wide`/`wide10` composites clean gameplay
  only; menus and dialogues fall back to the canonical pillarboxed frame.
- **Some UI sits below the visible screen.** Several original elements
  (scroll arrows, some bottom bars) are drawn into a clipped band the 320-tall
  LCD never showed. That's faithful — the real game did it too.
