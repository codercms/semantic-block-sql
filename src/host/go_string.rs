use std::fmt::Write as _;

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GoStringError {
    #[error("Go string literal is missing matching delimiters")]
    Delimiters,
    #[error("raw Go string literal contains an invalid delimiter")]
    RawDelimiter,
    #[error("interpreted Go string contains an unescaped line break")]
    LineBreak,
    #[error("incomplete Go escape sequence")]
    IncompleteEscape,
    #[error("invalid Go escape sequence \\{0}")]
    InvalidEscape(char),
    #[error("invalid {kind} escape in Go string literal")]
    InvalidDigits { kind: &'static str },
    #[error("Go escape value {0:#x} is not a valid Unicode scalar value")]
    InvalidUnicode(u32),
    #[error("Go byte escape value {0:#x} exceeds 0xff")]
    ByteOverflow(u32),
    #[error("decoded Go string is not valid UTF-8")]
    InvalidUtf8,
}

pub(super) fn decode_literal(source: &str) -> Result<String, GoStringError> {
    if source.starts_with('`') && source.ends_with('`') && source.len() >= 2 {
        return decode_raw(source);
    }
    decode_interpreted(source)
}

pub(super) fn decode_raw(source: &str) -> Result<String, GoStringError> {
    let body = source
        .strip_prefix('`')
        .and_then(|source| source.strip_suffix('`'))
        .ok_or(GoStringError::Delimiters)?;
    if body.contains('`') {
        return Err(GoStringError::RawDelimiter);
    }
    // The Go specification discards carriage returns from raw string values.
    Ok(body.replace('\r', ""))
}

pub(super) fn decode_interpreted(source: &str) -> Result<String, GoStringError> {
    let body = source
        .strip_prefix('"')
        .and_then(|source| source.strip_suffix('"'))
        .ok_or(GoStringError::Delimiters)?;
    let mut output = Vec::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\n' | '\r' => return Err(GoStringError::LineBreak),
            '\\' => {
                let escaped = chars.next().ok_or(GoStringError::IncompleteEscape)?;
                match escaped {
                    'a' => output.push(0x07),
                    'b' => output.push(0x08),
                    'f' => output.push(0x0c),
                    'n' => output.push(b'\n'),
                    'r' => output.push(b'\r'),
                    't' => output.push(b'\t'),
                    'v' => output.push(0x0b),
                    '\\' => output.push(b'\\'),
                    '"' => output.push(b'"'),
                    '\'' => output.push(b'\''),
                    'x' => {
                        let value = parse_digits(&mut chars, 2, 16, "hexadecimal")?;
                        output.push(value as u8);
                    }
                    'u' => {
                        let value = parse_digits(&mut chars, 4, 16, "Unicode")?;
                        push_unicode_bytes(&mut output, value)?;
                    }
                    'U' => {
                        let value = parse_digits(&mut chars, 8, 16, "Unicode")?;
                        push_unicode_bytes(&mut output, value)?;
                    }
                    digit @ '0'..='7' => {
                        let mut value = digit.to_digit(8).expect("matched octal digit");
                        for _ in 0..2 {
                            let next = chars
                                .next()
                                .ok_or(GoStringError::InvalidDigits { kind: "octal" })?;
                            let digit = next
                                .to_digit(8)
                                .ok_or(GoStringError::InvalidDigits { kind: "octal" })?;
                            value = value * 8 + digit;
                        }
                        if value > u8::MAX.into() {
                            return Err(GoStringError::ByteOverflow(value));
                        }
                        output.push(value as u8);
                    }
                    other => return Err(GoStringError::InvalidEscape(other)),
                }
            }
            other => {
                let mut bytes = [0u8; 4];
                output.extend_from_slice(other.encode_utf8(&mut bytes).as_bytes());
            }
        }
    }
    String::from_utf8(output).map_err(|_| GoStringError::InvalidUtf8)
}

fn parse_digits<I>(
    chars: &mut I,
    count: usize,
    radix: u32,
    kind: &'static str,
) -> Result<u32, GoStringError>
where
    I: Iterator<Item = char>,
{
    let mut value = 0u32;
    for _ in 0..count {
        let character = chars.next().ok_or(GoStringError::InvalidDigits { kind })?;
        let digit = character
            .to_digit(radix)
            .ok_or(GoStringError::InvalidDigits { kind })?;
        value = value * radix + digit;
    }
    Ok(value)
}

