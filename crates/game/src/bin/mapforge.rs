//! `mapforge` — custom-map generator for the validated engine: builds a
//! `.jtm` (RLE writer, round-trip asserted against `formats::parse_jtm`)
//! plus a minimal `.scr` (entry table + bytecode assembled to the exact
//! operand encoding `e.java::b(long)` decodes), writes them into `assets/`,
//! then boots the shell and renders the result through the same path the
//! `levelmap` atlases use.
//!
//!     cargo run -p game --bin mapforge --release [-- palette|world]
//!
//! `world` (default) emits `/lush.jtm` + `/lush.scr` — the custom
//! walking-sim level (no enemies, no pickups, no triggers). `palette`
//! emits `/palette.jtm` + `/palette.scr`, a contact sheet laying out every
//! group key of `/l11_l11.cml` on a spaced grid (the stdout table maps
//! key -> tile coordinates) so tiles can be identified visually.
//!
//! Custom content only: nothing here touches shipped files or fixtures.

use anyhow::{ensure, Context, Result};
use game::shell::Shell;
use game::text::TextMasks;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// ---------------------------------------------------------------- tiles --

// The grass family (record 2 of /l11_l11.cml, ts_lvl11.png) — values proven
// by l06_a's op47 config row (wall/floor + the fringe decor `decorate`
// stamps where floor meets tall grass), identities confirmed on the
// mapforge montage. 33 is the maze's blocking hedge, but collision is a
// separate plane — here it is the walkable meadow ground.
const GRASS: u8 = 33; // full grass diamond
const FRINGE_L: u8 = 32;
const FRINGE_R: u8 = 28;
const FRINGE_D: u8 = 34;
const FRINGE_U: u8 = 31;
const FRINGE_LU: u8 = 27;
const FRINGE_LD: u8 = 30;
const FRINGE_RU: u8 = 29;
// The proven dirt-path floor diamond (record 1, ts_lvl9.png).
const DIRT: u8 = 72;
// Identified on the palette montage (record 1 + record 3 keys).
const TREE: u8 = 91; // blossoming green tree, 30x60
const COLUMN: u8 = 115; // round marble column
const COLUMN_SHORT: u8 = 120; // ornate short column
const PILLAR_BIG: u8 = 113; // thick square pillar
const POST: u8 = 67; // tall stone stack (the maze marker post)
const RUBBLE: &[u8] = &[68, 69, 70, 71, 73]; // broken stone bits
const SCRATCH: u8 = 111; // faint ground scuffs
const MUSHROOMS: u8 = 22; // yellow mushroom cluster
const TUFTS: &[u8] = &[25, 26, 28, 29, 31, 32, 34, 93]; // grass accents

/// One authored map: three grids indexed `x * h + y` like the engine.
struct MapGrids {
    w: usize,
    h: usize,
    collision: Vec<u8>,
    ground: Vec<u8>,
    top: Vec<u8>,
}

impl MapGrids {
    fn new(w: usize, h: usize, ground_fill: u8) -> Self {
        Self {
            w,
            h,
            collision: vec![0; w * h],
            ground: vec![ground_fill; w * h],
            top: vec![0; w * h],
        }
    }
    fn idx(&self, x: usize, y: usize) -> usize {
        x * self.h + y
    }
    fn set_ground(&mut self, x: usize, y: usize, v: u8) {
        let i = self.idx(x, y);
        self.ground[i] = v;
    }
    fn set_top(&mut self, x: usize, y: usize, v: u8) {
        let i = self.idx(x, y);
        self.top[i] = v;
    }
    /// Place a blocking object: top-layer sprite + collision.
    fn set_solid(&mut self, x: usize, y: usize, v: u8) {
        let i = self.idx(x, y);
        self.top[i] = v;
        self.collision[i] = 1;
    }
    fn ground_at(&self, x: usize, y: usize) -> u8 {
        self.ground[x * self.h + y]
    }
    fn top_at(&self, x: usize, y: usize) -> u8 {
        self.top[x * self.h + y]
    }
}

// ----------------------------------------------------------- jtm writer --

