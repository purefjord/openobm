//! `b.paint(Graphics)` — the covered modes. Transcribed from `b.javap.txt`
//! `paint` (the `tableswitch(m:B)` at offset 149; see `docs/loop-decode-notes.md`).
//! Only the modes on the boot→class-select path are implemented; the rest are
//! `todo!`-fenced so nothing silently renders wrong.
//!
//! Screen constants (from the ctor / `a(II)`): logical size is `a:S=240` wide,
//! `b:S=345` tall (25px taller than the 320 LCD, so the bottom band is clipped);
//! centers `c:S=120`, `d:S=172`. Fonts: `c:Font`=small-bold, `d:Font`=large-bold.

use crate::asset::{draw_image, Assets};
use crate::fb::Fb;
use crate::text::{GameFont, TextMasks};

pub const SCREEN_W: i32 = 240;
pub const SCREEN_H: i32 = 345; // LOGICAL height; the LCD only shows the top 320
pub const LCD_H: i32 = 320;

/// Which mode-3 page is showing (`b.k:B`). Recon: `-1` outside menus, `0` main
/// menu, `1` class select; `4`/`5` are in-game/pause variants (out of slice).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuPage {
    Main,        // k=0
    ClassSelect, // k=1
}

impl MenuPage {
    fn k(self) -> i32 {
        match self {
            MenuPage::Main => 0,
            MenuPage::ClassSelect => 1,
        }
    }
}

/// The word-wrap free draw the game does for a single menu item: centered on
/// `x = a:S/2 - stringWidth/2` (== `120 - w/2`) unless it is too wide AND
/// contains a space, in which case it splits at the first space onto two lines.
/// No on-path item triggers the split (all fit), but it's transcribed faithfully.
fn draw_centered_item(fb: &mut Fb, masks: &TextMasks, s: &str, y: i32, rgb: u32) {
    let avail = SCREEN_W - masks.string_width(GameFont::LargeBold, "<<  >>");
    let w = masks.string_width(GameFont::LargeBold, s);
    let split_at = if w < avail { None } else { s.find(' ') };
    match split_at {
        None => {
            let x = SCREEN_W / 2 - w / 2;
            masks.stamp(fb, GameFont::LargeBold, s, x, y, rgb);
        }
        Some(sp) => {
            let fh = masks.metrics(GameFont::LargeBold).midp_height;
            let (a, b) = s.split_at(sp);
            let b = &b[1..]; // drop the space
            let wa = masks.string_width(GameFont::LargeBold, a);
            let wb = masks.string_width(GameFont::LargeBold, b);
            masks.stamp(
                fb,
                GameFont::LargeBold,
                a,
                SCREEN_W / 2 - wa / 2,
                y - fh / 2,
                rgb,
            );
            masks.stamp(
                fb,
                GameFont::LargeBold,
                b,
                SCREEN_W / 2 - wb / 2,
                y + fh / 2,
                rgb,
            );
        }
    }
}

/// Paint the title screen (`b.paint` case 8). Background `bg` is the color set
/// by the boot script's `op10` (`b.b(anim,color)` -> `n:I`; the final title
/// frame uses cream `0xF5F2E2`). The logo is `b:Ld` frame `m:I` (startup.cml
/// record `m:I`, a static full-image) centered on `(c:S, d:S) = (120, 172)`.
/// "Press any key" (small-bold black) is drawn only on the blink-ON phase
/// (`i:Z`) once the script key-gate (`e.a:Z`) is set; centered, below the logo.
///
/// `blink_on` is the shell's current blink phase. The two phases are separate
/// static frames (both captured); the test matches whichever the deterministic
/// schedule lands on — no mask needed.
pub fn paint_title(
    fb: &mut Fb,
    masks: &TextMasks,
    assets: &Assets,
    bg: u32,
    blink_on: bool,
) -> anyhow::Result<()> {
    fb.fill(bg);
    // startup.cml record 5 = /5.png, a static full image (no sub-rects), drawn
    // centered on (120, 172). (The boot anim steps records 1..5; the settled
    // title is frame 5. Rendering intermediate frames is the animated-boot
    // sub-slice — visual-review only, not gated.)
    let logo = assets.image("/5.png")?;
    let (lw, lh) = (logo.w as i32, logo.h as i32);
    draw_image(fb, &logo, 120 - lw / 2, 172 - lh / 2);
    if blink_on {
        let s = "Press any key";
        let w = masks.string_width(GameFont::SmallBold, s);
        // y = d:S + logoH/2 + 12
        masks.stamp(
            fb,
            GameFont::SmallBold,
            s,
            120 - w / 2,
            172 + lh / 2 + 12,
            0x00_00_00,
        );
    }
    Ok(())
}

/// Paint a mode-3 menu page (`b.paint` case 3). `banner` is `/main.png`
/// (the flame art, `c:Image`), `items` the page's carousel entries
/// (`a:[[String[k]`), and `cursor` the current index (`e:[B[k]`).
pub fn paint_menu_page(
    fb: &mut Fb,
    masks: &TextMasks,
    assets: &Assets,
    page: MenuPage,
    items: &[&str],
    cursor: usize,
) -> anyhow::Result<()> {
    fb.fill(0x00_00_00);
    let banner = assets.image("/main.png")?;
    let img_x = SCREEN_W / 2 - banner.w as i32 / 2;
    draw_image(fb, &banner, img_x, 0);
    let img_h = banner.h as i32;

    // class-select header (green), just under the banner
    if page == MenuPage::ClassSelect {
        let hdr = "Select Your Class";
        let w = masks.string_width(GameFont::LargeBold, hdr);
        masks.stamp(
            fb,
            GameFont::LargeBold,
            hdr,
            SCREEN_W / 2 - w / 2,
            img_h + 1,
            0x0F_F0_00,
        );
    }

    // red carousel arrows at y = img_h + 25
    let arrow_y = img_h + 25;
    masks.stamp(fb, GameFont::LargeBold, "<<", 0, arrow_y, 0xFF_00_00);
    let ww = masks.string_width(GameFont::LargeBold, ">>");
    masks.stamp(
        fb,
        GameFont::LargeBold,
        ">>",
        SCREEN_W - ww,
        arrow_y,
        0xFF_00_00,
    );

    // white current item, centered
    draw_centered_item(fb, masks, items[cursor], arrow_y, 0xFF_FF_FF);

    // "BACK" (small-bold) at (2, b:S - smallH - 2) for pages other than 0/4/5.
    // On class select this lands at y=333 — below the 320 LCD, so it's clipped
    // (present in the logical frame, invisible on screen). Faithful to the game.
    if !matches!(page.k(), 0 | 4 | 5) {
        let small_h = masks.metrics(GameFont::SmallBold).midp_height;
        masks.stamp(
            fb,
            GameFont::SmallBold,
            "BACK",
            2,
            SCREEN_H - small_h - 2,
            0xFF_FF_FF,
        );
    }
    Ok(())
}
