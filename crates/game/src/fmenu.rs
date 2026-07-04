//! The `f.java` in-game menu system (the mode-2 action menu; also the shop's
//! Buy/Sell pages, out of slice) + `c.java` (the menu-item tree node).
//! Transcribed from the CFR decompile of `f.java`/`c.java` with javap
//! cross-checks (the bl/bl2 arrow flags, the marquee arming). `b.paint`'s
//! mode-2 case draws NOTHING — `f.a(Graphics)` paints in the paint TAIL
//! (offset 5590-5609) whenever `f.a:B == 1`.
//!
//! The item graph (`c`) is stored as an arena ([`FMenu::items`]) because the
//! original mutates shared nodes across pages (the `b.var_c_a`/`var_c_b`
//! active-weapon/spell re-marking in `b.a(c)`).
//!
//! Fonts (b statics): `a:Font` = SMALL PLAIN (the item/title font — its
//! height is the row pitch `f.var_byte_b` = 10), `c:Font` = small bold
//! (active items + stat labels), `d:Font` = large bold (the `var_boolean_a`
//! "Resume game?" overlay — armed by the pause slice, default off).

use crate::asset::Assets;
use crate::fb::Fb;
use crate::gpaint::draw_model_frame;
use crate::paint::{SCREEN_H, SCREEN_W};
use crate::text::{GameFont, TextMasks};
use formats::anim::Anim;
use formats::Cml;

/// `c.java` — one menu node.
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub name: String,          // var_java_lang_String_a
    pub desc: Option<String>,  // var_java_lang_String_b (the selected-item box)
    pub active: bool,          // var_boolean_a (checkmark + bold)
    pub enabled: bool,         // var_boolean_b (false = red, fire ignored)
    pub children: Vec<usize>,  // var_java_util_Vector_a
    pub parent: Option<usize>, // var_c_a
    /// `var_java_lang_String_arr_a` — the two-column stat rows (label, value
    /// pairs; a `None` label/value cell is skipped).
    pub stat_rows: Option<Vec<Option<String>>>,
    /// The `f.a(c, c)` potion grouping, precomputed at build time from the
    /// item name (lang 149/151 = healing group, 150/152 = magicka group):
    /// the Items-page radio-clear only clears within the fired item's group.
    pub potion_group: u8,
}

impl MenuItem {
    pub fn new(name: impl Into<String>, desc: Option<String>, active: bool) -> Self {
        Self {
            name: name.into(),
            desc,
            active,
            enabled: true, // var_boolean_b = true (field initializer)
            children: Vec::new(),
            parent: None,
            stat_rows: None,
            potion_group: 0,
        }
    }
}

/// Colors from `f.a(Graphics)`.
const BAR: u32 = 0xFA_FD_CE; // 16448974 — the selection bar
const SELECTED_ENABLED: u32 = 0x9D_73_39; // 10318649 — selected, enabled
const RED: u32 = 0xFF_00_00;

pub struct FMenu {
    /// The item arena (the `c` graph).
    pub items: Vec<MenuItem>,
    /// `var_c_arr_a` — the CURRENT node shown per tab (descending replaces
    /// the slot; `a()` pops to its parent).
    pub pages: Vec<usize>,
    /// `var_byte_arr_a` — the tab-bar anim keys (`[0]` = the bar frame,
    /// `[1..]` = per-tab icons; tab count = len - 1... the original wraps
    /// `f` at `length - 1`).
    tabs: Vec<i32>,
    /// `var_byte_a == 1`.
    pub open: bool,
    /// `var_java_lang_String_a` — the status line (the shop's gold text).
    pub status: Option<String>,
    cursor: i32,        // c:B
    last_drawn: i32,    // d:B (exclusive; the input's scroll-window math)
    first_visible: i32, // e:B
    page: usize,        // f:B
    marq_off: i32,      // g:B (the marquee substring offset)
    marq_dir: i32,      // h:B (1 / -1)
    marq_on: bool,      // i:B
    marq_pause: bool,   // j:B (the 1000ms end pause)
    scroll: i32,        // var_short_a
    marq_acc: i32,      // var_short_b
    /// The pre-rendered background (`b(Graphics)` into the passed Image —
    /// the original bakes it into b's offscreen; we keep our own copy and
    /// the caller dirties the base-map cache instead).
    bg: Option<Fb>,
    /// `f.var_boolean_a` (static) — the "Resume game?" overlay; armed by the
    /// pause slice (b.java:2211 clears it), stays off here.
    pub resume_overlay: bool,
    /// `f.var_byte_b` (static) — the row pitch = `a:Font.getHeight()`.
    rowh: i32,
    /// The f model (`var_d_a = g.a("/oh_menu.cml")` — its OWN `d` instance,
    /// per the no-cache correction).
    cml: Cml,
    anim: Anim,
}