/// RLE-encode one grid in the loader's fill order (y outer, x inner over
/// `grid[x*h+y]`). Literal bytes for short runs; `0xFF, count, value` for
/// runs — value 255 would be ambiguous as a literal so it always goes
/// through the run form (unused by our palette anyway). Runs never span
/// layers; the caller emits each grid separately.
fn rle_encode_grid(out: &mut Vec<u8>, grid: &[u8], w: usize, h: usize) {
    let mut cells = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            cells.push(grid[x * h + y]);
        }
    }
    let mut i = 0;
    while i < cells.len() {
        let v = cells[i];
        let mut run = 1usize;
        while i + run < cells.len() && cells[i + run] == v && run < 255 {
            run += 1;
        }
        if run >= 4 || v == 0xFF {
            out.push(0xFF);
            out.push(run as u8);
            out.push(v);
        } else {
            for _ in 0..run {
                out.push(v);
            }
        }
        i += run;
    }
}

/// Serialize the three grids as a `.jtm` and assert the validated parser
/// reads back exactly what we meant (collision = grid 0, then the visual
/// layers — the same split `World::load_map` applies).
fn write_jtm(map: &MapGrids) -> Result<Vec<u8>> {
    ensure!(map.w <= 255 && map.h <= 255, "jtm dims are u8");
    let mut out = vec![map.w as u8, map.h as u8];
    rle_encode_grid(&mut out, &map.collision, map.w, map.h);
    rle_encode_grid(&mut out, &map.ground, map.w, map.h);
    rle_encode_grid(&mut out, &map.top, map.w, map.h);

    let parsed = formats::parse_jtm(&out).context("round-trip parse")?;
    ensure!(parsed.width == map.w && parsed.height == map.h, "dims");
    ensure!(parsed.layers.len() == 3, "layer count");
    ensure!(parsed.layers[0] == map.collision, "collision round-trip");
    ensure!(parsed.layers[1] == map.ground, "ground round-trip");
    ensure!(parsed.layers[2] == map.top, "top round-trip");
    Ok(out)
}

// ----------------------------------------------------------- scr writer --

/// Assemble the minimal level script, byte-encoded exactly as the VM's
/// operand readers consume it:
///
/// ```text
/// op73                       close the mode gate
/// op8  map model             load the .jtm + tileset .cml
/// op64 rgb                   gameplay clear color
/// op15 0xF19E 0 1 x y        spawn the player (slot 0, pristine row 1 —
///                            the same operands l06_a.scr uses; the row and
///                            its model string persist from startup.scr)
/// op71 x y                   respawn anchor
/// op74                       reopen the gate
/// op12                       mode 0 (gameplay)
/// op2                        return
/// ```
///
/// File layout per `e.void_a(String)`: `u8 entry_count`, entry records
/// `{id, off_hi, off_lo}` with absolute offsets, no sections (a non-30
/// terminator byte), 2 skipped bytes, then bytecode.
fn write_scr(map_name: &str, bg: u32, spawn: (u16, u16)) -> Result<Vec<u8>> {
    let mut code: Vec<u8> = Vec::new();
    let push_str = |code: &mut Vec<u8>, s: &str| {
        code.push(s.len() as u8);
        code.extend(s.bytes());
    };
    let push_u16 = |code: &mut Vec<u8>, v: u16| {
        code.push((v >> 8) as u8);
        code.push(v as u8);
    };

    code.push(73);
    code.push(8);
    push_str(&mut code, map_name);
    push_str(&mut code, "/l11_l11.cml");
    code.push(64);
    code.push((bg >> 16) as u8);
    code.push((bg >> 8) as u8);
    code.push(bg as u8);
    code.push(15);
    push_u16(&mut code, 0xF19E); // lang 61854 name ref (l06_a's player spawn)
    code.push(0); // slot 0
    code.push(1); // subtype-0 row 1
    push_u16(&mut code, spawn.0);
    push_u16(&mut code, spawn.1);
    code.push(71);
    push_u16(&mut code, spawn.0);
    push_u16(&mut code, spawn.1);
    code.push(74);
    code.push(12);
    code.push(2);

    // Header: 1 entry record; code starts after count + record + the
    // section terminator + 2 skipped bytes = 7.
    let code_start = 7u16;
    let mut out = vec![
        1,
        1,
        (code_start >> 8) as u8,
        code_start as u8,
        0, // non-30 byte: ends the (empty) section list
        0,
        0, // the two skipped bytes
    ];
    out.extend(&code);

    let parsed = formats::parse_scr(&out).context("scr parse-back")?;
    ensure!(parsed.entry(1) == Some(0), "entry 1 at code offset 0");
    ensure!(parsed.code == code, "code region round-trip");
    ensure!(parsed.sections.is_empty(), "no sections");
    Ok(out)
}

