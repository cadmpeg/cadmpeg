// SPDX-License-Identifier: Apache-2.0
//! Walk status-byte-framed Parasolid deltas records.

#![deny(clippy::disallowed_methods)]

use crate::framing::node_kind::NodeKind;
use std::collections::BTreeMap;
use std::num::NonZeroU16;

pub(crate) mod census;
use census::{walk, Census};

pub(crate) mod record_family;
pub(crate) mod record_kind;
use record_family::{FixedPosition, RecordFamily};
pub(crate) mod list;
pub(crate) mod pointer_list_block;
pub(crate) mod reference_lanes;
pub(crate) mod world;
use crate::records::instance::SchemaNode;
use list::{ListFields, ListWire};
use pointer_list_block::PointerListBlockFields;
use reference_lanes::{MapEntries, TaggedReferences};
use world::WorldLayout;
pub(crate) mod packet_marker;
pub(crate) mod partition_prefix;
pub(crate) mod pmark_nodes;
mod pmark_packets;
pub(crate) mod pmark_state;
use partition_prefix::PartitionPrefixState;
use pmark_packets::PmarkPacket;
pub(crate) mod transmit_state;
use crate::framing::xmt_reference::NonNullXmt;
use packet_marker::ReferenceMarker;
use transmit_state::TransmitState;
pub(crate) mod group;
mod intersection;
pub(crate) mod tails;
use crate::records::group::GroupSelector;
use group::GroupReferenceStatus;
use record_kind::RecordKind;

use crate::framing::read_xmt_width as read_xmt;
use crate::vec3_at::vec3_be_at;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

/// One complete admitted deltas record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Record {
    /// Semantic family, including family-owned node identity and POINT/GROUP data.
    pub(crate) family: RecordFamily,
    /// Stream-local XMT identifier.
    pub(crate) xmt: u32,
    /// Partition-style bytes for fixed records and exact bytes for variable records.
    pub(crate) canonical_bytes: Vec<u8>,
    /// Record start offset in the inflated stream.
    pub(crate) offset: usize,
    /// First byte following the record.
    pub(crate) end: usize,
}

impl Record {
    /// Numeric Parasolid node type.
    pub(crate) const fn kind(&self) -> u16 {
        self.family.kind()
    }

    /// Stable family name used by the deltas census and native records.
    const fn family_name(&self) -> &'static str {
        self.family.family_name()
    }
}

/// One compact deletion carrying an explicit Parasolid type and XMT identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tombstone {
    /// Admitted Parasolid node kind.
    pub(crate) kind: RecordKind,
    /// Stream-local XMT identifier.
    pub(crate) xmt: u32,
    /// Record start offset in the inflated deltas stream.
    pub(crate) offset: usize,
}

/// One deltas BODY revision envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BodyRevision {
    /// Stream-local BODY XMT identity.
    pub(crate) xmt: NonNullXmt,
    /// Monotonic kernel revision identity.
    pub(crate) node_id: u32,
    /// Eight ordered BODY references decoded from status-framed XMT fields.
    pub(crate) references: [u32; 8],
    /// Record start offset in the inflated deltas stream.
    pub(crate) offset: usize,
    /// First byte after the validated prefix.
    pub(crate) prefix_end: usize,
    /// First byte following the complete revision envelope.
    pub(crate) end: usize,
}

/// Framed Parasolid transmit header at the start of a deltas stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransmitHeader {
    pub(crate) state: TransmitState,
    /// First byte following the header.
    pub(crate) end: usize,
}

/// Maximal deltas gap containing only typed stream-local references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaggedReferenceLane {
    /// Ordered `(Parasolid record kind, XMT identity)` references.
    pub(crate) references: TaggedReferences,
    /// First byte of the first tagged reference.
    pub(crate) offset: usize,
    /// First byte following the final tagged reference.
    pub(crate) end: usize,
}

/// One framed map from stream-local references to Parasolid type codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReferenceTypeMap {
    /// Ordered `(reference, type_code)` entries.
    pub(crate) entries: MapEntries,
    /// Optional raw word following the terminal null-reference separator.
    pub(crate) terminal_word: Option<NonZeroU16>,
    /// First byte of the map.
    pub(crate) offset: usize,
    /// First byte following the map.
    pub(crate) end: usize,
}

/// The ordinary leading PARTITION and current PMARK node pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PartitionPrefix {
    pub(crate) state: PartitionPrefixState,
    pub(crate) offset: usize,
    pub(crate) end: usize,
}

/// One deltas packet carrying a reference and a serialized marker byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReferenceMarkerPacket {
    /// Non-null stream-local XMT reference.
    pub(crate) reference: NonNullXmt,
    /// Serialized marker byte.
    pub(crate) marker: ReferenceMarker,
    /// First byte of the packet.
    pub(crate) offset: usize,
    /// First byte following the packet.
    pub(crate) end: usize,
}

/// One complete inline schema declaration.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SchemaNodeEvent {
    /// Schema-specific declaration body.
    pub(crate) fields: SchemaNode,
    /// First byte of the declaration.
    pub(crate) offset: usize,
    /// First byte following the declaration.
    pub(crate) end: usize,
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

fn transmit_header(stream: &[u8]) -> Option<TransmitHeader> {
    let (description_bytes, schema_bytes, maximum_node_type, user_field_size, end) = (|| {
        let mut view = View::over_retained(stream);
        (view.take(2)? == b"PS").then_some(())?;
        let description_len = usize::try_from(view.u32_be()?).ok()?;
        let description_bytes = view.take(description_len)?;
        let schema_len = usize::try_from(view.u32_be()?).ok()?;
        let schema_bytes = view.take(schema_len)?;
        let maximum_node_type = view.u16_be()?;
        let user_field_size = view.u32_be()?;
        Some((
            description_bytes,
            schema_bytes,
            maximum_node_type,
            user_field_size,
            view.position(),
        ))
    })()?;
    let description = String::from_utf8((description_bytes).to_vec()).ok();
    let schema = String::from_utf8((schema_bytes).to_vec()).ok();
    description.zip(schema).and_then(|(description, schema)| {
        Some(TransmitHeader {
            state: TransmitState::new(description, schema, maximum_node_type, user_field_size)
                .ok()?,
            end,
        })
    })
}

fn tagged_reference_lanes(stream: &[u8], covered: &[(usize, usize)]) -> Vec<TaggedReferenceLane> {
    let mut lanes = Vec::new();
    for (offset, end) in uncovered_spans(stream.len(), covered) {
        let mut at = offset;
        let mut references = Vec::new();
        let mut complete = true;
        while at < end {
            let Some((kind, xmt, next)) = (|| {
                let kind = View::u16_be_at(stream, at)?;
                let (xmt, consumed) = read_xmt(stream, at.checked_add(2)?)?;
                let next = at.checked_add(2 + consumed)?;
                (next <= end).then_some((kind, xmt, next))
            })() else {
                complete = false;
                break;
            };
            at = next;
            references.push((kind, xmt));
        }
        if complete && at == end {
            if let Ok(references) = TaggedReferences::try_from(references) {
                lanes.push(TaggedReferenceLane {
                    references,
                    offset,
                    end,
                });
            }
        }
    }
    lanes
}

fn reference_type_maps(
    stream: &[u8],
    census: &Census,
    covered: &[(usize, usize)],
) -> Vec<ReferenceTypeMap> {
    let mut maps = Vec::new();
    for (offset, end) in uncovered_spans(stream.len(), covered) {
        let map = if let Some(map) =
            reference_type_map(stream, offset, ReferenceTypeMapLimit::Bounded(end))
        {
            Some(map)
        } else {
            let following_kind = census
                .records
                .iter()
                .map(|record| (record.offset, record.kind()))
                .chain(
                    census
                        .tombstones
                        .iter()
                        .map(|tombstone| (tombstone.offset, u16::from(tombstone.kind.code()))),
                )
                .find_map(|(event_offset, kind)| (event_offset == end).then_some(kind));
            if let Some((following_kind, shared_end)) = following_kind.zip(end.checked_add(2)) {
                reference_type_map(stream, offset, ReferenceTypeMapLimit::Bounded(shared_end))
                    .filter(|map| {
                        map.terminal_word.is_none() && map.entries.last_kind() == following_kind
                    })
            } else {
                None
            }
        };
        if let Some(map) = map {
            maps.push(map);
        }
    }
    maps
}

#[derive(Clone, Copy)]
enum ReferenceTypeMapLimit {
    WithTerminalWord,
    Bounded(usize),
}