impl FMenu {
    /// The `f(String, b)` ctor — loads the menu model.
    pub fn new(cml: Cml) -> Self {
        let anim = Anim::from_cml(&cml);
        Self {
            items: Vec::new(),
            pages: Vec::new(),
            tabs: Vec::new(),
            open: false,
            status: None,
            cursor: 0,
            last_drawn: 0,
            first_visible: 0,
            page: 0,
            marq_off: 0,
            marq_dir: 1,
            marq_on: false,
            marq_pause: false,
            scroll: 0,
            marq_acc: 0,
            bg: None,
            resume_overlay: false,
            rowh: 10,
            cml,
            anim,
        }
    }

    /// `g.a(d, key)` / `g.b(d, key)` on the f model.
    fn fsize(&self, key: i32) -> (i32, i32) {
        crate::world::frame_size_of(&self.cml, &self.anim, key)
    }

    /// `f.a(byte[], c[], String, Image, Graphics)` — open the menu: install
    /// the tab keys + page nodes + status, reset the cursor/scroll/marquee,
    /// and pre-render the background.
    pub fn open(
        &mut self,
        tabs: Vec<i32>,
        items: Vec<MenuItem>,
        pages: Vec<usize>,
        status: Option<String>,
        masks: &TextMasks,
        assets: &Assets,
    ) {
        self.items = items;
        self.pages = pages;
        self.tabs = tabs;
        self.open = true;
        self.page = 0;
        self.status = status;
        self.cursor = 0;
        self.last_drawn = 0;
        self.first_visible = 0;
        self.scroll = 0;
        self.marq_off = 0;
        self.marq_on = false;
        self.rowh = masks.metrics(GameFont::SmallPlain).midp_height;
        self.bg = Some(self.build_bg(masks, assets));
    }

    /// `f.b(Graphics)` — the tiled parchment background: black fill, the
    /// group-13 tile over the body, group-11/12 left/right edge columns,
    /// group-8 top row, group-5 bottom row above the soft-key band, corners
    /// 9/10 (top) and 6/7 (bottom).
    fn build_bg(&mut self, masks: &TextMasks, assets: &Assets) -> Fb {
        let mut fb = Fb::new(SCREEN_W, SCREEN_H);
        fb.fill(0x00_00_00);
        let fa_h = masks.metrics(GameFont::SmallPlain).midp_height;
        let (w13, h13) = self.fsize(13);
        let h11 = self.fsize(11).1;
        let (w12, h12) = self.fsize(12);
        let w8 = self.fsize(8).0;
        let (w5, h5) = self.fsize(5);
        let band = SCREEN_H - fa_h - 4;
        let mut x = 0;
        while x < SCREEN_W {
            let mut y = 0;
            while y < band - h13 {
                draw_model_frame(&mut fb, assets, &self.cml, &self.anim, 13, x, y);
                y += h13;
            }
            x += w13;
        }
        let mut y = 0;
        while y < band - h11 {
            draw_model_frame(&mut fb, assets, &self.cml, &self.anim, 11, 0, y);
            y += h11;
        }
        let mut y = 0;
        while y < band - h12 {
            draw_model_frame(
                &mut fb,
                assets,
                &self.cml,
                &self.anim,
                12,
                SCREEN_W - w12,
                y,
            );
            y += h12;
        }
        let mut x = 0;
        while x < SCREEN_W {
            draw_model_frame(&mut fb, assets, &self.cml, &self.anim, 8, x, 0);
            x += w8;
        }
        let mut x = 0;
        while x < SCREEN_W {
            draw_model_frame(&mut fb, assets, &self.cml, &self.anim, 5, x, band - h5);
            x += w5;
        }
        draw_model_frame(&mut fb, assets, &self.cml, &self.anim, 9, 0, 0);
        let w10 = self.fsize(10).0;
        draw_model_frame(
            &mut fb,
            assets,
            &self.cml,
            &self.anim,
            10,
            SCREEN_W - w10,
            0,
        );
        let h6 = self.fsize(6).1;
        draw_model_frame(&mut fb, assets, &self.cml, &self.anim, 6, 0, band - h6);
        let (w7, h7) = self.fsize(7);
        draw_model_frame(
            &mut fb,
            assets,
            &self.cml,
            &self.anim,
            7,
            SCREEN_W - w7,
            band - h7,
        );
        fb
    }

