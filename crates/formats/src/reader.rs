//! Centralized big-endian byte reader.
//!
//! Java ME source reads unsigned bytes with the idiom `(char)(b[n] & 0xFF)` and
//! multi-byte values big-endian. To avoid scattering signedness bugs across the
//! port (a documented trap in GOAL.md section 5), *every* raw-byte read in this
//! crate goes through this one reader. See `spec.txt` for the original rationale.

/// Error returned when a read runs past the end of the backing slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// Attempted to read beyond the end of the input.
    Eof,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ParseError::Eof => write!(f, "unexpected end of input"),
        }
    }
}

impl std::error::Error for ParseError {}

/// A cursor over a byte slice exposing the exact integer reads the Java code uses.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Wraps `data` with the cursor positioned at the start.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Current read offset.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Number of unread bytes.
    pub fn len(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// True when no unread bytes remain.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Reads one unsigned byte (`(b & 0xFF)` in Java).
    pub fn u8(&mut self) -> Result<u8, ParseError> {
        let b = *self.data.get(self.pos).ok_or(ParseError::Eof)?;
        self.pos += 1;
        Ok(b)
    }

    /// Reads one signed byte (Java raw `byte`).
    pub fn i8(&mut self) -> Result<i8, ParseError> {
        Ok(self.u8()? as i8)
    }

    /// Reads a big-endian unsigned 16-bit value.
    pub fn u16_be(&mut self) -> Result<u16, ParseError> {
        let hi = u16::from(self.u8()?);
        let lo = u16::from(self.u8()?);
        Ok((hi << 8) | lo)
    }

    /// Reads a big-endian signed 16-bit value.
    pub fn i16_be(&mut self) -> Result<i16, ParseError> {
        Ok(self.u16_be()? as i16)
    }

    /// Borrows the next `n` bytes, advancing the cursor.
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        let end = self.pos.checked_add(n).ok_or(ParseError::Eof)?;
        let out = self.data.get(self.pos..end).ok_or(ParseError::Eof)?;
        self.pos = end;
        Ok(out)
    }

    /// All remaining unread bytes.
    pub fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos.min(self.data.len())..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_unsigned_and_signed_bytes() {
        let mut r = Reader::new(&[0x00, 0xFF, 0x80]);
        assert_eq!(r.u8(), Ok(0x00));
        assert_eq!(r.u8(), Ok(0xFF));
        assert_eq!(r.i8(), Ok(-128));
        assert_eq!(r.u8(), Err(ParseError::Eof));
    }

    #[test]
    fn reads_big_endian_words() {
        let mut r = Reader::new(&[0x12, 0x34, 0xFF, 0xFE]);
        assert_eq!(r.u16_be(), Ok(0x1234));
        assert_eq!(r.i16_be(), Ok(-2));
    }

    #[test]
    fn bytes_and_remaining_track_position() {
        let mut r = Reader::new(&[1, 2, 3, 4, 5]);
        assert_eq!(r.bytes(2), Ok(&[1, 2][..]));
        assert_eq!(r.pos(), 2);
        assert_eq!(r.remaining(), &[3, 4, 5]);
        assert_eq!(r.bytes(99), Err(ParseError::Eof));
    }
}
