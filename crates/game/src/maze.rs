//! The op47 procedural maze generator (`b.a([I[III)V` + its four private
//! helpers + `m()`) — the L01 "Prison Sewers" second act (`/l01_1r.scr`).
//!
//! Transcribed from `b.javap.txt` (offsets 4588 = the generator, 3242 = the
//! carve walker, 3206 = the cell setter, 4302 = the 3x3 stamp, 3914 = the
//! edge decorator, 3047 = `m()`); CFR's rendering of all six methods was
//! verified instruction-level against the bytecode. The generator is a pure
//! function of the shared combat RNG (`b.var_java_util_Random_a` =
//! `world.rng`), the subtype-9 config row, the tag-20 pickup list
//! (`e.var_int_arr_g`) and the two script ints — which is exactly how the
//! oracle parity gate drives it (`setseed` + `callmaze` re-run the real
//! generator on both sides with zero frames in between).
//!
//! The subtype-9 row layout (l01_1r.scr row 1 =
//! `[1,30,30,5,8,15,14,13,16,9,12,11,10,114,4,5,30,7,15,25,0]`):
//! `[1]`/`[2]` map rows/cols (`b.f`/`b.g`), `[3]` wall tile, `[4]` floor
//! tile, `[5..=12]` the edge/corner decor tiles (left, right, down, up,
//! left+up, left+down, right+up, right+down), `[13]` the entry/exit marker
//! tile, `[14]` carve width, `[15]` max branches (`s` cap), `[16]` branch
//! chance (`nextInt() % [16] == 0`), `[17]` the subtype-0 enemy stat row,
//! `[18]` max placement coords consumed, `[19]` placement chance.
//!
//! Faithful quirks (all bytecode-pinned):
//! - Every array access bounds-checks the FLAT index only — a column one
//!   past the grid edge wraps onto the next row (the carve spans and the
//!   decorator's left/right neighbor reads both do this).
//! - The 3x3 stamp clears collision around the ENTRY but not the EXIT
//!   (the second loop writes the floor tile only).
//! - The branch spawn draws `abs(nextInt()) % 4 + 1` (abs BEFORE mod,
//!   re-rolled while == the current direction) but the branch coordinate
//!   and the step direction draw `abs(nextInt() % n)` (abs AFTER mod).
//! - A step that can't move (direction points away from the target)
//!   re-rolls the direction WITHOUT a done-check; a roll of 0 leaves the
//!   direction unchanged entirely.
//! - The `s` branch counter increments on every branch-chance hit even
//!   when the cap check then fails.

use crate::world::{ModelCache, World};
use formats::{JavaRandom, Tables};

/// The generator's per-run counters (`b.var_byte_s/t/u` + `var_int_arr_m`) —
/// instance fields in the original, but written only by this subsystem and
/// reset at the top of every generation.
struct Counters {
    /// `var_byte_s` — branch-chance hits (caps recursion at cfg[15]).
    s: i8,
    /// `var_byte_t` — placement-coordinate count (pairs into `m`, cap 50).
    t: i8,
    /// `var_byte_u` — carve-step count (placements only arm after 10).
    u: i8,
    /// `var_int_arr_m` — the (row, col) placement pairs.
    m: [i32; 50],
}

/// `b.a([BIII)V` (javap 3206) — set one cell: floor tile into the layer,
/// collision cleared. The bounds check is the flat index against the
/// COLLISION array (same length); a column overflow wraps onto the next row.
fn cell_set(layer: &mut [i8], collision: &mut [i8], g: i32, r: i32, c: i32, v: i32) {
    let i = r * g + c;
    if i >= collision.len() as i32 || i < 0 {
        return;
    }
    layer[i as usize] = v as i8;
    collision[i as usize] = 0;
}

