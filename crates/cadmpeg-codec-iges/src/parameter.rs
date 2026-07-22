// SPDX-License-Identifier: Apache-2.0
//! Parameter Data assembly and count-driven token spans.

use crate::card::{CardScan, PhysicalLine, Section};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use cadmpeg_ir::codec::CodecError;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

/// One typed lexical value in an entity parameter record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TokenValue {
    Omitted,
    Integer(i64),
    Real(f64),
    String(Vec<u8>),
}

/// Typed value and its half-open offset in the assembled 64-column stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    pub(crate) value: TokenValue,
    pub(crate) span: Range<usize>,
}

/// One entity's assembled Parameter Data.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParameterRecord {
    pub(crate) directory_sequence: u32,
    pub(crate) line_range: Range<u32>,
    pub(crate) bytes: Vec<u8>,
    pub(crate) tokens: Vec<Token>,
    pub(crate) comment: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrailingPointerGroups {
    pub(crate) token_start: usize,
    pub(crate) associations: Vec<u32>,
    pub(crate) properties: Vec<u32>,
}

impl ParameterRecord {
    pub(crate) fn integer(&self, index: usize) -> Option<i64> {
        match self.tokens.get(index).map(|token| &token.value)? {
            TokenValue::Integer(value) => Some(*value),
            TokenValue::Omitted | TokenValue::Real(_) | TokenValue::String(_) => None,
        }
    }

    pub(crate) fn number(&self, index: usize) -> Option<f64> {
        match self.tokens.get(index).map(|token| &token.value)? {
            TokenValue::Integer(value) => Some(*value as f64),
            TokenValue::Real(value) => Some(*value),
            TokenValue::Omitted | TokenValue::String(_) => None,
        }
    }

    pub(crate) fn string(&self, index: usize) -> Option<&[u8]> {
        match self.tokens.get(index).map(|token| &token.value)? {
            TokenValue::String(value) => Some(value),
            TokenValue::Omitted | TokenValue::Integer(_) | TokenValue::Real(_) => None,
        }
    }

    /// Return a nonnegative declared list count only when at least that many
    /// tokens remain in this record. Each list item consumes one or more
    /// tokens, so this is a format-derived upper bound for every count-driven
    /// loop before its entity-specific stride is validated.
    pub(crate) fn count(&self, index: usize) -> Option<usize> {
        self.integer(index)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|count| *count <= self.tokens.len().saturating_sub(index + 1))
    }
}

