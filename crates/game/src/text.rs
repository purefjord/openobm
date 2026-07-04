//! Whole-string text stamping from oracle-captured ink masks.
//!
//! The game draws ALL text via `Graphics.drawString` with MIDP system fonts,
//! rasterized on the oracle by java.awt (SansSerif, antialiasing OFF —
//! verified two-color text regions). Those pixels can't be re-derived in Rust,
//! so `oracle/TextCapture.java` captures every string the slice draws as a
//! binary ink mask through the exact drawString code path (same anchor-0 +
//! `ascent-1` baseline math), keyed by **(font, verbatim string)**. Stamping a
//! mask in the draw color at the game-requested (x, y) reproduces the
//! oracle's text byte-for-byte over any background.
//!
//! Recon facts baked in here (see `docs/loop-recon.md` + artifacts/recon4):
//! - the slice uses four fonts: system/bold/large (carousel items, headers),
//!   system/bold/small ("Press any key", "BACK"), system/plain/small (legal
//!   scroll), system/plain/medium ("Loading...");
//! - the game's centering formula is `x = 120 - stringWidth/2` (Java int div);
//! - FreeJ2ME gotcha recorded for posterity: `PlatformGraphics.font` shadows
//!   the superclass field and goes stale — the destination `gc` font decides
//!   the pixels (that's what the capture uses).
//!
//! A string missing from the fixture is a hard error by design: it means the
//! corpus needs extending (rerun the capture), never that we should guess.

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
    bits: Vec<bool>, // mw*mh, row-major
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
    /// Per-char advance widths (`Font.charWidth`) captured per font. The
    /// capture VERIFIES `substringWidth(s,i,n)` equals the char-width sum for
    /// every substring of the corpus, so summing these reproduces the exact
    /// measurement the game's word-wrap (`b.a(String,Vector,int)`) performs.
    char_widths: HashMap<GameFont, HashMap<char, i32>>,
}

impl TextMasks {
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
            let mut bits = Vec::with_capacity(mw * mh);
            for _ in 0..mh {
                let row = lines
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("truncated mask for {s:?}"))?;
                anyhow::ensure!(row.len() == mw, "bad row width for {s:?}");
                bits.extend(row.chars().map(|c| c == '1'));
            }
            masks.insert(
                (font, s),
                Mask {
                    string_width,
                    dx,
                    dy,
                    mw,
                    mh,
                    bits,
                },
            );
        }
        anyhow::ensure!(!fonts.is_empty(), "no font headers in fixture");
        Ok(Self {
            fonts,
            masks,
            char_widths,
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

    /// `Font.stringWidth` for layout math (the game centers as `120 - w/2`).
    pub fn string_width(&self, font: GameFont, s: &str) -> i32 {
        self.get(font, s).string_width
    }

    /// `Font.substringWidth`-equivalent measurement for the word-wrap: the sum
    /// of captured per-char advances (verified additive by the capture). A
    /// char missing from the fixture is a hard error, same policy as masks.
    pub fn substring_width(&self, font: GameFont, s: &str) -> i32 {
        let table = self
            .char_widths
            .get(&font)
            .unwrap_or_else(|| panic!("no charw table for {font:?} (rerun oracle/TextCapture)"));
        s.chars()
            .map(|c| {
                *table.get(&c).unwrap_or_else(|| {
                    panic!("char not in width fixture (extend the corpus + rerun oracle/TextCapture): {font:?} {c:?}")
                })
            })
            .sum()
    }

    /// Draw `s` exactly as `Graphics.drawString(s, x, y, 0)` does on the
    /// oracle with `font` set: ink pixels become `rgb`, the rest untouched.
    pub fn stamp(&self, fb: &mut Fb, font: GameFont, s: &str, x: i32, y: i32, rgb: u32) {
        let m = self.get(font, s);
        for row in 0..m.mh {
            for col in 0..m.mw {
                if m.bits[row * m.mw + col] {
                    fb.set(x + m.dx + col as i32, y + m.dy + row as i32, rgb);
                }
            }
        }
    }
}