/// `b.a([B[I[III[I)V` (javap 3242) — the recursive corridor walker: carve a
/// `width`-wide span at the cursor each step, walk one cell toward `target`
/// (direction re-rolled from the RNG), occasionally spawn a recursive branch
/// toward a random edge point, and record enemy-placement coordinates.
#[allow(clippy::too_many_arguments)]
fn walker(
    layer: &mut [i8],
    collision: &mut [i8],
    start: [i32; 2],
    target: [i32; 2],
    width: i32,
    dir0: i32,
    cfg: &[i32],
    f: i32,
    g: i32,
    rng: &mut JavaRandom,
    cnt: &mut Counters,
) {
    let mut cur = start; // nArray4 — a COPY; the caller's array is untouched
    let mut dir = dir0; // n4
    let mut done = false; // bl2
    while !done {
        // Branch spawn: the chance draw happens every iteration; the `s`
        // counter increments on every hit even when the cap then fails.
        if rng.next_int() % cfg[16] == 0 {
            cnt.s = cnt.s.wrapping_add(1);
            if i32::from(cnt.s) < cfg[15] {
                let mut nd = rng.next_int().wrapping_abs() % 4 + 1;
                while nd == dir {
                    nd = rng.next_int().wrapping_abs() % 4 + 1;
                }
                let n6 = (rng.next_int() % f.min(g)).wrapping_abs();
                let branch_target = match nd {
                    2 => [n6, 3],
                    1 => [n6, g - 3],
                    3 => [f - 3, n6],
                    _ => [3, n6], // nd == 4
                };
                walker(
                    layer,
                    collision,
                    cur,
                    branch_target,
                    width,
                    nd,
                    cfg,
                    f,
                    g,
                    rng,
                    cnt,
                );
            }
        }
        // Carve the span at the cursor + the placement roll. Directions 1/2
        // (column moves) span ACROSS rows; 3/4 (row moves) span across cols.
        if dir == 1 || dir == 2 {
            let place = rng.next_int() % cfg[19] == 0;
            cnt.u = cnt.u.wrapping_add(1);
            for i in 0..width {
                cell_set(layer, collision, g, cur[0] + i, cur[1], cfg[4]);
            }
            if place && i32::from(cnt.t) < 50 && i32::from(cnt.u) > 10 {
                cnt.m[cnt.t as usize] = cur[0] + 1;
                cnt.t += 1;
                cnt.m[cnt.t as usize] = cur[1];
                cnt.t += 1;
            }
        } else if dir == 3 || dir == 4 {
            let place = rng.next_int() % cfg[19] == 0;
            cnt.u = cnt.u.wrapping_add(1);
            for i in 0..width {
                cell_set(layer, collision, g, cur[0], cur[1] + i, cfg[4]);
            }
            if place && i32::from(cnt.t) < 50 && i32::from(cnt.u) > 10 {
                cnt.m[cnt.t as usize] = cur[0];
                cnt.t += 1;
                cnt.m[cnt.t as usize] = cur[1] + 1;
                cnt.t += 1;
            }
        }
        // One step toward the target: re-roll until a legal move lands. A
        // roll of 0 never remaps the direction; an illegal direction just
        // re-rolls (no done-check on that path).
        loop {
            let roll = (rng.next_int() % 4).wrapping_abs();
            if target[0] < cur[0] {
                if roll == 1 {
                    dir = 2;
                }
                if roll == 2 {
                    dir = 1;
                }
                if roll == 3 {
                    dir = 4;
                }
            } else if target[0] > cur[0] {
                if roll == 1 {
                    dir = 2;
                }
                if roll == 2 {
                    dir = 1;
                }
                if roll == 3 {
                    dir = 3;
                }
            } else {
                dir = if roll < 2 { 2 } else { 1 };
            }
            if dir == 1 && cur[1] < target[1] {
                cur[1] += 1;
                break;
            } else if dir == 3 && cur[0] < target[0] {
                cur[0] += 1;
                break;
            } else if dir == 2 && cur[1] > target[1] {
                cur[1] -= 1;
                break;
            } else if dir == 4 && cur[0] > target[0] {
                cur[0] -= 1;
                break;
            }
        }
        done = cur == target;
    }
    // The tail carve at the arrival cell — no placement roll here.
    if dir == 1 || dir == 2 {
        cnt.u = cnt.u.wrapping_add(1);
        for i in 0..width {
            cell_set(layer, collision, g, cur[0] + i, cur[1], cfg[4]);
        }
    } else if dir == 3 || dir == 4 {
        cnt.u = cnt.u.wrapping_add(1);
        for i in 0..width {
            cell_set(layer, collision, g, cur[0], cur[1] + i, cfg[4]);
        }
    }
}

