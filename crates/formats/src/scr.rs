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
//! Data-section *values* (actor/item/spell stat tables) are not needed to run a
//! script to its first visible action (M3), so this loader faithfully walks the
//! sections to find `code_start` and records each section's `(subtype, index)`,
//! but defers materializing the stat tables to a later milestone. The structural
//! outputs that the VM depends on — `entry_offsets`, `code_start`, `code` — are
//! validated byte-for-byte against the original algorithm by the oracle.

use crate::reader::ParseError;

/// One parsed data section header (the stat values themselves are deferred).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrSection {
    /// Section subtype byte that followed the `30` tag.
    pub subtype: u8,
    /// Target table index (`n2`, taken from the record tagged `0`).
    pub index: usize,
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
    /// Parsed section headers, in file order.
    pub sections: Vec<ScrSection>,
    /// Number of inline strings consumed by the sections (`var_int_d`).
    pub string_count: usize,
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

/// Bounds-checked byte fetch.
fn at(b: &[u8], n: usize) -> Result<u8, ParseError> {
    b.get(n).copied().ok_or(ParseError::Eof)
}

fn be16(b: &[u8], n: usize) -> Result<i32, ParseError> {
    Ok((i32::from(at(b, n)?) << 8) | i32::from(at(b, n + 1)?))
}

/// Per-subtype record grammar for [`Loader::walk_generic`]: which tag is an
/// inline string, whether a 0xF0 high nibble on its length means "2-byte value
/// instead of string", and which tags carry 2-byte/3-byte/flag (no-value) data.
struct GenericRules {
    string_tag: Option<u8>,
    string_hi_is_value: bool,
    wide2: &'static [u8],
    wide3: &'static [u8],
    flag_tags: &'static [u8],
}

/// Loader scratch state shared by the section sub-parsers.
struct Loader {
    string_count: usize, // var_int_d
    sections: Vec<ScrSection>,
}

impl Loader {
    /// Generic section walker covering the common record grammar of
    /// `int_a/int_b/int_c/d/h/i` (subtypes 0,1,2,4,8,9): records are
    /// `tag` then either an inline string (`tag==1`, unless the high nibble of
    /// the length byte is 0xF0 which means a 2-byte value), a fixed-width value
    /// for `wide`-listed tags, or a 1-byte value; terminated by `31`.
    ///
    /// The per-subtype differences are entirely in *which tags are wide and how
    /// wide*, captured by `wide2`/`wide3` and `string_hi_nibble`. This mirrors
    /// the Java sub-parsers' byte advancement exactly.
    fn walk_generic(
        &mut self,
        b: &[u8],
        mut n: usize,
        subtype: u8,
        rules: &GenericRules,
    ) -> Result<usize, ParseError> {
        let mut index = 0usize;
        while at(b, n)? != 31 {
            let tag = at(b, n)?;
            if Some(tag) == rules.string_tag {
                let len_byte = at(b, n + 1)?;
                if rules.string_hi_is_value && (len_byte & 0xF0) == 0xF0 {
                    // packed 2-byte value instead of a string
                    n += 2;
                } else {
                    // inline string of `len_byte` bytes
                    self.string_count += 1;
                    n += usize::from(len_byte) + 1;
                }
            } else if rules.wide3.contains(&tag) {
                n += 3;
            } else if rules.wide2.contains(&tag) {
                n += 2;
            } else if rules.flag_tags.contains(&tag) {
                // a bare flag: no value byte
            } else {
                if tag == 0 {
                    index = usize::from(at(b, n + 1)?);
                }
                n += 1; // consume value byte
            }
            n += 1;
        }
        self.sections.push(ScrSection { subtype, index });
        Ok(n + 1)
    }
}

/// `e.java::e` (subtype 5): like the generic walk but also has `2`/`3` tags that
/// each consume one extra byte (animation frame lists).
fn walk_e(ld: &mut Loader, b: &[u8], mut n: usize) -> Result<usize, ParseError> {
    let mut index = 0usize;
    while at(b, n)? != 31 {
        let tag = at(b, n)?;
        if tag == 1 {
            let len_byte = at(b, n + 1)?;
            if (len_byte & 0xF0) == 0xF0 {
                n += 2;
            } else {
                ld.string_count += 1;
                n += usize::from(len_byte) + 1;
            }
        } else if tag == 6 || tag == 13 || tag == 14 {
            n += 2;
        } else if tag == 2 || tag == 3 {
            n += 1; // nArray2/3[..] = b[++n]
        } else {
            if tag == 0 {
                index = usize::from(at(b, n + 1)?);
            }
            n += 1;
        }
        n += 1;
    }
    ld.sections.push(ScrSection { subtype: 5, index });
    Ok(n + 1)
}

