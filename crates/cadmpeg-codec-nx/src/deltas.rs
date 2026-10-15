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
pub(crate) mod embedded_schema;
pub(crate) mod list;
pub(crate) mod pointer_list_block;
pub(crate) mod reference_lanes;
pub(crate) mod world;
use embedded_schema::instance::SchemaNode;
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
use transmit_state::TransmitState;
pub(crate) mod type150_state;
use packet_marker::{ReferenceMarker, Type150Marker};
use type150_state::Type150State;
pub(crate) mod group;
mod intersection;
pub(crate) mod tails;
use group::{GroupReferenceStatus, GroupSelector};
use record_kind::RecordKind;

use crate::framing::read_xmt_width as read_xmt;
use crate::vec3_at::vec3_be_at;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
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
    /// Type code of the optional terminal map target.
    pub(crate) target_kind: Option<NonZeroU16>,
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

/// One single-byte type-150 deltas state packet.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Type150StatePacket {
    /// Validated references, marker, and finite state values.
    pub(crate) state: Type150State,
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

fn transmit_header(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<Option<TransmitHeader>, CodecError> {
    let Some((description_bytes, schema_bytes, maximum_node_type, user_field_size, end)) = (|| {
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
    })() else {
        return Ok(None);
    };
    let description =
        String::from_utf8(ctx.copy_retained(description_bytes, "NX deltas description")?).ok();
    let schema = String::from_utf8(ctx.copy_retained(schema_bytes, "NX deltas schema")?).ok();
    Ok(description.zip(schema).and_then(|(description, schema)| {
        Some(TransmitHeader {
            state: TransmitState::new(description, schema, maximum_node_type, user_field_size)
                .ok()?,
            end,
        })
    }))
}

fn tagged_reference_lanes(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    covered: &[(usize, usize)],
) -> Result<Vec<TaggedReferenceLane>, CodecError> {
    let mut lanes = Vec::new();
    for (offset, end) in uncovered_spans(ctx, stream.len(), covered)? {
        let mut at = offset;
        let mut references = Vec::new();
        let mut complete = true;
        while at < end {
            ctx.charge_work(1, "scan NX tagged references")?;
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
            ctx.push_vec(&mut references, (kind, xmt), "NX tagged references")?;
        }
        if complete && at == end {
            if let Ok(references) = TaggedReferences::try_from(references) {
                ctx.push_vec(
                    &mut lanes,
                    TaggedReferenceLane {
                        references,
                        offset,
                        end,
                    },
                    "NX tagged reference lanes",
                )?;
            }
        }
    }
    Ok(lanes)
}

fn reference_type_maps(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    census: &Census,
    covered: &[(usize, usize)],
) -> Result<Vec<ReferenceTypeMap>, CodecError> {
    let mut maps = Vec::new();
    for (offset, end) in uncovered_spans(ctx, stream.len(), covered)? {
        let map = if let Some(map) =
            reference_type_map(ctx, stream, offset, ReferenceTypeMapLimit::Bounded(end))?
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
                reference_type_map(
                    ctx,
                    stream,
                    offset,
                    ReferenceTypeMapLimit::Bounded(shared_end),
                )?
                .filter(|map| {
                    map.target_kind.is_none() && map.entries.last_kind() == following_kind
                })
            } else {
                None
            }
        };
        if let Some(map) = map {
            ctx.push_vec(&mut maps, map, "NX reference type maps")?;
        }
    }
    Ok(maps)
}

#[derive(Clone, Copy)]
enum ReferenceTypeMapLimit {
    TargetTerminated,
    Bounded(usize),
}

fn reference_type_map(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
    limit: ReferenceTypeMapLimit,
) -> Result<Option<ReferenceTypeMap>, CodecError> {
    let expected_end = match limit {
        ReferenceTypeMapLimit::TargetTerminated => None,
        ReferenceTypeMapLimit::Bounded(end) => Some(end),
    };
    let Some(mut at) = (|| {
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
    })() else {
        return Ok(None);
    };
    let mut entries = Vec::new();
    loop {
        ctx.charge_work(1, "scan NX reference type map")?;
        if expected_end == Some(at) {
            return Ok(MapEntries::try_from(entries)
                .ok()
                .map(|entries| ReferenceTypeMap {
                    entries,
                    target_kind: None,
                    offset,
                    end: at,
                }));
        }
        if expected_end.is_some_and(|end| at >= end) {
            return Ok(None);
        }
        let Some((reference, consumed)) = read_xmt(stream, at) else {
            return Ok(None);
        };
        let Some(next) = at.checked_add(consumed) else {
            return Ok(None);
        };
        at = next;
        if expected_end.is_some_and(|end| at > end) {
            return Ok(None);
        }
        if reference == 1 {
            if View::u16_be_at(stream, at) != Some(0) {
                return Ok(None);
            }
            let Some(next) = at.checked_add(2) else {
                return Ok(None);
            };
            at = next;
            if expected_end == Some(at) {
                return Ok(MapEntries::try_from(entries)
                    .ok()
                    .map(|entries| ReferenceTypeMap {
                        entries,
                        target_kind: None,
                        offset,
                        end: at,
                    }));
            }
            let Some(target_kind) = View::u16_be_at(stream, at).and_then(NonZeroU16::new) else {
                return Ok(None);
            };
            let Some(next) = at.checked_add(2) else {
                return Ok(None);
            };
            at = next;
            if expected_end.is_some_and(|end| at > end) {
                return Ok(None);
            }
            return Ok(MapEntries::try_from(entries)
                .ok()
                .map(|entries| ReferenceTypeMap {
                    entries,
                    target_kind: Some(target_kind),
                    offset,
                    end: at,
                }));
        }
        let Some(kind) = View::u16_be_at(stream, at) else {
            return Ok(None);
        };
        let Some(next) = at.checked_add(2) else {
            return Ok(None);
        };
        at = next;
        if expected_end.is_some_and(|end| at > end) {
            return Ok(None);
        }
        ctx.push_vec(
            &mut entries,
            (reference, kind),
            "NX reference type map entries",
        )?;
    }
}

fn reference_marker_packets(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    covered: &[(usize, usize)],
) -> Result<Vec<ReferenceMarkerPacket>, CodecError> {
    let mut packets = Vec::new();
    for (offset, end) in uncovered_spans(ctx, stream.len(), covered)? {
        if let Some(packet) = reference_marker_packet(stream, offset, end) {
            ctx.push_vec(&mut packets, packet, "NX reference marker packets")?;
        }
    }
    Ok(packets)
}

