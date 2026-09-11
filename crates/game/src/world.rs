//! The `b.java` world state — the gameplay half of the shell: map layers,
//! the 25-slot actor array + spawner, camera, pickups, HUD text, dialogue,
//! and the per-frame actor loop from `run()` (b.java:1322). Transcribed from
//! the CFR decompile (line refs in doc comments); the pure actor/combat/tick
//! math lives in `formats` (oracle-validated per-method).
//!
//! Actor animation state: the original's `g.a(String)` returns a *shared*,
//! cached `d` per resource — actors with the same model share one animation
//! cursor. [`ModelCache`] reproduces that (one [`Anim`] per model name).

use formats::anim::Anim;
use formats::cml::parse_cml;
use formats::{Actor, Effects, JavaRandom, Tables, WorldEvent};
use std::collections::HashMap;
use std::path::PathBuf;

/// Logical screen center (`b.var_short_c/d` = 240/2, 345/2).
pub const CENTER_X: i32 = 120;
pub const CENTER_Y: i32 = 172;

/// `b.a(int[], int[])` — world -> iso/screen (verified shifts, `iso.rs`).
fn world_to_iso(p: [i32; 2]) -> [i32; 2] {
    let v = formats::world_to_screen(formats::Vec2i::new(p[0], p[1]));
    [v.x, v.y]
}

/// One cached model: the shared playback [`Anim`] plus the parsed `.cml`
/// (for frame geometry queries — `g.a(d,int)`/`g.b(d,int)`).
pub struct Model {
    pub anim: Anim,
    pub cml: formats::Cml,
}

/// One resolved drawable frame — what `g.a(Graphics, d, key, x, y)` reads
/// off the group's CURRENT frame node.
pub enum FrameRef {
    /// A static record (`f == 0`): the whole image drawn at `(x+off, y+off)`.
    Static {
        path: String,
        off_x: i32,
        off_y: i32,
        /// The node's `var_short_c` (the draw's return value; usually 0 for
        /// statics — the record flag block's `[3]`).
        width: i32,
    },
    /// An animated frame: a source sub-rect + offset + flip of the sheet.
    Rect {
        path: String,
        view: render::sprite::FrameView,
    },
}

/// Walk the records in Anim::from_cml's node order to the flag block +
/// record behind the anim node for `key`, at `anim`'s current cursor.
fn resolve_node<'a>(
    cml: &'a formats::Cml,
    anim: &Anim,
    key: i32,
) -> Option<(&'a formats::cml::CmlRecord, Option<&'a formats::cml::Flags>)> {
    let target = anim.lookup(key)?;
    let cursor = anim.current_frame(key)?;
    let mut node = 0usize;
    for rec in &cml.records {
        if rec.skipped {
            continue;
        }
        if rec.is_static {
            if node == target {
                return Some((rec, None));
            }
            node += 1;
        } else {
            for g in &rec.anim_groups {
                if node == target {
                    let f = &g.frames[cursor.min(g.frames.len().saturating_sub(1))];
                    return Some((rec, Some(f)));
                }
                node += 1;
            }
        }
    }
    None
}

/// `g.a(d, int)` / `g.b(d, int)` — the width/height of group `key`'s
/// *current* frame (its flag block's `[3]`/`[4]`); 0 for a missing group
/// (the original catches the NPE and returns 0).
pub fn frame_size_of(cml: &formats::Cml, anim: &Anim, key: i32) -> (i32, i32) {
    match resolve_node(cml, anim, key) {
        Some((_, Some(f))) => (f[3], f[4]),
        Some((rec, None)) => (rec.flags[3], rec.flags[4]),
        None => (0, 0),
    }
}

