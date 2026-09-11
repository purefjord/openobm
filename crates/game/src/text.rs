//! Text rendering for public play and optional reference comparisons.
//!
//! [`TextMasks::bundled`] rasterizes the embedded Liberation Sans fonts with
//! fontdue. It needs no installed fonts, Java, game text corpus, or capture file.
//! Layout uses integer character advances without kerning, as the MIDP shell
//! expects. Glyph appearance and wrapping can differ from the reference emulator.
//!
//! [`TextMasks::load`] retains the strict, captured whole-string/character path
//! for private pixel-comparison tests. Those masks are never loaded implicitly
//! by the playable tools, so local fixtures cannot change public rendering.

use crate::fb::Fb;
use std::collections::HashMap;
use std::path::Path;

/// The MIDP fonts the slice draws with (face-style-size triples).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GameFont {
    /// `Font.getFont(0, BOLD, LARGE)` — AWT SansSerif bold 14
    LargeBold,
    /// `Font.getFont(0, BOLD, SMALL)` — AWT SansSerif bold 10
    SmallBold,
    /// `Font.getFont(0, PLAIN, SMALL)` — AWT SansSerif plain 10
    SmallPlain,
    /// `Font.getFont(0, PLAIN, MEDIUM)` — the default font
    MediumPlain,
}

impl GameFont {
    fn from_key(k: &str) -> Option<Self> {
        Some(match k {
            "0-1-16" => GameFont::LargeBold,
            "0-1-8" => GameFont::SmallBold,
            "0-0-8" => GameFont::SmallPlain,
            "0-0-0" => GameFont::MediumPlain,
            _ => return None,
        })
    }
}

pub struct Mask {
    /// `Font.stringWidth` — the game's layout math (`120 - w/2`) uses it.
    pub string_width: i32,
    /// Ink bounding-box offset relative to the requested anchor-0 (x, y).
    pub dx: i32,
    pub dy: i32,
    pub mw: usize,
    pub mh: usize,
    ink: Vec<u8>, // mw*mh, row-major coverage (0 = transparent, 255 = solid)
}

pub struct FontMetrics {
    /// MIDP `Font.getHeight()` — layout constant.
    pub midp_height: i32,
    /// AWT ascent — `drawString` puts the baseline at `y + ascent - 1`.
    pub ascent: i32,
}

pub struct TextMasks {
    fonts: HashMap<GameFont, FontMetrics>,
    masks: HashMap<(GameFont, String), Mask>,
    /// Integer character advances shared by drawing, centering, and wrapping.
    char_widths: HashMap<GameFont, HashMap<char, i32>>,
    /// Public fonts replace unsupported characters; reference captures stay strict.
    fallback: Option<char>,
}

impl TextMasks {
    /// The public font setup, embedded at compile time and independent of the
    /// filesystem. The unmodified font files and their OFL license are in `fonts/`.
    pub fn bundled() -> Self {
        let regular = fontdue::Font::from_bytes(
            &include_bytes!("../fonts/LiberationSans-Regular.ttf")[..],
            fontdue::FontSettings::default(),
        )
        .expect("valid bundled regular font");
        let bold = fontdue::Font::from_bytes(
            &include_bytes!("../fonts/LiberationSans-Bold.ttf")[..],
            fontdue::FontSettings::default(),
        )
        .expect("valid bundled bold font");
        let mut result = Self {
            fonts: HashMap::new(),
            masks: HashMap::new(),
            char_widths: HashMap::new(),
            fallback: Some('?'),
        };
        for (font, face, size) in [
            (GameFont::LargeBold, &bold, 14),
            (GameFont::SmallBold, &bold, 10),
            (GameFont::SmallPlain, &regular, 10),
            (GameFont::MediumPlain, &regular, 12),
        ] {
            // Preserve the shell's nominal line heights and anchor-0 baseline.
            // These are layout constants, not a claim of reference pixel parity.
            result.fonts.insert(
                font,
                FontMetrics {
                    midp_height: size,
                    ascent: size + 1,
                },
            );
            let mut widths = HashMap::new();
            for &c in face.chars().keys() {
                let (m, ink) = face.rasterize(c, size as f32);
                let width = m.advance_width.round() as i32;
                widths.insert(c, width);
                result.masks.insert(
                    (font, c.to_string()),
                    Mask {
                        string_width: width,
                        dx: m.xmin,
                        dy: size - m.ymin - m.height as i32,
                        mw: m.width,
                        mh: m.height,
                        ink,
                    },
                );
            }
            result.char_widths.insert(font, widths);
        }
        result
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Self::parse(&std::fs::read_to_string(path)?)
    }

