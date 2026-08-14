// SPDX-License-Identifier: Apache-2.0
//! Parameter Data assembly and count-driven token spans.

use crate::card::{CardScan, PhysicalLine, Section};
use crate::directory::DirectoryEntry;
use crate::global::{Global, RealPrecision};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;
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
    pub(crate) association_pointers: Vec<TrailingPointer>,
    pub(crate) property_pointers: Vec<TrailingPointer>,
    pub(crate) fully_valid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrailingPointer {
    pub(crate) token_index: usize,
    pub(crate) raw_pointer: i64,
}

impl ParameterRecord {
    pub(crate) fn integer(&self, index: usize) -> Option<i64> {
        match self.tokens.get(index).map(|token| &token.value)? {
            TokenValue::Integer(value) => Some(*value),
            TokenValue::Omitted | TokenValue::Real(_) | TokenValue::String(_) => None,
        }
    }

    pub(crate) fn integer_or(&self, index: usize, default: i64) -> Option<i64> {
        match self.tokens.get(index).map(|token| &token.value) {
            None | Some(TokenValue::Omitted) => Some(default),
            Some(TokenValue::Integer(value)) => Some(*value),
            Some(TokenValue::Real(_) | TokenValue::String(_)) => None,
        }
    }

    pub(crate) fn number(&self, index: usize) -> Option<f64> {
        match self.tokens.get(index).map(|token| &token.value)? {
            TokenValue::Integer(value) => Some(*value as f64),
            TokenValue::Real(value) => Some(*value),
            TokenValue::Omitted | TokenValue::String(_) => None,
        }
    }

    pub(crate) fn number_or(&self, index: usize, default: f64) -> Option<f64> {
        match self.tokens.get(index).map(|token| &token.value) {
            None | Some(TokenValue::Omitted) => Some(default),
            Some(TokenValue::Integer(value)) => Some(*value as f64),
            Some(TokenValue::Real(value)) => Some(*value),
            Some(TokenValue::String(_)) => None,
        }
    }

    /// Return the sending-system significance for a real token. A `D`
    /// exponent selects double precision; every other real syntax selects
    /// single precision. Integer tokens are exact and have no such bound.
    pub(crate) fn number_uncertainty(
        &self,
        index: usize,
        value: f64,
        precision: RealPrecision,
    ) -> f64 {
        self.number_significance_with(index, precision)
            .map_or(0.0, |digits| {
                if value == 0.0 {
                    0.0
                } else {
                    0.5 * 10.0_f64.powf(value.abs().log10().floor() - f64::from(digits) + 1.0)
                }
            })
    }

    fn number_significance_with(&self, index: usize, precision: RealPrecision) -> Option<u32> {
        let token = self.tokens.get(index)?;
        if !matches!(token.value, TokenValue::Real(_)) {
            return None;
        }
        let bytes = self.bytes.get(token.span.clone())?;
        if bytes.iter().any(|byte| matches!(byte, b'D' | b'd')) {
            Some(precision.double_significance)
        } else {
            Some(precision.single_significance)
        }
    }

    pub(crate) fn string(&self, index: usize) -> Option<&[u8]> {
        match self.tokens.get(index).map(|token| &token.value)? {
            TokenValue::String(value) => Some(value),
            TokenValue::Omitted | TokenValue::Integer(_) | TokenValue::Real(_) => None,
        }
    }

    pub(crate) fn string_or_empty(&self, index: usize) -> Option<&[u8]> {
        match self.tokens.get(index).map(|token| &token.value) {
            None | Some(TokenValue::Omitted) => Some(&[]),
            Some(TokenValue::String(value)) => Some(value),
            Some(TokenValue::Integer(_) | TokenValue::Real(_)) => None,
        }
    }

    /// Return a nonnegative declared list count only when at least that many
    /// tokens remain in this record. Each list item consumes one or more
    /// tokens, so this is a format-derived upper bound for every count-driven
    /// loop before its entity-specific stride is validated.
    pub(crate) fn count(&self, index: usize) -> Option<usize> {
        self.count_with_stride(index, 1)
    }

    /// Return a nonnegative declared count only when all fixed-width items fit.
    pub(crate) fn count_with_stride(&self, index: usize, stride: usize) -> Option<usize> {
        let count = self
            .integer(index)
            .and_then(|value| usize::try_from(value).ok())?;
        let required = count.checked_mul(stride)?;
        let item_start = index.checked_add(1)?;
        (required <= self.tokens.len().saturating_sub(item_start)).then_some(count)
    }
}

pub(crate) fn trailing_pointer_groups(
    record: &ParameterRecord,
    directory: &BTreeMap<u32, &DirectoryEntry>,
) -> Option<TrailingPointerGroups> {
    trailing_pointer_group_candidates(record, directory)
        .into_iter()
        .filter(|groups| groups.fully_valid)
        .min_by_key(|groups| groups.token_start)
}