    /// `f.a()Z` — pop the current tab's node to its parent. Returns false at
    /// a top page (the caller closes the menu).
    pub fn back(&mut self) -> bool {
        let Some(&node) = self.pages.get(self.page) else {
            return false;
        };
        if let Some(parent) = self.items[node].parent {
            self.pages[self.page] = parent;
            self.cursor = 0;
            self.last_drawn = 0;
            self.first_visible = 0;
            self.scroll = 0;
            true
        } else {
            false
        }
    }

    /// `f.a(int)` — arm/disarm the marquee (state reset on change).
    fn set_marq(&mut self, on: bool) {
        if on == self.marq_on {
            return;
        }
        self.marq_acc = 0;
        self.marq_off = 0;
        self.marq_dir = 1;
        self.marq_pause = false;
        self.marq_on = on;
    }

    /// `f.a(long)` — the marquee tick: after the 1000ms end pause, step the
    /// substring offset every 500ms in the current direction.
    pub fn tick(&mut self, dt: i32) {
        if self.marq_on {
            self.marq_acc += dt;
            if self.marq_pause {
                if self.marq_acc >= 1000 {
                    self.marq_acc = 0;
                    self.marq_pause = false;
                }
            } else if self.marq_acc >= 500 {
                self.marq_off += self.marq_dir;
                self.marq_acc = 0;
            }
        }
    }

    /// `f.a(c, c)` — may firing `fired` clear `other`'s active flag on the
    /// Items page? Only within the same potion group (healing 149/151,
    /// magicka 150/152).
    fn same_potion_group(&self, other: usize, fired: usize) -> bool {
        let (a, b) = (
            self.items[other].potion_group,
            self.items[fired].potion_group,
        );
        a != 0 && a == b
    }

    /// `f.a(char)` — the menu input (the codes are the mode-0 remap: 3 up,
    /// 4 down, 5 left, 6 right, 7 fire). Returns the fired node for the
    /// caller's `b.a(c)` activation callback.
    pub fn input(&mut self, code: i32, items_page_name: &str) -> Option<usize> {
        let rowh = self.rowh; // var_byte_b = a:Font height
        let page_node = self.pages[self.page];
        let mut fired = None;
        match code {
            4 => {
                // DOWN: cursor wraps; a stat-row page scrolls by rows with a
                // bottom clamp.
                self.cursor += 1;
                if self.cursor >= self.items[page_node].children.len() as i32 {
                    self.cursor = 0;
                }
                if let Some(rows) = &self.items[page_node].stat_rows {
                    self.scroll -= rowh;
                    let n = (rows.len() as i32 >> 1) * rowh;
                    let h5 = self.fsize(5).1;
                    if n + self.scroll + (rowh << 2) < SCREEN_H - 10 - 4 - h5 {
                        self.scroll += rowh;
                    }
                }
            }
            3 => {
                // UP
                self.cursor -= 1;
                if self.cursor < 0 {
                    self.cursor = self.items[page_node].children.len() as i32 - 1;
                }
                if self.items[page_node].stat_rows.is_some() {
                    self.scroll += rowh;
                    if self.scroll > 0 {
                        self.scroll = 0;
                    }
                }
            }
            5 => {
                // LEFT: pop, then previous tab.
                self.back();
                if self.page == 0 {
                    self.page = self.tabs.len() - 2;
                } else {
                    self.page -= 1;
                }
                self.cursor = 0;
                self.last_drawn = 0;
                self.first_visible = 0;
                self.scroll = 0;
            }
            6 => {
                // RIGHT: pop, then next tab.
                self.back();
                self.page += 1;
                if self.page == self.tabs.len() - 1 {
                    self.page = 0;
                }
                self.cursor = 0;
                self.last_drawn = 0;
                self.first_visible = 0;
                self.scroll = 0;
            }
            7 => {
                let kids = &self.items[page_node].children;
                if self.cursor >= 0 && (self.cursor as usize) < kids.len() {
                    let node = kids[self.cursor as usize];
                    // Disabled items ignored (except on the shop's Buy/Sell
                    // pages — out of slice; the caller fences those).
                    if !self.items[node].enabled {
                        return None;
                    }
                    if self.items[node].children.is_empty() {
                        // Leaf: the radio-clear pass (Items page: only within
                        // the fired potion group), then mark active.
                        let siblings = self.items[page_node].children.clone();
                        let items_page = self.items[page_node].name == items_page_name;
                        for sib in siblings {
                            if items_page && !self.same_potion_group(sib, node) {
                                continue;
                            }
                            self.items[sib].active = false;
                        }
                        self.items[node].active = true;
                    } else {
                        // Descend: the node replaces the tab's page.
                        self.pages[self.page] = node;
                        self.cursor = 0;
                    }
                    fired = Some(node); // var_b_a.a(c3) — for leaves AND descents
                }
            }
            _ => {}
        }
        // The scroll-window follow (runs for every code).
        if self.cursor > self.last_drawn {
            self.scroll = -rowh * (self.cursor - (self.last_drawn - self.first_visible));
        } else if self.cursor < self.first_visible {
            self.scroll = -rowh * self.cursor;
        }
        self.set_marq(false); // a(0)
        fired
    }

