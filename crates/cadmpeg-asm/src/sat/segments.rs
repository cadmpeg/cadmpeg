// SPDX-License-Identifier: Apache-2.0
//! Bound concatenated text streams and keep their reference tables disjoint.

use super::{is_ws, Record, Token};
use cadmpeg_core::CodecError;
use std::ops::Range;

/// A complete four-word header at a record boundary. Once the declared
/// record count is met, intervening full lines without record delimiters are
/// descriptive text rather than another record in that table.
pub(super) fn next_header(bytes: &[u8], start: usize, table_complete: bool) -> Option<usize> {
    let mut pos = start;
    while pos < bytes.len() {
        while bytes.get(pos).is_some_and(|byte| is_ws(*byte)) {
            pos += 1;
        }

        let tail = bytes.get(pos..)?;
        if tail.is_empty() {
            return None;
        }
        if !table_complete && !tail[0].is_ascii_digit() {
            return None;
        }
        let end = tail
            .iter()
            .position(|byte| *byte == b'\n')
            .unwrap_or(tail.len());
        let line = &tail[..end];

        let mut words = line
            .split(|byte| is_ws(*byte))
            .filter(|word| !word.is_empty());
        let fields: [Option<&[u8]>; 4] = std::array::from_fn(|_| words.next());
        if fields[0].is_some_and(|word| matches!(word, b"End-of-ASM-data" | b"End-of-ACIS-data")) {
            return None;
        }
        if fields[0].is_some_and(|word| word.len() >= 3)
            && fields.iter().all(|field| {
                field.is_some_and(|word| {
                    !word.is_empty()
                        && word.iter().all(u8::is_ascii_digit)
                        && std::str::from_utf8(word)
                            .ok()
                            .and_then(|word| word.parse::<u64>().ok())
                            .is_some()
                })
            })
            && words.next().is_none()
        {
            return Some(pos);
        }
        if !table_complete
            || line
                .iter()
                .any(|byte| matches!(byte, b'#' | b'{' | b'}' | b'@'))
        {
            return None;
        }
        if end == tail.len() {
            return None;
        }
        pos += end + 1;
    }
    None
}

fn definition_count(records: &[Record]) -> usize {
    records
        .iter()
        .map(|record| {
            record
                .tokens
                .windows(2)
                .filter(|tokens| {
                    matches!(tokens,
        [Token::SubtypeOpen, Token::Ident(name) | Token::SubIdent(name)] if name != "ref")
                })
                .count()
        })
        .sum()
}

fn scoped_reference(value: i64, base: usize, len: usize, total: usize) -> Option<i64> {
    if value < 0 {
        return Some(value);
    }
    let Ok(local) = usize::try_from(value) else {
        return Some(value);
    };
    let target = if local < len {
        base.checked_add(local)?
    } else if local < total {
        total.checked_add(local)?
    } else {
        local
    };
    i64::try_from(target).ok()
}

/// Rebase entity and subtype references after every segment boundary is known.
/// An out-of-table reference stays outside the combined table; it cannot bind
/// a record or construction in a neighboring stream.
pub(super) fn rebase(records: &mut [Record], segments: &[Range<usize>]) -> Result<(), CodecError> {
    if segments.len() <= 1 {
        return Ok(());
    }

    let total_records = records.len();
    let total_definitions = definition_count(records);
    let mut definition_base = 0;
    for segment in segments {
        let definition_len = definition_count(&records[segment.clone()]);
        for record in &mut records[segment.clone()] {
            let tokens = std::sync::Arc::get_mut(&mut record.tokens).ok_or_else(|| {
                CodecError::malformed("SAT reference typing must own its token table")
            })?;
            for index in 0..tokens.len() {
                let reference = matches!(tokens.get(index), Some(Token::SubtypeOpen))
                    && (matches!(tokens.get(index + 1), Some(Token::Ident(name) | Token::SubIdent(name)) if name == "ref")
                        || matches!(
                            (tokens.get(index + 1), tokens.get(index + 2)),
                            (Some(Token::Long(_)), Some(Token::SubtypeClose))
                        ));
                if reference {
                    let value_index = if matches!(tokens.get(index + 1), Some(Token::Long(_))) {
                        index + 1
                    } else {
                        index + 2
                    };
                    if let Some(Token::Long(value)) = tokens.get_mut(value_index) {
                        *value = scoped_reference(
                            *value,
                            definition_base,
                            definition_len,
                            total_definitions,
                        )
                        .ok_or_else(|| {
                            CodecError::malformed(
                                "SAT subtype reference index overflows the combined table",
                            )
                        })?;
                    }
                }
                if let Token::Ref(value) = &mut tokens[index] {
                    *value = scoped_reference(*value, segment.start, segment.len(), total_records)
                        .ok_or_else(|| {
                            CodecError::malformed(
                                "SAT entity reference index overflows the combined table",
                            )
                        })?;
                }
            }
        }
        definition_base += definition_len;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
