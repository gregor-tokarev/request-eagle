use std::fmt::Write as _;

/// A response body's hex dump covers this many bytes. Larger bodies are saved
/// to a file to see them whole.
pub(super) const HEX_LIMIT: usize = 1024 * 1024;

const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Rows of 16 bytes: their offset, their values and their printable characters.
pub(crate) fn hex_dump(bytes: &[u8]) -> String {
    let mut dump = String::with_capacity(bytes.len() * 4 + bytes.len() / 16 * 12);

    for (line, chunk) in bytes.chunks(16).enumerate() {
        let _ = write!(dump, "{:08x}  ", line * 16);
        for index in 0..16 {
            match chunk.get(index) {
                Some(&byte) => {
                    dump.push(DIGITS[usize::from(byte >> 4)] as char);
                    dump.push(DIGITS[usize::from(byte & 0x0f)] as char);
                    dump.push(' ');
                }
                None => dump.push_str("   "),
            }
        }
        dump.push(' ');
        dump.extend(chunk.iter().map(|&byte| {
            if byte.is_ascii_graphic() || byte == b' ' {
                byte as char
            } else {
                '.'
            }
        }));
        dump.push('\n');
    }

    dump
}
