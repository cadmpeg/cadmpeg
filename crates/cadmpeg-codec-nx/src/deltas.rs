// SPDX-License-Identifier: Apache-2.0
//! Walk status-byte-framed Parasolid deltas topology records.
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeMap;

use cadmpeg_ir::wire::be;

/// One complete status-framed topology record.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Parasolid node type.
    pub kind: u16,
    /// Stream-local XMT identifier.
    pub xmt: u32,
    /// Kernel node identifier; FIN records do not carry one.
    pub node_id: Option<u32>,
    /// Ordered reference fields without their framing status bytes.
    pub references: Vec<u32>,
    /// POINT coordinates in Parasolid metres, when present.
    pub position: Option<[f64; 3]>,
    /// Equivalent partition-style record with status bytes removed.
    pub canonical_bytes: Vec<u8>,
    /// Record start offset in the inflated stream.
    pub offset: usize,
    /// First byte following the record.
    pub end: usize,
}

/// One compact deletion carrying an explicit Parasolid type and XMT identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tombstone {
    /// Parasolid node type.
    pub kind: u16,
    /// Stream-local XMT identifier.
    pub xmt: u32,
    /// Record start offset in the inflated deltas stream.
    pub offset: usize,
}

/// Result of a deterministic deltas topology walk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Census {
    /// Complete records in source order.
    pub records: Vec<Record>,
    /// Compact tombstones in source order.
    pub tombstones: Vec<Tombstone>,
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
    OffsetDiscriminator,
    BlendSubtype,
    Boolean,
    Position,
    Vector,
    Scalar,
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
const LOOP: &[Token] = &[Token::Ref; 4];
const BODY_OR_SHELL: &[Token] = &[Token::Ref; 8];
const REGION: &[Token] = &[Token::Ref; 4];
const FIN: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
];
const LINE: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::Position,
    Token::Vector,
];
const PLANE: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::Position,
    Token::Vector,
    Token::Vector,
];
const CIRCLE: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::Position,
    Token::Vector,
    Token::Vector,
    Token::Scalar,
];
const ELLIPSE: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::Position,
    Token::Vector,
    Token::Vector,
    Token::Scalar,
    Token::Scalar,
];
const CYLINDER: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::Position,
    Token::Vector,
    Token::Scalar,
    Token::Vector,
];
const CONE: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::Position,
    Token::Vector,
    Token::Scalar,
    Token::Scalar,
    Token::Scalar,
    Token::Vector,
];
const SPHERE: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::Position,
    Token::Scalar,
    Token::Vector,
    Token::Vector,
];
const TORUS: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::Position,
    Token::Vector,
    Token::Scalar,
    Token::Scalar,
    Token::Vector,
];
const COMPACT_TWO_REFS: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::Ref,
    Token::Ref,
];
const OFFSET_SURFACE: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::OffsetDiscriminator,
    Token::Boolean,
    Token::Ref,
    Token::Scalar,
];
const BLEND_SURFACE: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::BlendSubtype,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Scalar,
    Token::Scalar,
    Token::Scalar,
    Token::Scalar,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
];
const TRIMMED_CURVE: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::Ref,
    Token::Position,
    Token::Position,
    Token::Scalar,
    Token::Scalar,
];
const SURFACE_CURVE: &[Token] = &[
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Sense,
    Token::Ref,
    Token::Ref,
    Token::Ref,
    Token::Tolerance,
];
const COMPOSITE_CURVE: &[Token] = &[
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
    Token::Ref,
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
            .filter(|record| plausible_next(stream, record.end));
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
                census.tombstones.push(Tombstone { kind, xmt, offset });
                offset += 6;
                continue;
            }
        }
        offset += 1;
    }
    census
}

