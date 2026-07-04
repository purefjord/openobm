//! The `b.java` front-end state machine, VM-driven: the `var_byte_m` mode +
//! `k:B` menu page + per-page cursor, with `run()`'s frame order, the `b(J)`
//! input dispatch, the `a(byte)` set-mode transitions, and `b.paint` for the
//! covered modes. Transcribed from `b.javap.txt` (`run()` at 8874, `b(J)` at
//! 11679, `a(byte)` at 11199, `l()` at 2374, `a(String)` loader at 2876,
//! `b.e()` at 16940, `b.b(II)` at 16432; see `docs/loop-decode-notes.md`).
//!
//! Boot is REAL: the ctor path (`blt.Main`: `new b(midlet, "/startup.scr",
//! "/oh_menu.cml", version)`) runs the loader on `/startup.scr` and the M7
//! script VM then drives the logo pages (op10 -> `b.b(anim,color)`), the legal
//! scroll (anim 4 -> mode 21), the title key-gate (op60), the script chain
//! (op29 -> loader on `/startup2.scr`), the lang load (op56), and the menu
//! entry (op44 -> `b.e()`). Nothing on this path is seeded or modeled.
//!
//! The class fire runs the REAL level load (`a("/l01_1.scr")`, mode
//! 6->15->10->0) through the `b`/`e` world layer ([`crate::world`] +
//! [`crate::vm`]): the whole L01 choreography (map, spawns, class init +
//! equips, overlays, camera, dialogue) is ported and state-validated
//! byte-for-byte against the real runtime (`tests/level_load.rs`).
//!
//! Explicit fences (everything leaving the slice is loud, never guessed):
//! - the gameplay/please-wait/intro-page PAINTS (`render()` bails for modes
//!   0/15/10 — the state slice landed first; screenshot parity is next);
//! - the in-game `n()` action menu (mode 2) and the quick heal/fatigue keys
//!   yield [`Leave::GameKey`]; Custom Controls (mode 5), the overview stat
//!   tables (mode 18), Save/Load/overwrite (13/14/16) and the shop yield
//!   [`Leave::Mode`]; the player-death screen (mode 11) asserts in
//!   `World::remove_actor`; the mode-9 outro end-transition is loud;
//! - `b()Z` (RecordStore has-save probe) is modeled as `false` — the pinned
//!   wiped-RMS baseline (no "Continue" item); the save-capture slice lifts it;
//! - mode 4 (About) renders fenced — its credits roll always draws a scroll
//!   ARROW (`.cml` frame render, unported) and is animated/visual-only;
//! - the in-game `f.java` menus (`f.a:B == 1`) stay out of slice.

use crate::asset::Assets;
use crate::fb::Fb;
use crate::paint::{
    paint_exit_dialog, paint_loader, paint_menu_page, paint_startup, paint_text_page, SCREEN_H,
    SCREEN_W,
};
use crate::text::TextMasks;
use crate::vm::{GameVm, KEY_SENTINEL};
use crate::world::{Dialogue, ModelCache, World};
use crate::wrap::build_pages;
use formats::cml::{parse_cml, Cml};
use formats::lang::Lang;
use formats::vm::Step;
use formats::{Actor, MapRef};
use std::path::PathBuf;

/// `a(int)` remapped input codes (up=3 down=4 left=5 right=6 fire=7).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Fire,
}

impl Action {
    /// `b.a(int)`: raw MIDP keycode (or game-action) -> internal code. Returns
    /// `None` for keys the menu ignores (the `-1122868` default).
    pub fn from_key(key: i32) -> Option<Action> {
        match key {
            1 | 50 => Some(Action::Up),
            6 | 56 => Some(Action::Down),
            2 | 52 => Some(Action::Left),
            5 | 54 => Some(Action::Right),
            8 | 20 | 53 => Some(Action::Fire),
            _ => None,
        }
    }
}

/// What a fire did when it leaves the ported slice (fenced, not yet ported).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Leave {
    /// a menu item that opens an unported mode (Save/Load/shop/…)
    Mode(u8),
    /// an in-game key that opens an unported screen (the `n()` action menu,
    /// the quick-heal/quick-fatigue use)
    GameKey(i32),
}

/// A readable view of the front-end screens for tests (`b.m:B` + `k:B`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Title,        // m=8 (key-gate set)
    MainMenu,     // m=3, k=0
    ClassSelect,  // m=3, k=1
    HelpTopics,   // m=3, k=6 (the Help submenu carousel)
    ExitDialog,   // m=19
    ControlsPage, // m=17 (Basic Controls text page)
    OverviewPage, // m=23 (Game Overview text page)
    AboutRoll,    // m=4 (animated credits — visual-only, render fenced)
    PleaseWait,   // m=15 (level-load anim — visual-only)
    IntroText,    // m=10 (the level intro page, auto-scrolls into mode 0)
    Gameplay,     // m=0
}

pub struct Shell {
    assets: Assets,
    masks: TextMasks,
    assets_dir: PathBuf,
    vm: GameVm,
    /// The gameplay world (`b`'s map/actor/camera/HUD state) + the shared
    /// per-model anim cache (`g`).
    pub world: World,
    pub models: ModelCache,
    lang: Lang,
    lang_loaded: bool, // a.a:Z (base-load idempotence)
    version: String,   // d:String (MIDlet-Version, spliced by h())

    mode: i8, // m:B (static mode)
    /// `b.a:Z` — the script mode-change gate: op73 closes it (every `a(byte)`
    /// early-returns), op74 reopens it. `<clinit>` starts it open.
    mode_gate: bool,
    anim: i32,                    // m:I (startup anim id)
    bg: u32,                      // n:I (startup background + op64 gameplay bg)
    model: Option<Cml>,           // b:Ld (op43 UI model, var_d_b)
    ui_model: Option<String>,     // var_d_b's resource name (dialogue frame dims)
    level_model: Option<String>,  // var_d_a (op8's level model, c(String))
    level_bg: u32,                // b.var_int_c (op64, the mode-0 clear color)
    page: i8,                     // k:B (menu page)
    page_saved: i8,               // x:B (the page Help was entered from)
    topic: u16,                   // l:S (help-topic lang id — 17/23 title)
    cursors: [usize; 7],          // e:[B (per-page cursors)
    pages: Vec<Vec<String>>,      // a:[[String (menu page table, from l())
    text_pages: Vec<Vec<String>>, // a:[Ljava/util/Vector; (from h())
    scroll: i16,                  // g:S (text-page scroll)
    scroll_acc: i16,              // h:S (scroll dt accumulator)
    end_latched: bool,            // p:Z (17/23 end-of-text latch; blocks DOWN)
    end_debounce: bool,           // b:Z (end-of-text 2-paint debounce toggle)
    blink_ms: i32,                // l:I (-1 disarmed; 500 saw-tooth)
    blink_on: bool,               // i:Z
    latched: i32,                 // a:I (raw keycode; sentinel idle)
    released: bool,               // p:B (keyReleased seen: consume at the tail)
    banner: bool,                 // c:Image != null (/main.png)
    left_gameplay: bool,          // f:Z
    progress: i8,                 // r:B (loader bar; -1 outside b.c(int))
    pending_leave: Option<Leave>, // fenced boundary crossings
    exited: bool,                 // notifyDestroyed() fired (b.c(), YES on exit)
    pw_acc: i16,                  // k:S (the mode-15 please-wait anim timer)
    /// The key bindings `g:[B` (quick-health, quick-magika, toggle-weapon;
    /// defaults `f:[B = {55, 57, 51}` — keys 7/9/3).
    bindings: [i32; 3],
}

