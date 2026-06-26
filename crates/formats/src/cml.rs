//! `.cml` model / animation / sprite-table parser.
//!
//! Faithful port of `g.java::d_a(String)` (the loader, ~line 76) and its flag
//! reader `g.java::a(byte[], int, int[])` (~line 25). `d.java` is just the
//! linked frame-node struct the original builds; we use flat vectors instead
//! (per spec.txt).
//!
//! File layout (verified against `g.class` bytecode where the decompiler was
//! ambiguous — see below):
//! ```text
//! u8 prefix_len
//! prefix_len bytes : a path prefix prepended to every relative image path
//! repeated records until end of file:
//!   u8 frame_id                       (c2)
//!   u8 path_len  (c3)
//!   path_len bytes : image path; if it doesn't start with '/', prefix is prepended
//!   flag block  (see read_flags) -> 10 fields
//!   u8 box_count (n7); box_count * { u24 BE, u24 BE }
//!   u8 group_count (n6)
//!   if path == "/4.png": record ends here (the original `continue`s)
//!   else if group_count == 0: a single static frame (size comes from the PNG)
//!   else: group_count groups, each { flag block; u8 frame_count; frame_count * flag block }
//! ```
//!
//! **Decompiler trap (resolved via `javap -c g`)**: CFR renders the path read as
//! `new String(byArray, n4, c3 = byArray[n4++])`, which *reads* as "string offset
//! = n4 before the increment". The bytecode evaluates the `c3 = ...` (with its
//! `iinc`) first and only then loads `n4` for the offset, so the offset is `n4`
//! **after** the increment: `path = bytes[n4+1 .. n4+1+c3]`, with the flag block
//! then read at `n4+1+c3` (the original computes `a(byArray, n4 + c3, ..)` with
//! the already-incremented `n4`). We port the bytecode's actual behavior.

use crate::reader::ParseError;

/// A decoded flag block: 10 fields, populated by bit flags. Fields 0..=4 are
/// unsigned; fields 5..=9 are signed bytes (offsets, may be negative).
pub type Flags = [i32; 10];

/// One animation group: its own flag block plus a list of per-frame flag blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmlAnimGroup {
    pub flags: Flags,
    pub frames: Vec<Flags>,
}

/// One model record (a named image with frames/animations).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmlRecord {
    /// Raw frame id byte (`c2`).
    pub frame_id: u8,
    /// Effective id (`nArray[0]` if non-zero, else `c2`) — the table key.
    pub effective_id: i32,
    /// Image path (prefix prepended when relative).
    pub path: String,
    /// Record-level flag block (with field 0 resolved to `effective_id`).
    pub flags: Flags,
    /// Bounding boxes: `box_count` pairs of 24-bit values.
    pub boxes: Vec<(i32, i32)>,
    /// Animation groups (empty when `is_static`).
    pub anim_groups: Vec<CmlAnimGroup>,
    /// True when `group_count == 0` (a single static frame; size from the PNG).
    pub is_static: bool,
    /// True for the special `/4.png` record, which the original skips.
    pub skipped: bool,
}

/// A parsed `.cml` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cml {
    /// Path prefix applied to relative image paths.
    pub prefix: String,
    pub records: Vec<CmlRecord>,
    /// Total bytes consumed (should equal the file length for well-formed input).
    pub consumed: usize,
}

fn at(b: &[u8], n: usize) -> Result<u8, ParseError> {
    b.get(n).copied().ok_or(ParseError::Eof)
}

