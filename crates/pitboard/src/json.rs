//! The one writer of `--json` output, the envelope of every command and of every error.
//!
//! It prints ASCII alone, on every system. A character outside ASCII, such as a Vietnamese
//! letter or an emoji in a label, is written as a `\u` escape, and one above U+FFFF as its
//! UTF-16 surrogate pair, the only way JSON escapes one (RFC 8259, section 7). The values are
//! unchanged: a JSON parser reads each escape back as the character it stands for. A program
//! that decodes the bytes as something other than UTF-8 then parses them intact, where raw
//! UTF-8 would reach it as other characters.

use serde::Serialize;
use serde_json::Value;
use serde_json::ser::{Formatter, Serializer};
use std::io;

/// `value` as one line of JSON in which every byte is ASCII.
pub fn ascii(value: &Value) -> String {
    let mut written = Vec::new();
    value
        .serialize(&mut Serializer::with_formatter(&mut written, Ascii))
        .expect("a JSON value serializes into memory");
    String::from_utf8(written).expect("ASCII is UTF-8")
}

/// serde_json's compact form, with every character outside ASCII escaped.
///
/// serde_json escapes the quote, the backslash and the control characters itself, and hands
/// each run of text between those to `write_string_fragment`, the text of object keys
/// included. That run is the one place a character outside ASCII can be.
struct Ascii;

impl Formatter for Ascii {
    fn write_string_fragment<W>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        let mut unwritten = 0;
        for (at, character) in fragment.char_indices() {
            if character.is_ascii() {
                continue;
            }
            writer.write_all(&fragment.as_bytes()[unwritten..at])?;
            for unit in character.encode_utf16(&mut [0; 2]) {
                writer.write_all(&escape(*unit))?;
            }
            unwritten = at + character.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[unwritten..])
    }
}

/// `\u` and four hex digits, in lower case as serde_json writes its own.
fn escape(unit: u16) -> [u8; 6] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digit = |shift: u16| HEX[usize::from((unit >> shift) & 0xf)];
    [b'\\', b'u', digit(12), digit(8), digit(4), digit(0)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_character_outside_ascii_is_a_u_escape() {
        assert_eq!(
            ascii(&json!({ "label": "Đạt" })),
            r#"{"label":"\u0110\u1ea1t"}"#
        );
        assert_eq!(ascii(&json!("é")), r#""\u00e9""#);
        assert_eq!(ascii(&json!("\u{ffff}")), r#""\uffff""#);
    }

    #[test]
    fn a_character_above_u_ffff_is_its_surrogate_pair() {
        assert_eq!(ascii(&json!("🏁")), r#""\ud83c\udfc1""#);
        assert_eq!(ascii(&json!("\u{10000}")), r#""\ud800\udc00""#);
        assert_eq!(ascii(&json!("\u{10ffff}")), r#""\udbff\udfff""#);
    }

    #[test]
    fn a_key_is_escaped_as_a_value_is() {
        assert_eq!(
            ascii(&json!({ "Đạt": "Đạt" })),
            r#"{"\u0110\u1ea1t":"\u0110\u1ea1t"}"#
        );
    }

    /// Text that is ASCII already, and every escape serde_json makes, come out as they did:
    /// the quote, the backslash and the control characters beside the text around them.
    #[test]
    fn ascii_is_written_as_serde_json_writes_it() {
        let value = json!({
            "v": 1,
            "ok": true,
            "data": null,
            "text": "a \"quoted\" back\\slash, a\ttab, a\nline, \u{0}\u{1f} and \u{7f}",
            "numbers": [0, -1, 1.5, 12.0, 1_790_474_400u64],
            "nested": { "list": [{}, [], ""] },
        });
        assert_eq!(ascii(&value), value.to_string());
    }

    #[test]
    fn what_is_printed_parses_back_as_the_same_values() {
        let value = json!({
            "label": "Đạt",
            "emoji": "🏁 and 🏎️",
            "combined": "e\u{301}",
            "joined": "👩\u{200d}💻",
            "separators": "\u{2028}\u{2029}",
            "control": "\u{0}é\u{1f}\"\\",
            "Đ": ["日本語", "한국어", "Ελληνικά", "עברית"],
        });
        let printed = ascii(&value);
        assert!(printed.is_ascii(), "{printed}");
        assert_eq!(
            serde_json::from_str::<Value>(&printed).unwrap(),
            value,
            "{printed}"
        );
    }
}
