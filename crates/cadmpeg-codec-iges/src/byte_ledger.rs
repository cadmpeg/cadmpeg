// SPDX-License-Identifier: Apache-2.0
//! Parsed Fixed ASCII source-byte ownership.

use crate::card::{CardScan, PhysicalLine, Section};
use crate::parameter::ParameterRecord;
use cadmpeg_ir::{ByteLedger, ByteSpan, ByteSpanClass};
use std::collections::BTreeMap;

fn push(
    spans: &mut Vec<ByteSpan>,
    start: u64,
    end: u64,
    class: ByteSpanClass,
    owner: &str,
    meaning: &str,
    retained_record: Option<&str>,
) {
    if start == end {
        return;
    }
    spans.push(ByteSpan {
        start,
        end,
        class,
        owner: owner.into(),
        meaning: meaning.into(),
        retained_record: retained_record.map(str::to_owned),
    });
}

fn fixed_card_framing(spans: &mut Vec<ByteSpan>, line: &PhysicalLine, owner: &str) {
    let offset = line.offset;
    push(
        spans,
        offset + 72,
        offset + 73,
        ByteSpanClass::Structural,
        owner,
        "section_marker",
        None,
    );
    push(
        spans,
        offset + 73,
        offset + 80,
        ByteSpanClass::Structural,
        owner,
        "section_sequence",
        None,
    );
}

fn parameter_data(
    spans: &mut Vec<ByteSpan>,
    line: &PhysicalLine,
    record: &ParameterRecord,
    retained_card: &str,
) {
    let sequence = line.sequence.unwrap_or_default();
    let owner = format!("iges:parameter-record#D{}", record.directory_sequence);
    let line_index = sequence.saturating_sub(record.line_range.start) as usize;
    let assembled_start = line_index.saturating_mul(64);
    let comment_start = record.bytes.len().saturating_sub(record.comment.len());
    let mut column = 0_usize;
    while column < 64 {
        let assembled = assembled_start + column;
        let token_index = record
            .tokens
            .iter()
            .enumerate()
            .find(|(_, token)| token.span.contains(&assembled))
            .map(|(index, _)| index);
        let (class, meaning, retained_record) = if assembled >= comment_start {
            (
                ByteSpanClass::Opaque,
                "parameter_comment".into(),
                Some(retained_card),
            )
        } else if let Some(index) = token_index {
            (
                ByteSpanClass::Typed,
                format!("parameter_token_{index}"),
                None,
            )
        } else {
            (
                ByteSpanClass::Structural,
                "parameter_delimiter_or_padding".into(),
                None,
            )
        };
        let mut end = column + 1;
        while end < 64 {
            let next = assembled_start + end;
            let same = if class == ByteSpanClass::Opaque {
                next >= comment_start
            } else if class == ByteSpanClass::Typed {
                record
                    .tokens
                    .get(token_index.unwrap_or_default())
                    .is_some_and(|token| token.span.contains(&next))
            } else {
                next < comment_start
                    && !record.tokens.iter().any(|token| token.span.contains(&next))
            };
            if !same {
                break;
            }
            end += 1;
        }
        push(
            spans,
            line.offset + column as u64,
            line.offset + end as u64,
            class,
            &owner,
            &meaning,
            retained_record,
        );
        column = end;
    }
    push(
        spans,
        line.offset + 64,
        line.offset + 72,
        ByteSpanClass::Typed,
        &owner,
        "directory_back_pointer",
        None,
    );
    fixed_card_framing(spans, line, &owner);
}

pub(crate) fn build(scan: &CardScan, parameters: &[ParameterRecord]) -> ByteLedger {
    let parameter_by_line = parameters
        .iter()
        .flat_map(|record| record.line_range.clone().map(move |line| (line, record)))
        .collect::<BTreeMap<_, _>>();
    let mut spans = Vec::new();
    let mut terminated = false;
    for (index, line) in scan.lines.iter().enumerate() {
        let owner = format!("iges:physical:card#{}", index + 1);
        if terminated || line.payload.len() != 80 {
            push(
                &mut spans,
                line.offset,
                line.offset + line.payload.len() as u64,
                ByteSpanClass::Opaque,
                &owner,
                "post_terminate_bytes",
                Some(&owner),
            );
        } else {
            match line.section {
                Some(Section::Parameter) => {
                    if let Some(record) =
                        line.sequence.and_then(|line| parameter_by_line.get(&line))
                    {
                        parameter_data(&mut spans, line, record, &owner);
                    }
                }
                Some(section) => {
                    let (class, meaning, retained) = match section {
                        Section::Start => {
                            (ByteSpanClass::Opaque, "start_text", Some(owner.as_str()))
                        }
                        Section::Global => (ByteSpanClass::Typed, "global_data", None),
                        Section::Directory => (ByteSpanClass::Typed, "directory_fields", None),
                        Section::Terminate => (ByteSpanClass::Typed, "terminate_counts", None),
                        Section::Parameter => unreachable!("handled above"),
                    };
                    push(
                        &mut spans,
                        line.offset,
                        line.offset + 72,
                        class,
                        &owner,
                        meaning,
                        retained,
                    );
                    fixed_card_framing(&mut spans, line, &owner);
                }
                None => {}
            }
        }
        let payload_end = line.offset + line.payload.len() as u64;
        push(
            &mut spans,
            payload_end,
            payload_end + line.line_ending().len() as u64,
            ByteSpanClass::Structural,
            &owner,
            "line_ending",
            None,
        );
        terminated |= line.section == Some(Section::Terminate);
    }
    let mut ledger = ByteLedger {
        source_length: scan.source.len() as u64,
        spans,
    };
    ledger.finalize();
    ledger
}