impl Shell {
    /// The ctor path: field defaults (`<clinit>` + `j()`), then the loader on
    /// the initial script — the real boot, no seeding. `assets_dir` is the
    /// extracted-jar resource root; `masks` the oracle text fixture.
    pub fn boot(assets_dir: impl Into<PathBuf>, masks: TextMasks) -> anyhow::Result<Self> {
        let assets_dir = assets_dir.into();
        let mut shell = Self {
            assets: Assets::new(&assets_dir),
            masks,
            models: ModelCache::new(&assets_dir),
            world: World::new(),
            assets_dir,
            vm: GameVm::new(),
            lang: Lang::default(),
            lang_loaded: false,
            version: "1.0.10".into(), // MIDlet-Version (jar manifest)
            mode: -1,                 // <clinit>: m = -1
            mode_gate: true,          // <clinit>: a:Z = true
            anim: 0,
            bg: 0,
            model: None,
            ui_model: None,
            level_model: None,
            level_bg: 0xFF0000, // <clinit>: c:I = 0xFF0000
            page: -1,
            page_saved: 0,
            topic: 0,
            cursors: [0; 7],
            pages: vec![Vec::new(); 7],
            text_pages: Vec::new(),
            scroll: 0,
            scroll_acc: 0,
            end_latched: false,
            end_debounce: true, // <clinit>: b:Z = true
            blink_ms: -1,
            blink_on: true,
            latched: KEY_SENTINEL,
            released: false,
            banner: false,
            left_gameplay: false,
            progress: -1,
            pending_leave: None,
            exited: false,
            pw_acc: 0,
            bindings: [55, 57, 51],
        };
        shell.loader("/startup.scr")?;
        Ok(shell)
    }

    /// `b.a(String)` — the loader: mode 6, reset the per-level state (dialogue
    /// closed, actors nulled with the player object surviving, effects
    /// cleared), then `e.void_a(name)` parses the script and pushes entry 1.
    /// Resource-load progress (`b.c(int)`: mode-7 flips + `r:B`) is
    /// latency-driven on the real device and instant here; `r:B` stays -1
    /// (empty bar), visual-only.
    fn loader(&mut self, name: &str) -> anyhow::Result<()> {
        self.set_mode(6);
        self.page = -1;
        self.latched = KEY_SENTINEL;
        self.progress = -1;
        self.scroll = SCREEN_H as i16
            - 8 * self
                .masks
                .metrics(crate::text::GameFont::SmallBold)
                .midp_height as i16;
        self.scroll_acc = 0;
        // b.a(String) 330-348: dialogue closed, per-level actor reset.
        self.world.dialogue = None;
        self.world.reset_actors_for_load();
        let path = self.assets_dir.join(name.trim_start_matches('/'));
        let bytes = std::fs::read(&path)
            .map_err(|e| anyhow::anyhow!("loading script {}: {e}", path.display()))?;
        self.vm.load(&bytes)
    }

    /// `b.a(byte)` — the only general mode-transition function. The `!a:Z`
    /// guard is the script mode-change gate (op73 closes / op74 reopens): a
    /// closed gate swallows EVERY transition.
    fn set_mode(&mut self, new: i8) {
        if !self.mode_gate {
            return;
        }
        if self.mode == 12 {
            return;
        }
        let old = self.mode;
        if old == 0 {
            self.left_gameplay = true;
        }
        self.mode = new;
        if new == 3 && !self.banner {
            if !self.left_gameplay && self.page != 4 {
                self.banner = true; // createImage("/main.png")
            }
        } else if old != 22 {
            self.banner = false; // c:Image = null
        }
        match new {
            9 => {
                self.left_gameplay = false;
                self.text_pages = self.h(self.lang.get(547));
            }
            4 => {
                // BYTECODE CORRECTION (a(byte) offset 134–147): the credits
                // scroll starts at b:S - 4 * c:Font height — c:Font is SMALL
                // BOLD (=305), not the medium font the recon prose said.
                self.text_pages = self.h(self.lang.get(548));
                let small_h = self
                    .masks
                    .metrics(crate::text::GameFont::SmallBold)
                    .midp_height;
                self.scroll = SCREEN_H as i16 - 4 * small_h as i16;
                self.scroll_acc = 0;
            }
            21 => {
                // h(new a().a()) — the /copywrite.txt legal text
                let legal = std::fs::read_to_string(self.assets_dir.join("copywrite.txt"))
                    .expect("copywrite.txt");
                self.text_pages = self.h(&legal);
                self.scroll = 0;
            }
            23 => {
                self.text_pages = self.h(self.lang.get(574));
                self.scroll = 15;
                self.end_latched = false; // p:Z = 0
            }
            17 => {
                self.text_pages = self.h(self.lang.get(465));
                self.scroll = 15;
                self.end_latched = false; // p:Z = 0
            }
            _ => {}
        }
    }

    /// `b.h(String)` — build the text-page model (see `wrap`).
    fn h(&self, text: &str) -> Vec<Vec<String>> {
        build_pages(&self.masks, text, &self.version)
    }

    /// `b.l()` — build the menu page table. Page 0 (main) has no "Continue":
    /// `b()Z` (RecordStore probe) is fenced `false` on the wiped-RMS baseline.
    /// Page 1 (class select) walks the persistent script class table.
    fn l(&mut self) {
        let g = |id: u16| self.lang.get(id).to_string();
        self.pages[0] = vec![g(2), g(456), g(6), g(22)];
        self.pages[5] = vec![g(21), g(2), g(456), g(6), g(22)];
        self.pages[1] = self.vm.class_name_ids().iter().map(|&id| g(id)).collect();
        // page 6 = the Help submenu (build order verified in l() bytecode):
        // Basic/Custom Controls, Game/Classes/Weapons/Armor/Spells/Items
        self.pages[6] = [457u16, 458, 573, 522, 459, 460, 461, 462]
            .iter()
            .map(|&id| g(id))
            .collect();
        // pages 2/3 (debug level list) and 4 (lang 18/19/20 settings) are
        // out of slice: left empty, and rendering an empty page is a loud
        // index panic rather than a wrong frame.
    }

    /// `b.e()` — the op44 native: menu tables, mode 3, main page.
    fn e_menu(&mut self) {
        self.l();
        self.set_mode(3);
        self.page = 0;
    }

    /// Resolve an opcode text operand: `0xF___` = lang id, else a string-pool
    /// index (the operand was decoded as an inline string — `step.strings[0]`).
    fn step_text(&self, step: &Step, n16: i32) -> String {
        if (n16 & 0xF000) == 0xF000 {
            self.lang.get((n16 & 0xFFF) as u16).to_string()
        } else {
            step.strings.first().cloned().unwrap_or_default()
        }
    }

