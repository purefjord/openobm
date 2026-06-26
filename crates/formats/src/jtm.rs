//! `.jtm` tile-map parser.
//!
//! Faithful port of `b.java::void_b(String)` (~line 662), the map loader.
//!
//! Layout (verified against bytecode):
//! ```text
//! u8 width   (var_byte_f)
//! u8 height  (var_byte_g)
//! RLE base layer            -> one array
//! RLE additional layers...  -> appended, read until the file is consumed
//! ```
//!
//! Load-bearing details the Java gets "wrong" by modern conventions, preserved
//! here exactly:
//!  - **Tile storage index is `x * height + y`**, NOT row-major. The fill loops
//!    are `for y in 0..height { for x in 0..width { ... } }` (y outer, x inner).
//!  - **RLE**: sentinel byte `0xFF`, followed by `count` then `value`; the run
//!    writes `count` cells. A `count == 0` run still writes exactly **one** cell
//!    (the `++n7 < n` test fails immediately).
//!  - RLE run state resets at each layer boundary (runs never span layers), but
//!    the byte cursor is continuous across the whole file.
//!
//! The three `-1`-filled side arrays (`var_byte_arr_j/k/c`) in the Java loader
//! are NOT part of the file — they are collision/actor planes initialized in
//! memory — so they are not produced here.

use crate::reader::{ParseError, Reader};

/// A decoded tile map: width/height in tiles plus one or more byte layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JtmMap {
    pub width: usize,
    pub height: usize,
    /// `layers[0]` is the base layer; the rest are overlay layers in file order.
    /// Each layer is `width * height` bytes indexed by `x * height + y`.
    pub layers: Vec<Vec<u8>>,
}

impl JtmMap {
    /// Tile value at `(x, y)` in `layer`, using the original `x*height+y` index.
    /// Returns `None` if any index is out of range.
    pub fn tile(&self, layer: usize, x: usize, y: usize) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.layers.get(layer)?.get(x * self.height + y).copied()
    }
}

/// RLE decoder state, mirroring the `n6`/`n7`/`n`/`n4` locals in `void_b`.
struct RleState {
    cur: i32,     // n6: current opcode byte, -1 == "need to read next"
    run_pos: u32, // n7: cells emitted so far in the current run
    run_len: u32, // n:  run length read after a 0xFF sentinel
    run_val: u8,  // n4: run value read after a 0xFF sentinel
}

impl RleState {
    fn new() -> Self {
        Self {
            cur: -1,
            run_pos: 0,
            run_len: 0,
            run_val: 0,
        }
    }

    /// Emit the value for the next cell, pulling bytes from `r` as needed.
    fn next(&mut self, r: &mut Reader<'_>) -> Result<u8, ParseError> {
        if self.cur == -1 {
            self.cur = i32::from(r.u8()?);
        }
        if self.cur == 255 {
            if self.run_pos == 0 {
                self.run_len = u32::from(r.u8()?);
                self.run_val = r.u8()?;
            }
            self.run_pos += 1;
            if self.run_pos < self.run_len {
                return Ok(self.run_val);
            }
            // Last cell of the run (also the count==0 case: run_pos==1 > 0).
            let v = self.run_val;
            self.cur = -1;
            self.run_pos = 0;
            Ok(v)
        } else {
            let v = self.cur as u8;
            self.cur = -1;
            self.run_pos = 0;
            Ok(v)
        }
    }
}

fn read_layer(r: &mut Reader<'_>, width: usize, height: usize) -> Result<Vec<u8>, ParseError> {
    let cells = width.checked_mul(height).ok_or(ParseError::Eof)?;
    let mut out = vec![0u8; cells];
    let mut rle = RleState::new();
    // y outer, x inner; index x*height + y (matches the Java loops exactly).
    for y in 0..height {
        for x in 0..width {
            out[x * height + y] = rle.next(r)?;
        }
    }
    Ok(out)
}

