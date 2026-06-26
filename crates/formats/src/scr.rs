//! `.scr` script loader.
//!
//! Faithful port of `e.java::void_a(String)` (~line 84) and its data-section
//! sub-parsers `int_a`/`int_b(int)`/`int_c`/`d`/`e`/`f`/`g`/`h`/`i`/`j`.
//!
//! File layout (verified against source + the bytes of `startup.scr`):
//! ```text
//! u8 entry_count
//! entry_count * { u8 id, u8 off_hi, u8 off_lo }      // entry -> absolute offset
//! sections: repeated { u8 tag==30, u8 subtype, <records...>, u8 31 }
//! u8 (non-30 terminator)  + 2 skipped bytes           // -> code start
//! bytecode (to end of file)
//! ```
//! After the sections, entry offsets are rebased to be relative to the start of
//! the bytecode (`offset -= code_start`), and the trailing bytes become the VM's
//! `code` array. See [`crate::vm`] for execution.
//!
//! Each section is a sequence of `tag`-led records terminated by `31`, decoded
//! into a fixed-width row keyed by tag value (`nArray[tag] = value`). The default
//! row index comes from the record tagged `0`. The per-subtype rules differ in
//! which tags carry 2-/3-byte values, which are inline strings, and crucially
//! **signedness** (some subtypes mask `& 0xFF`, others store the raw signed
//! byte) — all reproduced here exactly. Inline string fields store the running
//! string-pool index (`var_int_d`); subtype 9's tag-20 records and subtype 7
//! write to global lists rather than the row. These value semantics are
//! validated byte-for-byte against the original algorithm by the oracle.

use crate::reader::ParseError;

/// Widths of the fixed-size rows produced by each subtype (index = subtype's
/// `nArray.length` in `e.java`).
const fn row_width(subtype: u8) -> usize {
    match subtype {
        0 => 21, // int_a  -> actors (var_int_arr_arr_a)
        1 => 10, // int_b  -> items  (var_int_arr_arr_d)
        2 => 14, // int_c  -> spells (var_int_arr_arr_e)
        4 => 8,  // d      -> var_int_arr_arr_c
        5 => 15, // e      -> var_int_arr_arr_h (+ i/j aux lists)
        6 => 7,  // f      -> var_int_arr_arr_g
        8 => 15, // h      -> k
        9 => 21, // i      -> var_int_arr_arr_f (+ global aux)
        10 => 4, // j      -> l
        _ => 0,  // g (7) has no indexed row
    }
}

/// One parsed data section: a fixed-width value row plus optional aux lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrSection {
    /// Section subtype byte that followed the `30` tag.
    pub subtype: u8,
    /// Target table index (`n2`, taken from the record tagged `0`).
    pub index: usize,
    /// Tag-keyed value row (`nArray`); empty for subtype 7 (global only).
    pub fields: Vec<i32>,
    /// Auxiliary list A: subtype 5 -> `i[]` frame list (term. -1); subtype 7 ->
    /// the flat global list (term. -1); subtype 9 -> tag-20 global values.
    pub aux_a: Vec<i32>,
    /// Auxiliary list B: subtype 5 -> `j[]` frame list (term. -1); else empty.
    pub aux_b: Vec<i32>,
}

/// A loaded script: entry points + bytecode, ready for the VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrProgram {
    /// Number of entry-table records.
    pub entry_count: usize,
    /// `entry_offsets[id]` is the code-relative offset of entry `id`, or 0 if
    /// unused (matches the Java `var_int_arr_d` default).
    pub entry_offsets: Vec<i32>,
    /// Absolute file offset where the bytecode begins.
    pub code_start: usize,
    /// The bytecode (`var_byte_arr_a`).
    pub code: Vec<u8>,
    /// Parsed sections, in file order.
    pub sections: Vec<ScrSection>,
    /// Number of inline strings consumed by the sections (`var_int_d`).
    pub string_count: usize,
    /// Global slot counter advanced by subtype-9 tag-20 records (`var_int_e`).
    pub global_e_count: usize,
}

impl ScrProgram {
    /// Offset of an entry within [`Self::code`], or `None` if out of range. Note
    /// that 0 is a valid offset (an unused entry is also 0; the game never calls
    /// unused entries — see [`ScriptVm::start_entry`](crate::vm::ScriptVm::start_entry)).
    pub fn entry(&self, id: u8) -> Option<usize> {
        let off = *self.entry_offsets.get(id as usize)?;
        usize::try_from(off).ok().filter(|&o| o <= self.code.len())
    }
}