/// The group's current frame as a drawable [`FrameRef`], or `None` for a
/// missing group (the original draw returns 0 without drawing).
pub fn resolve_frame(cml: &formats::Cml, anim: &Anim, key: i32) -> Option<FrameRef> {
    let (rec, flags) = resolve_node(cml, anim, key)?;
    Some(match flags {
        None => FrameRef::Static {
            path: rec.path.clone(),
            off_x: rec.flags[5],
            off_y: rec.flags[6],
            width: rec.flags[3],
        },
        Some(f) => FrameRef::Rect {
            path: rec.path.clone(),
            view: render::sprite::FrameView::from_flags(f),
        },
    })
}

impl Model {
    pub fn frame_size(&self, key: i32) -> (i32, i32) {
        frame_size_of(&self.cml, &self.anim, key)
    }
    pub fn frame(&self, key: i32) -> Option<FrameRef> {
        resolve_frame(&self.cml, &self.anim, key)
    }
}

/// The shared per-model animation cache (`g`'s `d`-instance cache).
#[derive(Default)]
pub struct ModelCache {
    assets_dir: PathBuf,
    models: HashMap<String, Model>,
}

impl ModelCache {
    pub fn new(assets_dir: impl Into<PathBuf>) -> Self {
        Self {
            assets_dir: assets_dir.into(),
            models: HashMap::new(),
        }
    }

    /// `g.a(String)` — load-or-get the shared model for a resource name.
    ///
    /// Panics on a missing or unparseable resource: on the canonical path a
    /// bad model name is a transcription bug, and the panic is the fidelity
    /// tripwire (the port audit's ruling — the canonical path keeps its
    /// panics). Names that came from OUTSIDE the shipped assets — today only
    /// the user-editable save file — must clear [`Self::try_load`] first.
    pub fn get(&mut self, name: &str) -> &mut Model {
        if let Err(e) = self.try_load(name) {
            panic!("{e}");
        }
        self.models.get_mut(name).unwrap()
    }

    /// The fallible half of [`Self::get`]: ensure `name` is cached, reporting
    /// a bad resource name as `Err` instead of a panic. This is the
    /// validate-then-trust gate for untrusted model names — after it returns
    /// `Ok` the entry is cached, so the following `get` cannot fail.
    pub fn try_load(&mut self, name: &str) -> anyhow::Result<()> {
        if self.models.contains_key(name) {
            return Ok(());
        }
        let path = crate::asset::resource_path(&self.assets_dir, name)?;
        let bytes = std::fs::read(&path)
            .map_err(|e| anyhow::anyhow!("model resource {}: {e}", path.display()))?;
        let cml = parse_cml(&bytes).map_err(|e| anyhow::anyhow!("model cml parse: {e}"))?;
        let anim = Anim::from_cml(&cml);
        self.models.insert(name.to_string(), Model { anim, cml });
        Ok(())
    }

    /// The actor factory's box extent: `g.a(d, 1)` — group-1 current-frame width.
    pub fn frame_w(&mut self, name: &str) -> i32 {
        self.get(name).frame_size(1).0
    }

    /// Reset every loaded model's anim cursors (the normalized-shot harness).
    pub fn reset_all_anims(&mut self) {
        for m in self.models.values_mut() {
            m.anim.reset_all();
        }
    }
}

/// A HUD floating-text line (`b.a(String,int,int,int)`, b.java:2719).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudText {
    pub text: String,       // var_java_lang_String_b
    pub timeout_ms: i32,    // var_int_d (n * 1000)
    pub color: i32,         // var_int_f
    pub style: i8,          // var_byte_l (0 static, 1 blink, 2/3 slide)
    pub elapsed: i32,       // var_int_e
    pub x: i32,             // var_int_g (-1 = unset; positioned at first paint)
    pub y: i32,             // var_int_h
    pub blink_hidden: bool, // var_boolean_k
    pub blink_acc: i32,     // var_int_i
    pub slide_acc: i32,     // var_int_j
}

