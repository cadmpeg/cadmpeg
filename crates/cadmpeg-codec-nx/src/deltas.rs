// SPDX-License-Identifier: Apache-2.0
//! Walk status-byte-framed Parasolid deltas topology records.

use std::collections::BTreeMap;

use cadmpeg_ir::be;

/// One complete status-framed topology record.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Parasolid node type.
    pub kind: u16,
    /// Stream-local XMT identifier.
    pub xmt: u32,
    /// Kernel node identifier.
    pub node_id: u32,
    /// Ordered reference fields without their framing status bytes.
    pub references: Vec<u32>,
    /// Record start offset in the inflated stream.
    pub offset: usize,
    /// First byte following the record.
    pub end: usize,
}

/// Result of a deterministic deltas topology walk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Census {
    /// Complete records in source order.
    pub records: Vec<Record>,
    /// Complete-record counts keyed by Parasolid family name.
    pub full_counts: BTreeMap<&'static str, usize>,
    /// Compact tombstone counts keyed by Parasolid family name.
    pub tombstone_counts: BTreeMap<&'static str, usize>,
    /// Sum of accepted complete-record byte lengths.
    pub bytes_decoded: usize,
}

#[derive(Debug, Clone, Copy)]
enum Token {
    Ref,
    Tolerance,
    Sense,
    Position,
}

const FACE: &[Token] = &[
    Token::Ref,
    Token::Tolerance,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
];
const EDGE: &[Token] = &[
    Token::Ref,
    Token::Tolerance,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
];
const VERTEX: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Tolerance,
    Token::Ref,
];
const POINT: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Position,
];

/// Walk all accepted full records and compact tombstones in an inflated
/// deltas stream.
pub fn walk(stream: &[u8]) -> Census {
    let mut census = Census::default();
    let mut offset = 0;
    while offset + 4 <= stream.len() {
        let Some(kind) = be::u16_at(stream, offset) else {
            break;
        };
        let Some(name) = family_name(kind) else {
            offset += 1;
            continue;
        };
        let decoded = fixed_signature(kind)
            .and_then(|signature| consume_fixed(stream, offset, kind, signature))
            .filter(|record| plausible_next(stream, record.end))
            .or_else(|| {
                variable_bounds(kind)
                    .and_then(|bounds| consume_variable(stream, offset, kind, bounds))
            });
        if let Some(record) = decoded {
            census.bytes_decoded += record.end - record.offset;
            *census.full_counts.entry(name).or_default() += 1;
            offset = record.end;
            census.records.push(record);
            continue;
        }
        if let Some(xmt) = compact_tombstone(stream, offset) {
            if xmt > 1 && plausible_next(stream, offset + 6) {
                *census.tombstone_counts.entry(name).or_default() += 1;
                offset += 6;
                continue;
            }
        }
        offset += 1;
    }
    census
}

fn consume_fixed(stream: &[u8], offset: usize, kind: u16, signature: &[Token]) -> Option<Record> {
    let (xmt, consumed) = read_xmt(stream, offset + 2)?;
    if xmt <= 1 {
        return None;
    }
    let mut at = offset + 2 + consumed;
    let node_id = be::u32_at(stream, at)?;
    at += 4;
    let mut references = Vec::new();
    for token in signature {
        match token {
            Token::Ref => {
                let (reference, consumed) = read_xmt(stream, at)?;
                at += consumed;
                if stream.get(at) != Some(&1) {
                    return None;
                }
                at += 1;
                references.push(reference);
            }
            Token::Tolerance => {
                be::f64_at(stream, at)?.is_finite().then_some(())?;
                at += 8;
            }
            Token::Sense => {
                matches!(stream.get(at), Some(b'+' | b'-')).then_some(())?;
                at += 1;
            }
            Token::Position => {
                let xyz = be::vec3_at(stream, at)?;
                xyz.iter().all(|value| value.is_finite()).then_some(())?;
                at += 24;
            }
        }
    }
    Some(Record {
        kind,
        xmt,
        node_id,
        references,
        offset,
        end: at,
    })
}

fn consume_variable(
    stream: &[u8],
    offset: usize,
    kind: u16,
    bounds: (usize, usize),
) -> Option<Record> {
    let (xmt, consumed) = read_xmt(stream, offset + 2)?;
    if xmt <= 1 {
        return None;
    }
    let mut at = offset + 2 + consumed;
    let node_id = be::u32_at(stream, at)?;
    at += 4;
    let mut references = Vec::new();
    let mut accepted = None;
    for _ in 0..bounds.1 {
        let Some((reference, consumed)) = read_xmt(stream, at) else {
            break;
        };
        let status = at + consumed;
        if stream.get(status) != Some(&1) {
            break;
        }
        references.push(reference);
        at = status + 1;
        if references.len() >= bounds.0 && plausible_next(stream, at) {
            accepted = Some((at, references.clone()));
        }
    }
    let (end, references) = accepted?;
    Some(Record {
        kind,
        xmt,
        node_id,
        references,
        offset,
        end,
    })
}

fn compact_tombstone(stream: &[u8], offset: usize) -> Option<u32> {
    (stream.get(offset + 4..offset + 6)? == [0, 1])
        .then(|| be::u16_at(stream, offset + 2).map(u32::from))
        .flatten()
}

fn plausible_next(stream: &[u8], offset: usize) -> bool {
    if offset >= stream.len() {
        return true;
    }
    be::u16_at(stream, offset).is_some_and(is_next_kind)
}

fn is_next_kind(kind: u16) -> bool {
    matches!(
        kind,
        12..=19 | 29..=32 | 38 | 40 | 41 | 51 | 56 | 60 | 81 | 90 | 91 | 124 | 133 | 134 | 137 | 204
    )
}

fn family_name(kind: u16) -> Option<&'static str> {
    Some(match kind {
        14 => "FACE",
        15 => "FIN",
        16 => "EDGE",
        17 => "LOOP",
        18 => "VERTEX",
        29 => "POINT",
        _ => return None,
    })
}

fn fixed_signature(kind: u16) -> Option<&'static [Token]> {
    Some(match kind {
        14 => FACE,
        16 => EDGE,
        18 => VERTEX,
        29 => POINT,
        _ => return None,
    })
}

fn variable_bounds(kind: u16) -> Option<(usize, usize)> {
    Some(match kind {
        15 => (2, 12),
        17 => (2, 8),
        _ => return None,
    })
}

fn read_xmt(stream: &[u8], at: usize) -> Option<(u32, usize)> {
    let first = i16::from_be_bytes([*stream.get(at)?, *stream.get(at + 1)?]);
    if first >= 0 {
        return Some((first as u32, 2));
    }
    let remainder = first.unsigned_abs();
    let quotient = u16::from_be_bytes([*stream.get(at + 2)?, *stream.get(at + 3)?]);
    Some((u32::from(quotient) * 32_767 + u32::from(remainder), 4))
}
