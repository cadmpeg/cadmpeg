// SPDX-License-Identifier: Apache-2.0
//! Versioned finding normalization.
use crate::report::{
    check::{Check, Finding},
    Severity,
};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Pipeline stage that emits a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Decoder finding.
    Decode,
    /// Validator finding.
    Check,
    /// Encoder finding.
    Write,
}

/// A numeric observation with an inclusive reproduction band.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NumberBand {
    /// Original value.
    pub value: f64,
    /// Inclusive lower bound.
    pub lower: f64,
    /// Inclusive upper bound.
    pub upper: f64,
}

/// Stable normalized identity of a finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Signature {
    /// Normalization contract version.
    pub signature_version: u32,
    /// Producing stage.
    pub stage: Stage,
    /// Registry codec id.
    pub codec: String,
    /// Validator id.
    pub check: Check,
    /// Finding severity.
    pub severity: Severity,
    /// Entity kind from its identity.
    pub entity_kind: String,
    /// Message with entity identities and numeric values replaced.
    pub message: String,
    /// Observations, excluded from the deduplication hash.
    pub numbers: Vec<NumberBand>,
    /// First eight SHA-256 bytes of the normalized fields, in lowercase hex.
    pub hash: String,
}

const NUMBER_BAND_FRACTION: f64 = 0.1;
const HEX: &[u8; 16] = b"0123456789abcdef";

impl Signature {
    /// Normalize an existing check finding.
    pub fn new(finding: &Finding, codec: &str, stage: Stage) -> Self {
        let mut message = finding.message.clone();
        let mut ids: Vec<&str> = finding
            .entity
            .iter()
            .chain(&finding.related)
            .map(String::as_str)
            .collect();
        ids.sort_by_key(|id| std::cmp::Reverse(id.len()));
        for id in ids {
            message = message.replace(id, "{entity}");
        }
        // Quoted identities can name additional entities beyond the related list.
        let words: Vec<String> = message
            .split(|c: char| c.is_whitespace() || matches!(c, '`' | '\'' | '"' | ',' | '(' | ')'))
            .filter(|word| crate::ids::is_valid_identity(word))
            .map(str::to_owned)
            .collect();
        for id in words {
            message = message.replace(&id, "{entity}");
        }
        let message = private_tokens(&message);
        let (message, numbers) = normalize_numbers(&message);
        let entity_kind = finding
            .entity
            .as_deref()
            .map_or_else(|| "document".into(), entity_kind);
        let signature_version = 1;
        let fields = serde_json::json!([
            signature_version,
            stage,
            codec,
            finding.check,
            finding.severity,
            entity_kind,
            message
        ]);
        let digest = Sha256::digest(fields.to_string().as_bytes());
        let mut hash = String::new();
        for byte in &digest[..8] {
            hash.push(char::from(HEX[usize::from(byte >> 4)]));
            hash.push(char::from(HEX[usize::from(byte & 15)]));
        }
        Self {
            signature_version,
            stage,
            codec: codec.into(),
            check: finding.check,
            severity: finding.severity,
            entity_kind,
            message,
            numbers,
            hash,
        }
    }

    /// Match normalized fields and each retained numeric observation's band.
    pub fn reproduces(&self, finding: &Finding) -> bool {
        let candidate = Self::new(finding, &self.codec, self.stage);
        self.signature_version == candidate.signature_version
            && candidate.hash == self.hash
            && candidate.numbers.len() == self.numbers.len()
            && self
                .numbers
                .iter()
                .zip(&candidate.numbers)
                .all(|(band, number)| number.value >= band.lower && number.value <= band.upper)
    }
}

fn normalize_numbers(message: &str) -> (String, Vec<NumberBand>) {
    let mut output = String::new();
    let mut numbers = Vec::new();
    let mut rest = message;
    while !rest.is_empty() {
        let first = rest.chars().next().unwrap_or_default();
        if let Some((length, value)) = number_at(rest, !output.ends_with('.')) {
            let radius = value.abs() * NUMBER_BAND_FRACTION;
            numbers.push(NumberBand {
                value,
                lower: (value - radius).max(-f64::MAX),
                upper: (value + radius).min(f64::MAX),
            });
            output.push_str("{number}");
            rest = &rest[length..];
            continue;
        }
        output.push(first);
        rest = &rest[first.len_utf8()..];
    }
    (output, numbers)
}

fn private_tokens(message: &str) -> String {
    let mut output = String::new();
    let mut rest = message;
    while let Some(begin) = rest.find(['`', '"']) {
        let delimiter = rest.as_bytes()[begin];
        output.push_str(&rest[..=begin]);
        rest = &rest[begin + 1..];
        let Some(end) = rest.find(char::from(delimiter)) else {
            output.push_str(rest);
            return output;
        };
        let token = &rest[..end];
        if token == "{entity}" || token.parse::<f64>().is_ok() {
            output.push_str(token);
        } else {
            output.push_str("{text}");
        }
        output.push(char::from(delimiter));
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    output
        .split_whitespace()
        .map(|word| {
            if word.contains('/') || word.contains('\\') {
                "{text}"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn entity_kind(id: &str) -> String {
    let kind = id
        .split(':')
        .nth(2)
        .and_then(|tail| tail.split('#').next())
        .unwrap_or("record");
    if kind
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        kind.into()
    } else {
        "record".into()
    }
}

fn number_at(text: &str, leading_dot: bool) -> Option<(usize, f64)> {
    let bytes = text.as_bytes();
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let start = end;
    if bytes.get(start) == Some(&b'0') && matches!(bytes.get(start + 1), Some(b'x' | b'X')) {
        end = start + 2;
        let digits = end;
        while bytes.get(end).is_some_and(u8::is_ascii_hexdigit) {
            end += 1;
        }
        if end == digits {
            return None;
        }
        let integer = u128::from_str_radix(&text[digits..end], 16).ok()?;
        let mut value = integer.to_string().parse::<f64>().ok()?;
        if bytes.first() == Some(&b'-') {
            value = -value;
        }
        return Some((end, value));
    }
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let mut digits = end - start;
    if bytes.get(end) == Some(&b'.') && (digits > 0 || leading_dot) {
        end += 1;
        let fraction = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        digits += end - fraction;
    }
    if digits == 0 {
        return None;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let exponent = end;
        end += 1;
        if matches!(bytes.get(end), Some(b'+' | b'-')) {
            end += 1;
        }
        let digits = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == digits {
            end = exponent;
        }
    }
    let value = text[..end].parse::<f64>().ok()?;
    value.is_finite().then_some((end, value))
}
