//! `ESO` save-game (de)serialization.
//!
//! Faithful port of the save format written by `b.java::g()` and read by
//! `b.java::b(boolean)`, plus the actor blob written by `h.java::a(j,
//! ByteArrayOutputStream)` (~line 1812) and read by `h.java::a(byte[], int)`
//! (~line 40). The `RecordStore` is named `"ESO"`, record 1.
//!
//! Top-level layout:
//! ```text
//! 3 bytes : var_byte_arr_f KEY BINDINGS (the quick-key table Custom
//!           Controls edits — not progress flags), defaults {55,57,51}
//!           = keys '7','9','3'
//! u8      : bool_o flag
//! u8      : player present? (0/1)
//! if present:
//!   u8         : player-name length
//!   name bytes : player name (var_java_lang_String_c)
//!   actor blob : see [`SaveActor`]
//! ```
//!
//! Actor blob (`h.a`): a fixed header of byte/short/int fields (big-endian; some
//! `byte` fields are written as two bytes — the high byte is the sign
//! extension), then the model name, then `count` item records of two bytes each
//! (`(active<<7 | id_hi), id_lo`). The item `active` bit is recomputed from
//! actor state on write, so the item list is preserved here as raw byte pairs
//! (the format/structure is the save concern; recomputing the bit is combat
//! logic, M8). Replicating the original's exact reads and writes makes a
//! game-written blob round-trip byte-for-byte.

use crate::reader::ParseError;

/// The actor (player) portion of a save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveActor {
    pub var_byte_c: i8,
    pub var_byte_f: i8,
    pub var_int_b: i32, // 24-bit
    pub var_byte_o: i8, // written as 2 bytes
    pub var_short_s: i16,
    pub var_short_t: i16,
    pub var_short_v: i16,
    pub var_short_w: i16,
    pub var_short_x: i16,
    pub var_short_u: i16,
    pub var_byte_j: i8, // written as 2 bytes
    pub e: i16,
    pub f: i16,
    pub var_byte_r: i8,
    pub global_int_b: u16, // b.var_int_b, written as 2 bytes
    pub model_name: Vec<u8>,
    /// Item/spell records as raw `(byte0, byte1)` pairs (byte0 = active<<7 | hi).
    pub items: Vec<(u8, u8)>,
}

/// The player portion (present only after a game has started).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavePlayer {
    pub name: Vec<u8>,
    pub actor: SaveActor,
}

/// A parsed `ESO` save record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Save {
    pub flags: [u8; 3],
    pub bool_o: u8,
    pub player: Option<SavePlayer>,
}

struct Rd<'a> {
    b: &'a [u8],
    n: usize,
}

impl<'a> Rd<'a> {
    fn u8(&mut self) -> Result<u8, ParseError> {
        let v = *self.b.get(self.n).ok_or(ParseError::Eof)?;
        self.n += 1;
        Ok(v)
    }
    fn i8(&mut self) -> Result<i8, ParseError> {
        Ok(self.u8()? as i8)
    }
    fn be16(&mut self) -> Result<u16, ParseError> {
        let hi = u16::from(self.u8()?);
        let lo = u16::from(self.u8()?);
        Ok((hi << 8) | lo)
    }
    fn be24(&mut self) -> Result<i32, ParseError> {
        let a = i32::from(self.u8()?);
        let b = i32::from(self.u8()?);
        let c = i32::from(self.u8()?);
        Ok((a << 16) | (b << 8) | c)
    }
    fn bytes(&mut self, len: usize) -> Result<Vec<u8>, ParseError> {
        let end = self.n.checked_add(len).ok_or(ParseError::Eof)?;
        let s = self.b.get(self.n..end).ok_or(ParseError::Eof)?.to_vec();
        self.n = end;
        Ok(s)
    }
}

fn parse_actor(r: &mut Rd<'_>) -> Result<SaveActor, ParseError> {
    let var_byte_c = r.i8()?;
    let var_byte_f = r.i8()?;
    let var_int_b = r.be24()?;
    let var_byte_o = r.be16()? as i8; // (byte) of 16-bit == low byte signed
    let var_short_s = r.be16()? as i16;
    let var_short_t = r.be16()? as i16;
    let var_short_v = r.be16()? as i16;
    let var_short_w = r.be16()? as i16;
    let var_short_x = r.be16()? as i16;
    let var_short_u = r.be16()? as i16;
    let var_byte_j = r.be16()? as i8;
    let e = r.be16()? as i16;
    let f = r.be16()? as i16;
    let var_byte_r = r.i8()?;
    let global_int_b = r.be16()?;
    let name_len = usize::from(r.u8()?);
    let model_name = r.bytes(name_len)?;
    let count = usize::from(r.u8()?);
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        let b0 = r.u8()?;
        let b1 = r.u8()?;
        items.push((b0, b1));
    }
    Ok(SaveActor {
        var_byte_c,
        var_byte_f,
        var_int_b,
        var_byte_o,
        var_short_s,
        var_short_t,
        var_short_v,
        var_short_w,
        var_short_x,
        var_short_u,
        var_byte_j,
        e,
        f,
        var_byte_r,
        global_int_b,
        model_name,
        items,
    })
}