/// The open-dialogue state (`b.f(String)` + `b.i()`, b.java:3065/3114).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialogue {
    /// The wrapped lines (`var_java_util_Vector_b`).
    pub lines: Vec<String>,
    /// Scroll offset (`var_int_u`, -1 on open).
    pub scroll: i32,
    /// The box's inner text-window height (`var_int_w` = `min(b:S/2,
    /// frame_h(51)) - 4`, computed by `b.f(String)`).
    pub window_h: i32,
    /// All lines visible at the current scroll (`var_boolean_r`).
    pub shown_all: bool,
    /// Open-time accumulator standing in for the original's wall-clock
    /// `var_long_c` (FIRE dismisses only after >= 1000ms).
    pub open_ms: i32,
}

/// The `b.java` gameplay world.
pub struct World {
    // --- map (`void_b(String)` + the script writes) ---
    /// Map dims: `var_byte_f` (x extent) and `var_byte_g` (y extent — the
    /// column stride; tile index = `x * h + y`).
    pub map_w: i32,
    pub map_h: i32,
    /// Collision layer (`b.var_byte_arr_a`).
    pub collision: Vec<i8>,
    /// Visual tile layers (`b.var_java_util_Vector_a`).
    pub layers: Vec<Vec<i8>>,
    /// Event overlays: enter (`var_byte_arr_j`), leave (`var_byte_arr_k`),
    /// action (`b.var_byte_arr_c`, static).
    pub enter: Vec<i8>,
    pub leave: Vec<i8>,
    pub action: Vec<i8>,

    // --- actors ---
    pub actors: Vec<Option<Actor>>, // b.var_j_arr_a (25)
    /// Each actor's OWN animation instance (`j.var_d_a` — `g.a(String)`
    /// re-parses the model per call, so cursors are per-actor, NOT shared).
    pub actor_anims: Vec<Option<Anim>>,
    pub max_actor: i32, // var_int_o
    /// The persistent player (`b.var_j_a` survives level loads; slot 0 reuse).
    pub player_persists: bool,

    // --- camera + the paint's render state ---
    pub cam_follow: i8, // var_byte_q (-1 = free)
    pub view: [i32; 2], // var_int_arr_i (screen offset)
    pub dirty: bool,    // var_boolean_n
    /// The visible tile range (`q()`: `var_byte_arr_h` lo / `var_byte_arr_i` hi).
    pub range_lo: [i32; 2],
    pub range_hi: [i32; 2],
    /// The cached base-map offscreen (`var_javax_microedition_lcdui_Image_a`,
    /// re-rendered by `b(Graphics)` when `dirty`). The real loop paints every
    /// frame; our shell consumes `dirty` per frame into `base_stale` and only
    /// spends the pixels at render time.
    pub base_cache: Option<crate::fb::Fb>,
    pub base_stale: bool,

    // --- pickups (`a(int,boolean,int,int)`, b.java:2940) ---
    pub pickups: Vec<i8>,  // b.var_byte_arr_d triples (x, y, item)
    pub pickup_count: i32, // var_byte_h

    // --- respawn anchor (`d(int,int)`) ---
    pub respawn: [i16; 2], // var_short_i/j

    // --- flags ---
    pub input_unlocked: bool, // var_boolean_l (op19)
    /// `b.a(true)` consumes the key latch (`var_int_a = sentinel`); the shell
    /// owns the latch, so the unlock raises this flag for it.
    pub consume_key: bool,
    /// `b.void_a(0)` ran (the player-death sequence): the shell converts this
    /// to `set_mode(11)` immediately after the event drain — the real mode
    /// flip happens inside `void_a`, and the actor loop's per-slot mode check
    /// must see it the same frame.
    pub player_died: bool,
    pub combat_flag: bool, // var_boolean_c (op7)
    pub hud_enabled: bool, // var_boolean_e (op76)
    pub gold: i32,         // b.var_int_b

    // --- HUD text + dialogue ---
    pub hud: Option<HudText>,
    pub dialogue: Option<Dialogue>,
    /// The speaker-name prefix (`var_java_lang_String_f`, set by `b.g(String)`
    /// on camera follow, cleared by `g(null)`).
    pub speaker: Option<String>,

    // --- effects + RNG (`i`'s pool, `b.var_java_util_Random_a`) ---
    pub effects: Effects,
    pub rng: JavaRandom,
}

