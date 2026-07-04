//! `lang_*.txt` string-table parser.
//!
//! Faithful port of `b.java::a(String, int)` (~line 2967) plus the lookup in
//! `java_lang_String_a(int)` (~line 3023) and the size/count tables in `a.java`
//! (`int_a` and `b`).
//!
//! File format (verified against bytes of `lang_0.txt`): a sequence of records
//! `"<id> <text>|\r\n"`. The id is the ASCII integer before the first space;
//! the text is everything after that space up to (not including) the `|`. Each
//! record is terminated by `|` and followed by a `\r\n` (CRLF) before the next
//! record — the Java loader skips that CRLF with `n3 += 2` for every record
//! after the first.
//!
//! The loader reads *exactly* `record_count(lang_id)` records (from `a.b`), not
//! "until EOF" — extra `|`-segments in a file are never registered. We mirror
//! that.
//!
//! Two corrections to `spec.txt`, verified against source:
//!  1. **Base wins over overlay.** `java_lang_String_a` checks the base table
//!     (`lang_0`, `var_short_arr_b`) first and only falls back to the secondary
//!     table (`var_short_arr_c`) for ids missing from the base. spec.txt claims
//!     overlay wins; the bytecode says otherwise.
//!  2. **Text is Latin-1, not UTF-8.** The Java reads each byte and widens it
//!     with `(char)n`, i.e. ISO-8859-1. We decode byte->char the same way; this
//!     matters for the German/French translation files.

use std::collections::BTreeMap;

/// Number of distinct string ids reserved for each `lang_N` file.
///
/// Ported from `a.java::int_a(int)`. Used by the Java loader only to size the
/// id->offset table; we keep it for fidelity and bounds documentation.
pub fn lang_table_size(lang_id: u8) -> Option<usize> {
    const SIZES: [usize; 13] = [
        574, 550, 399, 345, 362, 565, 494, 546, 495, 495, 495, 409, 547,
    ];
    SIZES.get(lang_id as usize).copied()
}

/// Number of `id text|` records present in each `lang_N` file.
///
/// Ported from `a.java::b(int)`. This bounds the parse loop exactly as the
/// original does.
pub fn lang_record_count(lang_id: u8) -> Option<usize> {
    const COUNTS: [usize; 13] = [305, 34, 46, 9, 16, 6, 42, 20, 16, 30, 9, 7, 6];
    COUNTS.get(lang_id as usize).copied()
}

/// Decode bytes as ISO-8859-1 (each byte is its own code point), matching the
/// Java `(char)n` widening.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn parse_ascii_u16(bytes: &[u8]) -> Option<u16> {
    if bytes.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    for &b in bytes {
        let d = b.checked_sub(b'0').filter(|&d| d <= 9)?;
        acc = acc.checked_mul(10)?.checked_add(u32::from(d))?;
    }
    u16::try_from(acc).ok()
}

/// Parse a `lang_*.txt` buffer into an id->text map, reading exactly
/// `record_count` records (mirroring the Java loop bound). Malformed input
/// stops early rather than panicking, so this is safe to fuzz.
pub fn parse_lang(bytes: &[u8], record_count: usize) -> BTreeMap<u16, String> {
    let mut out = BTreeMap::new();
    let mut start = 0usize; // n3: start offset of the current record
    let mut scan = 0usize; // n4: scan cursor searching for '|'

    for _ in 0..record_count {
        while scan < bytes.len() && bytes[scan] != b'|' {
            scan += 1;
        }
        if scan >= bytes.len() {
            break; // fewer records than expected; stop cleanly
        }
        if start != 0 {
            start += 2; // skip the CRLF preceding every record after the first
        }
        if start > scan {
            break; // malformed; avoid an out-of-range slice
        }

        let record = &bytes[start..scan];
        if let Some(sp) = record.iter().position(|&b| b == b' ') {
            if let Some(id) = parse_ascii_u16(&record[..sp]) {
                out.insert(id, latin1(&record[sp + 1..]));
            }
        }

        start = scan + 1;
        scan += 1;
    }

    out
}

