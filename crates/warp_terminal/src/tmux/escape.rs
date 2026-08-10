//! Byte-exact tmux `%output` escaping.
//!
//! tmux escapes ONLY bytes `< 0x20` and backslash (`0x5C`) as a backslash
//! followed by exactly three octal digits (`printf "\\%03o"`). Every other
//! byte (including `>= 0x80`) passes through verbatim.

/// Encoder/decoder for the tmux `%output` wire escaping, grouped under one type.
pub struct Escaper;

impl Escaper {
    #[inline]
    fn is_octal_digit(b: u8) -> bool {
        b.wrapping_sub(b'0') < 8
    }

    /// Encode raw bytes into the tmux wire form.
    pub fn escape(raw: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(raw.len());
        for &b in raw {
            if b < 0x20 || b == b'\\' {
                out.push(b'\\');
                out.push(b'0' + (b >> 6));
                out.push(b'0' + ((b >> 3) & 7));
                out.push(b'0' + (b & 7));
            } else {
                out.push(b);
            }
        }
        out
    }

    /// Decode the tmux wire form back into raw bytes.
    ///
    /// A backslash followed by exactly three octal digits whose value is `0..=255`
    /// decodes to that byte. Any other backslash (wrong following bytes, out-of-range
    /// value, or a trailing lone backslash) is emitted literally. Never panics.
    pub fn unescape(escaped: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(escaped.len());
        let mut i = 0;
        let n = escaped.len();
        while i < n {
            let b = escaped[i];
            if b == b'\\' && i + 3 < n {
                let d0 = escaped[i + 1];
                let d1 = escaped[i + 2];
                let d2 = escaped[i + 3];
                if Self::is_octal_digit(d0) && Self::is_octal_digit(d1) && Self::is_octal_digit(d2)
                {
                    let val =
                        ((d0 - b'0') as u16) * 64 + ((d1 - b'0') as u16) * 8 + (d2 - b'0') as u16;
                    if val <= 255 {
                        out.push(val as u8);
                        i += 4;
                        continue;
                    }
                }
            }
            out.push(b);
            i += 1;
        }
        out
    }
}

#[cfg(test)]
#[path = "escape_tests.rs"]
mod tests;