fn inline_schema_nodes(
    ctx: &DecodeContext<'_>,
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
    for (offset, gap_end) in uncovered_spans(ctx, stream.len(), covered)? {
        let mut at = offset;
        while at < gap_end {
            let Some(node) = inline_schema_node(ctx, stream, at, gap_end, &mut seen)? else {
                break;
            };
            at = node.end;
            ctx.push_vec(&mut nodes, node, "NX schema nodes")?;
        }
    }
    Ok(nodes)
}

fn inline_schema_node(
    ctx: &DecodeContext<'_>,
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
    let Some((fields, end)) =
        embedded_schema::instance::SchemaNode::read(ctx, &stream[..gap_end], offset)?
    else {
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

fn type_150_state_packets(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    covered: &[(usize, usize)],
) -> Result<Vec<Type150StatePacket>, CodecError> {
    let mut packets = Vec::new();
    for (offset, end) in uncovered_spans(ctx, stream.len(), covered)? {
        if let Some(packet) = type_150_state_packet(stream, offset, end) {
            ctx.push_vec(&mut packets, packet, "NX type 150 state packets")?;
        }
    }
    Ok(packets)
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
    let marker = Type150Marker::try_from(*stream.get(at)?).ok()?;
    at = at.checked_add(1)?;
    let mut values = [0.0; 9];
    for value in &mut values {
        *value = View::f64_be_at(stream, at)?;
        at = at.checked_add(8)?;
    }
    (at == expected_end).then_some(Type150StatePacket {
        state: Type150State::new(references, marker, values).ok()?,
        offset,
        end: at,
    })
}

fn uncovered_spans(
    ctx: &DecodeContext<'_>,
    stream_len: usize,
    covered: &[(usize, usize)],
) -> Result<impl Iterator<Item = (usize, usize)>, CodecError> {
    ctx.charge_work(
        u64_from_index(covered.len()),
        "scan NX deltas covered spans",
    )?;
    let mut gaps = Vec::new();
    let mut at = 0;
    for &(start, end) in covered {
        if at < start {
            ctx.reserve_vec(&mut gaps, 1, "NX deltas uncovered spans")?;
            gaps.push((at, start));
        }
        at = at.max(end);
    }
    if at < stream_len {
        ctx.reserve_vec(&mut gaps, 1, "NX deltas uncovered spans")?;
        gaps.push((at, stream_len));
    }
    Ok(gaps.into_iter())
}

fn merged_event_spans(
    ctx: &DecodeContext<'_>,
    census: &Census,
) -> Result<Vec<(usize, usize)>, CodecError> {
    let event_counts = [
        usize::from(census.transmit_header.is_some()),
        usize::from(census.partition_prefix.is_some()),
        census.pmark_packets.len(),
        usize::from(census.terminal_null_references.is_some()),
        census.records.len(),
        census.tombstones.len(),
        census.body_revisions.len(),
        census.tagged_reference_lanes.len(),
        census.reference_type_maps.len(),
        census.reference_marker_packets.len(),
        census.type_150_state_packets.len(),
        census.schema_nodes.len(),
    ];
    let count = event_counts
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX deltas event spans",
                0,
                u64_from_index(census.records.len()),
            )
        })?;
    ctx.charge_collection_items(u64_from_index(count), "NX deltas event spans")?;
    let (mut covered, _covered_reservation) =
        ctx.scoped_vector_storage(count, "NX deltas event span allocation")?;
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
            .type_150_state_packets
            .iter()
            .map(|packet| (packet.offset, packet.end)),
    );
    covered.extend(
        census
            .schema_nodes
            .iter()
            .map(|declaration| (declaration.offset, declaration.end)),
    );
    ctx.sort_unstable_by(
        &mut covered,
        Ord::cmp,
        |_| 0,
        "sort NX deltas covered spans",
    )?;
    let mut merged = Vec::<(usize, usize)>::new();
    for (start, end) in covered {
        if let Some((_, merged_end)) = merged.last_mut().filter(|(_, end)| start <= *end) {
            *merged_end = (*merged_end).max(end);
        } else {
            ctx.reserve_vec(&mut merged, 1, "NX deltas merged spans")?;
            merged.push((start, end));
        }
    }
    Ok(merged)
}

fn consume_shared_record(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
    records: &[Record],
    intersection_schema_anchor_seen: bool,
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
    if let Some(record) = consume_intersection_auxiliary(ctx, stream, record_offset)? {
        return Ok(Some(record));
    }
    if let Some(record) = first_complete_record(
        ctx,
        stream,
        record_offset,
        intersection_schema_anchor_seen,
        false,
        world_layout,
    )? {
        return Ok(Some(record));
    }
    let Some(kind) = stream.get(offset).copied().map(u16::from) else {
        return Ok(None);
    };
    let fixed = if let Some(signature) = fixed_signature(kind, intersection_layout) {
        consume_fixed(ctx, stream, record_offset, kind, signature, world_layout)?
    } else {
        None
    };
    Ok(if fixed.is_some() {
        fixed
    } else {
        consume_variable(ctx, stream, record_offset, kind)?
    })
}

