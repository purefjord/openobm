//! Map collision detection — `b.java`'s per-actor collision sampling
//! (`h.java::boolean_a(j)` + its `(j, byte)` helper).
//!
//! An actor samples three tile cells (its collision-box corners). A cell blocks
//! depending on its collision-layer value: `0` = open, `1` = solid, and `2..=5`
//! are directional **slope** tiles resolved against the corner's sub-tile position
//! (`world % 128`). Out-of-bounds samples count as blocked.

use crate::Actor;

/// `h.boolean_a(j)` — true if the actor collides with the map at any of its three
/// sampled cells. `map` is the collision layer (`b.var_byte_arr_a`, signed bytes);
/// `width`/`height` are the map dims (`b.var_byte_g`/`b.var_byte_f`).
pub fn collides(a: &Actor, map: &[i8], width: i32, height: i32) -> bool {
    if a.var_byte_p == 0 {
        return false;
    }
    // Outer bounds checks (note: only these specific corners/axes are guarded,
    // exactly as the original).
    if a.var_byte_arr_b[0] < 0
        || i32::from(a.var_byte_arr_b[1]) >= width
        || i32::from(a.var_byte_arr_d[0]) >= height
        || a.var_byte_arr_d[1] < 0
    {
        return true;
    }
    cell_blocks(map, width, a.var_byte_arr_b, a.var_int_arr_b)
        || cell_blocks(map, width, a.var_byte_arr_c, a.var_int_arr_c)
        || cell_blocks(map, width, a.var_byte_arr_d, a.var_int_arr_d)
}

/// One sampled cell (`h.boolean_a(j, by)`): `tile` is the cell `[row, col]`,
/// `world` the corner's world position whose low 7 bits pick the slope side.
fn cell_blocks(map: &[i8], width: i32, tile: [i8; 2], world: [i32; 2]) -> bool {
    let idx = i32::from(tile[0]) * width + i32::from(tile[1]);
    // The original tests `> length` (an exact-length index would throw in Java);
    // valid play never reaches it, so we mirror the bound and index in range.
    if idx > map.len() as i32 {
        return true;
    }
    let n = map[idx as usize];
    if n == 0 {
        return false;
    }
    let n2 = world[0] % 128;
    let n3 = world[1] % 128;
    match n {
        1 => true,
        4 => n3 <= n2,
        3 => n3 >= n2,
        2 => n2 <= n3,
        5 => n2 >= n3,
        _ => false,
    }
}
