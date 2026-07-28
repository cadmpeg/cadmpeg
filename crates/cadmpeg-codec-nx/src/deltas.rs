// SPDX-License-Identifier: Apache-2.0
//! Walk status-byte-framed Parasolid deltas records.
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeMap;

use cadmpeg_ir::be;

/// One complete admitted deltas record.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Parasolid node type.
    pub kind: u16,
    /// Stream-local XMT identifier.
    pub xmt: u32,
    /// Kernel node identifier. FIN and variable entity records do not carry one.
    pub node_id: Option<u32>,
    /// Ordered reference fields without their framing status bytes.
    pub references: Vec<u32>,
    /// POINT coordinates in Parasolid metres, when present.
    pub position: Option<[f64; 3]>,
    /// Partition-style bytes for fixed records and exact bytes for variable records.
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

/// One deltas BODY revision envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRevision {
    /// Monotonic kernel revision identity.
    pub node_id: u32,
    /// Eight ordered BODY references decoded from status-framed XMT fields.
    pub references: [u32; 8],
    /// Record start offset in the inflated deltas stream.
    pub offset: usize,
    /// First byte after the validated prefix.
    pub prefix_end: usize,
    /// First byte following the complete revision envelope.
    pub end: usize,
}

/// Count-selected binary64 lane immediately following one deltas `term_use`.
#[derive(Debug, Clone, PartialEq)]
pub struct TermUseNumericTail {
    /// XMT identity of the owning `term_use` record.
    pub term_use_xmt: u32,
    /// Serialized endpoint count selecting the numeric-tail cardinality.
    pub term_use_count: u32,
    /// Ordered finite binary64 values without assigned semantic roles.
    pub values: Vec<f64>,
    /// First numeric byte following the complete `term_use` record.
    pub offset: usize,
    /// First byte following the numeric lane.
    pub end: usize,
}

/// Maximal deltas gap containing only typed stream-local references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedReferenceLane {
    /// Ordered `(Parasolid record kind, XMT identity)` references.
    pub references: Vec<(u16, u32)>,
    /// First byte of the first tagged reference.
    pub offset: usize,
    /// First byte following the final tagged reference.
    pub end: usize,
}

/// One framed map from stream-local references to Parasolid type codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceTypeMap {
    /// Ordered `(reference, type_code)` entries.
    pub entries: Vec<(u32, u16)>,
    /// Type code of the map target.
    pub target_kind: u16,
    /// First byte of the map.
    pub offset: usize,
    /// First byte following the map.
    pub end: usize,
}

/// One deltas packet carrying four references and unassigned state words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceStatePacket {
    /// Four ordered stream-local XMT references.
    pub references: [u32; 4],
    /// Five ordered big-endian state words.
    pub state_words: [u32; 5],
    /// Terminal serialized state byte.
    pub state_byte: u8,
    /// First byte of the packet.
    pub offset: usize,
    /// First byte following the packet.
    pub end: usize,
}

/// One deltas packet carrying a reference and a serialized marker byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceMarkerPacket {
    /// Non-null stream-local XMT reference.
    pub reference: u32,
    /// Serialized marker byte.
    pub marker: u8,
    /// First byte of the packet.
    pub offset: usize,
    /// First byte following the packet.
    pub end: usize,
}

/// Body of an inline schema declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineSchemaFields {
    /// REGION declaration state.
    Region {
        /// Non-null stream-local declaration identity.
        xmt: u32,
        /// Serialized big-endian state word.
        state_word: u32,
        /// Four ordered stream-local XMT references.
        references: [u32; 4],
    },
    /// `ATTDEF_LIST` declaration state.
    AttdefList {
        /// Non-null stream-local declaration identity.
        xmt: u32,
        /// Number of serialized reference slots.
        slot_count: u32,
        /// Number of leading non-null slots.
        active_count: u32,
        /// Slot references, excluding the null sentinel.
        references: Vec<u32>,
    },
    /// Type 70 declaration state.
    Type70 {
        /// Non-null stream-local declaration identity.
        xmt: u32,
        /// Serialized node identity.
        node_id: u32,
        /// Four ordered body references.
        references: [u32; 4],
        /// Serialized declaration count.
        count: u16,
        /// Repeated terminal non-null reference.
        trailing_reference: u32,
    },
}

/// One complete inline schema declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineSchemaDeclaration {
    /// Schema-specific declaration body.
    pub fields: InlineSchemaFields,
    /// First byte of the declaration.
    pub offset: usize,
    /// First byte following the declaration.
    pub end: usize,
}

/// Result of a deterministic deltas record walk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Census {
    /// Complete records in source order.
    pub records: Vec<Record>,
    /// Compact tombstones in source order.
    pub tombstones: Vec<Tombstone>,
    /// BODY revision envelopes in source order.
    pub body_revisions: Vec<BodyRevision>,
    /// Complete count-selected numeric tails following `term_use` records.
    pub term_use_numeric_tails: Vec<TermUseNumericTail>,
    /// Maximal event gaps composed entirely of typed stream-local references.
    pub tagged_reference_lanes: Vec<TaggedReferenceLane>,
    /// Complete framed reference/type maps in source order.
    pub reference_type_maps: Vec<ReferenceTypeMap>,
    /// Complete four-reference state packets in source order.
    pub reference_state_packets: Vec<ReferenceStatePacket>,
    /// Complete reference-marker packets in source order.
    pub reference_marker_packets: Vec<ReferenceMarkerPacket>,
    /// Complete inline schema declarations in source order.
    pub inline_schema_declarations: Vec<InlineSchemaDeclaration>,
    /// Complete-record counts keyed by Parasolid family name.
    pub full_counts: BTreeMap<&'static str, usize>,
    /// Compact tombstone counts keyed by Parasolid family name.
    pub tombstone_counts: BTreeMap<&'static str, usize>,
    /// Sum of all admitted event bytes.
    pub bytes_decoded: usize,
}