    pub fn parse(src: &str) -> anyhow::Result<Self> {
        let mut lines = src.lines();
        let mut fonts = HashMap::new();
        let mut masks = HashMap::new();
        let mut char_widths: HashMap<GameFont, HashMap<char, i32>> = HashMap::new();
        while let Some(line) = lines.next() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("charw ") {
                let mut it = rest.split_whitespace();
                let font = it
                    .next()
                    .and_then(GameFont::from_key)
                    .ok_or_else(|| anyhow::anyhow!("bad charw line: {line}"))?;
                let table = char_widths.entry(font).or_default();
                for kv in it {
                    let (code, w) = kv
                        .split_once('=')
                        .ok_or_else(|| anyhow::anyhow!("bad charw pair {kv}: {line}"))?;
                    let code: u32 = code.parse()?;
                    let c = char::from_u32(code)
                        .ok_or_else(|| anyhow::anyhow!("bad char code {code}: {line}"))?;
                    table.insert(c, w.parse()?);
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("font ") {
                let mut it = rest.split_whitespace();
                let font = it
                    .next()
                    .and_then(GameFont::from_key)
                    .ok_or_else(|| anyhow::anyhow!("bad font line: {line}"))?;
                let mut height = None;
                let mut ascent = None;
                for kv in it {
                    if let Some(v) = kv.strip_prefix("midp_height=") {
                        height = Some(v.parse()?);
                    } else if let Some(v) = kv.strip_prefix("ascent=") {
                        ascent = Some(v.parse()?);
                    }
                }
                fonts.insert(
                    font,
                    FontMetrics {
                        midp_height: height
                            .ok_or_else(|| anyhow::anyhow!("no midp_height: {line}"))?,
                        ascent: ascent.ok_or_else(|| anyhow::anyhow!("no ascent: {line}"))?,
                    },
                );
                continue;
            }
            let rest = line
                .strip_prefix("str ")
                .ok_or_else(|| anyhow::anyhow!("unexpected line: {line}"))?;
            let (font_key, rest) = rest
                .split_once(" \"")
                .ok_or_else(|| anyhow::anyhow!("bad str line: {line}"))?;
            let font = GameFont::from_key(font_key)
                .ok_or_else(|| anyhow::anyhow!("unknown font {font_key}: {line}"))?;
            // the string is escaped with \\ and \" and ends at the unescaped quote
            let mut s = String::new();
            let mut chars = rest.chars();
            let tail: String = loop {
                match chars.next() {
                    Some('\\') => match chars.next() {
                        Some(c) => s.push(c),
                        None => anyhow::bail!("dangling escape in: {line}"),
                    },
                    Some('"') => break chars.collect(),
                    Some(c) => s.push(c),
                    None => anyhow::bail!("unterminated string in: {line}"),
                }
            };
            let kv = |key: &str| -> anyhow::Result<i32> {
                tail.split_whitespace()
                    .find_map(|t| t.strip_prefix(key))
                    .ok_or_else(|| anyhow::anyhow!("missing {key} in: {line}"))?
                    .parse()
                    .map_err(Into::into)
            };
            let (string_width, dx, dy) = (kv("w=")?, kv("dx=")?, kv("dy=")?);
            let (mw, mh) = (kv("mw=")? as usize, kv("mh=")? as usize);
            let mut ink = Vec::with_capacity(mw * mh);
            for _ in 0..mh {
                let row = lines
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("truncated mask for {s:?}"))?;
                anyhow::ensure!(row.len() == mw, "bad row width for {s:?}");
                ink.extend(row.chars().map(|c| if c == '1' { 255 } else { 0 }));
            }
            masks.insert(
                (font, s),
                Mask {
                    string_width,
                    dx,
                    dy,
                    mw,
                    mh,
                    ink,
                },
            );
        }
        anyhow::ensure!(!fonts.is_empty(), "no font headers in fixture");
        Ok(Self {
            fonts,
            masks,
            char_widths,
            fallback: None,
        })
    }