impl World {
    pub fn new() -> Self {
        Self {
            map_w: 0,
            map_h: 0,
            collision: Vec::new(),
            layers: Vec::new(),
            enter: Vec::new(),
            leave: Vec::new(),
            action: Vec::new(),
            actors: vec![None; 25],
            actor_anims: vec![None; 25],
            max_actor: 0,
            player_persists: false,
            cam_follow: -1,
            view: [0, 0],
            dirty: true,
            range_lo: [0, 0],
            range_hi: [0, 0],
            base_cache: None,
            base_stale: true,
            pickups: vec![0; 75],
            pickup_count: 0,
            respawn: [0, 0],
            input_unlocked: true,
            consume_key: false,
            player_died: false,
            combat_flag: true, // <clinit>: var_boolean_c = true
            hud_enabled: true,
            gold: 0,
            hud: None,
            dialogue: None,
            speaker: None,
            effects: Effects::new(),
            rng: JavaRandom::new(0),
        }
    }

    /// `b.void_b(String)` (b.java:662) — load a `.jtm` into the map state:
    /// dims, zeroed collision + cleared event overlays, the base layer into
    /// `layers[0]`… wait — the base grid goes to `b.var_byte_arr_a` (the
    /// COLLISION layer) and the remaining RLE grids into the layer Vector.
    /// Also clears the effects pool and (on the actor side) the level state.
    /// The parse itself is the validated `formats::jtm`. Ends in `m()` (the
    /// actor sweep — see [`World::map_load_actor_sweep`]), hence `tables`.
    pub fn load_map(&mut self, bytes: &[u8], tables: &Tables) -> anyhow::Result<()> {
        let map = formats::parse_jtm(bytes)?;
        self.map_w = map.width as i32;
        self.map_h = map.height as i32;
        let cells = (self.map_w * self.map_h) as usize;
        // First grid = the collision layer (b.var_byte_arr_a); the rest are
        // the visual layers (var_java_util_Vector_a).
        let mut grids = map.layers.into_iter();
        self.collision = grids
            .next()
            .map(|g| g.into_iter().map(|v| v as i8).collect())
            .unwrap_or_else(|| vec![0; cells]);
        self.layers = grids
            .map(|g| g.into_iter().map(|v| v as i8).collect())
            .collect();
        self.enter = vec![-1; cells];
        self.leave = vec![-1; cells];
        self.action = vec![-1; cells];
        // b.java:688: the map load zeroes the PICKUP count (var_byte_h
        // only — the [75] triple array persists). Invisible on a first
        // load, but a reload (the post-outro New Game) would otherwise
        // double the marker list (oracle-pinned).
        self.pickup_count = 0;
        self.effects.clear_all();
        self.dirty = true;
        self.map_load_actor_sweep(tables);
        Ok(())
    }

    /// `b.m()`'s actor tail (b.java:379-386), run at the end of `void_b(String)`:
    /// null actor slots 1..24, and — if the slot-0 player survives — re-init it
    /// (`h.a(j)` = the array-wide `var_j_a` back-ref sweep + the player
    /// re-init, then `h.b(j)` = tile resync). On a first load or an op29 chain
    /// load the array is already empty here (the `a(String)` script loader ran
    /// first, nulling every slot incl. 0), so both parts are no-ops and the
    /// following op15 respawns the player. It only bites on an IN-SCRIPT reload
    /// — a nested `op23 Call` back to entry 1 with no `a(String)` between, e.g.
    /// l02_2's Martin-death fail-reload — where the previous instance's actors
    /// are still live: this is what clears them. `max_actor` is NOT reset (the
    /// original never touches `var_int_o` in `m()`; the spawner only maxes up).
    pub fn map_load_actor_sweep(&mut self, tables: &Tables) {
        for i in 1..self.actors.len() {
            self.actors[i] = None;
            self.actor_anims[i] = None;
        }
        if self.actors[0].is_some() {
            for a in self.actors.iter_mut().flatten() {
                a.var_j_a = -1; // h.a(j): the array-wide aggro back-ref sweep
            }
            let p = self.actors[0].as_mut().unwrap();
            p.player_reset(tables);
            formats::resync_tiles(p); // h.b(j)
        }
    }