fn push_unicode_bytes(output: &mut Vec<u8>, value: u32) -> Result<(), GoStringError> {
    let character = char::from_u32(value).ok_or(GoStringError::InvalidUnicode(value))?;
    let mut bytes = [0u8; 4];
    output.extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
    Ok(())
}

pub(super) fn encode_interpreted(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '\u{0007}' => output.push_str("\\a"),
            '\u{0008}' => output.push_str("\\b"),
            '\u{000c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{000b}' => output.push_str("\\v"),
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            character if character.is_control() && u32::from(character) <= 0xff => {
                write!(output, "\\x{:02x}", u32::from(character))
                    .expect("writing to String cannot fail");
            }
            character if character.is_control() && u32::from(character) <= 0xffff => {
                write!(output, "\\u{:04x}", u32::from(character))
                    .expect("writing to String cannot fail");
            }
            character if character.is_control() => {
                write!(output, "\\U{:08x}", u32::from(character))
                    .expect("writing to String cannot fail");
            }
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

pub(super) fn can_encode_raw(value: &str) -> bool {
    !value.contains('`') && !value.contains('\r')
}

pub(super) fn encode_raw(value: &str) -> Result<String, GoStringError> {
    if !can_encode_raw(value) {
        return Err(GoStringError::RawDelimiter);
    }
    Ok(format!("`{value}`"))
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::fs;
    use std::process::Command;

    use tempfile::tempdir;

    use super::{decode_interpreted, decode_literal, encode_interpreted, encode_raw};

    #[test]
    fn decodes_complete_go_escape_set() {
        let source = r#""\a\b\f\n\r\t\v\\\"\'\141\x62\u03bb\U0001f642""#;
        assert_eq!(
            decode_interpreted(source).expect("decode interpreted string"),
            "\u{7}\u{8}\u{c}\n\r\t\u{b}\\\"'abλ🙂"
        );
    }

    #[test]
    fn rejects_non_utf8_byte_escape_sequences() {
        assert!(decode_interpreted(r#""\xff""#).is_err());
        assert_eq!(
            decode_interpreted(r#""\xc3\xa9""#).expect("UTF-8 byte escapes"),
            "é"
        );
    }

    #[test]
    fn deterministic_encoder_round_trips() {
        for value in test_values() {
            let encoded = encode_interpreted(value);
            assert_eq!(decode_interpreted(&encoded).expect("round trip"), value);
        }
    }

    #[test]
    fn raw_codec_matches_go_carriage_return_semantics() {
        assert_eq!(decode_literal("`a\r\nb`").expect("decode raw"), "a\nb");
        assert!(encode_raw("contains ` delimiter").is_err());
        assert!(encode_raw("contains\rreturn").is_err());
    }

    #[test]
    fn codec_cross_checks_with_go_strconv() {
        let values = test_values();
        let directory = tempdir().expect("create temporary Go project");
        let mut source = String::from(
            "package main\n\nimport (\n    \"encoding/hex\"\n    \"fmt\"\n    \"strconv\"\n)\n\nfunc main() {\n",
        );
        for (index, value) in values.iter().enumerate() {
            let encoded = encode_interpreted(value);
            source.push_str(&format!(
                "    value{index}, err{index} := strconv.Unquote({encoded:?})\n    if err{index} != nil {{ panic(err{index}) }}\n    fmt.Println(hex.EncodeToString([]byte(value{index})))\n    fmt.Println(strconv.Quote(value{index}))\n"
            ));
        }
        source.push_str("}\n");
        fs::write(directory.path().join("main.go"), source).expect("write Go source");
        let output = Command::new("go")
            .arg("run")
            .arg(".")
            .current_dir(directory.path())
            .env("GO111MODULE", "off")
            .output()
            .expect("run Go strconv cross-check");
        assert!(
            output.status.success(),
            "Go strconv cross-check failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("Go output is UTF-8");
        let lines = stdout.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), values.len() * 2);
        for (index, value) in values.iter().enumerate() {
            assert_eq!(lines[index * 2], hex(value.as_bytes()));
            assert_eq!(
                decode_interpreted(lines[index * 2 + 1]).expect("decode strconv.Quote output"),
                *value
            );
        }
    }

    fn test_values() -> [&'static str; 4] {
        [
            "plain",
            "quote: \" and slash: \\",
            "line one\nline two\tλ🙂",
            "controls: \u{0001}\u{007f}",
        ]
    }

    fn hex(bytes: &[u8]) -> String {
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
        }
        output
    }
}
