//! Text-page word-wrap — `b.java`'s `h(String)` (javap 11352) and the line
//! breaker `a(String, Vector, int)` (javap 18317), transcribed from bytecode.
//!
//! `h(String)` builds the text-page model (`a:[Ljava/util/Vector;`): it splices
//! the MIDlet version over the first literal `VERSION`, splits the text into
//! paragraphs on the TWO-CHAR escape `\n` (backslash + 'n', not a newline),
//! then wraps each paragraph at width `a:S - 10` (= 230) measuring with
//! **`c:Font` (small bold)** via `Font.substringWidth`. The paint draws mode-21
//! lines with the DEFAULT medium font, so drawn lines can overflow the wrap
//! width and clip at the right edge — faithful (oracle textlog shows w=255
//! lines on the 240px legal page).
//!
//! Out-of-slice fences: the breaker's key-name substitutions (`ACTION_KEY`,
//! `TOGGLE_WEAPON_KEY`, `QUICK_HEALTH_KEY`, `QUICK_MAGIKA_KEY` — Help-page
//! tokens replaced from the key-name table + cheat state) and the `f:String`
//! speaker-prefix state (dialog "name:" splitting) are not modeled; input
//! containing them is rejected loudly rather than wrapped wrong.

use crate::text::{GameFont, TextMasks};

/// Tokens the real breaker substitutes from game state (Help page); fenced.
const KEY_TOKENS: [&str; 4] = [
    "ACTION_KEY",
    "TOGGLE_WEAPON_KEY",
    "QUICK_HEALTH_KEY",
    "QUICK_MAGIKA_KEY",
];

/// `b.a(String, Vector, int)` — wrap one paragraph into lines at `width`,
/// measuring `c:Font` (small bold) substring widths. Greedy scan: track the
/// last space; when the measured run reaches `width` and a space was seen,
/// emit up to that space and restart after it.
pub fn break_lines(masks: &TextMasks, text: &str, width: i32) -> Vec<String> {
    for tok in KEY_TOKENS {
        assert!(
            !text.contains(tok),
            "key-name substitution not ported (out of slice): {tok:?}"
        );
    }
    assert!(
        !text.contains(':'),
        "speaker-prefix (f:String) splitting not ported (out of slice)"
    );
    // NOTE the original iterates chars by Java index; the slice's corpus is
    // ASCII/Latin-1 so byte==char here (asserted).
    assert!(
        text.is_ascii(),
        "non-ASCII wrap input needs char-index care"
    );
    let b = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0usize; // i6: line start
    let mut last_space = 0usize; // i5: last space seen (0 = none)
    let mut i = 0usize; // i3
    while i + 1 < b.len() {
        // for (i3 = 0; i3 < len - 1; i3++)
        let w = masks.substring_width(GameFont::SmallBold, &text[start..=i]);
        if b[i] == b' ' {
            last_space = i;
        }
        if w >= width && last_space > 0 {
            lines.push(text[start..last_space].to_string());
            start = last_space + 1;
            i = last_space + 1;
            last_space = 0;
        }
        i += 1;
    }
    // if (i3 > i6) add substring(i6, i3 + 1)
    if i > start {
        lines.push(text[start..(i + 1).min(text.len())].to_string());
    }
    lines
}

/// `b.h(String)` — the text-page builder: VERSION splice, `\n`-escape
/// paragraph split, per-paragraph wrap at `a:S - 10`.
pub fn build_pages(masks: &TextMasks, text: &str, version: &str) -> Vec<Vec<String>> {
    let text = match text.find("VERSION") {
        Some(i) => format!("{}{}{}", &text[..i], version, &text[i + 7..]),
        None => text.to_string(),
    };
    text.split("\\n")
        .map(|para| break_lines(masks, para, 240 - 10))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn masks() -> TextMasks {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        TextMasks::load(&root.join("tests/fixtures/oracle/text_masks.txt")).unwrap()
    }

    /// The legal page (`new a().a()` = /copywrite.txt) must wrap into exactly
    /// the 7 lines the REAL game drew (oracle textlog, artifacts/recon).
    #[test]
    fn legal_wrap_matches_oracle_lines() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let legal = std::fs::read_to_string(root.join("assets/copywrite.txt")).unwrap();
        let pages = build_pages(&masks(), &legal, "1.0.10");
        assert_eq!(pages.len(), 1, "copywrite.txt is a single paragraph");
        assert_eq!(
            pages[0],
            vec![
                "@2006 Vir2L Studios LLC/Bethesda Softworks",
                "LLC, ZeniMax Media companies. The Elder",
                "Scrolls, Oblivion, Oblivion Mobile, Vir2L,",
                "Bethesda Softworks, ZeniMax and related",
                "logos are registered trademarks and/or",
                "trademarks of ZeniMax Media Inc. All rights",
                "reserved.",
            ]
        );
    }
}