impl Census {
    /// Return the sorted disjoint union of every admitted event byte span.
    pub(crate) fn covered_spans(&self) -> Vec<(usize, usize)> {
        merged_event_spans(self, true)
    }
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
const SHELL: &[Token] = &[Token::Ref; 8];
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

/// Walk all accepted records, revisions, tombstones, and numeric tails in an
/// inflated deltas stream.
pub fn walk(stream: &[u8]) -> Census {
    let mut census = Census::default();
    let mut offset = 0;
    while offset + 4 <= stream.len() {
        if let Some(record) = consume_shared_record(stream, offset, &census.records) {
            census.bytes_decoded += record.end - offset;
            let name =
                family_name(record.kind).expect("shared records have admitted deltas families");
            *census.full_counts.entry(name).or_default() += 1;
            offset = record.end;
            census.records.push(record);
            continue;
        }
        if let Some(record) = consume_intersection_auxiliary(stream, offset) {
            census.bytes_decoded += record.end - record.offset;
            let family = match record.kind {
                40 => "CHART",
                41 => "TERM_USE",
                59 => "BLEND_BOUND",
                204 => "SUPPORT_UV",
                _ => unreachable!("intersection auxiliary parser returns owned auxiliary types"),
            };
            *census.full_counts.entry(family).or_default() += 1;
            offset = record.end;
            census.records.push(record);
            continue;
        }
        if let Some(record) = consume_nurbs_auxiliary(stream, offset) {
            census.bytes_decoded += record.end - record.offset;
            let family = match record.kind {
                125 => "B_SURFACE_DATA",
                126 => "B_SURFACE_DESCRIPTOR",
                127 => "MULTIPLICITIES",
                128 => "KNOTS",
                135 => "B_CURVE_DATA",
                136 => "B_CURVE_DESCRIPTOR",
                _ => unreachable!("NURBS auxiliary parser returns owned auxiliary types"),
            };
            *census.full_counts.entry(family).or_default() += 1;
            offset = record.end;
            census.records.push(record);
            continue;
        }
        if let Some(record) = consume_type_141(stream, offset) {
            census.bytes_decoded += record.end - record.offset;
            *census.full_counts.entry("TYPE_141").or_default() += 1;
            offset = record.end;
            census.records.push(record);
            continue;
        }
        if let Some(record) = consume_type_45(stream, offset) {
            census.bytes_decoded += record.end - record.offset;
            *census.full_counts.entry("TYPE_45").or_default() += 1;
            offset = record.end;
            census.records.push(record);
            continue;
        }
        if let Some(record) = consume_type_70(stream, offset) {
            census.bytes_decoded += record.end - record.offset;
            *census.full_counts.entry("TYPE_70").or_default() += 1;
            offset = record.end;
            census.records.push(record);
            continue;
        }
        if let Some(record) = consume_attdef_list(stream, offset) {
            census.bytes_decoded += record.end - record.offset;
            *census.full_counts.entry("ATTDEF_LIST").or_default() += 1;
            offset = record.end;
            census.records.push(record);
            continue;
        }
        if let Some(record) = consume_type_101(stream, offset) {
            census.bytes_decoded += record.end - record.offset;
            *census.full_counts.entry("TYPE_101").or_default() += 1;
            offset = record.end;
            census.records.push(record);
            continue;
        }
        if let Some(record) = consume_intersection_data(stream, offset) {
            census.bytes_decoded += record.end - record.offset;
            *census.full_counts.entry("INTERSECTION_DATA").or_default() += 1;
            offset = record.end;
            census.records.push(record);
            continue;
        }
        let Some(kind) = be::u16_at(stream, offset) else {
            break;
        };
        let Some(name) = family_name(kind) else {
            offset += 1;
            continue;
        };
        if kind == 12 {
            if let Some(revision) = body_revision_prefix(stream, offset) {
                census.bytes_decoded += revision.prefix_end - revision.offset;
                offset = revision.prefix_end;
                census.body_revisions.push(revision);
                continue;
            }
        }
        let decoded = fixed_signature(kind)
            .and_then(|signature| consume_fixed(stream, offset, kind, signature))
            .or_else(|| consume_variable(stream, offset, kind));
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
                census.bytes_decoded += 6;
                offset += 6;
                continue;
            }
        }
        offset += 1;
    }
    census.term_use_numeric_tails = term_use_numeric_tails(stream, &census);
    census.bytes_decoded += census
        .term_use_numeric_tails
        .iter()
        .map(|tail| tail.end - tail.offset)
        .sum::<usize>();
    census.tagged_reference_lanes = tagged_reference_lanes(stream, &census);
    census.bytes_decoded += census
        .tagged_reference_lanes
        .iter()
        .map(|lane| lane.end - lane.offset)
        .sum::<usize>();
    census.reference_type_maps = reference_type_maps(stream, &census);
    census.bytes_decoded += census
        .reference_type_maps
        .iter()
        .map(|map| map.end - map.offset)
        .sum::<usize>();
    census.reference_state_packets = reference_state_packets(stream, &census);
    census.bytes_decoded += census
        .reference_state_packets
        .iter()
        .map(|packet| packet.end - packet.offset)
        .sum::<usize>();
    census.inline_schema_declarations = inline_schema_declarations(stream, &census);
    census.bytes_decoded += census
        .inline_schema_declarations
        .iter()
        .map(|declaration| declaration.end - declaration.offset)
        .sum::<usize>();
    census.reference_marker_packets = reference_marker_packets(stream, &census);
    census.bytes_decoded += census
        .reference_marker_packets
        .iter()
        .map(|packet| packet.end - packet.offset)
        .sum::<usize>();
    let body_revision_state_bytes = populate_body_revision_state_tails(stream, &mut census);
    census.bytes_decoded += body_revision_state_bytes;
    census
}

fn populate_body_revision_state_tails(stream: &[u8], census: &mut Census) -> usize {
    let tails = uncovered_spans(stream.len(), census, true)
        .filter_map(|(start, end)| {
            census
                .body_revisions
                .iter()
                .position(|revision| revision.prefix_end == start)
                .map(|index| (index, start, end))
        })
        .collect::<Vec<_>>();
    let mut byte_len = 0;
    for (index, start, end) in tails {
        let revision = &mut census.body_revisions[index];
        revision.end = end;
        byte_len += end - start;
    }
    byte_len
}

fn term_use_numeric_tails(stream: &[u8], census: &Census) -> Vec<TermUseNumericTail> {
    let mut event_starts = census
        .records
        .iter()
        .map(|record| record.offset)
        .chain(census.tombstones.iter().map(|tombstone| tombstone.offset))
        .chain(census.body_revisions.iter().map(|revision| revision.offset))
        .collect::<Vec<_>>();
    event_starts.sort_unstable();
    event_starts.dedup();

    census
        .records
        .iter()
        .filter(|record| record.kind == 41)
        .filter_map(|record| {
            let (term_use, parsed_end) = crate::intersection::term_use_at(stream, record.offset)?;
            (parsed_end == record.end && term_use.xmt == record.xmt).then_some(())?;
            let value_count = match term_use.count {
                1 => 8,
                2 => 19,
                _ => return None,
            };
            let end = record.end.checked_add(value_count * 8)?;
            let bytes = stream.get(record.end..end)?;
            let values = (0..value_count)
                .map(|ordinal| be::f64_at(bytes, ordinal * 8))
                .collect::<Option<Vec<_>>>()?;
            values.iter().all(|value| value.is_finite()).then_some(())?;
            let next_event =
                event_starts.get(event_starts.partition_point(|start| *start <= record.end));
            next_event
                .is_none_or(|start| *start >= end)
                .then_some(TermUseNumericTail {
                    term_use_xmt: record.xmt,
                    term_use_count: term_use.count,
                    values,
                    offset: record.end,
                    end,
                })
        })
        .collect()
}

fn tagged_reference_lanes(stream: &[u8], census: &Census) -> Vec<TaggedReferenceLane> {
    uncovered_spans(stream.len(), census, false)
        .filter_map(|(offset, end)| {
            let mut at = offset;
            let mut references = Vec::new();
            while at < end {
                let kind = be::u16_at(stream, at)?;
                is_tagged_reference_kind(kind).then_some(())?;
                let (xmt, consumed) = read_xmt(stream, at.checked_add(2)?)?;
                (xmt > 1).then_some(())?;
                at = at.checked_add(2 + consumed)?;
                (at <= end).then_some(())?;
                references.push((kind, xmt));
            }
            (at == end && !references.is_empty()).then_some(TaggedReferenceLane {
                references,
                offset,
                end,
            })
        })
        .collect()
}