fn at(b: &[u8], n: usize) -> Result<u8, ParseError> {
    b.get(n).copied().ok_or(ParseError::Eof)
}

/// Unsigned byte (`b & 0xFF`).
fn u8m(b: &[u8], n: usize) -> Result<i32, ParseError> {
    Ok(i32::from(at(b, n)?))
}

/// Raw signed byte.
fn i8r(b: &[u8], n: usize) -> Result<i32, ParseError> {
    Ok(i32::from(at(b, n)? as i8))
}

fn be16(b: &[u8], n: usize) -> Result<i32, ParseError> {
    Ok((i32::from(at(b, n)?) << 8) | i32::from(at(b, n + 1)?))
}

fn be24(b: &[u8], n: usize) -> Result<i32, ParseError> {
    Ok((i32::from(at(b, n)?) << 16) | (i32::from(at(b, n + 1)?) << 8) | i32::from(at(b, n + 2)?))
}

/// Set `row[tag] = v`, bounds-checked (an out-of-range tag would be an
/// `ArrayIndexOutOfBounds` in the original; we surface it as `Eof` for fuzzing).
fn set(row: &mut [i32], tag: usize, v: i32) -> Result<(), ParseError> {
    *row.get_mut(tag).ok_or(ParseError::Eof)? = v;
    Ok(())
}

/// Loader scratch state shared by the section sub-parsers.
struct Loader {
    string_count: usize, // var_int_d
    global_e: usize,     // var_int_e
    sections: Vec<ScrSection>,
}

impl Loader {
    /// Consume an inline-string field: bump the string pool and return the index
    /// the original would store (`var_int_d++`).
    fn take_string(&mut self) -> i32 {
        let id = self.string_count as i32;
        self.string_count += 1;
        id
    }
}

/// Generic indexed-row sub-parser covering subtypes 0/1/2/4/6/8/10. The closures
/// of differences (string tag, 0xF0 packing, which tags are 2-/3-byte, flag
/// tags, and signedness of the default 1-byte value) are passed in explicitly,
/// matching each `e.java` sub-parser's grammar.
struct RowRules {
    subtype: u8,
    string_tag: Option<u8>,
    string_hi_is_value: bool,
    wide2: &'static [u8],
    wide3: &'static [u8],
    flag_tags: &'static [u8],
    /// True if the default 1-byte value is masked (`& 0xFF`); false = raw signed.
    masked_default: bool,
}

impl Loader {
    fn walk_row(&mut self, b: &[u8], mut n: usize, r: &RowRules) -> Result<usize, ParseError> {
        let mut row = vec![0i32; row_width(r.subtype)];
        let mut index = 0usize;
        while at(b, n)? != 31 {
            let tag = at(b, n)?;
            let tu = usize::from(tag);
            if Some(tag) == r.string_tag {
                let len_byte = at(b, n + 1)?;
                if r.string_hi_is_value && (len_byte & 0xF0) == 0xF0 {
                    set(&mut row, tu, be16(b, n + 1)?)?;
                    n += 2;
                } else {
                    let s = self.take_string();
                    set(&mut row, tu, s)?;
                    n += usize::from(len_byte) + 1;
                }
            } else if r.wide3.contains(&tag) {
                set(&mut row, tu, be24(b, n + 1)?)?;
                n += 3;
            } else if r.wide2.contains(&tag) {
                set(&mut row, tu, be16(b, n + 1)?)?;
                n += 2;
            } else if r.flag_tags.contains(&tag) {
                set(&mut row, tu, 1)?;
            } else {
                if tag == 0 {
                    index = usize::from(at(b, n + 1)?);
                }
                let v = if r.masked_default {
                    u8m(b, n + 1)?
                } else {
                    i8r(b, n + 1)?
                };
                set(&mut row, tu, v)?;
                n += 1;
            }
            n += 1;
        }
        self.sections.push(ScrSection {
            subtype: r.subtype,
            index,
            fields: row,
            aux_a: Vec::new(),
            aux_b: Vec::new(),
        });
        Ok(n + 1)
    }
}

