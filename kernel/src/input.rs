#![allow(clippy::module_name_repetitions)]

//! PS/2 Set-1 keyboard decoding used by the early interactive shell.

/// Stateful decoder for the subset of PS/2 Set-1 needed by the kernel shell.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeyboardDecoder {
    left_shift: bool,
    right_shift: bool,
    caps_lock: bool,
}

impl KeyboardDecoder {
    /// Creates a decoder with no active modifiers.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            left_shift: false,
            right_shift: false,
            caps_lock: false,
        }
    }

    /// Consumes one Set-1 scancode and returns an ASCII byte when the key
    /// represents shell input.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn decode(&mut self, scancode: u8) -> Option<u8> {
        let released = scancode & 0x80 != 0;
        let code = scancode & 0x7f;

        match code {
            0x2a => {
                self.left_shift = !released;
                return None;
            }
            0x36 => {
                self.right_shift = !released;
                return None;
            }
            0x3a if !released => {
                self.caps_lock = !self.caps_lock;
                return None;
            }
            _ => {}
        }

        if released {
            return None;
        }

        let shifted = self.left_shift || self.right_shift;
        let byte = match code {
            0x01 => 0x1b,
            0x02 => select(shifted, b'1', b'!'),
            0x03 => select(shifted, b'2', b'@'),
            0x04 => select(shifted, b'3', b'#'),
            0x05 => select(shifted, b'4', b'$'),
            0x06 => select(shifted, b'5', b'%'),
            0x07 => select(shifted, b'6', b'^'),
            0x08 => select(shifted, b'7', b'&'),
            0x09 => select(shifted, b'8', b'*'),
            0x0a => select(shifted, b'9', b'('),
            0x0b => select(shifted, b'0', b')'),
            0x0c => select(shifted, b'-', b'_'),
            0x0d => select(shifted, b'=', b'+'),
            0x0e => 0x08,
            0x0f => b'\t',
            0x10 => self.letter(b'q', shifted),
            0x11 => self.letter(b'w', shifted),
            0x12 => self.letter(b'e', shifted),
            0x13 => self.letter(b'r', shifted),
            0x14 => self.letter(b't', shifted),
            0x15 => self.letter(b'y', shifted),
            0x16 => self.letter(b'u', shifted),
            0x17 => self.letter(b'i', shifted),
            0x18 => self.letter(b'o', shifted),
            0x19 => self.letter(b'p', shifted),
            0x1a => select(shifted, b'[', b'{'),
            0x1b => select(shifted, b']', b'}'),
            0x1c => b'\n',
            0x1e => self.letter(b'a', shifted),
            0x1f => self.letter(b's', shifted),
            0x20 => self.letter(b'd', shifted),
            0x21 => self.letter(b'f', shifted),
            0x22 => self.letter(b'g', shifted),
            0x23 => self.letter(b'h', shifted),
            0x24 => self.letter(b'j', shifted),
            0x25 => self.letter(b'k', shifted),
            0x26 => self.letter(b'l', shifted),
            0x27 => select(shifted, b';', b':'),
            0x28 => select(shifted, b'\'', b'"'),
            0x29 => select(shifted, b'`', b'~'),
            0x2b => select(shifted, b'\\', b'|'),
            0x2c => self.letter(b'z', shifted),
            0x2d => self.letter(b'x', shifted),
            0x2e => self.letter(b'c', shifted),
            0x2f => self.letter(b'v', shifted),
            0x30 => self.letter(b'b', shifted),
            0x31 => self.letter(b'n', shifted),
            0x32 => self.letter(b'm', shifted),
            0x33 => select(shifted, b',', b'<'),
            0x34 => select(shifted, b'.', b'>'),
            0x35 => select(shifted, b'/', b'?'),
            0x39 => b' ',
            _ => return None,
        };

        Some(byte)
    }

    fn letter(self, lower: u8, shifted: bool) -> u8 {
        if shifted ^ self.caps_lock {
            lower.to_ascii_uppercase()
        } else {
            lower
        }
    }
}

const fn select(condition: bool, normal: u8, alternate: u8) -> u8 {
    if condition { alternate } else { normal }
}

#[cfg(test)]
mod tests {
    use super::KeyboardDecoder;

    #[test]
    fn decoder_handles_letters_modifiers_and_releases() {
        let mut decoder = KeyboardDecoder::new();
        assert_eq!(decoder.decode(0x1e), Some(b'a'));
        assert_eq!(decoder.decode(0x9e), None);
        assert_eq!(decoder.decode(0x2a), None);
        assert_eq!(decoder.decode(0x1e), Some(b'A'));
        assert_eq!(decoder.decode(0xaa), None);
        assert_eq!(decoder.decode(0x1e), Some(b'a'));
    }

    #[test]
    fn decoder_handles_shell_control_keys() {
        let mut decoder = KeyboardDecoder::new();
        assert_eq!(decoder.decode(0x1c), Some(b'\n'));
        assert_eq!(decoder.decode(0x0e), Some(0x08));
        assert_eq!(decoder.decode(0x39), Some(b' '));
    }
}
