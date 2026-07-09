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
//! - the in-game `n()` action menu (mode 2, the f.java menu system) yields
//!   [`Leave::GameKey`]; Custom Controls (mode 5), the overview stat
//!   tables (mode 18), Save/Load/overwrite (13/14/16) and the shop yield
//!   [`Leave::Mode`];
//! - the mode-9 outro end-transition is loud (its null-all keeps `b.var_j_a`
//!   alive for a later spawner reuse — needs a player stash, outro slice);
//! - `b()Z` (RecordStore has-save probe) is modeled as `false` — the pinned
//!   wiped-RMS baseline (no "Continue" item); the save-capture slice lifts it;
//! - the in-game `f.java` menus (`f.a:B == 1`) stay out of slice.
//!
//! The mode-10 intro page and the mode-4 About credits RENDER fully (the
//! body + the clipped-band arrow/parchment tails, see `paint_text_page`),
//! gated at fixed-scroll normalized shots; their rolls stay animated
//! (wall-clock scroll), so free-running frames are visual-only.

use crate::asset::Assets;
use crate::fb::Fb;
use crate::fmenu::{FMenu, MenuItem};
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

/// `a(I)I`'s fall-through value (`-1122868`) — a key that is neither a
/// direction/fire equivalent nor a live binding remaps to this.
const REMAP_NONE: i32 = -1122868;

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

/// `a:I >= 0 ? a:I : getGameAction(a:I)` — the stored form of a binding (and
/// the collision-check normalization). A negative code resolves to its game
/// action (1/6/2/5/8); an unresolvable one to 0 (MIDP's "no action").
fn normalized_key(key: i32) -> i32 {
    if key >= 0 {
        return key;
    }
    match Action::from_key(key) {
        Some(Action::Up) => 1,
        Some(Action::Down) => 6,
        Some(Action::Left) => 2,
        Some(Action::Right) => 5,
        Some(Action::Fire) => 8,
        None => 0,
    }
}

/// What a fire did when it leaves the ported slice (fenced, not yet ported).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Leave {
    /// a menu item that opens an unported mode (Save/Load/shop/…)
    Mode(u8),
    /// an in-game key that opens an unported screen (the `n()` action menu —
    /// the quick heal/fatigue keys are ported, `Actor::quick_use`)
    GameKey(i32),
}

/// A readable view of the front-end screens for tests (`b.m:B` + `k:B`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Title,            // m=8 (key-gate set)
    MainMenu,         // m=3, k=0
    ClassSelect,      // m=3, k=1
    HelpTopics,       // m=3, k=6 (the Help submenu carousel)
    ExitDialog,       // m=19
    ControlsPage,     // m=17 (Basic Controls text page)
    OverviewPage,     // m=23 (Game Overview text page)
    AboutRoll,        // m=4 (animated credits; gated at a fixed-scroll shot)
    PleaseWait,       // m=15 (level-load anim — visual-only)
    IntroText,        // m=10 (the level intro page, auto-scrolls into mode 0)
    Gameplay,         // m=0
    Death,            // m=11 (the player-death "Continue?" screen)
    ActionMenu,       // m=2 (the f.java Attack/Armor/Items/Stats menu)
    GameSaved,        // m=13 ("Game Saved" + press any key)
    LoadConfirm,      // m=14 ("Load Saved Game?" YES/NO)
    OverwriteConfirm, // m=16 ("Saved Game Exists" / "Overwrite?" YES/NO)
    ControlsRedefine, // m=5 (the Custom Controls redefine list; m=20 confirms)
    KeyTaken,         // m=20 ("Key Already Taken" + OK)
    StatTable,        // m=18 (the overview stat tables)
    Shop,             // m=1 (the o() Buy/Sell f menu)
    Interrupt,        // m=22 (hideNotify's "Resume game?" screen)
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
    /// `f:[B` — the LIVE key bindings (quick-health, quick-magika,
    /// toggle-weapon; defaults `{55, 57, 51}` = keys 7/9/3). The `a(I)I`
    /// remap and the save record read THIS table; mode 5 edits a copy.
    bindings: [i32; 3],
    /// `g:[B` — the mode-5 EDIT copy: Custom Controls entry copies f in,
    /// "Save Changes" commits it back (f ← g) + `g()`. `j()` zeroes it.
    bindings_edit: [i32; 3],
    /// `k:I` — the mode-5 list cursor. Reset only by `j()`, so it PERSISTS
    /// across Custom Controls visits (faithful).
    redef_cursor: usize,
    /// `j:Z` — mode-5 capture armed ("Select new value"; next accepted
    /// non-soft key binds or bounces to mode 20).
    capture: bool,
    /// `b:[[String` — the CURRENT overview table shown by mode 18.
    stat_table: Vec<Vec<String>>,
    /// The memoized per-topic builders (`c..h:[[String`), nulled by `k()`.
    stat_caches: crate::stattab::StatCaches,
    /// `v:B` / `w:B` — the mode-18 record index / top-line index.
    stat_v: usize,
    stat_w: usize,
    /// `q:Z` — "the down arrow was drawn" paint side effect; gates DOWN.
    q_flag: bool,
    /// The first unported script opcode hit (a content boundary in a
    /// deeper level the port hasn't reached). The real game would run it;
    /// the port records it and STOPS the VM gracefully instead of panicking,
    /// so a frontend can show an honest "unported content" screen rather
    /// than a hard crash. `None` while inside the ported slice.
    unported_op: Option<u8>,
    /// `n:B` — the mode saved by hideNotify (the interrupt screen's YES
    /// restores it); -1 idle.
    saved_mode: i8,
    /// `d:Z` — the hideNotify park flag: `run()` sleeps instead of ticking
    /// until showNotify clears it.
    paused: bool,
    /// `var_java_lang_String_c` — the current level's script path (set by the
    /// loader; the save writes it as the record "name", the load re-runs the
    /// loader on it).
    level_script: String,
    /// `var_boolean_o` — the sound flag (persisted; the Sound toggle is out of
    /// slice, so it stays false).
    bool_o: bool,
    /// The `ESO` RecordStore, modeled as the in-memory record-1 blob. `None` =
    /// no save (`boolean_b()` false — the wiped-RMS baseline until `g()`).
    save_slot: Option<Vec<u8>>,
    /// `a:Lf;` — the f.java in-game menu system (the mode-2 action menu).
    pub fmenu: FMenu,
    /// `b.var_c_a` / `var_c_b` — the active-weapon / active-spell menu nodes
    /// (rebuilt per `n()`; the original keeps stale refs to the PREVIOUS
    /// graph, whose re-marking is unobservable — modeled as a per-build
    /// reset, see `n_action_menu`).
    active_weapon_item: Option<usize>,
    active_spell_item: Option<usize>,
}