// -------------------------------------------------------------- palette --

/// Every group key `/l11_l11.cml` defines (from the eso-tools cml dump).
const PALETTE_KEYS: &[u8] = &[
    22, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79,
    80, 81, 82, 83, 84, 85, 86, 87, 88, 91, 93, 111, 112, 113, 115, 120, 200, 201, 202, 203, 204,
    211, 212,
];

/// Contact sheet: each key on a dirt field, 3 tiles apart, 8 per row.
fn build_palette() -> MapGrids {
    let cols = 8usize;
    let rows = PALETTE_KEYS.len().div_ceil(cols);
    let mut m = MapGrids::new(cols * 6 + 6, rows * 6 + 6, DIRT);
    println!("palette grid (key -> tile x,y):");
    for (i, &key) in PALETTE_KEYS.iter().enumerate() {
        let (cx, cy) = (3 + (i % cols) * 6, 3 + (i / cols) * 6);
        m.set_top(cx, cy, key);
        println!("  {key:>3} -> ({cx},{cy})");
    }
    m
}

// ---------------------------------------------------------------- world --

/// Tiny deterministic LCG so layouts are reproducible run to run.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

const W: usize = 60;
const H: usize = 60;

/// Carve a wobbling dirt trail from `a` to `b`: step toward the target
/// with sideways jitter, painting the cell + one randomized neighbor
/// (paths punch through decor: top cleared, collision cleared).
fn carve_trail(m: &mut MapGrids, rng: &mut Lcg, a: (usize, usize), b: (usize, usize)) {
    let (mut x, mut y) = (a.0 as i32, a.1 as i32);
    let (tx, ty) = (b.0 as i32, b.1 as i32);
    let mut guard = 0;
    while (x, y) != (tx, ty) && guard < 4000 {
        guard += 1;
        for (px, py) in [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1), (x - 1, y)] {
            let inside = px >= 3 && py >= 3 && px < W as i32 - 3 && py < H as i32 - 3;
            // 2x2 block always (clean trail interior against the iso
            // overdraw of neighboring grass); the 5th cell is jitter.
            let jitter = px < x;
            if inside && (!jitter || rng.below(3) == 0) {
                let i = m.idx(px as usize, py as usize);
                m.ground[i] = DIRT;
                m.top[i] = 0;
                m.collision[i] = 0;
            }
        }
        // Bias 2:1 toward the target, jitter otherwise.
        let dx = (tx - x).signum();
        let dy = (ty - y).signum();
        let r = rng.below(4);
        let (sx, sy) = match r {
            0 | 1 => {
                if (tx - x).abs() >= (ty - y).abs() {
                    (dx, 0)
                } else {
                    (0, dy)
                }
            }
            2 => (
                if dx != 0 {
                    dx
                } else {
                    1 - 2 * rng.below(2) as i32
                },
                0,
            ),
            _ => (
                0,
                if dy != 0 {
                    dy
                } else {
                    1 - 2 * rng.below(2) as i32
                },
            ),
        };
        x = (x + sx).clamp(3, W as i32 - 4);
        y = (y + sy).clamp(3, H as i32 - 4);
    }
}

/// The maze `decorate` rule, repurposed: grass tufts in the top layer of
/// dirt cells that border meadow grass, softening the trail edges.
fn fringe_trails(m: &mut MapGrids) {
    for x in 1..W - 1 {
        for y in 1..H - 1 {
            if m.ground_at(x, y) != DIRT || m.top_at(x, y) != 0 {
                continue;
            }
            let g = |xx: usize, yy: usize| m.ground_at(xx, yy) == GRASS;
            let (l, r, u, d) = (g(x - 1, y), g(x + 1, y), g(x, y - 1), g(x, y + 1));
            let v = if l {
                if u {
                    FRINGE_LU
                } else if d {
                    FRINGE_LD
                } else {
                    FRINGE_L
                }
            } else if r {
                // right+down shares the plain-right tile in the maze cfg
                if u {
                    FRINGE_RU
                } else {
                    FRINGE_R
                }
            } else if d {
                FRINGE_D
            } else if u {
                FRINGE_U
            } else {
                continue;
            };
            m.set_top(x, y, v);
        }
    }
}