fn first_complete_record(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
    intersection_schema_anchor_seen: bool,
    include_type_67: bool,
    world_layout: Option<WorldLayout>,
) -> Result<Option<Record>, CodecError> {
    if let Some(record) = consume_nurbs_auxiliary(ctx, stream, offset)? {
        return Ok(Some(record));
    }
    if let Some(record) = consume_type_141(ctx, stream, offset)? {
        return Ok(Some(record));
    }
    if let Some(record) = consume_type_45(ctx, stream, offset)? {
        return Ok(Some(record));
    }
    if include_type_67 {
        if let Some(record) = consume_type_67(ctx, stream, offset)? {
            return Ok(Some(record));
        }
    }
    if let Some(record) = consume_list(ctx, stream, offset)? {
        return Ok(Some(record));
    }
    if let Some(record) = consume_pointer_list_block(ctx, stream, offset)? {
        return Ok(Some(record));
    }
    if let Some(record) = consume_world(ctx, stream, offset, world_layout)? {
        return Ok(Some(record));
    }
    consume_intersection_data(ctx, stream, offset, intersection_schema_anchor_seen)
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

fn push_merge_event(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    events: &mut BTreeMap<(u8, u32), Vec<MergeEvent>>,
    key: (u8, u32),
    event: MergeEvent,
) -> Result<(), CodecError> {
    if !events.contains_key(&key) {
        ctx.charge_collection_items(1, "NX deltas merge event keys")?;
        reservation.grow(u64_from_index(std::mem::size_of::<(
            (u8, u32),
            Vec<MergeEvent>,
        )>()))?;
    }
    let bucket = events.entry(key).or_default();
    ctx.reserve_scoped_vec(reservation, bucket, 1, "NX deltas merge events")?;
    bucket.push(event);
    Ok(())
}

/// Overlay supported complete deltas records onto one paired partition stream.
///
/// Replaced partition records are masked with non-tag bytes. Status-free
/// canonical current replacements are appended once. When BODY revision
/// envelopes are present, only records in the current interval of each body
/// sequence contribute to the current image. Raw current-revision deltas bytes
/// remain available to independent procedural decoders.
#[cfg(test)]
pub(crate) fn merge_full_records(
    ctx: &DecodeContext<'_>,
    partition: &[u8],
    deltas: &[u8],
) -> Result<Vec<u8>, CodecError> {
    let census = walk(ctx, deltas)?;
    merge_full_records_with_census(ctx, partition, deltas, &census)
}

/// Merge deltas records with a previously decoded census.
pub(crate) fn merge_full_records_with_census(
    ctx: &DecodeContext<'_>,
    partition: &[u8],
    deltas: &[u8],
    census: &Census,
) -> Result<Vec<u8>, CodecError> {
    merge_records(ctx, partition, deltas, census, None)
}

/// Merge deltas records and collect unmatched terminal tombstone counts.
pub(crate) fn merge_full_records_with_tombstone_census(
    ctx: &DecodeContext<'_>,
    partition: &[u8],
    deltas: &[u8],
    census: &Census,
) -> Result<MergeFullRecordsResult, CodecError> {
    let mut unmatched_tombstones = BTreeMap::new();
    let merged = merge_records(
        ctx,
        partition,
        deltas,
        census,
        Some(&mut unmatched_tombstones),
    )?;
    Ok(MergeFullRecordsResult {
        merged,
        unmatched_tombstones,
    })
}

fn merge_records(
    ctx: &DecodeContext<'_>,
    partition: &[u8],
    deltas: &[u8],
    census: &Census,
    unmatched_tombstones: Option<&mut BTreeMap<&'static str, usize>>,
) -> Result<Vec<u8>, CodecError> {
    let current_scopes = current_revision_scopes(ctx, census, deltas.len())?;
    let mut replacements = BTreeMap::<(u8, u32), &Record>::new();
    let mut replacement_reservation = ctx.reserve_scoped(0, "NX deltas replacement keys")?;
    let mut unmatched_events = unmatched_tombstones.map(|totals| (totals, BTreeMap::new()));
    let mut event_reservation = ctx.reserve_scoped(0, "NX deltas merge events")?;
    for record in census
        .records
        .iter()
        .filter(|record| current_scope_contains(&current_scopes, record.offset))
    {
        let Ok(kind) = u8::try_from(record.kind()) else {
            continue;
        };
        if mergeable_record(ctx, record, kind)? {
            if !replacements.contains_key(&(kind, record.xmt)) {
                ctx.charge_collection_items(1, "NX deltas replacement keys")?;
                replacement_reservation
                    .grow(u64_from_index(std::mem::size_of::<((u8, u32), &Record)>()))?;
            }
            replacements.insert((kind, record.xmt), record);
            if let Some((_, events)) = &mut unmatched_events {
                push_merge_event(
                    ctx,
                    &mut event_reservation,
                    events,
                    (kind, record.xmt),
                    MergeEvent::Full {
                        offset: record.offset,
                    },
                )?;
            }
        }
    }

    let mut tombstones = BTreeMap::new();
    let mut tombstone_reservation = ctx.reserve_scoped(0, "NX deltas tombstone keys")?;
    for tombstone in census
        .tombstones
        .iter()
        .filter(|tombstone| current_scope_contains(&current_scopes, tombstone.offset))
    {
        let kind = tombstone.kind.code();
        if !tombstones.contains_key(&(kind, tombstone.xmt)) {
            ctx.charge_collection_items(1, "NX deltas tombstone keys")?;
            tombstone_reservation
                .grow(u64_from_index(
                    std::mem::size_of::<((u8, u32), &Tombstone)>(),
                ))?;
        }
        tombstones.insert((kind, tombstone.xmt), tombstone);
        if let Some((_, events)) = &mut unmatched_events {
            push_merge_event(
                ctx,
                &mut event_reservation,
                events,
                (kind, tombstone.xmt),
                MergeEvent::Tombstone {
                    offset: tombstone.offset,
                    kind: tombstone.kind,
                },
            )?;
        }
    }

    let graph = crate::topology::Graph::parse(ctx, partition)?;
    if let Some((totals, events)) = unmatched_events {
        *totals = count_unmatched_events(ctx, events, &graph)?;
    }
    let topology_carriers = graph.referenced_carrier_xmts(ctx)?;
    replacements.retain(|key, record| {
        tombstones
            .get(key)
            .is_none_or(|tombstone| record.offset > tombstone.offset)
    });
    let mut deletions = BTreeMap::new();
    let mut deletion_reservation = ctx.reserve_scoped(0, "NX deltas deletion keys")?;
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
            deletion_reservation
                .grow(u64_from_index(
                    std::mem::size_of::<((u8, u32), &Tombstone)>(),
                ))?;
            ctx.insert_btree_map(&mut deletions, key, tombstone, "NX deltas deletion keys")?;
        }
    }
    let build = |include_topology: bool| -> Result<_, CodecError> {
        let included = |kind: u8| include_topology || !matches!(kind, 12..=19);
        let total_len = replacements
            .iter()
            .filter(|((kind, _), _)| included(*kind))
            .try_fold(partition.len(), |sum, (_, record)| {
                sum.checked_add(record.canonical_bytes.len())
            })
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX merged partition bytes",
                    0,
                    u64_from_index(partition.len()),
                )
            })?;
        let (mut merged, reservation) =
            ctx.scoped_vector_storage(total_len, "NX merged partition bytes")?;
        merged.extend_from_slice(partition);
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
        Ok((merged, reservation))
    };
    if graph.body_shape_shells(ctx)?.next().is_some() {
        let (merged, reservation) = build(false)?;
        reservation.commit()?;
        return Ok(merged);
    }
    let (merged, merged_reservation) = build(true)?;
    let merged_graph = crate::topology::Graph::parse(ctx, &merged)?;
    let base_complete = graph.has_complete_body_topology(ctx)?;
    let merged_complete = merged_graph.has_complete_body_topology(ctx)?;
    let deletes_owner = deletions.keys().any(|(kind, _)| matches!(kind, 12 | 13));
    let deleted_faces = deletions.keys().filter(|(kind, _)| *kind == 14).count();
    let accounted_faces = merged_graph
        .body_shape_face_count(ctx)?
        .checked_add(deleted_faces)
        .ok_or_else(|| CodecError::Malformed("NX accounted face count overflow".into()))?;
    let unaccounted_face_loss =
        !deletes_owner && accounted_faces < graph.body_shape_face_count(ctx)?;
    if base_complete && (!merged_complete || unaccounted_face_loss) {
        let (selected, reservation) = build(false)?;
        reservation.commit()?;
        Ok(selected)
    } else {
        merged_reservation.commit()?;
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
    ctx: &DecodeContext<'_>,
    partition: &[u8],
    deltas: &[u8],
) -> Result<usize, CodecError> {
    Ok(
        unmatched_terminal_tombstones_by_family(ctx, partition, deltas)?
            .values()
            .sum(),
    )
}