/// `e.java::e` (subtype 5): row (`var_int_arr_arr_h`) plus two frame lists
/// `i[]` (tag 2) and `j[]` (tag 3), each terminated with -1.
fn walk_e(ld: &mut Loader, b: &[u8], mut n: usize) -> Result<usize, ParseError> {
    let mut row = vec![0i32; row_width(5)];
    let mut list_i = Vec::new();
    let mut list_j = Vec::new();
    let mut index = 0usize;
    while at(b, n)? != 31 {
        let tag = at(b, n)?;
        let tu = usize::from(tag);
        if tag == 1 {
            let len_byte = at(b, n + 1)?;
            if (len_byte & 0xF0) == 0xF0 {
                set(&mut row, 1, be16(b, n + 1)?)?;
                n += 2;
            } else {
                let s = ld.take_string();
                set(&mut row, 1, s)?;
                n += usize::from(len_byte) + 1;
            }
        } else if tag == 6 || tag == 13 || tag == 14 {
            set(&mut row, tu, be16(b, n + 1)?)?;
            n += 2;
        } else if tag == 2 {
            list_i.push(i8r(b, n + 1)?);
            n += 1;
        } else if tag == 3 {
            list_j.push(i8r(b, n + 1)?);
            n += 1;
        } else {
            if tag == 0 {
                index = usize::from(at(b, n + 1)?);
            }
            set(&mut row, tu, i8r(b, n + 1)?)?;
            n += 1;
        }
        n += 1;
    }
    list_i.push(-1);
    list_j.push(-1);
    ld.sections.push(ScrSection {
        subtype: 5,
        index,
        fields: row,
        aux_a: list_i,
        aux_b: list_j,
    });
    Ok(n + 1)
}

/// `e.java::g` (subtype 7): a flat global list of byte pairs (raw signed),
/// terminated with -1. No indexed row.
fn walk_g(ld: &mut Loader, b: &[u8], mut n: usize) -> Result<usize, ParseError> {
    let mut list = Vec::new();
    while at(b, n)? != 31 {
        list.push(i8r(b, n)?);
        list.push(i8r(b, n + 1)?);
        n += 2;
    }
    list.push(-1);
    ld.sections.push(ScrSection {
        subtype: 7,
        index: 0,
        fields: Vec::new(),
        aux_a: list,
        aux_b: Vec::new(),
    });
    Ok(n + 1)
}

/// `e.java::i` (subtype 9): row (`var_int_arr_arr_f`); tags 1/2 are 2-byte, tag
/// 20 appends a masked byte to the global slot list (`var_int_arr_g`/`var_int_e`),
/// captured in `aux_a`; other tags are masked 1-byte values.
fn walk_i(ld: &mut Loader, b: &[u8], mut n: usize) -> Result<usize, ParseError> {
    let mut row = vec![0i32; row_width(9)];
    let mut globals = Vec::new();
    let mut index = 0usize;
    while at(b, n)? != 31 {
        let tag = at(b, n)?;
        let tu = usize::from(tag);
        if tag == 1 || tag == 2 {
            set(&mut row, tu, be16(b, n + 1)?)?;
            n += 2;
        } else if tag == 20 {
            globals.push(u8m(b, n + 1)?);
            ld.global_e += 1;
            n += 1;
        } else {
            if tag == 0 {
                index = usize::from(at(b, n + 1)?);
            }
            set(&mut row, tu, u8m(b, n + 1)?)?;
            n += 1;
        }
        n += 1;
    }
    ld.sections.push(ScrSection {
        subtype: 9,
        index,
        fields: row,
        aux_a: globals,
        aux_b: Vec::new(),
    });
    Ok(n + 1)
}