/// Parse a `.jtm` file into a [`JtmMap`]. Reads additional layers until the
/// input is exhausted, mirroring the Java `while (n5 < fileLength)` loop.
///
/// Returns [`ParseError::Eof`] on truncated/over-long input rather than
/// panicking, so this is safe to fuzz.
pub fn parse_jtm(bytes: &[u8]) -> Result<JtmMap, ParseError> {
    let mut r = Reader::new(bytes);
    let width = usize::from(r.u8()?);
    let height = usize::from(r.u8()?);

    let mut layers = Vec::new();
    layers.push(read_layer(&mut r, width, height)?);

    // A zero-cell layer would consume no bytes and loop forever; the real assets
    // never have width/height == 0, but guard anyway.
    if width != 0 && height != 0 {
        while !r.is_empty() {
            let before = r.pos();
            layers.push(read_layer(&mut r, width, height)?);
            if r.pos() == before {
                break; // no forward progress; bail rather than spin
            }
        }
    }

    Ok(JtmMap {
        width,
        height,
        layers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_run_no_rle() {
        // 2x2, single layer, four literal (non-0xFF) bytes.
        // y outer, x inner: emission order is (x=0,y=0),(x=1,y=0),(x=0,y=1),(x=1,y=1)
        // values 10,11,12,13 -> idx x*h+y: [0]=10,[2]=11,[1]=12,[3]=13
        let data = [2u8, 2, 10, 11, 12, 13];
        let m = parse_jtm(&data).unwrap();
        assert_eq!(m.width, 2);
        assert_eq!(m.height, 2);
        assert_eq!(m.layers.len(), 1);
        assert_eq!(m.tile(0, 0, 0), Some(10));
        assert_eq!(m.tile(0, 1, 0), Some(11));
        assert_eq!(m.tile(0, 0, 1), Some(12));
        assert_eq!(m.tile(0, 1, 1), Some(13));
    }

    #[test]
    fn rle_run_expands_to_count_cells() {
        // 2x2 = 4 cells: 0xFF, count=4, value=7 -> all four cells == 7.
        let data = [2u8, 2, 0xFF, 4, 7];
        let m = parse_jtm(&data).unwrap();
        assert_eq!(m.layers[0], vec![7, 7, 7, 7]);
    }

    #[test]
    fn rle_count_zero_writes_one_cell() {
        // 1x2 = 2 cells. First a count==0 run (writes ONE cell, value 9), then a
        // literal 5 for the remaining cell.
        let data = [1u8, 2, 0xFF, 0, 9, 5];
        let m = parse_jtm(&data).unwrap();
        // emission order (x=0,y=0)=9 then (x=0,y=1)=5; idx x*h+y -> [0]=9,[1]=5
        assert_eq!(m.layers[0], vec![9, 5]);
    }

    #[test]
    fn multiple_layers_until_eof() {
        // 1x1: base literal 1, then a second layer literal 2.
        let data = [1u8, 1, 1, 2];
        let m = parse_jtm(&data).unwrap();
        assert_eq!(m.layers.len(), 2);
        assert_eq!(m.layers[0], vec![1]);
        assert_eq!(m.layers[1], vec![2]);
    }

    #[test]
    fn truncated_input_is_error_not_panic() {
        assert_eq!(parse_jtm(&[]), Err(ParseError::Eof));
        assert_eq!(parse_jtm(&[2, 2, 10]), Err(ParseError::Eof)); // needs 4 cells
                                                                  // 0xFF sentinel with missing count/value
        assert_eq!(parse_jtm(&[1, 1, 0xFF]), Err(ParseError::Eof));
    }

    #[test]
    fn zero_dimension_does_not_loop() {
        // width 0 -> no cells, no infinite layer loop.
        let m = parse_jtm(&[0u8, 5]).unwrap();
        assert_eq!(m.width, 0);
        assert_eq!(m.layers, vec![Vec::<u8>::new()]);
    }
}