pub(crate) fn trailing_pointer_group_candidates(
    record: &ParameterRecord,
    directory: &BTreeMap<u32, &DirectoryEntry>,
) -> Vec<TrailingPointerGroups> {
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
            let association_pointers = (0..association_count)
                .map(|index| {
                    let token_index = association_start + index;
                    Some(TrailingPointer {
                        token_index,
                        raw_pointer: record.integer(token_index)?,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            let property_pointers = (0..property_count)
                .map(|index| {
                    let token_index = property_count_index + 1 + index;
                    Some(TrailingPointer {
                        token_index,
                        raw_pointer: record.integer(token_index)?,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            let associations = association_pointers
                .iter()
                .filter_map(|pointer| {
                    u32::try_from(pointer.raw_pointer)
                        .ok()
                        .filter(|sequence| sequence % 2 == 1)
                        .filter(|sequence| {
                            directory
                                .get(sequence)
                                .is_some_and(|entry| matches!(entry.entity_type, 212 | 312 | 402))
                        })
                })
                .collect::<Vec<_>>();
            let properties = property_pointers
                .iter()
                .filter_map(|pointer| {
                    u32::try_from(pointer.raw_pointer)
                        .ok()
                        .filter(|sequence| sequence % 2 == 1)
                        .filter(|sequence| {
                            directory.get(sequence).is_some_and(|entry| {
                                matches!(entry.entity_type, 316 | 322 | 406 | 422)
                            })
                        })
                })
                .collect::<Vec<_>>();
            let fully_valid = associations.len() == association_pointers.len()
                && properties.len() == property_pointers.len();
            Some(TrailingPointerGroups {
                token_start: association_count_index,
                associations,
                properties,
                association_pointers,
                property_pointers,
                fully_valid,
            })
        })
        .collect()
}

fn malformed(sequence: u32, message: impl Into<String>) -> CodecError {
    crate::error::malformed(format!(
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
    if text.is_empty() {
        return Ok(Token {
            value: TokenValue::Omitted,
            span,
        });
    }
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
    ctx: Option<&DecodeContext<'_>>,
) -> Result<(Vec<Token>, usize), CodecError> {
    let mut tokens = Vec::new();
    let mut cursor = 0_usize;
    loop {
        if bytes.get(cursor) == Some(&record_delimiter) {
            return Ok((tokens, cursor + 1));
        }
        if bytes.get(cursor) == Some(&parameter_delimiter) {
            charge_token(ctx)?;
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
        charge_token(ctx)?;
        tokens.push(token);
        match bytes.get(end).copied() {
            Some(value) if value == parameter_delimiter => cursor = end + 1,
            Some(value) if value == record_delimiter => return Ok((tokens, end + 1)),
            _ => return Err(malformed(sequence, "token is not followed by a delimiter")),
        }
    }
}

pub(crate) fn assemble_with_context(
    scan: &CardScan,
    directory: &[DirectoryEntry],
    global: &Global,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Vec<ParameterRecord>, CodecError> {
    let lines = scan
        .lines
        .iter()
        .filter(|line| line.section == Some(Section::Parameter))
        .map(|line| (line.sequence.unwrap_or_default(), line))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .filter(|entry| !(entry.entity_type == 0 && entry.parameter_line_count == 0))
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut owned_by_entry = BTreeMap::<u32, Vec<u32>>::new();
    for (sequence, line) in &lines {
        let pointer = back_pointer(line)?;
        if pointer == 0 || pointer % 2 == 0 || !entries.contains_key(&pointer) {
            return Err(CodecError::Malformed(format!(
                "IGES Parameter Data card P{sequence} back-pointer {pointer} is not an owning odd Directory Entry sequence"
            )));
        }
        owned_by_entry.entry(pointer).or_default().push(*sequence);
    }
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
        let owned = owned_by_entry
            .get(&entry.sequence)
            .map_or(&[][..], Vec::as_slice);
        let actual_start = owned.first().copied().ok_or_else(|| {
            malformed(
                entry.sequence,
                "no Parameter Data card points to this Directory Entry",
            )
        })?;
        let actual_end = owned
            .last()
            .copied()
            .and_then(|sequence| sequence.checked_add(1))
            .ok_or_else(|| malformed(entry.sequence, "Parameter Data range overflow"))?;
        if actual_start != start || owned.windows(2).any(|pair| pair[1] != pair[0] + 1) {
            return Err(malformed(
                entry.sequence,
                "Parameter Data back-pointer range is not contiguous at the declared start",
            ));
        }
        let declared_count = usize::try_from(count)
            .map_err(|_| malformed(entry.sequence, "Parameter Data count overflows usize"))?;
        if owned.len() != declared_count {
            return Err(malformed(
                entry.sequence,
                format!(
                    "declares {declared_count} Parameter Data cards but owns {} by back-pointer",
                    owned.len()
                ),
            ));
        }
        let mut bytes = Vec::new();
        for sequence in actual_start..actual_end {
            let line = lines.get(&sequence).ok_or_else(|| {
                malformed(
                    entry.sequence,
                    format!("Parameter Data card P{sequence} is missing"),
                )
            })?;
            bytes.extend_from_slice(&line.payload[..64]);
        }
        let (tokens, record_end) = tokenize(
            &bytes,
            global.parameter_delimiter,
            global.record_delimiter,
            entry.sequence,
            ctx,
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
            line_range: actual_start..actual_end,
            comment: bytes[record_end..].to_vec(),
            bytes,
            tokens,
        });
    }
    Ok(records)
}

fn charge_token(ctx: Option<&DecodeContext<'_>>) -> Result<(), CodecError> {
    ctx.map_or(Ok(()), |ctx| {
        ctx.charge_collection_items(1, "iges_parameter_tokens")
    })
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

#[cfg(test)]
mod tests;
