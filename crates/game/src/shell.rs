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
//! Explicit fences (everything leaving the slice is loud, never guessed):
//! - firing a class yields [`Leave::LoadLevel`] — the level load
//!   (`a("/l01_1.scr")`, mode 6->15->0) is the gameplay slice. The exit
//!   dialog (19), Help (menu page 6), About (4), Basic Controls (17) and
//!   Game Overview (23) ARE ported; Custom Controls (mode 5), the overview
//!   stat tables (mode 18), Save/Load/overwrite (13/14/16) and the pause
//!   items yield [`Leave::Mode`];
//! - `b()Z` (RecordStore has-save probe) is modeled as `false` — the pinned
//!   wiped-RMS baseline (no "Continue" item); the save-capture slice lifts it;
//! - rendering an unported paint mode is an error; mode 4 (About) renders
//!   fenced too — its credits roll always draws a scroll ARROW (`.cml` frame
//!   render, unported) and is animated/visual-only anyway;
//! - the mode-15 please-wait anim, floating-text overlay (`a(J)`), key-name
//!   substitution (`d(char)` redefine buffer), and the in-game `f.java`
//!   menus stay out of slice.

use crate::asset::Assets;
use crate::fb::Fb;
use crate::paint::{
    paint_exit_dialog, paint_loader, paint_menu_page, paint_startup, paint_text_page, SCREEN_H,
    SCREEN_W,
};
use crate::text::TextMasks;
use crate::vm::{GameVm, KEY_SENTINEL};
use crate::wrap::build_pages;
use formats::cml::{parse_cml, Cml};
use formats::lang::Lang;
use formats::vm::Step;
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
    /// class selected -> level load (mode 6 -> 15 -> gameplay)
    LoadLevel(String),
    /// a menu item that opens an unported mode (Help/About/…)
    Mode(u8),
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
}

pub struct Shell {
    assets: Assets,
    masks: TextMasks,
    assets_dir: PathBuf,
    vm: GameVm,
    lang: Lang,
    lang_loaded: bool, // a.a:Z (base-load idempotence)
    version: String,   // d:String (MIDlet-Version, spliced by h())

    mode: i8,                     // m:B (static mode)
    anim: i32,                    // m:I (startup anim id)
    bg: u32,                      // n:I (startup background)
    model: Option<Cml>,           // b:Ld (op43 model)
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
            assets_dir,
            vm: GameVm::new(),
            lang: Lang::default(),
            lang_loaded: false,
            version: "1.0.10".into(), // MIDlet-Version (jar manifest)
            mode: -1,                 // <clinit>: m = -1
            anim: 0,
            bg: 0,
            model: None,
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
        };
        shell.loader("/startup.scr")?;
        Ok(shell)
    }

    /// `b.a(String)` — the loader: mode 6, reset the per-level state, then
    /// `e.void_a(name)` parses the script and pushes entry 1. Resource-load
    /// progress (`b.c(int)`: mode-7 flips + `r:B`) is latency-driven on the
    /// real device and instant here; `r:B` stays -1 (empty bar), visual-only.
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
        let path = self.assets_dir.join(name.trim_start_matches('/'));
        let bytes = std::fs::read(&path)
            .map_err(|e| anyhow::anyhow!("loading script {}: {e}", path.display()))?;
        self.vm.load(&bytes)
    }

    /// `b.a(byte)` — the only general mode-transition function.
    fn set_mode(&mut self, new: i8) {
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

    /// Apply one executed script opcode's `b`-side effect (the op set the
    /// startup scripts use; anything else is a loud fence).
    fn apply(&mut self, step: &Step) {
        match step.opcode {
            2 | 11 => {} // return / wait: handled by GameVm
            60 => {}     // key gate: set by GameVm
            10 => {
                // b.b(anim, color): m:I, n:I, mode 21 for anim 4 else 8
                self.anim = step.operands[0];
                self.bg = step.operands[1] as u32;
                self.set_mode(if self.anim == 4 { 21 } else { 8 });
            }
            43 => {
                // b.d(String): b:Ld = g.a(name)
                let name = &step.strings[0];
                let path = self.assets_dir.join(name.trim_start_matches('/'));
                let bytes = std::fs::read(&path).expect("op43 model resource");
                self.model = Some(parse_cml(&bytes).expect("op43 cml parse"));
            }
            44 => self.e_menu(),
            56 => {
                // b.a(String, int): lang load; id 0/65535 = the base table,
                // idempotent once loaded (a.a:Z). The path string is vestigial
                // (class a reads /lang_<id>.txt).
                let id = step.operands[1];
                assert!(
                    id == 0 || id == 65535,
                    "lang overlay load not ported (out of slice): {id}"
                );
                if !self.lang_loaded {
                    let bytes =
                        std::fs::read(self.assets_dir.join("lang_0.txt")).expect("lang_0.txt");
                    self.lang = Lang::load_base(&bytes);
                    self.lang_loaded = true;
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

    /// One frame of `run()` (dt in ms): VM tick, input dispatch, blink,
    /// text-scroll timer — in the original loop order.
    pub fn tick(&mut self, dt_ms: i32) {
        if self.mode == 12 {
            return;
        }
        // f.a:B == 1 (in-game menu tick) never happens pre-gameplay; the
        // effects pool i.a(J) has no armed effects on this path.
        if !matches!(self.mode, 3 | 10 | 9 | 13) {
            if let Some(step) = self.vm.tick(dt_ms) {
                self.apply(&step);
            }
        }
        self.input(dt_ms); // this.b(J)
        if self.blink_ms >= 0 {
            self.blink_ms -= dt_ms;
            if self.blink_ms <= 0 {
                self.blink_ms = 500;
                self.blink_on = !self.blink_on;
            }
        }
        // a(J) floating-text timer: b:String is never set on the front end.
        // The mode-0 actor loop is the gameplay slice (todo-fenced).
        if matches!(self.mode, 9 | 10 | 4 | 21) {
            if self.scroll_acc > 100 {
                self.scroll -= 1;
                self.scroll_acc = 0;
            }
            self.scroll_acc = self.scroll_acc.wrapping_add(dt_ms as i16);
        }
        // paint-side blink arming (paint mode 8 runs every frame on the real
        // loop; our render only runs at shots, so arm here — same frame).
        if self.mode == 8 && self.vm.key_gate && self.blink_ms == -1 {
            self.blink_ms = 500;
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
        if !straight_to_tail {
            match self.mode {
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
        // TAIL: f.a == 0 pre-gameplay -> feed the VM; the key-redefine buffer
        // d(char) is out of slice. If released (p:B): consume the latch.
        self.vm.feed_key(key);
        if self.released {
            self.latched = KEY_SENTINEL;
            self.released = false;
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
            // k(); mode 6; null actors; a("/l01_1.scr") — the gameplay slice
            self.pending_leave = Some(Leave::LoadLevel(item));
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
            10 => anyhow::bail!("intro text page (mode 10) is the gameplay slice"),
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
                    other => anyhow::bail!(
                        "text-page end transition for mode {other} not ported \
                         (outro/intro — the gameplay slice)"
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