/// Count unmatched terminal tombstones by Parasolid record family.
fn unmatched_terminal_tombstones_by_family(
    ctx: &DecodeContext<'_>,
    partition: &[u8],
    deltas: &[u8],
) -> Result<BTreeMap<&'static str, usize>, CodecError> {
    let census = walk(ctx, deltas)?;
    let graph = crate::topology::Graph::parse(ctx, partition)?;
    let (events, _reservation) = collect_unmatched_events(ctx, &census, deltas.len())?;
    count_unmatched_events(ctx, events, &graph)
}

type ChargedMergeEvents<'ctx> = (
    BTreeMap<(u8, u32), Vec<MergeEvent>>,
    cadmpeg_core::decode::ScopedReservation<'ctx>,
);

fn collect_unmatched_events<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    census: &Census,
    stream_len: usize,
) -> Result<ChargedMergeEvents<'ctx>, CodecError> {
    let current_scopes = current_revision_scopes(ctx, census, stream_len)?;
    let mut events = BTreeMap::<(u8, u32), Vec<MergeEvent>>::new();
    let mut reservation = ctx.reserve_scoped(0, "NX unmatched deltas events")?;
    for record in census
        .records
        .iter()
        .filter(|record| current_scope_contains(&current_scopes, record.offset))
    {
        let Ok(kind) = u8::try_from(record.kind()) else {
            continue;
        };
        if !mergeable_record(ctx, record, kind)? {
            continue;
        }
        push_merge_event(
            ctx,
            &mut reservation,
            &mut events,
            (kind, record.xmt),
            MergeEvent::Full {
                offset: record.offset,
            },
        )?;
    }
    for tombstone in census
        .tombstones
        .iter()
        .filter(|tombstone| current_scope_contains(&current_scopes, tombstone.offset))
    {
        let kind = tombstone.kind.code();
        push_merge_event(
            ctx,
            &mut reservation,
            &mut events,
            (kind, tombstone.xmt),
            MergeEvent::Tombstone {
                offset: tombstone.offset,
                kind: tombstone.kind,
            },
        )?;
    }
    Ok((events, reservation))
}

fn count_unmatched_events(
    ctx: &DecodeContext<'_>,
    events: BTreeMap<(u8, u32), Vec<MergeEvent>>,
    graph: &crate::topology::Graph,
) -> Result<BTreeMap<&'static str, usize>, CodecError> {
    let mut unmatched = BTreeMap::new();
    for ((kind, xmt), mut events) in events {
        ctx.stable_sort_by(
            &mut events,
            |first, second| {
                let offset = |event: &MergeEvent| match event {
                    MergeEvent::Full { offset } | MergeEvent::Tombstone { offset, .. } => *offset,
                };
                offset(first).cmp(&offset(second))
            },
            |_| 0,
            "sort NX unmatched deltas events",
        )?;
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
            let name = tombstone_kind.name();
            ctx.admit_btree_entry(&unmatched, &name, "NX unmatched tombstone families")?;
            *unmatched.entry(name).or_default() += 1;
        }
    }
    Ok(unmatched)
}

fn mergeable_record(
    ctx: &DecodeContext<'_>,
    record: &Record,
    kind: u8,
) -> Result<bool, CodecError> {
    let Ok(kind) = NodeKind::try_from(kind) else {
        return Ok(false);
    };
    crate::topology::Graph::has_canonical_record(ctx, &record.canonical_bytes, kind, record.xmt)
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

fn current_revision_scopes(
    ctx: &DecodeContext<'_>,
    census: &Census,
    stream_len: usize,
) -> Result<Vec<RevisionScope>, CodecError> {
    // Only xmt 3 BODY envelopes delimit snapshots. Other validated type-12
    // envelopes remain available to the byte ledger without changing scope.
    ctx.charge_work(
        u64_from_index(census.body_revisions.len()),
        "scan NX BODY revisions",
    )?;
    let count = census
        .body_revisions
        .iter()
        .filter(|revision| u32::from(revision.xmt) == 3)
        .count();
    let (mut snapshot_revisions, _snapshot_reservation) =
        ctx.temporary_vec(count, "NX snapshot revision indices")?;
    for (index, revision) in census.body_revisions.iter().enumerate() {
        if u32::from(revision.xmt) == 3 {
            snapshot_revisions.push(index);
        }
    }
    if snapshot_revisions.is_empty() {
        ctx.charge_collection_items(1, "NX current revision scopes")?;
        ctx.charge_retained(
            u64_from_index(std::mem::size_of::<RevisionScope>()),
            "NX current revision scopes",
        )?;
        return Ok(vec![RevisionScope {
            start: 0,
            end: stream_len,
        }]);
    }

    let direction = revision_direction(census, &snapshot_revisions);
    ctx.charge_collection_items(1, "NX revision run starts")?;
    let mut run_reservation = ctx.reserve_scoped(
        u64_from_index(std::mem::size_of::<usize>()),
        "NX revision run starts",
    )?;
    let mut run_starts = vec![0];
    for (position, pair) in snapshot_revisions.windows(2).enumerate() {
        let previous = census.body_revisions[pair[0]].node_id;
        let current = census.body_revisions[pair[1]].node_id;
        if !revision_follows_direction(previous, current, direction) {
            ctx.reserve_scoped_vec(
                &mut run_reservation,
                &mut run_starts,
                1,
                "NX revision run starts",
            )?;
            run_starts.push(position + 1);
        }
    }

    let mut scopes = Vec::new();
    let mut scopes_reservation = ctx.reserve_scoped(0, "NX current revision scopes")?;
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
            ctx.reserve_scoped_vec(
                &mut scopes_reservation,
                &mut scopes,
                1,
                "NX current revision scopes",
            )?;
            scopes.push(RevisionScope {
                start: current_revision.offset,
                end,
            });
        }
    }
    scopes_reservation.commit()?;
    Ok(scopes)
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