/// Parse a `.scr` file into a [`ScrProgram`]. Returns [`ParseError::Eof`] on
/// truncated/malformed input rather than panicking (safe to fuzz).
pub fn parse_scr(bytes: &[u8]) -> Result<ScrProgram, ParseError> {
    let entry_count = usize::from(at(bytes, 0)?);
    let mut entry_offsets = vec![0i32; 256];

    // Entry table: records of (id, off_hi, off_lo) starting at index 1.
    let mut n = 1usize;
    while n < entry_count.saturating_mul(3) {
        let id = usize::from(at(bytes, n)?);
        entry_offsets[id] = be16(bytes, n + 1)?;
        n += 3;
    }

    // Data sections: `while (bytes[n++] == 30) { dispatch on subtype }`.
    let mut ld = Loader {
        string_count: 0,
        global_e: 0,
        sections: Vec::new(),
    };
    loop {
        let tag = at(bytes, n)?;
        n += 1;
        if tag != 30 {
            break;
        }
        let subtype = at(bytes, n)?;
        n += 1;
        n = match subtype {
            0 => ld.walk_row(
                bytes,
                n,
                &RowRules {
                    subtype: 0,
                    string_tag: Some(1),
                    string_hi_is_value: false,
                    wide2: &[7, 14, 15],
                    wide3: &[],
                    flag_tags: &[],
                    masked_default: true,
                },
            )?,
            1 => ld.walk_row(
                bytes,
                n,
                &RowRules {
                    subtype: 1,
                    string_tag: Some(1),
                    string_hi_is_value: true,
                    wide2: &[9],
                    wide3: &[5],
                    flag_tags: &[6],
                    masked_default: false,
                },
            )?,
            2 => ld.walk_row(
                bytes,
                n,
                &RowRules {
                    subtype: 2,
                    string_tag: Some(1),
                    string_hi_is_value: true,
                    wide2: &[13],
                    wide3: &[5],
                    flag_tags: &[4],
                    masked_default: true,
                },
            )?,
            4 => ld.walk_row(
                bytes,
                n,
                &RowRules {
                    subtype: 4,
                    string_tag: Some(1),
                    string_hi_is_value: true,
                    wide2: &[7],
                    wide3: &[],
                    flag_tags: &[],
                    masked_default: false,
                },
            )?,
            5 => walk_e(&mut ld, bytes, n)?,
            6 => ld.walk_row(
                bytes,
                n,
                &RowRules {
                    subtype: 6,
                    string_tag: None,
                    string_hi_is_value: false,
                    wide2: &[2],
                    wide3: &[],
                    flag_tags: &[],
                    masked_default: true,
                },
            )?,
            7 => walk_g(&mut ld, bytes, n)?,
            8 => ld.walk_row(
                bytes,
                n,
                &RowRules {
                    subtype: 8,
                    string_tag: Some(1),
                    string_hi_is_value: true,
                    wide2: &[14],
                    wide3: &[6],
                    flag_tags: &[],
                    masked_default: false,
                },
            )?,
            9 => walk_i(&mut ld, bytes, n)?,
            10 => ld.walk_row(
                bytes,
                n,
                &RowRules {
                    subtype: 10,
                    string_tag: None,
                    string_hi_is_value: false,
                    wide2: &[],
                    wide3: &[],
                    flag_tags: &[],
                    masked_default: true,
                },
            )?,
            // Unknown subtype: Java leaves n at the subtype byte and the while
            // loop's `bytes[n++]` consumes it and exits.
            _ => break,
        };
    }

    // Three bytes between the last section and the code (the non-30 terminator
    // already consumed above, then two skipped).
    n += 2;
    let code_start = n;
    if code_start > bytes.len() {
        return Err(ParseError::Eof);
    }

    // Rebase entry offsets to be relative to the bytecode.
    for off in entry_offsets.iter_mut() {
        if *off != 0 {
            *off -= code_start as i32;
        }
    }

    let code = bytes[code_start..].to_vec();
    Ok(ScrProgram {
        entry_count,
        entry_offsets,
        code_start,
        code,
        sections: ld.sections,
        string_count: ld.string_count,
        global_e_count: ld.global_e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_is_error_not_panic() {
        assert_eq!(parse_scr(&[]).err(), Some(ParseError::Eof));
        assert_eq!(parse_scr(&[5]).err(), Some(ParseError::Eof)); // claims 5 entries
    }

    #[test]
    fn no_sections_minimal_program() {
        // count=0, then a non-30 byte (=0) ends the (empty) section loop; +2.
        let data = [0u8, 0, 1, 2, 99, 98];
        let p = parse_scr(&data).unwrap();
        assert_eq!(p.entry_count, 0);
        assert_eq!(p.code_start, 4);
        assert_eq!(p.code, vec![99, 98]);
    }

    #[test]
    fn item_section_captures_values() {
        // count=0; one section: 30, subtype 1 (int_b), records:
        //   tag 0 val 3  -> index = 3, row[0] = 3 (raw)
        //   tag 4 val 7  -> row[4] = 7
        //   tag 9 hi/lo 0x12 0x34 -> row[9] = 0x1234
        //   tag 6 (flag) -> row[6] = 1
        //   31 terminator
        // then non-30 terminator + 2 -> code.
        let data = [
            0u8, // entry_count
            30, 1, // section tag + subtype 1
            0, 3, // tag 0 (index/default) value 3
            4, 7, // tag 4 value 7
            9, 0x12, 0x34, // tag 9 = 0x1234
            6,    // tag 6 flag
            31,   // end section
            0,    // non-30 terminator
            0xAA, 0xBB, // +2 skipped
        ];
        let p = parse_scr(&data).unwrap();
        assert_eq!(p.sections.len(), 1);
        let s = &p.sections[0];
        assert_eq!(s.subtype, 1);
        assert_eq!(s.index, 3);
        assert_eq!(s.fields.len(), 10);
        assert_eq!(s.fields[0], 3);
        assert_eq!(s.fields[4], 7);
        assert_eq!(s.fields[9], 0x1234);
        assert_eq!(s.fields[6], 1);
    }
}