/// The lush walking-sim world: a tree-walled meadow with dirt trails
/// linking a spawn glade, a blossom grove, a ruined marble temple and a
/// wildflower meadow. No enemies, no pickups, no triggers — just ground,
/// decor and collision.
fn build_world() -> MapGrids {
    let mut m = MapGrids::new(W, H, GRASS);
    let mut rng = Lcg(0x6C757368); // "lush"

    // --- the tree wall: map edge hard-fenced, two jittered tree rings ---
    for x in 0..W {
        for y in 0..H {
            let ring = x.min(y).min(W - 1 - x).min(H - 1 - y);
            match ring {
                0 | 1 => {
                    let i = m.idx(x, y);
                    m.collision[i] = 1;
                    if (x + y) % 2 == 0 {
                        m.top[i] = TREE;
                    }
                }
                2 => {
                    if rng.below(100) < 55 {
                        m.set_solid(x, y, TREE);
                    }
                }
                3 => {
                    if rng.below(100) < 18 {
                        m.set_solid(x, y, TREE);
                    }
                }
                _ => {}
            }
        }
    }

    // --- the spawn glade: a dirt clearing with a column gate north ---
    let glade = (30usize, 44usize);
    for x in 24..37 {
        for y in 39..50 {
            let (dx, dy) = (x as i32 - glade.0 as i32, y as i32 - glade.1 as i32);
            if dx * dx + dy * dy <= 12 {
                m.set_ground(x, y, DIRT);
            }
        }
    }
    m.set_solid(27, 40, COLUMN);
    m.set_solid(33, 40, COLUMN);
    for (mx, my) in [(26, 47), (34, 46), (31, 49)] {
        m.set_top(mx, my, MUSHROOMS);
    }

    // --- the blossom grove (NW): a jittered tree lattice ---
    for gx in 0..7 {
        for gy in 0..7 {
            let (x, y) = (6 + gx * 3, 5 + gy * 3);
            if rng.below(100) < 82 {
                let (jx, jy) = (rng.below(2) as usize, rng.below(2) as usize);
                let (x, y) = (x + jx, y + jy);
                if m.ground_at(x, y) == GRASS && m.top_at(x, y) == 0 {
                    m.set_solid(x, y, TREE);
                }
            }
        }
    }
    for _ in 0..10 {
        let (x, y) = (5 + rng.below(21) as usize, 4 + rng.below(21) as usize);
        if m.ground_at(x, y) == GRASS && m.top_at(x, y) == 0 {
            m.set_top(x, y, MUSHROOMS);
        }
    }

    // --- the ruined temple (NE): a packed-earth floor strewn with broken
    // slabs (the ts5 checkered tile 112 has black baked above its diamond,
    // so a contiguous pavement blacks itself out — ruins read better anyway)
    let (tx0, tx1, ty0, ty1) = (40usize, 53usize, 7usize, 18usize);
    for x in tx0..=tx1 {
        for y in ty0..=ty1 {
            let rim = x == tx0 || x == tx1 || y == ty0 || y == ty1;
            if !(rim && rng.below(100) < 30) {
                m.set_ground(x, y, DIRT);
            }
        }
    }
    for x in tx0 + 1..tx1 {
        for y in ty0 + 1..ty1 {
            if m.top_at(x, y) != 0 {
                continue;
            }
            let roll = rng.below(100);
            if roll < 22 {
                m.set_top(x, y, RUBBLE[rng.below(RUBBLE.len() as u32) as usize]);
            } else if roll < 34 {
                m.set_top(x, y, SCRATCH);
            }
        }
    }
    for x in (tx0..=tx1).step_by(2) {
        for &y in &[ty0, ty1] {
            if rng.below(100) < 68 {
                m.set_solid(x, y, COLUMN);
            } else if rng.below(100) < 60 {
                m.set_top(x, y, RUBBLE[rng.below(RUBBLE.len() as u32) as usize]);
            }
        }
    }
    for &y in &[ty0 + 2, ty0 + 4, ty1 - 2, ty1 - 4] {
        for &x in &[tx0, tx1] {
            if rng.below(100) < 68 {
                m.set_solid(x, y, COLUMN_SHORT);
            }
        }
    }
    for &(x, y) in &[(tx0, ty0), (tx1, ty0), (tx0, ty1), (tx1, ty1)] {
        m.set_solid(x, y, PILLAR_BIG);
    }
    // altar + scuffs inside
    m.set_solid(46, 12, PILLAR_BIG);
    m.set_top(45, 13, SCRATCH);
    m.set_top(47, 12, SCRATCH);
    m.set_top(44, 11, RUBBLE[2]);
    m.set_top(49, 14, RUBBLE[0]);
    // gate posts where the trail will arrive
    m.set_solid(45, ty1 + 1, POST);
    m.set_solid(48, ty1 + 1, POST);

    // --- landmarks: a weathered stone circle + a mushroom fairy ring ---
    let ring = [
        (0i32, -3i32),
        (2, -2),
        (3, 0),
        (2, 2),
        (0, 3),
        (-2, 2),
        (-3, 0),
        (-2, -2),
    ];
    for (i, (dx, dy)) in ring.iter().enumerate() {
        let (x, y) = ((33 + dx) as usize, (13 + dy) as usize);
        if m.top_at(x, y) == 0 {
            m.set_solid(x, y, if i % 2 == 0 { POST } else { COLUMN_SHORT });
        }
        let (fx, fy) = ((20 + dx * 2 / 3) as usize, (32 + dy * 2 / 3) as usize);
        if m.ground_at(fx, fy) == GRASS && m.top_at(fx, fy) == 0 {
            m.set_top(fx, fy, MUSHROOMS);
        }
    }
    m.set_top(33, 13, SCRATCH);
    m.set_top(32, 12, RUBBLE[4]);

    // --- the wildflower meadow (SE): lone trees + dense tufts ---
    for _ in 0..7 {
        let (x, y) = (38 + rng.below(16) as usize, 34 + rng.below(18) as usize);
        if m.ground_at(x, y) == GRASS && m.top_at(x, y) == 0 {
            m.set_solid(x, y, TREE);
        }
    }

    // --- trails: glade -> grove, glade -> temple gate, glade -> meadow ---
    carve_trail(&mut m, &mut rng, glade, (15, 15));
    carve_trail(&mut m, &mut rng, (32, 41), (46, 20));
    carve_trail(&mut m, &mut rng, (33, 45), (46, 44));
    carve_trail(&mut m, &mut rng, (15, 15), (28, 10));

    // --- scatter: tufts + the odd mushroom over plain meadow ---
    for x in 3..W - 3 {
        for y in 3..H - 3 {
            if m.ground_at(x, y) != GRASS || m.top_at(x, y) != 0 || m.collision[m.idx(x, y)] != 0 {
                continue;
            }
            let roll = rng.below(100);
            if roll < 14 {
                m.set_top(x, y, TUFTS[rng.below(TUFTS.len() as u32) as usize]);
            } else if roll < 15 {
                m.set_top(x, y, MUSHROOMS);
            }
        }
    }
    fringe_trails(&mut m);

    // The spawn cell must be open (the glade circle guarantees it, but be
    // explicit: the engine drops the player exactly here).
    let i = m.idx(glade.0, glade.1);
    m.collision[i] = 0;
    m.top[i] = 0;
    m
}