    /// The `a(String)` loader's world reset (b.java:313-348): map layers
    /// dropped, input unlocked, the combat flag re-armed (`var_boolean_c =
    /// true` — the spawner copies it into the player's collide flag), the
    /// camera/actor bookkeeping cleared; then null all slots (the player
    /// object survives in `var_j_a` for the spawner's slot-0 reuse) and break
    /// the player's summon link.
    pub fn reset_actors_for_load(&mut self) {
        self.input_unlocked = true;
        self.combat_flag = true;
        self.max_actor = 0;
        self.cam_follow = -1;
        self.view = [0, 0];
        if let Some(p) = self.actors[0].as_mut() {
            p.var_j_c = -1;
        }
        // var_j_a persists across the load; model: slot 0 stays in place and
        // `player_persists` marks it for the spawner's reuse path (the real
        // reuse: an op29 mid-level script chain re-spawning slot 0).
        self.player_persists = self.actors[0].is_some();
        let player = self.actors[0].take();
        let player_anim = self.actor_anims[0].take();
        for a in self.actors.iter_mut() {
            *a = None;
        }
        for a in self.actor_anims.iter_mut() {
            *a = None;
        }
        self.actors[0] = player;
        self.actor_anims[0] = player_anim;
        self.effects.clear_all();
    }