    pub fn metrics(&self, font: GameFont) -> &FontMetrics {
        &self.fonts[&font]
    }

    pub fn get(&self, font: GameFont, s: &str) -> &Mask {
        self.masks.get(&(font, s.to_owned())).unwrap_or_else(|| {
            panic!("string not in text-mask fixture (extend the corpus + rerun oracle/TextCapture): {font:?} {s:?}")
        })
    }

    fn try_get(&self, font: GameFont, s: &str) -> Option<&Mask> {
        self.masks.get(&(font, s.to_owned()))
    }

    /// `Font.stringWidth` for layout math (the game centers as `120 - w/2`).
    /// Falls back to the (capture-verified additive) char-advance sum for a
    /// string with no whole-string mask — dynamic text (damage numbers, HUD
    /// labels off the gated paths) measures identically either way.
    pub fn string_width(&self, font: GameFont, s: &str) -> i32 {
        match self.try_get(font, s) {
            Some(m) => m.string_width,
            None => self.substring_width(font, s),
        }
    }

    /// Non-panicking [`Self::substring_width`]. Missing characters use `?` in
    /// bundled mode, or return `None` in strict reference mode.
    pub fn try_substring_width(&self, font: GameFont, s: &str) -> Option<i32> {
        let table = self.char_widths.get(&font)?;
        s.chars()
            .map(|c| {
                table
                    .get(&c)
                    .or_else(|| self.fallback.and_then(|fallback| table.get(&fallback)))
                    .copied()
            })
            .sum()
    }

    /// Sum the integer character advances used by the stamper. Reference mode
    /// rejects missing characters; bundled mode uses the replacement advance.
    pub fn substring_width(&self, font: GameFont, s: &str) -> i32 {
        let table = self
            .char_widths
            .get(&font)
            .unwrap_or_else(|| panic!("no charw table for {font:?} (rerun oracle/TextCapture)"));
        s.chars()
            .map(|c| {
                *table.get(&c).or_else(|| self.fallback.and_then(|fallback| table.get(&fallback))).unwrap_or_else(|| {
                    panic!("char not in width fixture (extend the corpus + rerun oracle/TextCapture): {font:?} {c:?}")
                })
            })
            .sum()
    }

    /// Draw at the MIDP anchor-0 position, using a whole-string capture when
    /// present, otherwise composing glyphs at integer advances. Bundled fonts
    /// blend antialiased coverage; binary reference masks keep their exact ink.
    pub fn stamp(&self, fb: &mut Fb, font: GameFont, s: &str, x: i32, y: i32, rgb: u32) {
        if let Some(m) = self.try_get(font, s) {
            Self::blit(fb, m, x, y, rgb);
            return;
        }
        let mut ax = x;
        for c in s.chars() {
            if c != ' ' {
                let key = c.to_string();
                let m = self
                    .try_get(font, &key)
                    .unwrap_or_else(|| match self.fallback {
                        Some(fallback) => self.get(font, fallback.encode_utf8(&mut [0u8; 4])),
                        None => self.get(font, &key),
                    });
                Self::blit(fb, m, ax, y, rgb);
            }
            ax += self.substring_width(font, c.encode_utf8(&mut [0u8; 4]));
        }
    }

    fn blit(fb: &mut Fb, m: &Mask, x: i32, y: i32, rgb: u32) {
        for row in 0..m.mh {
            for col in 0..m.mw {
                let coverage = u32::from(m.ink[row * m.mw + col]);
                let (xx, yy) = (x + m.dx + col as i32, y + m.dy + row as i32);
                if coverage == 0 || xx < 0 || yy < 0 || xx >= fb.w || yy >= fb.h {
                    continue;
                }
                let bg = fb.get(xx, yy);
                let channel = |shift: u32| -> u32 {
                    let fg = (rgb >> shift) & 255;
                    let bg = (bg >> shift) & 255;
                    (fg * coverage + bg * (255 - coverage) + 127) / 255
                };
                fb.set(xx, yy, (channel(16) << 16) | (channel(8) << 8) | channel(0));
            }
        }
    }
}