fn current_scope_contains(scopes: &[RevisionScope], offset: usize) -> bool {
    scopes
        .iter()
        .any(|scope| scope.start <= offset && offset < scope.end)
}

/// Return raw deltas bytes with decoded records and compact tombstones masked.
/// Historical BODY revision intervals are also masked. Current-revision records
/// needed by semantic scanners are appended in their partition form.
#[cfg(test)]
pub(crate) fn semantic_residual(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<Vec<u8>, CodecError> {
    let census = walk(ctx, stream)?;
    semantic_residual_with_census(ctx, stream, &census)
}

/// Return the semantic residual using a census already produced for the stream.
///
/// The census owns the complete-record boundaries and canonical forms used by
/// both topology merging and semantic scanning. Reusing it avoids a second
/// full walk of a large delta stream while keeping this transformation
/// byte-for-byte identical to `semantic_residual`.
pub(crate) fn semantic_residual_with_census(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    census: &Census,
) -> Result<Vec<u8>, CodecError> {
    let current_scopes = current_revision_scopes(ctx, census, stream.len())?;
    // Mask the whole stream and restore each current-revision scope. Restoring
    // the kept spans, rather than filling the gaps between them, needs no
    // ordering or disjointness relation between the scopes: every byte outside
    // every scope reads 0xff whatever order the scopes arrive in.
    let scan_work = u64_from_index(census.records.len())
        .checked_mul(u64_from_index(current_scopes.len()))
        .and_then(|value| value.checked_add(u64_from_index(census.tombstones.len())))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX semantic residual scan",
                0,
                u64_from_index(census.records.len()),
            )
        })?;
    ctx.charge_work(scan_work, "NX semantic residual scan")?;
    let is_semantic = |record: &Record| {
        let is_current = current_scope_contains(&current_scopes, record.offset);
        let is_semantic = matches!(
            record.kind(),
            40 | 41 | 45 | 59 | 81..=84 | 91 | 125..=128 | 135..=136 | 141 | 204
        ) || record.kind() == 90 && record.canonical_bytes.first() == Some(&0x5a);
        is_current && is_semantic
    };
    let mut total_len = stream.len();
    for record in census.records.iter().filter(|record| is_semantic(record)) {
        let prefix_len = if record.kind() == 90 && record.canonical_bytes.first() == Some(&0x5a) {
            crate::topology::TYPE_38_SCHEMA_HEADER.len() - 1
        } else {
            0
        };
        total_len = total_len
            .checked_add(prefix_len)
            .and_then(|length| length.checked_add(record.canonical_bytes.len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX semantic residual bytes",
                    0,
                    u64_from_index(stream.len()),
                )
            })?;
    }
    let mut residual = Vec::new();
    ctx.reserve_capacity(&mut residual, total_len, "NX semantic residual bytes")?;
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
    for record in census.records.iter().filter(|record| is_semantic(record)) {
        if record.kind() == 90 && record.canonical_bytes.first() == Some(&0x5a) {
            let prefix_len = crate::topology::TYPE_38_SCHEMA_HEADER.len() - 1;
            residual.extend_from_slice(&crate::topology::TYPE_38_SCHEMA_HEADER[..prefix_len]);
        }
        residual.extend_from_slice(&record.canonical_bytes);
    }
    Ok(residual)
}

fn consume_fixed(
    ctx: &DecodeContext<'_>,
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
    candidate.materialize(ctx, stream, signature).map(Some)
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
    canonical_len: usize,
}