/// Crop a per-key thumbnail from the rendered palette sheet into a montage
/// grid (8 per row, PALETTE_KEYS order) — same iso anchor math as
/// `render_full_map` (TILE_W=32, TILE_H=16, pads 32/128 left/top).
fn montage(fb: &game::fb::Fb, map: &MapGrids) -> game::fb::Fb {
    let cell_iso = |x: i32, y: i32| {
        let v = formats::world_to_screen(formats::Vec2i::new(x * 128, y * 128));
        (v.x - 16, v.y)
    };
    let (mut min_x, mut min_y) = (i32::MAX, i32::MAX);
    for x in 0..map.w as i32 {
        for y in 0..map.h as i32 {
            let (ix, iy) = cell_iso(x, y);
            min_x = min_x.min(ix);
            min_y = min_y.min(iy);
        }
    }
    let view = (32 - min_x, 128 - min_y);

    const SLOT_W: i32 = 100;
    const SLOT_H: i32 = 124;
    let cols = 8i32;
    let rows = (PALETTE_KEYS.len() as i32 + cols - 1) / cols;
    let mut out = game::fb::Fb::new(cols * SLOT_W, rows * SLOT_H);
    out.fill(0x102008);
    for (i, _) in PALETTE_KEYS.iter().enumerate() {
        let (cx, cy) = (3 + (i % 8) * 6, 3 + (i / 8) * 6);
        let (ax, ay) = cell_iso(cx as i32, cy as i32);
        let (ax, ay) = (ax + view.0, ay + view.1);
        let (sx, sy) = ((i as i32 % cols) * SLOT_W, (i as i32 / cols) * SLOT_H);
        for dy in 0..SLOT_H - 4 {
            for dx in 0..SLOT_W - 4 {
                let (px, py) = (ax - 32 + dx, ay - 72 + dy);
                if px >= 0 && py >= 0 && px < fb.w && py < fb.h {
                    out.set(sx + 2 + dx, sy + 2 + dy, fb.get(px, py));
                }
            }
        }
    }
    out
}

