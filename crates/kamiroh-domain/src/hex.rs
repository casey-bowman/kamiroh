//! Fixed-width hex coding, shared by [`crate::EndpointId`] and
//! [`crate::NodeSecret`].
//!
//! Both encode and decode work into a caller-supplied buffer and allocate
//! nothing. That matters for `NodeSecret`: an allocating API would leave a copy
//! of key material in a `String` or `Vec` outside the protected type.
//!
//! Private to the crate. Each public type maps [`HexError`] onto its own error,
//! which is what lets `NodeSecret` report a parse failure without echoing any of
//! the bytes it was reading.

/// Why a hex string could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HexError {
    /// The input was not exactly twice the output length.
    Length {
        /// The input length that was supplied.
        got: usize,
    },
    /// The input held a character outside `[0-9a-fA-F]`.
    NotHex {
        /// The first offending character.
        found: char,
    },
}

/// Decodes `src` into `out`, which must be exactly half its length.
///
/// Accepts upper or lower case. On error `out` may be partially written; callers
/// holding secret material must still wipe it.
pub(crate) fn decode_into(src: &[u8], out: &mut [u8]) -> Result<(), HexError> {
    if src.len() != out.len() * 2 {
        return Err(HexError::Length { got: src.len() });
    }
    for (i, slot) in out.iter_mut().enumerate() {
        let hi = digit(src[i * 2])?;
        let lo = digit(src[i * 2 + 1])?;
        *slot = (hi << 4) | lo;
    }
    Ok(())
}

/// Encodes `src` into `out` as lowercase hex; `out` must be twice its length.
///
/// # Panics
///
/// Panics if `out.len() != src.len() * 2`. Callers pass fixed-size arrays, so a
/// mismatch is a bug rather than bad input.
pub(crate) fn encode_into(src: &[u8], out: &mut [u8]) {
    assert_eq!(out.len(), src.len() * 2, "hex output buffer is missized");
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for (i, byte) in src.iter().enumerate() {
        out[i * 2] = DIGITS[usize::from(byte >> 4)];
        out[i * 2 + 1] = DIGITS[usize::from(byte & 0x0f)];
    }
}

fn digit(c: u8) -> Result<u8, HexError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(HexError::NotHex {
            found: char::from(c),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let src = [0x00, 0x0f, 0xa5, 0xff];
        let mut encoded = [0u8; 8];
        encode_into(&src, &mut encoded);
        assert_eq!(&encoded, b"000fa5ff");

        let mut decoded = [0u8; 4];
        decode_into(&encoded, &mut decoded).unwrap();
        assert_eq!(decoded, src);
    }

    #[test]
    fn decodes_either_case() {
        let mut out = [0u8; 2];
        decode_into(b"AbCd", &mut out).unwrap();
        assert_eq!(out, [0xab, 0xcd]);
    }

    #[test]
    fn rejects_bad_length_and_bad_digits() {
        let mut out = [0u8; 2];
        assert_eq!(
            decode_into(b"abc", &mut out),
            Err(HexError::Length { got: 3 })
        );
        assert_eq!(
            decode_into(b"abcz", &mut out),
            Err(HexError::NotHex { found: 'z' })
        );
    }
}
