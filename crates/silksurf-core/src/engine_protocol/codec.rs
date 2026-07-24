//! Bounded byte reader and writer for the engine-protocol wire format.
//!
//! Every multi-byte integer is big-endian. Strings are a `u32` byte length
//! followed by UTF-8 bytes; sequences are a `u32` count followed by elements.
//! Each read is checked against the remaining buffer and against a hard limit,
//! so a hostile or truncated message decodes to a typed `ProtocolError` and
//! never over-reads or over-allocates.

use thiserror::Error;

/// Upper bound on a single control-message body. Control messages are small;
/// a body past this bound is rejected before allocation.
pub const MAX_MESSAGE_BYTES: usize = 1 << 20;

/// Upper bound on any single string field (URL, title, status, text).
pub const MAX_STRING_BYTES: usize = 64 * 1024;

/// Upper bound on the `FrameReady` damage list.
pub const MAX_DAMAGE_RECTS: usize = 4096;

/// Upper bound on any other length-prefixed sequence.
pub const MAX_VEC_LEN: usize = 65_536;

/// Every rejection at the protocol boundary. Returned instead of panicking so
/// a page engine cannot crash the shell with a malformed message.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtocolError {
    /// Envelope format byte is not understood by this build.
    #[error("unsupported wire version {0}")]
    UnsupportedWireVersion(u8),
    /// Header does not begin with the magic bytes.
    #[error("bad envelope magic")]
    BadMagic,
    /// Fewer bytes remain than the header or a field requires.
    #[error("truncated: need {need}, have {have}")]
    Truncated { need: usize, have: usize },
    /// A body decoded successfully but bytes remain after it.
    #[error("trailing bytes: {0}")]
    TrailingBytes(usize),
    /// Declared body length exceeds `MAX_MESSAGE_BYTES`.
    #[error("message too large: {len} > {max}")]
    MessageTooLarge { len: usize, max: usize },
    /// Envelope kind byte is neither command nor event.
    #[error("unknown message kind {0}")]
    UnknownKind(u8),
    /// Discriminant does not name a known message type.
    #[error("unknown message type {0}")]
    UnknownMessageType(u16),
    /// An enum discriminant byte is out of range for its type.
    #[error("bad discriminant for type {message_type}: {value}")]
    BadDiscriminant { message_type: u16, value: u8 },
    /// String bytes are not valid UTF-8.
    #[error("invalid utf-8")]
    InvalidUtf8,
    /// A length-prefixed field exceeds its configured limit.
    #[error("limit exceeded: {value} > {limit}")]
    LimitExceeded { limit: usize, value: usize },
}

/// Appends protocol values to an output buffer in wire order.
#[derive(Debug, Default)]
pub struct ByteWriter {
    buffer: Vec<u8>,
}

impl ByteWriter {
    /// A writer over an empty buffer.
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// The encoded bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }

    /// The bytes written so far.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Whether nothing has been written yet.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Writes one byte.
    pub fn put_u8(&mut self, value: u8) {
        self.buffer.push(value);
    }

    /// Writes a big-endian `u16`.
    pub fn put_u16(&mut self, value: u16) {
        self.buffer.extend_from_slice(&value.to_be_bytes());
    }

    /// Writes a big-endian `u32`.
    pub fn put_u32(&mut self, value: u32) {
        self.buffer.extend_from_slice(&value.to_be_bytes());
    }

    /// Writes a big-endian `u64`.
    pub fn put_u64(&mut self, value: u64) {
        self.buffer.extend_from_slice(&value.to_be_bytes());
    }

    /// Writes a big-endian `i32`.
    pub fn put_i32(&mut self, value: i32) {
        self.buffer.extend_from_slice(&value.to_be_bytes());
    }

