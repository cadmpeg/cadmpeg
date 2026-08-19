// SPDX-License-Identifier: Apache-2.0
//! Walk status-byte-framed Parasolid deltas records.
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeMap;

use crate::framing::read_xmt_width as read_xmt;
use crate::vec3_at::vec3_be_at;
use cadmpeg_core::decode::View;

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
    /// Stream-local BODY XMT identity.
    pub xmt: u32,
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

/// Framed Parasolid transmit header at the start of a deltas stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransmitHeader {
    /// Printable transmit-file description.
    pub description: String,
    /// Declared Parasolid schema token.
    pub schema: String,
    /// Consecutive stream-local header identities.
    pub references: [u32; 2],
    /// First byte following the header.
    pub end: usize,
}

/// Null references closing a deltas stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalNullReferences {
    /// First byte of the first null reference.
    pub offset: usize,
    /// Stream boundary following the final null reference.
    pub end: usize,
    /// Number of compact null references.
    pub count: u8,
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
    /// Type code of the optional terminal map target.
    pub target_kind: Option<u16>,
    /// First byte of the map.
    pub offset: usize,
    /// First byte following the map.
    pub end: usize,
}

/// One four-reference frame in a deltas state packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceStateFrame {
    /// Four ordered stream-local XMT references.
    pub references: [u32; 4],
    /// Five ordered big-endian state words.
    pub state_words: [u32; 5],
    /// Terminal serialized state byte.
    pub state_byte: u8,
}

/// One deltas packet carrying one or more reference-state frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceStatePacket {
    /// Ordered packet frames.
    pub frames: Vec<ReferenceStateFrame>,
    /// Whether the packet ends with `ref(1)[3], u32(1)`.
    pub terminal: bool,
    /// First byte of the packet.
    pub offset: usize,
    /// First byte following the packet.
    pub end: usize,
}

/// One deltas schema preamble carrying typed references and unassigned state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaReferencePreamble {
    /// Repeated serialized identity.
    pub identity: u16,
    /// Two consecutive non-null stream-local XMT references.
    pub references: [u32; 2],
    /// Three ordered state-lane XMT references.
    pub state_references: [u32; 3],
    /// Four ordered big-endian state words.
    pub state_words: [u32; 4],
    /// Serialized state count.
    pub count: u16,
    /// Ordered `(Parasolid record kind, XMT identity)` entries.
    pub entries: Vec<(u16, u32)>,
    /// Terminal serialized state value.
    pub terminal_value: u16,
    /// First byte of the preamble.
    pub offset: usize,
    /// First byte following the preamble.
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

/// One single-byte type-150 deltas state packet.
#[derive(Debug, Clone, PartialEq)]
pub struct Type150StatePacket {
    /// Five ordered stream-local XMT references.
    pub references: [u32; 5],
    /// Serialized state discriminator.
    pub marker: u8,
    /// Nine finite binary64 state values.
    pub values: [f64; 9],
    /// First byte of the packet.
    pub offset: usize,
    /// First byte following the packet.
    pub end: usize,
}

/// Body of an inline schema declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum InlineSchemaFields {
    /// Type 12 `BODY` schema header without following instance state.
    BodyHeader,
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
    /// Type 100 declaration and its precision state.
    Type100 {
        /// Non-null stream-local declaration identity.
        xmt: u32,
        /// Ordered precision-state references.
        references: [u32; 3],
        /// Serialized affine state.
        transform: [f64; 13],
    },
    /// Type 101 declaration and its schema-bound instance state.
    Type101 {
        /// Four ordered stream-local XMT references.
        references: [u32; 4],
        /// Optional non-null reference following the zero sentinel.
        anchor_reference: Option<u32>,
        /// Three serialized big-endian state words.
        state_words: [u32; 3],
        /// Terminal unsigned 40-bit state value.
        terminal_value: u64,
    },
    /// Type 101 declaration with the compact fixed state.
    Type101Compact,
    /// Type 38 intersection-data declaration state.
    Type38 {
        /// Non-null stream-local declaration identity.
        xmt: u32,
        /// Serialized node identity.
        node_id: u32,
        /// Five leading XMT references.
        leading_references: [u32; 5],
        /// Serialized statuses of the five leading references.
        leading_statuses: [u8; 5],
        /// Intersection-state discriminator.
        marker: u8,
        /// Non-null status-one XMT references selected by the marker.
        linked_references: Vec<u32>,
        /// Non-null status-zero declaration-state references.
        state_references: Vec<u32>,
        /// Eleven finite binary64 values from the optional nested term-use state.
        numeric_values: Option<[f64; 11]>,
    },
    /// Type 41 term-use declaration state.
    Type41 {
        /// Non-null stream-local term-use reference.
        reference: u32,
        /// Eleven finite binary64 state values.
        numeric_values: [f64; 11],
    },
}

/// One complete inline schema declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineSchemaDeclaration {
    /// Schema-specific declaration body.
    pub fields: InlineSchemaFields,
    /// First byte of the declaration.
    pub offset: usize,
    /// First byte following the declaration.
    pub end: usize,
}

/// Schema-bound type-12 `BODY` instance state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineBodyStateFields {
    /// Compact reference form followed by status zero.
    Compact {
        /// Non-null stream-local XMT reference.
        reference: u32,
    },
    /// Revision form with a bounded opaque state tail.
    Revision {
        /// Monotonic kernel revision identity.
        node_id: u32,
        /// Eight ordered status-framed XMT references.
        references: [u32; 8],
        /// Exact state bytes following the reference prefix.
        state_bytes: Vec<u8>,
    },
}

/// One complete schema-bound type-12 `BODY` instance state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineBodyState {
    /// Serialized state form.
    pub fields: InlineBodyStateFields,
    /// First state byte following a type-12 schema header.
    pub offset: usize,
    /// First byte following the state.
    pub end: usize,
}