fn reference_type_map(
    stream: &[u8],
    offset: usize,
    limit: ReferenceTypeMapLimit,
) -> Option<ReferenceTypeMap> {
    let expected_end = match limit {
        ReferenceTypeMapLimit::WithTerminalWord => None,
        ReferenceTypeMapLimit::Bounded(end) => Some(end),
    };
    let mut at = (|| {
        if let Some((1, consumed)) = read_xmt(stream, offset) {
            let separator = offset.checked_add(consumed)?;
            (View::u16_be_at(stream, separator) == Some(1)).then_some(())?;
            separator.checked_add(2)
        } else {
            (stream.get(offset) == Some(&1)).then_some(())?;
            let leading_null = offset.checked_add(1)?;
            let (reference, consumed) = read_xmt(stream, leading_null)?;
            (reference == 1).then_some(())?;
            leading_null.checked_add(consumed)
        }
    })()?;
    let mut entries = Vec::new();
    loop {
        if expected_end == Some(at) {
            return MapEntries::try_from(entries)
                .ok()
                .map(|entries| ReferenceTypeMap {
                    entries,
                    terminal_word: None,
                    offset,
                    end: at,
                });
        }
        if expected_end.is_some_and(|end| at >= end) {
            return None;
        }
        let (reference, consumed) = read_xmt(stream, at)?;
        let next = at.checked_add(consumed)?;
        at = next;
        if expected_end.is_some_and(|end| at > end) {
            return None;
        }
        if reference == 1 {
            if View::u16_be_at(stream, at) != Some(0) {
                return None;
            }
            let next = at.checked_add(2)?;
            at = next;
            if expected_end == Some(at) {
                return MapEntries::try_from(entries)
                    .ok()
                    .map(|entries| ReferenceTypeMap {
                        entries,
                        terminal_word: None,
                        offset,
                        end: at,
                    });
            }
            let terminal_word = View::u16_be_at(stream, at).and_then(NonZeroU16::new)?;
            let next = at.checked_add(2)?;
            at = next;
            if expected_end.is_some_and(|end| at > end) {
                return None;
            }
            return MapEntries::try_from(entries)
                .ok()
                .map(|entries| ReferenceTypeMap {
                    entries,
                    terminal_word: Some(terminal_word),
                    offset,
                    end: at,
                });
        }
        let kind = View::u16_be_at(stream, at)?;
        let next = at.checked_add(2)?;
        at = next;
        if expected_end.is_some_and(|end| at > end) {
            return None;
        }
        entries.push((reference, kind));
    }
}

fn reference_marker_packets(
    stream: &[u8],
    covered: &[(usize, usize)],
) -> Vec<ReferenceMarkerPacket> {
    let mut packets = Vec::new();
    for (offset, end) in uncovered_spans(stream.len(), covered) {
        if let Some(packet) = reference_marker_packet(stream, offset, end) {
            packets.push(packet);
        }
    }
    packets
}

fn inline_schema_nodes(
    stream: &[u8],
    covered: &[(usize, usize)],
    prior: &[SchemaNodeEvent],
) -> Result<Vec<SchemaNodeEvent>, CodecError> {
    let mut nodes = Vec::new();
    let mut seen = [false; 102];
    for node in prior {
        if let Some(slot) = seen.get_mut(usize::from(node.fields.kind())) {
            *slot = true;
        }
    }
    for (offset, gap_end) in uncovered_spans(stream.len(), covered) {
        let mut at = offset;
        while at < gap_end {
            let Some(node) = inline_schema_node(stream, at, gap_end, &mut seen)? else {
                break;
            };
            at = node.end;
            nodes.push(node);
        }
    }
    Ok(nodes)
}

fn inline_schema_node(
    stream: &[u8],
    offset: usize,
    gap_end: usize,
    seen: &mut [bool; 102],
) -> Result<Option<SchemaNodeEvent>, CodecError> {
    let Some(kind) = View::u16_be_at(stream, offset) else {
        return Ok(None);
    };
    if seen.get(usize::from(kind)).is_none_or(|present| *present) {
        return Ok(None);
    }
    // Without an established first-use cursor, ff is also an escaped ordinary
    // record envelope. The existing ordinary-node readers own that ambiguity.
    if stream
        .get(offset + 2)
        .is_none_or(|count| *count == 0 || *count == 255)
        || stream
            .get(offset + 3)
            .is_none_or(|op| !matches!(op, b'C' | b'D' | b'I'))
    {
        return Ok(None);
    }
    let Some((fields, end)) = SchemaNode::read(&stream[..gap_end], offset)? else {
        return Ok(None);
    };
    seen[usize::from(kind)] = true;
    Ok(Some(SchemaNodeEvent {
        fields,
        offset,
        end,
    }))
}