    /// MIDP `drawRect` (inclusive outline).
    fn draw_rect(fb: &mut Fb, x: i32, y: i32, w: i32, h: i32, rgb: u32) {
        for i in 0..=w {
            fb.set(x + i, y, rgb);
            fb.set(x + i, y + h, rgb);
        }
        for j in 0..=h {
            fb.set(x, y + j, rgb);
            fb.set(x + w, y + j, rgb);
        }
    }

    /// `f.a(Graphics)` — the menu paint. `&mut self` because the paint owns
    /// the marquee arming (`bl3` -> `a(1)`) and the `d`/`e` window fields.
    pub fn paint(&mut self, fb: &mut Fb, masks: &TextMasks, assets: &Assets) {
        let rowh = self.rowh; // var_byte_b
        let fa_h = masks.metrics(GameFont::SmallPlain).midp_height;
        let page_node = self.pages[self.page];
        let mut up_arrow = false; // bl
        let mut down_arrow = false; // bl2
        if let Some(bg) = &self.bg {
            for y in 0..SCREEN_H {
                for x in 0..SCREEN_W {
                    fb.set(x, y, bg.get(x, y));
                }
            }
        }
        // The tab bar + the selected tab's icon, bottom-centered above the
        // soft-key band.
        if !self.tabs.is_empty() {
            for &key in &[self.tabs[0], self.tabs[self.page + 1]] {
                let (w, h) = self.fsize(key);
                draw_model_frame(
                    fb,
                    assets,
                    &self.cml,
                    &self.anim,
                    key,
                    (SCREEN_W >> 1) - (w >> 1),
                    SCREEN_H - fa_h - 4 - h,
                );
            }
        }
        // Title (a:Font black, centered at y=12).
        let title = self.items[page_node].name.clone();
        let tw = masks.string_width(GameFont::SmallPlain, &title);
        masks.stamp(
            fb,
            GameFont::SmallPlain,
            &title,
            (SCREEN_W >> 1) - (tw >> 1),
            12,
            0x00_00_00,
        );
        // Status line (the shop's gold text).
        if let Some(s) = self.status.clone() {
            let w = masks.string_width(GameFont::SmallPlain, &s);
            let h5 = self.fsize(5).1;
            masks.stamp(
                fb,
                GameFont::SmallPlain,
                &s,
                (SCREEN_W >> 1) - (w >> 1),
                SCREEN_H - h5 - (rowh << 1),
                0x00_00_00,
            );
        }
        let h5 = self.fsize(5).1;
        let top = 12 + (rowh << 1);
        let bottom = SCREEN_H - fa_h - 4 - h5;
        let mut y = self.scroll + top;
        self.first_visible = -1;
        let kids = self.items[page_node].children.clone();
        let mut n2 = 0i32;
        for (i, &node) in kids.iter().enumerate() {
            n2 = i as i32;
            if y >= top {
                if self.first_visible == -1 {
                    self.first_visible = i as i32;
                }
                let selected = i as i32 == self.cursor;
                if selected {
                    fb.fill_rect(15, y, SCREEN_W - 30, rowh, BAR);
                    if let Some(desc) = self.items[node].desc.clone() {
                        // The selected item's description box, above the
                        // status band (a:Font, black).
                        let dy = SCREEN_H - fa_h - 4 - h5 - (rowh << 1);
                        Self::draw_rect(fb, 20, dy - 6, SCREEN_W - 40, rowh + 4, 0x00_00_00);
                        masks.stamp(fb, GameFont::SmallPlain, &desc, 23, dy - 3, 0x00_00_00);
                    }
                }
                let color = match (selected, self.items[node].enabled) {
                    (true, true) => SELECTED_ENABLED,
                    (false, true) => 0,
                    (_, false) => RED,
                };
                let (font, x_off) = if self.items[node].active {
                    draw_model_frame(fb, assets, &self.cml, &self.anim, 14, 15, y);
                    (GameFont::SmallBold, 15)
                } else {
                    (GameFont::SmallPlain, 0)
                };
                if self.items[node].children.is_empty() {
                    // Leaf: the marquee window + "..." truncation. The width
                    // math is Font.stringWidth == the charw sum (additivity
                    // verified by the capture).
                    let full = self.items[node].name.clone();
                    let mut s = full.clone();
                    if selected {
                        let off = self.marq_off.max(0) as usize;
                        s = s.chars().skip(off).collect();
                    }
                    let w12 = self.fsize(12).0;
                    let mut truncated = false;
                    // The original keeps a DOTLESS base that shrinks one char
                    // per lap; the drawn string is base + "...".
                    let mut base = s.clone();
                    while w12 + x_off + 15 > SCREEN_W - masks.substring_width(font, &s) {
                        truncated = true;
                        base.pop();
                        s = format!("{base}...");
                    }
                    if selected {
                        if truncated {
                            if self.marq_dir == -1 && self.marq_off == 0 {
                                self.marq_dir = 1;
                                self.marq_pause = true;
                            }
                            self.set_marq(true);
                        } else if self.marq_on && self.marq_dir == 1 {
                            self.marq_dir = -1;
                            self.marq_pause = true;
                        }
                    }
                    masks.stamp(fb, font, &s, 15 + x_off, y, color);
                } else {
                    let s = format!("<{}>", self.items[node].name);
                    masks.stamp(fb, font, &s, 15 + x_off, y, color);
                }
            } else {
                // Above the visible top: BOTH arrows (faithful — javap 859).
                up_arrow = true;
                down_arrow = true;
            }
            y += rowh;
            if y + (rowh << 1) >= bottom - (rowh << 1) {
                up_arrow = true;
                down_arrow = true;
                break;
            }
            n2 = i as i32 + 1;
        }
        // The two-column stat rows (label small-bold black at x=10; value
        // a:Font RED at 15 + labelWidth).
        if let Some(rows) = self.items[page_node].stat_rows.clone() {
            let mut y = self.scroll + rowh * 3;
            let mut n4 = 0usize;
            while n4 < rows.len() {
                if y >= top {
                    if let Some(label) = &rows[n4] {
                        masks.stamp(fb, GameFont::SmallBold, label, 10, y, 0x00_00_00);
                    }
                    if let Some(value) = &rows[n4 + 1] {
                        let lw = rows[n4]
                            .as_ref()
                            .map_or(0, |l| masks.substring_width(GameFont::SmallBold, l));
                        masks.stamp(fb, GameFont::SmallPlain, value, 15 + lw, y, RED);
                    }
                } else {
                    up_arrow = true;
                }
                y += rowh;
                if y + rowh >= SCREEN_H - fa_h - 4 - h5 {
                    if n4 >= rows.len() - 2 {
                        break;
                    }
                    down_arrow = true;
                    break;
                }
                n4 += 2;
            }
        }
        self.last_drawn = n2;
        // "BACK" (a:Font, red) in the clipped soft-key band.
        let back = String::from("BACK"); // lang 449 .toUpperCase()
        masks.stamp(fb, GameFont::SmallPlain, &back, 2, SCREEN_H - fa_h - 2, RED);
        if up_arrow {
            let w54 = self.fsize(54).0;
            draw_model_frame(
                fb,
                assets,
                &self.cml,
                &self.anim,
                54,
                SCREEN_W - w54 - 10,
                35,
            );
        }
        if down_arrow {
            let (w53, h53) = self.fsize(53);
            draw_model_frame(
                fb,
                assets,
                &self.cml,
                &self.anim,
                53,
                SCREEN_W - w53 - 10,
                SCREEN_H - fa_h - h53 - h5 - 6,
            );
        }
        // The f.var_boolean_a "Resume game?" overlay (pause slice arms it).
        if self.resume_overlay {
            fb.fill(0x00_00_00);
            let fd_h = masks.metrics(GameFont::LargeBold).midp_height;
            let prompt = "Resume game?"; // lang 571
            let w = masks.string_width(GameFont::LargeBold, prompt);
            masks.stamp(
                fb,
                GameFont::LargeBold,
                prompt,
                (SCREEN_W >> 1) - (w >> 1),
                (SCREEN_H >> 1) - (fd_h >> 1),
                0xFF_FF_FF,
            );
            masks.stamp(
                fb,
                GameFont::LargeBold,
                "EXIT", // lang 22 .toUpperCase()
                2,
                SCREEN_H - fd_h - 2,
                0xFF_FF_FF,
            );
            let w_yes = masks.string_width(GameFont::LargeBold, "Yes"); // pre-uppercase
            masks.stamp(
                fb,
                GameFont::LargeBold,
                "YES",
                SCREEN_W - w_yes - 2,
                SCREEN_H - fd_h - 2,
                0xFF_FF_FF,
            );
        }
    }
}
