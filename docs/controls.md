# Controls — the real game on our FreeJ2ME build

Derived from the game's key handling (`b.java`) + our FreeJ2ME PC keymap
(`oracle/freej2me/src/org/recompile/freej2me/FreeJ2ME.java`). Use these when
play-testing the interactive window
(`java -cp oracle/freej2me/build org.recompile.freej2me.FreeJ2ME
"file:///<url-encoded-jar-path>" 240 320 2`).

## Keys (this FreeJ2ME build's mapping)

| PC key      | MIDP code | In-game effect |
|-------------|-----------|----------------|
| Arrow keys  | NUM2/8/4/6 | Move / navigate menus (up/down/left/right) |
| Enter       | NUM5 (53) | Attack · select/confirm · dismiss dialogue (fire) |
| **W**       | **22**    | Open the ACTION/ITEM menu (`b`: key==var_byte_a -> `n(); a((byte)2)`) |
| **Q**       | **21**    | Open the pause menu (`key==var_byte_b -> l(); a((byte)3)` — Save/Load/Continue) |
| 1..9 (top)  | NUM1..9   | Phone keypad. Default quick keys (`b.java:199` `var_byte_arr_f`): |
|  → 7        | NUM7 (55) |   Quick-use armed HEALING potion (`arr_f[0]`) — restores HP only |
|  → 9        | NUM9 (57) |   Quick-use armed MAGICKA potion (`arr_f[1]`) |
|  → 3        | NUM3 (51) |   Toggle weapon/attack mode (`arr_f[2]`) |
| E / R       | star/pound | (soft/extra) |
| Esc         | —         | FreeJ2ME config menu (NOT the game) |

The move/fire remap (`b.java` `a(I)I`, ~1728): up=NUM2, down=NUM8, left=NUM4,
right=NUM6, fire=NUM5; the quick keys are the configurable `var_byte_arr_f`
row (rebindable via the in-game Custom Controls menu, mode 5/20).

## Poison (and other DoT) — how to cure

A scamp's poison spell applies a damage-over-time effect TO THE PLAYER; killing
the caster does NOT remove it (correct behavior — matches the wiki warning).
The quick-heal key (7) restores HP but does not stop the poison ticking.
To CURE: **W** (item menu) -> Left/Right to the **Items** tab -> select
**Remove Poison** (lang 153; consumable #5, carried as inventory item `517` =
kind2<<8|5) -> Enter. Or use a healing potion / quick-heal (7) to out-pace the
damage if no cure is available. Item use is `h.b(j,int[])` (loop #17): vicar
heal / arm quick-slots / cure / the timed buff block.