fn main() -> Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "world".into());
    let assets = root().join("assets");
    let out_dir = root().join("artifacts/lush");
    std::fs::create_dir_all(&out_dir)?;

    let (map, jtm_name, scr_name, png_name, spawn, with_actors) = match mode.as_str() {
        "palette" | "montage" => (
            build_palette(),
            "/palette.jtm",
            "/palette.scr",
            "palette.png",
            (300u16, 300u16),
            false,
        ),
        "world" => (
            build_world(),
            "/lush.jtm",
            "/lush.scr",
            "lush.png",
            (30 * 128 + 64, 44 * 128 + 64), // the glade center, tile (30,44)
            true,
        ),
        other => anyhow::bail!("unknown mode {other:?} (palette|world)"),
    };

    let jtm = write_jtm(&map)?;
    let scr = write_scr(jtm_name, 0x1B3A10, spawn)?;
    std::fs::write(assets.join(jtm_name.trim_start_matches('/')), &jtm)?;
    std::fs::write(assets.join(scr_name.trim_start_matches('/')), &scr)?;
    println!(
        "{jtm_name}: {} bytes, {scr_name}: {} bytes",
        jtm.len(),
        scr.len()
    );

    // Boot the real shell and render the whole map through the validated
    // paint (same pre-roll the levelmap atlases use).
    let masks = TextMasks::load(&root().join("tests/fixtures/oracle/text_masks.txt"))
        .expect("text masks fixture");
    let mut shell = Shell::boot(&assets, masks)?;
    game::script::drive(
        &mut shell,
        "timescale 10\nwait 5000\ntap fire\nwait 1000\ntap fire\nwait 500\ntap fire\nwait 20000\n",
    )?;
    game::script::drive(&mut shell, &format!("callscript {scr_name}\nwait 5000\n"))?;
    let fb = shell.render_level_map(with_actors)?;
    let out = out_dir.join(png_name);
    fb.save_png(&out)?;
    println!("{}x{} px -> {}", fb.w, fb.h, out.display());
    if mode == "montage" {
        let m = montage(&fb, &map);
        let mout = out_dir.join("montage.png");
        m.save_png(&mout)?;
        println!(
            "montage: 8 per row in key order {:?} -> {}",
            PALETTE_KEYS,
            mout.display()
        );
    }

    // World mode: also walk the player around through the real input path
    // and save LCD-sized shots — spawn view, then a stroll toward each
    // compass area (verifies movement, collision and the camera follow).
    if mode == "world" {
        let walk = "wait 1000\nshot spawn\n\
                    press 50\nwait 2500\nrelease 50\nshot walk_up\n\
                    press 52\nwait 2500\nrelease 52\nshot walk_left\n\
                    press 56\nwait 3500\nrelease 56\nshot walk_down\n\
                    press 54\nwait 5000\nrelease 54\nshot walk_right\n";
        let arts = game::script::drive(&mut shell, walk)?;
        for (name, art) in arts {
            if let game::script::Artifact::Frame(fb) = art {
                let p = out_dir.join(format!("{name}.png"));
                fb.save_png(&p)?;
                println!("shot {} -> {}", name, p.display());
            }
        }
        let p = shell.world.actors[0].as_ref().expect("player");
        println!(
            "player at world ({}, {}) after the stroll",
            p.var_int_arr_b[0], p.var_int_arr_b[1]
        );
    }
    Ok(())
}