    /// `b.a(String, String, byte, int, int, int[])` (b.java:2538) — the actor
    /// spawner. Slot 0 with a persistent player reuses it (`h.void_a(j)`);
    /// otherwise builds a fresh actor from the model (factory `h.a(String,byte)`),
    /// class-inits the player from the class-select cursor, and applies the
    /// spawn stat row. Then teleports to `(x, y)`; slot 0 additionally becomes
    /// the camera-followed player. Returns the slot used.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &mut self,
        name: Option<&str>,
        model: &str,
        slot: i32,
        x: i32,
        y: i32,
        row: &[i32],
        class_cursor: i32,
        tables: &Tables,
        models: &mut ModelCache,
    ) -> Option<usize> {
        if !(0..25).contains(&slot) {
            return None;
        }
        let slot = slot as usize;
        if slot == 0 && self.player_persists && self.actors[0].is_some() {
            let p = self.actors[0].as_mut().unwrap();
            p.player_reset(tables); // h.a(var_j_a) — the void_a(j) re-init
                                    // (the array-wide var_j_a aggro sweep):
            for a in self.actors.iter_mut().flatten() {
                a.var_j_a = -1;
            }
        } else {
            let frame_w = models.frame_w(model);
            // h.a(String, byte): var_d_a = g.a(model) — a FRESH anim instance.
            self.actor_anims[slot] = Some(Anim::from_cml(&models.get(model).cml));
            let mut a = Actor::create(model, (slot + 1) as i8, frame_w);
            if slot == 0 {
                // h.a(j, e:[B[1] + 1, false) — class id from the class-select
                // cursor.
                a.class_init((class_cursor + 1) as i8, false, tables);
            }
            a.apply_stat_row(row, tables);
            a.display_name = name.map(str::to_string);
            self.actors[slot] = Some(a);
        }
        {
            let a = self.actors[slot].as_mut().unwrap();
            formats::set_position(a, x, y); // h.a(j, n, n2)
        }
        if slot == 0 {
            self.player_persists = true;
            let p = self.actors[0].as_mut().unwrap();
            p.var_byte_p = i8::from(self.combat_flag);
            p.var_byte_s = 0;
            p.var_byte_k = -1;
            let iso = p.var_int_arr_i;
            self.dirty = true;
            self.cam_follow = 0;
            self.view = [CENTER_X - iso[0], CENTER_Y - iso[1]];
        }
        self.max_actor = self.max_actor.max(slot as i32);
        Some(slot)
    }

    /// `b.void_a(int)` (b.java:2570) — remove an actor. Slot 0 is the PLAYER
    /// DEATH sequence: effects cleared, mode 11 (raised as [`Self::player_died`]
    /// — the shell converts it to `set_mode(11)` immediately after the drain),
    /// `h.a(var_j_a)` re-init + teleport to the op71 respawn anchor, HUD text
    /// cleared; the slot is NOT nulled.
    pub fn remove_actor(&mut self, slot: usize, tables: &Tables) {
        if self.actors[slot].is_none() {
            return;
        }
        if slot == 0 {
            self.effects.clear_all(); // i.a()
            self.player_died = true; // b.a((byte)11)
            let p = self.actors[0].as_mut().unwrap();
            p.player_reset(tables); // h.a(j)
            let (x, y) = (i32::from(self.respawn[0]), i32::from(self.respawn[1]));
            formats::set_position(p, x, y); // h.a(j, var_short_i, var_short_j)
            self.set_hud_text(None, 0, 0, 0); // b.a(null, 0, 0, 0)
            return;
        }
        if slot as i8 == self.cam_follow {
            self.speaker = None; // b.g(null)
        }
        self.actors[slot] = None;
        self.actor_anims[slot] = None;
        let mut n = slot as i32;
        if n == self.max_actor {
            while n > 0 && self.actors[n as usize].is_none() {
                n -= 1;
            }
            self.max_actor = n;
        }
    }

    /// `b.a(int, int, boolean)` — collision write (op22).
    pub fn set_collision(&mut self, x: i32, y: i32, solid: bool) {
        self.collision[(x * self.map_h + y) as usize] = i8::from(solid);
    }

    /// `b.a(int, int, int, int)` — visual tile write (op18) + dirty.
    pub fn set_tile(&mut self, x: i32, y: i32, layer: i32, tile: i32) {
        self.layers[layer as usize][(x * self.map_h + y) as usize] = tile as i8;
        self.dirty = true;
    }

    /// `b.a(int, int, int, int, int)` (b.java:2665) — event-overlay write
    /// (op50 region loop / op27 clear): enter, leave, action.
    pub fn set_overlay(&mut self, x: i32, y: i32, enter: i32, leave: i32, action: i32) {
        if self.enter.is_empty() {
            return;
        }
        let i = (x * self.map_h + y) as usize;
        self.enter[i] = enter as i8;
        self.leave[i] = leave as i8;
        self.action[i] = action as i8;
    }

    /// `b.a(int, boolean, int, int)` (b.java:2940) — drop a pickup marker:
    /// the top layer cell becomes -45 (item) / 22, and the (x, y, item)
    /// triple is appended to the pickup list.
    pub fn drop_pickup(&mut self, item: i32, marker: bool, x: i32, y: i32) {
        let cell = x * self.map_h + y;
        if self.pickup_count as usize >= self.pickups.len() - 4
            || cell < 0
            || cell >= (self.map_w * self.map_h)
        {
            return;
        }
        let top = self.layers.len() - 1;
        self.layers[top][cell as usize] = if marker { -45 } else { 22 };
        let c = self.pickup_count as usize;
        self.pickups[c] = x as i8;
        self.pickups[c + 1] = y as i8;
        self.pickups[c + 2] = item as i8;
        self.pickup_count += 3;
    }

    /// `b.c(int, int)` (b.java:2625) — camera hold at a world position
    /// (iso-transform, unfollow, clear the speaker).
    pub fn camera_hold(&mut self, x: i32, y: i32) {
        let iso = world_to_iso([x, y]);
        self.view = [CENTER_X - iso[0], CENTER_Y - iso[1]];
        self.dirty = true;
        self.cam_follow = -1;
        self.speaker = None; // b.g(null)
    }

    /// `b.b(int)` (b.java:2636) — camera follow an actor + speaker = its name.
    pub fn camera_follow(&mut self, slot: i32) {
        let Some(a) = self.actors.get(slot as usize).and_then(Option::as_ref) else {
            return;
        };
        self.cam_follow = slot as i8;
        self.dirty = true;
        self.view = [CENTER_X - a.var_int_arr_i[0], CENTER_Y - a.var_int_arr_i[1]];
        self.speaker = a.display_name.clone(); // b.g(name)
    }

    /// `b.a(String, int, int, int)` (b.java:2719) — set the HUD floating text.
    pub fn set_hud_text(&mut self, text: Option<String>, secs: i32, color_code: i32, style: i32) {
        self.hud = text.map(|t| HudText {
            text: t,
            timeout_ms: secs * 1000,
            color: match color_code {
                3 => 0x0000FF,
                5 => 0x00FF00,
                2 => 0xFF0000,
                1 => 0xFFFFFF,
                4 => 0xFFFF00,
                _ => 0,
            },
            style: style as i8,
            elapsed: 0,
            x: -1,
            y: -1,
            blink_hidden: false,
            blink_acc: 0,
            slide_acc: 0,
        });
    }

    /// `b.a(long)` (b.java:1391) — the HUD text timer: blink (style 1) /
    /// slide (2/3) then the timeout clear.
    pub fn hud_tick(&mut self, dt: i32) {
        let Some(h) = self.hud.as_mut() else { return };
        match h.style {
            1 => {
                if h.blink_acc >= 500 {
                    h.blink_hidden = !h.blink_hidden;
                    h.blink_acc = 0;
                }
                h.blink_acc += dt;
            }
            2 => {
                if h.slide_acc >= 50 {
                    h.x += 2;
                    if h.x == -1 {
                        h.x += 1;
                    }
                    h.slide_acc = 0;
                }
                h.slide_acc += dt;
            }
            3 => {
                if h.slide_acc >= 50 {
                    h.x -= 2;
                    if h.x == -1 {
                        h.x -= 1;
                    }
                    h.slide_acc = 0;
                }
                h.slide_acc += dt;
            }
            _ => {}
        }
        if h.elapsed > h.timeout_ms {
            self.hud = None;
        } else {
            h.elapsed += dt;
        }
    }

    /// Apply the deferred actor-tick [`WorldEvent`]s in emission order.
    /// Returns the script entries to push (the death triggers), in order —
    /// the caller feeds them to the `e` stack.
    pub fn apply_events(
        &mut self,
        events: Vec<WorldEvent>,
        tables: &Tables,
        models: &mut ModelCache,
    ) -> Vec<u8> {
        let mut pushes = Vec::new();
        for ev in events {
            match ev {
                WorldEvent::PushEntry(entry) => pushes.push(entry),
                WorldEvent::DropLoot { item, x, y } => self.drop_pickup(item, false, x, y),
                WorldEvent::RemoveActor(slot) => self.remove_actor(slot, tables),
                WorldEvent::Summon { caster, x, y } => {
                    // h.c:1524: b.var_b_a.a("/oh_scamp.cml", x, y, caster row):
                    // top-down free-slot scan, then the six-arg spawner.
                    let row = self.actors[caster]
                        .as_ref()
                        .and_then(|c| c.var_int_arr_o.clone())
                        .expect("summoner must carry its spawn stat row");
                    let mut slot = self.actors.len() as i32 - 1;
                    while slot >= 0 && self.actors[slot as usize].is_some() {
                        slot -= 1;
                    }
                    let spawned =
                        self.spawn(None, "/oh_scamp.cml", slot, x, y, &row, 0, tables, models);
                    if let Some(s) = spawned {
                        // var_j_c/var_j_d links + h.b(summon, false).
                        if let Some(c) = self.actors[caster].as_mut() {
                            c.var_j_c = s as i32;
                        }
                        let summon = self.actors[s].as_mut().unwrap();
                        summon.var_j_d = caster as i32;
                        summon.var_byte_s = 0;
                    }
                }
            }
        }
        pushes
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}