/// Overlay supported complete deltas records onto one paired partition stream.
///
/// Replaced partition records are masked with non-tag bytes. Status-free
/// canonical current replacements are appended once. Raw deltas bytes remain
/// available to independent procedural decoders.
pub fn merge_full_records(partition: &[u8], deltas: &[u8]) -> Vec<u8> {
    let census = walk(deltas);
    let mut replacements = BTreeMap::<(u8, u32), &Record>::new();
    for record in &census.records {
        let Ok(kind) = u8::try_from(record.kind) else {
            continue;
        };
        if mergeable_record(record, kind) {
            replacements.insert((kind, record.xmt), record);
        }
    }

    let mut tombstones = BTreeMap::new();
    for tombstone in &census.tombstones {
        if let Ok(kind) = u8::try_from(tombstone.kind) {
            tombstones.insert((kind, tombstone.xmt), tombstone);
        }
    }

    let graph = crate::topology::Graph::parse(partition);
    let topology_carriers = graph.referenced_carrier_xmts();
    replacements.retain(|key, record| {
        tombstones
            .get(key)
            .is_none_or(|tombstone| record.offset > tombstone.offset)
    });
    let deletions = tombstones
        .into_iter()
        .filter(|(key, tombstone)| {
            graph.get(key.0, key.1).is_some()
                && !topology_carriers.contains(&key.1)
                && replacements
                    .get(key)
                    .is_none_or(|record| tombstone.offset > record.offset)
        })
        .collect::<BTreeMap<_, _>>();
    let build = |include_topology: bool| {
        let included = |kind: u8| include_topology || !matches!(kind, 12..=19);
        let mut merged = partition.to_vec();
        for &(kind, xmt) in replacements.keys().chain(deletions.keys()) {
            if included(kind) {
                if let Some(node) = graph.get(kind, xmt) {
                    merged[node.pos..node.end()].fill(0xff);
                }
            }
        }
        for (&(kind, _), record) in &replacements {
            if included(kind) {
                merged.extend_from_slice(&record.canonical_bytes);
            }
        }
        merged
    };
    if !graph.body_shape_shells().is_empty() {
        return build(false);
    }
    let merged = build(true);
    let merged_graph = crate::topology::Graph::parse(&merged);
    let base_complete = graph.has_complete_body_topology();
    let merged_complete = merged_graph.has_complete_body_topology();
    let deletes_owner = deletions.keys().any(|(kind, _)| matches!(kind, 12 | 13));
    let deleted_faces = deletions.keys().filter(|(kind, _)| *kind == 14).count();
    let unaccounted_face_loss = !deletes_owner
        && merged_graph
            .body_shape_face_count()
            .saturating_add(deleted_faces)
            < graph.body_shape_face_count();
    if base_complete && (!merged_complete || unaccounted_face_loss) {
        build(false)
    } else {
        merged
    }
}

/// Count terminal tombstones that have no exact carrier in the current image
/// and no earlier full-record addition in the same deltas stream.
///
/// Events are keyed by Parasolid type and XMT identity. A later full record
/// supersedes an earlier tombstone, while a full record followed by a
/// tombstone is a resolved deletion even when the base image lacked the key.
pub fn unmatched_terminal_tombstones(partition: &[u8], deltas: &[u8]) -> usize {
    #[derive(Clone, Copy)]
    enum Event {
        Full { offset: usize },
        Tombstone { offset: usize },
    }

    let census = walk(deltas);
    let graph = crate::topology::Graph::parse(partition);
    let mut events = BTreeMap::<(u8, u32), Vec<Event>>::new();
    for record in census.records {
        let Ok(kind) = u8::try_from(record.kind) else {
            continue;
        };
        if !mergeable_record(&record, kind) {
            continue;
        }
        events
            .entry((kind, record.xmt))
            .or_default()
            .push(Event::Full {
                offset: record.offset,
            });
    }
    for tombstone in census.tombstones {
        let Ok(kind) = u8::try_from(tombstone.kind) else {
            continue;
        };
        events
            .entry((kind, tombstone.xmt))
            .or_default()
            .push(Event::Tombstone {
                offset: tombstone.offset,
            });
    }

    events
        .into_iter()
        .filter_map(|((kind, xmt), mut events)| {
            events.sort_by_key(|event| match event {
                Event::Full { offset } | Event::Tombstone { offset } => *offset,
            });
            let Some(Event::Tombstone { offset }) = events.last().copied() else {
                return None;
            };
            (graph.get(kind, xmt).is_none()
                && !events.iter().any(|event| {
                    matches!(event, Event::Full { offset: full_offset } if full_offset < &offset)
                }))
            .then_some(())
        })
        .count()
}

fn mergeable_record(record: &Record, kind: u8) -> bool {
    matches!(
        kind,
        12..=19 | 29..=32 | 50..=54 | 56 | 60 | 124 | 133 | 134 | 137
    ) && crate::topology::Graph::parse(&record.canonical_bytes)
        .get(kind, record.xmt)
        .is_some()
}

/// Return raw deltas bytes with every decoded fixed record and compact
/// tombstone masked. Procedural families outside the fixed-record census keep
/// their original offsets and bytes.
pub fn procedural_residual(stream: &[u8]) -> Vec<u8> {
    let census = walk(stream);
    let mut residual = stream.to_vec();
    let canonical_procedural = census
        .records
        .iter()
        .filter(|record| record.kind == 38)
        .map(|record| record.canonical_bytes.clone())
        .collect::<Vec<_>>();
    for record in census.records {
        residual[record.offset..record.end].fill(0xff);
    }
    for tombstone in census.tombstones {
        residual[tombstone.offset..tombstone.offset + 6].fill(0xff);
    }
    for record in canonical_procedural {
        residual.extend_from_slice(&record);
    }
    residual
}