    /// Apply one executed script opcode's `b`-side effect (the op set the
    /// startup + L01 level scripts use; anything else is a loud fence).
    fn apply(&mut self, step: &Step) {
        let op = |i: usize| step.operands.get(i).copied().unwrap_or(0);
        match step.opcode {
            2 | 11 => {} // return / wait: handled by GameVm
            60 => {}     // key gate: set by GameVm
            23 => {}     // call: handled by ScriptVm's control flow
            3 => {
                // b.g(null); b.f(text); b.i() — a modal dialogue with the
                // speaker cleared.
                self.world.speaker = None;
                let text = self.step_text(step, op(0));
                self.dialogue_open(&text);
            }
            7 => self.world.combat_flag = op(0) == 1,
            8 => {
                // b.void_b(map) + b.c(model): the level map + level model.
                let map = self
                    .assets_dir
                    .join(step.strings[0].trim_start_matches('/'));
                let bytes = std::fs::read(&map).expect("op8 map resource");
                self.world.load_map(&bytes).expect("op8 jtm parse");
                let model = step.strings[1].clone();
                self.models.get(&model);
                self.level_model = Some(model);
            }
            10 => {
                // b.b(anim, color): m:I, n:I, mode 21 for anim 4 else 8
                self.anim = op(0);
                self.bg = op(1) as u32;
                self.set_mode(if self.anim == 4 { 21 } else { 8 });
            }
            12 => self.set_mode(0),
            14 => self.vm.set_handler(op(0), Some(op(1))),
            28 => self.vm.set_handler(op(0), None),
            15 => {
                // The actor spawner — operands (n16 name, n10 slot, n7 row,
                // x, y): name (0 = none), the WORKING subtype-0 row, model =
                // the string pool at row[1].
                let name = if op(0) != 0 {
                    Some(self.step_text(step, op(0)))
                } else {
                    None
                };
                let row = self
                    .vm
                    .tables
                    .row(0, op(2))
                    .expect("op15 stat row")
                    .to_vec();
                let model = self.vm.pool_string(row[1]).to_string();
                self.world.spawn(
                    name.as_deref(),
                    &model,
                    op(1),
                    op(3),
                    op(4),
                    &row,
                    self.cursors[1] as i32,
                    &self.vm.tables,
                    &mut self.models,
                );
            }
            17 => {
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    a.set_walk_target(op(1), op(2));
                }
            }
            41 => {
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    let y = a.var_int_arr_b[1];
                    a.set_walk_target(op(1), y);
                }
            }
            42 => {
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    let x = a.var_int_arr_b[0];
                    a.set_walk_target(x, op(1));
                }
            }
            18 => self.world.set_tile(op(0), op(1), op(2), op(3)),
            22 => self.world.set_collision(op(0), op(1), op(2) == 1),
            19 => {
                // b.a(boolean): the input lock; unlocking consumes the latch.
                self.world.input_unlocked = op(0) == 1;
                if op(0) == 1 {
                    self.latched = KEY_SENTINEL;
                }
            }
            20 => {
                // b.void_a(int): remove an actor (slot 0 = the mode-11 player
                // death screen — fenced inside remove_actor).
                self.world.remove_actor(op(0) as usize);
            }
            21 => {
                // The actor wait-list (var_int_arr_e).
                let list = step.operands[1..].to_vec();
                self.vm.set_wait_actors(list);
            }
            24 => {
                let slot = op(0) as usize;
                if let Some(a) = self.world.actors[slot].as_mut() {
                    let name = a.model_name.clone();
                    a.set_anim(op(1) as i8, Some(&mut self.models.get(&name).anim));
                }
            }
            25 => self.world.camera_hold(op(0), op(1)),
            26 => self.world.camera_follow(op(0)),
            27 => self.world.set_overlay(op(0), op(1), 255, 255, 255),
            32 => {
                // h.c(actor, n, n2): the death-trigger entry (var_byte_k).
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    a.var_byte_k = op(2) as i8;
                }
            }
            33 => {
                // h.void_a(actor, n): clear the death trigger (n unused).
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    a.var_byte_k = -1;
                }
            }
            34 => {
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    a.attr_set(op(1), op(2), &self.vm.tables);
                }
            }
            36 => {
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    formats::set_position(a, op(1), op(2));
                }
            }
            37 | 38 => {
                // equip (37) / unequip (38) by kind: 0 -> weapons (subtype 4),
                // 1 -> armor (subtype 1), 2 -> consumables (subtype 2).
                let subtype = match op(1) {
                    1 => 1u8,
                    2 => 2,
                    0 => 4,
                    k => panic!("op37/38 kind {k} out of range"),
                };
                let row = self
                    .vm
                    .tables
                    .row(subtype, op(2))
                    .expect("op37/38 item row")
                    .to_vec();
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    if step.opcode == 37 {
                        a.equip(op(1), &row, false, &self.vm.tables);
                    } else {
                        a.unequip(op(1), &row, &self.vm.tables);
                    }
                }
            }
            39 => {
                let text = self.step_text(step, op(0));
                self.world.set_hud_text(Some(text), op(1), op(2), op(3));
            }
            40 => self.world.set_hud_text(None, 0, 0, 0),
            43 => {
                // b.d(String): b:Ld = g.a(name)
                let name = &step.strings[0];
                let path = self.assets_dir.join(name.trim_start_matches('/'));
                let bytes = std::fs::read(&path).expect("op43 model resource");
                self.model = Some(parse_cml(&bytes).expect("op43 cml parse"));
                self.models.get(name);
                self.ui_model = Some(name.clone());
            }
            44 => self.e_menu(),
            46 => {
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    a.set_facing(op(1) as i8);
                }
            }
            49 => self.world.drop_pickup(op(0), true, op(1), op(2)),
            50 => {
                // Region overlay install: (x1, y1, x2, y2, enter, leave, action).
                for x in op(0)..=op(2) {
                    for y in op(1)..=op(3) {
                        self.world.set_overlay(x, y, op(4), op(5), op(6));
                    }
                }
            }
            51 => {
                for x in op(0)..=op(2) {
                    for y in op(1)..=op(3) {
                        self.world.set_overlay(x, y, 255, 255, 255);
                    }
                }
            }
            52 => self.vm.set_sequencer(op(0), op(1), [op(2), op(3)]),
            53 => {
                // NPC dialogue: face-reset arm, camera follow (sets the
                // speaker), the actor's facing, then the dialogue box.
                let slot = op(0);
                self.vm.set_face_reset(slot);
                self.world.camera_follow(slot);
                if let Some(a) = self.world.actors[slot as usize].as_mut() {
                    a.set_facing(op(1) as i8);
                }
                let text = self.step_text(step, op(2));
                self.dialogue_open(&text);
            }
            56 => {
                // b.a(String, int): lang load; id 0/65535 = the base table,
                // idempotent once loaded (a.a:Z); other ids load the overlay
                // table (base wins on lookup). The path string is vestigial
                // (class a reads /lang_<id>.txt).
                let id = step.operands[1];
                if id == 0 || id == 65535 {
                    if !self.lang_loaded {
                        let bytes =
                            std::fs::read(self.assets_dir.join("lang_0.txt")).expect("lang_0.txt");
                        self.lang = Lang::load_base(&bytes);
                        self.lang_loaded = true;
                    }
                } else {
                    let file = format!("lang_{id}.txt");
                    let bytes = std::fs::read(self.assets_dir.join(&file)).expect("overlay lang");
                    let overlay = formats::parse_lang_file(&bytes, id as u8)
                        .unwrap_or_else(|| panic!("unknown lang table id {id}"));
                    self.lang.set_overlay(overlay);
                }
            }
            58 => self.vm.scale_row(op(0), op(1)),
            67 => self.vm.restore_row(op(0)),
            59 => {
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    a.var_byte_s = i8::from(op(1) == 1);
                }
            }
            64 => self.level_bg = op(0) as u32, // b.var_int_c (gameplay clear)
            65 => {
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    a.grow_level(op(1), &self.vm.tables);
                }
            }
            66 => {
                // b.e(String): the intro text page — h(text) + mode 10.
                let text = self.step_text(step, op(0));
                self.text_pages = self.h(&text);
                self.set_mode(10);
            }
            68 | 69 => {
                let kind = match op(0) {
                    0 => 8,
                    1 => 9,
                    2 => 10,
                    k => k,
                };
                let lifetime = if step.opcode == 69 { op(3) * 1000 } else { 0 };
                self.world.effects.spawn_world(kind, op(1), op(2), lifetime);
            }
            70 => self.world.effects.clear_at(op(0), op(1)),
            71 => self.world.respawn = [op(0) as i16, op(1) as i16],
            73 => {
                self.set_mode(15);
                self.mode_gate = false;
            }
            74 => self.mode_gate = true,
            75 => {
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    a.var_byte_u = i8::from(a.var_byte_u != 1);
                }
            }
            76 => self.world.hud_enabled = op(0) == 1,
            78 => {
                if let Some(a) = self.world.actors[op(0) as usize].as_mut() {
                    a.var_byte_z = op(1) as i8;
                }
            }
            29 => {
                // e.b(long) case 29: b.a(String) — chain to the next script.
                // The loader resets the VM stack, so the caller's trailing ops
                // never run (faithful: startup.scr's op2 after op29 is dead).
                let name = step.strings[0].clone();
                self.loader(&name).expect("op29 script chain");
            }
            72 => {} // free cached graphics: no resource effect (validated M7)
            other => unimplemented!("script op{other} not wired to b (out of slice)"),
        }
    }

    /// The key-name table lookup (`var_java_lang_String_arr_b[code]`).
    fn key_name(&self, code: i32) -> String {
        match code {
            48..=57 => format!("# {}", code - 48),
            35 => self.lang.get(286).to_string(),
            42 => self.lang.get(287).to_string(),
            1 => self.lang.get(281).to_string(),
            6 => self.lang.get(282).to_string(),
            2 => self.lang.get(283).to_string(),
            5 => self.lang.get(284).to_string(),
            8 => self.lang.get(285).to_string(),
            other => panic!("key name for code {other} not in the table"),
        }
    }

    /// `b.f(String)` + `b.i()` — open a dialogue: wrap the text at the box
    /// width (screen − 10 − the group-54 frame width − 13, measured in the
    /// small-bold font, with the speaker-name prefix), then arm the open state
    /// (the VM halts; FIRE ≥ 1s dismisses).
    fn dialogue_open(&mut self, text: &str) {
        let ui = self
            .ui_model
            .clone()
            .expect("dialogue needs the op43 UI model (var_d_b)");
        let arrow_w = self.models.get(&ui).frame_size(54).0;
        let width = (SCREEN_W - 10) - arrow_w - 13; // var_int_v
                                                    // The key-name substitutions (l(): var_java_lang_String_arr_b):
                                                    // ACTION_KEY = arr_b[8] (lang 285); the bound quick/toggle keys map
                                                    // through the same table (digits 48..57 -> "# N", '#' 35 -> lang 286,
                                                    // '*' 42 -> lang 287; game actions 1/6/2/5/8 -> lang 281..285).
        let subs = [
            self.lang.get(285).to_string(),
            self.key_name(self.bindings[2]),
            self.key_name(self.bindings[0]),
            self.key_name(self.bindings[1]),
        ];
        let lines =
            crate::wrap::wrap_dialogue(&self.masks, text, width, &subs, &mut self.world.speaker);
        self.world.dialogue = Some(Dialogue {
            lines,
            scroll: -1,
            shown_all: true,
            open_ms: 0,
        });
    }

    /// One frame of `run()` (dt in ms), in the original loop order: effects +
    /// VM tick (mode-gated), input dispatch, blink, the HUD-text timer, the
    /// mode-0 actor loop (with the player's trigger/pickup sampling), the
    /// text-scroll timers, and the mode-15 please-wait anim step.
    pub fn tick(&mut self, dt_ms: i32) {
        if self.mode == 12 {
            return;
        }
        // f.a:B == 1 (the in-game f.java menus) is out of slice.
        if !matches!(self.mode, 3 | 10 | 9 | 13) {
            // i.a(l): the effect pool (projectile hits share the combat RNG).
            // The /oh_magic.cml model is the original `i.<clinit>` load.
            let mut events = Vec::new();
            {
                let model = self.models.get("/oh_magic.cml");
                let World {
                    effects,
                    actors,
                    rng,
                    ..
                } = &mut self.world;
                effects.update(
                    i64::from(dt_ms),
                    &mut model.anim,
                    actors,
                    &mut self.vm.tables,
                    &mut events,
                    rng,
                );
            }
            self.drain_events(events);
            // e.a(l): one opcode (the prologue guards run against the world).
            let shop = self.mode == 1;
            if let Some(step) = self.vm.tick(dt_ms, &mut self.world, &mut self.models, shop) {
                self.apply(&step);
            }
            if self.world.consume_key {
                self.world.consume_key = false;
                self.latched = KEY_SENTINEL;
            }
        }
        self.dialogue_age(dt_ms);
        self.input(dt_ms); // this.b(J)
        if self.blink_ms >= 0 {
            self.blink_ms -= dt_ms;
            if self.blink_ms <= 0 {
                self.blink_ms = 500;
                self.blink_on = !self.blink_on;
            }
        }
        // this.a(J): the HUD floating-text timer.
        self.world.hud_tick(dt_ms);
        // The mode-0 actor loop (run() 1322).
        if self.mode == 0 {
            self.actor_loop(dt_ms);
        }
        if matches!(self.mode, 9 | 10 | 4 | 21) {
            if self.scroll_acc > 100 {
                self.scroll -= 1;
                self.scroll_acc = 0;
            }
            self.scroll_acc = self.scroll_acc.wrapping_add(dt_ms as i16);
        }
        // Mode 15: the please-wait anim step (g.a(c:Ld, 5) every 200ms;
        // c:Ld = the ctor-loaded /oh_pc.cml).
        if self.mode == 15 {
            self.pw_acc = (i32::from(self.pw_acc) + dt_ms) as i16;
            if self.pw_acc >= 200 {
                self.models.get("/oh_pc.cml").anim.advance(5);
                self.pw_acc = 0;
            }
        }
        // paint-side blink arming (paint mode 8 runs every frame on the real
        // loop; our render only runs at shots, so arm here — same frame).
        if self.mode == 8 && self.vm.key_gate && self.blink_ms == -1 {
            self.blink_ms = 500;
        }
        // Paint-side end-of-text check for the intro page: the real loop
        // paints every frame and the m=10 -> m=0 transition lives in the paint
        // tail; our render is fenced for mode 10, so evaluate the (pure)
        // final-line y here each frame instead.
        if self.mode == 10 {
            let fy = self.text_final_y();
            self.text_page_end(fy)
                .expect("the mode-10 end transition is ported");
        }
    }

    /// The text-page paint's final line y, computed without painting: `3 +
    /// g:S` plus one pitch (`smallH + 1`) per wrapped line (an empty paragraph
    /// still advances one pitch). Mode 10 never takes the m=21 skip-first or
    /// per-paint decrement.
    fn text_final_y(&self) -> i32 {
        let small_h = self
            .masks
            .metrics(crate::text::GameFont::SmallBold)
            .midp_height;
        let pitch = small_h + 1;
        let mut y = 3 + i32::from(self.scroll);
        for para in &self.text_pages {
            y += pitch * (para.len().max(1) as i32);
        }
        y
    }

    /// Track a dialogue's age for the FIRE-dismiss rule (the original uses
    /// wall-clock `var_long_c`; dt-accumulation is the virtual equivalent).
    fn dialogue_age(&mut self, dt_ms: i32) {
        if let Some(d) = self.world.dialogue.as_mut() {
            d.open_ms += dt_ms;
        }
    }

    /// The `run()` mode-0 per-actor loop (b.java:1322): tick every live slot
    /// (`h.a(actor, l, !dialogue && unlocked)`); for the player, sample the
    /// enter/leave overlays and push the change-edge trigger entries, show the
    /// `-2` action hint (lang 24), and the pickup-proximity hint (lang 363).
    fn actor_loop(&mut self, dt_ms: i32) {
        let mut events = Vec::new();
        for n in 0..=(self.world.max_actor as usize) {
            if self.mode != 0 {
                break;
            }
            if self.world.actors[n].is_none() {
                continue;
            }
            let (mut by, mut by2) = (0i8, 0i8);
            if n == 0 {
                let p = self.world.actors[0].as_ref().unwrap();
                by = p.var_byte_l;
                by2 = p.var_byte_m;
            }
            let bl = self.world.dialogue.is_none() && self.world.input_unlocked;
            let model_name = self.world.actors[n].as_ref().unwrap().model_name.clone();
            {
                let model = if model_name.is_empty() {
                    None
                } else {
                    Some(&mut self.models.get(&model_name).anim)
                };
                let World {
                    actors,
                    rng,
                    effects,
                    collision,
                    layers,
                    map_h,
                    ..
                } = &mut self.world;
                let map = MapRef {
                    base: layers.first().map(Vec::as_slice).unwrap_or(&[]),
                    coll: collision,
                    height: *map_h,
                };
                Actor::tick(
                    n,
                    actors,
                    rng,
                    i64::from(dt_ms),
                    bl,
                    model,
                    &mut self.vm.tables,
                    effects,
                    Some(&map),
                    &mut events,
                );
            }
            // Apply this actor's deferred events before the next slot ticks
            // (a summon spawned into a higher slot is ticked this same frame,
            // exactly like the original's in-place array write).
            self.drain_events(std::mem::take(&mut events));
            if self.world.actors[n].is_none() || n != 0 {
                continue;
            }
            // The player's overlay sampling + trigger edge (run() 1333).
            let by3 = {
                let World {
                    actors,
                    enter,
                    leave,
                    map_h,
                    ..
                } = &mut self.world;
                actors[0]
                    .as_mut()
                    .unwrap()
                    .sample_overlay(enter, leave, *map_h)
            };
            if by3 != by {
                if by2 != 0 && by2 != -1 && by2 != -2 {
                    events.push(formats::WorldEvent::PushEntry(by2 as u8));
                }
                if by3 != 0 && by3 != -1 && by3 != -2 {
                    events.push(formats::WorldEvent::PushEntry(by3 as u8));
                } else if by3 == -2 {
                    let hint = self.lang.get(24).to_string();
                    self.world.set_hud_text(Some(hint), 60, 4, 0);
                }
            }
            // Pickup proximity (run() 1344): the lang-363 hint while within
            // 350 of any pickup marker; cleared when out of range.
            let hint363 = self.lang.get(363).to_string();
            let mut near = false;
            {
                let p_pos = self.world.actors[0].as_ref().unwrap().var_int_arr_b;
                let showing = self.world.hud.as_ref().map(|h| h.text.clone());
                let hint_or_none = showing.as_deref().is_none_or(|t| t == hint363);
                if hint_or_none {
                    for i in (0..self.world.pickup_count as usize).step_by(3) {
                        let pos = [
                            i32::from(self.world.pickups[i]) << 7,
                            i32::from(self.world.pickups[i + 1]) << 7,
                        ];
                        if formats::combat_distance(&pos, &p_pos).abs() < 350 {
                            near = true;
                            break;
                        }
                    }
                    if near {
                        self.world.set_hud_text(Some(hint363.clone()), 60, 4, 0);
                    }
                }
                let still_other = self.world.hud.as_ref().is_some_and(|h| h.text != hint363);
                if !still_other && !near {
                    self.world.set_hud_text(None, 0, 0, 0);
                }
            }
        }
        self.drain_events(events);
    }

    /// Apply the deferred actor-tick world events (trigger pushes into the
    /// script stack; loot drops; the summon spawner).
    fn drain_events(&mut self, events: Vec<formats::WorldEvent>) {
        let pushes = self
            .world
            .apply_events(events, &self.vm.tables, &mut self.models);
        for entry in pushes {
            self.vm.push_entry(entry);
        }
    }

    /// `keyPressed` + `keyReleased` (a tap): latch the raw keycode with the
    /// release flag set — the next frame's `b(J)` dispatches once and the
    /// tail consumes it (`p:B` -> sentinel).
    pub fn press(&mut self, key: i32) {
        self.latched = key;
        self.released = true;
    }

    /// `keyPressed` without a release: the latch persists, so `b(J)`
    /// re-dispatches EVERY frame (how held-key scrolling works — the tail
    /// only consumes when `p:B` is set; mode 3 forces `p:B=1` per key, which
    /// is why menu cursors do NOT auto-repeat).
    pub fn hold(&mut self, key: i32) {
        self.latched = key;
        self.released = false;
    }

    /// `keyReleased`: `p:B = 1` — the next dispatch is the last.
    pub fn release(&mut self) {
        self.released = true;
    }

    /// `b(J)` — consume the latched key, transcribing the real pre-dispatch
    /// order: the accept filter (raw soft/menu keys {23,22,21,-104,-105},
    /// digits, or a mapped game action), the `d:B`/`e:B` title-only shortcut,
    /// the `c:B` swallow, THEN the mode dispatch, then the tail feeds the
    /// script VM (`e.b(char)` — releases the title's op60 gate) and consumes
    /// the latch if the key was released.
    fn input(&mut self, dt_ms: i32) {
        let key = self.latched;
        if key == KEY_SENTINEL {
            return;
        }
        let action = Action::from_key(key);
        let accepted = matches!(key, 23 | 22 | 21 | -104 | -105)
            || (48..=57).contains(&key)
            || action.is_some();
        if !accepted {
            self.latched = KEY_SENTINEL;
            return;
        }
        // d:B/e:B (-104/-105): any-key on the title (straight to the tail,
        // skipping the mode dispatch); swallowed everywhere else.
        let straight_to_tail = matches!(key, -104 | -105);
        if straight_to_tail && self.mode != 8 {
            self.latched = KEY_SENTINEL;
            return;
        }
        // c:B (23) is swallowed before the mode dispatch (p:B = 0; return).
        if key == 23 {
            self.latched = KEY_SENTINEL;
            return;
        }
        // In mode 0 the local i5 is re-assigned to the FULL remap `a(I)I`
        // (bound quick keys -> 0/1/2, directions/fire -> 3..7); the tail then
        // feeds that remapped value to the VM and the dialogue handler.
        let mut i5 = key;
        if !straight_to_tail {
            match self.mode {
                0 => {
                    i5 = self.remap(key);
                    if self.gameplay_input(key, i5, dt_ms) {
                        return; // consumed before the tail
                    }
                }
                3 => {
                    self.menu_input(action, key);
                    self.released = true; // every mode-3 branch: p:B = 1 (2717)
                }
                4 | 9 | 10 | 17 | 23 => {
                    // shared text-page input (3333): scroll + BACK
                    if self.text_page_input(action, key, dt_ms) {
                        return; // consumed ({17,23} BACK: sentinel, p:B=0)
                    }
                }
                19 => {
                    // exit dialog (input 3290): RAW key compares, not the
                    // remap. a:B (22) = YES -> `c()`, falls to the tail;
                    // b:B (21) = NO -> mode 3 + CONSUME (skips the VM tail);
                    // anything else accepted goes straight to the tail.
                    if key == 22 {
                        self.exit_c();
                    } else if key == 21 {
                        self.set_mode(3);
                        self.latched = KEY_SENTINEL;
                        self.released = false;
                        return;
                    }
                }
                _ => {}
            }
        }
        // TAIL: f.a == 0 -> feed the VM (mode-0 keys arrive remapped, arming
        // the op14 handlers) and the dialogue input `d(char)` (scroll/dismiss).
        self.vm.feed_key(i5);
        self.dialogue_input(i5);
        if self.released {
            self.latched = KEY_SENTINEL;
            self.released = false;
        }
    }

    /// `b.a(I)I` — the full in-game key remap: the bound quick keys (`g:[B`) to
    /// 0/1/2, then the direction/fire actions to 3..=7; anything else passes
    /// through raw.
    fn remap(&self, key: i32) -> i32 {
        if key == self.bindings[0] {
            return 0; // quick health
        }
        if key == self.bindings[1] {
            return 1; // quick magika
        }
        if key == self.bindings[2] {
            return 2; // toggle weapon
        }
        match Action::from_key(key) {
            Some(Action::Up) => 3,
            Some(Action::Down) => 4,
            Some(Action::Left) => 5,
            Some(Action::Right) => 6,
            Some(Action::Fire) => 7,
            None => key,
        }
    }

    /// `b(J)` case 0 (input 1804) — the gameplay key dispatch. Returns `true`
    /// when the key was consumed before the tail.
    fn gameplay_input(&mut self, raw: i32, i5: i32, dt_ms: i32) -> bool {
        // a:B (22): the in-game action menu (`n()` + mode 2) — out of slice.
        if raw == 22 {
            if self.world.actors[0].is_none() || !self.world.hud_enabled {
                return false;
            }
            self.pending_leave = Some(Leave::GameKey(22));
            self.latched = KEY_SENTINEL;
            self.released = false;
            return true;
        }
        // b:B (21): the in-game pause menu — l(), page 5, cursors zeroed.
        if raw == 21 {
            self.l();
            self.page = 5;
            self.cursors = [0; 7];
            self.set_mode(3);
            self.latched = KEY_SENTINEL;
            self.released = false;
            return true;
        }
        // The movement/action guard (1822).
        let blocked = self.world.dialogue.is_some()
            || !self.world.input_unlocked
            || self.world.actors[0].is_none()
            || self.world.actors[0].as_ref().unwrap().var_byte_q != 0;
        if blocked {
            return false;
        }
        let (by, by2, by3) = {
            let p = self.world.actors[0].as_ref().unwrap();
            (p.var_byte_l, p.var_byte_m, p.var_byte_n)
        };
        // h.a(player, i5, l) — the input dispatch (h.java:2043).
        let mut events = Vec::new();
        let consumed = {
            let model_name = self.world.actors[0].as_ref().unwrap().model_name.clone();
            let World {
                actors,
                rng,
                effects,
                collision,
                action,
                map_w,
                map_h,
                ..
            } = &mut self.world;
            let mut p = actors[0].take().unwrap();
            let consumed = match i5 {
                3..=6 => {
                    let model = &mut self.models.get(&model_name).anim;
                    p.set_anim(1, Some(model));
                    let dir = match i5 {
                        3 => 2,
                        4 => 1,
                        5 => 4,
                        _ => 3,
                    };
                    formats::move_in_world(
                        &mut p,
                        dir,
                        i64::from(dt_ms),
                        collision,
                        *map_h,
                        *map_w,
                    );
                    false
                }
                7 => {
                    p.fire_action(
                        0,
                        actors,
                        action,
                        *map_h,
                        &mut self.vm.tables,
                        effects,
                        &mut events,
                        rng,
                    );
                    false
                }
                2 => {
                    // The weapon/spell toggle: arr_l <-> arr_m + icon refresh.
                    p.var_int_arr_l = if p.var_int_arr_l.is_none() && p.var_int_arr_m.is_some() {
                        p.var_int_arr_m.clone()
                    } else {
                        None
                    };
                    p.refresh_icon();
                    true
                }
                _ => false,
            };
            actors[0] = Some(p);
            consumed
        };
        self.drain_events(events);
        if consumed {
            self.latched = KEY_SENTINEL;
            self.released = false;
        }
        // The action-entry edge (1830): a fresh var_byte_n pushes its entry.
        let n_now = self.world.actors[0].as_ref().unwrap().var_byte_n;
        if by3 != n_now && n_now != 0 && n_now != -1 && n_now != -2 {
            self.vm.push_entry(n_now as u8);
        }
        match i5 {
            0 | 1 => {
                // Quick heal / quick fatigue (h.a(j, boolean)) — out of slice.
                self.pending_leave = Some(Leave::GameKey(i5));
                self.latched = KEY_SENTINEL;
                self.released = false;
                return true;
            }
            7 => self.try_pickup(),
            _ => {}
        }
        // Overlay resample + the enter/leave trigger edge (1881).
        let l_now = {
            let World {
                actors,
                enter,
                leave,
                map_h,
                ..
            } = &mut self.world;
            actors[0]
                .as_mut()
                .unwrap()
                .sample_overlay(enter, leave, *map_h)
        };
        if l_now != by {
            if by2 != 0 && by2 != -1 && by2 != -2 {
                self.vm.push_entry(by2 as u8);
            } else if by2 == -2 {
                self.world.set_hud_text(None, 0, 0, 0);
            }
            if l_now != 0 && l_now != -1 && l_now != -2 {
                self.vm.push_entry(l_now as u8);
            } else if l_now == -2 {
                let hint = self.lang.get(24).to_string();
                self.world.set_hud_text(Some(hint), 60, 4, 0);
            }
        }
        false
    }

    /// The FIRE pickup scan (input 1841): within 350 of a pickup marker whose
    /// subtype-6 row exists, puff the cell, restore the top-layer tile, shift
    /// the pickup list down, then apply gold / armor / weapon / consumable.
    fn try_pickup(&mut self) {
        let p_ok = self.world.actors[0]
            .as_ref()
            .is_some_and(|p| p.var_int_a > 1000);
        if !p_ok {
            return;
        }
        let p_pos = self.world.actors[0].as_ref().unwrap().var_int_arr_b;
        for i in (0..self.world.pickup_count as usize).step_by(3) {
            let (px, py, item) = (
                i32::from(self.world.pickups[i]),
                i32::from(self.world.pickups[i + 1]),
                i32::from(self.world.pickups[i + 2]),
            );
            let pos = [px << 7, py << 7];
            if formats::combat_distance(&pos, &p_pos) >= 350 {
                continue;
            }
            let Some(row6) = self.vm.tables.row(6, item).map(<[i32]>::to_vec) else {
                continue;
            };
            self.world.effects.spawn_world(8, pos[0], pos[1] + 128, 0);
            // The top-layer marker restores: -45 (marked) -> -44, else 0.
            let cell = (px * self.world.map_h + py) as usize;
            let top = self.world.layers.len() - 1;
            self.world.layers[top][cell] = if self.world.layers[top][cell] == -45 {
                -44
            } else {
                0
            };
            // Shift the pickup list down over this triple.
            let count = self.world.pickup_count as usize;
            for n5 in (i..count.min(self.world.pickups.len() - 3)).step_by(3) {
                self.world.pickups[n5] = self.world.pickups.get(n5 + 3).copied().unwrap_or(0);
                self.world.pickups[n5 + 1] = self.world.pickups.get(n5 + 4).copied().unwrap_or(0);
                self.world.pickups[n5 + 2] = self.world.pickups.get(n5 + 5).copied().unwrap_or(0);
            }
            self.world.pickup_count -= 3;
            // Gold / armor / weapon / consumable (each shows its HUD line).
            if row6[2] > 0 {
                let text = format!("{} {}", row6[2], self.lang.get(38));
                self.world.set_hud_text(Some(text), 3, 4, 0);
                self.world.gold += row6[2];
            } else if row6[4] > 0 {
                let row = self.vm.tables.row(1, row6[4]).expect("armor row").to_vec();
                let name = self.item_name(&row);
                self.world.set_hud_text(Some(name), 3, 4, 0);
                if let Some(a) = self.world.actors[0].as_mut() {
                    a.equip(1, &row, false, &self.vm.tables);
                }
            } else if row6[3] > 0 {
                let row = self.vm.tables.row(4, row6[3]).expect("weapon row").to_vec();
                let name = self.item_name(&row);
                self.world.set_hud_text(Some(name), 3, 4, 0);
                if let Some(a) = self.world.actors[0].as_mut() {
                    a.equip(0, &row, false, &self.vm.tables);
                }
            } else if row6[5] > 0 {
                let row = self
                    .vm
                    .tables
                    .row(2, row6[5])
                    .expect("consumable row")
                    .to_vec();
                let name = self.item_name(&row);
                self.world.set_hud_text(Some(name), 3, 4, 0);
                if let Some(a) = self.world.actors[0].as_mut() {
                    a.equip(2, &row, false, &self.vm.tables);
                }
            }
            break;
        }
    }

    /// `e.a(int)` on an item row's name field (`row[1]`): lang ref or pool
    /// string.
    fn item_name(&self, row: &[i32]) -> String {
        let v = row[1];
        if (v & 0xF000) == 0xF000 {
            self.lang.get((v & 0xFFF) as u16).to_string()
        } else {
            self.vm.pool_string(v).to_string()
        }
    }

    /// `b.d(int)` (b.java:3161) — the dialogue input, fed from the `b(J)`
    /// tail: UP scrolls back, DOWN scrolls forward while lines remain, FIRE
    /// (after >= 1000ms open) dismisses.
    fn dialogue_input(&mut self, i5: i32) {
        let Some(d) = self.world.dialogue.as_mut() else {
            return;
        };
        if i5 == 3 && d.scroll > -1 {
            d.scroll -= 4;
        }
        if i5 == 4 && !d.shown_all {
            d.scroll += 4;
        }
        if i5 == 7 && d.open_ms >= 1000 {
            self.world.dialogue = None;
        }
    }

    /// The shared text-page input (offset 3333, modes {4,9,10,17,23}):
    /// UP scrolls back (`g:S = min(305, g + dt/10)` — the {17,23} paint
    /// clamp then settles it at 20), DOWN scrolls forward (`g -= dt/10`,
    /// dead on {17,23} once the end-latch `p:Z` is set — the original never
    /// clears it in-mode, so after an UP overscroll DOWN stays blocked),
    /// and BACK (`b:B`) exits {4,17,23} to mode 3 ({17,23}: page 6 +
    /// consume; {4}: main/pause page, NOT consumed). Returns `true` when
    /// the key was consumed (the caller must skip the VM tail).
    fn text_page_input(&mut self, action: Option<Action>, key: i32, dt_ms: i32) -> bool {
        let small_h = self
            .masks
            .metrics(crate::text::GameFont::SmallBold)
            .midp_height;
        match action {
            Some(Action::Up) => {
                self.scroll_acc = 0;
                self.scroll =
                    (i32::from(self.scroll) + dt_ms / 10).min(SCREEN_H - 4 * small_h) as i16;
            }
            Some(Action::Down) => {
                if !(matches!(self.mode, 17 | 23) && self.end_latched) {
                    self.scroll_acc = 0;
                    self.scroll = (i32::from(self.scroll) - dt_ms / 10) as i16;
                }
            }
            _ => {}
        }
        if matches!(self.mode, 4 | 17 | 23) && key == 21 {
            if self.mode == 4 {
                self.page = if self.left_gameplay { 5 } else { 0 };
                self.set_mode(3); // not consumed: falls to the tail
            } else {
                self.page = 6;
                self.latched = KEY_SENTINEL;
                self.released = false;
                self.set_mode(3);
                return true;
            }
        }
        false
    }

    /// `b.c()` (javap 16071) — the YES/exit native: mode 12 (terminal — the
    /// `a(byte)` setter latches there and `run()` exits its loop), repaint,
    /// a 2s real-time sleep, then `MIDlet.notifyDestroyed()` (on FreeJ2ME:
    /// `System.exit`). The destruction is modeled as [`Self::exited`]; mode 12
    /// paints nothing, so the real LCD keeps the last frame until the JVM dies.
    fn exit_c(&mut self) {
        self.set_mode(12);
        self.exited = true;
    }

    /// `b(J)` mode-3 dispatch (input 1518): LEFT/RIGHT wrap the page cursor,
    /// BACK (`b:B`, RAW key — before the fire) pops the Help submenu
    /// (`k = x:B`) or class select (`k = f:Z ? 5 : 0`), FIRE dispatches on
    /// the selected item's string (the bytecode's compare chain).
    fn menu_input(&mut self, action: Option<Action>, key: i32) {
        let page = self.page as usize;
        let len = self.pages[page].len();
        match action {
            Some(Action::Left) => {
                // e[k]--; if < 0 wrap to len-1
                self.cursors[page] = if self.cursors[page] == 0 {
                    len - 1
                } else {
                    self.cursors[page] - 1
                };
            }
            Some(Action::Right) => {
                // e[k]++; if == len wrap to 0
                self.cursors[page] = (self.cursors[page] + 1) % len;
            }
            None if key == 21 => {
                // BACK (1611): Help submenu -> the page it was entered from;
                // class select -> main (or the in-game pause page)
                if self.page == 6 {
                    self.page = self.page_saved;
                } else if self.page == 1 {
                    self.page = if self.left_gameplay { 5 } else { 0 };
                }
            }
            Some(Action::Fire) => self.fire(),
            _ => {} // menus ignore up/down + other accepted keys
        }
    }

    /// The FIRE item dispatch (input 1671–2714) — the faithful string-compare
    /// chain in bytecode order. Items opening unported modes yield
    /// [`Leave::Mode`]; an item missing from the chain entirely would be a
    /// real no-op in the original, but here means `l()` built something
    /// unexpected -> panic.
    fn fire(&mut self) {
        let page = self.page;
        if page == 2 {
            // debug level list: loader on pages[3][cursor] (out of slice)
            self.pending_leave = Some(Leave::Mode(6));
            return;
        }
        let item = self.pages[page as usize][self.cursors[page as usize]].clone();
        let is = |id: u16| item == self.lang.get(id);
        if item.starts_with(self.lang.get(4)) && !self.lang.get(4).is_empty() {
            panic!("Sound toggle (o:Z + l() + g()) not ported (out of slice)");
        } else if is(19) {
            self.pending_leave = Some(Leave::Mode(13)); // Save Game
        } else if is(3) {
            self.pending_leave = Some(Leave::Mode(14)); // Load Game
        } else if is(21) {
            self.pending_leave = Some(Leave::Mode(0)); // Continue (resume)
        } else if is(2) {
            // New Game: b()Z (has-save) fenced false -> class select, never
            // the mode-16 overwrite confirm on the wiped-RMS baseline
            self.page = 1;
        } else if is(6) {
            self.set_mode(4); // About -> the animated credits roll
        } else if is(456) {
            // Help: SAVE the current page, switch to the submenu — stays
            // in mode 3 (input 2028: x:B = k; k = 6)
            self.page_saved = self.page;
            self.page = 6;
        } else if is(457) {
            self.topic = 457; // l:S (the 17/23 title); w:B/v:B are mode-5/18
            self.set_mode(17); // Basic Controls text page
        } else if is(458) {
            self.pending_leave = Some(Leave::Mode(5)); // Custom Controls
        } else if is(573) {
            self.topic = 573;
            self.set_mode(23); // Game Overview text page
        } else if is(522) || is(459) || is(460) || is(461) || is(462) {
            self.pending_leave = Some(Leave::Mode(18)); // stat-table overviews
        } else if is(18) {
            self.pending_leave = Some(Leave::Mode(1)); // Go Shopping
        } else if is(20) {
            self.pending_leave = Some(Leave::Mode(0)); // Continue Playing
        } else if page == 1 {
            // k==1 class fire (2593 — checked BEFORE the Exit compare):
            // k(); r:B = 0; mode 6; null actors; the loader on the HARDCODED
            // /l01_1.scr; b:I = 100 (the starting gold). The chosen class
            // reaches the spawner via the class-select cursor (e:[B[1]).
            self.progress = 0;
            self.loader("/l01_1.scr").expect("class-fire level load");
            self.world.gold = 100;
        } else if is(22) {
            self.set_mode(19); // Exit -> confirm dialog (2712: a((byte)19))
        } else {
            panic!("menu item not in the ported compare chain: {item:?}");
        }
    }

    /// A fenced boundary the input crossed (class fire, Exit/Help/About),
    /// consumed by the caller. `None` while inside the ported slice.
    pub fn take_leave(&mut self) -> Option<Leave> {
        self.pending_leave.take()
    }

    pub fn screen(&self) -> Option<Screen> {
        match (self.mode, self.page) {
            (8, _) if self.vm.key_gate => Some(Screen::Title),
            (3, 0) => Some(Screen::MainMenu),
            (3, 1) => Some(Screen::ClassSelect),
            (3, 6) => Some(Screen::HelpTopics),
            (19, _) => Some(Screen::ExitDialog),
            (17, _) => Some(Screen::ControlsPage),
            (23, _) => Some(Screen::OverviewPage),
            (4, _) => Some(Screen::AboutRoll),
            (15, _) => Some(Screen::PleaseWait),
            (10, _) => Some(Screen::IntroText),
            (0, _) => Some(Screen::Gameplay),
            _ => None,
        }
    }

    pub fn mode(&self) -> i8 {
        self.mode
    }

    /// `notifyDestroyed()` fired (YES on the exit dialog): the MIDlet is dead;
    /// the real `run()` loop has exited (mode 12) and the JVM is going down.
    pub fn exited(&self) -> bool {
        self.exited
    }

    pub fn blink_on(&self) -> bool {
        self.blink_on
    }

    /// `paint(Graphics)` for the current mode into a fresh logical 240x345
    /// framebuffer. `&mut self` because the mode-21 paint decrements the
    /// scroll (a faithful quirk — see `paint_text_page`).
    pub fn render(&mut self) -> anyhow::Result<Fb> {
        let mut fb = Fb::new(SCREEN_W, SCREEN_H);
        match self.mode {
            6 | 7 => {
                // "Loading..." = (lang 39, empty pre-lang-load -> start.txt[0])
                // + "..." — both spellings resolve to the same label here.
                let base = match self.lang.get(39) {
                    "" => self.start_txt(0),
                    s => s.to_string(),
                };
                paint_loader(&mut fb, &self.masks, &format!("{base}..."), self.progress);
            }
            8 => {
                let model = self.model.as_ref().expect("mode 8 with no model (b:Ld)");
                let rec = model
                    .records
                    .iter()
                    .find(|r| r.effective_id == self.anim)
                    .unwrap_or_else(|| panic!("model has no record for anim {}", self.anim));
                let path = rec.path.clone();
                paint_startup(
                    &mut fb,
                    &self.masks,
                    &self.assets,
                    &path,
                    self.bg,
                    self.vm.key_gate && self.blink_on,
                )?;
            }
            3 => {
                let page = self.page as usize;
                paint_menu_page(
                    &mut fb,
                    &self.masks,
                    &self.assets,
                    self.page,
                    &self.pages[page],
                    self.cursors[page],
                    self.banner,
                )?;
            }
            9 | 21 => {
                let final_y = paint_text_page(
                    &mut fb,
                    &self.masks,
                    self.mode,
                    &self.text_pages,
                    &mut self.scroll,
                    None,
                );
                self.text_page_end(final_y)?;
            }
            17 | 23 => {
                let title = self.lang.get(self.topic).to_string();
                let final_y = paint_text_page(
                    &mut fb,
                    &self.masks,
                    self.mode,
                    &self.text_pages,
                    &mut self.scroll,
                    Some(&title),
                );
                self.text_page_end(final_y)?;
            }
            4 => anyhow::bail!(
                "About (mode 4) render fenced: the credits roll always draws \
                 a scroll arrow (.cml frame render, unported) and is animated \
                 — visual-only, never gated"
            ),
            10 => anyhow::bail!(
                "intro text page (mode 10) render fenced: animated auto-scroll \
                 with the parchment scroll-arrow bar (.cml frame render, \
                 unported) — visual-only; its END transition (mode 0) is state"
            ),
            0 | 15 => anyhow::bail!(
                "gameplay/please-wait paint (mode {}) is the M11 render slice \
                 (r()/q()/b(G) tiles+actors+HUD); this slice validates STATE",
                self.mode
            ),
            19 => paint_exit_dialog(&mut fb, &self.masks),
            12 => anyhow::bail!(
                "paint mode 12 is terminal: the real paint draws NOTHING (the \
                 LCD keeps the last frame while c() destroys the MIDlet)"
            ),
            other => anyhow::bail!("paint mode {other} not ported (out of slice)"),
        }
        Ok(fb)
    }

    /// The end-of-text check at the text-page paint's tail (offset 3503):
    /// when the final line y sits above the page limit (`b:S - smallH`,
    /// minus `3*smallH` for {10,23,4,17}), a 2-paint `b:Z` debounce fires
    /// the end action: {17,23} latch `p:Z` (DOWN dead), 21 nothing, 4 ->
    /// menu (3s freeze elided; unreachable here — mode-4 render is fenced),
    /// 9/10 -> outro/intro transitions (gameplay slice, loud).
    fn text_page_end(&mut self, final_y: i32) -> anyhow::Result<()> {
        let small_h = self
            .masks
            .metrics(crate::text::GameFont::SmallBold)
            .midp_height;
        let mut limit = SCREEN_H - small_h;
        if matches!(self.mode, 10 | 23 | 4 | 17) {
            limit -= 3 * small_h;
        }
        if final_y < limit {
            if self.end_debounce {
                self.end_debounce = false;
            } else {
                self.end_debounce = true;
                match self.mode {
                    17 | 23 => self.end_latched = true, // p:Z = 1
                    21 => {}
                    4 => self.set_mode(3),
                    10 => self.set_mode(0), // the intro chains into gameplay
                    other => anyhow::bail!(
                        "text-page end transition for mode {other} not ported \
                         (the mode-9 outro menu-reset chain)"
                    ),
                }
            }
        }
        Ok(())
    }

    /// `new a().a(byte)` — the Nth `|`-separated segment of /start.txt.
    fn start_txt(&self, n: usize) -> String {
        let raw = std::fs::read_to_string(self.assets_dir.join("start.txt")).expect("start.txt");
        raw.split('|').nth(n).unwrap_or_default().to_string()
    }
}