/// The 3x3 neighborhood, in the bytecode's exact order.
const OFFS: [[i32; 2]; 9] = [
    [-1, 1],
    [-1, 0],
    [-1, -1],
    [0, 1],
    [0, 0],
    [0, -1],
    [1, 1],
    [1, 0],
    [1, -1],
];

/// `b.a([B[I[I[I)V` (javap 4302) — clear a 3x3 floor patch around the entry
/// (tile + collision) and the exit (tile ONLY — the exit's surround keeps
/// its collision, a faithful quirk).
fn stamp(
    layer: &mut [i8],
    collision: &mut [i8],
    entry: [i32; 2],
    exit: [i32; 2],
    cfg: &[i32],
    g: i32,
) {
    let len = layer.len() as i32;
    for o in OFFS {
        let i = (entry[0] + o[0]) * g + entry[1] + o[1];
        if i >= len || i < 0 {
            continue;
        }
        layer[i as usize] = cfg[4] as i8;
        collision[i as usize] = 0;
    }
    for o in OFFS {
        let i = (exit[0] + o[0]) * g + exit[1] + o[1];
        if i >= len || i < 0 {
            continue;
        }
        layer[i as usize] = cfg[4] as i8;
    }
}

/// `b.a([B[B[I)V` (javap 3914) — the wall-edge decorator: for every floor
/// cell of the base whose flat-index neighbors are in range, write the
/// matching edge/corner tile into the (zero-filled) top layer. Left/right
/// neighbor reads are flat-index too — they wrap across row ends (faithful).
fn decorate(base: &[i8], top: &mut [i8], cfg: &[i32], f: i32, g: i32) {
    let len = base.len() as i32;
    let at = |i: i32| i32::from(base[i as usize]);
    for r in 0..f {
        for c in 0..g {
            let cell = r * g + c;
            let left = r * g + (c - 1);
            let right = r * g + (c + 1);
            let up = (r - 1) * g + c;
            let down = (r + 1) * g + c;
            if cell > len - 1
                || cell < 0
                || left > len - 1
                || left < 0
                || right > len - 1
                || right < 0
                || up > len - 1
                || up < 0
                || down > len - 1
                || down < 0
                || at(cell) != cfg[4]
            {
                continue;
            }
            let v = if at(left) == cfg[3] {
                if at(up) == cfg[3] {
                    cfg[9]
                } else if at(down) == cfg[3] {
                    cfg[10]
                } else {
                    cfg[5]
                }
            } else if at(right) == cfg[3] {
                if at(up) == cfg[3] {
                    cfg[11]
                } else if at(down) == cfg[3] {
                    cfg[12]
                } else {
                    cfg[6]
                }
            } else if at(down) == cfg[3] {
                cfg[7]
            } else if at(up) == cfg[3] {
                cfg[8]
            } else {
                continue;
            };
            top[cell as usize] = v as i8;
        }
    }
}