/// Parse an `ESO` save record. Returns [`ParseError::Eof`] on truncated input.
pub fn parse_save(bytes: &[u8]) -> Result<Save, ParseError> {
    let mut r = Rd { b: bytes, n: 0 };
    let flags = [r.u8()?, r.u8()?, r.u8()?];
    let bool_o = r.u8()?;
    let present = r.u8()?;
    let player = if present == 1 {
        let name_len = usize::from(r.u8()?);
        let name = r.bytes(name_len)?;
        let actor = parse_actor(&mut r)?;
        Some(SavePlayer { name, actor })
    } else {
        None
    };
    Ok(Save {
        flags,
        bool_o,
        player,
    })
}

/// A byte that a `byte` field is serialized into two of: high byte is the sign
/// extension, low byte is the value — exactly Java `(byte)(v>>8), (byte)(v>>0)`.
fn push_be_of_byte(out: &mut Vec<u8>, v: i8) {
    let x = i32::from(v);
    out.push(((x >> 8) & 0xFF) as u8);
    out.push((x & 0xFF) as u8);
}

fn push_be16(out: &mut Vec<u8>, v: i16) {
    let x = v as u16;
    out.push((x >> 8) as u8);
    out.push((x & 0xFF) as u8);
}

fn serialize_actor(out: &mut Vec<u8>, a: &SaveActor) {
    out.push(a.var_byte_c as u8);
    out.push(a.var_byte_f as u8);
    out.push(((a.var_int_b >> 16) & 0xFF) as u8);
    out.push(((a.var_int_b >> 8) & 0xFF) as u8);
    out.push((a.var_int_b & 0xFF) as u8);
    push_be_of_byte(out, a.var_byte_o);
    push_be16(out, a.var_short_s);
    push_be16(out, a.var_short_t);
    push_be16(out, a.var_short_v);
    push_be16(out, a.var_short_w);
    push_be16(out, a.var_short_x);
    push_be16(out, a.var_short_u);
    push_be_of_byte(out, a.var_byte_j);
    push_be16(out, a.e);
    push_be16(out, a.f);
    out.push(a.var_byte_r as u8);
    out.push((a.global_int_b >> 8) as u8);
    out.push((a.global_int_b & 0xFF) as u8);
    out.push(a.model_name.len() as u8);
    out.extend_from_slice(&a.model_name);
    out.push(a.items.len() as u8);
    for (b0, b1) in &a.items {
        out.push(*b0);
        out.push(*b1);
    }
}

/// Serialize a [`Save`] back to the `ESO` record bytes. For a save the original
/// game wrote, this reproduces the bytes exactly.
pub fn serialize_save(save: &Save) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&save.flags);
    out.push(save.bool_o);
    match &save.player {
        None => out.push(0),
        Some(p) => {
            out.push(1);
            out.push(p.name.len() as u8);
            out.extend_from_slice(&p.name);
            serialize_actor(&mut out, &p.actor);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_actor() -> SaveActor {
        SaveActor {
            var_byte_c: 5,
            var_byte_f: -2, // exercises the sign-extension two-byte write
            var_int_b: 0x01_23_45,
            var_byte_o: -1,
            var_short_s: 1000,
            var_short_t: 2000,
            var_short_v: 3000,
            var_short_w: 4000,
            var_short_x: -5,
            var_short_u: 6,
            var_byte_j: 7,
            e: 8,
            f: 9,
            var_byte_r: 10,
            global_int_b: 0xBEEF,
            model_name: b"/oh_pc.cml".to_vec(),
            items: vec![(0x82, 0x10), (0x01, 0x20)],
        }
    }

    #[test]
    fn round_trips_no_player() {
        let save = Save {
            flags: [55, 57, 51],
            bool_o: 1,
            player: None,
        };
        let bytes = serialize_save(&save);
        assert_eq!(parse_save(&bytes).unwrap(), save);
        // exact bytes: 3 flags, bool_o, present=0
        assert_eq!(bytes, vec![55, 57, 51, 1, 0]);
    }

    #[test]
    fn round_trips_with_player() {
        let save = Save {
            flags: [55, 57, 51],
            bool_o: 0,
            player: Some(SavePlayer {
                name: b"Champion".to_vec(),
                actor: sample_actor(),
            }),
        };
        let bytes = serialize_save(&save);
        let parsed = parse_save(&bytes).unwrap();
        assert_eq!(parsed, save);
        // serialize -> parse -> serialize is stable
        assert_eq!(serialize_save(&parsed), bytes);
    }

    #[test]
    fn byte_field_written_as_two_bytes_with_sign_extension() {
        let save = Save {
            flags: [0, 0, 0],
            bool_o: 0,
            player: Some(SavePlayer {
                name: vec![],
                actor: sample_actor(),
            }),
        };
        let bytes = serialize_save(&save);
        // layout from index 5:
        //  5 nameLen=0
        //  6 var_byte_c = 5
        //  7 var_byte_f = -2 -> 0xFE (single byte)
        //  8,9,10 var_int_b = 0x01,0x23,0x45
        //  11,12 var_byte_o = -1 -> 0xFF,0xFF (two bytes, sign-extended)
        assert_eq!(&bytes[6..13], &[5, 0xFE, 0x01, 0x23, 0x45, 0xFF, 0xFF]);
    }

    #[test]
    fn truncated_is_error_not_panic() {
        assert_eq!(parse_save(&[]).err(), Some(ParseError::Eof));
        assert_eq!(parse_save(&[55, 57, 51, 0]).err(), Some(ParseError::Eof)); // missing present
        assert_eq!(parse_save(&[55, 57, 51, 0, 1]).err(), Some(ParseError::Eof));
        // present but no name
    }
}