impl FixedCandidate {
    fn materialize(
        self,
        ctx: &DecodeContext<'_>,
        stream: &[u8],
        signature: &[Token],
    ) -> Result<Record, CodecError> {
        let mut canonical_bytes = Vec::new();
        ctx.reserve_capacity(
            &mut canonical_bytes,
            self.canonical_len,
            "NX deltas fixed record bytes",
        )?;
        canonical_bytes.extend_from_slice(&stream[self.offset..self.prefix_end]);
        let mut at = self.prefix_end;
        for token in signature {
            let (len, skip) = match token {
                Token::Ref => {
                    let (_, consumed) = read_xmt(stream, at).ok_or_else(|| {
                        ctx.refuse_codec_limit("NX deltas fixed record bytes", 0, 0)
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
    let mut canonical_len = at.checked_sub(offset)?;
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
                canonical_len = canonical_len.checked_add(consumed)?;
                references.push(reference);
            }
            Token::Tolerance => {
                let tolerance = View::f64_be_at(stream, at)?;
                (tolerance.is_finite()
                    && (!matches!(kind, 16 | 18) || tolerance.abs() >= 1.0e-100))
                    .then_some(())?;
                stream.get(at..at + 8)?;
                canonical_len = canonical_len.checked_add(8)?;
                at += 8;
            }
            Token::Sense => {
                matches!(stream.get(at), Some(b'+' | b'-')).then_some(())?;
                stream.get(at)?;
                canonical_len = canonical_len.checked_add(1)?;
                at += 1;
            }
            Token::OffsetDiscriminator => {
                matches!(stream.get(at), Some(b'V' | b'I' | b'U')).then_some(())?;
                stream.get(at)?;
                canonical_len = canonical_len.checked_add(1)?;
                at += 1;
            }
            Token::BlendSubtype => {
                (stream.get(at) == Some(&b'R')).then_some(())?;
                canonical_len = canonical_len.checked_add(1)?;
                at += 1;
            }
            Token::Boolean => {
                matches!(stream.get(at), Some(0 | 1)).then_some(())?;
                stream.get(at)?;
                canonical_len = canonical_len.checked_add(1)?;
                at += 1;
            }
            Token::Position => {
                let xyz = vec3_be_at(stream, at)?;
                position = Some(FixedPosition::new(kind, xyz)?);
                stream.get(at..at + 24)?;
                canonical_len = canonical_len.checked_add(24)?;
                at += 24;
            }
            Token::Vector => {
                let xyz = vec3_be_at(stream, at)?;
                xyz.iter().all(|value| value.is_finite()).then_some(())?;
                stream.get(at..at + 24)?;
                canonical_len = canonical_len.checked_add(24)?;
                at += 24;
            }
            Token::Scalar => {
                View::f64_be_at(stream, at)?.is_finite().then_some(())?;
                stream.get(at..at + 8)?;
                canonical_len = canonical_len.checked_add(8)?;
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
        canonical_len,
    })
}

fn consume_variable(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
    kind: u16,
) -> Result<Option<Record>, CodecError> {
    if kind == 90 {
        return consume_group(ctx, stream, offset);
    }
    if kind == 91 {
        return consume_type_91(ctx, stream, offset);
    }
    let parsed: Option<Result<_, CodecError>> = (|| {
        Some(Ok(match kind {
            81 => {
                let record = propagate_resource!(crate::parasolid::entity_51_record_at(
                    ctx, stream, offset
                ))?;
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
                let (parsed_kind, xmt, byte_len) = propagate_resource!(
                    crate::parasolid::value_records::entity_value_record_identity_at(
                        ctx, stream, offset,
                    )
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
        }))
    })();
    let Some((xmt, byte_len, family)) = parsed.transpose()? else {
        return Ok(None);
    };
    let Some(end) = offset.checked_add(byte_len) else {
        return Ok(None);
    };
    let Some(bytes) = stream.get(offset..end) else {
        return Ok(None);
    };
    Ok(Some(Record {
        family,
        xmt,
        canonical_bytes: ctx.copy_retained(bytes, "NX deltas variable record bytes")?,
        offset,
        end,
    }))
}

fn is_value_family(kind: u16) -> bool {
    matches!(kind, 82..=89 | 98)
}

fn admitted_record(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
    end: usize,
    family: RecordFamily,
    xmt: u32,
) -> Result<Option<Record>, CodecError> {
    let Some(bytes) = stream.get(offset..end) else {
        return Ok(None);
    };
    Ok(Some(Record {
        family,
        xmt,
        canonical_bytes: ctx.copy_retained(bytes, "NX deltas canonical record bytes")?,
        offset,
        end,
    }))
}

fn consume_group(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
) -> Result<Option<Record>, CodecError> {
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
    let Some((family, xmt, end)) = parsed else {
        return Ok(None);
    };
    admitted_record(ctx, stream, offset, end, family, xmt)
}

fn consume_pointer_list_block(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
) -> Result<Option<Record>, CodecError> {
    let parsed = (|| {
        (View::u16_be_at(stream, offset) == Some(74)).then_some(())?;
        let direct = pointer_list_block_shape(stream, offset.checked_add(2)?);
        let escaped_marker = stream.get(offset + 2) == Some(&0xff);
        let escaped = escaped_marker
            .then(|| pointer_list_block_shape(stream, offset.checked_add(3)?))
            .flatten();
        select_enveloped_layout(escaped_marker, direct, escaped)
    })();
    let Some(shape) = parsed else {
        return Ok(None);
    };
    let Some(state) = materialize_pointer_list_block(ctx, stream, shape)? else {
        return Ok(None);
    };
    admitted_record(
        ctx,
        stream,
        offset,
        shape.end,
        RecordFamily::PointerListBlock { fields: state },
        shape.xmt,
    )
}

fn consume_list(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
) -> Result<Option<Record>, CodecError> {
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
    let Some((family, xmt, end)) = parsed else {
        return Ok(None);
    };
    admitted_record(ctx, stream, offset, end, family, xmt)
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

fn consume_world(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
    layout: Option<WorldLayout>,
) -> Result<Option<Record>, CodecError> {
    let Some((xmt, references, end)) =
        layout.and_then(|layout| world_shape(stream, offset, layout))
    else {
        return Ok(None);
    };
    admitted_record(
        ctx,
        stream,
        offset,
        end,
        RecordFamily::World { references },
        xmt,
    )
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
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    shape: PointerListBlockShape,
) -> Result<Option<PointerListBlockFields>, CodecError> {
    let count = usize::try_from(shape.slot_count).map_err(|_| {
        ctx.refuse_codec_limit(
            "NX POINTER_LIS_BLOCK entries",
            0,
            u64::from(shape.slot_count),
        )
    })?;
    let mut references = ctx.collection_vec(count, "NX POINTER_LIS_BLOCK entries")?;
    let mut at = shape.references_start;
    for _ in 0..count {
        let Some((reference, consumed)) = read_xmt(stream, at) else {
            return Ok(None);
        };
        let Some(next) = at
            .checked_add(consumed)
            .and_then(|next| next.checked_add(1))
        else {
            return Ok(None);
        };
        at = next;
        references.push(reference);
    }
    Ok(PointerListBlockFields::new(shape.active_count, shape.next_block, references).ok())
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
    let selector = GroupSelector::try_from(*stream.get(at)?).ok()?;
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
    let selector = GroupSelector::try_from(*stream.get(at)?).ok()?;
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

fn consume_type_91(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
) -> Result<Option<Record>, CodecError> {
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
    let Some((family, xmt, end)) = parsed else {
        return Ok(None);
    };
    admitted_record(ctx, stream, offset, end, family, xmt)
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

fn consume_type_141(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
) -> Result<Option<Record>, CodecError> {
    let parsed = (|| {
        (View::u16_be_at(stream, offset) == Some(141)).then_some(())?;
        let direct = type_141_layout(stream, offset, 0);
        let escaped_marker = stream.get(offset + 2) == Some(&0xff);
        let escaped = escaped_marker
            .then(|| type_141_layout(stream, offset, 1))
            .flatten();
        let (xmt, references, end) = if escaped_marker {
            escaped.or(direct)?
        } else {
            direct?
        };
        Some((
            RecordFamily::Type141 {
                references: references.try_into().ok()?,
            },
            xmt,
            end,
        ))
    })();
    let Some((family, xmt, end)) = parsed else {
        return Ok(None);
    };
    admitted_record(ctx, stream, offset, end, family, xmt)
}

fn consume_type_45(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
) -> Result<Option<Record>, CodecError> {
    let parsed: Option<Result<_, CodecError>> = (|| {
        (View::u16_be_at(stream, offset) == Some(45)).then_some(())?;
        let direct = propagate_resource!(type_45_layout(ctx, stream, offset, 0));
        let escaped_marker = stream.get(offset + 2) == Some(&0xff);
        let escaped = if escaped_marker {
            propagate_resource!(type_45_layout(ctx, stream, offset, 1))
        } else {
            None
        };
        select_enveloped_layout(escaped_marker, direct, escaped).map(Ok)
    })();
    let Some((xmt, end)) = parsed.transpose()? else {
        return Ok(None);
    };
    admitted_record(ctx, stream, offset, end, RecordFamily::Type45, xmt)
}

fn consume_type_67(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
) -> Result<Option<Record>, CodecError> {
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
    let Some((family, xmt, end)) = parsed else {
        return Ok(None);
    };
    admitted_record(ctx, stream, offset, end, family, xmt)
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

fn type_45_layout(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
    envelope_len: usize,
) -> Result<Option<(u32, usize)>, CodecError> {
    let parsed: Option<Result<_, CodecError>> = (|| {
        let count_at = offset.checked_add(2 + envelope_len)?;
        let count = usize::try_from(View::u32_be_at(stream, count_at)?).ok()?;
        (count > 0).then_some(())?;
        let (xmt, xmt_len) = read_xmt(stream, count_at.checked_add(4)?)?;
        (xmt > 1).then_some(())?;
        let data_at = count_at.checked_add(4 + xmt_len)?;
        let end = data_at.checked_add(count.checked_mul(8)?)?;
        let raw = stream.get(data_at..end)?;
        propagate_resource!(ctx.charge_work(u64_from_index(count), "validate NX type-45 lane"));
        (0..count)
            .all(|i| {
                View::f64_be_at(raw, i * 8)
                    .is_some_and(|value| value.is_finite() && (value == 0.0 || value.is_normal()))
            })
            .then_some(())?;
        Some(Ok((xmt, end)))
    })();
    parsed.transpose()
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

fn consume_intersection_data(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
    intersection_schema_anchor_seen: bool,
) -> Result<Option<Record>, CodecError> {
    let Some((curve, end)) = crate::topology::intersection_data_curve_at(
        stream,
        offset,
        intersection_schema_anchor_seen,
    ) else {
        return Ok(None);
    };
    let mut references = curve
        .header_references
        .map(crate::framing::xmt_reference::XmtTarget::to_wire)
        .to_vec();
    references.extend(
        curve
            .references
            .map(crate::framing::xmt_reference::XmtTarget::to_wire),
    );
    let Ok(references) = references.try_into() else {
        return Ok(None);
    };
    admitted_record(
        ctx,
        stream,
        offset,
        end,
        RecordFamily::IntersectionData { references },
        curve.xmt,
    )
}

fn consume_intersection_auxiliary(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
) -> Result<Option<Record>, CodecError> {
    let (family, xmt, end) = if let Some((chart, end)) =
        crate::intersection::chart_source_record_at(
            ctx,
            stream,
            offset,
            crate::intersection::ChartPointLayout::Ext11,
        )? {
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
        let Some((support_uv, end)) =
            crate::intersection::support_uv_record_at(ctx, stream, offset)?
        else {
            return Ok(None);
        };
        (RecordFamily::SupportUv, u32::from(support_uv.xmt), end)
    };
    let Some(bytes) = stream.get(offset..end) else {
        return Ok(None);
    };
    let canonical_bytes = ctx.copy_retained(bytes, "NX deltas intersection auxiliary bytes")?;
    Ok(Some(Record {
        family,
        xmt,
        canonical_bytes,
        offset,
        end,
    }))
}

fn consume_nurbs_auxiliary(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    offset: usize,
) -> Result<Option<Record>, CodecError> {
    let Some(auxiliary) = crate::nurbs::auxiliary_record_at(ctx, stream, offset)? else {
        return Ok(None);
    };
    admitted_record(
        ctx,
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
            let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

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

        let parsed =
            crate::test_support::with_decode_context(|ctx| consume_type_67(ctx, &bytes, 0))
                .expect("resource limit admits test record")
                .expect("complete current-record grammar");

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
            assert!(
                crate::test_support::with_decode_context(|ctx| consume_type_67(
                    ctx,
                    &bytes[..end],
                    0
                ))
                .expect("resource limit admits test prefix")
                .is_none()
            );
        }

        let mut invalid_status = bytes.clone();
        invalid_status[11] = 0;
        assert!(
            crate::test_support::with_decode_context(|ctx| consume_type_67(
                ctx,
                &invalid_status,
                0
            ))
            .expect("resource limit admits invalid test record")
            .is_none()
        );

        let mut subnormal_value = bytes;
        let value_at = subnormal_value.len() - 32;
        subnormal_value[value_at..value_at + 8].copy_from_slice(&1u64.to_be_bytes());
        assert!(
            crate::test_support::with_decode_context(|ctx| consume_type_67(
                ctx,
                &subnormal_value,
                0
            ))
            .expect("resource limit admits invalid test record")
            .is_none()
        );
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

        let parsed =
            crate::test_support::with_decode_context(|ctx| consume_type_45(ctx, &bytes, 0))
                .expect("resource limit admits test record")
                .expect("complete declared value lane");

        assert_eq!(parsed.end, record_end);
        assert_eq!(parsed.canonical_bytes, bytes[..record_end]);
    }
}

#[cfg(test)]
mod type_150_state_packet_tests {
    use super::census::walk;

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
        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

        assert_eq!(census.type_150_state_packets.len(), 1);
        let packet = &census.type_150_state_packets[0];
        assert_eq!(packet.state.references(), [1, 3, 6_192, 6_193, 6_194]);
        assert_eq!(u8::from(packet.state.marker), 0x2b);
        assert_eq!(
            packet.state.values(),
            [-0.025, -0.05, 0.25, 0.0, 1.0, 0.0, 0.0, -0.0, 1.0]
        );
        assert_eq!((packet.offset, packet.end), (0, bytes.len()));
        assert_eq!(census.bytes_decoded(), bytes.len());

        let mut malformed = bytes.clone();
        malformed[6] = 0;
        assert!(
            crate::test_support::with_decode_context(|ctx| walk(ctx, &malformed))
                .unwrap()
                .type_150_state_packets
                .is_empty()
        );

        let mut nonfinite = bytes;
        let value_offset = nonfinite.len() - 9 * 8;
        nonfinite[value_offset..value_offset + 8].copy_from_slice(&f64::NAN.to_be_bytes());
        assert!(
            crate::test_support::with_decode_context(|ctx| walk(ctx, &nonfinite))
                .unwrap()
                .type_150_state_packets
                .is_empty()
        );
    }
}

#[cfg(test)]
mod partition_prefix_tests {
    use super::census::walk;
    use crate::test_support::test_pmarks;

    #[test]
    fn leading_nodes_are_separate_from_the_counted_header() {
        let bytes = test_pmarks::prefix(1);
        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();
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
        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();
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
                crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes[..end])).unwrap();
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
        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

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
                let census =
                    crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

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
            assert!(
                crate::test_support::with_decode_context(|ctx| walk(ctx, malformed))
                    .unwrap()
                    .records
                    .is_empty()
            );
        }
    }

    #[test]
    fn retains_escaped_curve_descriptor_with_extended_state_references() {
        let bytes = escaped_curve_descriptor_with_extended_references();
        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

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
        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(40_000, 81), (3, 100)].try_into().unwrap(),
                target_kind: NonZeroU16::new(55),
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
        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(40_000, 81), (3, 100)].try_into().unwrap(),
                target_kind: NonZeroU16::new(55),
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn reference_type_map_accepts_null_target_kind() {
        let mut bytes = map_bytes();
        bytes[18..].copy_from_slice(&[0, 1]);

        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

        assert_eq!(census.reference_type_maps.len(), 1);
        assert_eq!(
            census.reference_type_maps[0]
                .target_kind
                .map(NonZeroU16::get),
            Some(1)
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn reference_type_map_accepts_schema_defined_target_kind() {
        let mut bytes = map_bytes();
        bytes[18..].copy_from_slice(&323u16.to_be_bytes());

        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

        assert_eq!(census.reference_type_maps.len(), 1);
        assert_eq!(
            census.reference_type_maps[0]
                .target_kind
                .map(NonZeroU16::get),
            Some(323)
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn reference_type_map_precedes_counted_record_at_shared_kind() {
        let map = vec![0, 1, 0, 1, 0, 3, 0, 82, 0, 1, 0, 0, 2, 100];
        let mut bytes = map.clone();
        bytes.resize(bytes.len() + 65_536 * 4, 0);

        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 82)].try_into().unwrap(),
                target_kind: NonZeroU16::new(612),
                offset: 0,
                end: map.len(),
            }]
        );
        assert!(census.records.is_empty());
        assert_eq!(census.bytes_decoded(), map.len());
    }

    #[test]
    fn reference_type_map_accepts_entry_table_without_target_clause() {
        let bytes = vec![0, 1, 0, 1, 0, 3, 0, 81, 0, 4, 0, 100];

        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 81), (4, 100)].try_into().unwrap(),
                target_kind: None,
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn reference_type_map_shares_its_final_kind_with_a_tombstone() {
        let bytes = vec![0, 1, 0, 1, 0, 3, 0, 81, 0, 9, 0, 1];

        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 81)].try_into().unwrap(),
                target_kind: None,
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
        assert!(
            crate::test_support::with_decode_context(|ctx| walk(ctx, &incomplete_tombstone))
                .unwrap()
                .reference_type_maps
                .is_empty()
        );
    }

    #[test]
    fn reference_type_map_accepts_targetless_terminal_clause() {
        let bytes = vec![1, 0, 1, 0, 3, 0, 81, 0, 4, 0, 100, 0, 1, 0, 0];

        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 81), (4, 100)].try_into().unwrap(),
                target_kind: None,
                offset: 0,
                end: bytes.len(),
            }]
        );
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn reference_type_map_accepts_map_only_type_codes() {
        let bytes = vec![1, 0, 1, 0, 3, 0, 67, 0, 4, 0, 11, 0, 1, 0, 0, 0, 61];

        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

        assert_eq!(
            census.reference_type_maps,
            [ReferenceTypeMap {
                entries: vec![(3, 67), (4, 11)].try_into().unwrap(),
                target_kind: NonZeroU16::new(61),
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

        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

        assert_eq!(census.pmark_packets.len(), 1);
        assert_eq!(census.reference_type_maps.len(), 1);
        assert_eq!(census.reference_type_maps[0].offset, map_offset);
        assert_eq!(census.bytes_decoded(), bytes.len());
    }

    #[test]
    fn target_clause_self_delimits_reference_type_map_prefixes() {
        let first = map_bytes();
        let mut bytes = first.clone();
        bytes.extend(map_bytes());

        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();

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
        let mut zero_target_type = bytes.clone();
        zero_target_type[18..].copy_from_slice(&0u16.to_be_bytes());

        for malformed in [
            &bytes[..bytes.len() - 1],
            unknown_entry_type.as_slice(),
            zero_target_type.as_slice(),
        ] {
            assert!(
                crate::test_support::with_decode_context(|ctx| walk(ctx, malformed))
                    .unwrap()
                    .reference_type_maps
                    .is_empty()
            );
        }
    }
}

#[cfg(test)]
mod terminal_null_reference_tests {
    use super::census::walk;

    #[test]
    fn retains_two_or_four_null_references_at_the_stream_boundary() {
        let four_references = [0, 1, 0, 1, 0, 1, 0, 1];
        let census =
            crate::test_support::with_decode_context(|ctx| walk(ctx, &four_references)).unwrap();

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
            crate::test_support::with_decode_context(|ctx| walk(ctx, &two_references))
                .unwrap()
                .terminal_null_references
                .map(|tail| (tail.offset(), tail.end(), tail.form().references().len())),
            Some((0, two_references.len(), 2))
        );

        let mut nonterminal = four_references.to_vec();
        nonterminal.extend_from_slice(&[0, 29]);
        assert!(
            crate::test_support::with_decode_context(|ctx| walk(ctx, &nonterminal))
                .unwrap()
                .terminal_null_references
                .is_none()
        );

        let mut nonnull = four_references;
        nonnull[7] = 2;
        assert!(
            crate::test_support::with_decode_context(|ctx| walk(ctx, &nonnull))
                .unwrap()
                .terminal_null_references
                .is_none()
        );
    }
}

#[cfg(test)]
mod transmit_header_tests {
    use super::census::walk;
    use crate::test_support::test_pmarks::header;

    #[test]
    fn deltas_census_route_refuses_retained_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        let bytes = header();
        crate::test_support::with_decode_context_over(
            &bytes,
            |policy| policy.limits.max_retained_bytes = 0,
            |ctx| {
                let error = walk(ctx, &bytes).expect_err("retained refusal");
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::RetainedBytes)
                );
            },
        );
    }

    #[test]
    fn header_ends_after_user_field_size_without_consuming_node_bytes() {
        let mut bytes = header();
        let end = bytes.len();
        bytes.extend([0xaa, 0xbb]);
        let census = crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();
        let parsed = census.transmit_header.as_ref().unwrap();
        assert_eq!(parsed.state.maximum_node_type(), 239);
        assert_eq!(parsed.state.user_field_size(), 0);
        assert_eq!(parsed.state.schema(), "SCH_3501171_35102_13006");
        assert_eq!(parsed.end, end);
        assert_eq!(census.bytes_decoded(), end);
        for len in 0..end {
            let census =
                crate::test_support::with_decode_context(|ctx| walk(ctx, &bytes[..len])).unwrap();
            assert!(census.transmit_header.is_none());
        }
    }
}

#[cfg(test)]
mod tests;