/// `b.a([I[III)V` (javap 4588) — the op47 entry: rebuild the whole world as
/// a generated maze. `cfg` = the subtype-9 row, `slots9` = the tag-20 pickup
/// list (`e.var_int_arr_g`), `stat_row`/`model` = the subtype-0 enemy row
/// `cfg[17]` and its resolved model (`e.a(row[1])`), `n`/`n2` = the entry
/// action event and the exit enter event.
///
/// `m()`'s projection-cache rebuild (`var_short_arr_a`) has no Rust state —
/// the port projects per frame — but its actor-nulling, dirty flag, and the
/// player `h.a(j)`/`h.b(j)` re-init are applied. `max_actor` is faithfully
/// NOT reset (the original never touches `var_int_o` here; the spawner only
/// maxes it up).
#[allow(clippy::too_many_arguments)]
pub fn generate(
    world: &mut World,
    tables: &Tables,
    models: &mut ModelCache,
    cfg: &[i32],
    slots9: &[i32],
    stat_row: &[i32],
    model: &str,
    n: i32,
    n2: i32,
    class_cursor: i32,
) {
    // The prologue reset (offsets 21-111): layers dropped, actors 1.. nulled,
    // the player's summon link broken, counters + pickup count zeroed.
    for i in 1..world.actors.len() {
        world.actors[i] = None;
        world.actor_anims[i] = None;
    }
    if let Some(p) = world.actors[0].as_mut() {
        p.var_j_c = -1;
    }
    let mut cnt = Counters {
        s: 0,
        t: 0,
        u: 0,
        m: [0; 50],
    };
    world.pickup_count = 0; // var_byte_h = 0
    world.layers.clear();

    let (f, g) = (cfg[1], cfg[2]);
    world.map_w = f;
    world.map_h = g;
    let cells = (f * g) as usize;
    world.enter = vec![-1; cells];
    world.leave = vec![-1; cells];
    world.action = vec![-1; cells];
    world.collision = vec![1; cells];
    let mut base = vec![cfg[3] as i8; cells];

    let entry = [2, 2];
    let exit = [f - 2, g - 2];
    walker(
        &mut base,
        &mut world.collision,
        entry,
        exit,
        cfg[14],
        1,
        cfg,
        f,
        g,
        &mut world.rng,
        &mut cnt,
    );
    stamp(&mut base, &mut world.collision, entry, exit, cfg, g);

    let mut top = vec![0i8; cells];
    decorate(&base, &mut top, cfg, f, g);
    let mut markers = vec![0i8; cells];
    markers[(entry[0] * g + entry[1]) as usize] = cfg[13] as i8;
    markers[(exit[0] * g + exit[1]) as usize] = cfg[13] as i8;
    world.layers.push(base);
    world.layers.push(top);
    world.layers.push(markers);

    // The event overlays: exit enter-event = n2; the entry cell gets the
    // -2 enter/leave sentinels + action event n.
    world.enter[(exit[0] * g + exit[1]) as usize] = n2 as i8;
    world.leave[(entry[0] * g + entry[1]) as usize] = -2;
    world.enter[(entry[0] * g + entry[1]) as usize] = -2;
    world.action[(entry[0] * g + entry[1]) as usize] = n as i8;

    // m() (javap 3047): dirty flag, actors 1.. nulled (again), the player
    // re-inited in place (h.a(j) incl. the array-wide aggro sweep + h.b(j)).
    world.dirty = true;
    for i in 1..world.actors.len() {
        world.actors[i] = None;
        world.actor_anims[i] = None;
    }
    if world.actors[0].is_some() {
        for a in world.actors.iter_mut().flatten() {
            a.var_j_a = -1;
        }
        let p = world.actors[0].as_mut().unwrap();
        p.player_reset(tables);
        formats::resync_tiles(p);
    }

    // The spawn loop (offsets 585-728): each placement pair optionally drops
    // the next nonzero pickup from the tag-20 list, then spawns an enemy of
    // the cfg[17] stat row in the first free slot >= 2.
    let mut gidx = 0usize;
    let mut i = 0usize;
    while (i as i32) < i32::from(cnt.t) && (i as i32) < cfg[18] {
        if slots9[gidx] != 0 {
            world.drop_pickup(slots9[gidx], false, cnt.m[i], cnt.m[i + 1]);
            gidx += 1;
        }
        let mut slot = 2;
        while world.actors[slot].is_some() {
            slot += 1;
        }
        world.spawn(
            None,
            model,
            slot as i32,
            cnt.m[i] << 7,
            cnt.m[i + 1] << 7,
            stat_row,
            class_cursor,
            tables,
            models,
        );
        i += 2;
    }
}
