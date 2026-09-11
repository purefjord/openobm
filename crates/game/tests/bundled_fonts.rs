//! Public rendering checks: no game archive, captures, or installed fonts.
use game::fb::Fb;
use game::text::{GameFont, TextMasks};
use std::sync::OnceLock;

fn fonts() -> &'static TextMasks {
    static FONTS: OnceLock<TextMasks> = OnceLock::new();
    FONTS.get_or_init(TextMasks::bundled)
}

const FONTS: [GameFont; 4] = [
    GameFont::LargeBold,
    GameFont::SmallBold,
    GameFont::SmallPlain,
    GameFont::MediumPlain,
];

#[test]
fn public_fonts_draw_printable_ascii_and_measure_dynamic_text() {
    let masks = fonts();
    for font in FONTS {
        for c in '!'..='~' {
            let mut fb = Fb::new(32, 32);
            masks.stamp(&mut fb, font, &c.to_string(), 4, 4, 0xFFFFFF);
            assert!(fb.pixels().iter().any(|&p| p != 0), "{font:?}: {c}");
        }
        let label = "Health 123 / 456";
        let width = masks.string_width(font, label);
        assert!(width > 0 && width < 230);
        assert_eq!(width, masks.substring_width(font, label));
        assert_eq!(
            width,
            label
                .chars()
                .map(|c| masks.string_width(font, &c.to_string()))
                .sum()
        );
        assert_eq!(masks.string_width(font, ""), 0);
        assert!(masks.string_width(font, " ") > 0);
    }
}

#[test]
fn missing_glyph_uses_the_same_replacement_for_layout_and_drawing() {
    for font in FONTS {
        let masks = fonts();
        let mut actual = Fb::new(120, 32);
        let mut expected = Fb::new(120, 32);
        masks.stamp(&mut actual, font, "A\u{10ffff}B", 0, 0, 0xFFFFFF);
        masks.stamp(&mut expected, font, "A?B", 0, 0, 0xFFFFFF);
        assert_eq!(actual.pixels(), expected.pixels());
        assert_eq!(
            masks.string_width(font, "A\u{10ffff}B"),
            masks.string_width(font, "A?B")
        );
    }
}

#[test]
fn public_text_clips_and_wraps_with_its_own_metrics() {
    let masks = fonts();
    let mut fb = Fb::new(32, 20);
    fb.fill(0x102030);
    masks.stamp(
        &mut fb,
        GameFont::LargeBold,
        "Clipped glyphs",
        -7,
        -6,
        0xFFFFFF,
    );
    assert!(fb.pixels().iter().any(|&p| p != 0x102030));
    let before = fb.pixels().to_vec();
    masks.stamp(&mut fb, GameFont::SmallPlain, " ", 0, 0, 0xFFFFFF);
    masks.stamp(&mut fb, GameFont::SmallPlain, "Offscreen", 32, 20, 0xFFFFFF);
    assert_eq!(fb.pixels(), before);

    let paragraph =
        "A traveller follows the road toward a village and stops to read a sign beside the gate.";
    let lines = game::wrap::break_lines(masks, paragraph, 100);
    assert!(lines.len() > 1);
    assert_eq!(lines.join(" "), paragraph);
    for line in lines {
        assert!(masks.substring_width(GameFont::SmallBold, &line) <= 100);
    }
}

#[test]
fn captured_binary_masks_keep_exact_pixels_and_strict_missing_glyphs() {
    let masks = TextMasks::parse(
        "font 0-0-8 midp_height=10 ascent=11\ncharw 0-0-8 65=3\nstr 0-0-8 \"A\" w=3 dx=1 dy=2 mw=2 mh=2\n10\n01\n",
    ).unwrap();
    let mut fb = Fb::new(8, 8);
    fb.fill(0x123456);
    masks.stamp(&mut fb, GameFont::SmallPlain, "AA", 0, 0, 0xABCDEF);
    for y in 0..8 {
        for x in 0..8 {
            let ink = [(1, 2), (2, 3), (4, 2), (5, 3)].contains(&(x, y));
            assert_eq!(fb.get(x, y), if ink { 0xABCDEF } else { 0x123456 });
        }
    }
    assert_eq!(masks.try_substring_width(GameFont::SmallPlain, "B"), None);
}