/// Convenience: parse a `lang_N` file using its known record count.
///
/// Returns `None` for an unknown `lang_id` (only 0..=12 exist in the asset set).
pub fn parse_lang_file(bytes: &[u8], lang_id: u8) -> Option<BTreeMap<u16, String>> {
    Some(parse_lang(bytes, lang_record_count(lang_id)?))
}

/// Runtime string table: a base table (`lang_0`) plus an optional overlay
/// (another `lang_N`). Lookup mirrors `java_lang_String_a`: base first, overlay
/// only as a fallback for ids absent from the base.
#[derive(Debug, Default, Clone)]
pub struct Lang {
    base: BTreeMap<u16, String>,
    overlay: BTreeMap<u16, String>,
}

impl Lang {
    /// Build from a parsed base table (`lang_0`).
    pub fn from_base(base: BTreeMap<u16, String>) -> Self {
        Self {
            base,
            overlay: BTreeMap::new(),
        }
    }

    /// Load `lang_0` bytes as the base table.
    pub fn load_base(bytes: &[u8]) -> Self {
        Self::from_base(parse_lang(bytes, lang_record_count(0).unwrap()))
    }

    /// Install a secondary table that supplies ids missing from the base.
    pub fn set_overlay(&mut self, overlay: BTreeMap<u16, String>) {
        self.overlay = overlay;
    }

    /// Look up a string by id. Base table wins; overlay is the fallback. Returns
    /// `""` for unknown ids, matching the Java behavior.
    pub fn get(&self, id: u16) -> &str {
        if let Some(s) = self.base.get(&id) {
            return s;
        }
        if let Some(s) = self.overlay.get(&id) {
            return s;
        }
        ""
    }

    /// `b.int_b(String)` — reverse lookup over the BASE table only (the
    /// original scans `var_short_arr_b`/`var_char_arr_a`, which hold lang_0;
    /// overlay strings never reverse-resolve). First matching id in id order
    /// (the scan runs ids 1..n ascending), or `None`.
    pub fn reverse(&self, s: &str) -> Option<u16> {
        self.base
            .iter()
            .find(|(_, v)| v.as_str() == s)
            .map(|(&id, _)| id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_records() {
        // "1 Language|\r\n2 New Game|\r\n" — mirrors the real file layout.
        let data = b"1 Language|\r\n2 New Game|\r\n";
        let map = parse_lang(data, 2);
        assert_eq!(map.get(&1).map(String::as_str), Some("Language"));
        assert_eq!(map.get(&2).map(String::as_str), Some("New Game"));
    }

    #[test]
    fn text_may_contain_spaces_split_on_first_only() {
        let data = b"10 Load Game|\r\n";
        let map = parse_lang(data, 1);
        assert_eq!(map.get(&10).map(String::as_str), Some("Load Game"));
    }

    #[test]
    fn record_count_bounds_the_loop() {
        let data = b"1 A|\r\n2 B|\r\n3 C|\r\n";
        let map = parse_lang(data, 2); // only first two registered
        assert_eq!(map.len(), 2);
        assert!(!map.contains_key(&3));
    }

    #[test]
    fn base_wins_over_overlay() {
        let mut base = BTreeMap::new();
        base.insert(1u16, "base-one".to_string());
        let mut overlay = BTreeMap::new();
        overlay.insert(1u16, "overlay-one".to_string());
        overlay.insert(2u16, "overlay-two".to_string());

        let mut lang = Lang::from_base(base);
        lang.set_overlay(overlay);

        assert_eq!(lang.get(1), "base-one"); // base wins
        assert_eq!(lang.get(2), "overlay-two"); // overlay fallback
        assert_eq!(lang.get(999), ""); // unknown -> empty
    }

    #[test]
    fn malformed_input_does_not_panic() {
        // No terminator, no space, truncated — must not panic.
        let _ = parse_lang(b"", 5);
        let _ = parse_lang(b"nodelim", 5);
        let _ = parse_lang(b"|||", 5);
        let _ = parse_lang(b"5", 5);
        let _ = parse_lang(b"abc def|", 5);
    }
}