fn reference_marker_packet(
    stream: &[u8],
    offset: usize,
    expected_end: usize,
) -> Option<ReferenceMarkerPacket> {
    let (reference, consumed) = read_xmt(stream, offset)?;
    let reference = NonNullXmt::try_from(reference).ok()?;
    let mut at = offset.checked_add(consumed)?;
    (stream.get(at) == Some(&1)).then_some(())?;
    at = at.checked_add(1)?;
    let (first_null, consumed) = read_xmt(stream, at)?;
    (first_null == 1).then_some(())?;
    at = at.checked_add(consumed)?;
    (stream.get(at) == Some(&1)).then_some(())?;
    at = at.checked_add(1)?;
    let marker = ReferenceMarker::try_from(*stream.get(at)?).ok()?;
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
    covered: &[(usize, usize)],
) -> impl Iterator<Item = (usize, usize)> {
    let mut gaps = Vec::new();
    let mut at = 0;
    for &(start, end) in covered {
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

fn merged_event_spans(census: &Census) -> Vec<(usize, usize)> {
    let mut covered = Vec::new();
    covered.extend(
        census
            .transmit_header
            .iter()
            .map(|header| (0, header.end))
            .chain(
                census
                    .terminal_null_references
                    .iter()
                    .map(|trailer| (trailer.offset(), trailer.end())),
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
            ),
    );
    covered.extend(
        census
            .pmark_packets
            .iter()
            .map(|packet| (packet.offset, packet.end)),
    );
    covered.extend(
        census
            .partition_prefix
            .iter()
            .map(|prefix| (prefix.offset, prefix.end)),
    );
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
            .reference_marker_packets
            .iter()
            .map(|packet| (packet.offset, packet.end)),
    );
    covered.extend(
        census
            .schema_nodes
            .iter()
            .map(|declaration| (declaration.offset, declaration.end)),
    );
    covered.sort_unstable_by(Ord::cmp);
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

fn consume_shared_record(
    stream: &[u8],
    offset: usize,
    records: &[Record],
    world_layout: Option<WorldLayout>,
    intersection_layout: intersection::IntersectionLayout,
) -> Result<Option<Record>, CodecError> {
    let Some(previous) = records.last() else {
        return Ok(None);
    };
    if previous.end != offset || !has_shareable_terminal(stream, previous) {
        return Ok(None);
    }
    let Some(record_offset) = offset.checked_sub(1) else {
        return Ok(None);
    };
    if let Some(record) = consume_intersection_auxiliary(stream, record_offset) {
        return Ok(Some(record));
    }
    if let Some(record) = first_complete_record(stream, record_offset, false, world_layout) {
        return Ok(Some(record));
    }
    let Some(kind) = stream.get(offset).copied().map(u16::from) else {
        return Ok(None);
    };
    let fixed = if let Some(signature) = fixed_signature(kind, intersection_layout) {
        consume_fixed(stream, record_offset, kind, signature, world_layout)?
    } else {
        None
    };
    Ok(if fixed.is_some() {
        fixed
    } else {
        consume_variable(stream, record_offset, kind)
    })
}

fn first_complete_record(
    stream: &[u8],
    offset: usize,
    include_type_67: bool,
    world_layout: Option<WorldLayout>,
) -> Option<Record> {
    if let Some(record) = consume_nurbs_auxiliary(stream, offset) {
        return Some(record);
    }
    if let Some(record) = consume_geometric_owner(stream, offset) {
        return Some(record);
    }
    if let Some(record) = consume_type_45(stream, offset) {
        return Some(record);
    }
    if include_type_67 {
        if let Some(record) = consume_type_67(stream, offset) {
            return Some(record);
        }
    }
    if let Some(record) = consume_list(stream, offset) {
        return Some(record);
    }
    if let Some(record) = consume_pointer_list_block(stream, offset) {
        return Some(record);
    }
    if let Some(record) = consume_world(stream, offset, world_layout) {
        return Some(record);
    }
    None
}

fn has_shareable_terminal(stream: &[u8], record: &Record) -> bool {
    if fixed_signature(record.kind(), intersection::IntersectionLayout::WithData).is_some_and(
        |signature| {
            signature
                .last()
                .is_some_and(|token| matches!(token, Token::Ref))
        },
    ) {
        return record
            .end
            .checked_sub(1)
            .and_then(|offset| stream.get(offset))
            == Some(&0);
    }
    if record.kind() == 84 {
        return true;
    }
    if record.kind() != 81 || record.canonical_bytes.last() != Some(&0) {
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
    let xmt = NonNullXmt::try_from(xmt).ok()?;
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

/// The merged partition image and collected terminal tombstone counts.
pub(crate) struct MergeFullRecordsResult {
    pub(crate) merged: Vec<u8>,
    pub(crate) unmatched_tombstones: BTreeMap<&'static str, usize>,
}

#[derive(Clone, Copy)]
enum MergeEvent {
    Full { offset: usize },
    Tombstone { offset: usize, kind: RecordKind },
}

/// Overlay supported complete deltas records onto one paired partition stream.
///
/// Replaced partition records are masked with non-tag bytes. Status-free
/// canonical current replacements are appended once. When BODY revision
/// envelopes are present, only records in the current interval of each body
/// sequence contribute to the current image. Raw current-revision deltas bytes
/// remain available to independent procedural decoders.
#[cfg(test)]
pub(crate) fn merge_full_records(partition: &[u8], deltas: &[u8]) -> Result<Vec<u8>, CodecError> {
    let census = walk(deltas)?;
    merge_full_records_with_census(partition, deltas, &census)
}

/// Merge deltas records with a previously decoded census.
pub(crate) fn merge_full_records_with_census(
    partition: &[u8],
    deltas: &[u8],
    census: &Census,
) -> Result<Vec<u8>, CodecError> {
    merge_records(partition, deltas, census, None)
}

/// Merge deltas records and collect unmatched terminal tombstone counts.
pub(crate) fn merge_full_records_with_tombstone_census(
    partition: &[u8],
    deltas: &[u8],
    census: &Census,
) -> Result<MergeFullRecordsResult, CodecError> {
    let mut unmatched_tombstones = BTreeMap::new();
    let merged = merge_records(partition, deltas, census, Some(&mut unmatched_tombstones))?;
    Ok(MergeFullRecordsResult {
        merged,
        unmatched_tombstones,
    })
}

fn merge_records(
    partition: &[u8],
    deltas: &[u8],
    census: &Census,
    unmatched_tombstones: Option<&mut BTreeMap<&'static str, usize>>,
) -> Result<Vec<u8>, CodecError> {
    let current_scopes = ScopeIndex::new(&current_revision_scopes(census, deltas.len()));
    let mut replacements = BTreeMap::<(u8, u32), &Record>::new();
    let mut unmatched_events =
        unmatched_tombstones.map(|totals| (totals, BTreeMap::<(u8, u32), Vec<MergeEvent>>::new()));
    for record in census
        .records
        .iter()
        .filter(|record| current_scopes.contains(record.offset))
    {
        let Ok(kind) = u8::try_from(record.kind()) else {
            continue;
        };
        if mergeable_record(record, kind) {
            replacements.insert((kind, record.xmt), record);
            if let Some((_, events)) = &mut unmatched_events {
                events
                    .entry((kind, record.xmt))
                    .or_default()
                    .push(MergeEvent::Full {
                        offset: record.offset,
                    });
            }
        }
    }

    let mut tombstones = BTreeMap::new();

    for tombstone in census.tombstones.iter().filter(|tombstone| {
        tombstone.kind != RecordKind::Intersection && current_scopes.contains(tombstone.offset)
    }) {
        let kind = tombstone.kind.code();
        tombstones.insert((kind, tombstone.xmt), tombstone);
        if let Some((_, events)) = &mut unmatched_events {
            events
                .entry((kind, tombstone.xmt))
                .or_default()
                .push(MergeEvent::Tombstone {
                    offset: tombstone.offset,
                    kind: tombstone.kind,
                });
        }
    }

    let graph = crate::topology::Graph::parse(partition);
    if let Some((totals, events)) = unmatched_events {
        *totals = count_unmatched_events(events, &graph);
    }
    let topology_carriers = graph.referenced_carrier_xmts();
    replacements.retain(|key, record| {
        tombstones
            .get(key)
            .is_none_or(|tombstone| record.offset > tombstone.offset)
    });
    let mut deletions = BTreeMap::new();
    for (key, tombstone) in tombstones {
        if NodeKind::try_from(key.0)
            .ok()
            .and_then(|kind| graph.get(kind, key.1))
            .is_some()
            && !topology_carriers.contains(&key.1)
            && replacements
                .get(&key)
                .is_none_or(|record| tombstone.offset > record.offset)
        {
            deletions.insert(key, tombstone);
        }
    }
    let build = |include_topology: bool| -> Vec<u8> {
        let included = |kind: u8| include_topology || !matches!(kind, 12..=19);
        let mut merged = partition.to_vec();
        for &(kind, xmt) in replacements.keys().chain(deletions.keys()) {
            if included(kind) {
                if let Some(node) = NodeKind::try_from(kind)
                    .ok()
                    .and_then(|kind| graph.get(kind, xmt))
                {
                    merged[node.pos()..node.end()].fill(0xff);
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
    if graph.body_shape_shells().next().is_some() {
        return Ok(build(false));
    }
    let merged = build(true);
    let merged_graph = crate::topology::Graph::parse(&merged);
    let base_complete = graph.has_complete_body_topology();
    let merged_complete = merged_graph.has_complete_body_topology();
    let deletes_owner = deletions.keys().any(|(kind, _)| matches!(kind, 12 | 13));
    let deleted_faces = deletions.keys().filter(|(kind, _)| *kind == 14).count();
    let accounted_faces = merged_graph
        .body_shape_face_count()
        .checked_add(deleted_faces)
        .ok_or_else(|| CodecError::Malformed("NX accounted face count overflow".into()))?;
    let unaccounted_face_loss = !deletes_owner && accounted_faces < graph.body_shape_face_count();
    if base_complete && (!merged_complete || unaccounted_face_loss) {
        Ok(build(false))
    } else {
        Ok(merged)
    }
}

/// Count terminal tombstones that have no exact carrier in the current image
/// and no earlier full-record addition in the current BODY revision.
///
/// Events are keyed by Parasolid type and XMT identity. A later full record
/// supersedes an earlier tombstone, while a full record followed by a
/// tombstone is a resolved deletion even when the base image lacked the key.
pub(crate) fn unmatched_terminal_tombstones(
    partition: &[u8],
    deltas: &[u8],
) -> Result<usize, CodecError> {
    Ok(unmatched_terminal_tombstones_by_family(partition, deltas)?
        .values()
        .sum())
}

/// Count unmatched terminal tombstones by Parasolid record family.
fn unmatched_terminal_tombstones_by_family(
    partition: &[u8],
    deltas: &[u8],
) -> Result<BTreeMap<&'static str, usize>, CodecError> {
    let census = walk(deltas)?;
    let graph = crate::topology::Graph::parse(partition);
    let events = collect_unmatched_events(&census, deltas.len());
    Ok(count_unmatched_events(events, &graph))
}

fn collect_unmatched_events(
    census: &Census,
    stream_len: usize,
) -> BTreeMap<(u8, u32), Vec<MergeEvent>> {
    let current_scopes = ScopeIndex::new(&current_revision_scopes(census, stream_len));
    let mut events = BTreeMap::<(u8, u32), Vec<MergeEvent>>::new();
    for record in census
        .records
        .iter()
        .filter(|record| current_scopes.contains(record.offset))
    {
        let Ok(kind) = u8::try_from(record.kind()) else {
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
    for tombstone in census.tombstones.iter().filter(|tombstone| {
        tombstone.kind != RecordKind::Intersection && current_scopes.contains(tombstone.offset)
    }) {
        let kind = tombstone.kind.code();
        events
            .entry((kind, tombstone.xmt))
            .or_default()
            .push(MergeEvent::Tombstone {
                offset: tombstone.offset,
                kind: tombstone.kind,
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
        events.sort_by(|first, second| {
            let offset = |event: &MergeEvent| match event {
                MergeEvent::Full { offset } | MergeEvent::Tombstone { offset, .. } => *offset,
            };
            offset(first).cmp(&offset(second))
        });
        let Some(MergeEvent::Tombstone {
            offset,
            kind: tombstone_kind,
        }) = events.last().copied()
        else {
            continue;
        };
        if NodeKind::try_from(kind).ok().and_then(|kind| graph.get(kind, xmt)).is_none()
            && !events.iter().any(|event| {
                matches!(event, MergeEvent::Full { offset: full_offset } if *full_offset < offset)
            })
        {
            *unmatched.entry(tombstone_kind.name()).or_default() += 1;
        }
    }
    unmatched
}

fn mergeable_record(record: &Record, kind: u8) -> bool {
    let Ok(kind) = NodeKind::try_from(kind) else {
        return false;
    };
    // Type-38 images in the rollback aggregate are saved transition states.
    // A BODY revision run does not select that transition. Preserve the
    // current partition relation and retain the saved fields in the ledger.
    if kind == NodeKind::Intersection {
        return false;
    }
    crate::topology::Graph::has_canonical_record(&record.canonical_bytes, kind, record.xmt)
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
    let mut snapshot_revisions = Vec::new();
    for (index, revision) in census.body_revisions.iter().enumerate() {
        if u32::from(revision.xmt) == 3 {
            snapshot_revisions.push(index);
        }
    }
    if snapshot_revisions.is_empty() {
        return vec![RevisionScope {
            start: 0,
            end: stream_len,
        }];
    }

    let direction = revision_direction(census, &snapshot_revisions);
    let mut run_starts = vec![0];
    for (position, pair) in snapshot_revisions.windows(2).enumerate() {
        let previous = census.body_revisions[pair[0]].node_id;
        let current = census.body_revisions[pair[1]].node_id;
        if !revision_follows_direction(previous, current, direction) {
            run_starts.push(position + 1);
        }
    }

    let mut scopes = Vec::new();
    for run in 0..run_starts.len() {
        let next_run_start = run_starts.get(run + 1).copied();
        // `run_starts` opens at zero, ascends strictly, and every pushed
        // element is under `snapshot_revisions.len()`, so each run holds at
        // least one snapshot revision and `run_end` is never zero.
        let run_end = next_run_start.unwrap_or(snapshot_revisions.len());
        let current_revision = &census.body_revisions[snapshot_revisions[run_end - 1]];
        let end = next_run_start.map_or(stream_len, |next_run_start| {
            census.body_revisions[snapshot_revisions[next_run_start]].offset
        });
        if current_revision.offset < end {
            scopes.push(RevisionScope {
                start: current_revision.offset,
                end,
            });
        }
    }
    scopes
}

fn revision_direction(census: &Census, revision_indices: &[usize]) -> RevisionDirection {
    // A stream can serialize one revision sequence in either direction. The
    // direction with fewer violations is the sequence direction; the opposite
    // transitions are the resets that begin another sequence.
    let ascending_violations = revision_indices
        .windows(2)
        .filter(|pair| {
            census.body_revisions[pair[1]].node_id < census.body_revisions[pair[0]].node_id
        })
        .count();
    let descending_violations = revision_indices
        .windows(2)
        .filter(|pair| {
            census.body_revisions[pair[1]].node_id > census.body_revisions[pair[0]].node_id
        })
        .count();
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

/// Revision scopes ordered by start for offset containment queries.
struct ScopeIndex {
    /// Each scope start in ascending order, with the furthest end of the
    /// scopes up to and including it.
    reach_by_start: Vec<(usize, usize)>,
}

impl ScopeIndex {
    fn new(scopes: &[RevisionScope]) -> Self {
        let mut spans = scopes
            .iter()
            .map(|scope| (scope.start, scope.end))
            .collect::<Vec<_>>();
        spans.sort_unstable();
        let mut reach = 0;
        let reach_by_start = spans
            .into_iter()
            .map(|(start, end)| {
                reach = reach.max(end);
                (start, reach)
            })
            .collect();
        Self { reach_by_start }
    }

    /// Return whether any scope satisfies `start <= offset < end`.
    fn contains(&self, offset: usize) -> bool {
        let started = self
            .reach_by_start
            .partition_point(|&(start, _)| start <= offset);
        started
            .checked_sub(1)
            .and_then(|last| self.reach_by_start.get(last))
            .is_some_and(|&(_, reach)| offset < reach)
    }
}

/// Return raw deltas bytes with decoded records and compact tombstones masked.
/// Historical BODY revision intervals are also masked. Current-revision records
/// needed by semantic scanners are appended in their partition form.
#[cfg(test)]
pub(crate) fn semantic_residual(stream: &[u8]) -> Result<Vec<u8>, CodecError> {
    let census = walk(stream)?;
    Ok(semantic_residual_with_census(stream, &census))
}

/// Return the semantic residual using a census already produced for the stream.
///
/// The census owns the complete-record boundaries and canonical forms used by
/// both topology merging and semantic scanning. Reusing it avoids a second
/// full walk of a large delta stream while keeping this transformation
/// byte-for-byte identical to `semantic_residual`.
pub(crate) fn semantic_residual_with_census(stream: &[u8], census: &Census) -> Vec<u8> {
    let current_scopes = current_revision_scopes(census, stream.len());
    let scope_index = ScopeIndex::new(&current_scopes);
    // Mask the whole stream and restore each current-revision scope. Restoring
    // the kept spans, rather than filling the gaps between them, needs no
    // ordering or disjointness relation between the scopes: every byte outside
    // every scope reads 0xff whatever order the scopes arrive in.
    let is_semantic = |record: &Record| {
        let is_current = scope_index.contains(record.offset);
        let is_semantic = matches!(
            record.kind(),
            40 | 41 | 45 | 59 | 81..=84 | 91 | 125..=128 | 135..=136 | 141 | 204
        );
        is_current && is_semantic
    };
    let mut residual = Vec::new();

    residual.extend_from_slice(stream);
    residual.fill(0xff);
    for scope in &current_scopes {
        residual[scope.start..scope.end].copy_from_slice(&stream[scope.start..scope.end]);
    }
    for record in &census.records {
        residual[record.offset..record.end].fill(0xff);
    }
    for tombstone in &census.tombstones {
        residual[tombstone.offset..tombstone.offset + 6].fill(0xff);
    }
    // A first-use type-38 schema event retains status-framed fields. It
    // cannot become a compact construction when the residual is appended to
    // a partition stream. Keep that saved state in the native schema arena.
    for declaration in &census.schema_nodes {
        if declaration.fields.kind() == 38 {
            residual[declaration.offset..declaration.end].fill(0xff);
        }
    }
    for record in census.records.iter().filter(|record| is_semantic(record)) {
        residual.extend_from_slice(&record.canonical_bytes);
    }
    residual
}

fn consume_fixed(
    stream: &[u8],
    offset: usize,
    kind: u16,
    signature: &[Token],
    world_layout: Option<WorldLayout>,
) -> Result<Option<Record>, CodecError> {
    let FixedEnvelope::Unique(candidate) = fixed_envelope(stream, offset, kind, signature) else {
        return Ok(None);
    };
    let shadows_type_101 = (candidate.offset + 1..candidate.end).any(|offset| {
        world_layout
            .and_then(|layout| world_shape(stream, offset, layout))
            .is_some_and(|(_, _, end)| end > candidate.end)
    });
    if shadows_type_101 {
        return Ok(None);
    }
    candidate.materialize(stream, signature).map(Some)
}

enum FixedEnvelope {
    Absent,
    Unique(FixedCandidate),
    Ambiguous { prefix_end: usize },
}

fn fixed_envelope(stream: &[u8], offset: usize, kind: u16, signature: &[Token]) -> FixedEnvelope {
    let direct = fixed_layout(stream, offset, kind, signature, 0);
    let escaped_marker = stream.get(offset + 2) == Some(&0xff);
    let escaped = if escaped_marker {
        fixed_layout(stream, offset, kind, signature, 1)
    } else {
        None
    };
    match (direct, escaped) {
        // The same ff can begin an extended XMT or mark a schema match.
        // Without established schema state, two complete local readings do
        // not establish which byte span or identity belongs to this record.
        (Some(direct), Some(escaped)) => FixedEnvelope::Ambiguous {
            prefix_end: direct.end.min(escaped.end),
        },
        (Some(candidate), None) | (None, Some(candidate)) => FixedEnvelope::Unique(candidate),
        (None, None) => FixedEnvelope::Absent,
    }
}

struct FixedCandidate {
    family: RecordFamily,
    xmt: u32,
    offset: usize,
    end: usize,
    prefix_end: usize,
}

impl FixedCandidate {
    fn materialize(self, stream: &[u8], signature: &[Token]) -> Result<Record, CodecError> {
        let mut canonical_bytes = Vec::new();
        canonical_bytes.extend_from_slice(&stream[self.offset..self.prefix_end]);
        let mut at = self.prefix_end;
        for token in signature {
            let (len, skip) = match token {
                Token::Ref => {
                    let (_, consumed) = read_xmt(stream, at).ok_or_else(|| {
                        CodecError::Malformed(
                            "NX deltas fixed record reference is truncated".into(),
                        )
                    })?;
                    (consumed, 1)
                }
                Token::Tolerance | Token::Scalar => (8, 0),
                Token::Sense
                | Token::OffsetDiscriminator
                | Token::BlendSubtype
                | Token::Boolean => (1, 0),
                Token::Position | Token::Vector => (24, 0),
            };
            canonical_bytes.extend_from_slice(&stream[at..at + len]);
            at += len + skip;
        }
        Ok(Record {
            family: self.family,
            xmt: self.xmt,
            canonical_bytes,
            offset: self.offset,
            end: self.end,
        })
    }
}

fn fixed_layout(
    stream: &[u8],
    offset: usize,
    kind: u16,
    signature: &[Token],
    envelope_len: usize,
) -> Option<FixedCandidate> {
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
    stream.get(offset..at)?;
    let prefix_end = at;
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
                stream.get(start..start + consumed)?;
                references.push(reference);
            }
            Token::Tolerance => {
                let tolerance = View::f64_be_at(stream, at)?;
                (tolerance.is_finite()
                    && (!matches!(kind, 16 | 18) || tolerance.abs() >= 1.0e-100))
                    .then_some(())?;
                stream.get(at..at + 8)?;
                at += 8;
            }
            Token::Sense => {
                matches!(stream.get(at), Some(b'+' | b'-')).then_some(())?;
                stream.get(at)?;
                at += 1;
            }
            Token::OffsetDiscriminator => {
                matches!(stream.get(at), Some(b'V' | b'I' | b'U')).then_some(())?;
                stream.get(at)?;
                at += 1;
            }
            Token::BlendSubtype => {
                (stream.get(at) == Some(&b'R')).then_some(())?;
                at += 1;
            }
            Token::Boolean => {
                matches!(stream.get(at), Some(0 | 1)).then_some(())?;
                stream.get(at)?;
                at += 1;
            }
            Token::Position => {
                let xyz = vec3_be_at(stream, at)?;
                position = Some(FixedPosition::new(kind, xyz)?);
                stream.get(at..at + 24)?;
                at += 24;
            }
            Token::Vector => {
                let xyz = vec3_be_at(stream, at)?;
                xyz.iter().all(|value| value.is_finite()).then_some(())?;
                stream.get(at..at + 24)?;
                at += 24;
            }
            Token::Scalar => {
                View::f64_be_at(stream, at)?.is_finite().then_some(())?;
                stream.get(at..at + 8)?;
                at += 8;
            }
        }
    }
    Some(FixedCandidate {
        family: RecordFamily::from_fixed_admitted(kind, node_id, position, references)?,
        xmt,
        offset,
        end: at,
        prefix_end,
    })
}

fn consume_variable(stream: &[u8], offset: usize, kind: u16) -> Option<Record> {
    if kind == 90 {
        return consume_group(stream, offset);
    }
    if kind == 91 {
        return consume_type_91(stream, offset);
    }
    let (xmt, byte_len, family) = (|| {
        Some(match kind {
            81 => {
                let record = crate::parasolid::entity_51_record_at(stream, offset)?;
                (
                    record.xmt.into(),
                    record.byte_len,
                    RecordFamily::Entity51 {
                        leading_references: record.leading_references,
                        trailing_references: record.trailing_references,
                    },
                )
            }
            82..=89 | 98 => {
                let (parsed_kind, xmt, byte_len) =
                    crate::parasolid::value_records::entity_value_record_identity_at(
                        stream, offset,
                    )?;
                (parsed_kind == kind).then_some(())?;
                let family = match parsed_kind {
                    82 => RecordFamily::Entity52,
                    83 => RecordFamily::Entity53,
                    84 => RecordFamily::Entity54,
                    85 => RecordFamily::Entity55,
                    86 => RecordFamily::Entity56,
                    87 => RecordFamily::Entity57,
                    88 => RecordFamily::Entity58,
                    89 => RecordFamily::Entity59,
                    98 => RecordFamily::Entity62,
                    _ => return None,
                };
                (xmt, byte_len, family)
            }
            _ => return None,
        })
    })()?;
    let end = offset.checked_add(byte_len)?;
    let bytes = stream.get(offset..end)?;
    Some(Record {
        family,
        xmt,
        canonical_bytes: bytes.to_vec(),
        offset,
        end,
    })
}

fn is_value_family(kind: u16) -> bool {
    matches!(kind, 82..=89 | 98)
}

fn admitted_record(
    stream: &[u8],
    offset: usize,
    end: usize,
    family: RecordFamily,
    xmt: u32,
) -> Option<Record> {
    let bytes = stream.get(offset..end)?;
    Some(Record {
        family,
        xmt,
        canonical_bytes: bytes.to_vec(),
        offset,
        end,
    })
}

fn consume_group(stream: &[u8], offset: usize) -> Option<Record> {
    let parsed = (|| {
        (View::u16_be_at(stream, offset) == Some(90)).then_some(())?;
        let direct = group_layout(stream, offset, 0);
        let escaped_marker = stream.get(offset + 2) == Some(&0xff);
        let escaped = escaped_marker
            .then(|| group_layout(stream, offset, 1))
            .flatten();
        let (xmt, node_id, references, selector, linked_reference_status, end) =
            select_enveloped_layout(escaped_marker, direct, escaped)?;
        Some((
            RecordFamily::Group {
                references: references.try_into().ok()?,
                node_id,
                selector,
                linked_reference_status,
            },
            xmt,
            end,
        ))
    })();
    let (family, xmt, end) = parsed?;
    admitted_record(stream, offset, end, family, xmt)
}

fn consume_pointer_list_block(stream: &[u8], offset: usize) -> Option<Record> {
    let parsed = (|| {
        (View::u16_be_at(stream, offset) == Some(74)).then_some(())?;
        let direct = pointer_list_block_shape(stream, offset.checked_add(2)?);
        let escaped_marker = stream.get(offset + 2) == Some(&0xff);
        let escaped = escaped_marker
            .then(|| pointer_list_block_shape(stream, offset.checked_add(3)?))
            .flatten();
        select_enveloped_layout(escaped_marker, direct, escaped)
    })();
    let shape = parsed?;
    let state = materialize_pointer_list_block(stream, shape)?;
    admitted_record(
        stream,
        offset,
        shape.end,
        RecordFamily::PointerListBlock { fields: state },
        shape.xmt,
    )
}

fn consume_list(stream: &[u8], offset: usize) -> Option<Record> {
    let parsed = (|| {
        (View::u16_be_at(stream, offset) == Some(70)).then_some(())?;
        let direct = list_body(stream, offset.checked_add(2)?);
        let escaped_marker = stream.get(offset + 2) == Some(&0xff);
        let escaped = escaped_marker
            .then(|| list_body(stream, offset.checked_add(3)?))
            .flatten();
        let (xmt, state, end) = select_enveloped_layout(escaped_marker, direct, escaped)?;
        Some((RecordFamily::List { fields: state }, xmt, end))
    })();
    let (family, xmt, end) = parsed?;
    admitted_record(stream, offset, end, family, xmt)
}

fn list_body(stream: &[u8], body: usize) -> Option<(u32, ListFields, usize)> {
    let mut view = View::over_retained(stream);
    view.seek(body)?;
    let mut at = body;
    let xmt = crate::framing::read_and_advance(stream, &mut at)?;
    view.seek(at)?;
    NonNullXmt::try_from(xmt).ok()?;
    let node_id = view.u32_be()?;
    let list_type = view.u8()?;
    let notransmit = match view.u8()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let owner = read_list_reference(stream, &mut view, 1)?;
    let next = read_list_reference(stream, &mut view, 1)?;
    let previous = read_list_reference(stream, &mut view, 1)?;
    let list_length = view.u32_be()?;
    let block_length = view.u32_be()?;
    let finger_index = view.u32_be()?;
    let finger_block = read_list_reference(stream, &mut view, 0)?;
    let list_block = read_list_reference(stream, &mut view, 0)?;
    let fields = ListFields::try_from(ListWire {
        node_id,
        list_type,
        notransmit,
        owner,
        next,
        previous,
        list_length,
        block_length,
        finger_index,
        finger_block,
        list_block,
    })
    .ok()?;
    Some((xmt, fields, view.position()))
}

fn read_list_reference(stream: &[u8], view: &mut View<'_>, status: u8) -> Option<u32> {
    let mut at = view.position();
    let value = crate::framing::read_and_advance(stream, &mut at)?;
    view.seek(at)?;
    (view.u8()? == status).then_some(value)
}

fn consume_world(stream: &[u8], offset: usize, layout: Option<WorldLayout>) -> Option<Record> {
    let (xmt, references, end) = layout.and_then(|layout| world_shape(stream, offset, layout))?;
    admitted_record(stream, offset, end, RecordFamily::World { references }, xmt)
}

fn world_shape(
    stream: &[u8],
    offset: usize,
    layout: WorldLayout,
) -> Option<(u32, world::WorldReferences, usize)> {
    (View::u16_be_at(stream, offset) == Some(101)).then_some(())?;
    let at = offset.checked_add(2)?;
    let direct = layout.read(stream, at);
    let escaped_marker = stream.get(at) == Some(&255);
    let escaped = escaped_marker
        .then(|| layout.read(stream, at.checked_add(1)?))
        .flatten();
    select_enveloped_layout(escaped_marker, direct, escaped)
}

#[derive(Clone, Copy)]
struct PointerListBlockShape {
    xmt: u32,
    slot_count: u32,
    active_count: u32,
    next_block: u32,
    references_start: usize,
    end: usize,
}

fn pointer_list_block_shape(stream: &[u8], body: usize) -> Option<PointerListBlockShape> {
    let slot_count_value = View::u32_be_at(stream, body)?;
    let slot_count = usize::try_from(slot_count_value).ok()?;
    (slot_count > 0).then_some(())?;
    let (xmt, consumed) = read_xmt(stream, body.checked_add(4)?)?;
    NonNullXmt::try_from(xmt).ok()?;
    let mut at = body.checked_add(4 + consumed)?;
    let active_count_value = View::u32_be_at(stream, at)?;
    (active_count_value <= slot_count_value).then_some(())?;
    at += 4;
    (View::u32_be_at(stream, at) == Some(0)).then_some(())?;
    at += 4;
    let remaining = stream.len().checked_sub(at)?;
    (slot_count <= remaining / 3).then_some(())?;
    let (next_block, consumed) = read_xmt(stream, at)?;
    std::num::NonZeroU32::new(next_block)?;
    at = at.checked_add(consumed)?;
    (stream.get(at) == Some(&1)).then_some(())?;
    at += 1;
    let references_start = at;
    for index in 0..slot_count {
        let (reference, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        (stream.get(at) == Some(&1)).then_some(())?;
        at += 1;
        if index < usize::try_from(active_count_value).ok()? {
            NonNullXmt::try_from(reference).ok()?;
        } else {
            (reference == 1).then_some(())?;
        }
    }
    Some(PointerListBlockShape {
        xmt,
        slot_count: slot_count_value,
        active_count: active_count_value,
        next_block,
        references_start,
        end: at,
    })
}

fn materialize_pointer_list_block(
    stream: &[u8],
    shape: PointerListBlockShape,
) -> Option<PointerListBlockFields> {
    let count = usize::try_from(shape.slot_count).ok()?;
    let mut references = Vec::new();
    let mut at = shape.references_start;
    for _ in 0..count {
        let (reference, consumed) = read_xmt(stream, at)?;
        let next = at
            .checked_add(consumed)
            .and_then(|next| next.checked_add(1))?;
        at = next;
        references.push(reference);
    }
    PointerListBlockFields::new(shape.active_count, shape.next_block, references).ok()
}

fn group_layout(
    stream: &[u8],
    offset: usize,
    envelope_len: usize,
) -> Option<(
    u32,
    u32,
    Vec<u32>,
    GroupSelector,
    GroupReferenceStatus,
    usize,
)> {
    group_layout_with_statuses(stream, offset, envelope_len)
        .or_else(|| group_layout_without_leading_statuses(stream, offset, envelope_len))
}

fn group_layout_with_statuses(
    stream: &[u8],
    offset: usize,
    envelope_len: usize,
) -> Option<(
    u32,
    u32,
    Vec<u32>,
    GroupSelector,
    GroupReferenceStatus,
    usize,
)> {
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
    let selector = group::selector(*stream.get(at)?)?;
    at += 1;
    let (reference, consumed) = read_xmt(stream, at)?;
    at = at.checked_add(consumed)?;
    let linked_reference_status = GroupReferenceStatus::try_from(*stream.get(at)?).ok()?;
    at += 1;
    references.push(reference);
    Some((
        xmt,
        node_id,
        references,
        selector,
        linked_reference_status,
        at,
    ))
}

fn group_layout_without_leading_statuses(
    stream: &[u8],
    offset: usize,
    envelope_len: usize,
) -> Option<(
    u32,
    u32,
    Vec<u32>,
    GroupSelector,
    GroupReferenceStatus,
    usize,
)> {
    let (xmt, consumed) = read_xmt(stream, offset.checked_add(2 + envelope_len)?)?;
    (xmt > 1).then_some(())?;
    let mut at = offset.checked_add(2 + envelope_len + consumed)?;
    let node_id = View::u32_be_at(stream, at)?;
    at += 4;
    let mut references = Vec::new();
    for _ in 0..4 {
        let (reference, consumed) = read_xmt(stream, at)?;
        at = at.checked_add(consumed)?;
        references.push(reference);
    }
    let selector = group::selector(*stream.get(at)?)?;
    at += 1;
    let (reference, consumed) = read_xmt(stream, at)?;
    at = at.checked_add(consumed)?;
    let linked_reference_status = GroupReferenceStatus::try_from(*stream.get(at)?).ok()?;
    at += 1;
    references.push(reference);
    Some((
        xmt,
        node_id,
        references,
        selector,
        linked_reference_status,
        at,
    ))
}

fn consume_type_91(stream: &[u8], offset: usize) -> Option<Record> {
    let parsed = (|| {
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
        Some((
            RecordFamily::Type91 {
                references: references.try_into().ok()?,
            },
            xmt,
            end,
        ))
    })();
    let (family, xmt, end) = parsed?;
    admitted_record(stream, offset, end, family, xmt)
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

fn consume_geometric_owner(stream: &[u8], offset: usize) -> Option<Record> {
    let parsed = (|| {
        (View::u16_be_at(stream, offset) == Some(141)).then_some(())?;
        let direct = geometric_owner_layout(stream, offset, 0);
        let escaped_marker = stream.get(offset + 2) == Some(&0xff);
        let escaped = escaped_marker
            .then(|| geometric_owner_layout(stream, offset, 1))
            .flatten();
        let (xmt, references, end) = if escaped_marker {
            escaped.or(direct)?
        } else {
            direct?
        };
        Some((
            {
                let [owner, next, previous, shared_geometry] = references;
                RecordFamily::GeometricOwner {
                    owner,
                    next,
                    previous,
                    shared_geometry,
                }
            },
            xmt,
            end,
        ))
    })();
    let (family, xmt, end) = parsed?;
    admitted_record(stream, offset, end, family, xmt)
}

fn consume_type_45(stream: &[u8], offset: usize) -> Option<Record> {
    (View::u16_be_at(stream, offset) == Some(45)).then_some(())?;
    let direct = type_45_layout(stream, offset, 0);
    let escaped_marker = stream.get(offset + 2) == Some(&0xff);
    let escaped = escaped_marker
        .then(|| type_45_layout(stream, offset, 1))
        .flatten();
    let (xmt, end) = select_enveloped_layout(escaped_marker, direct, escaped)?;
    admitted_record(stream, offset, end, RecordFamily::Type45, xmt)
}

fn consume_type_67(stream: &[u8], offset: usize) -> Option<Record> {
    let parsed = (|| {
        (View::u16_be_at(stream, offset) == Some(67)).then_some(())?;
        let direct = type_67_layout(stream, offset, 0);
        let escaped_marker = stream.get(offset + 2) == Some(&0xff);
        let escaped = escaped_marker
            .then(|| type_67_layout(stream, offset, 1))
            .flatten();
        let (xmt, node_id, references, end) =
            select_enveloped_layout(escaped_marker, direct, escaped)?;
        Some((
            RecordFamily::Type67 {
                node_id,
                references: references.try_into().ok()?,
            },
            xmt,
            end,
        ))
    })();
    let (family, xmt, end) = parsed?;
    admitted_record(stream, offset, end, family, xmt)
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
    let end = data_at.checked_add(count.checked_mul(8)?)?;
    let raw = stream.get(data_at..end)?;
    (0..count)
        .all(|i| {
            View::f64_be_at(raw, i * 8)
                .is_some_and(|value| value.is_finite() && (value == 0.0 || value.is_normal()))
        })
        .then_some(())?;
    Some((xmt, end))
}

fn geometric_owner_layout(
    stream: &[u8],
    offset: usize,
    envelope_len: usize,
) -> Option<(u32, [crate::framing::xmt_reference::NonNullXmt; 4], usize)> {
    use crate::framing::xmt_reference::NonNullXmt;

    let (xmt, consumed) = read_xmt(stream, offset.checked_add(2 + envelope_len)?)?;
    (xmt > 1).then_some(())?;
    let mut at = offset.checked_add(2 + envelope_len + consumed)?;
    let mut reference = |required_status: Option<u8>| {
        let (value, consumed) = read_xmt(stream, at)?;
        let target = NonNullXmt::try_from(value).ok()?;
        at = at.checked_add(consumed)?;
        let status = *stream.get(at)?;
        required_status
            .map_or(matches!(status, 0 | 1), |required| status == required)
            .then_some(())?;
        at = at.checked_add(1)?;
        Some(target)
    };
    let owner = reference(None)?;
    let next = reference(Some(0))?;
    let previous = reference(Some(0))?;
    let shared_geometry = reference(None)?;
    Some((xmt, [owner, next, previous, shared_geometry], at))
}

fn select_enveloped_layout<T>(
    escaped_marker: bool,
    direct: Option<T>,
    escaped: Option<T>,
) -> Option<T> {
    if escaped_marker {
        escaped.or(direct)
    } else {
        direct
    }
}

fn consume_intersection_auxiliary(stream: &[u8], offset: usize) -> Option<Record> {
    let (family, xmt, end) = if let Some((chart, end)) = crate::intersection::chart_source_record_at(
        stream,
        offset,
        crate::intersection::ChartPointLayout::Ext11,
    ) {
        (RecordFamily::Chart, u32::from(chart.xmt), end)
    } else if let Some((term, end)) = crate::intersection::term_use_at(
        stream,
        offset,
        crate::intersection::ChartPointLayout::Ext11,
    ) {
        (RecordFamily::TermUse, u32::from(term.xmt), end)
    } else if let Some((bound, end)) = crate::intersection::blend_bound_at(stream, offset) {
        let headers = bound.state.header_references();
        let references = [
            headers[0],
            headers[1],
            headers[2],
            headers[3],
            headers[4],
            bound.state.boundary_index(),
            bound.state.blend_surface(),
        ];
        (
            RecordFamily::BlendBound { references },
            bound.state.xmt(),
            end,
        )
    } else {
        let (support_uv, end) = crate::intersection::support_uv_record_at(stream, offset)?;
        (RecordFamily::SupportUv, u32::from(support_uv.xmt), end)
    };
    let bytes = stream.get(offset..end)?;
    let canonical_bytes = bytes.to_vec();
    Some(Record {
        family,
        xmt,
        canonical_bytes,
        offset,
        end,
    })
}

fn consume_nurbs_auxiliary(stream: &[u8], offset: usize) -> Option<Record> {
    let auxiliary = crate::nurbs::auxiliary_record_at(stream, offset)?;
    admitted_record(
        stream,
        offset,
        auxiliary.end,
        auxiliary.family,
        auxiliary.xmt,
    )
}

fn compact_tombstone(stream: &[u8], offset: usize) -> Option<u32> {
    let first = View::i16_be_at(stream, offset + 2)?;
    if first < 0 {
        let quotient = View::u16_be_at(stream, offset + 4)?;
        return (quotient == 1)
            .then_some(u32::from(quotient) * 32_767 + u32::from(first.unsigned_abs()));
    }
    (stream.get(offset + 4..offset + 6)? == [0, 1]).then_some(u32::try_from(first).ok()?)
}

fn fixed_signature(
    kind: u16,
    intersection_layout: intersection::IntersectionLayout,
) -> Option<&'static [Token]> {
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
        38 => return intersection_layout.signature(),
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
    use super::census::walk;
    use super::record_family::RecordFamily;
    use super::{consume_type_67, select_enveloped_layout, Record};

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
            let census = walk(&bytes).unwrap();

            assert!(matches!(
                census.records.as_slice(),
                [Record {
                    family: RecordFamily::Type67 { node_id: 1_061, references },
                    xmt: 67,
                    canonical_bytes,
                    end,
                    ..
                }] if references == &[1, 3, 440, 10, 149, 71]
                    && canonical_bytes == &bytes
                    && *end == bytes.len()
            ));
            assert_eq!(census.bytes_decoded(), bytes.len());
        }
    }

    #[test]
    fn retains_type_67_before_an_unknown_successor() {
        let mut bytes = record(true);
        let record_end = bytes.len();
        bytes.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);

        let parsed = consume_type_67(&bytes, 0).expect("complete current-record grammar");

        assert_eq!(parsed.end, record_end);
        assert_eq!(parsed.canonical_bytes, bytes[..record_end]);
    }

    #[test]
    fn escaped_marker_selects_the_complete_escaped_form() {
        #[derive(Debug, PartialEq, Eq)]
        struct Candidate(&'static str);

        assert_eq!(
            select_enveloped_layout(
                true,
                Some(Candidate("coincidental direct")),
                Some(Candidate("escaped")),
            ),
            Some(Candidate("escaped"))
        );
        assert_eq!(
            select_enveloped_layout(false, Some(Candidate("direct")), None),
            Some(Candidate("direct"))
        );
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
mod type_45_record_tests {
    use super::consume_type_45;

    #[test]
    fn declared_lane_ends_before_an_unknown_successor() {
        let mut bytes = 45u16.to_be_bytes().to_vec();
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.extend_from_slice(&1.0f64.to_be_bytes());
        let record_end = bytes.len();
        bytes.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);

        let parsed = consume_type_45(&bytes, 0).expect("consume the type 45 record");

        assert_eq!(parsed.end, record_end);
        assert_eq!(parsed.canonical_bytes, bytes[..record_end]);
    }
}

#[cfg(test)]
mod plane_boundary_tests {
    use super::census::walk;
    use crate::test_support::test_bytes::encoded_xmt;

    #[test]
    fn plane_node_id_low_byte_is_not_an_independent_record() {
        for node_id in [150u32, 406, 662, 2_454, 36_246, 36_502] {
            let mut bytes = 50u16.to_be_bytes().to_vec();
            bytes.extend(encoded_xmt(66_706));
            bytes.extend(node_id.to_be_bytes());
            let mut canonical = bytes.clone();
            for (reference, status) in [(1, 1), (3, 1), (6_192, 0), (66_545, 1), (66_707, 0)] {
                bytes.extend(encoded_xmt(reference));
                bytes.push(status);
                canonical.extend(encoded_xmt(reference));
            }
            bytes.push(b'+');
            canonical.push(b'+');
            for value in [-0.025f64, -0.05, 0.213_334, 0.0, 1.0, 0.0, 0.0, -0.0, 1.0] {
                bytes.extend(value.to_be_bytes());
                canonical.extend(value.to_be_bytes());
            }
            let plane_end = bytes.len();
            bytes.extend(141u16.to_be_bytes());
            bytes.extend(encoded_xmt(66_707));
            for (reference, status) in [(66_709, 1), (66_708, 0), (66_708, 0), (66_706, 1)] {
                bytes.extend(encoded_xmt(reference));
                bytes.push(status);
            }

            let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();

            assert_eq!(bytes[9], 150);
            assert_eq!(plane_end, 102);
            assert_eq!(census.records.len(), 2);
            let plane = &census.records[0];
            assert_eq!(
                (plane.kind(), plane.xmt, plane.offset, plane.end),
                (50, 66_706, 0, plane_end)
            );
            assert_eq!(plane.canonical_bytes, canonical);
            let owner = &census.records[1];
            assert_eq!(
                (owner.kind(), owner.xmt, owner.offset, owner.end),
                (141, 66_707, plane_end, bytes.len())
            );
            assert!(census.tombstones.is_empty());
            assert_eq!(census.bytes_decoded(), bytes.len());
        }
    }
}

#[cfg(test)]
mod partition_prefix_tests {
    use super::census::walk;
    use crate::test_support::test_pmarks;

    #[test]
    fn leading_nodes_are_separate_from_the_counted_header() {
        let bytes = test_pmarks::prefix(1);
        let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();
        let header = census.transmit_header.as_ref().unwrap();
        assert_eq!(header.end, test_pmarks::header().len());
        let prefix = census.partition_prefix.as_ref().unwrap();
        assert_eq!(prefix.offset, header.end);
        assert_eq!(prefix.end, bytes.len());
        assert_eq!(prefix.state.highest_id(), u32::MAX);
        assert_eq!(prefix.state.current_pmark().identifier(), 0x1234_5678);
        assert_eq!(census.bytes_decoded(), bytes.len());
        assert!(census.records.is_empty());
        assert!(census.tombstones.is_empty());
    }

    #[test]
    fn a_full_pmark_identifier_owns_null_trailer_shaped_bytes() {
        let mut bytes = test_pmarks::prefix(1);
        let start = bytes.len() - 4;
        bytes[start..].copy_from_slice(&0x0001_0001_u32.to_be_bytes());
        let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();
        let prefix = census.partition_prefix.as_ref().unwrap();
        assert_eq!(prefix.state.current_pmark().identifier(), 0x0001_0001);
        assert!(census.terminal_null_references.is_none());
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn incomplete_prefix_preserves_the_complete_header() {
        let bytes = test_pmarks::prefix(1);
        for end in test_pmarks::header().len()..bytes.len() {
            let census =
                crate::test_support::with_decode_context(|_ctx| walk(&bytes[..end])).unwrap();
            assert!(census.transmit_header.is_some());
            assert!(census.partition_prefix.is_none());
        }
    }
}

#[cfg(test)]
mod nurbs_auxiliary_tests {
    use super::census::walk;

    fn status_framed_curve_descriptor() -> Vec<u8> {
        vec![
            0, 136, 0x0d, 0xd1, // type and XMT
            0, 3, // degree
            0, 0, 0, 4, // pole count
            0, 4, // homogeneous dimension
            0, 0, 0, 2, // distinct-knot count
            1, 0, 0, 1, 4, // form and reference-lane prefix
            0x0d, 0xd4, 0, // control-array reference
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
            0xb8, 0xf9, 0, 1, 0, // control-array reference
            0xb8, 0xfa, 0, 1, 0, // multiplicity reference
            0xb8, 0xfb, 0, 1, 0, // knot reference
        ]
    }

    #[test]
    fn retains_status_framed_rational_curve_descriptor() {
        let bytes = status_framed_curve_descriptor();
        let census = walk(&bytes).unwrap();

        assert_eq!(census.records.len(), 1);
        assert_eq!(census.records[0].kind(), 136);
        assert_eq!(census.records[0].xmt, 3_537);
        assert_eq!(census.records[0].family.references(), [3_540, 3_539, 3_538]);
        assert_eq!(census.records[0].end, bytes.len());
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn descriptor_form_byte_does_not_select_reference_cardinality() {
        for form in [0, 1, 4, 5, 255] {
            for flags in [[0, 0, 1], [1, 1, 0]] {
                let mut bytes = status_framed_curve_descriptor();
                bytes[17..20].copy_from_slice(&flags);
                bytes[20] = form;
                let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();

                assert_eq!(census.records.len(), 1);
                assert_eq!(census.records[0].family.references(), [3_540, 3_539, 3_538]);
                assert_eq!(census.records[0].end, bytes.len());
            }
        }
    }

    #[test]
    fn rejects_incomplete_or_noncanonical_curve_descriptor_state() {
        let bytes = status_framed_curve_descriptor();
        let mut bad_status = bytes.clone();
        bad_status[23] = 1;
        let mut bad_dimension = bytes.clone();
        bad_dimension[11] = 5;

        for malformed in [&bytes[..bytes.len() - 1], &bad_status, &bad_dimension] {
            assert!(walk(malformed).unwrap().records.is_empty());
        }
    }

    #[test]
    fn retains_escaped_curve_descriptor_with_extended_state_references() {
        let bytes = escaped_curve_descriptor_with_extended_references();
        let census = walk(&bytes).unwrap();

        assert_eq!(census.records.len(), 1);
        assert_eq!(census.records[0].kind(), 136);
        assert_eq!(census.records[0].xmt, 50_947);
        assert_eq!(
            census.records[0].family.references(),
            [50_950, 50_949, 50_948]
        );
        assert_eq!(census.records[0].end, bytes.len());
        assert_eq!(census.bytes_decoded(), bytes.len());
    }
}

#[cfg(test)]
mod reference_type_map_tests {
    use super::census::walk;
    use super::record_kind::RecordKind;
    use super::{ReferenceTypeMap, Tombstone};
    use std::num::NonZeroU16;

    fn map_bytes() -> Vec<u8> {
        vec![
            0, 1, 0, 1, 0xe3, 0xbf, 0, 1, 0, 81, 0, 3, 0, 100, 0, 1, 0, 0, 0, 55,
        ]
    }

    #[test]
    fn reference_type_map_accepts_compact_and_extended_references() {
        let bytes = map_bytes();
        let census = walk(&bytes).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(40_000, 81), (3, 100)].try_into().unwrap(),
                terminal_word: NonZeroU16::new(55),
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn reference_type_map_accepts_status_prefixed_null_reference() {
        let canonical = map_bytes();
        let mut bytes = vec![1, 0, 1];
        bytes.extend_from_slice(&canonical[4..]);
        let census = walk(&bytes).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(40_000, 81), (3, 100)].try_into().unwrap(),
                terminal_word: NonZeroU16::new(55),
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn reference_type_map_retains_terminal_word_one() {
        let mut bytes = map_bytes();
        bytes[18..].copy_from_slice(&[0, 1]);

        let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();

        assert_eq!(census.reference_type_maps.len(), 1);
        assert_eq!(
            census.reference_type_maps[0]
                .terminal_word
                .map(NonZeroU16::get),
            Some(1)
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn reference_type_map_retains_raw_terminal_word() {
        let mut bytes = map_bytes();
        bytes[18..].copy_from_slice(&323u16.to_be_bytes());

        let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();

        assert_eq!(census.reference_type_maps.len(), 1);
        assert_eq!(
            census.reference_type_maps[0]
                .terminal_word
                .map(NonZeroU16::get),
            Some(323)
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn reference_type_map_retains_full_width_terminal_word() {
        let mut bytes = map_bytes();
        bytes[18..].copy_from_slice(&u16::MAX.to_be_bytes());

        let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();

        assert_eq!(census.reference_type_maps.len(), 1);
        assert_eq!(
            census.reference_type_maps[0]
                .terminal_word
                .map(NonZeroU16::get),
            Some(u16::MAX)
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn reference_type_map_precedes_counted_record_at_shared_kind() {
        let map = vec![0, 1, 0, 1, 0, 3, 0, 82, 0, 1, 0, 0, 2, 100];
        let mut bytes = map.clone();
        bytes.resize(bytes.len() + 65_536 * 4, 0);

        let census = walk(&bytes).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 82)].try_into().unwrap(),
                terminal_word: NonZeroU16::new(612),
                offset: 0,
                end: map.len(),
            }]
        );
        assert!(census.records.is_empty());
        assert_eq!(census.bytes_decoded(), map.len());
    }

    #[test]
    fn reference_type_map_accepts_entry_table_without_terminal_clause() {
        let bytes = vec![0, 1, 0, 1, 0, 3, 0, 81, 0, 4, 0, 100];

        let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 81), (4, 100)].try_into().unwrap(),
                terminal_word: None,
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn reference_type_map_shares_its_final_kind_with_a_tombstone() {
        let bytes = vec![0, 1, 0, 1, 0, 3, 0, 81, 0, 9, 0, 1];

        let census = walk(&bytes).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 81)].try_into().unwrap(),
                terminal_word: None,
                offset: 0,
                end: 8,
            }]
        );
        assert_eq!(
            census.tombstones,
            [Tombstone {
                kind: RecordKind::Entity51,
                xmt: 9,
                offset: 6,
            }]
        );
        assert_eq!(census.bytes_decoded(), bytes.len());

        let mut incomplete_tombstone = bytes;
        incomplete_tombstone[11] = 2;
        assert!(walk(&incomplete_tombstone)
            .unwrap()
            .reference_type_maps
            .is_empty());
    }

    #[test]
    fn reference_type_map_accepts_terminal_clause_without_word() {
        let bytes = vec![1, 0, 1, 0, 3, 0, 81, 0, 4, 0, 100, 0, 1, 0, 0];

        let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 81), (4, 100)].try_into().unwrap(),
                terminal_word: None,
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn reference_type_map_accepts_map_only_type_codes() {
        let bytes = vec![1, 0, 1, 0, 3, 0, 67, 0, 4, 0, 11, 0, 1, 0, 0, 0, 61];

        let census = walk(&bytes).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 67), (4, 11)].try_into().unwrap(),
                terminal_word: NonZeroU16::new(61),
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn reference_type_map_is_discovered_after_state_packet_prefix() {
        let mut bytes = crate::test_support::test_pmarks::prefix(40_000);
        bytes.extend(crate::test_support::test_pmarks::packet());
        let map_offset = bytes.len();
        bytes.extend(map_bytes());

        let census = walk(&bytes).unwrap();

        assert_eq!(census.pmark_packets.len(), 1);
        assert_eq!(census.reference_type_maps.len(), 1);
        assert_eq!(census.reference_type_maps[0].offset, map_offset);
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn terminal_word_self_delimits_reference_type_map_prefixes() {
        let first = map_bytes();
        let mut bytes = first.clone();
        bytes.extend(map_bytes());

        let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();

        assert_eq!(census.reference_type_maps.len(), 2);
        assert_eq!(census.reference_type_maps[0].offset, 0);
        assert_eq!(census.reference_type_maps[0].end, first.len());
        assert_eq!(census.reference_type_maps[1].offset, first.len());
        assert_eq!(census.reference_type_maps[1].end, bytes.len());
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn reference_type_map_requires_complete_framing_and_known_types() {
        let bytes = map_bytes();
        let mut unknown_entry_type = bytes.clone();
        unknown_entry_type[9] = 0xfe;
        let mut zero_terminal_word = bytes.clone();
        zero_terminal_word[18..].copy_from_slice(&0u16.to_be_bytes());

        for malformed in [
            &bytes[..bytes.len() - 1],
            unknown_entry_type.as_slice(),
            zero_terminal_word.as_slice(),
        ] {
            assert!(walk(malformed).unwrap().reference_type_maps.is_empty());
        }
    }
}

#[cfg(test)]
mod terminal_null_reference_tests {
    use super::census::walk;

    #[test]
    fn retains_two_or_four_null_references_at_the_stream_boundary() {
        let four_references = [0, 1, 0, 1, 0, 1, 0, 1];
        let census = walk(&four_references).unwrap();

        assert_eq!(
            census.terminal_null_references.map(|tail| (
                tail.offset(),
                tail.end(),
                tail.form().references().len()
            )),
            Some((0, four_references.len(), 4))
        );
        assert_eq!(census.bytes_decoded(), four_references.len());

        let two_references = [0, 1, 0, 1];
        assert_eq!(
            walk(&two_references)
                .unwrap()
                .terminal_null_references
                .map(|tail| (tail.offset(), tail.end(), tail.form().references().len())),
            Some((0, two_references.len(), 2))
        );

        let mut nonterminal = four_references.to_vec();
        nonterminal.extend_from_slice(&[0, 29]);
        assert!(walk(&nonterminal)
            .unwrap()
            .terminal_null_references
            .is_none());

        let mut nonnull = four_references;
        nonnull[7] = 2;
        assert!(walk(&nonnull).unwrap().terminal_null_references.is_none());
    }
}

#[cfg(test)]
mod transmit_header_tests {
    use super::census::walk;
    use crate::test_support::test_pmarks::header;

    #[test]
    fn header_ends_after_user_field_size_without_consuming_node_bytes() {
        let mut bytes = header();
        let end = bytes.len();
        bytes.extend([0xaa, 0xbb]);
        let census = crate::test_support::with_decode_context(|_ctx| walk(&bytes)).unwrap();
        let parsed = census.transmit_header.as_ref().unwrap();
        assert_eq!(parsed.state.maximum_node_type(), 239);
        assert_eq!(parsed.state.user_field_size(), 0);
        assert_eq!(parsed.state.schema(), "SCH_3501171_35102_13006");
        assert_eq!(parsed.end, end);
        assert_eq!(census.bytes_decoded(), end);
        for len in 0..end {
            let census =
                crate::test_support::with_decode_context(|_ctx| walk(&bytes[..len])).unwrap();
            assert!(census.transmit_header.is_none());
        }
    }
}

#[cfg(test)]
mod tests;