    /// Writes a raw byte slice with no length prefix.
    pub fn put_bytes(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// Writes a `u32` byte length followed by the UTF-8 bytes. Refuses a
    /// string past `MAX_STRING_BYTES` so an over-long field cannot be encoded
    /// into an undecodeable message.
    pub fn put_str(&mut self, value: &str) -> Result<(), ProtocolError> {
        let len = value.len();
        if len > MAX_STRING_BYTES {
            return Err(ProtocolError::LimitExceeded {
                limit: MAX_STRING_BYTES,
                value: len,
            });
        }
        let prefix = u32::try_from(len).map_err(|_| ProtocolError::LimitExceeded {
            limit: MAX_STRING_BYTES,
            value: len,
        })?;
        self.put_u32(prefix);
        self.buffer.extend_from_slice(value.as_bytes());
        Ok(())
    }

    /// Writes a `u32` sequence count, bounded by `limit`.
    pub fn put_len(&mut self, count: usize, limit: usize) -> Result<(), ProtocolError> {
        if count > limit {
            return Err(ProtocolError::LimitExceeded {
                limit,
                value: count,
            });
        }
        let prefix = u32::try_from(count).map_err(|_| ProtocolError::LimitExceeded {
            limit,
            value: count,
        })?;
        self.put_u32(prefix);
        Ok(())
    }
}

/// Reads protocol values from a byte slice, tracking an offset and rejecting
/// any read that would exceed the buffer or a configured limit.
#[derive(Debug)]
pub struct ByteReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ByteReader<'a> {
    /// A reader positioned at the start of `bytes`.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    /// Bytes not yet consumed.
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    /// Succeeds only when every byte has been consumed.
    pub fn expect_end(&self) -> Result<(), ProtocolError> {
        let left = self.remaining();
        if left == 0 {
            Ok(())
        } else {
            Err(ProtocolError::TrailingBytes(left))
        }
    }

    fn take(&mut self, need: usize) -> Result<&'a [u8], ProtocolError> {
        let have = self.remaining();
        if need > have {
            return Err(ProtocolError::Truncated { need, have });
        }
        let start = self.offset;
        self.offset += need;
        Ok(&self.bytes[start..self.offset])
    }

    /// Borrows the next `len` bytes without interpreting them.
    pub fn get_slice(&mut self, len: usize) -> Result<&'a [u8], ProtocolError> {
        self.take(len)
    }

    /// Reads one byte.
    pub fn get_u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.take(1)?[0])
    }

    /// Reads a big-endian `u16`.
    pub fn get_u16(&mut self) -> Result<u16, ProtocolError> {
        let slice = self.take(2)?;
        Ok(u16::from_be_bytes([slice[0], slice[1]]))
    }

    /// Reads a big-endian `u32`.
    pub fn get_u32(&mut self) -> Result<u32, ProtocolError> {
        let slice = self.take(4)?;
        Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    /// Reads a big-endian `u64`.
    pub fn get_u64(&mut self) -> Result<u64, ProtocolError> {
        let slice = self.take(8)?;
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(slice);
        Ok(u64::from_be_bytes(bytes))
    }

    /// Reads a big-endian `i32`.
    pub fn get_i32(&mut self) -> Result<i32, ProtocolError> {
        let slice = self.take(4)?;
        Ok(i32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    /// Reads a `u32`-prefixed UTF-8 string, bounded by `MAX_STRING_BYTES`.
    pub fn get_str(&mut self) -> Result<String, ProtocolError> {
        let len = self.get_u32()? as usize;
        if len > MAX_STRING_BYTES {
            return Err(ProtocolError::LimitExceeded {
                limit: MAX_STRING_BYTES,
                value: len,
            });
        }
        let slice = self.take(len)?;
        core::str::from_utf8(slice)
            .map(str::to_owned)
            .map_err(|_| ProtocolError::InvalidUtf8)
    }

    /// Reads a `u32` sequence count, rejecting a count past `limit`.
    pub fn get_len(&mut self, limit: usize) -> Result<usize, ProtocolError> {
        let count = self.get_u32()? as usize;
        if count > limit {
            return Err(ProtocolError::LimitExceeded {
                limit,
                value: count,
            });
        }
        Ok(count)
    }
}
