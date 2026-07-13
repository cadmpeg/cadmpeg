// SPDX-License-Identifier: Apache-2.0
//! ISO 10303-21 string escape decoding and canonical encoding.

/// A malformed or unsupported string escape.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} at string byte {offset}")]
pub struct StringError {
    /// Byte position within the unquoted string token.
    pub offset: usize,
    /// Description of the violated escape invariant.
    pub message: String,
}

/// Decode the bytes between a Part 21 string token's apostrophe delimiters.
pub fn decode(input: &[u8]) -> Result<String, StringError> {
    let mut output = String::new();
    let mut at = 0;
    let mut page = b'A';
    while at < input.len() {
        match input[at] {
            b'\'' if input.get(at + 1) == Some(&b'\'') => {
                output.push('\'');
                at += 2;
            }
            b'\\' if input.get(at + 1) == Some(&b'\\') => {
                output.push('\\');
                at += 2;
            }
            b'\\' if input.get(at + 1) == Some(&b'P') => {
                if !matches!(input.get(at + 2), Some(b'A'..=b'I'))
                    || input.get(at + 3) != Some(&b'\\')
                {
                    return error(at, "invalid page-selection escape");
                }
                page = input[at + 2];
                at += 4;
            }
            b'\\' if input.get(at + 1) == Some(&b'S') => {
                if input.get(at + 2) != Some(&b'\\') {
                    return error(at, "invalid S escape");
                }
                let Some(&code) = input.get(at + 3) else {
                    return error(at, "truncated S escape");
                };
                if page != b'A' {
                    return error(at, "selected ISO 8859 page is not implemented");
                }
                output.push(char::from(code | 0x80));
                at += 4;
            }
            b'\\' if input.get(at + 1) == Some(&b'X') => match input.get(at + 2) {
                Some(b'\\') => {
                    let byte = hex_byte(input, at + 3)?;
                    output.push(char::from(byte));
                    at += 5;
                }
                Some(b'2') if input.get(at + 3) == Some(&b'\\') => {
                    let (decoded, end) = decode_wide(input, at + 4, 4)?;
                    output.push_str(&decoded);
                    at = end;
                }
                Some(b'4') if input.get(at + 3) == Some(&b'\\') => {
                    let (decoded, end) = decode_wide(input, at + 4, 8)?;
                    output.push_str(&decoded);
                    at = end;
                }
                _ => return error(at, "invalid X escape"),
            },
            b'\'' => return error(at, "unpaired apostrophe"),
            b'\\' => return error(at, "unknown reverse-solidus escape"),
            byte => {
                output.push(char::from(byte));
                at += 1;
            }
        }
    }
    Ok(output)
}

/// Encode text as bytes suitable between Part 21 apostrophe delimiters.
pub fn encode(input: &str) -> String {
    let mut output = String::new();
    for character in input.chars() {
        match character {
            '\'' => output.push_str("''"),
            '\\' => output.push_str("\\\\"),
            '\u{20}'..='\u{7e}' => output.push(character),
            character if u32::from(character) <= 0xffff => {
                output.push_str(&format!("\\X2\\{:04X}\\X0\\", u32::from(character)));
            }
            character => {
                output.push_str(&format!("\\X4\\{:08X}\\X0\\", u32::from(character)));
            }
        }
    }
    output
}

fn decode_wide(input: &[u8], start: usize, width: usize) -> Result<(String, usize), StringError> {
    let Some(relative_end) = input[start..]
        .windows(4)
        .position(|bytes| bytes == b"\\X0\\")
    else {
        return error(start, "unterminated wide escape");
    };
    let end = start + relative_end;
    if (end - start) % width != 0 {
        return error(start, "wide escape has incomplete code unit");
    }
    let mut scalars = Vec::new();
    for offset in (start..end).step_by(width) {
        let raw = std::str::from_utf8(&input[offset..offset + width])
            .ok()
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .ok_or_else(|| StringError {
                offset,
                message: "wide escape contains non-hexadecimal digits".into(),
            })?;
        scalars.push(raw);
    }
    let decoded = if width == 4 {
        let units = scalars.into_iter().map(|value| value as u16);
        char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .map_err(|_| StringError {
                offset: start,
                message: "wide escape contains an isolated surrogate".into(),
            })?
    } else {
        scalars
            .into_iter()
            .map(|value| {
                char::from_u32(value).ok_or_else(|| StringError {
                    offset: start,
                    message: "wide escape contains an invalid Unicode scalar".into(),
                })
            })
            .collect::<Result<String, _>>()?
    };
    Ok((decoded, end + 4))
}

fn hex_byte(input: &[u8], offset: usize) -> Result<u8, StringError> {
    let Some(bytes) = input.get(offset..offset + 2) else {
        return error(offset, "truncated byte escape");
    };
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        .ok_or_else(|| StringError {
            offset,
            message: "byte escape contains non-hexadecimal digits".into(),
        })
}

fn error<T>(offset: usize, message: &str) -> Result<T, StringError> {
    Err(StringError {
        offset,
        message: message.into(),
    })
}