impl Shell {
    /// The ctor path: field defaults (`<clinit>` + `j()`), then the loader on
    /// the initial script — the real boot, no seeding. `assets_dir` is the
    /// extracted-jar resource root; `masks` the oracle text fixture.
    pub fn boot(assets_dir: impl Into<PathBuf>, masks: TextMasks) -> anyhow::Result<Self> {
        let assets_dir = assets_dir.into();
        // a:Lf; = new f(ctorArg3, this) — the menu model loads at boot.
        let menu_cml = {
            let bytes = std::fs::read(assets_dir.join("oh_menu.cml"))?;
            parse_cml(&bytes)?
        };
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
            bindings_edit: [0, 0, 0], // j(): g:[B zeroed
            redef_cursor: 0,
            capture: false,
            stat_table: Vec::new(),
            stat_caches: crate::stattab::StatCaches::default(),
            stat_v: 0,
            stat_w: 0,
            q_flag: false, // <clinit>
            unported_op: None,
            saved_mode: -1,
            paused: false,
            level_script: String::new(),
            bool_o: false,
            save_slot: None,
            fmenu: FMenu::new(menu_cml),
            active_weapon_item: None,
            active_spell_item: None,
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
        self.level_script = name.to_string(); // var_java_lang_String_c
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
                let text = self.lang.get(547).to_string();
                self.text_pages = self.h(&text);
            }
            4 => {
                // BYTECODE CORRECTION (a(byte) offset 134–147): the credits
                // scroll starts at b:S - 4 * c:Font height — c:Font is SMALL
                // BOLD (=305), not the medium font the recon prose said.
                let text = self.lang.get(548).to_string();
                self.text_pages = self.h(&text);
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
                let text = self.lang.get(574).to_string();
                self.text_pages = self.h(&text);
                self.scroll = 15;
                self.end_latched = false; // p:Z = 0
            }
            17 => {
                let text = self.lang.get(465).to_string();
                self.text_pages = self.h(&text);
                self.scroll = 15;
                self.end_latched = false; // p:Z = 0
            }
            _ => {}
        }
    }

    /// `b.h(String)` — build the text-page model (see `wrap`). QUIRK
    /// (offset 121, caught by the outro drive's menu dump): the page build
    /// calls `g(null)` — the dialogue SPEAKER clears on every text-page
    /// entry (which is also why the outro paragraphs draw unprefixed while
    /// a dialogue is still open underneath).
    fn h(&mut self, text: &str) -> Vec<Vec<String>> {
        self.world.speaker = None; // g(null)
        build_pages(&self.masks, text, &self.version)
    }

    /// `b.l()` — build the menu page table. Page 0 (main) and page 5 (in-game
    /// pause) insert "Load Game" (lang 3) right after "New Game" when a save
    /// exists (`b.boolean_b()`, l() 262/275). Page 1 (class select) walks the
    /// persistent script class table.
    fn l(&mut self) {
        let g = |id: u16| self.lang.get(id).to_string();
        let has_save = self.has_save();
        self.pages[0] = if has_save {
            vec![g(2), g(3), g(456), g(6), g(22)]
        } else {
            vec![g(2), g(456), g(6), g(22)]
        };
        self.pages[5] = if has_save {
            vec![g(21), g(2), g(3), g(456), g(6), g(22)]
        } else {
            vec![g(21), g(2), g(456), g(6), g(22)]
        };
        self.pages[1] = self.vm.class_name_ids().iter().map(|&id| g(id)).collect();
        // page 4 = the op45 checkpoint menu (b.f() -> k=4):
        // Go Shopping / Save Game / Continue Playing
        self.pages[4] = vec![g(18), g(19), g(20)];
        // page 6 = the Help submenu (build order verified in l() bytecode):
        // Basic/Custom Controls, Game/Classes/Weapons/Armor/Spells/Items
        self.pages[6] = [457u16, 458, 573, 522, 459, 460, 461, 462]
            .iter()
            .map(|&id| g(id))
            .collect();
        // pages 2/3 (the debug level list) stay out of slice: left empty,
        // and rendering an empty page is a loud index panic rather than a
        // wrong frame.
    }

    /// `b.f()` (b.java:2754) — the op45 checkpoint-menu native: page 4,
    /// mode 3. No cursor reset, no table rebuild (e:[B[4] persists).
    pub fn f_checkpoint_menu(&mut self) {
        self.page = 4;
        self.set_mode(3);
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
                // b.void_a(int): remove an actor (slot 0 = the player-death
                // sequence + the mode-11 death screen).
                self.world.remove_actor(op(0) as usize, &self.vm.tables);
                self.check_player_death();
            }
            21 => {
                // The actor wait-list (var_int_arr_e).
                let list = step.operands[1..].to_vec();
                self.vm.set_wait_actors(list);
            }
            24 => {
                let slot = op(0) as usize;
                let World {
                    actors,
                    actor_anims,
                    ..
                } = &mut self.world;
                if let Some(a) = actors[slot].as_mut() {
                    a.set_anim(op(1) as i8, actor_anims[slot].as_mut());
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
            45 => self.f_checkpoint_menu(), // b.f(): the shop/save checkpoint menu
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
            47 => {
                // e.b(long) case 47: b.a(int_arr_a(9, row), var_int_arr_g,
                // n, n2) — the procedural maze generator (the L01 sewers).
                self.op47_maze(op(0), op(1), op(2));
            }
            72 => {} // free cached graphics: no resource effect (validated M7)
            other => {
                // An unported content opcode (deeper-level content the port
                // hasn't reached yet). Record the boundary and HALT the VM
                // instead of panicking — a frontend shows an honest stop; the
                // parity drives never reach here, so an unexpected op still
                // surfaces (as a stalled VM) rather than corrupting a gated
                // frame.
                self.unported_op = Some(other);
                self.vm.halt();
            }
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
        let window_h = (SCREEN_H >> 1).min(self.models.get(&ui).frame_size(51).1) - 4;
        self.world.dialogue = Some(Dialogue {
            lines,
            scroll: -1,
            shown_all: true,
            open_ms: 0,
            window_h,
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
        // run() step 2: d:Z (hideNotify) parks the loop — sleep(1000) +
        // continue; nothing ticks (and nothing repaints) until showNotify.
        if self.paused {
            return;
        }
        // run() step 4: f.a:B == 1 -> the menu tick (marquee) REPLACES the
        // effects + VM tick — the world freezes under the open menu.
        if self.fmenu.open {
            self.fmenu.tick(dt_ms);
        } else if !matches!(self.mode, 3 | 10 | 9 | 13) {
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
        // Paint-side end-of-text checks for the auto-scrolling pages: the
        // real loop paints every frame and the m=10 -> m=0 / m=4 -> m=3 /
        // m=9 -> m=4 transitions live in the paint tail; our render only
        // runs at shots, so evaluate the (pure) final-line y here each
        // frame instead (render for 4/9/10 correspondingly skips the end
        // check).
        if matches!(self.mode, 10 | 4 | 9) {
            let fy = self.text_final_y();
            self.text_page_end(fy)
                .expect("the mode-9/10/4 end transitions are ported");
        }
        // The paint's per-frame STATE effects for mode 0 (the real loop paints
        // every frame): r() camera recenter, q() visible range, and b(G)'s
        // dirty consumption (the pixel work is deferred to render — the flag
        // lifecycle must match the original, or a later r() would see a stale
        // dirty flag and recenter when the real game did not).
        if self.mode == 0 {
            crate::gpaint::r_camera(&mut self.world, &mut self.models);
            crate::gpaint::q_range(&mut self.world);
            if self.world.dirty {
                self.world.dirty = false;
                self.world.base_stale = true;
            }
        }
    }

    /// The text-page paint's final line y, computed without painting: `3 +
    /// g:S` plus one pitch (`smallH + 1`) per wrapped line (an empty paragraph
    /// still advances one pitch). Modes 10/4 never take the m=21 skip-first or
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
            {
                let World {
                    actors,
                    actor_anims,
                    rng,
                    effects,
                    collision,
                    layers,
                    map_h,
                    ..
                } = &mut self.world;
                let model = actor_anims[n].as_mut();
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
    /// script stack; loot drops; the summon spawner). A slot-0 removal ran
    /// the death sequence inside the world — convert it to `set_mode(11)`
    /// here (the real flip happens inside `void_a(0)`, and the actor loop's
    /// per-slot mode check must see it the same frame).
    fn drain_events(&mut self, events: Vec<formats::WorldEvent>) {
        let pushes = self
            .world
            .apply_events(events, &self.vm.tables, &mut self.models);
        for entry in pushes {
            self.vm.push_entry(entry);
        }
        self.check_player_death();
    }

    /// Convert the world's `player_died` flag (the `b.void_a(0)` sequence)
    /// into the mode-11 death screen.
    fn check_player_death(&mut self) {
        if self.world.player_died {
            self.world.player_died = false;
            self.set_mode(11);
        }
    }

    /// `hideNotify` (b.java:3184): ignored while loading (`boolean_c()` =
    /// m in {6,7,15}); parks the run loop (`d:Z`), and outside the boot
    /// pages {8,21,15,10} saves the mode in `n:B`, writes `m = 22`
    /// DIRECTLY (bypassing the `a(byte)` gate) and arms the f overlay.
    pub fn hide_notify(&mut self) {
        if matches!(self.mode, 6 | 7 | 15) {
            return;
        }
        self.paused = true;
        if !matches!(self.mode, 8 | 21 | 15 | 10) {
            if self.mode != 22 {
                self.saved_mode = self.mode;
            }
            self.mode = 22;
            self.fmenu.resume_overlay = true;
        }
    }

    /// `showNotify`: same loading guard; only unparks the loop.
    pub fn show_notify(&mut self) {
        if matches!(self.mode, 6 | 7 | 15) {
            return;
        }
        self.paused = false;
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
                    // 1518: `i5 = a(i5)` — the TAIL feeds the REMAPPED value
                    // (a mode-3 FIRE over a still-open dialogue leaks a 7
                    // into d(char) and DISMISSES it — oracle-pinned by the
                    // shop drive's q10).
                    i5 = self.remap(key);
                    self.menu_input(action, key);
                    self.released = true; // every mode-3 branch: p:B = 1 (2717)
                }
                4 | 9 | 10 | 17 | 23 => {
                    // shared text-page input (3333): `i5 = a(i5)` + scroll/BACK
                    i5 = self.remap(key);
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
                11 => {
                    // player-death screen (input 2974): a:B (22) "Continue?"
                    // -> mode 0 (the player was already re-initialized +
                    // respawned by the void_a(0) death sequence); b:B (21)
                    // -> p() (cursors zeroed, page = f:Z ? 5 : 0) + mode 3.
                    // Every branch sets p:B = 1 and falls to the tail.
                    if key == 22 {
                        self.set_mode(0);
                    } else if key == 21 {
                        self.p_reset();
                        self.set_mode(3);
                    }
                    self.released = true; // p:B = 1 (3007)
                }
                2 => {
                    // the action menu (input 1429): i5 is the FULL remap
                    // (assigned — the tail sees it); b:B (21) pops
                    // (`f.a()Z`) — a failed pop closes the menu + mode 0; a
                    // successful one consumes the latch. a:B (22) is
                    // swallowed. Everything else feeds `f.a(char)` and its
                    // activation callback `b.a(c)`.
                    i5 = self.remap(key);
                    let i5r = i5;
                    if key == 21 {
                        if !self.fmenu.back() {
                            self.fmenu.open = false;
                            self.set_mode(0);
                        } else {
                            self.latched = KEY_SENTINEL;
                        }
                    } else if key != 22 && self.fmenu.open {
                        let items_page = self.lang.get(27).to_string();
                        if let Some(node) = self.fmenu.input(i5r, &items_page) {
                            self.activate_item(node);
                        }
                    }
                    self.released = true; // p:B = 1 (1510)
                }
                13 => {
                    // Game Saved (input 3062): ANY accepted key -> mode 3.
                    self.set_mode(3);
                    self.released = true;
                }
                14 => {
                    // Load Saved Game? (input 3015): a:B (22) YES -> h() load;
                    // b:B (21) NO -> p() + mode 3 + e[0]=1 (cursor back on the
                    // "Load Game" item) + consume.
                    if key == 22 {
                        self.load_game().expect("save load");
                    } else if key == 21 {
                        self.p_reset();
                        self.cursors[0] = 1; // e[0] = 1 (3048, literal)
                        self.set_mode(3);
                        self.latched = KEY_SENTINEL;
                        self.released = false;
                        return;
                    }
                    self.released = true;
                }
                16 => {
                    // Saved Game Exists / Overwrite? (input 3074): a:B (22)
                    // YES -> class select (k=1) + mode 3 + consume; b:B (21)
                    // NO -> mode 3 + consume.
                    if key == 22 {
                        self.page = 1;
                        self.set_mode(3);
                    } else if key == 21 {
                        self.set_mode(3);
                    }
                    self.latched = KEY_SENTINEL;
                    self.released = false;
                    return;
                }
                5 => {
                    // Custom Controls (input 2725): i5 = a(i5) first (the
                    // LIVE table; assigned — the tail sees it). Capturing
                    // (j:Z): soft keys do NOTHING (capture stays armed); a
                    // valid key binds into the EDIT table (raw if >= 0,
                    // else its game action), an invalid one bounces to mode
                    // 20 — j:Z clears either way. Browsing: BACK -> mode 3
                    // (page still 6); UP/DOWN move the cursor CLAMPED (no
                    // wrap); FIRE on "Save Changes" commits f <- g + mode 3
                    // + g() (the real save write); FIRE elsewhere arms the
                    // capture. All branches p:B = 1.
                    i5 = self.remap(key);
                    let i5r = i5;
                    if self.capture {
                        if key != 21 && key != 22 {
                            if self.key_valid(key, i5r) {
                                self.bindings_edit[self.redef_cursor] = normalized_key(key);
                            } else {
                                self.set_mode(20);
                            }
                            self.capture = false;
                        }
                    } else if key == 21 {
                        self.set_mode(3);
                    } else if i5r == 3 {
                        self.redef_cursor = self.redef_cursor.saturating_sub(1);
                    } else if i5r == 4 {
                        // min(a:[String.len - 1, k+1); the l() list is fixed
                        // [Quick Health, Quick Magicka, Toggle Attack,
                        // Save Changes]
                        self.redef_cursor = (self.redef_cursor + 1).min(3);
                    } else if i5r == 7 {
                        if self.redef_cursor == 3 {
                            // a:[String[k] == lang 294 "Save Changes"
                            self.bindings = self.bindings_edit; // f <- g
                            self.capture = false;
                            self.set_mode(3);
                            self.save_game(); // g()
                        } else {
                            self.capture = true;
                        }
                    }
                    self.released = true; // p:B = 1 (2957)
                }
                18 => {
                    // Stat tables (input 3132): BACK -> mode 3; LEFT/RIGHT
                    // cycle the record (wrapping) and reset the line; UP
                    // clamps at 0; DOWN only if the last paint drew the down
                    // arrow (q:Z). EVERY key is consumed directly (3276) —
                    // mode-18 keys never reach the VM tail (the real
                    // `i5 = a(i5)` write is equally dead there).
                    let i5r = self.remap(key);
                    if key == 21 {
                        self.set_mode(3);
                    } else if i5r == 5 {
                        self.stat_w = 0;
                        self.stat_v = if self.stat_v == 0 {
                            self.stat_table.len() - 1
                        } else {
                            self.stat_v - 1
                        };
                    } else if i5r == 6 {
                        self.stat_w = 0;
                        self.stat_v = (self.stat_v + 1) % self.stat_table.len();
                    } else if i5r == 3 {
                        self.stat_w = self.stat_w.saturating_sub(1);
                    } else if i5r == 4 && self.q_flag {
                        self.stat_w += 1;
                    }
                    self.latched = KEY_SENTINEL;
                    self.released = false;
                    return;
                }
                20 => {
                    // Key Already Taken (input 3538): ONLY b:B (21, the "OK"
                    // soft key) -> mode 5, consumed; everything else falls
                    // to the tail.
                    if key == 21 {
                        self.set_mode(5);
                        self.latched = KEY_SENTINEL;
                        self.released = false;
                        return;
                    }
                }
                1 => {
                    // The SHOP (input 1359): `i5 = a(i5)` (the tail sees
                    // it); b:B (21) CLOSES the f menu outright (f.a:B = 0 —
                    // no hierarchical pop, unlike mode 2) + mode 3 (k:B
                    // still 4); a:B (22) is swallowed; everything else
                    // feeds `f.a(char)` (tabs/cursor/FIRE -> the Buy/Sell
                    // activation). Every branch p:B = 1.
                    i5 = self.remap(key);
                    let i5r = i5;
                    if key == 21 {
                        self.fmenu.open = false;
                        self.set_mode(3);
                    } else if key != 22 && self.fmenu.open {
                        let items_page = self.lang.get(27).to_string();
                        if let Some(node) = self.fmenu.input(i5r, &items_page) {
                            self.activate_item(node);
                        }
                    }
                    self.released = true; // p:B = 1 (1421)
                }
                22 => {
                    // The interrupt screen (input 3565): a:B (22, "YES") ->
                    // restore the saved mode (through the setter — the op73
                    // gate applies), clear n:B + the f overlay, consume;
                    // b:B (21, "EXIT") -> c() (falls to the tail).
                    if key == 22 {
                        self.set_mode(self.saved_mode);
                        self.saved_mode = -1;
                        self.fmenu.resume_overlay = false;
                        self.latched = KEY_SENTINEL;
                        self.released = false;
                        return;
                    } else if key == 21 {
                        self.exit_c();
                    }
                }
                _ => {}
            }
        }
        // TAIL (3615): a latch consumed by a mode arm (the quick keys, the
        // weapon toggle) skips the VM feed entirely — the sentinel check is
        // the tail's FIRST instruction.
        if self.latched == KEY_SENTINEL {
            return;
        }
        // f.a == 0 -> feed the VM (mode-0 keys arrive remapped, arming
        // the op14 handlers) and the dialogue input `d(char)` (scroll/
        // dismiss); an OPEN f menu swallows both.
        if !self.fmenu.open {
            self.vm.feed_key(i5);
            self.dialogue_input(i5);
        }
        if self.released {
            self.latched = KEY_SENTINEL;
            self.released = false;
        }
    }

    /// `b.a(I)I` (javap 11492) — the full key remap, in bytecode order: the
    /// direction/fire equivalences FIRST (1|50→3, 6|56→4, 2|52→5, 5|54→6,
    /// 8|20|53→7 — so those digits can never be shadowed by a binding), then
    /// the LIVE bindings `f:[B` → 0/1/2, else the `-1122868` sentinel (the
    /// real fall-through — NOT the raw key; nothing downstream matches it).
    fn remap(&self, key: i32) -> i32 {
        match Action::from_key(key) {
            Some(Action::Up) => return 3,
            Some(Action::Down) => return 4,
            Some(Action::Left) => return 5,
            Some(Action::Right) => return 6,
            Some(Action::Fire) => return 7,
            None => {}
        }
        if key == self.bindings[0] {
            return 0; // quick health
        }
        if key == self.bindings[1] {
            return 1; // quick magika
        }
        if key == self.bindings[2] {
            return 2; // toggle weapon
        }
        REMAP_NONE
    }

    /// `b.a(II)Z` (javap 13709) — can `key` bind to the selected mode-5 row?
    /// Its stored form must not collide with another EDIT-table slot (the
    /// row being edited is skipped — rebinding a slot to its own key is
    /// fine); remapped direction/fire actions (3..=7 — so the hardwired
    /// digits 2/4/5/6/8 are unbindable) and the soft keys are reserved.
    fn key_valid(&self, key: i32, action: i32) -> bool {
        let k1 = normalized_key(key);
        for (i, &b) in self.bindings_edit.iter().enumerate() {
            if i != self.redef_cursor && k1 == b {
                return false; // taken -> mode 20 "Key Already Taken"
            }
        }
        if (3..=7).contains(&action) {
            return false;
        }
        k1 != 22 && k1 != 21
    }

    /// `b(J)` case 0 (input 1804) — the gameplay key dispatch. Returns `true`
    /// when the key was consumed before the tail.
    fn gameplay_input(&mut self, raw: i32, i5: i32, dt_ms: i32) -> bool {
        // a:B (22): the in-game action menu — n() + mode 2, latch consumed
        // (b(J) 356-392; the p:B flag is left alone — the tail's sentinel
        // check ends the dispatch).
        if raw == 22 {
            if self.world.actors[0].is_none() || !self.world.hud_enabled {
                return false;
            }
            self.n_action_menu();
            self.set_mode(2);
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
            let World {
                actors,
                actor_anims,
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
                    p.set_anim(1, actor_anims[0].as_mut());
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
                // h.a(j, i5 == 0) — quick health / quick magika-fatigue use
                // (b(J) 611/639). The latch is consumed (a:I = sentinel,
                // p:B = 0) and flow FALLS THROUGH to the overlay resample
                // below (goto 1216) — the consumed latch then skips the VM
                // tail via its sentinel check.
                if let Some(p) = self.world.actors[0].as_mut() {
                    p.quick_use(i5 == 0, &self.vm.tables);
                }
                self.latched = KEY_SENTINEL;
                self.released = false;
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
            if let Some(p) = self.world.actors[0].as_mut() {
                p.var_byte_n = 0; // var_j_a.var_byte_n = 0
            }
            self.released = true; // var_byte_p = 1
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

    /// `e.int_a(String)` — reverse-resolve a display name to its name ref:
    /// an exact match in the CURRENT script's string pool wins (the index),
    /// else the BASE-lang reverse lookup (`0xF000 | id`), else `None`.
    fn name_ref(&self, name: &str) -> Option<i32> {
        if let Some(i) = self.vm.pool_reverse(name) {
            return Some(i);
        }
        self.lang.reverse(name).map(|id| 0xF000 | i32::from(id))
    }

    /// `e.b(String)` — resolve a display name to its ITEM KIND: the scan
    /// order is weapons (4) -> 0, consumables (2) -> 2, armor (1) -> 1;
    /// -1 when absent (the shop activation still charges/pays, faithful).
    fn kind_by_name(&self, name: &str) -> i32 {
        let Some(n) = self.name_ref(name) else {
            return -1;
        };
        for (subtype, kind) in [(4u8, 0i32), (2, 2), (1, 1)] {
            for row in self.vm.tables.rows(subtype) {
                if row.len() > 1 && row[0] != 0 && row[1] == n {
                    return kind;
                }
            }
        }
        -1
    }

    /// `e.int_arr_a(String)` — resolve a display name to its stat row: the
    /// table scan order is weapons (4), consumables (2), armor (1), classes
    /// (5), spells (8); unwritten rows (id 0) are the Java nulls, skipped.
    fn row_by_name(&self, name: &str) -> Option<Vec<i32>> {
        let n = self.name_ref(name)?;
        for &subtype in &[4u8, 2, 1, 5, 8] {
            for row in self.vm.tables.rows(subtype) {
                if row.len() > 1 && row[0] != 0 && row[1] == n {
                    return Some(row.clone());
                }
            }
        }
        None
    }

    /// `b.n()` (javap 9498) — build the mode-2 action menu from the live
    /// player: the Attack page (weapons with the lang-305/400 prefix + the
    /// class spell list with lang-304), the Armor page (eight lang-28..35
    /// slot sub-pages), the Items page (kind-2 inventory), and the
    /// Character Stats rows; then open the f menu (tabs {4,1,2,3,18},
    /// status null) and dirty the base-map cache (the original bakes the
    /// menu background into b's offscreen).
    fn n_action_menu(&mut self) {
        let g = |id: u16| self.lang.get(id).to_string();
        let mut items: Vec<MenuItem> = Vec::new();
        let add = |items: &mut Vec<MenuItem>, item: MenuItem| -> usize {
            items.push(item);
            items.len() - 1
        };
        let attack = add(&mut items, MenuItem::new(g(25), None, false));
        let armor = add(&mut items, MenuItem::new(g(26), None, false));
        let items_pg = add(&mut items, MenuItem::new(g(27), None, false));
        let stats = add(&mut items, MenuItem::new(g(394), None, false));
        let mut slot_pages = [0usize; 8];
        for (i, &id) in [28u16, 29, 30, 31, 32, 33, 34, 35].iter().enumerate() {
            let pg = add(&mut items, MenuItem::new(g(id), None, false));
            items[pg].parent = Some(armor);
            items[armor].children.push(pg);
            slot_pages[i] = pg;
        }
        let p = self.world.actors[0].as_ref().expect("n() needs the player");
        // The stats rows (labels get ": " appended; attribute values ×3; the
        // 42/40 speed/luck values are hardcoded in the original).
        let class_name = match p.var_byte_f {
            4 => g(12),
            3 => g(11),
            8 => g(16),
            5 => g(13),
            1 => g(9),
            2 => g(10),
            7 => g(15),
            6 => g(14),
            _ => String::new(),
        };
        let lvl = i32::from(p.var_byte_o);
        let xp_next = if (lvl as usize) < formats::actor::XP_THRESHOLD.len() - 1 {
            (formats::actor::XP_THRESHOLD[(lvl + 1) as usize] - p.var_int_b).to_string()
        } else {
            "0".into()
        };
        let rows: Vec<Option<String>> = vec![
            Some(format!("{}: ", g(443))),
            Some(class_name),
            Some(format!("{}: ", g(17))),
            Some(lvl.to_string()),
            Some(format!("{}: ", g(441))),
            Some(p.var_int_b.to_string()),
            Some(format!("{}: ", g(442))),
            Some(xp_next),
            Some(format!("{}: ", g(415))),
            Some((i32::from(p.var_short_s) * 3).to_string()),
            Some(format!("{}: ", g(416))),
            Some((i32::from(p.var_short_t) * 3).to_string()),
            Some(format!("{}: ", g(417))),
            Some((i32::from(p.var_short_u) * 3).to_string()),
            Some(format!("{}: ", g(418))),
            Some((i32::from(p.var_short_v) * 3).to_string()),
            Some(format!("{}: ", g(419))),
            Some((i32::from(p.var_short_x) * 3).to_string()),
            Some(format!("{}: ", g(420))),
            Some((i32::from(p.var_short_y) * 3).to_string()),
            Some(format!("{}: ", g(431))),
            Some((i32::from(p.prog_c) * 3).to_string()),
            Some(format!("{}: ", g(432))),
            Some((i32::from(p.prog_d) * 3).to_string()),
            Some(format!("{}: ", g(563))),
            Some("42".into()),
            Some(format!("{}: ", g(562))),
            Some("40".into()),
            Some(format!("{}: ", g(38))),
            Some(self.world.gold.to_string()),
        ];
        items[stats].stat_rows = Some(rows);
        // The inventory walk (var_int_arr_k until 0).
        self.active_weapon_item = None;
        self.active_spell_item = None;
        let inv: Vec<i32> = p
            .var_int_arr_k
            .iter()
            .take_while(|&&v| v != 0)
            .copied()
            .collect();
        let spells: Vec<i32> = p
            .var_int_arr_h
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .take_while(|&&v| v != -1)
            .copied()
            .collect();
        let (arr_f, arr_g) = (p.var_int_arr_f.clone(), p.var_int_arr_g.clone());
        let player = p.clone();
        let mut seen_weapon = false;
        let mut seen_slot = [false; 9];
        let (mut seen_f, mut seen_g) = (false, false);
        for entry in inv {
            let kind = (entry >> 8) & 0xFF;
            let idx = entry & 0xFF;
            match kind {
                0 => {
                    let row = self.vm.tables.row(4, idx).expect("weapon row").to_vec();
                    let tname = self.item_name(&row);
                    let bow = row[2] == 4;
                    let prefix = if bow { g(400) } else { g(305) };
                    let active = player.is_active_row(&row, false) && !seen_weapon;
                    let mut item = MenuItem::new(
                        format!("{prefix}{tname}"),
                        Some(format!("{}: {}", g(432), row[3])),
                        active,
                    );
                    item.enabled = player.class_allows_item(0, &row, &self.vm.tables);
                    item.parent = Some(attack);
                    let node = add(&mut items, item);
                    items[attack].children.push(node);
                    // b.var_c_a is set in the NON-bow branch only (javap 1527).
                    if !bow && player.is_active_row(&row, false) {
                        self.active_weapon_item = Some(node);
                    }
                    seen_weapon |= player.is_active_row(&row, false);
                }
                1 => {
                    let row = self.vm.tables.row(1, idx).expect("armor row").to_vec();
                    let equipped = player.has_armor_equipped(row[0]);
                    let slot = row[3] as usize;
                    let mut item = MenuItem::new(
                        self.item_name(&row),
                        Some(format!("{}: {}", g(444), row[4])),
                        equipped && !seen_slot[slot],
                    );
                    item.enabled = player.class_allows_item(1, &row, &self.vm.tables);
                    item.parent = Some(slot_pages[slot]);
                    let node = add(&mut items, item);
                    items[slot_pages[slot]].children.push(node);
                    seen_slot[slot] |= equipped;
                }
                2 => {
                    let row = self.vm.tables.row(2, idx).expect("consumable row").to_vec();
                    let mut active = false;
                    // The original compares row REFERENCES against the armed
                    // arr_f/arr_g (they point into the same table); value
                    // equality is equivalent — rows carry unique ids.
                    if arr_f.as_deref() == Some(row.as_slice()) && !seen_f {
                        active = true;
                        seen_f = true;
                    }
                    if arr_g.as_deref() == Some(row.as_slice()) && !seen_g {
                        active = true;
                        seen_g = true;
                    }
                    let name = self.item_name(&row);
                    let mut item = MenuItem::new(name.clone(), None, active);
                    item.potion_group = if name == g(149) || name == g(151) {
                        1
                    } else if name == g(150) || name == g(152) {
                        2
                    } else {
                        0
                    };
                    item.parent = Some(items_pg);
                    let node = add(&mut items, item);
                    items[items_pg].children.push(node);
                }
                _ => {}
            }
        }
        for id in spells {
            let row = self.vm.tables.row(8, id).expect("spell row").to_vec();
            let active = player.is_active_row(&row, true);
            let mut item =
                MenuItem::new(format!("{}{}", g(304), self.item_name(&row)), None, active);
            item.parent = Some(attack);
            let node = add(&mut items, item);
            items[attack].children.push(node);
            if active {
                self.active_spell_item = Some(node);
            }
        }
        self.fmenu.open(
            vec![4, 1, 2, 3, 18],
            items,
            vec![attack, armor, items_pg, stats],
            None,
            &self.masks,
            &self.assets,
        );
        self.world.dirty = true; // var_boolean_n = true (the baked-over offscreen)
    }

    /// `b.o()` (javap 10613) — build the two-tab SHOP menu: Buy from the
    /// subtype-7 flat [kind,id] pair list (full prices), Sell from the
    /// player's inventory tags (quarter prices, `>> 2`). Weapon/armor nodes
    /// carry a rating desc and the class-allows flag (`c.b:Z` — CFR shows
    /// its phantom `new c` again; javap writes the SAME node); consumables
    /// keep the field-init `enabled`. Opens the f menu (bar 17, icons
    /// 15/16, status = the gold line) and dirties the base cache. The
    /// CALLER sets mode 1.
    fn o_shop(&mut self) {
        let g = |id: u16| self.lang.get(id).to_string();
        let gold_word = g(38);
        let mut items: Vec<MenuItem> = vec![
            MenuItem::new(g(36), None, false), // Buy
            MenuItem::new(g(37), None, false), // Sell
        ];
        let (buy, sell) = (0usize, 1usize);
        // One stock/inventory entry; `quarter` halves twice for the Sell tab.
        let add = |items: &mut Vec<MenuItem>,
                   shell: &Shell,
                   tab: usize,
                   kind: i32,
                   id: i32,
                   quarter: bool| {
            let (subtype, price_col, desc) = match kind {
                0 => (4u8, 7usize, Some((432u16, 3usize))),
                1 => (1, 9, Some((444, 4))),
                2 => (2, 13, None),
                // subtype-3 stock never appears in shipped data
                3 => unimplemented!("shop flat7 kind 3 (subtype-3 stock) not in shipped data"),
                k => panic!("shop stock kind {k} out of range"),
            };
            let row = shell
                .vm
                .tables
                .row(subtype, id)
                .expect("shop stock row")
                .to_vec();
            let price = if quarter {
                row[price_col] >> 2
            } else {
                row[price_col]
            };
            let label = format!("{} : {} {}", shell.item_name(&row), price, gold_word);
            let mut item = MenuItem::new(
                label,
                desc.map(|(lid, col)| format!("{}: {}", shell.lang.get(lid), row[col])),
                false,
            );
            if matches!(kind, 0 | 1) {
                // c.b:Z = h.a(j, kind, row)Z — red + fire-dead when the
                // class may not use it (or there is NO player: h.boolean_a
                // returns false for j == null — oracle-observed as an
                // all-red Buy tab on a menu-only shop).
                item.enabled = shell.world.actors[0]
                    .as_ref()
                    .is_some_and(|p| p.class_allows_item(kind, &row, &shell.vm.tables));
            }
            item.parent = Some(tab);
            items.push(item);
            let node = items.len() - 1;
            items[tab].children.push(node);
        };
        // BUY: the subtype-7 flat pair list, -1 terminated.
        let flat = self.vm.flat7.clone();
        let mut n = 0;
        while flat[n] != -1 {
            let kind = flat[n];
            let id = flat[n + 1];
            n += 2;
            add(&mut items, self, buy, kind, id, false);
        }
        // SELL: the inventory tags (kind << 8 | id), zero-terminated.
        if let Some(p) = self.world.actors[0].as_ref() {
            let tags: Vec<i32> = p
                .var_int_arr_k
                .iter()
                .take_while(|&&t| t != 0)
                .copied()
                .collect();
            for tag in tags {
                add(&mut items, self, sell, (tag >> 8) & 0xFF, tag & 0xFF, true);
            }
        }
        let status = format!("{} : {}", gold_word, self.world.gold);
        self.fmenu.open(
            vec![17, 15, 16],
            items,
            vec![buy, sell],
            Some(status),
            &self.masks,
            &self.assets,
        );
        self.world.dirty = true; // n:Z
    }

    /// `b.a(c)` (b.java:2763) — the menu activation callback, dispatched on
    /// the fired node's PARENT page name: Buy/Sell (lang 36/37) is the shop
    /// (out of slice, loud); the Armor top page (lang 26 — descending into a
    /// slot sub-page) un-marks the node; Attack (lang 25) arms the weapon or
    /// spell by NAME (`h.a(j,String)`) with the `var_c_a`/`var_c_b`
    /// cross-marking; Items (lang 27) USES the consumable (`h.b(j,int[])`);
    /// anything else (the armor slot pages) equips the armor (`h.c`).
    fn activate_item(&mut self, node: usize) {
        let parent = self.fmenu.items[node]
            .parent
            .expect("fired node has a page");
        let page_name = self.fmenu.items[parent].name.clone();
        let name = self.fmenu.items[node].name.clone();
        let is = |id: u16| page_name == self.lang.get(id);
        if is(36) {
            // BUY (a(c) head): parse the price out of the LABEL ("Name : 25
            // Gold"); a parse failure is the original's System.exit(1).
            let colon = name.find(':').expect("shop label has ' : '");
            let start = colon + 2;
            let end = name[start..].find(' ').expect("price ends at a space") + start;
            let price: i32 = name[start..end]
                .parse()
                .expect("IsoMap::menuSelected() buy — the original exits here");
            if self.world.gold >= price {
                self.world.gold -= price;
                let item = name[..start - 3].to_string();
                let kind = self.kind_by_name(&item);
                let row = self.row_by_name(&item);
                if let Some(row) = row {
                    if self.world.actors[0].is_some() {
                        // h.a(j,kind,row)V = the force=false equip, then the
                        // FULL o() rebuild (cursor/page reset — faithful).
                        let tables = &self.vm.tables;
                        if let Some(p) = self.world.actors[0].as_mut() {
                            p.equip(kind, &row, false, tables);
                        }
                        self.o_shop();
                    }
                }
            }
            // c2.a:Z = false lands on the pre-rebuild node (a no-op when the
            // rebuild replaced the arena); the title always refreshes.
            if let Some(item) = self.fmenu.items.get_mut(node) {
                item.active = false;
            }
            self.fmenu.status = Some(format!("{} : {}", self.lang.get(38), self.world.gold));
            return;
        }
        if is(37) {
            // SELL: pay the label's quarter price, remove the item (h.b —
            // disarm/shift/re-arm best/re-derive) and the NODE (with the
            // f.a('\u{3}') cursor-up fix when it was last in the page).
            let colon = name.find(':').expect("shop label has ' : '");
            let start = colon + 2;
            let end = name[start..].find(' ').expect("price ends at a space") + start;
            let price: i32 = name[start..end]
                .parse()
                .expect("IsoMap::menuSelected() //sell — the original exits here");
            if self.world.actors[0].is_some() {
                let item = name[..start - 3].to_string();
                let kind = self.kind_by_name(&item);
                if let Some(row) = self.row_by_name(&item) {
                    let tables = &self.vm.tables;
                    if let Some(p) = self.world.actors[0].as_mut() {
                        p.unequip(kind, &row, tables);
                    }
                }
                let siblings = &self.fmenu.items[parent].children;
                if siblings.last() == Some(&node) {
                    let items_page = self.lang.get(27).to_string();
                    self.fmenu.input(3, &items_page); // f.a('\u{3}') = UP
                }
                let siblings = &mut self.fmenu.items[parent].children;
                siblings.retain(|&c| c != node); // removeElement
            }
            self.fmenu.items[node].active = false;
            self.world.gold += price;
            self.fmenu.status = Some(format!("{} : {}", self.lang.get(38), self.world.gold));
            return;
        }
        if is(26) {
            self.fmenu.items[node].active = false;
            return;
        }
        if is(25) {
            // h.a(j, String): the lang-304 prefix = spell, lang-400 = bow,
            // else the lang-305 weapon prefix.
            let spell = name.starts_with(self.lang.get(304));
            let (stripped, bow) = if spell {
                (name[self.lang.get(304).len()..].to_string(), false)
            } else if name.starts_with(self.lang.get(400)) {
                (name[self.lang.get(400).len()..].to_string(), true)
            } else {
                (name[self.lang.get(305).len()..].to_string(), false)
            };
            let row = self.row_by_name(&stripped).expect("attack row by name");
            if let Some(p) = self.world.actors[0].as_mut() {
                if spell {
                    p.activate_spell(&row);
                } else {
                    p.activate_weapon(&row, bow);
                }
            }
            if spell {
                if let Some(w) = self.active_weapon_item {
                    self.fmenu.items[w].active = true;
                }
                self.active_spell_item = Some(node);
            } else {
                if let Some(sp) = self.active_spell_item {
                    self.fmenu.items[sp].active = true;
                }
                self.active_weapon_item = Some(node);
            }
            return;
        }
        if is(27) {
            let row = self.row_by_name(&name).expect("item row by name");
            let is_vicar = self.item_name(&row) == self.lang.get(158);
            if let Some(p) = self.world.actors[0].as_mut() {
                p.use_consumable(&row, is_vicar, &self.vm.tables);
            }
            return;
        }
        // The armor slot pages: h.c(j, row) — permission-gated equip.
        let row = self.row_by_name(&name).expect("armor row by name");
        if let Some(p) = self.world.actors[0].as_mut() {
            p.equip_armor(&row, &self.vm.tables);
        }
    }

    /// `b.p()` (javap 11641) — zero every page cursor and reset the page to
    /// the main (or in-game pause) menu. Called by the death screen's and
    /// Load-Game's `b:B` branches.
    fn p_reset(&mut self) {
        self.cursors = [0; 7];
        self.page = if self.left_gameplay { 5 } else { 0 };
    }

    /// `b.g()` (b.java:2845) — write the `ESO` record from the live state
    /// (progress flags + sound flag + the level-script name + the player
    /// blob, `active` bits recomputed) into the in-memory save slot.
    fn save_game(&mut self) {
        let blob = crate::save::build_save(
            self.bindings,
            self.bool_o,
            &self.level_script,
            self.world.actors[0].as_ref(),
            self.world.gold,
            &self.vm.tables,
        );
        self.save_slot = Some(blob);
    }

    /// `b.boolean_b()` (b.java:2875) — is there a saved player? The `l()` menu
    /// build gates "Load Game" / the New Game overwrite confirm on it.
    fn has_save(&self) -> bool {
        crate::save::has_save(self.save_slot.as_deref())
    }

    /// `b.h()` = `b.b(true)` (b.java:2900) — load the `ESO` record: restore the
    /// progress + sound flags, mode 6, then (if a player is stored) re-run the
    /// loader on the saved level-script name and install the restored player
    /// into slot 0. The subsequent level choreography spawns the world around
    /// the reused `var_j_a`, exactly like a fresh class fire.
    fn load_game(&mut self) -> anyhow::Result<()> {
        let Some(blob) = self.save_slot.clone() else {
            return Ok(());
        };
        let save = formats::parse_save(&blob).map_err(|e| anyhow::anyhow!("save parse: {e}"))?;
        self.bindings = [
            i32::from(save.flags[0]),
            i32::from(save.flags[1]),
            i32::from(save.flags[2]),
        ];
        self.bool_o = save.bool_o != 0;
        self.set_mode(6);
        if let Some(sp) = save.player {
            let name = String::from_utf8_lossy(&sp.name).into_owned();
            // b.var_int_b = the saved gold (h.a(byte[],int) writes it back).
            self.world.gold = i32::from(sp.actor.global_int_b);
            let player = crate::save::restore_actor(&sp.actor, &mut self.models, &self.vm.tables);
            self.loader(&name)?;
            // var_j_a = var_j_arr_a[0] = the restored actor (survives the
            // loader's per-level reset, like the class-fire player reuse).
            self.world.actors[0] = Some(player);
            self.world.actor_anims[0] = Some(formats::anim::Anim::from_cml(
                &self.models.get("/oh_pc.cml").cml,
            ));
            self.world.player_persists = true;
        }
        Ok(())
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
            // Save Game (2093): g() writes the record, e[k]=2, mode 13.
            self.save_game();
            self.cursors[self.page as usize] = 2;
            self.set_mode(13);
        } else if is(3) {
            // Load Game (1887): k() nulls the overview caches, mode 14, and
            // the key is consumed (sentinel — it never feeds the VM tail).
            self.k_clear();
            self.set_mode(14);
            self.latched = KEY_SENTINEL;
        } else if is(21) {
            self.set_mode(0); // Continue (resume in-game pause)
            self.world.dirty = true;
        } else if is(2) {
            // New Game (2143): if a save exists, the mode-16 overwrite
            // confirm; else straight to class select.
            if self.has_save() {
                self.set_mode(16);
            } else {
                self.page = 1;
            }
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
            // Custom Controls (2133): copy the LIVE bindings into the edit
            // table (g <- f), consume the key, mode 5. The cursor (k:I) is
            // NOT reset — it persists across visits (only j() zeroes it).
            self.bindings_edit = self.bindings;
            self.latched = KEY_SENTINEL;
            self.set_mode(5);
        } else if is(573) {
            // Game Overview (2220): l:S, w/v reset, b:[[Str = a() (built but
            // unused by the mode-23 text-page paint), then mode 23.
            self.enter_stat_table(573);
        } else if is(522) {
            self.enter_stat_table(522); // Classes Overview -> b()
        } else if is(459) {
            self.enter_stat_table(459); // Weapons Overview -> f()
        } else if is(460) {
            self.enter_stat_table(460); // Armor Overview -> e()
        } else if is(461) {
            self.enter_stat_table(461); // Spells Overview -> d()
        } else if is(462) {
            self.enter_stat_table(462); // Items Overview -> c()
        } else if is(18) {
            // Go Shopping (2470): o() builds the Buy/Sell menu, mode 1.
            self.o_shop();
            self.set_mode(1);
        } else if is(20) {
            // Continue Playing (2575): a((byte)0) only — no dirty write
            // (o() already dirtied the base cache if the shop was opened).
            self.set_mode(0);
        } else if page == 1 {
            // k==1 class fire (2593 — checked BEFORE the Exit compare):
            // k(); r:B = 0; mode 6; null actors; the loader on the HARDCODED
            // /l01_1.scr; b:I = 100 (the starting gold). The chosen class
            // reaches the spawner via the class-select cursor (e:[B[1]).
            self.k_clear();
            self.progress = 0;
            // 2644-2668: null EVERY slot AND `var_j_a` (dup_x2 aastore +
            // putfield each iteration) — a class fire ALWAYS builds a fresh
            // player (the factory spawn), even right after the outro.
            for a in self.world.actors.iter_mut() {
                *a = None;
            }
            for a in self.world.actor_anims.iter_mut() {
                *a = None;
            }
            self.world.player_persists = false;
            self.loader("/l01_1.scr").expect("class-fire level load");
            self.world.gold = 100;
        } else if is(22) {
            self.set_mode(19); // Exit -> confirm dialog (2712: a((byte)19))
        } else {
            panic!("menu item not in the ported compare chain: {item:?}");
        }
    }

    /// `b.k()` (2345) — null the `b..h:[[String` overview caches. Only two
    /// call sites: the Load-Game fire and the class fire.
    fn k_clear(&mut self) {
        self.stat_caches.clear();
    }

    /// The shared overview-item fire tail (2220..2545): `l:S = <id>`,
    /// `w:B = v:B = 0`, `b:[[String` = the (memoized) builder, then mode 18
    /// (mode 23 for Game Overview, whose table the text-page paint ignores).
    fn enter_stat_table(&mut self, id: u16) {
        self.topic = id;
        self.stat_w = 0;
        self.stat_v = 0;
        let lang = &self.lang;
        let vm = &self.vm;
        let caches = &mut self.stat_caches;
        self.stat_table = match id {
            573 => caches
                .overview
                .get_or_insert_with(|| crate::stattab::overview(lang)),
            522 => caches
                .classes
                .get_or_insert_with(|| crate::stattab::classes(lang, vm)),
            459 => caches
                .weapons
                .get_or_insert_with(|| crate::stattab::weapons(lang, vm)),
            460 => caches
                .armor
                .get_or_insert_with(|| crate::stattab::armor(lang, vm)),
            461 => caches
                .spells
                .get_or_insert_with(|| crate::stattab::spells(lang, vm)),
            462 => caches
                .items
                .get_or_insert_with(|| crate::stattab::items(lang, vm)),
            other => unreachable!("no overview builder for lang {other}"),
        }
        .clone();
        self.set_mode(if id == 573 { 23 } else { 18 });
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
            (11, _) => Some(Screen::Death),
            (2, _) => Some(Screen::ActionMenu),
            (13, _) => Some(Screen::GameSaved),
            (14, _) => Some(Screen::LoadConfirm),
            (16, _) => Some(Screen::OverwriteConfirm),
            (5, _) => Some(Screen::ControlsRedefine),
            (20, _) => Some(Screen::KeyTaken),
            (18, _) => Some(Screen::StatTable),
            (1, _) => Some(Screen::Shop),
            (22, _) => Some(Screen::Interrupt),
            _ => None,
        }
    }

    /// The normalized-shot determinism reset, mirroring the oracle's
    /// `Instrument.normalizeWorld`: reset every anim cursor (the singleton
    /// models + each actor's own instance), zero the effect pool's anim
    /// counters, re-follow the camera target (view = center - iso; sets the
    /// dirty flag so the next paint's r() recenters with the NORMALIZED pose
    /// height), then run the paint-state pass (r + q). Render afterward.
    pub fn normalize_for_shot(&mut self) {
        self.models.reset_all_anims();
        for anim in self.world.actor_anims.iter_mut().flatten() {
            anim.reset_all();
        }
        self.world.effects.reset_anim_counters();
        let q = self.world.cam_follow;
        if q >= 0 {
            self.world.camera_follow(i32::from(q));
        }
        crate::gpaint::r_camera(&mut self.world, &mut self.models);
        crate::gpaint::q_range(&mut self.world);
    }

    pub fn mode(&self) -> i8 {
        self.mode
    }

    /// The unported content boundary the VM hit, if any (a frontend renders
    /// an honest stop screen instead of a broken world). Empty since loop
    /// #22 ported op47 (the L01 sewer maze) — the next boundary would be a
    /// deeper-level opcode.
    pub fn unported_boundary(&self) -> Option<String> {
        self.unported_op
            .map(|op| format!("Reached unported content (script op{op})."))
    }

    /// The wrapped text-page model (`a:[Ljava/util/Vector;`) — the fixed-
    /// scroll anchor tests derive the mask corpus from it.
    pub fn text_pages(&self) -> &[Vec<String>] {
        &self.text_pages
    }

    /// The live script stat tables (`b.var_e_a`) — test access (the anchor
    /// tests arm items through the same rows the game reads).
    pub fn tables(&self) -> &formats::Tables {
        &self.vm.tables
    }

    /// The lang table — test/corpus-tooling access.
    pub fn lang(&self) -> &formats::lang::Lang {
        &self.lang
    }

    /// The subtype-7 flat pair list (`e.a(7,0)` — the shop stock) — test
    /// access.
    pub fn flat7(&self) -> &[i32] {
        &self.vm.flat7
    }

    /// The `callmode` injection: invoke the REAL mode setter `b.a((byte)n)`
    /// (arms and all — unlike `setmode`'s raw field write). The outro drive
    /// uses it as the op61 stand-in.
    pub fn call_mode(&mut self, n: i8) {
        self.set_mode(n);
    }

    /// The `calllang` injection: the op56 native `b.a(String, int)` for an
    /// overlay id (the outro text lang 547 lives in lang_12).
    pub fn load_lang_overlay(&mut self, id: u16) {
        let file = format!("lang_{id}.txt");
        let bytes = std::fs::read(self.assets_dir.join(&file)).expect("overlay lang");
        let overlay = formats::parse_lang_file(&bytes, id as u8)
            .unwrap_or_else(|| panic!("unknown lang table id {id}"));
        self.lang.set_overlay(overlay);
    }

    /// The `callscript` injection: the real `b.a(String)` loader invoked
    /// directly (the op29 native) — jumps the drive to any script without
    /// walking the choreography to its exit trigger.
    pub fn call_script(&mut self, name: &str) {
        self.loader(name).expect("callscript loader");
    }

    /// The `setseed` injection: re-base the shared combat/maze RNG
    /// (`b.var_java_util_Random_a.setSeed(seed)` on the oracle side) so a
    /// following `callmaze` is a pure function of the seed on both sides.
    pub fn set_seed(&mut self, seed: i64) {
        self.world.rng.set_seed(seed);
    }

    /// The `callentry` injection: the real `e.a(int)` script-entry push —
    /// the exact mechanism an actor's death trigger and the overlay events
    /// use. Lets a drive run a trigger-gated entry (e.g. the maze boss's
    /// death entry) without the RNG/wall-clock of a real kill.
    pub fn call_entry(&mut self, n: u8) {
        self.vm.push_entry(n);
    }

    /// The op47 native `b.a(int_arr_a(9, row), var_int_arr_g, n, n2)` — also
    /// the `callmaze` injection. Resolves the subtype-9 config row, the
    /// tag-20 pickup list, and the enemy stat row/model exactly like the
    /// dispatch site, then runs the generator.
    pub fn op47_maze(&mut self, row: i32, n: i32, n2: i32) {
        let cfg = self
            .vm
            .tables
            .row(9, row)
            .expect("op47 subtype-9 row")
            .to_vec();
        let slots9 = self.vm.slots9.clone();
        let stat_row = self
            .vm
            .tables
            .row(0, cfg[17])
            .expect("op47 enemy stat row")
            .to_vec();
        let model = self.vm.pool_string(stat_row[1]).to_string();
        crate::maze::generate(
            &mut self.world,
            &self.vm.tables,
            &mut self.models,
            &cfg,
            &slots9,
            &stat_row,
            &model,
            n,
            n2,
            self.cursors[1] as i32,
        );
    }

    /// The `setflat` injection: write the shop stock pairs + the -1
    /// terminator into `e.f:[I` (mirrors `Instrument.setFlat`).
    pub fn set_flat7(&mut self, vals: &[i32]) {
        for (i, &v) in vals.iter().enumerate().take(self.vm.flat7.len()) {
            self.vm.flat7[i] = v;
        }
        if vals.len() < self.vm.flat7.len() {
            self.vm.flat7[vals.len()] = -1;
        }
    }

    /// Build all six overview tables fresh (corpus/regen tooling; bypasses
    /// the caches on purpose — the content is what matters).
    pub fn all_stat_tables(&self) -> Vec<(u16, Vec<Vec<String>>)> {
        vec![
            (573, crate::stattab::overview(&self.lang)),
            (522, crate::stattab::classes(&self.lang, &self.vm)),
            (459, crate::stattab::weapons(&self.lang, &self.vm)),
            (460, crate::stattab::armor(&self.lang, &self.vm)),
            (461, crate::stattab::spells(&self.lang, &self.vm)),
            (462, crate::stattab::items(&self.lang, &self.vm)),
        ]
    }

    /// Invoke `b.g()` and return the written `ESO` blob — the save-parity
    /// gate compares this to the real game's captured record.
    pub fn save_and_get_blob(&mut self) -> Vec<u8> {
        self.save_game();
        self.save_slot.clone().expect("save wrote a blob")
    }

    /// Firing "Save Game" (`b.g()` + mode 13) — the page-4 in-game menu path
    /// is script-gated (op45), so the m13 parity test enters it directly.
    pub fn enter_save_screen_for_test(&mut self) {
        self.save_game();
        self.set_mode(13);
    }

    /// The text-page scroll `g:S` — read/set for the fixed-scroll anchors
    /// (the oracle side injects the same value via `setscroll`).
    pub fn scroll_value(&self) -> i16 {
        self.scroll
    }

    pub fn set_scroll(&mut self, g: i16) {
        self.scroll = g;
        self.scroll_acc = 0; // h:S = 0
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
            21 => {
                let ui = self.ui_model.clone();
                let ui_ref = ui.as_deref().map(|n| &*self.models.get(n));
                let final_y = paint_text_page(
                    &mut fb,
                    &self.masks,
                    &self.assets,
                    ui_ref,
                    self.mode,
                    &self.text_pages,
                    &mut self.scroll,
                    None,
                );
                // mode 21's end action is only the b:Z debounce toggle
                self.text_page_end(final_y)?;
            }
            17 | 23 => {
                let title = self.lang.get(self.topic).to_string();
                let ui = self.ui_model.clone();
                let ui_ref = ui.as_deref().map(|n| &*self.models.get(n));
                let final_y = paint_text_page(
                    &mut fb,
                    &self.masks,
                    &self.assets,
                    ui_ref,
                    self.mode,
                    &self.text_pages,
                    &mut self.scroll,
                    Some(&title),
                );
                self.text_page_end(final_y)?;
            }
            4 | 9 | 10 => {
                // About credits roll / the OUTRO page / the intro page: the
                // body + the clipped-band tails (About bottom bar + BACK;
                // the m10 parchment bar; the 53/54 scroll arrows from
                // b:Ld). Their per-frame END transitions live in `tick`
                // (the real loop paints every frame; render here is one of
                // those paints, pixel-realized — running the end debounce
                // again would double-count it).
                let ui = self.ui_model.clone();
                let ui_ref = ui.as_deref().map(|n| &*self.models.get(n));
                paint_text_page(
                    &mut fb,
                    &self.masks,
                    &self.assets,
                    ui_ref,
                    self.mode,
                    &self.text_pages,
                    &mut self.scroll,
                    None,
                );
            }
            0 => {
                let level_model = self
                    .level_model
                    .clone()
                    .expect("mode 0 paint needs the op8 level model (var_d_a)");
                let ui_model = self
                    .ui_model
                    .clone()
                    .expect("mode 0 paint needs the op43 UI model (var_d_b)");
                crate::gpaint::paint_gameplay(
                    &mut fb,
                    &mut self.world,
                    &mut self.models,
                    &self.assets,
                    &self.masks,
                    &self.lang,
                    &level_model,
                    &ui_model,
                    self.level_bg,
                );
            }
            15 => crate::gpaint::paint_please_wait(
                &mut fb,
                &self.masks,
                &mut self.models,
                &self.assets,
            ),
            19 => paint_exit_dialog(&mut fb, &self.masks),
            11 => crate::paint::paint_death(&mut fb, &self.masks),
            13 => crate::paint::paint_game_saved(
                &mut fb,
                &self.masks,
                self.lang.get(450),
                self.lang.get(401),
            ),
            14 => crate::paint::paint_yesno(
                &mut fb,
                &self.masks,
                self.lang.get(451), // "Load Saved Game?"
                None,
            ),
            16 => crate::paint::paint_yesno(
                &mut fb,
                &self.masks,
                self.lang.get(455),       // "Saved Game Exists"
                Some(self.lang.get(464)), // "Overwrite?"
            ),
            5 => {
                // The l() redefine list is the fixed lang 292/293/463/294
                // labels (resolved lazily — all base-table ids, so the
                // overlay can never shadow them); the selected row's current
                // binding renders through the key-name table.
                let title = self
                    .lang
                    .get(if self.capture { 424 } else { 425 })
                    .to_string();
                let items: Vec<String> = [292u16, 293, 463, 294]
                    .iter()
                    .map(|&id| self.lang.get(id).to_string())
                    .collect();
                let save_changes = self.lang.get(294).to_string();
                let binding = (items[self.redef_cursor] != save_changes)
                    .then(|| self.key_name(self.bindings_edit[self.redef_cursor]));
                crate::paint::paint_redefine(
                    &mut fb,
                    &self.masks,
                    &title,
                    &items,
                    self.redef_cursor,
                    &save_changes,
                    binding.as_deref(),
                    self.capture,
                );
            }
            20 => {
                let msg = self.lang.get(566).to_string(); // "Key Already Taken"
                let ok = self.lang.get(567).to_uppercase(); // "OK"
                crate::paint::paint_key_taken(&mut fb, &self.masks, &msg, &ok);
            }
            18 => {
                let title = self.lang.get(self.topic).to_string();
                let ui = self.ui_model.clone();
                let ui_ref = ui.as_deref().map(|n| &*self.models.get(n));
                // q:Z is a PAINT side effect the DOWN input reads.
                self.q_flag = crate::paint::paint_stat_table(
                    &mut fb,
                    &self.masks,
                    &self.assets,
                    ui_ref,
                    &title,
                    &self.stat_table,
                    self.stat_v,
                    self.stat_w,
                    self.topic == 573,
                );
            }
            // b.paint cases 1 and 2 draw NOTHING (1147/1150 -> 5590) — the
            // open f menu (the shop / action menu) paints in the tail below.
            1 | 2 => {}
            22 => {
                // The interrupt screen: lang571 "Resume game?" centered both
                // axes in LARGE bold, "EXIT" (lang22) bottom-left, "YES"
                // (lang426) right-aligned by the PRE-uppercase width. The
                // pre-lang branch (start.txt segments 2/4/3) only fires for
                // an interrupt before the lang load — unreachable outside
                // the guarded boot modes, but transcribed faithfully.
                let (center, exit_label, yes_pre) = if self.lang.get(571).is_empty() {
                    (self.start_txt(2), self.start_txt(4), self.start_txt(3))
                } else {
                    (
                        self.lang.get(571).to_string(),
                        self.lang.get(22).to_string(),
                        self.lang.get(426).to_string(),
                    )
                };
                crate::paint::paint_interrupt(&mut fb, &self.masks, &center, &exit_label, &yes_pre);
            }
            12 => anyhow::bail!(
                "paint mode 12 is terminal: the real paint draws NOTHING (the \
                 LCD keeps the last frame while c() destroys the MIDlet)"
            ),
            other => anyhow::bail!("paint mode {other} not ported (out of slice)"),
        }
        // The paint TAIL (5590-5609): the open f menu draws over whatever the
        // mode painted (mode 2 painted nothing, so the menu IS the frame).
        if self.fmenu.open {
            self.fmenu.paint(&mut fb, &self.masks, &self.assets);
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
                    4 => {
                        // 3690-3718: mode 3, then the page reset
                        // (k:B = f:Z ? 5 : 0). The real 3s freeze
                        // (Thread.sleep) is elided in the virtual timeline.
                        self.set_mode(3);
                        self.page = if self.left_gameplay { 5 } else { 0 };
                    }
                    10 => self.set_mode(0), // the intro chains into gameplay
                    9 => {
                        // The OUTRO end (3590-3672): dt rebase (elided in
                        // virtual time), the menu reset (g:S = 265, h:S = 0,
                        // FRESH page cursors, k = f:Z ? 5 : 0 — f:Z was
                        // zeroed by the m9 entry), then null ALL actor
                        // slots. The surviving `var_j_a` is WRITE-ONLY
                        // garbage afterward (the class fire nulls it, a
                        // load replaces it — oracle-pinned: the post-outro
                        // New Game spawns a FRESH player), so no stash is
                        // modeled. max_actor/camera/effects/the open
                        // dialogue are untouched. Then mode 4 (the credits,
                        // re-inited by its own arm). The 3s freeze elided.
                        self.scroll = SCREEN_H as i16
                            - 8 * self
                                .masks
                                .metrics(crate::text::GameFont::SmallBold)
                                .midp_height as i16;
                        self.scroll_acc = 0;
                        self.cursors = [0; 7];
                        self.page = if self.left_gameplay { 5 } else { 0 };
                        for a in self.world.actors.iter_mut() {
                            *a = None;
                        }
                        for a in self.world.actor_anims.iter_mut() {
                            *a = None;
                        }
                        self.set_mode(4);
                    }
                    other => anyhow::bail!("text-page end transition for mode {other} not ported"),
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