fn u24(b: &[u8], n: usize) -> Result<i32, ParseError> {
    Ok((i32::from(at(b, n)?) << 16) | (i32::from(at(b, n + 1)?) << 8) | i32::from(at(b, n + 2)?))
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Port of `g.java::a(byte[], int, int[])`: read a 16-bit flag word, then read
/// each present field. Returns `(flags, next_offset)`.
fn read_flags(b: &[u8], mut n: usize) -> Result<(Flags, usize), ParseError> {
    let word = (i32::from(at(b, n)?) << 8) | i32::from(at(b, n + 1)?);
    n += 2;
    let mut f: Flags = [0; 10];
    // Fields 0..=4 are read unsigned; 5..=9 as signed bytes.
    if word & 0x200 != 0 {
        f[0] = i32::from(at(b, n)?);
        n += 1;
    }
    if word & 0x100 != 0 {
        f[1] = (i32::from(at(b, n)?) << 8) | i32::from(at(b, n + 1)?);
        n += 2;
    }
    if word & 0x80 != 0 {
        f[2] = (i32::from(at(b, n)?) << 8) | i32::from(at(b, n + 1)?);
        n += 2;
    }
    if word & 0x40 != 0 {
        f[3] = i32::from(at(b, n)?);
        n += 1;
    }
    if word & 0x20 != 0 {
        f[4] = i32::from(at(b, n)?);
        n += 1;
    }
    if word & 0x10 != 0 {
        f[5] = i32::from(at(b, n)? as i8);
        n += 1;
    }
    if word & 0x08 != 0 {
        f[6] = i32::from(at(b, n)? as i8);
        n += 1;
    }
    if word & 0x04 != 0 {
        f[7] = i32::from(at(b, n)? as i8);
        n += 1;
    }
    if word & 0x02 != 0 {
        f[8] = i32::from(at(b, n)? as i8);
        n += 1;
    }
    if word & 0x01 != 0 {
        f[9] = i32::from(at(b, n)? as i8);
        n += 1;
    }
    Ok((f, n))
}

/// Parse a `.cml` file. Returns [`ParseError::Eof`] on truncated/malformed input
/// rather than panicking (safe to fuzz).
pub fn parse_cml(bytes: &[u8]) -> Result<Cml, ParseError> {
    let total = bytes.len();
    let prefix_len = usize::from(at(bytes, 0)?);
    let prefix = if prefix_len > 0 {
        latin1(bytes.get(1..1 + prefix_len).ok_or(ParseError::Eof)?)
    } else {
        String::new()
    };

    let mut n4 = 1 + prefix_len;
    let mut records = Vec::new();
    while n4 < total {
        let frame_id = at(bytes, n4)?;
        n4 += 1;
        let path_len = usize::from(at(bytes, n4)?);
        n4 += 1;
        // Offset is n4 AFTER the length increment (see module docs / bytecode).
        let path_bytes = bytes.get(n4..n4 + path_len).ok_or(ParseError::Eof)?;
        let raw = latin1(path_bytes);
        let path = if raw.starts_with('/') {
            raw
        } else {
            format!("{prefix}{raw}")
        };

        // Flag block is read at n4 + path_len (n4 is not advanced by the string).
        let (mut flags, next) = read_flags(bytes, n4 + path_len)?;
        n4 = next;
        let effective_id = if flags[0] != 0 {
            flags[0]
        } else {
            i32::from(frame_id)
        };
        flags[0] = effective_id; // mirrors `if (nArray[0]==0) nArray[0]=c2;`

        let box_count = at(bytes, n4)?;
        n4 += 1;
        let mut boxes = Vec::with_capacity(usize::from(box_count));
        for _ in 0..box_count {
            let a = u24(bytes, n4)?;
            let b = u24(bytes, n4 + 3)?;
            n4 += 6;
            boxes.push((a, b));
        }

        let group_count = at(bytes, n4)?;
        n4 += 1;

        if path == "/4.png" {
            records.push(CmlRecord {
                frame_id,
                effective_id,
                path,
                flags,
                boxes,
                anim_groups: Vec::new(),
                is_static: group_count == 0,
                skipped: true,
            });
            continue;
        }

        let mut anim_groups = Vec::new();
        if group_count != 0 {
            for _ in 0..group_count {
                let (gflags, next) = read_flags(bytes, n4)?;
                n4 = next;
                let frame_count = at(bytes, n4)?;
                n4 += 1;
                let mut frames = Vec::with_capacity(usize::from(frame_count));
                for _ in 0..frame_count {
                    let (fflags, next) = read_flags(bytes, n4)?;
                    n4 = next;
                    frames.push(fflags);
                }
                anim_groups.push(CmlAnimGroup {
                    flags: gflags,
                    frames,
                });
            }
        }

        records.push(CmlRecord {
            frame_id,
            effective_id,
            path,
            flags,
            boxes,
            anim_groups,
            is_static: group_count == 0,
            skipped: false,
        });
    }

    Ok(Cml {
        prefix,
        records,
        consumed: n4,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_is_error_not_panic() {
        assert_eq!(parse_cml(&[]).err(), Some(ParseError::Eof));
        assert_eq!(parse_cml(&[5]).err(), Some(ParseError::Eof)); // prefix_len 5, no data
    }

    #[test]
    fn static_record_with_absolute_path() {
        // prefix_len=0; one record: id=1, path "/x" (len 2), flags word=0 (no
        // fields), box_count=0, group_count=0 (static).
        let data = [
            0u8, // prefix_len
            1,   // frame_id
            2, b'/', b'x', // path_len + "/x"
            0x00, 0x00, // flag word: no fields
            0,    // box_count
            0,    // group_count -> static
        ];
        let cml = parse_cml(&data).unwrap();
        assert_eq!(cml.prefix, "");
        assert_eq!(cml.records.len(), 1);
        let r = &cml.records[0];
        assert_eq!(r.frame_id, 1);
        assert_eq!(r.path, "/x");
        assert_eq!(r.effective_id, 1); // flags[0] was 0 -> c2
        assert!(r.is_static);
        assert_eq!(cml.consumed, data.len());
    }

    #[test]
    fn relative_path_gets_prefix() {
        // prefix_len=3 "ab/"; record path "c.png" (relative) -> "ab/c.png".
        let data = [
            3u8, b'a', b'b', b'/', // prefix
            7,    // frame_id
            5, b'c', b'.', b'p', b'n', b'g', // path_len + "c.png"
            0x00, 0x00, // flag word
            0,    // boxes
            0,    // groups
        ];
        let cml = parse_cml(&data).unwrap();
        assert_eq!(cml.prefix, "ab/");
        assert_eq!(cml.records[0].path, "ab/c.png");
    }

    #[test]
    fn flag_word_reads_present_fields_only() {
        // word 0x0240 = bits 0x200 (field0, 1 byte) + 0x40 (field3, 1 byte).
        let data = [
            0u8, // prefix
            9,   // id
            2, b'/', b'q', // "/q"
            0x02, 0x40, // flags: field0=next byte, field3=next byte
            0x05, // field0 = 5
            0x07, // field3 = 7
            0,    // boxes
            0,    // groups
        ];
        let cml = parse_cml(&data).unwrap();
        let f = cml.records[0].flags;
        assert_eq!(f[0], 5); // field0 present, non-zero -> effective id
        assert_eq!(f[3], 7);
        assert_eq!(cml.records[0].effective_id, 5);
        assert_eq!(cml.consumed, data.len());
    }
}