fn consume_fixed(stream: &[u8], offset: usize, kind: u16, signature: &[Token]) -> Option<Record> {
    let (xmt, consumed) = read_xmt(stream, offset + 2)?;
    if xmt <= 1 {
        return None;
    }
    let mut at = offset + 2 + consumed;
    let node_id = if kind == 17 {
        None
    } else {
        let node_id = be::u32_at(stream, at)?;
        at += 4;
        Some(node_id)
    };
    let mut canonical_bytes = stream.get(offset..at)?.to_vec();
    let mut references = Vec::new();
    let mut position = None;
    for token in signature {
        match token {
            Token::Ref => {
                let start = at;
                let (reference, consumed) = read_xmt(stream, at)?;
                at += consumed;
                if stream.get(at) != Some(&1) {
                    return None;
                }
                at += 1;
                canonical_bytes.extend_from_slice(stream.get(start..start + consumed)?);
                references.push(reference);
            }
            Token::Tolerance => {
                be::f64_at(stream, at)?.is_finite().then_some(())?;
                canonical_bytes.extend_from_slice(stream.get(at..at + 8)?);
                at += 8;
            }
            Token::Sense => {
                matches!(stream.get(at), Some(b'+' | b'-')).then_some(())?;
                canonical_bytes.push(*stream.get(at)?);
                at += 1;
            }
            Token::OffsetDiscriminator => {
                matches!(stream.get(at), Some(b'V' | b'I' | b'U')).then_some(())?;
                canonical_bytes.push(*stream.get(at)?);
                at += 1;
            }
            Token::BlendSubtype => {
                (stream.get(at) == Some(&b'R')).then_some(())?;
                canonical_bytes.push(b'R');
                at += 1;
            }
            Token::Boolean => {
                matches!(stream.get(at), Some(0 | 1)).then_some(())?;
                canonical_bytes.push(*stream.get(at)?);
                at += 1;
            }
            Token::Position => {
                let xyz = be::vec3_at(stream, at)?;
                xyz.iter().all(|value| value.is_finite()).then_some(())?;
                position = Some(xyz);
                canonical_bytes.extend_from_slice(stream.get(at..at + 24)?);
                at += 24;
            }
            Token::Vector => {
                let xyz = be::vec3_at(stream, at)?;
                xyz.iter().all(|value| value.is_finite()).then_some(())?;
                canonical_bytes.extend_from_slice(stream.get(at..at + 24)?);
                at += 24;
            }
            Token::Scalar => {
                be::f64_at(stream, at)?.is_finite().then_some(())?;
                canonical_bytes.extend_from_slice(stream.get(at..at + 8)?);
                at += 8;
            }
        }
    }
    Some(Record {
        kind,
        xmt,
        node_id,
        references,
        position,
        canonical_bytes,
        offset,
        end: at,
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
        15 => "LOOP",
        16 => "EDGE",
        17 => "FIN",
        18 => "VERTEX",
        29 => "POINT",
        30 => "LINE",
        31 => "CIRCLE",
        32 => "ELLIPSE",
        50 => "PLANE",
        51 => "CYLINDER",
        52 => "CONE",
        53 => "SPHERE",
        54 => "TORUS",
        56 => "BLEND_SURF",
        60 => "OFFSET_SURF",
        38 => "INTERSECTION",
        124 => "B_SURFACE",
        133 => "TRIMMED_CURVE",
        134 => "B_CURVE",
        137 => "SP_CURVE",
        12 => "BODY",
        13 => "SHELL",
        19 => "REGION",
        _ => return None,
    })
}

fn fixed_signature(kind: u16) -> Option<&'static [Token]> {
    Some(match kind {
        14 => FACE,
        12 | 13 => BODY_OR_SHELL,
        15 => LOOP,
        16 => EDGE,
        17 => FIN,
        18 => VERTEX,
        29 => POINT,
        30 => LINE,
        31 => CIRCLE,
        32 => ELLIPSE,
        50 => PLANE,
        51 => CYLINDER,
        52 => CONE,
        53 => SPHERE,
        54 => TORUS,
        56 => BLEND_SURFACE,
        60 => OFFSET_SURFACE,
        38 => COMPOSITE_CURVE,
        124 => COMPACT_TWO_REFS,
        133 => TRIMMED_CURVE,
        134 => COMPACT_TWO_REFS,
        137 => SURFACE_CURVE,
        19 => REGION,
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
