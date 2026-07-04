//! `b.paint(Graphics)` — the covered modes. Transcribed from `b.javap.txt`
//! `paint` (the `tableswitch(m:B)` at offset 149; see `docs/loop-decode-notes.md`).
//! Only the modes on the boot→class-select path are implemented; the rest are
//! `todo!`-fenced so nothing silently renders wrong.
//!
//! Screen constants (from the ctor / `a(II)`): logical size is `a:S=240` wide,
//! `b:S=345` tall (25px taller than the 320 LCD, so the bottom band is clipped);
//! centers `c:S=120`, `d:S=172`. Fonts: `a:Font`=default medium plain,
//! `c:Font`=small-bold, `d:Font`=large-bold.

use crate::asset::{draw_image, Assets};
use crate::fb::Fb;
use crate::text::{GameFont, TextMasks};
use crate::world::Model;

pub const SCREEN_W: i32 = 240;
pub const SCREEN_H: i32 = 345; // LOGICAL height; the LCD only shows the top 320
pub const LCD_H: i32 = 320;

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

/// Paint a startup page (`b.paint` case 8): fill `n:I`, then draw the current
/// model anim (`g.a(G, b:Ld, m:I, ...)`) centered on `(c:S, d:S) = (120, 172)`.
/// During boot `b:Ld` is `/startup.cml` whose records 1..5 are static
/// full-image frames (`/1.png`.. logos, `/5.png` title), so the anim draw is a
/// centered image blit. "Press any key" (`new a().a(1)`, small-bold black) is
/// drawn only once the script key-gate (`e.a:Z`, op60) is set AND the blink
/// phase (`i:Z`) is on; y sits below the frame: `d:S + frameH/2 + 12`.
pub fn paint_startup(
    fb: &mut Fb,
    masks: &TextMasks,
    assets: &Assets,
    anim_png: &str,
    bg: u32,
    press_any_key: bool,
) -> anyhow::Result<()> {
    fb.fill(bg);
    let logo = assets.image(anim_png)?;
    let (lw, lh) = (logo.w as i32, logo.h as i32);
    draw_image(fb, &logo, 120 - lw / 2, 172 - lh / 2);
    if press_any_key {
        let s = "Press any key";
        let w = masks.string_width(GameFont::SmallBold, s);
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

/// Paint a mode-3 menu page (`b.paint` case 3). `k` is the page (`b.k:B`),
/// `items` the page's carousel entries (`a:[[String[k]`), `cursor` the current
/// index (`e:[B[k]`), and `banner` whether `c:Image` (`/main.png`) is loaded
/// (set-mode loads it entering mode 3, nulls it entering other modes).
pub fn paint_menu_page(
    fb: &mut Fb,
    masks: &TextMasks,
    assets: &Assets,
    k: i8,
    items: &[String],
    cursor: usize,
    banner: bool,
) -> anyhow::Result<()> {
    fb.fill(0x00_00_00);
    let img_h = if banner {
        let banner = assets.image("/main.png")?;
        let img_x = SCREEN_W / 2 - banner.w as i32 / 2;
        draw_image(fb, &banner, img_x, 0);
        banner.h as i32
    } else {
        0
    };

    // class-select header (green), just under the banner
    if k == 1 {
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
    draw_centered_item(fb, masks, &items[cursor], arrow_y, 0xFF_FF_FF);

    // "BACK" (small-bold) at (2, b:S - smallH - 2) for pages other than 0/4/5.
    // On class select this lands at y=333 — below the 320 LCD, so it's clipped
    // (present in the logical frame, invisible on screen). Faithful to the game.
    if !matches!(k, 0 | 4 | 5) {
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

/// Paint the exit-confirm dialog (`b.paint` case 19, offset 5045): black fill,
/// then three white LARGE-BOLD strings, with two faithful quirks from the
/// bytecode (see `docs/loop-decode-notes.md` "Exit dialog"):
/// - the prompt (lang 475) x-centers by the width of **lang 451**
///   ("Load Saved Game?") — a copy-paste bug in the original, kept;
/// - the "YES" label's x measures the PRE-uppercase "Yes" (lang 426), then
///   draws the uppercased string.
///
/// The NO/YES soft-key row lands at `y = 345 - fontH - 2 = 329` — entirely
/// inside the clipped logical band (the LCD shows rows < 320), so the visible
/// dialog is just the centered prompt. Painted faithfully regardless.
pub fn paint_exit_dialog(fb: &mut Fb, masks: &TextMasks) {
    fb.fill(0x00_00_00);
    let fh = masks.metrics(GameFont::LargeBold).midp_height;
    let w451 = masks.string_width(GameFont::LargeBold, "Load Saved Game?");
    masks.stamp(
        fb,
        GameFont::LargeBold,
        "Exit...Are you sure?", // lang 475
        SCREEN_W / 2 - w451 / 2,
        SCREEN_H / 2 - fh / 2,
        0xFF_FF_FF,
    );
    let soft_y = SCREEN_H - fh - 2;
    // lang 427 "No".toUpperCase() at x=2; lang 426 "Yes".toUpperCase() right-
    // aligned by the pre-uppercase width
    masks.stamp(fb, GameFont::LargeBold, "NO", 2, soft_y, 0xFF_FF_FF);
    let w_yes = masks.string_width(GameFont::LargeBold, "Yes");
    masks.stamp(
        fb,
        GameFont::LargeBold,
        "YES",
        SCREEN_W - w_yes - 2,
        soft_y,
        0xFF_FF_FF,
    );
}

/// Paint the player-death screen (`b.paint` case 11, offset 3745): black
/// fill, then three white LARGE-BOLD strings — the lang-428 "Continue?"
/// prompt centered by its OWN width (unlike the exit dialog's lang-451
/// quirk), and the same NO / YES soft-key row as the exit dialog (the YES x
/// measures the pre-uppercase "Yes"), wholly inside the clipped 320..345
/// band. The visible screen is just the centered prompt.
pub fn paint_death(fb: &mut Fb, masks: &TextMasks) {
    fb.fill(0x00_00_00);
    let fh = masks.metrics(GameFont::LargeBold).midp_height;
    let prompt = "Continue?"; // lang 428
    let w = masks.string_width(GameFont::LargeBold, prompt);
    masks.stamp(
        fb,
        GameFont::LargeBold,
        prompt,
        SCREEN_W / 2 - w / 2,
        SCREEN_H / 2 - fh / 2,
        0xFF_FF_FF,
    );
    let soft_y = SCREEN_H - fh - 2;
    masks.stamp(fb, GameFont::LargeBold, "NO", 2, soft_y, 0xFF_FF_FF);
    let w_yes = masks.string_width(GameFont::LargeBold, "Yes");
    masks.stamp(
        fb,
        GameFont::LargeBold,
        "YES",
        SCREEN_W - w_yes - 2,
        soft_y,
        0xFF_FF_FF,
    );
}

/// Paint a text page (`b.paint` cases 4/9/10/17/21/23, offset 2401):
/// word-wrapped paragraph lines from `h(String)` at x=2, pitched
/// `c:Font.height + 1` (11px), starting at `y = 3 + g:S`. Inverted pages
/// ({4,21,23,17}) are white-on-black; others black-on-parchment `0xE9E1C3` (a bytecode correction; see below).
/// Mode 21 draws with the DEFAULT medium font (all others small-bold), skips
/// the pitch-add on the first line, and — faithful quirk — **decrements the
/// scroll inside paint** (1px per repaint, on top of `run()`'s 1px/100ms
/// timer), which is why the legal scroll speed tracks the oracle's frame
/// rate. {23,17} clamp the scroll to ≤ 20 per paint (a held UP overscroll
/// settles at exactly 20 — oracle-pinned). Line prefixes `1~` (gold, switch
/// to small-bold for the page's remainder) and `3~` (red, centered) are the
/// credits markup.
///
/// Tails (offsets 2851–3502): {23,17} draw a black top band + white title
/// box with `lang(l:S)` (the help topic) in dark red 0xDD0000, small-bold,
/// centered at y=10 — `title` carries that string. {4,23,17} draw a black
/// bottom bar + white small-bold "BACK" at (2, 333) — entirely inside the
/// clipped 320..345 band, painted faithfully anyway. The scroll ARROWS
/// (3186–3502) draw `ui` (= `b:Ld`, the op43 model) anim keys 54 (up) / 53
/// (down); mode 10 first paints a parchment bottom bar — ALL of it lands in
/// the clipped band (`b:S` = LCD + 25; arrow y ≥ 328, bar y = 327), painted
/// faithfully anyway. A `ui` model without groups 53/54 draws nothing (the
/// original `g.a` catches the NPE and returns 0 — e.g. the legal page's
/// `/startup.cml`).
///
/// Returns the final line y (`var5`) — the caller's end-of-text check
/// (`b:Z` debounce → `p:Z` latch / mode transition) needs it.
#[allow(clippy::too_many_arguments)]
pub fn paint_text_page(
    fb: &mut Fb,
    masks: &TextMasks,
    assets: &Assets,
    ui: Option<&Model>,
    mode: i8,
    pages: &[Vec<String>],
    scroll: &mut i16,
    title: Option<&str>,
) -> i32 {
    let inverted = matches!(mode, 4 | 21 | 23 | 17);
    // BYTECODE CORRECTION (paint 2444, constant #513 = 15327683): the
    // parchment fill is 0xE9E1C3 — the recon prose (and this port until the
    // fixed-scroll m10 gate) had 0xE9E9C3, a transposition typo no earlier
    // gate could see: the non-inverted pages (9/10/21) were never pixel-gated.
    fb.fill(if inverted { 0x00_00_00 } else { 0xE9_E1_C3 });
    // BYTECODE CORRECTION (b.java:1012 + <clinit> 3266): mode 21 switches to
    // `a:Font` = getFont(0,0,8) = SMALL PLAIN — the old recon prose said
    // "default medium" (the legal scroll is animated/never gated, so the
    // wrong draw font was invisible until the font table was pinned).
    let mut font = if mode == 21 {
        GameFont::SmallPlain
    } else {
        GameFont::SmallBold
    };
    let mut skip_first_pitch = mode == 21;
    if matches!(mode, 23 | 17) && *scroll > 20 {
        *scroll = 20;
    }
    if mode == 21 {
        *scroll -= 1;
    }
    let small_h = masks.metrics(GameFont::SmallBold).midp_height;
    let pitch = small_h + 1;
    let mut y = 3 + i32::from(*scroll);
    for para in pages {
        for line in para {
            let mut color = if inverted { 0xFF_FF_FF } else { 0x00_00_00 };
            let mut x = 2;
            if skip_first_pitch {
                skip_first_pitch = false;
            } else {
                y += pitch;
            }
            let mut s = line.as_str();
            if s.len() > 1 && s.as_bytes()[1] == b'~' {
                let rest = &s[s.find('~').unwrap() + 1..];
                match s.as_bytes()[0] {
                    b'1' => {
                        font = GameFont::SmallBold;
                        color = 0xAA_AA_00;
                    }
                    b'3' => {
                        color = 0xAA_00_00;
                        x = SCREEN_W / 2 - masks.string_width(GameFont::SmallBold, rest) / 2;
                    }
                    _ => {}
                }
                s = rest;
            }
            masks.stamp(fb, font, s, x, y, color);
        }
        if para.is_empty() {
            y += pitch;
        }
    }
    // {23,17} header tail: black band, white title box, red topic title
    if matches!(mode, 23 | 17) {
        let title = title.expect("mode 17/23 with no topic title (l:S)");
        fb.fill_rect(0, 0, SCREEN_W, small_h + 20, 0x00_00_00);
        fb.fill_rect(5, 5, SCREEN_W - 10, small_h + 10, 0xFF_FF_FF);
        let w = masks.string_width(GameFont::SmallBold, title);
        masks.stamp(
            fb,
            GameFont::SmallBold,
            title,
            SCREEN_W / 2 - w / 2,
            10,
            0xDD_00_00,
        );
    }
    // {4,23,17} bottom bar + "BACK" — fully inside the clipped logical band
    if matches!(mode, 4 | 23 | 17) {
        fb.fill_rect(0, SCREEN_H - small_h - 8, SCREEN_W, small_h + 8, 0x00_00_00);
        masks.stamp(
            fb,
            GameFont::SmallBold,
            "BACK",
            2,
            SCREEN_H - small_h - 2,
            0xFF_FF_FF,
        );
    }
    // The scroll-arrow tail (3186–3502): draw `b:Ld` group 54 (up) when the
    // scroll is above the mode threshold `var16` (m4: 305, m17/23: 10, else
    // 0), group 53 (down) when the final line overruns the limit `var15`.
    // The down arrow's y adds the UP arrow's height (`g.b(d,54)` — faithful
    // quirk). Mode 10 then fills a bottom bar in the SAME parchment color as
    // the page (constant #513 — it erases scrolled text in the band) and
    // redraws both arrows over it with the 305 threshold. Every y here is
    // ≥ 327: wholly inside the clipped 320..345 band, on any device.
    let limit = {
        let mut l = SCREEN_H - small_h;
        if matches!(mode, 10 | 23 | 4 | 17) {
            l -= 3 * small_h;
        }
        l
    };
    let arrow = |fb: &mut Fb, key: i32, y: i32| {
        if let Some(m) = ui {
            let w = m.frame_size(key).0;
            crate::gpaint::draw_model_frame(
                fb,
                assets,
                &m.cml,
                &m.anim,
                key,
                SCREEN_W / 2 - w / 2,
                y,
            );
        }
    };
    let var16 = match mode {
        4 => SCREEN_H - (small_h << 2),
        23 | 17 => 10,
        _ => 0,
    };
    let h54 = ui.map_or(0, |m| m.frame_size(54).1);
    if i32::from(*scroll) < var16 {
        arrow(fb, 54, SCREEN_H - small_h - 7);
    }
    if y > limit {
        arrow(fb, 53, SCREEN_H - small_h - 5 + h54);
    }
    if mode == 10 {
        fb.fill_rect(0, SCREEN_H - small_h - 8, SCREEN_W, small_h + 8, 0xE9_E1_C3);
        if i32::from(*scroll) < SCREEN_H - (small_h << 2) {
            arrow(fb, 54, SCREEN_H - small_h - 7);
        }
        if y > limit {
            arrow(fb, 53, SCREEN_H - small_h - 5 + h54);
        }
    }
    y
}

/// Paint the loader (`b.paint` cases 6/7, offset 2091): black fill, the
/// "Loading..." label at (10,10) in the default font, and the progress bar —
/// red fill `r:B * (a:S-20) / 100` wide inside a white 1px outline at
/// (10,30,220,10). `r:B` is -1 outside an active chunked load (`b.c(int)`),
/// which draws an empty bar, matching the real loader between resources.
/// Visual-only: never behind the byte-identical gate (progress timing is
/// load-latency-dependent even on the oracle).
pub fn paint_loader(fb: &mut Fb, masks: &TextMasks, label: &str, progress: i8) {
    fb.fill(0x00_00_00);
    masks.stamp(fb, GameFont::MediumPlain, label, 10, 10, 0xFF_FF_FF);
    let bar_w = i32::from(progress) * (SCREEN_W - 20) / 100;
    fb.fill_rect(10, 30, bar_w, 10, 0xFF_00_00);
    // drawRect outline (w,h are inclusive spans in MIDP)
    for x in 10..=10 + (SCREEN_W - 20) {
        fb.set(x, 30, 0xFF_FF_FF);
        fb.set(x, 40, 0xFF_FF_FF);
    }
    for y in 30..=40 {
        fb.set(10, y, 0xFF_FF_FF);
        fb.set(10 + (SCREEN_W - 20), y, 0xFF_FF_FF);
    }
}