/// Result of a deterministic deltas record walk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Census {
    /// Complete stream transmit header.
    pub transmit_header: Option<TransmitHeader>,
    /// Complete null-reference stream trailer.
    pub terminal_null_references: Option<TerminalNullReferences>,
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
    /// Complete schema reference preambles in source order.
    pub schema_reference_preambles: Vec<SchemaReferencePreamble>,
    /// Complete reference-marker packets in source order.
    pub reference_marker_packets: Vec<ReferenceMarkerPacket>,
    /// Complete single-byte type-150 state packets in source order.
    pub type_150_state_packets: Vec<Type150StatePacket>,
    /// Complete inline schema declarations in source order.
    pub inline_schema_declarations: Vec<InlineSchemaDeclaration>,
    /// Complete schema-bound type-12 BODY states in source order.
    pub inline_body_states: Vec<InlineBodyState>,
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
    let transmit_header = transmit_header(stream);
    let header_byte_len = transmit_header.as_ref().map_or(0, |header| header.end);
    let terminal_null_references = terminal_null_references(stream);
    let trailer_byte_len = terminal_null_references
        .as_ref()
        .map_or(0, |trailer| trailer.end - trailer.offset);
    let mut census = Census {
        transmit_header,
        terminal_null_references,
        bytes_decoded: header_byte_len + trailer_byte_len,
        ..Census::default()
    };
    let mut offset = census
        .transmit_header
        .as_ref()
        .map_or(0, |header| header.end);
    let mut intersection_schema_anchor_seen = false;
    while offset + 4 <= stream.len() {
        intersection_schema_anchor_seen |=
            crate::topology::intersection_data_schema_prefix_at(stream, offset);
        if let Some(preamble) = schema_reference_preamble(stream, offset, stream.len()) {
            census.bytes_decoded += preamble.end - preamble.offset;
            offset = preamble.end;
            census.schema_reference_preambles.push(preamble);
            continue;
        }
        if let Some(declaration) = inline_schema_declaration(stream, offset, stream.len()) {
            census.bytes_decoded += declaration.end - declaration.offset;
            offset = declaration.end;
            census.inline_schema_declarations.push(declaration);
            continue;
        }
        if let Some(map) =
            reference_type_map(stream, offset, ReferenceTypeMapLimit::TargetTerminated)
        {
            census.bytes_decoded += map.end - map.offset;
            offset = map.end;
            census.reference_type_maps.push(map);
            continue;
        }
        if let Some(record) = consume_shared_record(
            stream,
            offset,
            &census.records,
            intersection_schema_anchor_seen,
        ) {
            census.bytes_decoded += record.end - offset;
            let name =
                record_family_name(&record).expect("shared records have admitted deltas families");
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
        if let Some(record) = consume_type_67(stream, offset) {
            census.bytes_decoded += record.end - record.offset;
            *census.full_counts.entry("TYPE_67").or_default() += 1;
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
        if let Some(record) =
            consume_intersection_data(stream, offset, intersection_schema_anchor_seen)
        {
            census.bytes_decoded += record.end - record.offset;
            *census.full_counts.entry("INTERSECTION_DATA").or_default() += 1;
            offset = record.end;
            census.records.push(record);
            continue;
        }
        let Some(kind) = View::u16_be_at(stream, offset) else {
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
            if xmt > 1 {
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
    populate_gap_events(stream, &mut census);
    let body_revision_state_bytes = populate_body_revision_state_tails(stream, &mut census);
    census.bytes_decoded += body_revision_state_bytes;
    census
}

fn terminal_null_references(stream: &[u8]) -> Option<TerminalNullReferences> {
    const FOUR_REFERENCES: &[u8] = &[0, 1, 0, 1, 0, 1, 0, 1];
    const TWO_REFERENCES: &[u8] = &[0, 1, 0, 1];
    let (byte_len, count) = if stream.ends_with(FOUR_REFERENCES) {
        (FOUR_REFERENCES.len(), 4)
    } else if stream.ends_with(TWO_REFERENCES) {
        (TWO_REFERENCES.len(), 2)
    } else {
        return None;
    };
    Some(TerminalNullReferences {
        offset: stream.len().checked_sub(byte_len)?,
        end: stream.len(),
        count,
    })
}

fn populate_gap_events(stream: &[u8], census: &mut Census) {
    loop {
        let covered_before = merged_event_spans(census, true)
            .into_iter()
            .map(|(start, end)| end - start)
            .sum::<usize>();

        let lanes = tagged_reference_lanes(stream, census);
        census.tagged_reference_lanes.extend(lanes);

        let maps = reference_type_maps(stream, census);
        census.reference_type_maps.extend(maps);

        let state_packets = reference_state_packets(stream, census);
        census.reference_state_packets.extend(state_packets);

        let preambles = schema_reference_preambles(stream, census);
        census.schema_reference_preambles.extend(preambles);

        let declarations = inline_schema_declarations(stream, census);
        census.inline_schema_declarations.extend(declarations);

        let body_states = inline_body_states(stream, census);
        census.inline_body_states.extend(body_states);

        let marker_packets = reference_marker_packets(stream, census);
        census.reference_marker_packets.extend(marker_packets);

        let type_150_packets = type_150_state_packets(stream, census);
        census.type_150_state_packets.extend(type_150_packets);

        let covered_after = merged_event_spans(census, true)
            .into_iter()
            .map(|(start, end)| end - start)
            .sum::<usize>();
        let added_bytes = covered_after - covered_before;
        census.bytes_decoded += added_bytes;
        if added_bytes == 0 {
            break;
        }
    }

    census
        .tagged_reference_lanes
        .sort_unstable_by_key(|lane| lane.offset);
    census
        .reference_type_maps
        .sort_unstable_by_key(|map| map.offset);
    census
        .reference_state_packets
        .sort_unstable_by_key(|packet| packet.offset);
    census
        .schema_reference_preambles
        .sort_unstable_by_key(|preamble| preamble.offset);
    census
        .inline_schema_declarations
        .sort_unstable_by_key(|declaration| declaration.offset);
    census
        .inline_body_states
        .sort_unstable_by_key(|state| state.offset);
    census
        .reference_marker_packets
        .sort_unstable_by_key(|packet| packet.offset);
    census
        .type_150_state_packets
        .sort_unstable_by_key(|packet| packet.offset);
}

fn transmit_header(stream: &[u8]) -> Option<TransmitHeader> {
    (stream.get(..2) == Some(b"PS")).then_some(())?;
    let description_len = usize::try_from(View::u32_be_at(stream, 2)?).ok()?;
    (description_len > 0).then_some(())?;
    let description_start = 6usize;
    let description_end = description_start.checked_add(description_len)?;
    let description_bytes = stream.get(description_start..description_end)?;
    description_bytes
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        .then_some(())?;
    description_bytes
        .windows(b"(deltas)".len())
        .any(|window| window == b"(deltas)")
        .then_some(())?;

    let schema_len = usize::try_from(View::u32_be_at(stream, description_end)?).ok()?;
    (schema_len > 4).then_some(())?;
    let schema_start = description_end.checked_add(4)?;
    let schema_end = schema_start.checked_add(schema_len)?;
    let schema_bytes = stream.get(schema_start..schema_end)?;
    (schema_bytes.starts_with(b"SCH_")
        && schema_bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_'))
    .then_some(())?;

    let mut at = schema_end;
    (stream.get(at..at.checked_add(2)?) == Some([0x00, 0xe7].as_slice())).then_some(())?;
    at = at.checked_add(2)?;
    (View::u32_be_at(stream, at) == Some(0)).then_some(())?;
    at = at.checked_add(4)?;
    (View::u16_be_at(stream, at) == Some(3)).then_some(())?;
    at = at.checked_add(2)?;
    (stream.get(at) == Some(&0xff)).then_some(())?;
    at = at.checked_add(1)?;
    let (first, consumed) = read_xmt(stream, at)?;
    (first > 1).then_some(())?;
    at = at.checked_add(consumed)?;
    let (second, consumed) = read_xmt(stream, at)?;
    (second == first.checked_add(1)?).then_some(())?;
    at = at.checked_add(consumed)?;
    (View::u16_be_at(stream, at) == Some(0)).then_some(())?;
    at = at.checked_add(2)?;

    Some(TransmitHeader {
        description: String::from_utf8(description_bytes.to_vec()).ok()?,
        schema: String::from_utf8(schema_bytes.to_vec()).ok()?,
        references: [first, second],
        end: at,
    })
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
                .map(|ordinal| View::f64_be_at(bytes, ordinal * 8))
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
    uncovered_spans(stream.len(), census, true)
        .filter_map(|(offset, end)| {
            let mut at = offset;
            let mut references = Vec::new();
            while at < end {
                let kind = View::u16_be_at(stream, at)?;
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
        .filter_map(|(offset, end)| {
            reference_type_map(stream, offset, ReferenceTypeMapLimit::Bounded(end)).or_else(|| {
                let following_kind = census
                    .records
                    .iter()
                    .map(|record| (record.offset, record.kind))
                    .chain(
                        census
                            .tombstones
                            .iter()
                            .map(|tombstone| (tombstone.offset, tombstone.kind)),
                    )
                    .find_map(|(event_offset, kind)| (event_offset == end).then_some(kind))?;
                let shared_end = end.checked_add(2)?;
                let map =
                    reference_type_map(stream, offset, ReferenceTypeMapLimit::Bounded(shared_end))?;
                (map.target_kind.is_none()
                    && map
                        .entries
                        .last()
                        .is_some_and(|(_, kind)| *kind == following_kind))
                .then_some(map)
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
enum ReferenceTypeMapLimit {
    TargetTerminated,
    Bounded(usize),
}

fn reference_type_map(
    stream: &[u8],
    offset: usize,
    limit: ReferenceTypeMapLimit,
) -> Option<ReferenceTypeMap> {
    let expected_end = match limit {
        ReferenceTypeMapLimit::TargetTerminated => None,
        ReferenceTypeMapLimit::Bounded(end) => Some(end),
    };
    let mut at = if let Some((1, consumed)) = read_xmt(stream, offset) {
        let separator = offset.checked_add(consumed)?;
        (View::u16_be_at(stream, separator) == Some(1)).then_some(())?;
        separator.checked_add(2)?
    } else {
        (stream.get(offset) == Some(&1)).then_some(())?;
        let leading_null = offset.checked_add(1)?;
        let (reference, consumed) = read_xmt(stream, leading_null)?;
        (reference == 1).then_some(())?;
        leading_null.checked_add(consumed)?
    };
    let mut entries = Vec::new();
    loop {
        if expected_end == Some(at) {
            return (!entries.is_empty()).then_some(ReferenceTypeMap {
                entries,
                target_kind: None,
                offset,
                end: at,
            });
        }
        expected_end.is_none_or(|end| at < end).then_some(())?;
        let (reference, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        expected_end.is_none_or(|end| at <= end).then_some(())?;
        if reference == 1 {
            (View::u16_be_at(stream, at) == Some(0)).then_some(())?;
            at = at.checked_add(2)?;
            if expected_end == Some(at) {
                return (!entries.is_empty()).then_some(ReferenceTypeMap {
                    entries,
                    target_kind: None,
                    offset,
                    end: at,
                });
            }
            let target_kind = View::u16_be_at(stream, at)?;
            (target_kind > 0).then_some(())?;
            at = at.checked_add(2)?;
            return (expected_end.is_none_or(|end| at <= end) && !entries.is_empty()).then_some(
                ReferenceTypeMap {
                    entries,
                    target_kind: Some(target_kind),
                    offset,
                    end: at,
                },
            );
        }
        let kind = View::u16_be_at(stream, at)?;
        is_reference_type_kind(kind).then_some(())?;
        at = at.checked_add(2)?;
        expected_end.is_none_or(|end| at <= end).then_some(())?;
        entries.push((reference, kind));
    }
}

fn reference_state_packets(stream: &[u8], census: &Census) -> Vec<ReferenceStatePacket> {
    uncovered_spans(stream.len(), census, true)
        .flat_map(|(offset, gap_end)| {
            let mut packets = Vec::new();
            let mut at = offset;
            while let Some(packet) = reference_state_packet(stream, at, gap_end) {
                at = packet.end;
                packets.push(packet);
            }
            packets
        })
        .collect()
}

fn reference_state_packet(
    stream: &[u8],
    offset: usize,
    gap_end: usize,
) -> Option<ReferenceStatePacket> {
    (View::u16_be_at(stream, offset) == Some(1) && View::u16_be_at(stream, offset + 2) == Some(1))
        .then_some(())?;
    let mut at = offset.checked_add(4)?;
    let mut frames = Vec::new();
    while let Some((frame, end)) = reference_state_frame(stream, at, gap_end) {
        frames.push(frame);
        at = end;
    }
    (!frames.is_empty()).then_some(())?;
    let terminal_end = reference_state_terminal(stream, at, gap_end);
    let terminal = terminal_end.is_some();
    at = terminal_end.unwrap_or(at);
    Some(ReferenceStatePacket {
        frames,
        terminal,
        offset,
        end: at,
    })
}

fn reference_state_frame(
    stream: &[u8],
    offset: usize,
    gap_end: usize,
) -> Option<(ReferenceStateFrame, usize)> {
    (View::u16_be_at(stream, offset) == Some(4)).then_some(())?;
    let mut at = offset.checked_add(2)?;
    let mut references = [0; 4];
    for reference in &mut references {
        let (value, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        *reference = value;
    }
    let leading_non_null =
        references[..3].iter().all(|reference| *reference > 1) && references[3] >= 1;
    let interleaved_null =
        references[0] > 1 && references[1] == 1 && references[2] > 1 && references[3] == 1;
    (leading_non_null || interleaved_null).then_some(())?;
    (View::u16_be_at(stream, at) == Some(1)).then_some(())?;
    at = at.checked_add(2)?;
    let mut state_words = [0; 5];
    for word in &mut state_words {
        *word = View::u32_be_at(stream, at)?;
        at = at.checked_add(4)?;
    }
    let state_byte = *stream.get(at)?;
    at = at.checked_add(1)?;
    (at <= gap_end).then_some((
        ReferenceStateFrame {
            references,
            state_words,
            state_byte,
        },
        at,
    ))
}

fn reference_state_terminal(stream: &[u8], offset: usize, gap_end: usize) -> Option<usize> {
    let mut at = offset;
    for _ in 0..3 {
        let (reference, consumed) = read_xmt(stream, at)?;
        (reference == 1).then_some(())?;
        at = at.checked_add(consumed)?;
    }
    (View::u32_be_at(stream, at) == Some(1)).then_some(())?;
    at = at.checked_add(4)?;
    (at <= gap_end).then_some(at)
}

fn schema_reference_preambles(stream: &[u8], census: &Census) -> Vec<SchemaReferencePreamble> {
    uncovered_spans(stream.len(), census, true)
        .flat_map(|(offset, gap_end)| {
            let mut preambles = Vec::new();
            let mut at = offset;
            while let Some(preamble) = schema_reference_preamble(stream, at, gap_end) {
                at = preamble.end;
                preambles.push(preamble);
            }
            preambles
        })
        .collect()
}

fn schema_reference_preamble(
    stream: &[u8],
    offset: usize,
    gap_end: usize,
) -> Option<SchemaReferencePreamble> {
    let identity = View::u16_be_at(stream, offset)?;
    (identity > 1
        && View::u16_be_at(stream, offset.checked_add(2)?) == Some(4)
        && stream.get(offset.checked_add(4)?) == Some(&0xff))
    .then_some(())?;
    let mut at = offset.checked_add(5)?;
    let mut references = [0; 2];
    for reference in &mut references {
        let (value, consumed) = read_xmt(stream, at)?;
        (value > 1).then_some(())?;
        *reference = value;
        at = at.checked_add(consumed)?;
    }
    (references[1] == references[0].checked_add(1)?).then_some(())?;
    let mut state_references = [0; 3];
    for reference in &mut state_references {
        let (value, consumed) = read_xmt(stream, at)?;
        *reference = value;
        at = at.checked_add(consumed)?;
    }
    let linked_state = [1, references[1].checked_add(1)?, 1];
    (state_references == [1; 3] || state_references == linked_state).then_some(())?;
    let mut state_words = [0; 4];
    for state_word in &mut state_words {
        *state_word = View::u32_be_at(stream, at)?;
        at = at.checked_add(4)?;
    }
    (matches!(state_words[0], 0 | 2) && state_words[1] == 0 && state_words[2] == 1).then_some(())?;
    (stream.get(at..at.checked_add(3)?) == Some(&[0, 0, 0])).then_some(())?;
    at = at.checked_add(3)?;
    (View::u16_be_at(stream, at) == Some(identity)).then_some(())?;
    at = at.checked_add(2)?;
    for _ in 0..2 {
        let (reference, consumed) = read_xmt(stream, at)?;
        (reference == 1).then_some(())?;
        at = at.checked_add(consumed)?;
    }
    let count = View::u16_be_at(stream, at)?;
    (count > 0).then_some(())?;
    at = at.checked_add(2)?;
    let mut entries = Vec::new();
    loop {
        let entry_kind = View::u16_be_at(stream, at)?;
        matches!(entry_kind, 81 | 82).then_some(())?;
        at = at.checked_add(2)?;
        let (reference, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        if entry_kind == 82 && reference == 1 {
            (View::u16_be_at(stream, at) == Some(0)).then_some(())?;
            at = at.checked_add(2)?;
            let terminal_value = View::u16_be_at(stream, at)?;
            at = at.checked_add(2)?;
            return (at <= gap_end && !entries.is_empty()).then_some(SchemaReferencePreamble {
                identity,
                references,
                state_references,
                state_words,
                count,
                entries,
                terminal_value,
                offset,
                end: at,
            });
        }
        (reference > 1).then_some(())?;
        entries.push((entry_kind, reference));
    }
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

const BODY_SCHEMA_HEADER: &[u8] = &[
    0x00, 0x0c, 0x24, 0x43, 0x43, 0x43, 0x49, 0x07, 0x6c, 0x61, 0x74, 0x74, 0x69, 0x63, 0x65, 0x00,
    0xde, 0x00, 0x01, 0x43, 0x43, 0x43, 0x49, 0x04, 0x6d, 0x65, 0x73, 0x68, 0x03, 0xee, 0x00, 0x01,
    0x49, 0x08, 0x70, 0x6f, 0x6c, 0x79, 0x6c, 0x69, 0x6e, 0x65, 0x03, 0xf0, 0x00, 0x01, 0x43, 0x43,
    0x43, 0x43, 0x43, 0x43, 0x43, 0x44, 0x49, 0x05, 0x6f, 0x77, 0x6e, 0x65, 0x72, 0x04, 0x10, 0x00,
    0x01, 0x43, 0x43, 0x43, 0x49, 0x10, 0x62, 0x6f, 0x75, 0x6e, 0x64, 0x61, 0x72, 0x79, 0x5f, 0x6c,
    0x61, 0x74, 0x74, 0x69, 0x63, 0x65, 0x00, 0xde, 0x00, 0x01, 0x43, 0x43, 0x43, 0x49, 0x0d, 0x62,
    0x6f, 0x75, 0x6e, 0x64, 0x61, 0x72, 0x79, 0x5f, 0x6d, 0x65, 0x73, 0x68, 0x03, 0xee, 0x00, 0x01,
    0x49, 0x11, 0x62, 0x6f, 0x75, 0x6e, 0x64, 0x61, 0x72, 0x79, 0x5f, 0x70, 0x6f, 0x6c, 0x79, 0x6c,
    0x69, 0x6e, 0x65, 0x03, 0xf0, 0x00, 0x01, 0x43, 0x43, 0x43, 0x41, 0x10, 0x69, 0x6e, 0x64, 0x65,
    0x78, 0x5f, 0x6d, 0x61, 0x70, 0x5f, 0x6f, 0x66, 0x66, 0x73, 0x65, 0x74, 0x00, 0x00, 0x00, 0x01,
    0x01, 0x64, 0x41, 0x09, 0x69, 0x6e, 0x64, 0x65, 0x78, 0x5f, 0x6d, 0x61, 0x70, 0x00, 0x52, 0x00,
    0x01, 0x41, 0x11, 0x6e, 0x6f, 0x64, 0x65, 0x5f, 0x69, 0x64, 0x5f, 0x69, 0x6e, 0x64, 0x65, 0x78,
    0x5f, 0x6d, 0x61, 0x70, 0x00, 0x52, 0x00, 0x01, 0x41, 0x14, 0x73, 0x63, 0x68, 0x65, 0x6d, 0x61,
    0x5f, 0x65, 0x6d, 0x62, 0x65, 0x64, 0x64, 0x69, 0x6e, 0x67, 0x5f, 0x6d, 0x61, 0x70, 0x00, 0x52,
    0x00, 0x01, 0x41, 0x05, 0x63, 0x68, 0x69, 0x6c, 0x64, 0x00, 0x0c, 0x00, 0x01, 0x41, 0x0e, 0x6c,
    0x6f, 0x77, 0x65, 0x73, 0x74, 0x5f, 0x6e, 0x6f, 0x64, 0x65, 0x5f, 0x69, 0x64, 0x00, 0x00, 0x00,
    0x01, 0x01, 0x64, 0x41, 0x10, 0x6d, 0x65, 0x73, 0x68, 0x5f, 0x6f, 0x66, 0x66, 0x73, 0x65, 0x74,
    0x5f, 0x64, 0x61, 0x74, 0x61, 0x00, 0xce, 0x00, 0x01, 0x5a,
];

fn inline_schema_declarations(stream: &[u8], census: &Census) -> Vec<InlineSchemaDeclaration> {
    let covered = merged_event_spans(census, true);
    uncovered_spans(stream.len(), census, true)
        .flat_map(|(offset, gap_end)| {
            let parse_end = covered
                .iter()
                .position(|(start, end)| *start <= gap_end && gap_end < *end)
                .map_or(gap_end, |index| {
                    covered
                        .get(index + 1)
                        .map_or(stream.len(), |(start, _)| *start)
                });
            let mut declarations = Vec::new();
            let mut at = offset;
            while at < gap_end {
                let declaration = inline_schema_declaration(stream, at, gap_end).or_else(|| {
                    (parse_end > gap_end
                        && stream.get(at..at.checked_add(ATTDEF_LIST_SCHEMA_HEADER.len())?)
                            == Some(ATTDEF_LIST_SCHEMA_HEADER))
                    .then(|| inline_schema_declaration(stream, at, parse_end))
                    .flatten()
                });
                let Some(declaration) = declaration else {
                    break;
                };
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

const TYPE_100_SCHEMA_HEADER: &[u8] = &[
    0x00, 0x64, 0x0a, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x41, 0x09, 0x70, 0x72,
    0x65, 0x63, 0x69, 0x73, 0x69, 0x6f, 0x6e, 0x00, 0xe5, 0x00, 0x01, 0x5a,
];

const TYPE_41_SCHEMA_HEADER: &[u8] = &[
    0x00, 0x29, 0x03, 0x43, 0x49, 0x08, 0x74, 0x65, 0x72, 0x6d, 0x5f, 0x75, 0x73, 0x65, 0x00, 0x00,
    0x00, 0x01, 0x01, 0x63, 0x43, 0x5a,
];

const TYPE_101_SCHEMA_HEADER: &[u8] = &[
    0x00, 0x65, 0x13, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x49, 0x04, 0x6d, 0x65, 0x73, 0x68,
    0x03, 0xee, 0x00, 0x01, 0x49, 0x08, 0x70, 0x6f, 0x6c, 0x79, 0x6c, 0x69, 0x6e, 0x65, 0x03, 0xf0,
    0x00, 0x01, 0x49, 0x07, 0x6c, 0x61, 0x74, 0x74, 0x69, 0x63, 0x65, 0x00, 0xde, 0x00, 0x01, 0x43,
    0x43, 0x49, 0x0b, 0x61, 0x74, 0x74, 0x64, 0x65, 0x66, 0x5f, 0x6c, 0x69, 0x73, 0x74, 0x00, 0x4a,
    0x00, 0x01, 0x43, 0x43, 0x41, 0x10, 0x69, 0x6e, 0x64, 0x65, 0x78, 0x5f, 0x6d, 0x61, 0x70, 0x5f,
    0x6f, 0x66, 0x66, 0x73, 0x65, 0x74, 0x00, 0x00, 0x00, 0x01, 0x01, 0x64, 0x41, 0x09, 0x69, 0x6e,
    0x64, 0x65, 0x78, 0x5f, 0x6d, 0x61, 0x70, 0x00, 0x52, 0x00, 0x01, 0x41, 0x14, 0x73, 0x63, 0x68,
    0x65, 0x6d, 0x61, 0x5f, 0x65, 0x6d, 0x62, 0x65, 0x64, 0x64, 0x69, 0x6e, 0x67, 0x5f, 0x6d, 0x61,
    0x70, 0x00, 0x52, 0x00, 0x01, 0x41, 0x10, 0x6d, 0x65, 0x73, 0x68, 0x5f, 0x6f, 0x66, 0x66, 0x73,
    0x65, 0x74, 0x5f, 0x64, 0x61, 0x74, 0x61, 0x00, 0xce, 0x00, 0x01, 0x5a,
];

const TYPE_101_SCHEMA_STATE_PREFIX: &[u8] = &[
    0x00, 0x01, 0x01, 0x00, 0x01, 0x01, 0x00, 0x03, 0x01, 0x00, 0x04, 0x01, 0x00, 0x01, 0x01, 0x00,
    0x01, 0x01, 0x00, 0x01, 0x01, 0x00, 0x01, 0x01, 0x00, 0x01, 0x01, 0x00, 0x01, 0x01, 0x01, 0x00,
    0x01, 0x01, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x01, 0x00, 0x01, 0x01, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x01,
];

const TYPE_101_COMPACT_STATE_LEN: usize = 58;

fn inline_schema_declaration(
    stream: &[u8],
    offset: usize,
    gap_end: usize,
) -> Option<InlineSchemaDeclaration> {
    if stream.get(offset..offset.checked_add(BODY_SCHEMA_HEADER.len())?) == Some(BODY_SCHEMA_HEADER)
    {
        let end = offset.checked_add(BODY_SCHEMA_HEADER.len())?;
        (end <= gap_end).then_some(())?;
        return Some(InlineSchemaDeclaration {
            fields: InlineSchemaFields::BodyHeader,
            offset,
            end,
        });
    }
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
        let (xmt, node_id, all_references, count, end) = type_70_body(stream, body, 2)
            .filter(|(_, _, _, _, end)| *end <= gap_end)
            .or_else(|| {
                type_70_body(stream, body, 1).filter(|(_, _, _, _, end)| {
                    *end <= gap_end && (*end == gap_end || plausible_next(stream, *end))
                })
            })?;
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
    if stream.get(offset..offset.checked_add(TYPE_100_SCHEMA_HEADER.len())?)
        == Some(TYPE_100_SCHEMA_HEADER)
    {
        let mut at = offset.checked_add(TYPE_100_SCHEMA_HEADER.len())?;
        let (xmt, consumed) = read_xmt(stream, at)?;
        (xmt > 1).then_some(())?;
        at = at.checked_add(consumed)?;
        (View::u32_be_at(stream, at) == Some(0)).then_some(())?;
        at = at.checked_add(4)?;
        let mut references = [0; 3];
        for reference in &mut references {
            *reference = read_status_one_reference(stream, &mut at)?;
        }
        (references == [2, xmt.checked_add(1)?, 1]).then_some(())?;
        let mut transform = [0.0; 13];
        for (ordinal, transform_value) in transform.iter_mut().enumerate() {
            let value = View::f64_be_at(stream, at)?;
            let valid = match ordinal {
                0 | 4 | 8 | 12 => value.to_bits() == 1.0f64.to_bits(),
                9..=11 => value.is_finite(),
                _ => value.to_bits() == 0.0f64.to_bits(),
            };
            valid.then_some(())?;
            *transform_value = value;
            at = at.checked_add(8)?;
        }
        (View::u32_be_at(stream, at) == Some(1)).then_some(())?;
        at = at.checked_add(4)?;
        for _ in 0..3 {
            (View::u64_be_at(stream, at) == Some(0xc2bc_928f_996e_0000)).then_some(())?;
            at = at.checked_add(8)?;
        }
        (read_status_one_reference(stream, &mut at) == Some(1)).then_some(())?;
        (at <= gap_end).then_some(())?;
        return Some(InlineSchemaDeclaration {
            fields: InlineSchemaFields::Type100 {
                xmt,
                references,
                transform,
            },
            offset,
            end: at,
        });
    }
    if stream.get(offset..offset.checked_add(crate::topology::TYPE_38_SCHEMA_HEADER.len())?)
        == Some(crate::topology::TYPE_38_SCHEMA_HEADER)
    {
        let mut at = offset.checked_add(crate::topology::TYPE_38_SCHEMA_HEADER.len())?;
        let (xmt, consumed) = read_xmt(stream, at)?;
        (xmt > 1).then_some(())?;
        at = at.checked_add(consumed)?;
        let node_id = View::u32_be_at(stream, at)?;
        at = at.checked_add(4)?;
        let mut leading_references = [0; 5];
        let mut leading_statuses = [0; 5];
        for (reference, status) in leading_references.iter_mut().zip(&mut leading_statuses) {
            let (value, consumed) = read_xmt(stream, at)?;
            at = at.checked_add(consumed)?;
            *reference = value;
            *status = *stream.get(at)?;
            at = at.checked_add(1)?;
        }
        (leading_statuses[..4] == [1; 4] && matches!(leading_statuses[4], 0 | 1)).then_some(())?;
        (leading_statuses[4] == 1 || leading_references[4] > 1).then_some(())?;
        let marker = *stream.get(at)?;
        matches!(marker, 0x2b | 0x2d).then_some(())?;
        at = at.checked_add(1)?;
        if let Some((linked_references, state_references, state_end)) =
            type_38_reference_lanes(stream, at, 2, 3)
        {
            let descending_from_xmt = [
                xmt.checked_add(3)?,
                xmt.checked_add(2)?,
                xmt.checked_add(1)?,
            ];
            let prior_anchor = linked_references
                .iter()
                .chain(&leading_references)
                .copied()
                .max()?;
            let ascending_from_prior = [
                prior_anchor.checked_add(1)?,
                prior_anchor.checked_add(2)?,
                prior_anchor.checked_add(3)?,
            ];
            if stream.get(state_end..state_end.checked_add(TYPE_41_SCHEMA_HEADER.len())?)
                != Some(TYPE_41_SCHEMA_HEADER)
            {
                if leading_statuses[4] == 0 {
                    let anchor = leading_references[4];
                    (state_references.as_slice()
                        == [
                            anchor.checked_add(1)?,
                            anchor.checked_add(2)?,
                            anchor.checked_add(3)?,
                        ])
                    .then_some(())?;
                    (state_end <= gap_end).then_some(())?;
                    return Some(InlineSchemaDeclaration {
                        fields: InlineSchemaFields::Type38 {
                            xmt,
                            node_id,
                            leading_references,
                            leading_statuses,
                            marker,
                            linked_references,
                            state_references,
                            numeric_values: None,
                        },
                        offset,
                        end: state_end,
                    });
                }
                (state_references.as_slice() == descending_from_xmt
                    || state_references.as_slice() == ascending_from_prior)
                    .then_some(())?;
                (state_end <= gap_end).then_some(())?;
                return Some(InlineSchemaDeclaration {
                    fields: InlineSchemaFields::Type38 {
                        xmt,
                        node_id,
                        leading_references,
                        leading_statuses,
                        marker,
                        linked_references,
                        state_references,
                        numeric_values: None,
                    },
                    offset,
                    end: state_end,
                });
            }
            (leading_statuses == [1; 5]).then_some(())?;
            (state_references.as_slice() == descending_from_xmt).then_some(())?;
            let (term_reference, numeric_values, end) =
                type_41_schema_state(stream, state_end, gap_end)?;
            (term_reference == state_references[1]).then_some(())?;
            return Some(InlineSchemaDeclaration {
                fields: InlineSchemaFields::Type38 {
                    xmt,
                    node_id,
                    leading_references,
                    leading_statuses,
                    marker,
                    linked_references,
                    state_references,
                    numeric_values: Some(numeric_values),
                },
                offset,
                end,
            });
        }
        let (linked_references, state_references, state_end) =
            type_38_reference_lanes(stream, at, 1, 4)?;
        (leading_statuses == [1; 5]).then_some(())?;
        (state_references[2] == state_references[1].checked_add(1)?
            && state_references[3] == state_references[2].checked_add(1)?)
        .then_some(())?;
        (state_end <= gap_end).then_some(())?;
        return Some(InlineSchemaDeclaration {
            fields: InlineSchemaFields::Type38 {
                xmt,
                node_id,
                leading_references,
                leading_statuses,
                marker,
                linked_references,
                state_references,
                numeric_values: None,
            },
            offset,
            end: state_end,
        });
    }
    if stream.get(offset..offset.checked_add(TYPE_41_SCHEMA_HEADER.len())?)
        == Some(TYPE_41_SCHEMA_HEADER)
    {
        let (reference, numeric_values, end) = type_41_schema_state(stream, offset, gap_end)?;
        return Some(InlineSchemaDeclaration {
            fields: InlineSchemaFields::Type41 {
                reference,
                numeric_values,
            },
            offset,
            end,
        });
    }
    if stream.get(offset..offset.checked_add(TYPE_101_SCHEMA_HEADER.len())?)
        == Some(TYPE_101_SCHEMA_HEADER)
    {
        let mut at = offset.checked_add(TYPE_101_SCHEMA_HEADER.len())?;
        let (xmt, consumed) = read_xmt(stream, at)?;
        (xmt == 2).then_some(())?;
        at = at.checked_add(consumed)?;
        let compact_end = at.checked_add(TYPE_101_COMPACT_STATE_LEN)?;
        let compact = stream.get(at..compact_end)
            == TYPE_101_SCHEMA_STATE_PREFIX.get(..TYPE_101_COMPACT_STATE_LEN);
        let prefix = stream.get(at..at.checked_add(TYPE_101_SCHEMA_STATE_PREFIX.len())?);
        let prefix_state = prefix.map(|prefix| [prefix[7], prefix[10], prefix[30]]);
        let full_prefix = prefix_state.is_some_and(|state| {
            matches!(state, [3, 4, 1] | [1, 1, 0])
                && prefix.is_some_and(|prefix| {
                    prefix
                        .iter()
                        .zip(TYPE_101_SCHEMA_STATE_PREFIX)
                        .enumerate()
                        .all(|(index, (actual, expected))| {
                            matches!(index, 7 | 10 | 30) || actual == expected
                        })
                })
        });
        if !full_prefix {
            (compact && compact_end <= gap_end && plausible_next(stream, compact_end))
                .then_some(())?;
            return Some(InlineSchemaDeclaration {
                fields: InlineSchemaFields::Type101Compact,
                offset,
                end: compact_end,
            });
        }
        let prefix_state = prefix_state?;
        at = at.checked_add(TYPE_101_SCHEMA_STATE_PREFIX.len())?;
        (View::u16_be_at(stream, at) == Some(4)).then_some(())?;
        at = at.checked_add(2)?;
        let mut references = [0; 4];
        for reference in &mut references {
            let (value, consumed) = read_xmt(stream, at)?;
            at = at.checked_add(consumed)?;
            *reference = value;
        }
        let (null_reference, consumed) = read_xmt(stream, at)?;
        (null_reference == 1).then_some(())?;
        at = at.checked_add(consumed)?;
        (View::u16_be_at(stream, at) == Some(0)).then_some(())?;
        at = at.checked_add(2)?;
        let (anchor_reference, consumed) = read_xmt(stream, at)?;
        let anchor_reference = match anchor_reference {
            0 => None,
            value if value > 1 => Some(value),
            _ => return None,
        };
        at = at.checked_add(consumed)?;
        let mut state_words = [0; 3];
        for state_word in &mut state_words {
            *state_word = View::u32_be_at(stream, at)?;
            at = at.checked_add(4)?;
        }
        matches!(
            (prefix_state, state_words[0], state_words[1]),
            ([3, 4, 1], 19, 9) | ([1, 1, 0], 0, 0)
        )
        .then_some(())?;
        let terminal = stream.get(at..at.checked_add(5)?)?;
        let terminal_value = terminal
            .iter()
            .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte));
        at = at.checked_add(5)?;
        (at <= gap_end).then_some(())?;
        return Some(InlineSchemaDeclaration {
            fields: InlineSchemaFields::Type101 {
                references,
                anchor_reference,
                state_words,
                terminal_value,
            },
            offset,
            end: at,
        });
    }
    None
}

fn type_38_reference_lanes(
    stream: &[u8],
    offset: usize,
    linked_count: usize,
    state_count: usize,
) -> Option<(Vec<u32>, Vec<u32>, usize)> {
    let mut at = offset;
    let mut linked_references = Vec::new();
    for _ in 0..linked_count {
        let reference = read_status_one_reference(stream, &mut at)?;
        (reference > 1).then_some(())?;
        linked_references.push(reference);
    }
    let mut state_references = Vec::new();
    for _ in 0..state_count {
        let (reference, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        (stream.get(at) == Some(&0)).then_some(())?;
        at = at.checked_add(1)?;
        (reference > 1).then_some(())?;
        state_references.push(reference);
    }
    (read_status_one_reference(stream, &mut at) == Some(1)).then_some(())?;
    Some((linked_references, state_references, at))
}

fn type_41_schema_state(
    stream: &[u8],
    offset: usize,
    gap_end: usize,
) -> Option<(u32, [f64; 11], usize)> {
    (stream.get(offset..offset.checked_add(TYPE_41_SCHEMA_HEADER.len())?)
        == Some(TYPE_41_SCHEMA_HEADER))
    .then_some(())?;
    let mut at = offset.checked_add(TYPE_41_SCHEMA_HEADER.len())?;
    (View::u32_be_at(stream, at) == Some(1)).then_some(())?;
    at = at.checked_add(4)?;
    let (reference, consumed) = read_xmt(stream, at)?;
    (reference > 1).then_some(())?;
    at = at.checked_add(consumed)?;
    (stream.get(at..at.checked_add(2)?) == Some(&[0x4c, 0x3f])).then_some(())?;
    at = at.checked_add(2)?;
    let mut numeric_values = [0.0; 11];
    for value in &mut numeric_values {
        *value = View::f64_be_at(stream, at)?;
        value.is_finite().then_some(())?;
        at = at.checked_add(8)?;
    }
    (at <= gap_end).then_some(())?;
    Some((reference, numeric_values, at))
}

fn inline_body_states(stream: &[u8], census: &Census) -> Vec<InlineBodyState> {
    uncovered_spans(stream.len(), census, true)
        .filter(|(offset, _)| {
            census.inline_schema_declarations.iter().any(|declaration| {
                declaration.end == *offset && declaration.fields == InlineSchemaFields::BodyHeader
            })
        })
        .filter_map(|(offset, gap_end)| inline_body_state(stream, offset, gap_end))
        .collect()
}

fn inline_body_state(stream: &[u8], offset: usize, gap_end: usize) -> Option<InlineBodyState> {
    let next_header = ((offset + 1)..gap_end).find(|candidate| {
        [
            BODY_SCHEMA_HEADER,
            REGION_SCHEMA_HEADER,
            ATTDEF_LIST_SCHEMA_HEADER,
            TYPE_70_SCHEMA_HEADER,
            crate::topology::TYPE_38_SCHEMA_HEADER,
            TYPE_41_SCHEMA_HEADER,
            TYPE_100_SCHEMA_HEADER,
            TYPE_101_SCHEMA_HEADER,
        ]
        .iter()
        .any(|header| {
            stream.get(*candidate..candidate.saturating_add(header.len())) == Some(*header)
        })
    });
    let expected_end = next_header.unwrap_or(gap_end);
    let (first, consumed) = read_xmt(stream, offset)?;
    let mut at = offset.checked_add(consumed)?;
    if first > 1 && stream.get(at) == Some(&0) && at.checked_add(1) == Some(expected_end) {
        return Some(InlineBodyState {
            fields: InlineBodyStateFields::Compact { reference: first },
            offset,
            end: expected_end,
        });
    }
    (first == 3).then_some(())?;

    let node_id = View::u32_be_at(stream, at)?;
    at = at.checked_add(4)?;
    let mut references = [0; 8];
    for reference in &mut references {
        let (value, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        (stream.get(at) == Some(&1)).then_some(())?;
        at = at.checked_add(1)?;
        *reference = value;
    }
    (at < expected_end).then_some(())?;
    let state_bytes = stream.get(at..expected_end)?.to_vec();
    Some(InlineBodyState {
        fields: InlineBodyStateFields::Revision {
            node_id,
            references,
            state_bytes,
        },
        offset,
        end: expected_end,
    })
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
    let state_word = View::u32_be_at(stream, at)?;
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

fn type_150_state_packets(stream: &[u8], census: &Census) -> Vec<Type150StatePacket> {
    uncovered_spans(stream.len(), census, true)
        .filter_map(|(offset, end)| type_150_state_packet(stream, offset, end))
        .collect()
}

fn type_150_state_packet(
    stream: &[u8],
    offset: usize,
    expected_end: usize,
) -> Option<Type150StatePacket> {
    (stream.get(offset) == Some(&150)).then_some(())?;
    let mut at = offset.checked_add(1)?;
    let mut references = [0; 5];
    for (reference, required_status) in references.iter_mut().zip([1, 1, 0, 1, 0]) {
        let (value, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        (stream.get(at) == Some(&required_status)).then_some(())?;
        at = at.checked_add(1)?;
        *reference = value;
    }
    (references[0] == 1 && references[1..].iter().all(|reference| *reference > 1)).then_some(())?;
    let marker = *stream.get(at)?;
    matches!(marker, 0x2b | 0x2d).then_some(())?;
    at = at.checked_add(1)?;
    let mut values = [0.0; 9];
    for value in &mut values {
        *value = View::f64_be_at(stream, at)?;
        value.is_finite().then_some(())?;
        at = at.checked_add(8)?;
    }
    (at == expected_end).then_some(Type150StatePacket {
        references,
        marker,
        values,
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
        .transmit_header
        .iter()
        .map(|header| (0, header.end))
        .chain(
            census
                .terminal_null_references
                .iter()
                .map(|trailer| (trailer.offset, trailer.end)),
        )
        .chain(
            census
                .records
                .iter()
                .map(|record| (record.offset, record.end)),
        )
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
                .schema_reference_preambles
                .iter()
                .map(|preamble| (preamble.offset, preamble.end)),
        );
        covered.extend(
            census
                .reference_marker_packets
                .iter()
                .map(|packet| (packet.offset, packet.end)),
        );
        covered.extend(
            census
                .type_150_state_packets
                .iter()
                .map(|packet| (packet.offset, packet.end)),
        );
        covered.extend(
            census
                .inline_schema_declarations
                .iter()
                .map(|declaration| (declaration.offset, declaration.end)),
        );
        covered.extend(
            census
                .inline_body_states
                .iter()
                .map(|state| (state.offset, state.end)),
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
    is_tagged_reference_kind(kind) || matches!(kind, 11 | 35 | 55 | 61 | 67 | 100)
}

fn consume_shared_record(
    stream: &[u8],
    offset: usize,
    records: &[Record],
    intersection_schema_anchor_seen: bool,
) -> Option<Record> {
    let previous = records.last()?;
    (previous.end == offset && has_shareable_terminal(stream, previous)).then_some(())?;
    let record_offset = offset.checked_sub(1)?;
    if let Some(record) = consume_intersection_auxiliary(stream, record_offset)
        .or_else(|| consume_nurbs_auxiliary(stream, record_offset))
        .or_else(|| consume_type_141(stream, record_offset))
        .or_else(|| consume_type_45(stream, record_offset))
        .or_else(|| consume_type_70(stream, record_offset))
        .or_else(|| consume_attdef_list(stream, record_offset))
        .or_else(|| consume_type_101(stream, record_offset))
        .or_else(|| {
            consume_intersection_data(stream, record_offset, intersection_schema_anchor_seen)
        })
    {
        return Some(record);
    }
    let kind = u16::from(*stream.get(offset)?);
    family_name(kind)?;
    fixed_signature(kind)
        .and_then(|signature| consume_fixed(stream, record_offset, kind, signature))
        .or_else(|| consume_variable(stream, record_offset, kind))
}

fn has_shareable_terminal(stream: &[u8], record: &Record) -> bool {
    if fixed_signature(record.kind).is_some_and(|signature| {
        signature
            .last()
            .is_some_and(|token| matches!(token, Token::Ref))
    }) {
        return record
            .end
            .checked_sub(1)
            .and_then(|offset| stream.get(offset))
            == Some(&0);
    }
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
    (xmt > 1).then_some(())?;
    let node_id_at = offset + 2 + consumed;
    let node_id = View::u32_be_at(stream, node_id_at)?;
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
        xmt,
        node_id,
        references,
        offset,
        prefix_end: at,
        end: at,
    })
}

/// The result of applying one deltas stream to one partition image.
pub(crate) struct MergeFullRecordsResult {
    pub(crate) merged: Vec<u8>,
    pub(crate) unmatched_tombstones: BTreeMap<&'static str, usize>,
}

#[derive(Clone, Copy)]
enum MergeEvent {
    Full { offset: usize },
    Tombstone { offset: usize },
}

/// Overlay supported complete deltas records onto one paired partition stream.
///
/// Replaced partition records are masked with non-tag bytes. Status-free
/// canonical current replacements are appended once. When BODY revision
/// envelopes are present, only records in the current interval of each body
/// sequence contribute to the current image. Raw current-revision deltas bytes
/// remain available to independent procedural decoders.
#[cfg(test)]
pub fn merge_full_records(partition: &[u8], deltas: &[u8]) -> Vec<u8> {
    let census = walk(deltas);
    merge_full_records_with_census(partition, deltas, &census, false).merged
}

pub(crate) fn merge_full_records_with_census(
    partition: &[u8],
    deltas: &[u8],
    census: &Census,
    collect_unmatched_tombstones: bool,
) -> MergeFullRecordsResult {
    let current_scopes = current_revision_scopes(census, deltas.len());
    let mut replacements = BTreeMap::<(u8, u32), &Record>::new();
    let mut unmatched_events = collect_unmatched_tombstones.then(BTreeMap::new);
    for record in census
        .records
        .iter()
        .filter(|record| current_scope_contains(&current_scopes, record.offset))
    {
        let Ok(kind) = u8::try_from(record.kind) else {
            continue;
        };
        if mergeable_record(record, kind) {
            replacements.insert((kind, record.xmt), record);
            if let Some(events) = &mut unmatched_events {
                events
                    .entry((kind, record.xmt))
                    .or_insert_with(Vec::new)
                    .push(MergeEvent::Full {
                        offset: record.offset,
                    });
            }
        }
    }

    let mut tombstones = BTreeMap::new();
    for tombstone in census
        .tombstones
        .iter()
        .filter(|tombstone| current_scope_contains(&current_scopes, tombstone.offset))
    {
        if let Ok(kind) = u8::try_from(tombstone.kind) {
            tombstones.insert((kind, tombstone.xmt), tombstone);
            if let Some(events) = &mut unmatched_events {
                events
                    .entry((kind, tombstone.xmt))
                    .or_insert_with(Vec::new)
                    .push(MergeEvent::Tombstone {
                        offset: tombstone.offset,
                    });
            }
        }
    }

    let graph = crate::topology::Graph::parse(partition);
    let unmatched_tombstones = unmatched_events
        .map(|events| count_unmatched_events(events, &graph))
        .unwrap_or_default();
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
        return MergeFullRecordsResult {
            merged: build(false),
            unmatched_tombstones,
        };
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
        MergeFullRecordsResult {
            merged: build(false),
            unmatched_tombstones,
        }
    } else {
        MergeFullRecordsResult {
            merged,
            unmatched_tombstones,
        }
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
    let census = walk(deltas);
    let graph = crate::topology::Graph::parse(partition);
    count_unmatched_events(collect_unmatched_events(&census, deltas.len()), &graph)
}

fn collect_unmatched_events(
    census: &Census,
    stream_len: usize,
) -> BTreeMap<(u8, u32), Vec<MergeEvent>> {
    let current_scopes = current_revision_scopes(census, stream_len);
    let mut events = BTreeMap::<(u8, u32), Vec<MergeEvent>>::new();
    for record in census
        .records
        .iter()
        .filter(|record| current_scope_contains(&current_scopes, record.offset))
    {
        let Ok(kind) = u8::try_from(record.kind) else {
            continue;
        };
        if !mergeable_record(record, kind) {
            continue;
        }
        events
            .entry((kind, record.xmt))
            .or_default()
            .push(MergeEvent::Full {
                offset: record.offset,
            });
    }
    for tombstone in census
        .tombstones
        .iter()
        .filter(|tombstone| current_scope_contains(&current_scopes, tombstone.offset))
    {
        let Ok(kind) = u8::try_from(tombstone.kind) else {
            continue;
        };
        events
            .entry((kind, tombstone.xmt))
            .or_default()
            .push(MergeEvent::Tombstone {
                offset: tombstone.offset,
            });
    }
    events
}

fn count_unmatched_events(
    events: BTreeMap<(u8, u32), Vec<MergeEvent>>,
    graph: &crate::topology::Graph,
) -> BTreeMap<&'static str, usize> {
    let mut unmatched = BTreeMap::new();
    for ((kind, xmt), mut events) in events {
        events.sort_by_key(|event| match event {
            MergeEvent::Full { offset } | MergeEvent::Tombstone { offset } => *offset,
        });
        let Some(MergeEvent::Tombstone { offset }) = events.last().copied() else {
            continue;
        };
        if graph.get(kind, xmt).is_none()
            && !events.iter().any(|event| {
                matches!(event, MergeEvent::Full { offset: full_offset } if *full_offset < offset)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RevisionScope {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RevisionDirection {
    Ascending,
    Descending,
}

fn current_revision_scopes(census: &Census, stream_len: usize) -> Vec<RevisionScope> {
    // Only xmt 3 BODY envelopes delimit snapshots. Other validated type-12
    // envelopes remain available to the byte ledger without changing scope.
    let snapshot_revisions = census
        .body_revisions
        .iter()
        .enumerate()
        .filter(|(_, revision)| revision.xmt == 3)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if snapshot_revisions.is_empty() {
        return vec![RevisionScope {
            start: 0,
            end: stream_len,
        }];
    }

    let direction = revision_direction(
        &snapshot_revisions
            .iter()
            .map(|index| census.body_revisions[*index].node_id)
            .collect::<Vec<_>>(),
    );
    let mut run_starts = vec![0];
    for (position, pair) in snapshot_revisions.windows(2).enumerate() {
        let previous = census.body_revisions[pair[0]].node_id;
        let current = census.body_revisions[pair[1]].node_id;
        if !revision_follows_direction(previous, current, direction) {
            run_starts.push(position + 1);
        }
    }

    let mut scopes = Vec::new();
    for (run, &run_start) in run_starts.iter().enumerate() {
        let run_end = run_starts
            .get(run + 1)
            .copied()
            .unwrap_or(snapshot_revisions.len());
        let current_revision = &census.body_revisions[snapshot_revisions[run_end - 1]];
        let end = run_starts
            .get(run + 1)
            .map_or(stream_len, |next_run_start| {
                census.body_revisions[snapshot_revisions[*next_run_start]].offset
            });
        if current_revision.offset < end {
            scopes.push(RevisionScope {
                start: current_revision.offset,
                end,
            });
        }
        debug_assert!(run_start < run_end);
    }

    debug_assert!(scopes.windows(2).all(|pair| pair[0].end <= pair[1].start));
    scopes
}

fn revision_direction(node_ids: &[u32]) -> RevisionDirection {
    // A stream can serialize one revision sequence in either direction. The
    // direction with fewer violations is the sequence direction; the opposite
    // transitions are the resets that begin another sequence.
    let ascending_violations = node_ids.windows(2).filter(|pair| pair[1] < pair[0]).count();
    let descending_violations = node_ids.windows(2).filter(|pair| pair[1] > pair[0]).count();
    if ascending_violations <= descending_violations {
        RevisionDirection::Ascending
    } else {
        RevisionDirection::Descending
    }
}

fn revision_follows_direction(previous: u32, current: u32, direction: RevisionDirection) -> bool {
    match direction {
        RevisionDirection::Ascending => current >= previous,
        RevisionDirection::Descending => current <= previous,
    }
}

fn current_scope_contains(scopes: &[RevisionScope], offset: usize) -> bool {
    scopes
        .iter()
        .any(|scope| scope.start <= offset && offset < scope.end)
}

/// Return raw deltas bytes with decoded records and compact tombstones masked.
/// Historical BODY revision intervals are also masked. Current-revision records
/// needed by semantic scanners are appended in their partition form.
#[cfg(test)]
pub fn semantic_residual(stream: &[u8]) -> Vec<u8> {
    let census = walk(stream);
    semantic_residual_with_census(stream, &census)
}

/// Return the semantic residual using a census already produced for the stream.
///
/// The census owns the complete-record boundaries and canonical forms used by
/// both topology merging and semantic scanning. Reusing it avoids a second
/// full walk of a large delta stream while keeping this transformation
/// byte-for-byte identical to `semantic_residual`.
pub(crate) fn semantic_residual_with_census(stream: &[u8], census: &Census) -> Vec<u8> {
    let mut residual = stream.to_vec();
    let current_scopes = current_revision_scopes(census, stream.len());
    let mut cursor = 0;
    for scope in &current_scopes {
        residual[cursor..scope.start].fill(0xff);
        cursor = scope.end;
    }
    residual[cursor..].fill(0xff);
    let canonical_residual_records = census
        .records
        .iter()
        .filter(|record| {
            let is_current = current_scope_contains(&current_scopes, record.offset);
            let is_semantic = matches!(
                record.kind,
                38 | 40 | 41 | 45 | 59 | 81..=84 | 91 | 125..=128 | 135..=136 | 141 | 204
            ) || record.kind == 90
                && record.canonical_bytes.first() == Some(&0x5a);
            is_current && is_semantic
        })
        .map(|record| {
            if record.kind == 90 && record.canonical_bytes.first() == Some(&0x5a) {
                let prefix_len = crate::topology::TYPE_38_SCHEMA_HEADER.len() - 1;
                let mut anchored = Vec::new();
                anchored.extend_from_slice(&crate::topology::TYPE_38_SCHEMA_HEADER[..prefix_len]);
                anchored.extend_from_slice(&record.canonical_bytes);
                anchored
            } else {
                record.canonical_bytes.clone()
            }
        })
        .collect::<Vec<_>>();
    for record in &census.records {
        residual[record.offset..record.end].fill(0xff);
    }
    for tombstone in &census.tombstones {
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
    let record = match (direct, escaped) {
        (Some(direct), Some(escaped)) => unique_layout(
            plausible_next(stream, direct.end).then_some(direct),
            plausible_next(stream, escaped.end).then_some(escaped),
        ),
        (Some(record), None) | (None, Some(record)) => Some(record),
        (None, None) => None,
    }?;
    let shadows_type_101 = (record.offset + 1..record.end)
        .any(|offset| consume_type_101(stream, offset).is_some_and(|later| later.end > record.end));
    (!shadows_type_101).then_some(record)
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
        let node_id = View::u32_be_at(stream, at)?;
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
                let tolerance = View::f64_be_at(stream, at)?;
                (tolerance.is_finite()
                    && (!matches!(kind, 16 | 18) || tolerance.abs() >= 1.0e-100))
                    .then_some(())?;
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
                let xyz = vec3_be_at(stream, at)?;
                xyz.iter()
                    .all(|value| {
                        value.is_finite() && (kind != 29 || *value == 0.0 || value.is_normal())
                    })
                    .then_some(())?;
                position = Some(xyz);
                canonical_bytes.extend_from_slice(stream.get(at..at + 24)?);
                at += 24;
            }
            Token::Vector => {
                let xyz = vec3_be_at(stream, at)?;
                xyz.iter().all(|value| value.is_finite()).then_some(())?;
                canonical_bytes.extend_from_slice(stream.get(at..at + 24)?);
                at += 24;
            }
            Token::Scalar => {
                View::f64_be_at(stream, at)?.is_finite().then_some(())?;
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
            let references = record
                .leading_references
                .into_iter()
                .chain(record.trailing_references)
                .collect();
            (record.xmt, record.byte_len, references)
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
    (View::u16_be_at(stream, offset) == Some(90)).then_some(())?;
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
    (View::u16_be_at(stream, offset) == Some(74)).then_some(())?;
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
    (View::u16_be_at(stream, offset) == Some(70)).then_some(())?;
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
    let (xmt, node_id, references, _, end) = type_70_body(stream, body, 2)?;
    Some((xmt, node_id, references, end))
}

fn type_70_body(
    stream: &[u8],
    body: usize,
    trailing_count: usize,
) -> Option<(u32, u32, Vec<u32>, u16, usize)> {
    matches!(trailing_count, 1 | 2).then_some(())?;
    let (xmt, consumed) = read_xmt(stream, body)?;
    (xmt > 1).then_some(())?;
    let mut at = body.checked_add(consumed)?;
    let node_id = View::u32_be_at(stream, at)?;
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
    let count = View::u16_be_at(stream, at)?;
    (count > 0).then_some(())?;
    at += 2;
    (View::u32_be_at(stream, at) == Some(20)).then_some(())?;
    at += 4;
    (View::u32_be_at(stream, at) == Some(1)).then_some(())?;
    at += 4;
    let first_trailing = references.len();
    for _ in 0..trailing_count {
        let (reference, consumed) = read_xmt(stream, at)?;
        (reference > 1).then_some(())?;
        at += consumed;
        (stream.get(at) == Some(&0)).then_some(())?;
        at += 1;
        references.push(reference);
    }
    references[first_trailing..]
        .windows(2)
        .all(|pair| pair[0] == pair[1])
        .then_some(())?;
    Some((xmt, node_id, references, count, at))
}

fn consume_type_101(stream: &[u8], offset: usize) -> Option<Record> {
    (View::u16_be_at(stream, offset) == Some(101)).then_some(())?;
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
    let slot_count_value = View::u32_be_at(stream, body)?;
    let slot_count = usize::try_from(slot_count_value).ok()?;
    (slot_count > 0).then_some(())?;
    let (xmt, consumed) = read_xmt(stream, body.checked_add(4)?)?;
    (xmt > 1).then_some(())?;
    let mut at = body.checked_add(4 + consumed)?;
    let active_count_value = View::u32_be_at(stream, at)?;
    let active_count = usize::try_from(active_count_value).ok()?;
    (active_count <= slot_count).then_some(())?;
    at += 4;
    (View::u32_be_at(stream, at) == Some(0)).then_some(())?;
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
    let node_id = View::u32_be_at(stream, at)?;
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
    (View::u16_be_at(stream, offset) == Some(91)).then_some(())?;
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
    matches!(View::u32_be_at(stream, at), Some(0 | 1)).then_some(())?;
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
    (View::u16_be_at(stream, offset) == Some(141)).then_some(())?;
    let direct = type_141_layout(stream, offset, 0);
    let escaped_marker = stream.get(offset + 2) == Some(&0xff);
    let escaped = escaped_marker
        .then(|| type_141_layout(stream, offset, 1))
        .flatten();
    let (xmt, references, at) = if escaped_marker {
        escaped.or(direct)?
    } else {
        direct?
    };
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
    (View::u16_be_at(stream, offset) == Some(45)).then_some(())?;
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

fn consume_type_67(stream: &[u8], offset: usize) -> Option<Record> {
    (View::u16_be_at(stream, offset) == Some(67)).then_some(())?;
    let direct =
        type_67_layout(stream, offset, 0).filter(|(_, _, _, end)| plausible_next(stream, *end));
    let escaped = (stream.get(offset + 2) == Some(&0xff))
        .then(|| type_67_layout(stream, offset, 1))
        .flatten()
        .filter(|(_, _, _, end)| plausible_next(stream, *end));
    let (xmt, node_id, references, end) = unique_layout(direct, escaped)?;
    Some(Record {
        kind: 67,
        xmt,
        node_id: Some(node_id),
        references,
        position: None,
        canonical_bytes: stream.get(offset..end)?.to_vec(),
        offset,
        end,
    })
}

fn type_67_layout(
    stream: &[u8],
    offset: usize,
    envelope_len: usize,
) -> Option<(u32, u32, Vec<u32>, usize)> {
    let (xmt, consumed) = read_xmt(stream, offset.checked_add(2 + envelope_len)?)?;
    (xmt > 1).then_some(())?;
    let mut at = offset.checked_add(2 + envelope_len + consumed)?;
    let node_id = View::u32_be_at(stream, at)?;
    at += 4;
    let mut references = Vec::new();
    for expected_status in [1, 1, 1, 1, 0] {
        let (reference, consumed) = read_xmt(stream, at)?;
        at += consumed;
        (stream.get(at) == Some(&expected_status)).then_some(())?;
        at += 1;
        references.push(reference);
    }
    (references[0] == 1
        && references[1] == 3
        && references[2..].iter().all(|reference| *reference > 1))
    .then_some(())?;
    (stream.get(at) == Some(&0x2b)).then_some(())?;
    at += 1;
    let (linked_reference, consumed) = read_xmt(stream, at)?;
    (linked_reference > 1).then_some(())?;
    at += consumed;
    (stream.get(at) == Some(&1)).then_some(())?;
    at += 1;
    references.push(linked_reference);
    for _ in 0..4 {
        let value = View::f64_be_at(stream, at)?;
        (value == 0.0 || value.is_normal()).then_some(())?;
        at += 8;
    }
    Some((xmt, node_id, references, at))
}

fn type_45_layout(stream: &[u8], offset: usize, envelope_len: usize) -> Option<(u32, usize)> {
    let count_at = offset.checked_add(2 + envelope_len)?;
    let count = usize::try_from(View::u32_be_at(stream, count_at)?).ok()?;
    (count > 0).then_some(())?;
    let (xmt, xmt_len) = read_xmt(stream, count_at.checked_add(4)?)?;
    (xmt > 1).then_some(())?;
    let data_at = count_at.checked_add(4 + xmt_len)?;
    let finite_end = |value_count: usize| {
        let end = data_at.checked_add(value_count.checked_mul(8)?)?;
        let raw = stream.get(data_at..end)?;
        (0..value_count)
            .all(|i| {
                View::f64_be_at(raw, i * 8)
                    .is_some_and(|value| value.is_finite() && (value == 0.0 || value.is_normal()))
            })
            .then_some(end)
    };
    let exact_end = finite_end(count);
    let successor_end = count.checked_add(1).and_then(finite_end);
    let end = match (exact_end, successor_end) {
        (Some(exact), Some(successor))
            if crate::nurbs::auxiliary_record_at(stream, exact)
                .is_some_and(|record| record.end == successor) =>
        {
            exact
        }
        (Some(exact), Some(successor))
            if plausible_next(stream, exact) && !plausible_next(stream, successor) =>
        {
            exact
        }
        (_, Some(successor)) => successor,
        (Some(exact), None) if plausible_next(stream, exact) => exact,
        (Some(_) | None, None) => return None,
    };
    Some((xmt, end))
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

fn consume_intersection_data(
    stream: &[u8],
    offset: usize,
    intersection_schema_anchor_seen: bool,
) -> Option<Record> {
    let (curve, end) = crate::topology::intersection_data_curve_at(
        stream,
        offset,
        intersection_schema_anchor_seen,
    )?;
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
    let (kind, xmt, references, end) = if let Some((chart, end)) =
        crate::intersection::chart_source_record_at(
            stream,
            offset,
            crate::intersection::ChartPointLayout::Ext11,
        ) {
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
        references: auxiliary.references,
        position: None,
        canonical_bytes: stream.get(offset..auxiliary.end)?.to_vec(),
        offset,
        end: auxiliary.end,
    })
}

fn compact_tombstone(stream: &[u8], offset: usize) -> Option<u32> {
    let first = View::i16_be_at(stream, offset + 2)?;
    if first < 0 {
        let quotient = View::u16_be_at(stream, offset + 4)?;
        return (quotient == 1)
            .then_some(u32::from(quotient) * 32_767 + u32::from(first.unsigned_abs()));
    }
    (stream.get(offset + 4..offset + 6)? == [0, 1]).then_some(first as u32)
}

fn plausible_next(stream: &[u8], offset: usize) -> bool {
    if offset >= stream.len() {
        return true;
    }
    View::u16_be_at(stream, offset).is_some_and(is_next_kind)
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
        67 => "TYPE_67",
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

/// Resolve the semantic family after the record-form discriminator is known.
/// Numeric tag 90 is `GROUP` in the two-byte fixed-record form and
/// `INTERSECTION_DATA` in the schema-anchored single-byte form.
pub(crate) fn record_family_name(record: &Record) -> Option<&'static str> {
    if record.kind == 90 && record.canonical_bytes.first() == Some(&0x5a) {
        Some("INTERSECTION_DATA")
    } else {
        family_name(record.kind)
    }
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

#[cfg(test)]
mod type_67_record_tests {
    use super::*;

    fn record(escaped: bool) -> Vec<u8> {
        let mut bytes = 67u16.to_be_bytes().to_vec();
        if escaped {
            bytes.push(0xff);
        }
        bytes.extend_from_slice(&67u16.to_be_bytes());
        bytes.extend_from_slice(&1_061u32.to_be_bytes());
        for (reference, status) in [(1u16, 1), (3, 1), (440, 1), (10, 1), (149, 0)] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(status);
        }
        bytes.push(0x2b);
        bytes.extend_from_slice(&71u16.to_be_bytes());
        bytes.push(1);
        for value in [0.0, -0.0, 1.0, 2.0] {
            bytes.extend_from_slice(&f64::to_be_bytes(value));
        }
        bytes
    }

    #[test]
    fn retains_direct_and_escaped_type_67_records() {
        for escaped in [false, true] {
            let bytes = record(escaped);
            let census = walk(&bytes);

            assert!(matches!(
                census.records.as_slice(),
                [Record {
                    kind: 67,
                    xmt: 67,
                    node_id: Some(1_061),
                    references,
                    canonical_bytes,
                    end,
                    ..
                }] if references == &[1, 3, 440, 10, 149, 71]
                    && canonical_bytes == &bytes
                    && *end == bytes.len()
            ));
            assert_eq!(census.bytes_decoded, bytes.len());
        }
    }

    #[test]
    fn rejects_incomplete_or_noncanonical_type_67_records() {
        let bytes = record(true);
        for end in 0..bytes.len() {
            assert!(consume_type_67(&bytes[..end], 0).is_none());
        }

        let mut invalid_status = bytes.clone();
        invalid_status[11] = 0;
        assert!(consume_type_67(&invalid_status, 0).is_none());

        let mut subnormal_value = bytes;
        let value_at = subnormal_value.len() - 32;
        subnormal_value[value_at..value_at + 8].copy_from_slice(&1u64.to_be_bytes());
        assert!(consume_type_67(&subnormal_value, 0).is_none());
    }
}

#[cfg(test)]
mod type_150_state_packet_tests {
    use super::*;

    fn packet() -> Vec<u8> {
        let mut bytes = vec![150];
        for (reference, status) in [(1u16, 1), (3, 1), (6_192, 0), (6_193, 1), (6_194, 0)] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(status);
        }
        bytes.push(0x2b);
        for value in [-0.025, -0.05, 0.25, 0.0, 1.0, 0.0, 0.0, -0.0, 1.0] {
            bytes.extend_from_slice(&f64::to_be_bytes(value));
        }
        bytes
    }

    #[test]
    fn retains_complete_type_150_state() {
        let bytes = packet();
        let census = walk(&bytes);

        assert_eq!(census.type_150_state_packets.len(), 1);
        let packet = &census.type_150_state_packets[0];
        assert_eq!(packet.references, [1, 3, 6_192, 6_193, 6_194]);
        assert_eq!(packet.marker, 0x2b);
        assert_eq!(
            packet.values,
            [-0.025, -0.05, 0.25, 0.0, 1.0, 0.0, 0.0, -0.0, 1.0]
        );
        assert_eq!((packet.offset, packet.end), (0, bytes.len()));
        assert_eq!(census.bytes_decoded, bytes.len());

        let mut malformed = bytes.clone();
        malformed[6] = 0;
        assert!(walk(&malformed).type_150_state_packets.is_empty());

        let mut nonfinite = bytes;
        let value_offset = nonfinite.len() - 9 * 8;
        nonfinite[value_offset..value_offset + 8].copy_from_slice(&f64::NAN.to_be_bytes());
        assert!(walk(&nonfinite).type_150_state_packets.is_empty());
    }
}

#[cfg(test)]
mod schema_reference_preamble_tests {
    use super::*;

    fn push_xmt(bytes: &mut Vec<u8>, reference: u32) {
        if i16::try_from(reference).is_ok() {
            bytes.extend_from_slice(&(reference as u16).to_be_bytes());
            return;
        }
        let quotient = reference / 32_767;
        let remainder = reference % 32_767;
        bytes.extend_from_slice(&(-(remainder as i16)).to_be_bytes());
        bytes.extend_from_slice(&(quotient as u16).to_be_bytes());
    }

    fn preamble() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&300u16.to_be_bytes());
        bytes.extend_from_slice(&4u16.to_be_bytes());
        bytes.push(0xff);
        for reference in [40_000, 40_001, 1, 1, 1] {
            push_xmt(&mut bytes, reference);
        }
        for state_word in [2u32, 0, 1, 55] {
            bytes.extend_from_slice(&state_word.to_be_bytes());
        }
        bytes.extend_from_slice(&[0, 0, 0]);
        bytes.extend_from_slice(&300u16.to_be_bytes());
        for reference in [1, 1] {
            push_xmt(&mut bytes, reference);
        }
        bytes.extend_from_slice(&7u16.to_be_bytes());
        for (kind, reference) in [(81u16, 4u32), (82, 40_000), (81, 5)] {
            bytes.extend_from_slice(&kind.to_be_bytes());
            push_xmt(&mut bytes, reference);
        }
        bytes.extend_from_slice(&82u16.to_be_bytes());
        push_xmt(&mut bytes, 1);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes
    }

    #[test]
    fn schema_reference_preamble_retains_variable_reference_lane() {
        let bytes = preamble();
        let parsed = schema_reference_preamble(&bytes, 0, bytes.len())
            .expect("complete preamble must be admitted");

        assert_eq!(
            parsed,
            SchemaReferencePreamble {
                identity: 300,
                references: [40_000, 40_001],
                state_references: [1; 3],
                state_words: [2, 0, 1, 55],
                count: 7,
                entries: vec![(81, 4), (82, 40_000), (81, 5)],
                terminal_value: 9,
                offset: 0,
                end: bytes.len(),
            }
        );
    }

    #[test]
    fn schema_reference_preamble_requires_complete_consistent_framing() {
        let bytes = preamble();
        let mut mismatched_kind = bytes.clone();
        let repeated_kind = 5 + 2 * 4 + 3 * 2 + 16 + 3;
        mismatched_kind[repeated_kind + 1] ^= 1;

        for malformed in [&bytes[..bytes.len() - 1], mismatched_kind.as_slice()] {
            assert!(schema_reference_preamble(malformed, 0, malformed.len()).is_none());
        }
    }

    #[test]
    fn schema_events_precede_record_like_bytes() {
        let preamble = preamble();
        let preamble_end = preamble.len();
        let bytes = [preamble, BODY_SCHEMA_HEADER.to_vec()].concat();
        let census = walk(&bytes);

        assert_eq!(census.schema_reference_preambles.len(), 1);
        assert_eq!(census.schema_reference_preambles[0].end, preamble_end);
        assert_eq!(
            census.inline_schema_declarations,
            [InlineSchemaDeclaration {
                fields: InlineSchemaFields::BodyHeader,
                offset: preamble_end,
                end: bytes.len(),
            }]
        );
        assert!(census.records.is_empty());
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn schema_reference_preamble_retains_linked_state_reference() {
        let mut bytes = preamble();
        let first_state_reference = 5 + 2 * 4;
        let mut linked_state = 1u16.to_be_bytes().to_vec();
        push_xmt(&mut linked_state, 40_002);
        linked_state.extend_from_slice(&1u16.to_be_bytes());
        bytes.splice(
            first_state_reference..first_state_reference + 6,
            linked_state,
        );

        let parsed = schema_reference_preamble(&bytes, 0, bytes.len())
            .expect("linked state-reference lane must be admitted");

        assert_eq!(parsed.references, [40_000, 40_001]);
        assert_eq!(parsed.state_references, [1, 40_002, 1]);

        let mut invalid = bytes;
        invalid[first_state_reference + 3] ^= 1;
        assert!(schema_reference_preamble(&invalid, 0, invalid.len()).is_none());
    }
}

#[cfg(test)]
mod inline_schema_tests {
    use super::*;

    fn push_xmt(bytes: &mut Vec<u8>, reference: u32) {
        if i16::try_from(reference).is_ok() {
            bytes.extend_from_slice(&(reference as u16).to_be_bytes());
            return;
        }
        let quotient = reference / 32_767;
        let remainder = reference % 32_767;
        bytes.extend_from_slice(&(-(remainder as i16)).to_be_bytes());
        bytes.extend_from_slice(&(quotient as u16).to_be_bytes());
    }

    #[test]
    fn body_schema_header_is_bounded_independently_of_instance_state() {
        let mut stream = BODY_SCHEMA_HEADER.to_vec();
        stream.extend_from_slice(&[0xaa, 0xbb]);

        let declaration = inline_schema_declaration(&stream, 0, stream.len())
            .expect("complete BODY schema header must be admitted");

        assert_eq!(declaration.fields, InlineSchemaFields::BodyHeader);
        assert_eq!(declaration.end, BODY_SCHEMA_HEADER.len());
        assert!(inline_schema_declaration(
            &BODY_SCHEMA_HEADER[..BODY_SCHEMA_HEADER.len() - 1],
            0,
            BODY_SCHEMA_HEADER.len() - 1,
        )
        .is_none());
    }

    #[test]
    fn body_schema_header_binds_compact_and_revision_instance_states() {
        for reference in [3u16, 9] {
            let mut compact = BODY_SCHEMA_HEADER.to_vec();
            compact.extend_from_slice(&reference.to_be_bytes());
            compact.push(0);
            let compact_census = walk(&compact);
            assert_eq!(
                compact_census.inline_body_states,
                [InlineBodyState {
                    fields: InlineBodyStateFields::Compact {
                        reference: u32::from(reference),
                    },
                    offset: BODY_SCHEMA_HEADER.len(),
                    end: compact.len(),
                }]
            );
            let compact_status = compact.len() - 1;
            compact[compact_status] = 1;
            assert!(walk(&compact).inline_body_states.is_empty());
        }

        let mut revision = BODY_SCHEMA_HEADER.to_vec();
        let state_offset = revision.len();
        revision.extend_from_slice(&3u16.to_be_bytes());
        revision.extend_from_slice(&7u32.to_be_bytes());
        for reference in [8u16, 1, 2, 3, 4, 5, 6, 7] {
            revision.extend_from_slice(&reference.to_be_bytes());
            revision.push(1);
        }
        revision.extend_from_slice(&[0xaa, 0xbb]);
        let state_end = revision.len();
        revision.extend_from_slice(TYPE_70_SCHEMA_HEADER);

        let revision_census = walk(&revision);
        assert_eq!(revision_census.inline_body_states.len(), 1);
        assert_eq!(
            revision_census.inline_body_states[0],
            InlineBodyState {
                fields: InlineBodyStateFields::Revision {
                    node_id: 7,
                    references: [8, 1, 2, 3, 4, 5, 6, 7],
                    state_bytes: vec![0xaa, 0xbb],
                },
                offset: state_offset,
                end: state_end,
            }
        );
    }

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

    fn type_101_declaration() -> Vec<u8> {
        let mut bytes = TYPE_101_SCHEMA_HEADER.to_vec();
        push_xmt(&mut bytes, 2);
        bytes.extend_from_slice(TYPE_101_SCHEMA_STATE_PREFIX);
        bytes.extend_from_slice(&4u16.to_be_bytes());
        for reference in [40_000, 3, 1, 9] {
            push_xmt(&mut bytes, reference);
        }
        push_xmt(&mut bytes, 1);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        push_xmt(&mut bytes, 11);
        for state_word in [19u32, 9, 27] {
            bytes.extend_from_slice(&state_word.to_be_bytes());
        }
        bytes.extend_from_slice(&[0, 0, 0, 1, 2]);
        bytes
    }

    fn type_100_declaration() -> Vec<u8> {
        let mut bytes = TYPE_100_SCHEMA_HEADER.to_vec();
        push_xmt(&mut bytes, 48);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        for reference in [2, 49, 1] {
            push_xmt(&mut bytes, reference);
            bytes.push(1);
        }
        for ordinal in 0..13 {
            let value: f64 = if ordinal % 4 == 0 { 1.0 } else { 0.0 };
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&1u32.to_be_bytes());
        for _ in 0..3 {
            bytes.extend_from_slice(&0xc2bc_928f_996e_0000u64.to_be_bytes());
        }
        push_xmt(&mut bytes, 1);
        bytes.push(1);
        bytes
    }

    fn type_38_declaration() -> Vec<u8> {
        let mut bytes = crate::topology::TYPE_38_SCHEMA_HEADER.to_vec();
        push_xmt(&mut bytes, 40_000);
        bytes.extend_from_slice(&17u32.to_be_bytes());
        for reference in [1, 7, 8, 9, 1] {
            push_xmt(&mut bytes, reference);
            bytes.push(1);
        }
        bytes.push(0x2d);
        for reference in [11, 12] {
            push_xmt(&mut bytes, reference);
            bytes.push(1);
        }
        for reference in [40_003, 40_002, 40_001] {
            push_xmt(&mut bytes, reference);
            bytes.push(0);
        }
        push_xmt(&mut bytes, 1);
        bytes.push(1);
        bytes.extend_from_slice(TYPE_41_SCHEMA_HEADER);
        bytes.extend_from_slice(&1u32.to_be_bytes());
        push_xmt(&mut bytes, 40_002);
        bytes.extend_from_slice(&[0x4c, 0x3f]);
        for value in [0.5, -0.25, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0] {
            bytes.extend_from_slice(&f64::to_be_bytes(value));
        }
        bytes
    }

    fn type_41_declaration() -> Vec<u8> {
        let mut bytes = TYPE_41_SCHEMA_HEADER.to_vec();
        bytes.extend_from_slice(&1u32.to_be_bytes());
        push_xmt(&mut bytes, 86);
        bytes.extend_from_slice(&[0x4c, 0x3f]);
        for value in [0.5, -0.25, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0] {
            bytes.extend_from_slice(&f64::to_be_bytes(value));
        }
        bytes
    }

    #[test]
    fn type_38_schema_declaration_retains_nested_term_state() {
        let bytes = type_38_declaration();
        let census = walk(&bytes);

        assert_eq!(
            census.inline_schema_declarations,
            [InlineSchemaDeclaration {
                fields: InlineSchemaFields::Type38 {
                    xmt: 40_000,
                    node_id: 17,
                    leading_references: [1, 7, 8, 9, 1],
                    leading_statuses: [1; 5],
                    marker: 0x2d,
                    linked_references: vec![11, 12],
                    state_references: vec![40_003, 40_002, 40_001],
                    numeric_values: Some(
                        [0.5, -0.25, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0,]
                    ),
                },
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn type_38_schema_declaration_accepts_compact_state_orders() {
        for (xmt, leading_references, linked_references, expected_state_references) in [
            (80, [1, 7, 8, 9, 1], [87, 12], [83, 82, 81]),
            (112, [1, 7, 8, 9, 1], [118, 119], [120, 121, 122]),
            (10, [1, 13, 47, 1, 1], [45, 9], [48, 49, 50]),
        ] {
            let mut bytes = crate::topology::TYPE_38_SCHEMA_HEADER.to_vec();
            push_xmt(&mut bytes, xmt);
            bytes.extend_from_slice(&17u32.to_be_bytes());
            for reference in leading_references {
                push_xmt(&mut bytes, reference);
                bytes.push(1);
            }
            bytes.push(0x2d);
            for reference in linked_references {
                push_xmt(&mut bytes, reference);
                bytes.push(1);
            }
            for reference in expected_state_references {
                push_xmt(&mut bytes, reference);
                bytes.push(0);
            }
            push_xmt(&mut bytes, 1);
            bytes.push(1);

            let census = walk(&bytes);

            assert!(matches!(
                &census.inline_schema_declarations[0].fields,
                InlineSchemaFields::Type38 {
                    xmt: parsed_xmt,
                    linked_references: parsed_links,
                    state_references,
                    numeric_values: None,
                    ..
                } if *parsed_xmt == xmt
                    && parsed_links == &linked_references
                    && state_references == &expected_state_references
            ));
            assert_eq!(census.bytes_decoded, bytes.len());
        }
    }

    #[test]
    fn type_38_schema_declaration_accepts_marker_2b_state() {
        let mut bytes = crate::topology::TYPE_38_SCHEMA_HEADER.to_vec();
        push_xmt(&mut bytes, 53);
        bytes.extend_from_slice(&2_711u32.to_be_bytes());
        for reference in [1, 778, 763, 372, 1] {
            push_xmt(&mut bytes, reference);
            bytes.push(1);
        }
        bytes.push(0x2b);
        push_xmt(&mut bytes, 381);
        bytes.push(1);
        for reference in [765, 803, 804, 805] {
            push_xmt(&mut bytes, reference);
            bytes.push(0);
        }
        push_xmt(&mut bytes, 1);
        bytes.push(1);

        let census = walk(&bytes);

        assert!(matches!(
            &census.inline_schema_declarations[0].fields,
            InlineSchemaFields::Type38 {
                xmt: 53,
                marker: 0x2b,
                linked_references,
                state_references,
                numeric_values: None,
                ..
            } if linked_references == &[381]
                && state_references == &[765, 803, 804, 805]
        ));
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn type_38_schema_declaration_retains_status_zero_anchor() {
        let mut bytes = crate::topology::TYPE_38_SCHEMA_HEADER.to_vec();
        push_xmt(&mut bytes, 1_118);
        bytes.extend_from_slice(&3_178u32.to_be_bytes());
        let mut fifth_status_offset = 0;
        for (index, (reference, status)) in [(1, 1), (3, 1), (907, 1), (1_082, 1), (1_119, 0)]
            .into_iter()
            .enumerate()
        {
            push_xmt(&mut bytes, reference);
            if index == 4 {
                fifth_status_offset = bytes.len();
            }
            bytes.push(status);
        }
        bytes.push(0x2d);
        for reference in [1_070, 1_063] {
            push_xmt(&mut bytes, reference);
            bytes.push(1);
        }
        let first_state_offset = bytes.len();
        for reference in [1_120, 1_121, 1_122] {
            push_xmt(&mut bytes, reference);
            bytes.push(0);
        }
        push_xmt(&mut bytes, 1);
        bytes.push(1);

        let census = walk(&bytes);

        assert!(matches!(
            &census.inline_schema_declarations[0].fields,
            InlineSchemaFields::Type38 {
                leading_references: [1, 3, 907, 1_082, 1_119],
                leading_statuses: [1, 1, 1, 1, 0],
                state_references,
                ..
            } if state_references == &[1_120, 1_121, 1_122]
        ));
        assert_eq!(census.bytes_decoded, bytes.len());

        let mut invalid_status = bytes.clone();
        invalid_status[fifth_status_offset] = 2;
        assert!(walk(&invalid_status).inline_schema_declarations.is_empty());

        let mut invalid_anchor = bytes;
        invalid_anchor[first_state_offset + 1] ^= 1;
        assert!(walk(&invalid_anchor).inline_schema_declarations.is_empty());
    }

    #[test]
    fn type_41_schema_declaration_retains_term_state() {
        let bytes = type_41_declaration();
        let census = walk(&bytes);

        assert_eq!(
            census.inline_schema_declarations,
            [InlineSchemaDeclaration {
                fields: InlineSchemaFields::Type41 {
                    reference: 86,
                    numeric_values: [0.5, -0.25, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0,],
                },
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn type_100_schema_declaration_retains_precision_state() {
        let bytes = type_100_declaration();
        let census = walk(&bytes);

        assert_eq!(
            census.inline_schema_declarations,
            [InlineSchemaDeclaration {
                fields: InlineSchemaFields::Type100 {
                    xmt: 48,
                    references: [2, 49, 1],
                    transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,],
                },
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded, bytes.len());

        let mut translated = bytes;
        let state = TYPE_100_SCHEMA_HEADER.len();
        translated[state..state + 2].copy_from_slice(&53u16.to_be_bytes());
        translated[state + 9..state + 11].copy_from_slice(&54u16.to_be_bytes());
        let translation = state + 15 + 9 * 8;
        translated[translation..translation + 8].copy_from_slice(&(-0.0f64).to_be_bytes());
        translated[translation + 8..translation + 16].copy_from_slice(&(-0.0f64).to_be_bytes());
        translated[translation + 16..translation + 24].copy_from_slice(&1.25f64.to_be_bytes());
        let translated_census = walk(&translated);
        assert!(matches!(
            translated_census.inline_schema_declarations[0].fields,
            InlineSchemaFields::Type100 {
                xmt: 53,
                references: [2, 54, 1],
                transform: [
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    value_x,
                    value_y,
                    1.25,
                    1.0
                ],
            } if value_x.to_bits() == (-0.0f64).to_bits()
                && value_y.to_bits() == (-0.0f64).to_bits()
        ));
        assert_eq!(translated_census.bytes_decoded, translated.len());
    }

    #[test]
    fn type_101_schema_declaration_retains_bound_state() {
        let bytes = type_101_declaration();
        let census = walk(&bytes);

        assert_eq!(
            census.inline_schema_declarations,
            [InlineSchemaDeclaration {
                fields: InlineSchemaFields::Type101 {
                    references: [40_000, 3, 1, 9],
                    anchor_reference: Some(11),
                    state_words: [19, 9, 27],
                    terminal_value: 258,
                },
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded, bytes.len());

        let mut alternate = TYPE_101_SCHEMA_HEADER.to_vec();
        push_xmt(&mut alternate, 2);
        let mut prefix = TYPE_101_SCHEMA_STATE_PREFIX.to_vec();
        prefix[7] = 1;
        prefix[10] = 1;
        prefix[30] = 0;
        alternate.extend(prefix);
        alternate.extend_from_slice(&4u16.to_be_bytes());
        for reference in [7, 6, 8, 1, 1] {
            push_xmt(&mut alternate, reference);
        }
        alternate.extend_from_slice(&0u16.to_be_bytes());
        push_xmt(&mut alternate, 5);
        for state_word in [0u32, 0, 9] {
            alternate.extend_from_slice(&state_word.to_be_bytes());
        }
        alternate.extend_from_slice(&[0, 0, 0, 0, 3]);

        let alternate_census = walk(&alternate);
        assert!(matches!(
            alternate_census.inline_schema_declarations[0].fields,
            InlineSchemaFields::Type101 {
                state_words: [0, 0, 9],
                terminal_value: 3,
                ..
            }
        ));
        assert_eq!(alternate_census.bytes_decoded, alternate.len());

        let mut unanchored = TYPE_101_SCHEMA_HEADER.to_vec();
        push_xmt(&mut unanchored, 2);
        let mut prefix = TYPE_101_SCHEMA_STATE_PREFIX.to_vec();
        prefix[7] = 1;
        prefix[10] = 1;
        prefix[30] = 0;
        unanchored.extend(prefix);
        unanchored.extend_from_slice(&4u16.to_be_bytes());
        for reference in [6022, 6021, 6019, 1, 1] {
            push_xmt(&mut unanchored, reference);
        }
        unanchored.extend_from_slice(&0u16.to_be_bytes());
        push_xmt(&mut unanchored, 0);
        for state_word in [0u32, 0, 0xc06f] {
            unanchored.extend_from_slice(&state_word.to_be_bytes());
        }
        unanchored.extend_from_slice(&[0, 0, 0, 0, 4]);

        let unanchored_census = walk(&unanchored);
        assert!(matches!(
            unanchored_census.inline_schema_declarations[0].fields,
            InlineSchemaFields::Type101 {
                anchor_reference: None,
                state_words: [0, 0, 0xc06f],
                terminal_value: 4,
                ..
            }
        ));
        assert_eq!(unanchored_census.bytes_decoded, unanchored.len());

        let mut compact = TYPE_101_SCHEMA_HEADER.to_vec();
        push_xmt(&mut compact, 2);
        compact.extend_from_slice(&TYPE_101_SCHEMA_STATE_PREFIX[..TYPE_101_COMPACT_STATE_LEN]);

        let compact_census = walk(&compact);
        assert_eq!(
            compact_census.inline_schema_declarations,
            [InlineSchemaDeclaration {
                fields: InlineSchemaFields::Type101Compact,
                offset: 0,
                end: compact.len(),
            }]
        );
        assert_eq!(compact_census.bytes_decoded, compact.len());

        compact.push(0);
        assert!(walk(&compact).inline_schema_declarations.is_empty());
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
    fn attdef_list_declaration_shares_its_slot_lane() {
        let mut stream = ATTDEF_LIST_SCHEMA_HEADER.to_vec();
        stream.extend_from_slice(&20u32.to_be_bytes());
        stream.extend_from_slice(&43u16.to_be_bytes());
        stream.extend_from_slice(&10u32.to_be_bytes());
        stream.extend_from_slice(&0u32.to_be_bytes());
        for reference in [1u16, 143, 155, 114, 150, 145, 167, 164, 105, 137, 141] {
            stream.extend_from_slice(&reference.to_be_bytes());
            stream.push(1);
        }
        for _ in 0..10 {
            stream.extend_from_slice(&1u16.to_be_bytes());
            stream.push(1);
        }
        let declaration_end = stream.len();
        let shared_offset = declaration_end - 33;
        stream.extend_from_slice(&[0; 3]);
        let census = Census {
            tagged_reference_lanes: vec![
                TaggedReferenceLane {
                    references: Vec::new(),
                    offset: shared_offset,
                    end: shared_offset + 16,
                },
                TaggedReferenceLane {
                    references: Vec::new(),
                    offset: declaration_end,
                    end: stream.len(),
                },
            ],
            ..Census::default()
        };
        let declarations = inline_schema_declarations(&stream, &census);

        assert!(matches!(
            &declarations[0].fields,
            InlineSchemaFields::AttdefList {
                xmt: 43,
                slot_count: 20,
                active_count: 10,
                references,
            } if references[..10] == [143, 155, 114, 150, 145, 167, 164, 105, 137, 141]
                && references[10..] == [1; 10]
        ));
        assert_eq!(declarations[0].end, declaration_end);
    }

    #[test]
    fn type_70_inline_declaration_accepts_one_or_two_equal_trailing_references() {
        let duplicated = type_70_declaration();
        let mut single = TYPE_70_SCHEMA_HEADER.to_vec();
        single.extend_from_slice(&6u16.to_be_bytes());
        single.extend_from_slice(&0u32.to_be_bytes());
        single.push(4);
        for reference in [3u16, 1, 1, 0] {
            single.push(1);
            single.extend_from_slice(&reference.to_be_bytes());
        }
        single.extend_from_slice(&13u16.to_be_bytes());
        single.extend_from_slice(&20u32.to_be_bytes());
        single.extend_from_slice(&1u32.to_be_bytes());
        single.extend_from_slice(&45u16.to_be_bytes());
        single.push(0);

        let single_declaration = inline_schema_declaration(&single, 0, single.len())
            .expect("complete single-tail type-70 declaration");
        assert!(matches!(
            single_declaration.fields,
            InlineSchemaFields::Type70 {
                trailing_reference: 45,
                ..
            }
        ));
        assert_eq!(single_declaration.end, single.len());

        let duplicated_declaration = inline_schema_declaration(&duplicated, 0, duplicated.len())
            .expect("complete duplicated-tail type-70 declaration");
        assert!(matches!(
            duplicated_declaration.fields,
            InlineSchemaFields::Type70 {
                trailing_reference: 11,
                ..
            }
        ));
        assert_eq!(duplicated_declaration.end, duplicated.len());
    }

    #[test]
    fn truncated_inline_schema_declaration_is_not_admitted() {
        for (name, mut stream) in [
            attdef_list_declaration(),
            type_70_declaration(),
            type_38_declaration(),
            type_41_declaration(),
            type_100_declaration(),
            type_101_declaration(),
        ]
        .into_iter()
        .enumerate()
        {
            stream.pop();
            assert!(
                walk(&stream).inline_schema_declarations.is_empty(),
                "truncated declaration {name}"
            );
        }
    }
}

#[cfg(test)]
mod nurbs_auxiliary_tests {
    use super::*;

    fn status_framed_curve_descriptor() -> Vec<u8> {
        vec![
            0, 136, 0x0d, 0xd1, // type and XMT
            0, 3, // degree
            0, 0, 0, 4, // pole count
            0, 4, // homogeneous dimension
            0, 0, 0, 2, // distinct-knot count
            1, 0, 0, 1, 4, // form and reference-lane prefix
            0x0d, 0xd4, 0, // term-use reference
            0x0d, 0xd3, 0, // multiplicity reference
            0x0d, 0xd2, 0, // knot reference
        ]
    }

    fn escaped_curve_descriptor_with_extended_references() -> Vec<u8> {
        vec![
            0, 136, 0xff, 0xb8, 0xfc, 0, 1, // envelope and extended XMT
            0, 1, // degree
            0, 0, 0, 2, // pole count
            0, 2, // dimension
            0, 0, 0, 2, // distinct-knot count
            5, 0, 0, 0, 1, // form and reference-lane prefix
            0xb8, 0xf9, 0, 1, 0, // term-use reference
            0xb8, 0xfa, 0, 1, 0, // multiplicity reference
            0xb8, 0xfb, 0, 1, 0, // knot reference
        ]
    }

    #[test]
    fn retains_status_framed_rational_curve_descriptor() {
        let bytes = status_framed_curve_descriptor();
        let census = walk(&bytes);

        assert_eq!(census.records.len(), 1);
        assert_eq!(census.records[0].kind, 136);
        assert_eq!(census.records[0].xmt, 3_537);
        assert_eq!(census.records[0].references, [3_540, 3_539, 3_538]);
        assert_eq!(census.records[0].end, bytes.len());
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn rejects_incomplete_or_noncanonical_curve_descriptor_state() {
        let bytes = status_framed_curve_descriptor();
        let mut bad_status = bytes.clone();
        bad_status[23] = 1;
        let mut bad_dimension = bytes.clone();
        bad_dimension[11] = 5;

        for malformed in [&bytes[..bytes.len() - 1], &bad_status, &bad_dimension] {
            assert!(walk(malformed).records.is_empty());
        }
    }

    #[test]
    fn retains_escaped_curve_descriptor_with_extended_state_references() {
        let bytes = escaped_curve_descriptor_with_extended_references();
        let census = walk(&bytes);

        assert_eq!(census.records.len(), 1);
        assert_eq!(census.records[0].kind, 136);
        assert_eq!(census.records[0].xmt, 50_947);
        assert_eq!(census.records[0].references, [50_950, 50_949, 50_948]);
        assert_eq!(census.records[0].end, bytes.len());
        assert_eq!(census.bytes_decoded, bytes.len());
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
                target_kind: Some(55),
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn reference_type_map_accepts_status_prefixed_null_reference() {
        let canonical = map_bytes();
        let mut bytes = vec![1, 0, 1];
        bytes.extend_from_slice(&canonical[4..]);
        let census = walk(&bytes);

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(40_000, 81), (3, 100)],
                target_kind: Some(55),
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn reference_type_map_accepts_null_target_kind() {
        let mut bytes = map_bytes();
        bytes[18..].copy_from_slice(&[0, 1]);

        let census = walk(&bytes);

        assert_eq!(census.reference_type_maps.len(), 1);
        assert_eq!(census.reference_type_maps[0].target_kind, Some(1));
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn reference_type_map_accepts_schema_defined_target_kind() {
        let mut bytes = map_bytes();
        bytes[18..].copy_from_slice(&323u16.to_be_bytes());

        let census = walk(&bytes);

        assert_eq!(census.reference_type_maps.len(), 1);
        assert_eq!(census.reference_type_maps[0].target_kind, Some(323));
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn reference_type_map_precedes_counted_record_at_shared_kind() {
        let map = vec![0, 1, 0, 1, 0, 3, 0, 82, 0, 1, 0, 0, 2, 100];
        let mut bytes = map.clone();
        bytes.resize(bytes.len() + 65_536 * 4, 0);

        let census = walk(&bytes);

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 82)],
                target_kind: Some(612),
                offset: 0,
                end: map.len(),
            }]
        );
        assert!(census.records.is_empty());
        assert_eq!(census.bytes_decoded, map.len());
    }

    #[test]
    fn reference_type_map_accepts_entry_table_without_target_clause() {
        let bytes = vec![0, 1, 0, 1, 0, 3, 0, 81, 0, 4, 0, 100];

        let census = walk(&bytes);

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 81), (4, 100)],
                target_kind: None,
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn reference_type_map_shares_its_final_kind_with_a_tombstone() {
        let bytes = vec![0, 1, 0, 1, 0, 3, 0, 81, 0, 9, 0, 1];

        let census = walk(&bytes);

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 81)],
                target_kind: None,
                offset: 0,
                end: 8,
            }]
        );
        assert_eq!(
            census.tombstones,
            [Tombstone {
                kind: 81,
                xmt: 9,
                offset: 6,
            }]
        );
        assert_eq!(census.bytes_decoded, bytes.len());

        let mut incomplete_tombstone = bytes;
        incomplete_tombstone[11] = 2;
        assert!(walk(&incomplete_tombstone).reference_type_maps.is_empty());
    }

    #[test]
    fn reference_type_map_accepts_targetless_terminal_clause() {
        let bytes = vec![1, 0, 1, 0, 3, 0, 81, 0, 4, 0, 100, 0, 1, 0, 0];

        let census = walk(&bytes);

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 81), (4, 100)],
                target_kind: None,
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn reference_type_map_accepts_map_only_type_codes() {
        let bytes = vec![1, 0, 1, 0, 3, 0, 67, 0, 4, 0, 11, 0, 1, 0, 0, 0, 61];

        let census = walk(&bytes);

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 67), (4, 11)],
                target_kind: Some(61),
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn reference_type_map_is_discovered_after_state_packet_prefix() {
        let mut bytes = Vec::new();
        for word in [1u16, 1, 4, 2, 3, 4, 1, 1] {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        for word in [5u32, 6, 7, 8, 9] {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        bytes.push(10);
        let map_offset = bytes.len();
        bytes.extend(map_bytes());

        let census = walk(&bytes);

        assert_eq!(census.reference_state_packets.len(), 1);
        assert_eq!(census.reference_type_maps.len(), 1);
        assert_eq!(census.reference_type_maps[0].offset, map_offset);
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn target_clause_self_delimits_reference_type_map_prefixes() {
        let first = map_bytes();
        let mut bytes = first.clone();
        bytes.extend(map_bytes());

        let census = walk(&bytes);

        assert_eq!(census.reference_type_maps.len(), 2);
        assert_eq!(census.reference_type_maps[0].offset, 0);
        assert_eq!(census.reference_type_maps[0].end, first.len());
        assert_eq!(census.reference_type_maps[1].offset, first.len());
        assert_eq!(census.reference_type_maps[1].end, bytes.len());
        assert_eq!(census.bytes_decoded, bytes.len());
    }

    #[test]
    fn reference_type_map_requires_complete_framing_and_known_types() {
        let bytes = map_bytes();
        let mut unknown_entry_type = bytes.clone();
        unknown_entry_type[9] = 0xfe;
        let mut zero_target_type = bytes.clone();
        zero_target_type[18..].copy_from_slice(&0u16.to_be_bytes());

        for malformed in [
            &bytes[..bytes.len() - 1],
            unknown_entry_type.as_slice(),
            zero_target_type.as_slice(),
        ] {
            assert!(walk(malformed).reference_type_maps.is_empty());
        }
    }
}

#[cfg(test)]
mod terminal_null_reference_tests {
    use super::*;

    #[test]
    fn retains_two_or_four_null_references_at_the_stream_boundary() {
        let four_references = [0, 1, 0, 1, 0, 1, 0, 1];
        let census = walk(&four_references);

        assert_eq!(
            census.terminal_null_references,
            Some(TerminalNullReferences {
                offset: 0,
                end: four_references.len(),
                count: 4,
            })
        );
        assert_eq!(census.bytes_decoded, four_references.len());

        let two_references = [0, 1, 0, 1];
        assert_eq!(
            walk(&two_references).terminal_null_references,
            Some(TerminalNullReferences {
                offset: 0,
                end: two_references.len(),
                count: 2,
            })
        );

        let mut nonterminal = four_references.to_vec();
        nonterminal.extend_from_slice(&[0, 29]);
        assert!(walk(&nonterminal).terminal_null_references.is_none());

        let mut nonnull = four_references;
        nonnull[7] = 2;
        assert!(walk(&nonnull).terminal_null_references.is_none());
    }
}

#[cfg(test)]
mod transmit_header_tests {
    use super::*;

    fn header(references: &[u8]) -> Vec<u8> {
        let description = b": TRANSMIT FILE (deltas) created by modeller version 3501171";
        let schema = b"SCH_3501171_35102_13006";
        let mut bytes = b"PS".to_vec();
        bytes.extend_from_slice(&(description.len() as u32).to_be_bytes());
        bytes.extend_from_slice(description);
        bytes.extend_from_slice(&(schema.len() as u32).to_be_bytes());
        bytes.extend_from_slice(schema);
        bytes.extend_from_slice(&[0, 0xe7, 0, 0, 0, 0, 0, 3, 0xff]);
        bytes.extend_from_slice(references);
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }

    #[test]
    fn transmit_header_accepts_compact_and_extended_consecutive_references() {
        for (references, expected) in [
            (&[0x04, 0x27, 0x04, 0x28][..], [1063, 1064]),
            (&[0xbc, 0xe4, 0, 1, 0xbc, 0xe3, 0, 1][..], [49_947, 49_948]),
        ] {
            let bytes = header(references);
            let census = walk(&bytes);
            let parsed = census.transmit_header.expect("complete transmit header");
            assert_eq!(parsed.references, expected);
            assert_eq!(parsed.schema, "SCH_3501171_35102_13006");
            assert_eq!(parsed.end, bytes.len());
            assert_eq!(census.bytes_decoded, bytes.len());
            assert!(census.records.is_empty());
            assert!(census.tombstones.is_empty());
        }
    }

    #[test]
    fn transmit_header_rejects_nonconsecutive_references_and_truncation() {
        let nonconsecutive = header(&[0x04, 0x27, 0x04, 0x29]);
        let complete = header(&[0x04, 0x27, 0x04, 0x28]);
        assert!(walk(&nonconsecutive).transmit_header.is_none());
        assert!(walk(&complete[..complete.len() - 1])
            .transmit_header
            .is_none());
    }
}

#[cfg(test)]
mod tests;
