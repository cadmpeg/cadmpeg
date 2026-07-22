// SPDX-License-Identifier: Apache-2.0
//! Parsed Fixed ASCII source-byte ownership.

use crate::card::{CardScan, PhysicalLine, Section};
use crate::global::Global;
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
    let retained_record = if class == ByteSpanClass::Opaque {
        Some(format!("iges:opaque:bytes#{start}-{end}"))
    } else {
        retained_record.map(str::to_owned)
    };
    spans.push(ByteSpan {
        start,
        end,
        class,
        owner: owner.into(),
        meaning: meaning.into(),
        retained_record,
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

fn global_data(
    spans: &mut Vec<ByteSpan>,
    line: &PhysicalLine,
    global: &Global,
    line_index: usize,
    owner: &str,
) {
    let assembled_start = line_index.saturating_mul(72);
    let mut column = 0_usize;
    while column < 72 {
        let assembled = assembled_start + column;
        let value_index = global
            .value_spans
            .iter()
            .position(|span| span.contains(&assembled));
        let (class, meaning) = if let Some(index) = value_index {
            (ByteSpanClass::Typed, format!("global_value_{index}"))
        } else if assembled < global.record_end {
            (ByteSpanClass::Structural, "global_delimiter".into())
        } else {
            (ByteSpanClass::Structural, "global_padding".into())
        };
        let mut end = column + 1;
        while end < 72 {
            let next = assembled_start + end;
            let next_value = global
                .value_spans
                .iter()
                .position(|span| span.contains(&next));
            let same = if let Some(index) = value_index {
                next_value == Some(index)
            } else if assembled < global.record_end {
                next_value.is_none() && next < global.record_end
            } else {
                next >= global.record_end
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
            owner,
            &meaning,
            None,
        );
        column = end;
    }
    fixed_card_framing(spans, line, owner);
}

fn directory_data(spans: &mut Vec<ByteSpan>, line: &PhysicalLine, owner: &str) {
    const FIRST: [&str; 9] = [
        "entity_type",
        "parameter_start",
        "structure",
        "line_font",
        "level",
        "view",
        "transformation",
        "label_display",
        "status",
    ];
    const SECOND: [&str; 9] = [
        "entity_type",
        "line_weight",
        "color",
        "parameter_line_count",
        "form",
        "reserved_1",
        "reserved_2",
        "label",
        "subscript",
    ];
    let meanings = if line.sequence.unwrap_or_default() % 2 == 1 {
        FIRST
    } else {
        SECOND
    };
    for (index, meaning) in meanings.into_iter().enumerate() {
        let class = if meaning.starts_with("reserved_") {
            ByteSpanClass::Structural
        } else {
            ByteSpanClass::Typed
        };
        push(
            spans,
            line.offset + (index * 8) as u64,
            line.offset + ((index + 1) * 8) as u64,
            class,
            owner,
            meaning,
            None,
        );
    }
    fixed_card_framing(spans, line, owner);
}

fn terminate_data(spans: &mut Vec<ByteSpan>, line: &PhysicalLine, owner: &str) {
    for (index, meaning) in [
        "start_count",
        "global_count",
        "directory_count",
        "parameter_count",
    ]
    .into_iter()
    .enumerate()
    {
        push(
            spans,
            line.offset + (index * 8) as u64,
            line.offset + ((index + 1) * 8) as u64,
            ByteSpanClass::Typed,
            owner,
            meaning,
            None,
        );
    }
    push(
        spans,
        line.offset + 32,
        line.offset + 72,
        ByteSpanClass::Structural,
        owner,
        "terminate_padding",
        None,
    );
    fixed_card_framing(spans, line, owner);
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

pub(crate) fn build(
    scan: &CardScan,
    global: &Global,
    parameters: &[ParameterRecord],
) -> ByteLedger {
    let parameter_by_line = parameters
        .iter()
        .flat_map(|record| record.line_range.clone().map(move |line| (line, record)))
        .collect::<BTreeMap<_, _>>();
    let mut spans = Vec::new();
    let mut terminated = false;
    let mut global_line_index = 0_usize;
    for (index, line) in scan.lines.iter().enumerate() {
        let owner = format!("iges:physical:card#{}", index + 1);
        if terminated || line.payload.len() != 80 || line.section.is_none() {
            let meaning = if terminated {
                "post_terminate_bytes"
            } else {
                "noncanonical_physical_record"
            };
            push(
                &mut spans,
                line.offset,
                line.offset + line.payload.len() as u64,
                ByteSpanClass::Opaque,
                &owner,
                meaning,
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
                Some(Section::Global) => {
                    global_data(&mut spans, line, global, global_line_index, &owner);
                    global_line_index += 1;
                }
                Some(Section::Directory) => directory_data(&mut spans, line, &owner),
                Some(Section::Terminate) => terminate_data(&mut spans, line, &owner),
                Some(Section::Start) => {
                    push(
                        &mut spans,
                        line.offset,
                        line.offset + 72,
                        ByteSpanClass::Opaque,
                        &owner,
                        "start_text",
                        Some(owner.as_str()),
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