/// `e.java::f` (subtype 6): only tag `2` is a 2-byte value; everything else is a
/// 1-byte value; no strings.
fn walk_f(ld: &mut Loader, b: &[u8], mut n: usize) -> Result<usize, ParseError> {
    let mut index = 0usize;
    while at(b, n)? != 31 {
        let tag = at(b, n)?;
        if tag == 2 {
            n += 2;
        } else {
            if tag == 0 {
                index = usize::from(at(b, n + 1)?);
            }
            n += 1;
        }
        n += 1;
    }
    ld.sections.push(ScrSection { subtype: 6, index });
    Ok(n + 1)
}

/// `e.java::g` (subtype 7): a flat list of byte pairs until `31`.
fn walk_g(ld: &mut Loader, b: &[u8], mut n: usize) -> Result<usize, ParseError> {
    while at(b, n)? != 31 {
        n += 1; // var_int_arr_f[..] = b[n++]
        n += 1; // var_int_arr_f[..] = b[n++]
    }
    ld.sections.push(ScrSection {
        subtype: 7,
        index: 0,
    });
    Ok(n + 1)
}

/// `e.java::j` (subtype 10): records of `tag,value` (1-byte values), no strings.
fn walk_j(ld: &mut Loader, b: &[u8], mut n: usize) -> Result<usize, ParseError> {
    let mut index = 0usize;
    while at(b, n)? != 31 {
        let tag = at(b, n)?;
        if tag == 0 {
            index = usize::from(at(b, n + 1)?);
        }
        n += 2;
    }
    ld.sections.push(ScrSection { subtype: 10, index });
    Ok(n + 1)
}

/// `e.java::i` (subtype 9): tags 1/2 are 2-byte values, tag 20 consumes one
/// extra byte into the global slot list, otherwise 1-byte value.
fn walk_i(ld: &mut Loader, b: &[u8], mut n: usize) -> Result<usize, ParseError> {
    let mut index = 0usize;
    while at(b, n)? != 31 {
        let tag = at(b, n)?;
        if tag == 1 || tag == 2 {
            n += 2;
        } else if tag == 20 {
            n += 1; // var_int_arr_g[var_int_e++] = b[++n]
        } else {
            if tag == 0 {
                index = usize::from(at(b, n + 1)?);
            }
            n += 1;
        }
        n += 1;
    }
    ld.sections.push(ScrSection { subtype: 9, index });
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
            // int_a: wide tags 7/14/15 (2-byte), string tag 1 (plain string).
            0 => ld.walk_generic(
                bytes,
                n,
                0,
                &GenericRules {
                    string_tag: Some(1),
                    string_hi_is_value: false,
                    wide2: &[7, 14, 15],
                    wide3: &[],
                    flag_tags: &[],
                },
            )?,
            // int_b: string tag 1 (0xF0 => 2-byte), tag 5 (3-byte), 9 (2-byte), 6 flag.
            1 => ld.walk_generic(
                bytes,
                n,
                1,
                &GenericRules {
                    string_tag: Some(1),
                    string_hi_is_value: true,
                    wide2: &[9],
                    wide3: &[5],
                    flag_tags: &[6],
                },
            )?,
            // int_c: string tag 1 (0xF0 => 2-byte), 5 (3-byte), 13 (2-byte), 4 flag.
            2 => ld.walk_generic(
                bytes,
                n,
                2,
                &GenericRules {
                    string_tag: Some(1),
                    string_hi_is_value: true,
                    wide2: &[13],
                    wide3: &[5],
                    flag_tags: &[4],
                },
            )?,
            // d: string tag 1 (0xF0 => 2-byte), 7 (2-byte).
            4 => ld.walk_generic(
                bytes,
                n,
                4,
                &GenericRules {
                    string_tag: Some(1),
                    string_hi_is_value: true,
                    wide2: &[7],
                    wide3: &[],
                    flag_tags: &[],
                },
            )?,
            5 => walk_e(&mut ld, bytes, n)?,
            6 => walk_f(&mut ld, bytes, n)?,
            7 => walk_g(&mut ld, bytes, n)?,
            // h: string tag 1 (0xF0 => 2-byte), 14 (2-byte), 6 (3-byte).
            8 => ld.walk_generic(
                bytes,
                n,
                8,
                &GenericRules {
                    string_tag: Some(1),
                    string_hi_is_value: true,
                    wide2: &[14],
                    wide3: &[6],
                    flag_tags: &[],
                },
            )?,
            9 => walk_i(&mut ld, bytes, n)?,
            10 => walk_j(&mut ld, bytes, n)?,
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
        // bytes: [0][0][a][b][code...]
        let data = [0u8, 0, 1, 2, 99, 98];
        let p = parse_scr(&data).unwrap();
        assert_eq!(p.entry_count, 0);
        // n: count==0 so entry loop skipped, n=1. loop: tag=bytes[1]=0 (!=30) ->
        // n=2, break. n+=2 -> 4. code_start=4, code=[99,98].
        assert_eq!(p.code_start, 4);
        assert_eq!(p.code, vec![99, 98]);
    }
}