pub(crate) fn trailing_pointer_groups(
    record: &ParameterRecord,
    directory: &BTreeMap<u32, &DirectoryEntry>,
) -> Option<TrailingPointerGroups> {
    (1..record.tokens.len())
        .filter_map(|association_count_index| {
            let association_count = record
                .integer(association_count_index)
                .and_then(|value| usize::try_from(value).ok())?;
            let association_start = association_count_index.checked_add(1)?;
            let property_count_index = association_start.checked_add(association_count)?;
            let property_count = record
                .integer(property_count_index)
                .and_then(|value| usize::try_from(value).ok())?;
            if association_count == 0 && property_count == 0 {
                return None;
            }
            let end = property_count_index
                .checked_add(1)?
                .checked_add(property_count)?;
            if end != record.tokens.len() {
                return None;
            }
            let associations = (0..association_count)
                .map(|index| {
                    record
                        .integer(association_start + index)
                        .and_then(|value| u32::try_from(value).ok())
                        .filter(|sequence| sequence % 2 == 1)
                        .filter(|sequence| {
                            directory
                                .get(sequence)
                                .is_some_and(|entry| matches!(entry.entity_type, 212 | 312 | 402))
                        })
                })
                .collect::<Option<Vec<_>>>()?;
            let properties = (0..property_count)
                .map(|index| {
                    record
                        .integer(property_count_index + 1 + index)
                        .and_then(|value| u32::try_from(value).ok())
                        .filter(|sequence| sequence % 2 == 1)
                        .filter(|sequence| {
                            directory.get(sequence).is_some_and(|entry| {
                                matches!(entry.entity_type, 316 | 322 | 406 | 422)
                            })
                        })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(TrailingPointerGroups {
                token_start: association_count_index,
                associations,
                properties,
            })
        })
        .min_by_key(|groups| groups.token_start)
}

fn malformed(sequence: u32, message: impl Into<String>) -> CodecError {
    CodecError::Malformed(format!(
        "IGES parameters for D{sequence}: {}",
        message.into()
    ))
}

fn positive_u32(value: i64, sequence: u32, name: &str) -> Result<u32, CodecError> {
    u32::try_from(value).map_err(|_| malformed(sequence, format!("{name} is not a positive u32")))
}

fn back_pointer(line: &PhysicalLine) -> Result<u32, CodecError> {
    let field = line.payload.get(64..72).ok_or_else(|| {
        CodecError::Malformed(format!(
            "IGES Parameter Data card P{} is shorter than 72 bytes",
            line.sequence.unwrap_or_default()
        ))
    })?;
    let text = std::str::from_utf8(field)
        .map_err(|_| CodecError::Malformed("IGES Parameter Data back-pointer is not ASCII".into()))?
        .trim();
    text.parse::<u32>()
        .map_err(|_| CodecError::Malformed("IGES Parameter Data back-pointer is not a u32".into()))
}

fn hollerith(
    bytes: &[u8],
    start: usize,
    sequence: u32,
) -> Result<Option<(Token, usize)>, CodecError> {
    let mut cursor = start;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == start || !matches!(bytes.get(cursor), Some(b'H' | b'h')) {
        return Ok(None);
    }
    let count = std::str::from_utf8(&bytes[start..cursor])
        .map_err(|_| malformed(sequence, "Hollerith count is not ASCII"))?
        .parse::<usize>()
        .map_err(|_| malformed(sequence, "Hollerith count is out of range"))?;
    let payload_start = cursor
        .checked_add(1)
        .ok_or_else(|| malformed(sequence, "Hollerith offset overflow"))?;
    let end = payload_start
        .checked_add(count)
        .ok_or_else(|| malformed(sequence, "Hollerith length overflow"))?;
    let payload = bytes
        .get(payload_start..end)
        .ok_or_else(|| malformed(sequence, "Hollerith payload is truncated"))?;
    Ok(Some((
        Token {
            value: TokenValue::String(payload.to_vec()),
            span: start..end,
        },
        end,
    )))
}

fn numeric(bytes: &[u8], span: Range<usize>, sequence: u32) -> Result<Token, CodecError> {
    let text = std::str::from_utf8(&bytes[span.clone()])
        .map_err(|_| malformed(sequence, "numeric token is not ASCII"))?
        .trim();
    let real = text
        .bytes()
        .any(|byte| matches!(byte, b'.' | b'E' | b'e' | b'D' | b'd'));
    let value = if real {
        let normalized = text.replace(['D', 'd'], "E");
        TokenValue::Real(
            normalized
                .parse::<f64>()
                .map_err(|_| malformed(sequence, format!("invalid real token {text:?}")))?,
        )
    } else {
        TokenValue::Integer(
            text.parse::<i64>()
                .map_err(|_| malformed(sequence, format!("invalid integer token {text:?}")))?,
        )
    };
    Ok(Token { value, span })
}

fn tokenize(
    bytes: &[u8],
    parameter_delimiter: u8,
    record_delimiter: u8,
    sequence: u32,
) -> Result<(Vec<Token>, usize), CodecError> {
    let mut tokens = Vec::new();
    let mut cursor = 0_usize;
    loop {
        if bytes.get(cursor) == Some(&record_delimiter) {
            return Ok((tokens, cursor + 1));
        }
        if bytes.get(cursor) == Some(&parameter_delimiter) {
            tokens.push(Token {
                value: TokenValue::Omitted,
                span: cursor..cursor,
            });
            cursor += 1;
            continue;
        }
        let (token, end) = if let Some(value) = hollerith(bytes, cursor, sequence)? {
            value
        } else {
            let end = bytes[cursor..]
                .iter()
                .position(|byte| {
                    matches!(*byte, value if value == parameter_delimiter || value == record_delimiter)
                })
                .and_then(|relative| cursor.checked_add(relative))
                .ok_or_else(|| malformed(sequence, "record delimiter is missing"))?;
            if end == cursor {
                return Err(malformed(sequence, "empty token has no delimiter"));
            }
            (numeric(bytes, cursor..end, sequence)?, end)
        };
        tokens.push(token);
        match bytes.get(end).copied() {
            Some(value) if value == parameter_delimiter => cursor = end + 1,
            Some(value) if value == record_delimiter => return Ok((tokens, end + 1)),
            _ => return Err(malformed(sequence, "token is not followed by a delimiter")),
        }
    }
}

pub(crate) fn assemble(
    scan: &CardScan,
    directory: &[DirectoryEntry],
    global: &Global,
) -> Result<Vec<ParameterRecord>, CodecError> {
    let lines = scan
        .lines
        .iter()
        .filter(|line| line.section == Some(Section::Parameter))
        .map(|line| (line.sequence.unwrap_or_default(), line))
        .collect::<BTreeMap<_, _>>();
    let mut used = BTreeSet::new();
    let mut records = Vec::new();
    for entry in directory {
        if entry.parameter_line_count == 0 && entry.entity_type == 0 {
            continue;
        }
        let start = positive_u32(
            entry.parameter_start,
            entry.sequence,
            "Parameter Data start",
        )?;
        let count = positive_u32(
            entry.parameter_line_count,
            entry.sequence,
            "Parameter Data line count",
        )?;
        if count == 0 {
            return Err(malformed(
                entry.sequence,
                "Parameter Data line count is zero",
            ));
        }
        let end = start
            .checked_add(count)
            .ok_or_else(|| malformed(entry.sequence, "Parameter Data range overflow"))?;
        let mut bytes = Vec::new();
        for sequence in start..end {
            let line = lines.get(&sequence).ok_or_else(|| {
                malformed(
                    entry.sequence,
                    format!("Parameter Data card P{sequence} is missing"),
                )
            })?;
            if back_pointer(line)? != entry.sequence {
                return Err(malformed(
                    entry.sequence,
                    format!("Parameter Data card P{sequence} has a different back-pointer"),
                ));
            }
            used.insert(sequence);
            bytes.extend_from_slice(&line.payload[..64]);
        }
        let (tokens, record_end) = tokenize(
            &bytes,
            global.parameter_delimiter,
            global.record_delimiter,
            entry.sequence,
        )?;
        if !matches!(tokens.first().map(|token| &token.value), Some(TokenValue::Integer(value)) if *value == entry.entity_type)
        {
            return Err(malformed(
                entry.sequence,
                "first parameter does not match the Directory Entry entity type",
            ));
        }
        records.push(ParameterRecord {
            directory_sequence: entry.sequence,
            line_range: start..end,
            comment: bytes[record_end..].to_vec(),
            bytes,
            tokens,
        });
    }
    if used.len() != lines.len() {
        let unowned = lines
            .keys()
            .find(|sequence| !used.contains(sequence))
            .copied()
            .unwrap_or_default();
        return Err(CodecError::Malformed(format!(
            "IGES Parameter Data card P{unowned} is not owned by a Directory Entry"
        )));
    }
    Ok(records)
}

pub(crate) fn summary_notes(records: &[ParameterRecord]) -> Vec<String> {
    vec![
        format!("parameter_records={}", records.len()),
        format!(
            "parameter_tokens={}",
            records
                .iter()
                .map(|record| record.tokens.len())
                .sum::<usize>()
        ),
        format!(
            "external_references={}",
            records
                .iter()
                .filter(|record| record.integer(0) == Some(416))
                .count()
        ),
    ]
}