fn reference_type_maps(stream: &[u8], census: &Census) -> Vec<ReferenceTypeMap> {
    uncovered_spans(stream.len(), census, true)
        .filter_map(|(offset, end)| reference_type_map(stream, offset, end))
        .collect()
}

fn reference_type_map(
    stream: &[u8],
    offset: usize,
    expected_end: usize,
) -> Option<ReferenceTypeMap> {
    let (leading_null, consumed) = read_xmt(stream, offset)?;
    (leading_null == 1).then_some(())?;
    let mut at = offset.checked_add(consumed)?;
    (be::u16_at(stream, at) == Some(1)).then_some(())?;
    at = at.checked_add(2)?;
    let mut entries = Vec::new();
    loop {
        (at < expected_end).then_some(())?;
        let (reference, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        (at <= expected_end).then_some(())?;
        if reference == 1 {
            (be::u16_at(stream, at) == Some(0)).then_some(())?;
            at = at.checked_add(2)?;
            let target_kind = be::u16_at(stream, at)?;
            is_reference_type_kind(target_kind).then_some(())?;
            at = at.checked_add(2)?;
            return (at == expected_end && !entries.is_empty()).then_some(ReferenceTypeMap {
                entries,
                target_kind,
                offset,
                end: at,
            });
        }
        let kind = be::u16_at(stream, at)?;
        is_reference_type_kind(kind).then_some(())?;
        at = at.checked_add(2)?;
        (at <= expected_end).then_some(())?;
        entries.push((reference, kind));
    }
}

fn reference_state_packets(stream: &[u8], census: &Census) -> Vec<ReferenceStatePacket> {
    uncovered_spans(stream.len(), census, true)
        .filter_map(|(offset, end)| reference_state_packet(stream, offset, end))
        .collect()
}

fn reference_state_packet(
    stream: &[u8],
    offset: usize,
    expected_end: usize,
) -> Option<ReferenceStatePacket> {
    (be::u16_at(stream, offset) == Some(1)
        && be::u16_at(stream, offset + 2) == Some(1)
        && be::u16_at(stream, offset + 4) == Some(4))
    .then_some(())?;
    let mut at = offset.checked_add(6)?;
    let mut references = [0; 4];
    for reference in &mut references {
        let (value, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        *reference = value;
    }
    (references[..3].iter().all(|reference| *reference > 1) && references[3] >= 1).then_some(())?;
    (be::u16_at(stream, at) == Some(1)).then_some(())?;
    at = at.checked_add(2)?;
    let mut state_words = [0; 5];
    for word in &mut state_words {
        *word = be::u32_at(stream, at)?;
        at = at.checked_add(4)?;
    }
    let state_byte = *stream.get(at)?;
    at = at.checked_add(1)?;
    (at == expected_end).then_some(ReferenceStatePacket {
        references,
        state_words,
        state_byte,
        offset,
        end: at,
    })
}

fn reference_marker_packets(stream: &[u8], census: &Census) -> Vec<ReferenceMarkerPacket> {
    uncovered_spans(stream.len(), census, true)
        .filter_map(|(offset, end)| reference_marker_packet(stream, offset, end))
        .collect()
}

const REGION_SCHEMA_HEADER: &[u8] = &[
    0x00, 0x13, 0x09, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x49, 0x05, 0x66, 0x72, 0x61, 0x6d, 0x65,
    0x00, 0xe6, 0x00, 0x01, 0x43, 0x41, 0x05, 0x6f, 0x77, 0x6e, 0x65, 0x72, 0x00, 0x0c, 0x00, 0x01,
    0x5a,
];

fn inline_schema_declarations(stream: &[u8], census: &Census) -> Vec<InlineSchemaDeclaration> {
    uncovered_spans(stream.len(), census, true)
        .flat_map(|(offset, gap_end)| {
            let mut declarations = Vec::new();
            let mut at = offset;
            while let Some(declaration) = inline_schema_declaration(stream, at, gap_end) {
                at = declaration.end;
                declarations.push(declaration);
            }
            declarations
        })
        .collect()
}

const ATTDEF_LIST_SCHEMA_HEADER: &[u8] = &[
    0x00, 0x4a, 0x04, 0x43, 0x49, 0x10, 0x69, 0x6e, 0x64, 0x65, 0x78, 0x5f, 0x6d, 0x61, 0x70, 0x5f,
    0x6f, 0x66, 0x66, 0x73, 0x65, 0x74, 0x00, 0x00, 0x00, 0x01, 0x01, 0x64, 0x43, 0x43, 0x5a,
];

const TYPE_70_SCHEMA_HEADER: &[u8] = &[
    0x00, 0x46, 0x0b, 0x43, 0x49, 0x09, 0x6c, 0x69, 0x73, 0x74, 0x5f, 0x74, 0x79, 0x70, 0x65, 0x00,
    0x00, 0x00, 0x01, 0x01, 0x75, 0x49, 0x0a, 0x6e, 0x6f, 0x74, 0x72, 0x61, 0x6e, 0x73, 0x6d, 0x69,
    0x74, 0x00, 0x00, 0x00, 0x01, 0x01, 0x6c, 0x43, 0x43, 0x43, 0x44, 0x43, 0x43, 0x44, 0x49, 0x0c,
    0x66, 0x69, 0x6e, 0x67, 0x65, 0x72, 0x5f, 0x69, 0x6e, 0x64, 0x65, 0x78, 0x00, 0x00, 0x00, 0x01,
    0x01, 0x64, 0x49, 0x0c, 0x66, 0x69, 0x6e, 0x67, 0x65, 0x72, 0x5f, 0x62, 0x6c, 0x6f, 0x63, 0x6b,
    0x03, 0xf4, 0x00, 0x01, 0x43, 0x5a,
];

fn inline_schema_declaration(
    stream: &[u8],
    offset: usize,
    gap_end: usize,
) -> Option<InlineSchemaDeclaration> {
    if stream.get(offset..offset.checked_add(REGION_SCHEMA_HEADER.len())?)
        == Some(REGION_SCHEMA_HEADER)
    {
        return region_schema_declaration(stream, offset, gap_end);
    }
    if stream.get(offset..offset.checked_add(ATTDEF_LIST_SCHEMA_HEADER.len())?)
        == Some(ATTDEF_LIST_SCHEMA_HEADER)
    {
        let body = offset.checked_add(ATTDEF_LIST_SCHEMA_HEADER.len())?;
        let (xmt, slot_count, active_count, references, end) = attdef_list_body(stream, body)?;
        (end <= gap_end).then_some(())?;
        return Some(InlineSchemaDeclaration {
            fields: InlineSchemaFields::AttdefList {
                xmt,
                slot_count,
                active_count,
                references: references.into_iter().skip(1).collect(),
            },
            offset,
            end,
        });
    }
    if stream.get(offset..offset.checked_add(TYPE_70_SCHEMA_HEADER.len())?)
        == Some(TYPE_70_SCHEMA_HEADER)
    {
        let body = offset.checked_add(TYPE_70_SCHEMA_HEADER.len())?;
        let (xmt, node_id, all_references, count, end) = type_70_body(stream, body)?;
        (end <= gap_end).then_some(())?;
        let references = all_references[..4].try_into().ok()?;
        return Some(InlineSchemaDeclaration {
            fields: InlineSchemaFields::Type70 {
                xmt,
                node_id,
                references,
                count,
                trailing_reference: all_references[4],
            },
            offset,
            end,
        });
    }
    None
}

fn region_schema_declaration(
    stream: &[u8],
    offset: usize,
    gap_end: usize,
) -> Option<InlineSchemaDeclaration> {
    let mut at = offset.checked_add(REGION_SCHEMA_HEADER.len())?;
    let (xmt, consumed) = read_xmt(stream, at)?;
    (xmt > 1).then_some(())?;
    at = at.checked_add(consumed)?;
    let state_word = be::u32_at(stream, at)?;
    at = at.checked_add(4)?;
    let mut references = [0; 4];
    for reference in &mut references {
        let (value, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        (stream.get(at) == Some(&1)).then_some(())?;
        at = at.checked_add(1)?;
        *reference = value;
    }
    (at <= gap_end).then_some(InlineSchemaDeclaration {
        fields: InlineSchemaFields::Region {
            xmt,
            state_word,
            references,
        },
        offset,
        end: at,
    })
}

fn reference_marker_packet(
    stream: &[u8],
    offset: usize,
    expected_end: usize,
) -> Option<ReferenceMarkerPacket> {
    let (reference, consumed) = read_xmt(stream, offset)?;
    (reference > 1).then_some(())?;
    let mut at = offset.checked_add(consumed)?;
    (stream.get(at) == Some(&1)).then_some(())?;
    at = at.checked_add(1)?;
    let (first_null, consumed) = read_xmt(stream, at)?;
    (first_null == 1).then_some(())?;
    at = at.checked_add(consumed)?;
    (stream.get(at) == Some(&1)).then_some(())?;
    at = at.checked_add(1)?;
    let marker = *stream.get(at)?;
    matches!(marker, 0x53 | 0x56).then_some(())?;
    at = at.checked_add(1)?;
    let (second_null, consumed) = read_xmt(stream, at)?;
    (second_null == 1).then_some(())?;
    at = at.checked_add(consumed)?;
    (stream.get(at) == Some(&1)).then_some(())?;
    at = at.checked_add(1)?;
    (at == expected_end).then_some(ReferenceMarkerPacket {
        reference,
        marker,
        offset,
        end: at,
    })
}

fn uncovered_spans(
    stream_len: usize,
    census: &Census,
    include_derived_events: bool,
) -> impl Iterator<Item = (usize, usize)> {
    let covered = merged_event_spans(census, include_derived_events);
    let mut gaps = Vec::new();
    let mut at = 0;
    for (start, end) in covered {
        if at < start {
            gaps.push((at, start));
        }
        at = at.max(end);
    }
    if at < stream_len {
        gaps.push((at, stream_len));
    }
    gaps.into_iter()
}

fn merged_event_spans(census: &Census, include_derived_events: bool) -> Vec<(usize, usize)> {
    let mut covered = census
        .records
        .iter()
        .map(|record| (record.offset, record.end))
        .chain(
            census
                .tombstones
                .iter()
                .map(|tombstone| (tombstone.offset, tombstone.offset + 6)),
        )
        .chain(
            census
                .body_revisions
                .iter()
                .map(|revision| (revision.offset, revision.end)),
        )
        .chain(
            census
                .term_use_numeric_tails
                .iter()
                .map(|tail| (tail.offset, tail.end)),
        )
        .collect::<Vec<_>>();
    if include_derived_events {
        covered.extend(
            census
                .tagged_reference_lanes
                .iter()
                .map(|lane| (lane.offset, lane.end)),
        );
        covered.extend(
            census
                .reference_type_maps
                .iter()
                .map(|map| (map.offset, map.end)),
        );
        covered.extend(
            census
                .reference_state_packets
                .iter()
                .map(|packet| (packet.offset, packet.end)),
        );
        covered.extend(
            census
                .reference_marker_packets
                .iter()
                .map(|packet| (packet.offset, packet.end)),
        );
        covered.extend(
            census
                .inline_schema_declarations
                .iter()
                .map(|declaration| (declaration.offset, declaration.end)),
        );
    }
    covered.sort_unstable();
    let mut merged = Vec::<(usize, usize)>::new();
    for (start, end) in covered {
        if let Some((_, merged_end)) = merged.last_mut().filter(|(_, end)| start <= *end) {
            *merged_end = (*merged_end).max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

fn is_tagged_reference_kind(kind: u16) -> bool {
    family_name(kind).is_some() || matches!(kind, 79 | 80)
}

fn is_reference_type_kind(kind: u16) -> bool {
    is_tagged_reference_kind(kind) || matches!(kind, 55 | 100)
}

fn consume_shared_record(stream: &[u8], offset: usize, records: &[Record]) -> Option<Record> {
    let previous = records.last()?;
    (previous.end == offset && has_shareable_terminal(previous)).then_some(())?;
    let record_offset = offset.checked_sub(1)?;
    if let Some(record) = consume_intersection_auxiliary(stream, record_offset)
        .or_else(|| consume_nurbs_auxiliary(stream, record_offset))
        .or_else(|| consume_type_141(stream, record_offset))
        .or_else(|| consume_type_45(stream, record_offset))
        .or_else(|| consume_type_70(stream, record_offset))
        .or_else(|| consume_attdef_list(stream, record_offset))
        .or_else(|| consume_type_101(stream, record_offset))
        .or_else(|| consume_intersection_data(stream, record_offset))
    {
        return Some(record);
    }
    let kind = u16::from(*stream.get(offset)?);
    family_name(kind)?;
    fixed_signature(kind)
        .and_then(|signature| consume_fixed(stream, record_offset, kind, signature))
        .or_else(|| consume_variable(stream, record_offset, kind))
}

fn has_shareable_terminal(record: &Record) -> bool {
    if record.kind == 84 {
        return true;
    }
    if record.kind != 81 || record.canonical_bytes.last() != Some(&0) {
        return false;
    }
    let bytes = &record.canonical_bytes;
    let mut at = 2 + usize::from(bytes.get(2) == Some(&0xff)) + 4;
    let Some((_, consumed)) = read_xmt(bytes, at) else {
        return false;
    };
    at += consumed + 6;
    bytes.get(at) == Some(&1)
}

fn body_revision_prefix(stream: &[u8], offset: usize) -> Option<BodyRevision> {
    let (xmt, consumed) = read_xmt(stream, offset + 2)?;
    (xmt == 3).then_some(())?;
    let node_id_at = offset + 2 + consumed;
    let node_id = be::u32_at(stream, node_id_at)?;
    let mut at = node_id_at + 4;
    let mut references = [0; 8];
    for reference in &mut references {
        let (value, consumed) = read_xmt(stream, at)?;
        at += consumed;
        (stream.get(at) == Some(&1)).then_some(())?;
        at += 1;
        *reference = value;
    }
    Some(BodyRevision {
        node_id,
        references,
        offset,
        prefix_end: at,
        end: at,
    })
}

/// Overlay supported complete deltas records onto one paired partition stream.
///
/// Replaced partition records are masked with non-tag bytes. Status-free
/// canonical current replacements are appended once. When BODY revision
/// envelopes are present, only records in the final revision contribute to the
/// current image. Raw current-revision deltas bytes remain available to
/// independent procedural decoders.
pub fn merge_full_records(partition: &[u8], deltas: &[u8]) -> Vec<u8> {
    let census = walk(deltas);
    let revision_start = current_revision_start(&census);
    let mut replacements = BTreeMap::<(u8, u32), &Record>::new();
    for record in census
        .records
        .iter()
        .filter(|record| record.offset >= revision_start)
    {
        let Ok(kind) = u8::try_from(record.kind) else {
            continue;
        };
        if mergeable_record(record, kind) {
            replacements.insert((kind, record.xmt), record);
        }
    }

    let mut tombstones = BTreeMap::new();
    for tombstone in census
        .tombstones
        .iter()
        .filter(|tombstone| tombstone.offset >= revision_start)
    {
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
/// and no earlier full-record addition in the current BODY revision.
///
/// Events are keyed by Parasolid type and XMT identity. A later full record
/// supersedes an earlier tombstone, while a full record followed by a
/// tombstone is a resolved deletion even when the base image lacked the key.
pub fn unmatched_terminal_tombstones(partition: &[u8], deltas: &[u8]) -> usize {
    unmatched_terminal_tombstones_by_family(partition, deltas)
        .values()
        .sum()
}

/// Count unmatched terminal tombstones by Parasolid record family.
pub fn unmatched_terminal_tombstones_by_family(
    partition: &[u8],
    deltas: &[u8],
) -> BTreeMap<&'static str, usize> {
    #[derive(Clone, Copy)]
    enum Event {
        Full { offset: usize },
        Tombstone { offset: usize },
    }

    let census = walk(deltas);
    let revision_start = current_revision_start(&census);
    let graph = crate::topology::Graph::parse(partition);
    let mut events = BTreeMap::<(u8, u32), Vec<Event>>::new();
    for record in census
        .records
        .into_iter()
        .filter(|record| record.offset >= revision_start)
    {
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
    for tombstone in census
        .tombstones
        .into_iter()
        .filter(|tombstone| tombstone.offset >= revision_start)
    {
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

    let mut unmatched = BTreeMap::new();
    for ((kind, xmt), mut events) in events {
        events.sort_by_key(|event| match event {
            Event::Full { offset } | Event::Tombstone { offset } => *offset,
        });
        let Some(Event::Tombstone { offset }) = events.last().copied() else {
            continue;
        };
        if graph.get(kind, xmt).is_none()
                && !events.iter().any(|event| {
                    matches!(event, Event::Full { offset: full_offset } if full_offset < &offset)
                })
            {
                let name = family_name(u16::from(kind))
                    .expect("event families originate from the accepted deltas census");
                *unmatched.entry(name).or_default() += 1;
            }
    }
    unmatched
}

fn mergeable_record(record: &Record, kind: u8) -> bool {
    matches!(
        kind,
        12..=19 | 29..=32 | 50..=54 | 56 | 60 | 124 | 133 | 134 | 137
    ) && crate::topology::Graph::parse(&record.canonical_bytes)
        .get(kind, record.xmt)
        .is_some()
}

fn current_revision_start(census: &Census) -> usize {
    census
        .body_revisions
        .last()
        .map_or(0, |revision| revision.offset)
}

/// Return raw deltas bytes with decoded records and compact tombstones masked.
/// Bytes preceding the final BODY revision are also masked. Current-revision
/// records needed by semantic scanners are appended in their partition form.
pub fn semantic_residual(stream: &[u8]) -> Vec<u8> {
    let census = walk(stream);
    let mut residual = stream.to_vec();
    let revision_start = current_revision_start(&census);
    residual[..revision_start].fill(0xff);
    let canonical_residual_records = census
        .records
        .iter()
        .filter(|record| {
            record.offset >= revision_start
                && matches!(
                    record.kind,
                    38
                        | 40
                        | 41
                        | 45
                        | 59
                        | 81..=84
                        | 91
                        | 125..=128
                        | 135..=136
                        | 141
                        | 204
                )
                || record.kind == 90 && record.canonical_bytes.first() == Some(&0x5a)
        })
        .map(|record| record.canonical_bytes.clone())
        .collect::<Vec<_>>();
    for record in census.records {
        residual[record.offset..record.end].fill(0xff);
    }
    for tombstone in census.tombstones {
        residual[tombstone.offset..tombstone.offset + 6].fill(0xff);
    }
    for record in canonical_residual_records {
        residual.extend_from_slice(&record);
    }
    residual
}

fn consume_fixed(stream: &[u8], offset: usize, kind: u16, signature: &[Token]) -> Option<Record> {
    let direct = fixed_layout(stream, offset, kind, signature, 0);
    let escaped = (stream.get(offset + 2) == Some(&0xff))
        .then(|| fixed_layout(stream, offset, kind, signature, 1))
        .flatten();
    match (direct, escaped) {
        (Some(direct), Some(escaped)) => unique_layout(
            plausible_next(stream, direct.end).then_some(direct),
            plausible_next(stream, escaped.end).then_some(escaped),
        ),
        (Some(record), None) | (None, Some(record)) => Some(record),
        (None, None) => None,
    }
}

fn fixed_layout(
    stream: &[u8],
    offset: usize,
    kind: u16,
    signature: &[Token],
    envelope_len: usize,
) -> Option<Record> {
    let xmt_at = offset.checked_add(2 + envelope_len)?;
    let (xmt, consumed) = read_xmt(stream, xmt_at)?;
    if xmt <= 1 {
        return None;
    }
    let mut at = xmt_at.checked_add(consumed)?;
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
                matches!(stream.get(at), Some(0 | 1)).then_some(())?;
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

fn consume_variable(stream: &[u8], offset: usize, kind: u16) -> Option<Record> {
    let (xmt, byte_len, references) = match kind {
        81 => {
            let record = crate::parasolid::entity_51_record_at(stream, offset)?;
            (record.xmt, record.byte_len, record.references)
        }
        82 => {
            let record = crate::parasolid::entity_52_integer_record_at(stream, offset)?;
            (record.xmt, record.byte_len, Vec::new())
        }
        83 => {
            let record = crate::parasolid::entity_53_double_record_at(stream, offset)?;
            (record.xmt, record.byte_len, Vec::new())
        }
        84 => {
            let record = crate::parasolid::entity_54_string_record_at(stream, offset)?;
            (record.xmt, record.byte_len, Vec::new())
        }
        90 => return consume_group(stream, offset),
        91 => return consume_type_91(stream, offset),
        _ => return None,
    };
    let end = offset.checked_add(byte_len)?;
    Some(Record {
        kind,
        xmt,
        node_id: None,
        references,
        position: None,
        canonical_bytes: stream
            .get(offset..end)
            .expect("validated variable record bounds")
            .to_vec(),
        offset,
        end,
    })
}

fn consume_group(stream: &[u8], offset: usize) -> Option<Record> {
    (be::u16_at(stream, offset) == Some(90)).then_some(())?;
    let direct = group_layout(stream, offset, 0);
    let escaped = (stream.get(offset + 2) == Some(&0xff))
        .then(|| group_layout(stream, offset, 1))
        .flatten();
    let (xmt, node_id, references, end) = unique_layout(direct, escaped)?;
    Some(Record {
        kind: 90,
        xmt,
        node_id: Some(node_id),
        references,
        position: None,
        canonical_bytes: stream.get(offset..end)?.to_vec(),
        offset,
        end,
    })
}

fn consume_attdef_list(stream: &[u8], offset: usize) -> Option<Record> {
    (be::u16_at(stream, offset) == Some(74)).then_some(())?;
    let direct = attdef_list_layout(stream, offset, 0);
    let escaped = (stream.get(offset + 2) == Some(&0xff))
        .then(|| attdef_list_layout(stream, offset, 1))
        .flatten();
    let (xmt, references, end) = unique_layout(direct, escaped)?;
    Some(Record {
        kind: 74,
        xmt,
        node_id: None,
        references,
        position: None,
        canonical_bytes: stream.get(offset..end)?.to_vec(),
        offset,
        end,
    })
}

fn consume_type_70(stream: &[u8], offset: usize) -> Option<Record> {
    (be::u16_at(stream, offset) == Some(70)).then_some(())?;
    let direct = type_70_layout(stream, offset, 0);
    let escaped = (stream.get(offset + 2) == Some(&0xff))
        .then(|| type_70_layout(stream, offset, 1))
        .flatten();
    let (xmt, node_id, references, end) = unique_layout(direct, escaped)?;
    Some(Record {
        kind: 70,
        xmt,
        node_id: Some(node_id),
        references,
        position: None,
        canonical_bytes: stream.get(offset..end)?.to_vec(),
        offset,
        end,
    })
}

fn type_70_layout(
    stream: &[u8],
    offset: usize,
    envelope_len: usize,
) -> Option<(u32, u32, Vec<u32>, usize)> {
    let body = offset.checked_add(2 + envelope_len)?;
    let (xmt, node_id, references, _, end) = type_70_body(stream, body)?;
    Some((xmt, node_id, references, end))
}

fn type_70_body(stream: &[u8], body: usize) -> Option<(u32, u32, Vec<u32>, u16, usize)> {
    let (xmt, consumed) = read_xmt(stream, body)?;
    (xmt > 1).then_some(())?;
    let mut at = body.checked_add(consumed)?;
    let node_id = be::u32_at(stream, at)?;
    at += 4;
    (stream.get(at) == Some(&4)).then_some(())?;
    at += 1;
    let mut references = Vec::new();
    for _ in 0..4 {
        (stream.get(at) == Some(&1)).then_some(())?;
        at += 1;
        let (reference, consumed) = read_xmt(stream, at)?;
        at += consumed;
        references.push(reference);
    }
    let count = be::u16_at(stream, at)?;
    (count > 0).then_some(())?;
    at += 2;
    (be::u32_at(stream, at) == Some(20)).then_some(())?;
    at += 4;
    (be::u32_at(stream, at) == Some(1)).then_some(())?;
    at += 4;
    for _ in 0..2 {
        let (reference, consumed) = read_xmt(stream, at)?;
        (reference > 1).then_some(())?;
        at += consumed;
        (stream.get(at) == Some(&0)).then_some(())?;
        at += 1;
        references.push(reference);
    }
    (references[4] == references[5]).then_some(())?;
    Some((xmt, node_id, references, count, at))
}

fn consume_type_101(stream: &[u8], offset: usize) -> Option<Record> {
    (be::u16_at(stream, offset) == Some(101)).then_some(())?;
    let direct = type_101_layout(stream, offset, 0);
    let escaped = (stream.get(offset + 2) == Some(&0xff))
        .then(|| type_101_layout(stream, offset, 1))
        .flatten();
    let (references, end) = unique_layout(direct, escaped)?;
    Some(Record {
        kind: 101,
        xmt: 2,
        node_id: None,
        references,
        position: None,
        canonical_bytes: stream.get(offset..end)?.to_vec(),
        offset,
        end,
    })
}

fn type_101_layout(stream: &[u8], offset: usize, envelope_len: usize) -> Option<(Vec<u32>, usize)> {
    let (xmt, consumed) = read_xmt(stream, offset.checked_add(2 + envelope_len)?)?;
    (xmt == 2).then_some(())?;
    let mut at = offset.checked_add(2 + envelope_len + consumed)?;
    let mut references = Vec::new();
    for _ in 0..12 {
        references.push(read_status_one_reference(stream, &mut at)?);
    }
    (stream.get(at) == Some(&1)).then_some(())?;
    at += 1;
    (stream.get(at..at + 12)? == [0; 12]).then_some(())?;
    at += 12;
    for _ in 0..3 {
        references.push(read_status_one_reference(stream, &mut at)?);
    }
    Some((references, at))
}

fn read_status_one_reference(stream: &[u8], at: &mut usize) -> Option<u32> {
    let (reference, consumed) = read_xmt(stream, *at)?;
    *at = at.checked_add(consumed)?;
    (stream.get(*at) == Some(&1)).then_some(())?;
    *at += 1;
    Some(reference)
}

fn attdef_list_layout(
    stream: &[u8],
    offset: usize,
    envelope_len: usize,
) -> Option<(u32, Vec<u32>, usize)> {
    let body = offset.checked_add(2 + envelope_len)?;
    let (xmt, _, _, references, end) = attdef_list_body(stream, body)?;
    Some((xmt, references, end))
}

fn attdef_list_body(stream: &[u8], body: usize) -> Option<(u32, u32, u32, Vec<u32>, usize)> {
    let slot_count_value = be::u32_at(stream, body)?;
    let slot_count = usize::try_from(slot_count_value).ok()?;
    (slot_count > 0).then_some(())?;
    let (xmt, consumed) = read_xmt(stream, body.checked_add(4)?)?;
    (xmt > 1).then_some(())?;
    let mut at = body.checked_add(4 + consumed)?;
    let active_count_value = be::u32_at(stream, at)?;
    let active_count = usize::try_from(active_count_value).ok()?;
    (active_count <= slot_count).then_some(())?;
    at += 4;
    (be::u32_at(stream, at) == Some(0)).then_some(())?;
    at += 4;
    (slot_count <= stream.len().saturating_sub(at) / 3).then_some(())?;
    let mut references = Vec::new();
    let (sentinel, consumed) = read_xmt(stream, at)?;
    (sentinel == 1).then_some(())?;
    at = at.checked_add(consumed)?;
    (stream.get(at) == Some(&1)).then_some(())?;
    at += 1;
    references.push(sentinel);
    for index in 0..slot_count {
        let (reference, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        if index < active_count {
            (reference > 1).then_some(())?;
        } else {
            (reference == 1).then_some(())?;
        }
        (stream.get(at) == Some(&1)).then_some(())?;
        at += 1;
        references.push(reference);
    }
    Some((xmt, slot_count_value, active_count_value, references, at))
}

fn group_layout(
    stream: &[u8],
    offset: usize,
    envelope_len: usize,
) -> Option<(u32, u32, Vec<u32>, usize)> {
    let (xmt, consumed) = read_xmt(stream, offset.checked_add(2 + envelope_len)?)?;
    (xmt > 1).then_some(())?;
    let mut at = offset.checked_add(2 + envelope_len + consumed)?;
    let node_id = be::u32_at(stream, at)?;
    at += 4;
    let mut references = Vec::new();
    for _ in 0..4 {
        let (reference, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        (stream.get(at) == Some(&1)).then_some(())?;
        at += 1;
        references.push(reference);
    }
    matches!(stream.get(at), Some(2 | 4 | 9)).then_some(())?;
    at += 1;
    let (reference, consumed) = read_xmt(stream, at)?;
    at = at.checked_add(consumed)?;
    matches!(stream.get(at), Some(0 | 1)).then_some(())?;
    at += 1;
    references.push(reference);
    Some((xmt, node_id, references, at))
}

fn consume_type_91(stream: &[u8], offset: usize) -> Option<Record> {
    (be::u16_at(stream, offset) == Some(91)).then_some(())?;
    let direct = type_91_layout(stream, offset, 0);
    let escaped_marker = stream.get(offset + 2) == Some(&0xff);
    let escaped = escaped_marker
        .then(|| type_91_layout(stream, offset, 1))
        .flatten();
    let (xmt, references, end) = if escaped_marker {
        escaped.or(direct)?
    } else {
        direct?
    };
    Some(Record {
        kind: 91,
        xmt,
        node_id: None,
        references,
        position: None,
        canonical_bytes: stream.get(offset..end)?.to_vec(),
        offset,
        end,
    })
}

fn type_91_layout(
    stream: &[u8],
    offset: usize,
    envelope_len: usize,
) -> Option<(u32, Vec<u32>, usize)> {
    let (xmt, consumed) = read_xmt(stream, offset.checked_add(2 + envelope_len)?)?;
    (xmt > 1).then_some(())?;
    let mut at = offset.checked_add(2 + envelope_len + consumed)?;
    matches!(be::u32_at(stream, at), Some(0 | 1)).then_some(())?;
    at += 4;
    let mut references = Vec::new();
    for _ in 0..6 {
        let (reference, consumed) = read_xmt(stream, at)?;
        (reference > 0).then_some(())?;
        at += consumed;
        matches!(stream.get(at), Some(0 | 1)).then_some(())?;
        at += 1;
        references.push(reference);
    }
    Some((xmt, references, at))
}

fn consume_type_141(stream: &[u8], offset: usize) -> Option<Record> {
    (be::u16_at(stream, offset) == Some(141)).then_some(())?;
    let direct = type_141_layout(stream, offset, 0);
    let escaped = (stream.get(offset + 2) == Some(&0xff))
        .then(|| type_141_layout(stream, offset, 1))
        .flatten();
    let (xmt, references, at) = unique_layout(direct, escaped)?;
    Some(Record {
        kind: 141,
        xmt,
        node_id: None,
        references,
        position: None,
        canonical_bytes: stream.get(offset..at)?.to_vec(),
        offset,
        end: at,
    })
}

fn consume_type_45(stream: &[u8], offset: usize) -> Option<Record> {
    (be::u16_at(stream, offset) == Some(45)).then_some(())?;
    let direct = type_45_layout(stream, offset, 0);
    let escaped = (stream.get(offset + 2) == Some(&0xff))
        .then(|| type_45_layout(stream, offset, 1))
        .flatten();
    let (xmt, end) = unique_layout(direct, escaped)?;
    Some(Record {
        kind: 45,
        xmt,
        node_id: None,
        references: Vec::new(),
        position: None,
        canonical_bytes: stream.get(offset..end)?.to_vec(),
        offset,
        end,
    })
}

fn type_45_layout(stream: &[u8], offset: usize, envelope_len: usize) -> Option<(u32, usize)> {
    let count_at = offset.checked_add(2 + envelope_len)?;
    let count = usize::try_from(be::u32_at(stream, count_at)?).ok()?;
    (count > 0).then_some(())?;
    let (xmt, xmt_len) = read_xmt(stream, count_at.checked_add(4)?)?;
    (xmt > 1).then_some(())?;
    let data_at = count_at.checked_add(4 + xmt_len)?;
    let value_count = count.checked_add(1)?;
    let end = data_at.checked_add(value_count.checked_mul(8)?)?;
    stream
        .get(data_at..end)?
        .chunks_exact(8)
        .all(|raw| {
            f64::from_be_bytes(
                raw.try_into()
                    .expect("chunks_exact(8) yields eight-byte slices"),
            )
            .is_finite()
        })
        .then_some((xmt, end))
}

fn type_141_layout(
    stream: &[u8],
    offset: usize,
    envelope_len: usize,
) -> Option<(u32, Vec<u32>, usize)> {
    let (xmt, consumed) = read_xmt(stream, offset.checked_add(2 + envelope_len)?)?;
    (xmt > 1).then_some(())?;
    let mut at = offset.checked_add(2 + envelope_len + consumed)?;
    let mut references = Vec::new();
    for required_status in [None, Some(0), Some(0), None] {
        let (reference, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        let status = *stream.get(at)?;
        required_status
            .map_or(matches!(status, 0 | 1), |required| status == required)
            .then_some(())?;
        at += 1;
        references.push(reference);
    }
    Some((xmt, references, at))
}

fn unique_layout<T>(direct: Option<T>, escaped: Option<T>) -> Option<T> {
    match (direct, escaped) {
        (Some(record), None) | (None, Some(record)) => Some(record),
        _ => None,
    }
}

fn consume_intersection_data(stream: &[u8], offset: usize) -> Option<Record> {
    let (curve, end) = crate::topology::intersection_data_curve_at(stream, offset)?;
    let mut references = curve.header_references.to_vec();
    references.extend(curve.references);
    Some(Record {
        kind: 90,
        xmt: curve.xmt,
        node_id: None,
        references,
        position: None,
        canonical_bytes: stream.get(offset..end)?.to_vec(),
        offset,
        end,
    })
}

fn consume_intersection_auxiliary(stream: &[u8], offset: usize) -> Option<Record> {
    let (kind, xmt, references, end) =
        if let Some((chart, end)) = crate::intersection::chart_source_record_at(stream, offset) {
            (40, chart.xmt, Vec::new(), end)
        } else if let Some((term, end)) = crate::intersection::term_use_at(stream, offset) {
            (41, term.xmt, Vec::new(), end)
        } else if let Some((bound, end)) = crate::intersection::blend_bound_at(stream, offset) {
            let mut references = bound.header_references.to_vec();
            references.extend([bound.boundary_index, bound.blend_surface]);
            (59, bound.xmt, references, end)
        } else {
            let (support_uv, end) = crate::intersection::support_uv_record_at(stream, offset)?;
            (204, support_uv.xmt, Vec::new(), end)
        };
    Some(Record {
        kind,
        xmt,
        node_id: None,
        references,
        position: None,
        canonical_bytes: stream.get(offset..end)?.to_vec(),
        offset,
        end,
    })
}

fn consume_nurbs_auxiliary(stream: &[u8], offset: usize) -> Option<Record> {
    let auxiliary = crate::nurbs::auxiliary_record_at(stream, offset)?;
    Some(Record {
        kind: auxiliary.kind,
        xmt: auxiliary.xmt,
        node_id: None,
        references: Vec::new(),
        position: None,
        canonical_bytes: stream.get(offset..auxiliary.end)?.to_vec(),
        offset,
        end: auxiliary.end,
    })
}

fn compact_tombstone(stream: &[u8], offset: usize) -> Option<u32> {
    let first = i16::from_be_bytes([*stream.get(offset + 2)?, *stream.get(offset + 3)?]);
    if first < 0 {
        let quotient = u16::from_be_bytes([*stream.get(offset + 4)?, *stream.get(offset + 5)?]);
        return (quotient == 1)
            .then_some(u32::from(quotient) * 32_767 + u32::from(first.unsigned_abs()));
    }
    (stream.get(offset + 4..offset + 6)? == [0, 1]).then_some(first as u32)
}

fn plausible_next(stream: &[u8], offset: usize) -> bool {
    if offset >= stream.len() {
        return true;
    }
    be::u16_at(stream, offset).is_some_and(is_next_kind)
}

fn is_next_kind(kind: u16) -> bool {
    family_name(kind).is_some() || matches!(kind, 70 | 79 | 80)
}

pub(crate) fn family_name(kind: u16) -> Option<&'static str> {
    Some(match kind {
        12 => "BODY",
        13 => "SHELL",
        14 => "FACE",
        15 => "LOOP",
        16 => "EDGE",
        17 => "FIN",
        18 => "VERTEX",
        19 => "REGION",
        29 => "POINT",
        30 => "LINE",
        31 => "CIRCLE",
        32 => "ELLIPSE",
        38 => "INTERSECTION",
        40 => "CHART",
        41 => "TERM_USE",
        45 => "TYPE_45",
        50 => "PLANE",
        51 => "CYLINDER",
        52 => "CONE",
        53 => "SPHERE",
        54 => "TORUS",
        56 => "BLEND_SURF",
        59 => "BLEND_BOUND",
        60 => "OFFSET_SURF",
        70 => "TYPE_70",
        74 => "ATTDEF_LIST",
        81 => "ENTITY_51",
        82 => "ENTITY_52",
        83 => "ENTITY_53",
        84 => "ENTITY_54",
        90 => "GROUP",
        91 => "TYPE_91",
        101 => "TYPE_101",
        124 => "B_SURFACE",
        125 => "B_SURFACE_DATA",
        126 => "B_SURFACE_DESCRIPTOR",
        127 => "MULTIPLICITIES",
        128 => "KNOTS",
        133 => "TRIMMED_CURVE",
        134 => "B_CURVE",
        135 => "B_CURVE_DATA",
        136 => "B_CURVE_DESCRIPTOR",
        137 => "SP_CURVE",
        141 => "TYPE_141",
        204 => "SUPPORT_UV",
        _ => return None,
    })
}

fn fixed_signature(kind: u16) -> Option<&'static [Token]> {
    Some(match kind {
        14 => FACE,
        13 => SHELL,
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

#[cfg(test)]
mod inline_schema_tests {
    use super::*;

    fn attdef_list_declaration() -> Vec<u8> {
        let mut bytes = ATTDEF_LIST_SCHEMA_HEADER.to_vec();
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&[0xe3, 0xbf, 0, 1]);
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        for reference in [1u16, 3, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(1);
        }
        bytes
    }

    fn type_70_declaration() -> Vec<u8> {
        let mut bytes = TYPE_70_SCHEMA_HEADER.to_vec();
        bytes.extend_from_slice(&7u16.to_be_bytes());
        bytes.extend_from_slice(&19u32.to_be_bytes());
        bytes.push(4);
        for reference in [2u16, 3, 1, 9] {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&20u32.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        for _ in 0..2 {
            bytes.extend_from_slice(&11u16.to_be_bytes());
            bytes.push(0);
        }
        bytes
    }

    #[test]
    fn adjacent_inline_schema_declarations_allow_either_order() {
        let attdef_list = attdef_list_declaration();
        let type_70 = type_70_declaration();

        for stream in [
            [attdef_list.as_slice(), type_70.as_slice()].concat(),
            [type_70.as_slice(), attdef_list.as_slice()].concat(),
        ] {
            let first = inline_schema_declaration(&stream, 0, stream.len())
                .expect("first declaration must be complete");
            let second = inline_schema_declaration(&stream, first.end, stream.len())
                .expect("second declaration must be complete");
            assert_eq!(second.end, stream.len());
            assert!(
                matches!(first.fields, InlineSchemaFields::AttdefList { .. })
                    || matches!(second.fields, InlineSchemaFields::AttdefList { .. })
            );
            assert!(
                matches!(first.fields, InlineSchemaFields::Type70 { .. })
                    || matches!(second.fields, InlineSchemaFields::Type70 { .. })
            );
        }
    }

    #[test]
    fn truncated_inline_schema_declaration_is_not_admitted() {
        for mut stream in [attdef_list_declaration(), type_70_declaration()] {
            stream.pop();
            assert!(walk(&stream).inline_schema_declarations.is_empty());
        }
    }
}

#[cfg(test)]
mod reference_type_map_tests {
    use super::*;

    fn map_bytes() -> Vec<u8> {
        vec![
            0, 1, 0, 1, 0xe3, 0xbf, 0, 1, 0, 81, 0, 3, 0, 100, 0, 1, 0, 0, 0, 55,
        ]
    }

    #[test]
    fn reference_type_map_accepts_compact_and_extended_references() {
        let bytes = map_bytes();
        let census = walk(&bytes);

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(40_000, 81), (3, 100)],
                target_kind: 55,
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn reference_type_map_requires_complete_framing_and_known_types() {
        let bytes = map_bytes();
        let mut unknown_entry_type = bytes.clone();
        unknown_entry_type[9] = 0xfe;
        let mut unknown_target_type = bytes.clone();
        unknown_target_type[19] = 0xfe;
        let trailing_byte = [bytes.as_slice(), &[0]].concat();

        for malformed in [
            &bytes[..bytes.len() - 1],
            unknown_entry_type.as_slice(),
            unknown_target_type.as_slice(),
            trailing_byte.as_slice(),
        ] {
            assert!(walk(malformed).reference_type_maps.is_empty());
        }
    }
}
