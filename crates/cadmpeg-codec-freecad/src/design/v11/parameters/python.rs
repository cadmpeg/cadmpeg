// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::collections::BTreeMap;

use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::{alphabet, Engine as _};
use cadmpeg_core::decode::View;
use serde_json::value::RawValue;

use crate::design::cell_content::formula::{Class, Value};
use crate::native::PropertyRecord;

const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::RequireNone)
        .with_decode_allow_trailing_bits(true),
);

/// The class restored by `FreeCAD`'s `PropertyPythonObject`.
pub(super) fn object(property: &PropertyRecord, entries: &BTreeMap<String, View<'_>>) -> Value {
    let Some(python) = property.values().iter().find(|value| value.tag == "Python") else {
        return Value::any();
    };
    let attribute = |key: &str| python.attributes.get(key).map(String::as_str);
    if let Some(file) = attribute("file") {
        return entries
            .get(file)
            .and_then(|view| std::str::from_utf8(view.window()).ok())
            .map_or_else(Value::any, json_value);
    }
    if attribute("module").is_some() && attribute("class").is_some() {
        return Value::of(&[Class::Refused]);
    }
    if attribute("json").is_none() {
        return Value::none();
    }
    let text = attribute("value").unwrap_or_default();
    if attribute("encoded") == Some("yes") {
        // The archive decoder stops at padding or the first non-alphabet
        // byte. An isolated final digit produces no byte.
        let end = text
            .find(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '+' | '/'))
            .unwrap_or(text.len());
        let end = end - usize::from(end % 4 == 1);
        match BASE64.decode(&text[..end]).map(String::from_utf8) {
            Ok(Ok(text)) => json_value(&text),
            Ok(Err(_)) | Err(_) => Value::any(),
        }
    } else {
        json_value(&decode_legacy(text))
    }
}

/// `PropertyPythonObject::decodeValue` processes the pre-base64 XML form.
fn decode_legacy(text: &str) -> Cow<'_, str> {
    if !text.contains('\\') {
        return Cow::Borrowed(text);
    }
    let mut decoded = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            if chars.next() == Some('n') {
                decoded.push('\n');
            }
        } else {
            decoded.push(character);
        }
    }
    Cow::Owned(decoded)
}

fn json_value(text: &str) -> Value {
    let text = text.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'));
    if let Ok(raw) = serde_json::from_str::<&RawValue>(text) {
        return match raw.get().as_bytes().first() {
            Some(b't' | b'f') => {
                Value::numeric(Class::Boolean, Some(f64::from(u8::from(text == "true"))))
            }
            Some(b'[') => Value::of(&[Class::List]),
            Some(b'{') => Value::of(&[Class::Dict]),
            Some(b'"') => serde_json::from_str::<String>(text)
                .map_or_else(|_| Value::of(&[Class::Text]), Value::text),
            Some(b'-' | b'0'..=b'9') if text.contains(['.', 'e', 'E']) => {
                Value::numeric(Class::Float, text.parse().ok())
            }
            Some(b'-' | b'0'..=b'9') => Value::integer_token(text),
            _ => Value::none(),
        };
    }
    match text {
        "NaN" => Value::numeric(Class::Float, Some(f64::NAN)),
        "Infinity" => Value::numeric(Class::Float, Some(f64::INFINITY)),
        "-Infinity" => Value::numeric(Class::Float, Some(f64::NEG_INFINITY)),
        _ if text.starts_with(['[', '{']) && json_with_non_finite_numbers(text) => {
            Value::of(&[if text.starts_with('[') {
                Class::List
            } else {
                Class::Dict
            }])
        }
        _ => Value::none(),
    }
}

/// Validate a container that uses Python json.loads' non-finite numeric tokens.
fn json_with_non_finite_numbers(text: &str) -> bool {
    if !text.contains("NaN") && !text.contains("Infinity") {
        return false;
    }
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    let mut quoted = false;
    let mut escaped = false;
    while let Some((index, character)) = chars.next() {
        if quoted {
            if !escaped && character == '"' {
                quoted = false;
            }
            escaped = !escaped && character == '\\';
        } else if character == '"' {
            quoted = true;
        } else if matches!(character, '-' | 'N' | 'I')
            && text[..index].chars().next_back().is_some_and(|previous| {
                matches!(previous, '[' | ',' | ':' | ' ' | '\t' | '\r' | '\n')
            })
        {
            let tail = &text[index..];
            let token = ["NaN", "Infinity", "-Infinity"].into_iter().find(|token| {
                tail.strip_prefix(token)
                    .is_some_and(|rest| rest.starts_with([',', ']', '}', ' ', '\t', '\r', '\n']))
            });
            if let Some(token) = token {
                let end = index + token.len();
                while chars.peek().is_some_and(|(offset, _)| *offset < end) {
                    chars.next();
                }
                normalized.push('0');
                continue;
            }
        }
        normalized.push(character);
    }
    serde_json::from_str::<&RawValue>(&normalized).is_ok()
}
