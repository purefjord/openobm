//! The `b.java` front-end state machine, driven by input — the `var_byte_m`
//! mode + `k:B` menu page + per-page cursor, with the `b(J)` input dispatch and
//! `b.paint` rendering for the covered modes. Transcribed from `b.javap.txt`
//! (`b(J)` at 11679, `a(int)` key-remap at 11492, `a(byte)` set-mode at 11199,
//! `l()` menu-table build at 2374; see `docs/loop-decode-notes.md`).
//!
//! Scope: the menu subsystem (mode 3, pages 0 main / 1 class select) reached
//! from the title, driven by LEFT/RIGHT/FIRE. Everything that leaves this
//! subsystem is fenced with an explicit `Leave` outcome rather than guessed:
//! - the boot sequence (loader + startup.scr VM stepping the logo anims) and
//!   the title->menu edge (script `op44` -> `b.e()`) are **modeled directly**
//!   here pending script-VM integration (tracked in HANDOFF);
//! - firing a class, Help/About/Exit, etc. yields `Leave(reason)` — those modes
//!   (level load 6/15, text pages 4/9, exit dialog 19) land in later sub-slices.

use crate::asset::Assets;
use crate::fb::Fb;
use crate::paint::{paint_menu_page, paint_title, MenuPage, SCREEN_H, SCREEN_W};
use crate::text::TextMasks;

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

/// What a fire did when it leaves the menu subsystem (fenced, not yet ported).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Leave {
    /// class selected -> level load (mode 6 -> 15 -> gameplay)
    LoadLevel(String),
    /// a menu item that opens an unported mode (Help/About/Exit dialog/…)
    Mode(u8),
}

/// The current front-end screen (`b.m:B` for the modes we model).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Title,       // m=8
    MainMenu,    // m=3, k=0
    ClassSelect, // m=3, k=1
}

pub struct Shell {
    screen: Screen,
    blink_on: bool, // i:Z (title "press any key" phase)
    blink_ms: i32,  // l:I countdown (armed to 500 on the title)
    /// per-page cursor `e:[B` (only pages 0/1 used here).
    cursor: [usize; 2],
    /// page 0 main-menu items (`a:[[String[0]`), from `l()`. With an empty
    /// RecordStore there is no "Continue" (`b()Z == false`); recon baseline.
    main_items: Vec<String>,
    /// page 1 class list (`a:[[String[1]`), one per script class row; from recon
    /// (the class table is loaded from the scripts — hardcoded here pending the
    /// table-load integration, matching the real 8-class list).
    classes: Vec<String>,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

impl Shell {
    /// Start at the settled title (mode 8, key-gate set, blink armed). The boot
    /// path (loader -> startup.scr) that reaches here is script-VM-driven and is
    /// modeled by seeding this state directly (see module docs).
    pub fn new() -> Self {
        Self {
            screen: Screen::Title,
            blink_on: true,
            blink_ms: 500,
            cursor: [0, 0],
            main_items: ["New Game", "Help", "About", "Exit"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            classes: [
                "Monk",
                "Archer",
                "Knight",
                "Sorcerer",
                "Barbarian",
                "Nightblade",
                "Spellsword",
                "Battlemage",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn blink_on(&self) -> bool {
        self.blink_on
    }

    fn page_items(&self) -> &[String] {
        match self.screen {
            Screen::MainMenu => &self.main_items,
            Screen::ClassSelect => &self.classes,
            Screen::Title => &[],
        }
    }

    fn page_idx(&self) -> usize {
        match self.screen {
            Screen::MainMenu => 0,
            Screen::ClassSelect => 1,
            Screen::Title => 0,
        }
    }

    /// Advance the 500ms blink (`run()`'s `l:I`/`i:Z` toggle). Only the title
    /// arms it; menus don't blink.
    pub fn tick_blink(&mut self, dt_ms: i32) {
        if self.blink_ms >= 0 {
            self.blink_ms -= dt_ms;
            if self.blink_ms <= 0 {
                self.blink_ms = 500;
                self.blink_on = !self.blink_on;
            }
        }
    }

    /// Feed one key press (raw MIDP keycode). Returns `Some(Leave)` if it exits
    /// the menu subsystem into an unported mode.
    pub fn press(&mut self, key: i32) -> Option<Leave> {
        let action = Action::from_key(key)?;
        match self.screen {
            Screen::Title => {
                // any accepted key -> the script VM resumes (startup2.scr op44
                // = b.e(): rebuild menus, k=0, set_mode(3)). Modeled directly.
                if action == Action::Fire
                    || matches!(
                        action,
                        Action::Up | Action::Down | Action::Left | Action::Right
                    )
                {
                    self.screen = Screen::MainMenu;
                    self.cursor[0] = 0;
                }
                None
            }
            Screen::MainMenu | Screen::ClassSelect => self.menu_input(action),
        }
    }

    /// `b(J)` mode-3 dispatch: LEFT/RIGHT wrap the page cursor; FIRE dispatches
    /// on the selected item's string (faithful to the bytecode's string compares).
    fn menu_input(&mut self, action: Action) -> Option<Leave> {
        let page = self.page_idx();
        let len = self.page_items().len();
        match action {
            Action::Left => {
                // e[k]--; if < 0 wrap to len-1
                self.cursor[page] = if self.cursor[page] == 0 {
                    len - 1
                } else {
                    self.cursor[page] - 1
                };
                None
            }
            Action::Right => {
                // e[k]++; if == len wrap to 0
                self.cursor[page] = (self.cursor[page] + 1) % len;
                None
            }
            Action::Fire => self.fire(),
            Action::Up | Action::Down => None, // menus ignore up/down (recon)
        }
    }

    fn fire(&mut self) -> Option<Leave> {
        match self.screen {
            Screen::MainMenu => {
                match self.main_items[self.cursor[0]].as_str() {
                    // New Game (no save) -> class select page (k=1)
                    "New Game" => {
                        self.screen = Screen::ClassSelect;
                        None
                    }
                    "Exit" => Some(Leave::Mode(19)), // exit-confirm dialog
                    "Help" => Some(Leave::Mode(9)),  // help text page
                    "About" => Some(Leave::Mode(4)), // about scroll
                    other => panic!("unported main-menu item: {other:?}"),
                }
            }
            Screen::ClassSelect => {
                // fire a class -> level load (mode 6 -> please-wait 15 -> game)
                Some(Leave::LoadLevel(self.classes[self.cursor[1]].clone()))
            }
            Screen::Title => None,
        }
    }

    /// Render the current screen to a fresh logical framebuffer (240x345).
    pub fn render(&self, masks: &TextMasks, assets: &Assets) -> anyhow::Result<Fb> {
        let mut fb = Fb::new(SCREEN_W, SCREEN_H);
        match self.screen {
            Screen::Title => paint_title(&mut fb, masks, assets, 0xF5_F2_E2, self.blink_on)?,
            Screen::MainMenu => {
                let items: Vec<&str> = self.main_items.iter().map(String::as_str).collect();
                paint_menu_page(
                    &mut fb,
                    masks,
                    assets,
                    MenuPage::Main,
                    &items,
                    self.cursor[0],
                )?;
            }
            Screen::ClassSelect => {
                let items: Vec<&str> = self.classes.iter().map(String::as_str).collect();
                paint_menu_page(
                    &mut fb,
                    masks,
                    assets,
                    MenuPage::ClassSelect,
                    &items,
                    self.cursor[1],
                )?;
            }
        }
        Ok(fb)
    }
}
