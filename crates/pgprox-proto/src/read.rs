//! Bounds-checked reads over a message body.
//!
//! Every read is checked and returns an error rather than panicking, because
//! these bytes come from the network. A malformed message must not take down a
//! node serving 100k other connections.

/// Why a field could not be read from a message body.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FieldError {
    /// The body ended before the field did.
    #[error("message body ended after {read} bytes while reading {what}")]
    Truncated {
        /// What was being read.
        what: &'static str,
        /// How far the cursor had got.
        read: usize,
    },
    /// A null-terminated string had no terminator.
    #[error("unterminated string in message body")]
    Unterminated,
    /// A string field was not valid UTF-8.
    ///
    /// Postgres sends text in the connection encoding, which for everything
    /// this proxy inspects is ASCII or UTF-8.
    #[error("string field {what} is not valid UTF-8")]
    NotUtf8 {
        /// Which field.
        what: &'static str,
    },
}

/// A cursor over a message body.
#[derive(Clone, Copy, Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Starts reading at the beginning of `buf`.
    #[must_use]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Bytes not yet consumed.
    #[must_use]
    pub fn remaining(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }

    /// Whether every byte has been consumed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }

    /// Reads one byte.
    ///
    /// # Errors
    ///
    /// Fails if the body has ended.
    pub fn u8(&mut self, what: &'static str) -> Result<u8, FieldError> {
        let byte = *self.buf.get(self.pos).ok_or(FieldError::Truncated {
            what,
            read: self.pos,
        })?;
        self.pos += 1;
        Ok(byte)
    }

    /// Reads a big-endian `i32`.
    ///
    /// # Errors
    ///
    /// Fails if fewer than four bytes remain.
    pub fn i32(&mut self, what: &'static str) -> Result<i32, FieldError> {
        let end = self.pos + 4;
        let slice = self.buf.get(self.pos..end).ok_or(FieldError::Truncated {
            what,
            read: self.pos,
        })?;
        self.pos = end;
        Ok(i32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    /// Reads a big-endian `i16`.
    ///
    /// # Errors
    ///
    /// Fails if fewer than two bytes remain.
    pub fn i16(&mut self, what: &'static str) -> Result<i16, FieldError> {
        let end = self.pos + 2;
        let slice = self.buf.get(self.pos..end).ok_or(FieldError::Truncated {
            what,
            read: self.pos,
        })?;
        self.pos = end;
        Ok(i16::from_be_bytes([slice[0], slice[1]]))
    }

    /// Reads a null-terminated string, consuming the terminator.
    ///
    /// # Errors
    ///
    /// Fails if there is no terminator, or the bytes are not UTF-8.
    pub fn cstr(&mut self, what: &'static str) -> Result<&'a str, FieldError> {
        let rest = &self.buf[self.pos..];
        let end = rest
            .iter()
            .position(|b| *b == 0)
            .ok_or(FieldError::Unterminated)?;
        let text = std::str::from_utf8(&rest[..end]).map_err(|_| FieldError::NotUtf8 { what })?;
        self.pos += end + 1;
        Ok(text)
    }

    /// Reads `n` bytes.
    ///
    /// # Errors
    ///
    /// Fails if fewer than `n` bytes remain.
    pub fn bytes(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], FieldError> {
        let end = self.pos + n;
        let slice = self.buf.get(self.pos..end).ok_or(FieldError::Truncated {
            what,
            read: self.pos,
        })?;
        self.pos = end;
        Ok(slice)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn reads_fixed_width_fields() {
        let buf = [0x01, 0x00, 0x00, 0x00, 0x2A, 0x00, 0x07];
        let mut r = Reader::new(&buf);
        assert_eq!(r.u8("tag").unwrap(), 1);
        assert_eq!(r.i32("pid").unwrap(), 42);
        assert_eq!(r.i16("count").unwrap(), 7);
        assert!(r.is_empty());
    }

    #[test]
    fn reads_a_null_terminated_string() {
        let buf = b"search_path\0public\0";
        let mut r = Reader::new(buf);
        assert_eq!(r.cstr("name").unwrap(), "search_path");
        assert_eq!(r.cstr("value").unwrap(), "public");
        assert!(r.is_empty());
    }

    #[test]
    fn an_empty_string_is_legal() {
        // An unnamed statement or portal is the empty string, which is the
        // common case for drivers that do not name their prepares.
        let buf = b"\0rest";
        let mut r = Reader::new(buf);
        assert_eq!(r.cstr("statement").unwrap(), "");
        assert_eq!(r.remaining(), b"rest");
    }

    #[test]
    fn truncation_is_an_error_not_a_panic() {
        // These bytes come from the network.
        for (len, what) in [(0, "u8"), (3, "i32"), (1, "i16")] {
            let buf = vec![0_u8; len];
            let mut r = Reader::new(&buf);
            let err = match what {
                "u8" => r.u8("f").unwrap_err(),
                "i32" => r.i32("f").unwrap_err(),
                _ => r.i16("f").unwrap_err(),
            };
            assert!(
                matches!(err, FieldError::Truncated { .. }),
                "{what}: {err:?}"
            );
        }
    }

    #[test]
    fn an_unterminated_string_is_an_error() {
        let buf = b"no terminator here";
        let mut r = Reader::new(buf);
        assert_eq!(r.cstr("name").unwrap_err(), FieldError::Unterminated);
    }

    #[test]
    fn invalid_utf8_is_rejected_by_name() {
        let buf = [0xFF, 0xFE, 0x00];
        let mut r = Reader::new(&buf);
        assert_eq!(
            r.cstr("value").unwrap_err(),
            FieldError::NotUtf8 { what: "value" }
        );
    }

    #[test]
    fn reads_a_byte_run() {
        let buf = b"abcdef";
        let mut r = Reader::new(buf);
        assert_eq!(r.bytes(3, "payload").unwrap(), b"abc");
        assert_eq!(r.remaining(), b"def");
        assert!(r.bytes(99, "payload").is_err());
    }

    #[test]
    fn error_messages_name_the_field_and_position() {
        let buf = [0_u8; 2];
        let mut r = Reader::new(&buf);
        r.u8("tag").unwrap();
        let err = r.i32("process_id").unwrap_err();
        assert!(err.to_string().contains("process_id"), "{err}");
        assert!(err.to_string().contains('1'), "{err}");
    }

    #[test]
    fn reading_never_panics_on_arbitrary_input() {
        let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = usize::try_from(seed % 24).unwrap();
            let buf: Vec<u8> = (0..len)
                .map(|i| u8::try_from((seed >> (i % 8 * 8)) & 0xFF).unwrap())
                .collect();

            let mut r = Reader::new(&buf);
            let _ = r.u8("a");
            let _ = r.i32("b");
            let _ = r.i16("c");
            let _ = r.cstr("d");
            let _ = r.bytes(7, "e");
        }
    }
}
