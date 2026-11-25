// SPDX-License-Identifier: Apache-2.0
//! The reading half of §2.5's one zero: a float field is read by its value,
//! so `-0`, `-0.0`, and `0` read as 0.
//!
//! Negative-zero JSON number tokens (`-0`, `-0.0`, `-0e0`, …) outside string
//! literals are rewritten to positive-zero float spellings (`0.0` or the same
//! spelling with a leading space in place of `-`) before feeding
//! `serde_json`'s `StrRead` deserializer directly. An integer field keeps
//! `serde_json`'s integer reading: `1.0`, `1e0`, and `-0` are floats, and an
//! integer field refuses them. Because reading uses `StrRead` directly, types
//! share the single `serde_json` string deserializer with `cadmpeg_core::tagged`
//! instead of instantiating an extra deserializer wrapper across the whole IR.

use std::borrow::Cow;

use serde::de;

/// Reads `text` as one JSON value of `T`, every float zero as 0.
pub(super) fn from_str<T: de::DeserializeOwned>(text: &str) -> Result<T, serde_json::Error> {
    let normalized = normalize_negative_zero(text);
    let mut deserializer = crate::bounds::deserializer(&normalized)?;
    let value = T::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

/// Reads `value` as one value of `T`, every float zero as 0.
pub(super) fn from_value<T: de::DeserializeOwned>(
    value: &serde_json::Value,
) -> Result<T, serde_json::Error> {
    let text = serde_json::to_string(value)?;
    from_str(&text)
}

/// Rewrites every negative-zero JSON number token outside string literals to a
/// positive-zero float spelling so float fields read `+0.0` while integer
/// fields still refuse `-0` and `-0.0`.
fn normalize_negative_zero(text: &str) -> Cow<'_, str> {
    if !text.contains("-0") {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut index = 0;
    let mut in_string = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            index += 1;
            continue;
        }
        if byte == b'-'
            && bytes.get(index + 1) == Some(&b'0')
            && (index == 0 || is_json_delimiter(bytes[index - 1]))
        {
            if let Some((end, has_float_marker)) = negative_zero_end(bytes, index) {
                out.push_str(&text[copied..index]);
                if has_float_marker {
                    out.push(' ');
                    out.push_str(&text[index + 1..end]);
                } else {
                    out.push_str("0.0");
                }
                copied = end;
                index = end;
                continue;
            }
        }
        index += 1;
    }
    if copied == 0 {
        Cow::Borrowed(text)
    } else {
        out.push_str(&text[copied..]);
        Cow::Owned(out)
    }
}

fn is_json_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t' | b'\n' | b'\r' | b',' | b':' | b'[' | b']' | b'{' | b'}'
    )
}

/// If `bytes[start..]` starts with a valid negative-zero JSON number token
/// followed by a JSON delimiter or EOF, returns `(end_offset, has_float_marker)`.
fn negative_zero_end(bytes: &[u8], start: usize) -> Option<(usize, bool)> {
    let mut cursor = start + 2;
    let mut has_float_marker = false;
    if bytes.get(cursor) == Some(&b'.') {
        has_float_marker = true;
        cursor += 1;
        let frac_start = cursor;
        while bytes.get(cursor) == Some(&b'0') {
            cursor += 1;
        }
        if cursor == frac_start {
            return None;
        }
        if bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            return None;
        }
    }
    if matches!(bytes.get(cursor), Some(b'e' | b'E')) {
        has_float_marker = true;
        cursor += 1;
        if matches!(bytes.get(cursor), Some(b'+' | b'-')) {
            cursor += 1;
        }
        let exp_start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == exp_start {
            return None;
        }
    }
    if cursor == bytes.len() || is_json_delimiter(bytes[cursor]) {
        Some((cursor, has_float_marker))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::from_str;
    use crate::CadIr;

    #[derive(Debug, Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Tagged {
        Point { x: f64 },
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum External {
        Pair(f64, f64),
        Scale(f32),
    }

    #[derive(Debug, Deserialize)]
    struct Floats {
        plain: f64,
        optional: Option<f64>,
        list: Vec<f64>,
        tagged: Tagged,
        external: Vec<External>,
        open: serde_json::Value,
    }

    fn is_negative_zero(value: f64) -> bool {
        value == 0.0 && value.is_sign_negative()
    }

    #[test]
    fn a_float_field_reads_its_value_and_one_zero() {
        let text = r#"{
            "plain": -0,
            "optional": -0.0,
            "list": [-0.0, 0, -0, 1, 1.00, 1e0],
            "tagged": {"kind": "point", "x": -0.0},
            "external": [{"pair": [-0.0, -0]}, {"scale": -0.0}],
            "open": {"deep": [[-0.0]]}
        }"#;
        let bare: Floats = serde_json::from_str(text).unwrap();
        assert!(
            is_negative_zero(bare.plain),
            "the fixture holds a negative zero"
        );
        let read: Floats = from_str(text).unwrap();
        assert_eq!(read.plain.to_bits(), 0);
        assert_eq!(read.optional.map(f64::to_bits), Some(0));
        assert_eq!(
            read.list
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            [0.0, 0.0, 0.0, 1.0, 1.0, 1.0].map(f64::to_bits)
        );
        let Tagged::Point { x } = read.tagged;
        assert_eq!(x.to_bits(), 0);
        let [External::Pair(first, second), External::Scale(scale)] = read.external.as_slice()
        else {
            panic!("two external variants: {:?}", read.external);
        };
        assert_eq!([first.to_bits(), second.to_bits()], [0, 0]);
        assert_eq!(scale.to_bits(), 0);
        assert_eq!(read.open["deep"][0][0].as_f64().map(f64::to_bits), Some(0));
    }

    #[test]
    fn an_integer_field_reads_an_integer_spelling_only() {
        assert_eq!(from_str::<i64>("-1").unwrap(), -1);
        assert_eq!(from_str::<u32>("0").unwrap(), 0);
        for spelling in ["1.0", "1e0", "-0", "-0.0"] {
            assert!(from_str::<i64>(spelling).is_err(), "{spelling}");
            assert!(from_str::<u32>(spelling).is_err(), "{spelling}");
        }
    }

    #[test]
    fn a_document_read_holds_zero_where_its_text_writes_negative_zero() {
        let text = crate::examples::unit_cube()
            .unwrap()
            .to_canonical_json()
            .unwrap();
        let negative = text.replace(": 0.0", ": -0.0");
        assert_ne!(negative, text, "the fixture writes a negative zero");
        let coordinates = |document: &CadIr| {
            document
                .geometry
                .points
                .iter()
                .flat_map(|point| {
                    let position = point.position();
                    [position.x, position.y, position.z]
                })
                .collect::<Vec<_>>()
        };
        let bare: CadIr = serde_json::from_str(&negative).unwrap();
        assert!(coordinates(&bare).into_iter().any(is_negative_zero));
        let read = CadIr::from_json(&negative).unwrap();
        assert!(!coordinates(&read).into_iter().any(is_negative_zero));
        assert_eq!(read.to_canonical_json().unwrap(), text);
    }
}
