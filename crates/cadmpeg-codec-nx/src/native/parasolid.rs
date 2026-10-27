// SPDX-License-Identifier: Apache-2.0
//! Parasolid source-record extractors and their record types.

use cadmpeg_ir::native::bytes::NativeBytes;

use crate::framing::xmt_reference::XmtTarget;
use crate::parasolid::name_references::NameReferences;
use crate::parasolid::{Stream, StreamKind};
use crate::topology::blend_surface_state::BlendSurfaceState;
use crate::topology::offset_surface_state::OffsetSurfaceState;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::deltas::record_family::RecordFamily;
use crate::parasolid::attribute_action::AttributeAction;
use crate::parasolid::attribute_field::AttributeField;
use crate::records::deltas::EventContent;
use crate::records::DeltasRecords;
use std::num::NonZeroU32;

pub(super) mod structured_value_kind;
use structured_value_kind::StructuredValueKind;

mod field_use_wire;
use field_use_wire::FieldUseWire;

pub(super) mod topology_attribute_kind;
use topology_attribute_kind::TopologyAttributeKind;

mod entity51_wire;
use crate::parasolid::counted_values::CountedValues;
use crate::parasolid::entity_references::{EntityReferences, FieldPosition};
use crate::parasolid::unicode_value::UnicodeValue;
use crate::printable_string::PrintableString;
use entity51_wire::Entity51Wire;
pub(super) mod named_fields;
use named_fields::NamedField;
pub(super) mod body_id;
pub(super) mod deltas_instance;
pub(super) mod deltas_metadata;
pub(super) mod deltas_source;
use deltas_instance::{ParasolidDeltasInstance, ParasolidDeltasSchema};
use deltas_metadata::ParasolidDeltasMetadata;
mod chart_wire;
mod support_uv_wire;
mod transmit_header_wire;
use crate::deltas::partition_prefix::PartitionPrefixState;
use crate::deltas::transmit_state::TransmitState;
use crate::framing::xmt_reference::NonNullXmt;
use transmit_header_wire::TransmitHeaderWire;
pub(super) mod group_member;
mod group_state;
use group_member::GroupMemberTarget;
pub(super) mod group_record;
use crate::deltas::group::GroupReferenceStatus;
use crate::records::group::GroupSelector;
use group_record::GroupOrigin;

use super::substrate::{ParsedStreams, StreamView};

use std::collections::BTreeMap;

/// One complete Parasolid GROUP record with its primary or Deltas source scope.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "group_record::GroupWire")]
pub(super) struct ParasolidGroupRecord {
    /// Globally unique source-record identity.
    pub(super) id: String,
    /// Exact source stream and any paired partition namespace.
    pub(super) origin: GroupOrigin,
    /// Stream-local XMT identity.
    pub(super) xmt: u32,
    /// Source-local serialized node label.
    pub(super) node_id: u32,
    /// Ordered GROUP references without their framing status bytes.
    pub(super) references: [u32; 5],
    /// Selector between the four leading references and the linked reference.
    pub(super) selector: GroupSelector,
    /// Exact serialized record length.
    pub(super) byte_len: u64,
    /// GROUP tag offset in the inflated source stream.
    pub(super) inflated_offset: u64,
}

#[cfg(test)]
std::thread_local! {
    static GROUP_RECORD_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ParasolidGroupRecord {
    fn clone(&self) -> Self {
        GROUP_RECORD_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            origin: self.origin,
            xmt: self.xmt,
            node_id: self.node_id,
            references: self.references,
            selector: self.selector,
            byte_len: self.byte_len,
            inflated_offset: self.inflated_offset,
        }
    }
}

/// One entity member in a fully closed current Parasolid GROUP chain.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "group_member::MemberWire")]
pub(super) struct ParasolidGroupMember {
    /// Globally unique membership identity.
    pub(super) id: String,
    /// Primary stream whose local XMT and node namespaces own the chain.
    pub(super) primary_stream_ordinal: u32,
    /// Current GROUP record XMT identity.
    pub(super) group_xmt: u32,
    /// Current GROUP kernel node identity.
    pub(super) group_node_id: u32,
    /// Zero-based member order from the list head to tail.
    pub(super) ordinal: u32,
    /// `TYPE_91` list-record XMT identity.
    pub(super) list_record_xmt: u32,
    /// Member record XMT identity.
    pub(super) member_xmt: u32,
    /// Member family with its required node and optional current identity.
    pub(super) target: GroupMemberTarget,
}

#[cfg(test)]
std::thread_local! {
    static GROUP_MEMBER_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ParasolidGroupMember {
    fn clone(&self) -> Self {
        GROUP_MEMBER_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            primary_stream_ordinal: self.primary_stream_ordinal,
            group_xmt: self.group_xmt,
            group_node_id: self.group_node_id,
            ordinal: self.ordinal,
            list_record_xmt: self.list_record_xmt,
            member_xmt: self.member_xmt,
            target: self.target,
        }
    }
}

/// Retain primary GROUP records and closed membership from one source cursor.
///
/// Deltas records use partition pairing only for source namespace attribution.
/// A record in an unpaired deltas stream remains exact native
/// evidence but has no partition-local namespace assignment.
pub(super) fn parasolid_groups(
    streams: &[Stream],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
    deltas: &ParasolidDeltasEvents,
) -> Result<(Vec<ParasolidGroupRecord>, Vec<ParasolidGroupMember>), CodecError> {
    let mut paired_partition = BTreeMap::new();

    for (&partition, deltas) in delta_pairs {
        let Ok(partition) = u32::try_from(partition) else {
            continue;
        };
        for &delta in deltas {
            paired_partition.contains_key(&delta);
            paired_partition.insert(delta, partition);
        }
    }
    let mut groups = Vec::new();
    let mut members = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !matches!(stream.kind(), StreamKind::Partition | StreamKind::Plain) {
            continue;
        }
        let Ok(stream_ordinal_u32) = u32::try_from(stream_ordinal) else {
            continue;
        };

        let Some(primary) = crate::records::PrimaryRecords::read(&stream.inflated)? else {
            continue;
        };
        for record in primary.groups() {
            groups.push(ParasolidGroupRecord {
                id: deltas_event_id(
                    stream_ordinal,
                    "parasolid-group",
                    record.offset,
                    Some(record.xmt),
                ),
                origin: if stream.kind() == StreamKind::Plain {
                    GroupOrigin::Plain {
                        stream_ordinal: stream_ordinal_u32,
                    }
                } else {
                    GroupOrigin::Partition {
                        stream_ordinal: stream_ordinal_u32,
                    }
                },
                xmt: record.xmt,
                node_id: record.node_id,
                references: record.references,
                selector: record.selector,
                byte_len: cadmpeg_core::decode::u64_from_index(record.end - record.offset),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
        let records = primary.membership_records();
        let current = group_state::GroupState::from_records(&records);
        current.append_members(stream_ordinal_u32, &mut members);
    }
    for record in &deltas.records {
        let crate::deltas::record_family::RecordFamily::Group {
            node_id,
            selector,
            linked_reference_status,
            references,
        } = &record.family
        else {
            continue;
        };
        groups.push(ParasolidGroupRecord {
            id: replace_group_record_id(&record.id),
            origin: GroupOrigin::Deltas {
                stream_ordinal: record.stream_ordinal,
                partition_stream_ordinal: usize::try_from(record.stream_ordinal)
                    .ok()
                    .and_then(|delta| paired_partition.get(&delta).copied()),
                linked_reference_status: *linked_reference_status,
            },
            xmt: record.xmt,
            node_id: *node_id,
            references: *references,
            selector: *selector,
            byte_len: record.byte_len,
            inflated_offset: record.inflated_offset,
        });
    }
    groups.sort_by(|first, second| {
        (first.origin.stream_ordinal(), first.inflated_offset)
            .cmp(&(second.origin.stream_ordinal(), second.inflated_offset))
    });
    Ok((groups, members))
}

fn replace_group_record_id(id: &str) -> String {
    id.replacen("deltas-record", "parasolid-group", 1)
}

pub(super) fn resolve_group_members(
    members: &mut [ParasolidGroupMember],
    parsed: &ParsedStreams<'_>,
) {
    for member in members {
        let Ok(primary) = usize::try_from(member.primary_stream_ordinal) else {
            continue;
        };
        let graph = parsed.stream(primary).graph.as_ref();
        member.target = member.target.resolve(graph, member.member_xmt);
    }
}

/// One completely bounded record in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "ParasolidDeltasRecordWire")]
pub(super) struct ParasolidDeltasRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Source projection derived from the owned generic instance.
    pub(super) family: crate::deltas::record_family::RecordFamily,
    /// Stream-local XMT identity.
    pub(super) xmt: u32,
    /// Exact serialized record length.
    pub(super) byte_len: u64,
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

impl Serialize for ParasolidDeltasRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use crate::deltas::record_family::RecordFamilyReferences;
        use serde::ser::SerializeStruct;

        let region_type = match &self.family {
            RecordFamily::Region { fields } => Some(fields.region_type),
            RecordFamily::Group { .. } => None,
        };
        let (group_selector, group_linked_reference_status) = match &self.family {
            RecordFamily::Group {
                selector,
                linked_reference_status,
                ..
            } => (Some(*selector), Some(*linked_reference_status)),
            RecordFamily::Region { .. } => (None, None),
        };
        let mut wire = serializer.serialize_struct(
            "ParasolidDeltasRecordWire",
            9 + usize::from(group_selector.is_some())
                + usize::from(group_linked_reference_status.is_some())
                + usize::from(region_type.is_some()),
        )?;
        wire.serialize_field("id", &self.id)?;
        wire.serialize_field("stream_ordinal", &self.stream_ordinal)?;
        wire.serialize_field("family", self.family.family_name())?;
        wire.serialize_field("kind", &self.family.kind())?;
        wire.serialize_field("xmt", &self.xmt)?;
        wire.serialize_field("node_id", &self.family.node_id())?;
        wire.serialize_field("references", &RecordFamilyReferences(&self.family))?;
        if let Some(value) = region_type {
            wire.serialize_field("region_type", &value)?;
        }
        if let Some(value) = group_selector {
            wire.serialize_field("group_selector", &value)?;
        }
        if let Some(value) = group_linked_reference_status {
            wire.serialize_field("group_linked_reference_status", &value)?;
        }
        wire.serialize_field("byte_len", &self.byte_len)?;
        wire.serialize_field("inflated_offset", &self.inflated_offset)?;
        wire.end()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParasolidDeltasRecordWire {
    id: String,
    stream_ordinal: u32,
    family: String,
    kind: u16,
    xmt: u32,
    node_id: u32,
    references: Vec<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_region_type"
    )]
    region_type: Option<crate::deltas::region::RegionType>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_group_selector"
    )]
    group_selector: Option<GroupSelector>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_group_linked_reference_status"
    )]
    group_linked_reference_status: Option<GroupReferenceStatus>,
    byte_len: u64,
    inflated_offset: u64,
}

impl TryFrom<ParasolidDeltasRecordWire> for ParasolidDeltasRecord {
    type Error = &'static str;
    fn try_from(wire: ParasolidDeltasRecordWire) -> Result<Self, Self::Error> {
        if wire.xmt <= 1
            || wire.references.contains(&0)
            || wire.byte_len == 0
            || wire.inflated_offset.checked_add(wire.byte_len).is_none()
        {
            return Err("invalid Deltas projection identity, references, or range");
        }
        let family = match (
            wire.kind,
            wire.family.as_str(),
            wire.region_type,
            wire.group_selector,
            wire.group_linked_reference_status,
        ) {
            (19, "REGION", Some(region_type), None, None) => RecordFamily::Region {
                fields: crate::deltas::region::RegionFields::from_wire(
                    wire.node_id,
                    wire.references,
                    region_type,
                )
                .ok_or("invalid REGION references")?,
            },
            (90, "GROUP", None, Some(selector), Some(linked_reference_status)) => {
                RecordFamily::Group {
                    references: wire
                        .references
                        .try_into()
                        .map_err(|_| "invalid GROUP references")?,
                    node_id: wire.node_id,
                    selector,
                    linked_reference_status,
                }
            }
            _ => return Err("Deltas projection family, kind, or controls disagree"),
        };
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            family,
            xmt: wire.xmt,
            byte_len: wire.byte_len,
            inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod deltas_record_wire_tests {
    use super::ParasolidDeltasRecord;

    #[test]
    fn group_and_region_projections_preserve_values_and_reject_conflicting_controls() {
        for json in [
            r#"{"id":"nx:test:group#0","stream_ordinal":0,"family":"GROUP","kind":90,"xmt":10,"node_id":7,"references":[3,4,5,6,30],"group_selector":4,"group_linked_reference_status":0,"byte_len":22,"inflated_offset":0}"#,
            r#"{"id":"nx:test:region#0","stream_ordinal":0,"family":"REGION","kind":19,"xmt":10,"node_id":7,"references":[3,4,5,6,30],"region_type":83,"byte_len":24,"inflated_offset":0}"#,
        ] {
            let value: ParasolidDeltasRecord = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
            cadmpeg_test_support::native_serialization::assert_native_limit(
                &value,
                serde_json::from_str::<serde_json::Value>(json).unwrap(),
            );
            for (field, invalid) in [
                ("kind", serde_json::json!(12)),
                ("xmt", serde_json::json!(1)),
                ("references", serde_json::json!([0, 1, 1, 1, 1])),
            ] {
                let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
                wire[field] = invalid;
                assert!(serde_json::from_value::<ParasolidDeltasRecord>(wire).is_err());
            }
        }
    }
}

/// Parasolid transmit header at the start of a deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "TransmitHeaderWire")]
pub(super) struct ParasolidDeltasTransmitHeader {
    /// Globally unique header identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    state: TransmitState,
    /// Exact header byte length.
    byte_len: u64,
    /// SHA-256 of the exact header bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
}

/// Leading PARTITION and current PMARK nodes in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidDeltasPartitionPrefix {
    /// Globally unique partition-prefix identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    #[serde(flatten)]
    state: PartitionPrefixState,
    /// Exact partition-prefix byte length.
    byte_len: u64,
    /// SHA-256 of the exact partition-prefix bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// First partition-prefix byte offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Maximal inflated-stream span outside every admitted deltas event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidDeltasResidualSpan {
    /// Globally unique residual-span identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Exact residual byte length.
    byte_len: u64,
    /// SHA-256 of the residual bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// First residual byte offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[derive(Default)]
pub(in crate::native) struct ParasolidDeltasEvents {
    pub(super) transmit_headers: Vec<ParasolidDeltasTransmitHeader>,
    pub(super) records: Vec<ParasolidDeltasRecord>,
    pub(super) instances: Vec<ParasolidDeltasInstance>,
    pub(super) partition_prefixes: Vec<ParasolidDeltasPartitionPrefix>,
    pub(super) schemas: Vec<ParasolidDeltasSchema>,
    pub(super) residual_spans: Vec<ParasolidDeltasResidualSpan>,
    pub(super) metadata: Vec<ParasolidDeltasMetadata>,
}

/// Retain every completely bounded event in every Parasolid deltas stream.
#[cfg(test)]
fn parasolid_deltas_events(streams: &[Stream]) -> ParasolidDeltasEvents {
    let delta_records = streams
        .iter()
        .map(|stream| {
            if stream.kind() == crate::parasolid::StreamKind::Deltas {
                Ok(Some(DeltasRecords::read(&stream.inflated)?))
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>, CodecError>>()
        .expect("bounded test deltas");
    parasolid_deltas_events_from_records(streams, delta_records).expect("bounded test deltas")
}

/// Format one deltas event identity.
fn deltas_event_id(
    stream_ordinal: usize,
    kind: &'static str,
    first: usize,
    second: Option<u32>,
) -> String {
    match second {
        Some(value) => format!("nx:s{stream_ordinal}:{kind}#{first}-{value}"),
        None => format!("nx:s{stream_ordinal}:{kind}#{first}"),
    }
}

/// Materialize native records exclusively from the shared sequential owner.
pub(super) fn parasolid_deltas_events_from_records(
    streams: &[Stream],
    mut owners: Vec<Option<DeltasRecords<'_>>>,
) -> Result<ParasolidDeltasEvents, CodecError> {
    let mut events = ParasolidDeltasEvents::default();
    for (ordinal, stream) in streams.iter().enumerate() {
        if stream.kind() != StreamKind::Deltas {
            continue;
        }
        let ordinal_u32 = u32::try_from(ordinal)
            .map_err(|_| CodecError::malformed("NX deltas stream ordinal exceeds u32"))?;
        let owner = owners
            .get_mut(ordinal)
            .and_then(Option::take)
            .ok_or_else(|| CodecError::Malformed("missing NX Deltas source owner".into()))?;
        if owner.source() != stream.inflated.as_slice() {
            return Err(CodecError::Malformed(
                "NX Deltas owner source differs from retained stream".into(),
            ));
        }
        deltas_events_for_stream(&owner, ordinal_u32, &mut events)?;
    }
    Ok(events)
}

fn deltas_events_for_stream(
    owner: &DeltasRecords<'_>,
    ordinal_u32: u32,
    events: &mut ParasolidDeltasEvents,
) -> Result<(), CodecError> {
    let ordinal = cadmpeg_core::decode::index_from_u32(ordinal_u32);
    let source = owner.source();
    let mut schemas = BTreeMap::new();
    for definition in owner.definitions() {
        let schema = ParasolidDeltasSchema::from_owned(source, ordinal_u32, definition);
        schemas.insert(definition.declaration().start, events.schemas.len());
        events.schemas.push(schema);
    }
    for event in owner.events() {
        let range = event.range();
        let bytes = &source[range.clone()];
        match event.content() {
            EventContent::Header(state) => {
                events.transmit_headers.push(ParasolidDeltasTransmitHeader {
                    id: deltas_event_id(ordinal, "deltas-transmit-header", range.start, None),
                    stream_ordinal: ordinal_u32,
                    state: state.clone(),
                    byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
                    sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(bytes),
                });
            }
            EventContent::PartitionPrefix(state) => {
                events
                    .partition_prefixes
                    .push(ParasolidDeltasPartitionPrefix {
                        id: deltas_event_id(ordinal, "deltas-partition-prefix", range.start, None),
                        stream_ordinal: ordinal_u32,
                        state: *state,
                        byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
                        sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(bytes),
                        inflated_offset: cadmpeg_core::decode::u64_from_index(range.start),
                    });
            }
            EventContent::Metadata(metadata) => events.metadata.push(
                ParasolidDeltasMetadata::from_owned(source, ordinal_u32, range, metadata),
            ),
            EventContent::Instance(instance) => {
                let index = schemas
                    .get(&instance.definition().declaration().start)
                    .ok_or_else(|| CodecError::Malformed("missing NX Deltas schema join".into()))?;
                let value = ParasolidDeltasInstance::from_owned(
                    source,
                    ordinal_u32,
                    event,
                    &events.schemas[*index],
                )?;
                if let Some(family) = value.project_record(&events.schemas[*index]) {
                    events.records.push(ParasolidDeltasRecord {
                        id: deltas_event_id(
                            ordinal,
                            "deltas-record",
                            range.start,
                            Some(u32::from(instance.xmt())),
                        ),
                        stream_ordinal: ordinal_u32,
                        family,
                        xmt: u32::from(instance.xmt()),
                        byte_len: cadmpeg_core::decode::u64_from_index(range.len()),
                        inflated_offset: cadmpeg_core::decode::u64_from_index(range.start),
                    });
                }
                events.instances.push(value);
            }
        }
    }
    let suffix = owner.suffix();
    if !suffix.is_empty() {
        push_deltas_residual_span(
            &mut events.residual_spans,
            ordinal,
            source,
            suffix.start,
            suffix.end,
        )?;
    }
    Ok(())
}

fn push_deltas_residual_span(
    residual_spans: &mut Vec<ParasolidDeltasResidualSpan>,
    stream_ordinal: usize,
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Result<(), CodecError> {
    let residual = &bytes[start..end];
    residual_spans.push(ParasolidDeltasResidualSpan {
        id: deltas_event_id(stream_ordinal, "deltas-residual", start, None),
        stream_ordinal: u32::try_from(stream_ordinal)
            .map_err(|_| CodecError::malformed("NX deltas stream ordinal exceeds u32"))?,
        byte_len: cadmpeg_core::decode::u64_from_index(residual.len()),
        sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(residual),
        inflated_offset: cadmpeg_core::decode::u64_from_index(start),
    });
    Ok(())
}

/// Shared skeleton for Parasolid record families read from the cached per-stream
/// record view. It owns the stream loop, the `nx:s{ordinal}:{ID_STEM}#{xmt}`
/// identity, and the sort by identity; each family supplies only its cached row
/// slice and its record constructor.
pub(super) trait ParasolidStreamRecords {
    /// Cached row type read from the stream's record [`StreamView`].
    type Row: Copy;
    /// Emitted native record type.
    type Record;
    /// Identity stem between the `nx:s{ordinal}:` prefix and the `#{xmt}` suffix.
    const ID_STEM: &'static str;
    /// The cached rows of one stream's record view.
    fn rows<'view>(view: &'view StreamView<'_>) -> &'view [Self::Row];
    /// Cross-reference index carried into the record identity.
    fn xmt(row: &Self::Row) -> u32;
    /// Build one record from its identity, stream ordinal, and cached row.
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record;
    /// The identity of a built record, used as the sort key.
    fn id(record: &Self::Record) -> &str;
}

fn parasolid_record_id(stream_ordinal: usize, stem: &'static str, xmt: u32) -> String {
    format!("nx:s{stream_ordinal}:{stem}#{xmt}")
}

fn parasolid_offset_record_id(
    stream_ordinal: usize,
    stem: &'static str,
    xmt: u32,
    offset: usize,
) -> String {
    format!("nx:s{stream_ordinal}:{stem}#{xmt}-{offset}")
}

/// Run the cached-view record skeleton for one family: map every cached row of
/// every stream to a record, then sort by identity. Deltas and non-Parasolid
/// streams hold empty views, so no per-stream guard is needed.
pub(super) fn per_parasolid_stream<P: ParasolidStreamRecords>(
    parsed: &ParsedStreams,
) -> Result<Vec<P::Record>, CodecError> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        let ordinal = u32::try_from(stream_ordinal)
            .map_err(|_| CodecError::malformed("NX Parasolid stream ordinal exceeds u32"))?;
        for row in P::rows(stream) {
            let id = parasolid_record_id(stream_ordinal, P::ID_STEM, P::xmt(row));
            records.push(P::record(id, ordinal, row));
        }
    }
    records.sort_by(|left, right| P::id(left).cmp(P::id(right)));
    Ok(records)
}

/// Shared skeleton for Parasolid record families scanned fresh from each
/// primary Parasolid stream's inflated bytes. It owns the stream-kind guard, the
/// stream loop, the `nx:s{ordinal}:{ID_STEM}#{xmt}` identity, and the sort; each
/// family supplies only its scanner and its record constructor.
trait ParasolidScanRecords {
    /// Scanned row type produced from the inflated stream bytes.
    type Row;
    /// Emitted native record type.
    type Record;
    /// Identity stem between the `nx:s{ordinal}:` prefix and the `#{xmt}` suffix.
    const ID_STEM: &'static str;
    /// Scan one inflated Parasolid stream into its rows.
    fn scan(bytes: &[u8]) -> Vec<Self::Row>;
    /// Cross-reference index carried into the record identity.
    fn xmt(row: &Self::Row) -> u32;
    /// Build one record from its identity, stream ordinal, and scanned row.
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record;
    /// The identity of a built record, used as the sort key.
    fn id(record: &Self::Record) -> &str;
}

/// Run the fresh-scan record skeleton for one family on primary Parasolid
/// streams, map each scanned row to a record, then sort by identity.
fn per_parasolid_scan<P: ParasolidScanRecords>(
    streams: &[Stream],
) -> Result<Vec<P::Record>, CodecError> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind().is_parasolid() || stream.kind() == StreamKind::Deltas {
            continue;
        }
        let ordinal = u32::try_from(stream_ordinal)
            .map_err(|_| CodecError::malformed("NX Parasolid stream ordinal exceeds u32"))?;
        for row in P::scan(&stream.inflated) {
            let id = parasolid_record_id(stream_ordinal, P::ID_STEM, P::xmt(&row));
            records.push(P::record(id, ordinal, row));
        }
    }
    records.sort_by(|left, right| P::id(left).cmp(P::id(right)));
    Ok(records)
}

/// Complete typed source record for one Parasolid offset surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidOffsetSurfaceRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the offset surface.
    xmt: u32,
    /// Serialized `V`, `I`, or `U` discriminator.
    discriminator: crate::topology::OffsetSurfaceDiscriminator,
    /// Serialized true-offset flag.
    true_offset: bool,
    /// Checked support reference and signed model distance.
    #[serde(flatten)]
    state: OffsetSurfaceState,
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

impl ParasolidStreamRecords for ParasolidOffsetSurfaceRecord {
    type Row = crate::topology::OffsetSurface;
    type Record = ParasolidOffsetSurfaceRecord;
    const ID_STEM: &'static str = "offset-surface-record";
    fn rows<'view>(view: &'view StreamView<'_>) -> &'view [Self::Row] {
        &view.offset_surfaces
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidOffsetSurfaceRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            discriminator: row.discriminator,
            true_offset: row.true_offset,
            state: row.state,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid trimmed curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidTrimmedCurveRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the trimmed curve.
    xmt: u32,
    #[serde(flatten)]
    state: crate::topology::trimmed_curve_state::TrimmedCurveState,
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

impl ParasolidStreamRecords for ParasolidTrimmedCurveRecord {
    type Row = crate::topology::TrimmedCurve;
    type Record = ParasolidTrimmedCurveRecord;
    const ID_STEM: &'static str = "trimmed-curve-record";
    fn rows<'view>(view: &'view StreamView<'_>) -> &'view [Self::Row] {
        &view.trimmed_curves
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidTrimmedCurveRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            state: row.state,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid surface curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidSurfaceCurveRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the surface curve.
    xmt: u32,
    #[serde(flatten)]
    state: crate::topology::surface_curve_state::SurfaceCurveState,
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

impl ParasolidStreamRecords for ParasolidSurfaceCurveRecord {
    type Row = crate::topology::SurfaceCurve;
    type Record = ParasolidSurfaceCurveRecord;
    const ID_STEM: &'static str = "surface-curve-record";
    fn rows<'view>(view: &'view StreamView<'_>) -> &'view [Self::Row] {
        &view.surface_curves
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidSurfaceCurveRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            state: row.state,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid blend-bound bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidBlendBoundRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    #[serde(flatten)]
    state: crate::intersection::blend_bound_state::BlendBoundState,
    /// Serialized partition/deltas and direct/escaped framing.
    framing: crate::intersection::BlendBoundFraming,
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid blend-bound bridges.
pub(super) fn parasolid_blend_bound_records(
    streams: &[Stream],
) -> Result<Vec<ParasolidBlendBoundRecord>, CodecError> {
    per_parasolid_scan::<ParasolidBlendBoundRecord>(streams)
}

impl ParasolidScanRecords for ParasolidBlendBoundRecord {
    type Row = crate::intersection::BlendBound;
    type Record = ParasolidBlendBoundRecord;
    const ID_STEM: &'static str = "blend-bound-record";
    fn scan(bytes: &[u8]) -> Vec<Self::Row> {
        crate::intersection::blend_bounds(bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.state.xmt()
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidBlendBoundRecord {
            id,
            stream_ordinal,
            state: row.state,
            framing: row.framing,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid `term_use` endpoint.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "ParasolidTermUseRecordWire")]
pub(super) struct ParasolidTermUseRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the endpoint.
    xmt: NonNullXmt,
    /// Two-byte endpoint-form discriminator as printable ASCII.
    form: crate::intersection::TermUseForm,
    /// Complete count-selected physical hvec storage.
    data: crate::intersection::limit_state::LimitState,
    /// Serialized record framing.
    framing: crate::intersection::TermUseFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[derive(Serialize)]
struct ParasolidTermUseRecordRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    xmt: NonNullXmt,
    count: u32,
    form: crate::intersection::TermUseForm,
    data: crate::intersection::limit_state::LimitState,
    framing: crate::intersection::TermUseFraming,
    inflated_offset: u64,
}

impl Serialize for ParasolidTermUseRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ParasolidTermUseRecordRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            xmt: self.xmt,
            count: self.form.count(),
            form: self.form,
            data: self.data,
            framing: self.framing,
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ParasolidTermUseRecordWire {
    /// Globally unique record identity.
    id: String,
    /// Zero-based source stream ordinal.
    stream_ordinal: u32,
    /// Cross-reference index of the endpoint.
    xmt: NonNullXmt,
    /// Serialized leading count.
    count: u32,
    /// Two-byte endpoint-form discriminator as printable ASCII.
    form: crate::intersection::TermUseForm,
    /// Complete count-selected physical hvec storage.
    data: crate::intersection::limit_state::LimitState,
    /// Serialized record framing.
    framing: crate::intersection::TermUseFraming,
    /// Tag or inline-payload offset in the inflated stream.
    inflated_offset: u64,
}

#[cfg(test)]
impl From<ParasolidTermUseRecord> for ParasolidTermUseRecordWire {
    fn from(value: ParasolidTermUseRecord) -> Self {
        Self {
            count: value.form.count(),
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            xmt: value.xmt,
            form: value.form,
            data: value.data,
            framing: value.framing,
            inflated_offset: value.inflated_offset,
        }
    }
}

impl TryFrom<ParasolidTermUseRecordWire> for ParasolidTermUseRecord {
    type Error = &'static str;
    fn try_from(wire: ParasolidTermUseRecordWire) -> Result<Self, Self::Error> {
        if wire.count != wire.form.count() || wire.count != wire.data.count() {
            return Err("term_use count disagrees with endpoint form");
        }
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            xmt: wire.xmt,
            form: wire.form,
            data: wire.data,
            framing: wire.framing,
            inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod term_use_wire_tests {
    use super::ParasolidTermUseRecord;

    #[test]
    fn complete_limit_retains_both_native_points_and_signed_zero() {
        let json = r#"{"id":"term","stream_ordinal":0,"xmt":2,"count":2,"form":"TF","data":{"layout":"xyz3","hvecs":[[0.0,-0.0,1.0],[0.0,-0.0,1.0]]},"framing":"direct","inflated_offset":10}"#;
        let record: ParasolidTermUseRecord = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&super::ParasolidTermUseRecordWire::from(record.clone())).unwrap()
        );
    }

    #[test]
    fn term_use_wire_rejects_count_form_and_storage_disagreement() {
        let valid = serde_json::json!({"id":"term", "stream_ordinal":0, "xmt":2,
            "count":2,"form":"TF", "data":{"layout":"xyz3","hvecs":[[0.0,0.0,0.0],[1.0,0.0,0.0]]},
            "framing":"direct","inflated_offset":0});
        for (field, value) in [
            ("count", serde_json::json!(1)),
            ("form", serde_json::json!("L?")),
            (
                "data",
                serde_json::json!({"layout":"xyz3","hvecs":[[0.0,0.0,0.0]]}),
            ),
            ("data", serde_json::json!({"layout":"xyz3","hvecs":[]})),
            (
                "data",
                serde_json::json!({"layout":"ext11","hvecs":[{"point":[0.0,0.0,0.0]}]}),
            ),
        ] {
            let mut malformed = valid.clone();
            malformed[field] = value;
            assert!(
                serde_json::from_value::<ParasolidTermUseRecord>(malformed).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn term_use_native_limit_refuses_before_id_copy() {
        let json = r#"{"id":"nx:parasolid:term-use#0","stream_ordinal":0,"xmt":2,"count":2,"form":"TF","data":{"layout":"xyz3","hvecs":[[0.0,-0.0,1.0],[0.0,-0.0,1.0]]},"framing":"direct","inflated_offset":10}"#;
        let record: ParasolidTermUseRecord = serde_json::from_str(json).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }
}

#[cfg(test)]
mod term_use_identity_tests {
    #[test]
    fn term_use_wire_rejects_reserved_record_identities() {
        for identity in [0, 1] {
            let json = format!(
                r#"{{"id":"term","stream_ordinal":0,"xmt":{identity},"count":2,"form":"TF","data":{{"layout":"xyz3","hvecs":[[0.0,0.0,1.0],[0.0,0.0,1.0]]}},"framing":"direct","inflated_offset":10}}"#
            );
            assert!(serde_json::from_str::<super::ParasolidTermUseRecord>(&json)
                .unwrap_err()
                .to_string()
                .contains("xmt"));
        }
    }
}

/// Decode complete typed source records for Parasolid `term_use` endpoints.
pub(super) fn parasolid_term_use_records(
    streams: &[Stream],
) -> Result<Vec<ParasolidTermUseRecord>, CodecError> {
    let mut records = Vec::new();
    for (ordinal, stream) in streams.iter().enumerate() {
        let crate::parasolid::StreamBody::Parasolid { subtype, .. } = &stream.body else {
            continue;
        };
        if stream.kind() == StreamKind::Deltas {
            continue;
        }
        let Some(point_layout) = subtype.chart_point_layout() else {
            continue;
        };
        let rows = crate::intersection::term_use_records(&stream.inflated, point_layout);
        if rows.is_empty() {
            continue;
        }
        let ordinal_u32 = u32::try_from(ordinal)
            .map_err(|_| CodecError::malformed("NX LIMIT stream ordinal exceeds u32"))?;
        for row in rows {
            records.push(ParasolidTermUseRecord {
                id: parasolid_offset_record_id(
                    ordinal,
                    "term-use-record",
                    u32::from(row.xmt),
                    row.pos,
                ),
                stream_ordinal: ordinal_u32,
                xmt: row.xmt,
                form: row.form,
                data: row.data,
                framing: row.framing,
                inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
            });
        }
    }
    records.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(records)
}

/// Complete typed source record for one Parasolid support-UV values array.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "support_uv_wire::SupportUvWire")]
pub(super) struct ParasolidSupportUvRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the values array.
    xmt: NonNullXmt,
    /// Exact finite packed support tuples.
    values: crate::intersection::support_uv_values::SupportUvValues,
    /// Serialized record framing.
    framing: crate::intersection::SupportUvFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid support-UV arrays.
pub(super) fn parasolid_support_uv_records(
    streams: &[Stream],
) -> Result<Vec<ParasolidSupportUvRecord>, CodecError> {
    per_parasolid_scan::<ParasolidSupportUvRecord>(streams)
}

impl ParasolidScanRecords for ParasolidSupportUvRecord {
    type Row = crate::intersection::SupportUvRecord;
    type Record = ParasolidSupportUvRecord;
    const ID_STEM: &'static str = "support-uv-record";
    fn scan(bytes: &[u8]) -> Vec<Self::Row> {
        crate::intersection::support_uv_records(bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        u32::from(row.xmt)
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidSupportUvRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            values: row.values,
            framing: row.framing,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one physical Parasolid `CHART_s` record.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "chart_wire::ChartWire")]
pub(super) struct ParasolidChartRecord {
    /// Globally unique physical-record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the chart.
    xmt: NonNullXmt,
    /// Checked chart preamble.
    preamble: crate::intersection::chart_samples::ChartPreamble,
    /// Points with exactly the fields admitted by their Hvec layout.
    data: crate::intersection::chart_samples::SourceChartData,
    /// Serialized record framing.
    framing: crate::intersection::ChartFraming,
    /// Type-tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Decode every complete physical Parasolid chart source record.
pub(super) fn parasolid_chart_records(
    streams: &[Stream],
) -> Result<Vec<ParasolidChartRecord>, CodecError> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        let crate::parasolid::StreamBody::Parasolid { subtype, .. } = &stream.body else {
            continue;
        };
        if stream.kind() == StreamKind::Deltas {
            continue;
        }
        let Some(point_layout) = subtype.chart_point_layout() else {
            continue;
        };
        for chart in crate::intersection::chart_source_records(&stream.inflated, point_layout) {
            records.push(ParasolidChartRecord {
                id: parasolid_offset_record_id(
                    stream_ordinal,
                    "chart-record",
                    u32::from(chart.xmt),
                    chart.pos,
                ),
                stream_ordinal: u32::try_from(stream_ordinal)
                    .map_err(|_| CodecError::malformed("NX stream ordinal exceeds u32"))?,
                xmt: chart.xmt,
                preamble: chart.preamble,
                data: chart.data,
                framing: chart.framing,
                inflated_offset: cadmpeg_core::decode::u64_from_index(chart.pos),
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

/// Complete typed source record for one Parasolid surface-intersection curve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidIntersectionRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the construction.
    xmt: u32,
    /// Five ordered common-header references.
    header_references: [u32; 5],
    /// Serialized orientation sense.
    sense: bool,
    /// Six ordered support and witness references.
    construction_references: [u32; 6],
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

impl ParasolidStreamRecords for ParasolidIntersectionRecord {
    type Row = crate::topology::CompositeCurve;
    type Record = ParasolidIntersectionRecord;
    const ID_STEM: &'static str = "intersection-record";
    fn rows<'view>(view: &'view StreamView<'_>) -> &'view [Self::Row] {
        &view.intersections.source_constructions
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidIntersectionRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            header_references: row
                .header_references
                .map(crate::framing::xmt_reference::XmtTarget::to_wire),
            sense: row.sense,
            construction_references: row
                .references
                .map(crate::framing::xmt_reference::XmtTarget::to_wire),
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed type-56 rolling-ball blend-surface record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidBlendSurfaceRecord {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based embedded Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local `BLEND_SURF` identity.
    xmt: u32,
    /// Checked supports, offsets, and thumb weights.
    #[serde(flatten)]
    state: BlendSurfaceState,
    /// Offset of the type tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[cfg(test)]
mod blend_surface_wire_tests {
    use super::ParasolidBlendSurfaceRecord;

    #[test]
    fn checked_blend_state_retains_the_flat_native_wire() {
        let json = r#"{"id":"blend","stream_ordinal":0,"xmt":20,"support_xmts":[6,7],"spine_xmt":0,"offsets":[-3.0,3.0],"thumb_weights":[-0.0,-2.0],"inflated_offset":10}"#;
        let record: ParasolidBlendSurfaceRecord = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
        for (field, value) in [
            ("support_xmts", serde_json::json!([1, 7])),
            ("offsets", serde_json::json!([3.0, 4.0])),
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire[field] = value;
            assert!(serde_json::from_value::<ParasolidBlendSurfaceRecord>(wire)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }
}

fn default_legal_owner_flag_count() -> u8 {
    16
}

fn is_default_legal_owner_flag_count(value: impl std::borrow::Borrow<u8>) -> bool {
    *value.borrow() == default_legal_owner_flag_count()
}

/// Named Parasolid attribute class declared in one inflated body stream.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ParasolidAttributeDefinitionWire")]
pub(super) struct ParasolidAttributeDefinition {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based embedded stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local definition record identity.
    pub(super) xmt: NonNullXmt,
    /// Optional stream-local next-definition target.
    pub(super) next_definition_xmt: Option<XmtTarget>,
    /// Stream-local type-79 identifier identity.
    pub(super) identifier_xmt: NonNullXmt,
    /// Offset of the resolved type-79 identifier in the inflated stream.
    pub(super) identifier_inflated_offset: u64,
    /// Exact printable attribute class name.
    pub(super) name: PrintableString<String>,
    /// Numeric attribute type identifier.
    pub(super) type_id: NonZeroU32,
    /// Ordered actions for the eight logged event families.
    pub(super) action_codes: [AttributeAction; 8],
    /// Optional stream-local field-name-list target.
    pub(super) field_names_xmt: Option<XmtTarget>,
    /// Ordered legal-owner flags.
    pub(super) legal_owner_flags: crate::parasolid::LegalOwnerFlags,
    /// One serialized code for every declared field.
    pub(super) field_codes: Vec<AttributeField>,
    /// Offset of the declaration in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[derive(Serialize)]
struct ParasolidAttributeDefinitionRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    xmt: u32,
    next_definition_xmt: u32,
    identifier_xmt: u32,
    identifier_inflated_offset: u64,
    name: &'a str,
    type_id: u32,
    action_codes: [AttributeAction; 8],
    field_names_xmt: u32,
    legal_owner_flags: NativeBytes<[u8; 16]>,
    #[serde(skip_serializing_if = "is_default_legal_owner_flag_count")]
    legal_owner_flag_count: u8,
    field_count: usize,
    field_codes: &'a [AttributeField],
    inflated_offset: u64,
}

impl Serialize for ParasolidAttributeDefinition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ParasolidAttributeDefinitionRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            xmt: self.xmt.into(),
            next_definition_xmt: XmtTarget::to_wire(self.next_definition_xmt),
            identifier_xmt: self.identifier_xmt.into(),
            identifier_inflated_offset: self.identifier_inflated_offset,
            name: self.name.as_str(),
            type_id: self.type_id.get(),
            action_codes: self.action_codes,
            field_names_xmt: XmtTarget::to_wire(self.field_names_xmt),
            legal_owner_flags: (self.legal_owner_flags.padded()).into(),
            legal_owner_flag_count: u8::try_from(self.legal_owner_flags.as_slice().len())
                .map_err(serde::ser::Error::custom)?,
            field_count: self.field_codes.len(),
            field_codes: &self.field_codes,
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ParasolidAttributeDefinitionWire {
    /// Globally unique native-record identity.
    id: String,
    /// Zero-based embedded stream ordinal.
    stream_ordinal: u32,
    /// Stream-local definition record identity.
    xmt: u32,
    /// Stream-local next-definition identity; `1` is null.
    next_definition_xmt: u32,
    /// Stream-local type-79 identifier identity.
    identifier_xmt: u32,
    /// Offset of the resolved type-79 identifier in the inflated stream.
    identifier_inflated_offset: u64,
    /// Exact printable attribute class name.
    name: String,
    /// Numeric attribute type identifier.
    type_id: u32,
    /// Ordered actions for the eight logged event families.
    action_codes: [AttributeAction; 8],
    /// Stream-local field-name-list identity; `1` is null.
    field_names_xmt: u32,
    /// Ordered legal-owner flags.
    legal_owner_flags: NativeBytes<[u8; 16]>,
    /// Number of legal-owner flags serialized by the definition.
    #[serde(
        default = "default_legal_owner_flag_count",
        skip_serializing_if = "is_default_legal_owner_flag_count"
    )]
    legal_owner_flag_count: u8,
    /// Declared number of fields.
    field_count: usize,
    /// One serialized code for every declared field.
    field_codes: Vec<AttributeField>,
    /// Offset of the declaration in the inflated stream.
    inflated_offset: u64,
}

#[cfg(test)]
impl From<ParasolidAttributeDefinition> for ParasolidAttributeDefinitionWire {
    fn from(value: ParasolidAttributeDefinition) -> Self {
        Self {
            legal_owner_flag_count: u8::try_from(value.legal_owner_flags.as_slice().len())
                .expect("fixture value fits u8"),
            legal_owner_flags: (value.legal_owner_flags.padded()).into(),
            field_count: value.field_codes.len(),
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            xmt: value.xmt.into(),
            next_definition_xmt: XmtTarget::to_wire(value.next_definition_xmt),
            identifier_xmt: value.identifier_xmt.into(),
            identifier_inflated_offset: value.identifier_inflated_offset,
            name: value.name.into_inner(),
            type_id: value.type_id.get(),
            action_codes: value.action_codes,
            field_names_xmt: XmtTarget::to_wire(value.field_names_xmt),
            field_codes: value.field_codes,
            inflated_offset: value.inflated_offset,
        }
    }
}

impl TryFrom<ParasolidAttributeDefinitionWire> for ParasolidAttributeDefinition {
    type Error = &'static str;
    fn try_from(wire: ParasolidAttributeDefinitionWire) -> Result<Self, Self::Error> {
        let count = usize::from(wire.legal_owner_flag_count);
        let flags = wire
            .legal_owner_flags
            .get(..count)
            .ok_or("attribute owner flag count exceeds the wire array")?;
        let legal_owner_flags = crate::parasolid::LegalOwnerFlags::try_from(flags)?;
        if wire.legal_owner_flags != legal_owner_flags.padded() {
            return Err("attribute owner flag padding is nonzero");
        }
        if wire.field_count != wire.field_codes.len() {
            return Err("attribute field count disagrees with field codes");
        }
        Ok(Self {
            legal_owner_flags,
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            xmt: NonNullXmt::try_from(wire.xmt).map_err(|_| "xmt must exceed one")?,
            next_definition_xmt: XmtTarget::from_wire(wire.next_definition_xmt),
            identifier_xmt: NonNullXmt::try_from(wire.identifier_xmt)
                .map_err(|_| "identifier_xmt must exceed one")?,
            identifier_inflated_offset: wire.identifier_inflated_offset,
            name: PrintableString::new(wire.name)
                .map_err(|_| "name must be nonempty printable ASCII")?,
            type_id: NonZeroU32::new(wire.type_id).ok_or("type_id must be nonzero")?,
            action_codes: wire.action_codes,
            field_names_xmt: XmtTarget::from_wire(wire.field_names_xmt),
            field_codes: wire.field_codes,
            inflated_offset: wire.inflated_offset,
        })
    }
}

/// Counted Parasolid type-99 field-name reference record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidFieldNamesRecord {
    /// Globally unique native-record identity.
    id: String,
    /// Zero-based embedded stream ordinal.
    stream_ordinal: u32,
    /// Stream-local record identity.
    xmt: NonNullXmt,
    /// Ordered stream-local character or Unicode value references.
    name_xmts: NameReferences,
    /// Exact framed record length.
    byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    inflated_offset: u64,
}

/// Complete type-80 declaration-to-field-name-list relation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "named_fields::FieldNamesWire")]
pub(super) struct ParasolidAttributeFieldNames {
    /// Globally unique relation identity.
    pub(super) id: String,
    /// Zero-based embedded stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Owning type-80 declaration.
    pub(super) attribute_definition: String,
    /// Uniquely resolved type-99 field-name record.
    pub(super) field_names_record: String,
    /// Ordered exact names paired with their resolved value records.
    pub(super) fields: Vec<NamedField>,
}

/// Explicit topology-record ownership of one Parasolid attribute list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidTopologyAttributeListReference {
    /// Globally unique reference identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Parasolid topology record type.
    pub(super) topology_type: TopologyAttributeKind,
    /// Stream-local topology-record identity.
    pub(super) topology_xmt: u32,
    /// Stream-local attribute-list identity.
    pub(super) attribute_list_xmt: u32,
    /// Uniquely resolved type-81 attribute-list record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_attribute_list_record"
    )]
    pub(super) attribute_list_record: Option<String>,
    /// Offset of the attribute-list field in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Framed Parasolid type-81 entity/attribute-list record.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Entity51Wire")]
pub(super) struct ParasolidEntity51Record {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    xmt: NonNullXmt,
    /// Serialized sequence value.
    sequence: NonZeroU32,
    /// Stream-local type-80 attribute-definition identity.
    definition_xmt: u32,
    /// Five fixed leading stream-local references.
    leading_references: [u32; 5],
    /// Variable trailing stream-local references counted by `flags`.
    trailing_references: EntityReferences,
    /// Exact framed record length.
    byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Self-framed printable Parasolid type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity54StringRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    xmt: NonNullXmt,
    /// Exact nonempty printable value.
    pub(super) value: PrintableString<String>,
    /// Exact framed record length.
    byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Counted Parasolid type-82 unsigned-integer record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity52IntegerRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    pub(super) xmt: NonNullXmt,
    /// Ordered big-endian unsigned values.
    pub(super) values: CountedValues<u32>,
    /// Exact framed record length.
    pub(super) byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Counted Parasolid type-83 finite binary64 record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity53DoubleRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    pub(super) xmt: NonNullXmt,
    /// Ordered finite big-endian binary64 values.
    pub(super) values: CountedValues<f64>,
    /// Exact framed record length.
    pub(super) byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Parasolid vector-shaped attribute-value family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ParasolidVectorValueKind {
    /// Type-85 point values.
    Points,
    /// Type-86 free-vector values.
    Vectors,
    /// Type-89 direction values.
    Directions,
}

/// Counted Parasolid type-85, type-86, or type-89 vector-shaped value record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidEntityVectorRecord {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Exact value family.
    pub(super) kind: ParasolidVectorValueKind,
    /// Stream-local record identity.
    pub(super) xmt: NonNullXmt,
    /// Ordered finite xyz values.
    pub(super) values: CountedValues<[f64; 3]>,
    /// Exact framed record length.
    pub(super) byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Counted Parasolid type-87 axis-value record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity57AxisRecord {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    pub(super) xmt: NonNullXmt,
    /// Ordered axes, each retaining its two serialized xyz vectors.
    pub(super) values: CountedValues<[[f64; 3]; 2]>,
    /// Exact framed record length.
    pub(super) byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Counted Parasolid type-88 tag-value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity58TagRecord {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    pub(super) xmt: NonNullXmt,
    /// Ordered exact tag values.
    pub(super) values: CountedValues<u32>,
    /// Exact framed record length.
    pub(super) byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Counted Parasolid type-98 Unicode-value record.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ParasolidEntity62UnicodeRecordWire")]
pub(super) struct ParasolidEntity62UnicodeRecord {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    pub(super) xmt: NonNullXmt,
    /// Validated Unicode scalar string.
    pub(super) value: UnicodeValue,
    /// Exact framed record length.
    pub(super) byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

struct UnicodeCodeUnits<'a>(&'a str);

impl Serialize for UnicodeCodeUnits<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut units = serializer.serialize_seq(Some(self.0.encode_utf16().count()))?;
        for unit in self.0.encode_utf16() {
            units.serialize_element(&unit)?;
        }
        units.end()
    }
}

#[derive(Serialize)]
struct ParasolidEntity62UnicodeRecordRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    xmt: NonNullXmt,
    code_units: UnicodeCodeUnits<'a>,
    value: &'a UnicodeValue,
    byte_len: u64,
    inflated_offset: u64,
}

impl Serialize for ParasolidEntity62UnicodeRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ParasolidEntity62UnicodeRecordRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            xmt: self.xmt,
            code_units: UnicodeCodeUnits(self.value.as_str()),
            value: &self.value,
            byte_len: self.byte_len,
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct ParasolidEntity62UnicodeRecordWire {
    id: String,
    stream_ordinal: u32,
    xmt: NonNullXmt,
    code_units: Vec<u16>,
    value: UnicodeValue,
    byte_len: u64,
    inflated_offset: u64,
}

#[cfg(test)]
impl From<ParasolidEntity62UnicodeRecord> for ParasolidEntity62UnicodeRecordWire {
    fn from(value: ParasolidEntity62UnicodeRecord) -> Self {
        let code_units = value.value.as_str().encode_utf16().collect();
        Self {
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            xmt: value.xmt,
            code_units,
            value: value.value,
            byte_len: value.byte_len,
            inflated_offset: value.inflated_offset,
        }
    }
}

impl TryFrom<ParasolidEntity62UnicodeRecordWire> for ParasolidEntity62UnicodeRecord {
    type Error = &'static str;
    fn try_from(wire: ParasolidEntity62UnicodeRecordWire) -> Result<Self, Self::Error> {
        if !wire
            .value
            .as_str()
            .encode_utf16()
            .eq(wire.code_units.iter().copied())
        {
            return Err("ParasolidEntity62UnicodeRecord.code_units disagrees with value");
        }
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            xmt: wire.xmt,
            value: wire.value,
            byte_len: wire.byte_len,
            inflated_offset: wire.inflated_offset,
        })
    }
}

/// Attribute-value records discovered by one pass over the Parasolid streams.
pub(in crate::native) struct ParasolidEntityValueRecords {
    pub(super) integers: Vec<ParasolidEntity52IntegerRecord>,
    pub(super) doubles: Vec<ParasolidEntity53DoubleRecord>,
    pub(super) strings: Vec<ParasolidEntity54StringRecord>,
    pub(super) vectors: Vec<ParasolidEntityVectorRecord>,
    pub(super) axes: Vec<ParasolidEntity57AxisRecord>,
    pub(super) tags: Vec<ParasolidEntity58TagRecord>,
    pub(super) unicode: Vec<ParasolidEntity62UnicodeRecord>,
}

/// Numeric value-record family referenced by a type-81 record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ParasolidEntity51NumericKind {
    /// Type-82 unsigned-integer lane.
    UnsignedIntegers,
    /// Type-83 binary64 lane.
    Doubles,
}

/// Exact type-81 reference to one uniquely resolved numeric value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity51NumericUse {
    /// Globally unique use identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Owning type-81 record.
    pub(super) entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    #[serde(rename = "reference_ordinal")]
    pub(super) position: FieldPosition,
    /// Stream-local referenced xmt.
    pub(super) referenced_xmt: NonNullXmt,
    /// Numeric record family.
    pub(super) kind: ParasolidEntity51NumericKind,
    /// Uniquely resolved numeric record.
    pub(super) value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Exact type-81 reference to a uniquely resolved type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity51StringUse {
    /// Globally unique use identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Owning type-81 record.
    pub(super) entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    #[serde(rename = "reference_ordinal")]
    pub(super) position: FieldPosition,
    /// Stream-local referenced xmt.
    referenced_xmt: NonNullXmt,
    /// Uniquely resolved type-84 string record.
    pub(super) string_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Exact type-81 reference to one uniquely resolved structured value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity51StructuredUse {
    /// Globally unique use identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Owning type-81 record.
    pub(super) entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    #[serde(rename = "reference_ordinal")]
    pub(super) position: FieldPosition,
    /// Stream-local referenced xmt.
    pub(super) referenced_xmt: NonNullXmt,
    /// Structured value-record family.
    pub(super) kind: StructuredValueKind,
    /// Uniquely resolved structured value record.
    pub(super) value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Resolved registered class of one Parasolid type-81 attribute instance.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(test), derive(Clone))]
pub(super) struct ParasolidAttributeClassUse {
    /// Globally unique relation identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Type-81 attribute-instance record.
    entity_51_record: String,
    /// Stream-local XMT of the matched type-80 definition.
    definition_xmt: NonNullXmt,
    /// Uniquely matched attribute definition.
    attribute_definition: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[cfg(test)]
std::thread_local! {
    static ATTRIBUTE_CLASS_USE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ParasolidAttributeClassUse {
    fn clone(&self) -> Self {
        ATTRIBUTE_CLASS_USE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            stream_ordinal: self.stream_ordinal,
            entity_51_record: self.entity_51_record.clone(),
            definition_xmt: self.definition_xmt,
            attribute_definition: self.attribute_definition.clone(),
            inflated_offset: self.inflated_offset,
        }
    }
}

#[derive(Serialize)]
struct ParasolidAttributeClassUseRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    entity_51_record: &'a str,
    definition_xmt: NonNullXmt,
    attribute_definition: &'a str,
}

impl Serialize for ParasolidAttributeClassUse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ParasolidAttributeClassUseRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            entity_51_record: &self.entity_51_record,
            definition_xmt: self.definition_xmt,
            attribute_definition: &self.attribute_definition,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
#[derive(Serialize, Deserialize)]
struct ParasolidAttributeClassUseWire {
    id: String,
    stream_ordinal: u32,
    entity_51_record: String,
    definition_xmt: NonNullXmt,
    attribute_definition: String,
}

#[cfg(test)]
impl From<ParasolidAttributeClassUse> for ParasolidAttributeClassUseWire {
    fn from(value: ParasolidAttributeClassUse) -> Self {
        Self {
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            entity_51_record: value.entity_51_record,
            definition_xmt: value.definition_xmt,
            attribute_definition: value.attribute_definition,
        }
    }
}

/// Value-record family assigned to one declared Parasolid attribute field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ParasolidAttributeFieldValueKind {
    /// Type-82 integer values.
    UnsignedIntegers,
    /// Type-83 binary64 values.
    Doubles,
    /// Type-84 character values.
    String,
    /// Type-85 point values.
    Points,
    /// Type-86 vector values.
    Vectors,
    /// Type-87 axis values.
    Axes,
    /// Type-88 tag values.
    Tags,
    /// Type-89 direction values.
    Directions,
    /// Type-98 Unicode values.
    Unicode,
}

impl ParasolidAttributeFieldValueKind {
    pub(super) fn field_code(self) -> AttributeField {
        match self {
            Self::UnsignedIntegers => AttributeField::Integer,
            Self::Doubles => AttributeField::Real,
            Self::String => AttributeField::Character,
            Self::Points => AttributeField::Point,
            Self::Vectors => AttributeField::Vector,
            Self::Directions => AttributeField::Direction,
            Self::Axes => AttributeField::Axis,
            Self::Tags => AttributeField::Tag,
            Self::Unicode => AttributeField::Unicode,
        }
    }
}

/// One uniquely typed type-81 field reference joined to its type-80 declaration.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "FieldUseWire")]
pub(super) struct ParasolidAttributeFieldUse {
    /// Globally unique relation identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Resolved class relation for the attribute instance.
    pub(super) attribute_class_use: String,
    /// Type-81 attribute-instance record.
    pub(super) entity_51_record: String,
    /// Uniquely matched attribute definition.
    pub(super) attribute_definition: String,
    /// Position in the field declaration and complete type-81 reference lane.
    pub(super) position: FieldPosition,
    /// Resolved value-record family.
    pub(super) value_kind: ParasolidAttributeFieldValueKind,
    /// Type-81-to-value relation carrying this field.
    pub(super) value_use: String,
    /// Uniquely resolved value record.
    pub(super) value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[cfg(test)]
std::thread_local! {
    static ATTRIBUTE_FIELD_USE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ParasolidAttributeFieldUse {
    fn clone(&self) -> Self {
        ATTRIBUTE_FIELD_USE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            stream_ordinal: self.stream_ordinal,
            attribute_class_use: self.attribute_class_use.clone(),
            entity_51_record: self.entity_51_record.clone(),
            attribute_definition: self.attribute_definition.clone(),
            position: self.position,
            value_kind: self.value_kind,
            value_use: self.value_use.clone(),
            value_record: self.value_record.clone(),
            inflated_offset: self.inflated_offset,
        }
    }
}

/// Resolved class of one topology-owned Parasolid attribute instance.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(test), derive(Clone))]
pub(super) struct ParasolidTopologyAttributeClassUse {
    /// Globally unique relation identity.
    pub(super) id: String,
    /// Owning topology-to-attribute relation.
    pub(super) topology_attribute_reference: String,
    /// Topology-owned type-81 attribute-instance record.
    pub(super) entity_51_record: String,
    /// Resolved class relation for the attribute instance.
    pub(super) attribute_class_use: String,
    /// Stream-local XMT of the matched type-80 definition.
    pub(super) definition_xmt: NonNullXmt,
    /// Uniquely matched attribute definition.
    pub(super) attribute_definition: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Offset of the owning type-81 record in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[cfg(test)]
std::thread_local! {
    static TOPOLOGY_ATTRIBUTE_CLASS_USE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ParasolidTopologyAttributeClassUse {
    fn clone(&self) -> Self {
        TOPOLOGY_ATTRIBUTE_CLASS_USE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            topology_attribute_reference: self.topology_attribute_reference.clone(),
            entity_51_record: self.entity_51_record.clone(),
            attribute_class_use: self.attribute_class_use.clone(),
            definition_xmt: self.definition_xmt,
            attribute_definition: self.attribute_definition.clone(),
            stream_ordinal: self.stream_ordinal,
            inflated_offset: self.inflated_offset,
        }
    }
}

#[derive(Serialize)]
struct ParasolidTopologyAttributeClassUseRef<'a> {
    id: &'a str,
    topology_attribute_reference: &'a str,
    entity_51_record: &'a str,
    attribute_class_use: &'a str,
    definition_xmt: NonNullXmt,
    attribute_definition: &'a str,
}

impl Serialize for ParasolidTopologyAttributeClassUse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ParasolidTopologyAttributeClassUseRef {
            id: &self.id,
            topology_attribute_reference: &self.topology_attribute_reference,
            entity_51_record: &self.entity_51_record,
            attribute_class_use: &self.attribute_class_use,
            definition_xmt: self.definition_xmt,
            attribute_definition: &self.attribute_definition,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
#[derive(Serialize, Deserialize)]
struct ParasolidTopologyAttributeClassUseWire {
    id: String,
    topology_attribute_reference: String,
    entity_51_record: String,
    attribute_class_use: String,
    definition_xmt: NonNullXmt,
    attribute_definition: String,
}

#[cfg(test)]
impl From<ParasolidTopologyAttributeClassUse> for ParasolidTopologyAttributeClassUseWire {
    fn from(value: ParasolidTopologyAttributeClassUse) -> Self {
        Self {
            id: value.id,
            topology_attribute_reference: value.topology_attribute_reference,
            entity_51_record: value.entity_51_record,
            attribute_class_use: value.attribute_class_use,
            definition_xmt: value.definition_xmt,
            attribute_definition: value.attribute_definition,
        }
    }
}

/// Retain named attribute-class declarations from all Parasolid streams.
pub(super) fn parasolid_attribute_definitions(
    streams: &[Stream],
) -> Result<Vec<ParasolidAttributeDefinition>, CodecError> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind().is_parasolid() || stream.kind() == StreamKind::Deltas {
            continue;
        }
        let ordinal = u32::try_from(stream_ordinal)
            .map_err(|_| CodecError::malformed("NX stream ordinal exceeds u32"))?;
        for definition in crate::parasolid::attribute_definitions(&stream.inflated) {
            let name = definition.name.as_str().to_owned();
            let name = crate::printable_string::PrintableString::new(name)
                .map_err(|message| CodecError::Malformed(message.into()))?;
            let id = parasolid_record_id(
                stream_ordinal,
                "attribute-definition",
                u32::from(definition.xmt),
            );
            records.push(ParasolidAttributeDefinition {
                id,
                stream_ordinal: ordinal,
                xmt: definition.xmt,
                next_definition_xmt: definition.next_definition_xmt,
                identifier_xmt: definition.identifier_xmt,
                identifier_inflated_offset: cadmpeg_core::decode::u64_from_index(
                    definition.identifier_offset,
                ),
                name,
                type_id: definition.type_id,
                action_codes: definition.action_codes,
                field_names_xmt: definition.field_names_xmt,
                legal_owner_flags: definition.legal_owner_flags,
                field_codes: definition.field_codes,
                inflated_offset: cadmpeg_core::decode::u64_from_index(definition.offset),
            });
        }
    }
    Ok(records)
}

/// Decode every counted type-99 attribute field-name record.
pub(super) fn parasolid_field_names_records(
    streams: &[Stream],
) -> Result<Vec<ParasolidFieldNamesRecord>, CodecError> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind().is_parasolid() || stream.kind() == StreamKind::Deltas {
            continue;
        }
        let ordinal = u32::try_from(stream_ordinal)
            .map_err(|_| CodecError::malformed("NX stream ordinal exceeds u32"))?;
        let scanned = crate::parasolid::field_names_records(&stream.inflated);
        for record in scanned {
            let id = parasolid_offset_record_id(
                stream_ordinal,
                "field-names",
                u32::from(record.xmt),
                record.offset,
            );
            records.push(ParasolidFieldNamesRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                name_xmts: record.name_xmts,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
    }
    records.sort_by(|first, second| first.id.cmp(&second.id));
    Ok(records)
}

/// Resolve complete type-80 field-name lists through type-99 and character records.
pub(super) fn parasolid_attribute_field_names(
    definitions: &[ParasolidAttributeDefinition],
    field_names: &[ParasolidFieldNamesRecord],
    strings: &[ParasolidEntity54StringRecord],
    unicode: &[ParasolidEntity62UnicodeRecord],
) -> Result<Vec<ParasolidAttributeFieldNames>, CodecError> {
    let mut definitions_by_identity =
        BTreeMap::<(u32, u32), Option<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        insert_unique_value(
            &mut definitions_by_identity,
            (definition.stream_ordinal, u32::from(definition.xmt)),
            definition,
        );
    }
    let mut lists = BTreeMap::<(u32, u32), Option<&ParasolidFieldNamesRecord>>::new();
    for list in field_names {
        insert_unique_value(&mut lists, (list.stream_ordinal, u32::from(list.xmt)), list);
    }
    let mut names_by_xmt = BTreeMap::<(u32, u32), Option<(&str, &str)>>::new();
    for string in strings {
        insert_unique_value(
            &mut names_by_xmt,
            (string.stream_ordinal, u32::from(string.xmt)),
            (string.id.as_str(), string.value.as_str()),
        );
    }
    for value in unicode {
        insert_unique_value(
            &mut names_by_xmt,
            (value.stream_ordinal, u32::from(value.xmt)),
            (value.id.as_str(), value.value.as_str()),
        );
    }
    let mut relations = Vec::new();
    for definition in definitions_by_identity.values().filter_map(|value| *value) {
        let Some(field_names_xmt) = definition.field_names_xmt else {
            continue;
        };
        let Some(Some(list)) = lists.get(&(definition.stream_ordinal, u32::from(field_names_xmt)))
        else {
            continue;
        };
        if list.name_xmts.as_slice().len() != definition.field_codes.len() {
            continue;
        }

        if !list.name_xmts.as_slice().iter().all(|xmt| {
            matches!(
                names_by_xmt.get(&(definition.stream_ordinal, u32::from(*xmt))),
                Some(Some(_))
            )
        }) {
            continue;
        }
        let mut resolved = Vec::new();
        for xmt in list.name_xmts.as_slice() {
            let Some(Some((value_record, name))) =
                names_by_xmt.get(&(definition.stream_ordinal, u32::from(*xmt)))
            else {
                continue;
            };

            resolved.push(NamedField {
                value_record: (*value_record).to_owned(),
                name: (*name).to_owned(),
            });
        }

        let ordinal = usize::try_from(definition.stream_ordinal)
            .map_err(|_| CodecError::malformed("NX stream ordinal exceeds u32"))?;
        relations.push(ParasolidAttributeFieldNames {
            id: parasolid_record_id(ordinal, "attribute-field-names", u32::from(definition.xmt)),
            stream_ordinal: definition.stream_ordinal,
            attribute_definition: definition.id.clone(),
            field_names_record: list.id.clone(),
            fields: resolved,
        });
    }
    relations.sort_by(|first, second| first.id.cmp(&second.id));
    Ok(relations)
}

impl ParasolidStreamRecords for ParasolidBlendSurfaceRecord {
    type Row = crate::topology::BlendSurface;
    type Record = ParasolidBlendSurfaceRecord;
    const ID_STEM: &'static str = "blend-surface-record";
    fn rows<'view>(view: &'view StreamView<'_>) -> &'view [Self::Row] {
        &view.blend_surfaces
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidBlendSurfaceRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            state: row.state,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Retain every non-null topology-to-attribute-list reference.
pub(super) fn parasolid_topology_attribute_list_references(
    parsed: &ParsedStreams,
    entity_records: &[ParasolidEntity51Record],
) -> Result<Vec<ParasolidTopologyAttributeListReference>, CodecError> {
    let mut records_by_identity = BTreeMap::<(u32, u32), Option<&str>>::new();
    for record in entity_records {
        insert_unique_value(
            &mut records_by_identity,
            (record.stream_ordinal, u32::from(record.xmt)),
            record.id.as_str(),
        );
    }
    let mut references = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        let graph = &stream.graph;
        for topology_type in TopologyAttributeKind::ALL {
            for node in graph.of_kind(topology_type.node_kind()) {
                let attribute_list_xmt = match topology_type {
                    TopologyAttributeKind::Shell => node
                        .shell_fields()
                        .and_then(|fields| fields.attributes.map(u32::from)),
                    TopologyAttributeKind::Face => node
                        .face_fields()
                        .and_then(|fields| fields.attributes.map(u32::from)),
                    TopologyAttributeKind::Loop => node
                        .loop_fields()
                        .and_then(|fields| fields.attributes.map(u32::from)),
                    TopologyAttributeKind::Edge => node
                        .edge_fields()
                        .and_then(|fields| fields.attributes.map(u32::from)),
                    TopologyAttributeKind::Fin => node
                        .fin_fields()
                        .and_then(|fields| fields.attributes.map(u32::from)),
                    TopologyAttributeKind::Vertex => node
                        .vertex_fields()
                        .and_then(|fields| fields.attributes.map(u32::from)),
                };
                let Some(attribute_list_xmt) = attribute_list_xmt.filter(|value| *value > 1) else {
                    continue;
                };
                let Some(inflated_offset) = node.attribute_field_offset() else {
                    continue;
                };
                let ordinal = u32::try_from(stream_ordinal)
                    .map_err(|_| CodecError::malformed("NX stream ordinal exceeds u32"))?;

                let id = format!(
                    "nx:s{stream_ordinal}:topology-attribute-list-reference#{}-{}",
                    topology_type.code(),
                    node.xmt()
                );
                let attribute_list_record = records_by_identity
                    .get(&(ordinal, attribute_list_xmt))
                    .and_then(|record| *record)
                    .map(str::to_owned);
                references.push(ParasolidTopologyAttributeListReference {
                    id,
                    stream_ordinal: ordinal,
                    topology_type,
                    topology_xmt: node.xmt(),
                    attribute_list_xmt,
                    attribute_list_record,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(inflated_offset),
                });
            }
        }
    }
    Ok(references)
}

/// Decode every framed type-81 entity/attribute-list record.
pub(super) fn parasolid_entity_51_records(
    streams: &[Stream],
) -> Result<Vec<ParasolidEntity51Record>, CodecError> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind().is_parasolid() || stream.kind() == StreamKind::Deltas {
            continue;
        }
        let ordinal = u32::try_from(stream_ordinal)
            .map_err(|_| CodecError::malformed("NX stream ordinal exceeds u32"))?;
        let scanned = crate::parasolid::entity_51_records(&stream.inflated);
        for record in scanned {
            let id = parasolid_offset_record_id(
                stream_ordinal,
                "entity-51",
                u32::from(record.xmt),
                record.offset,
            );
            records.push(ParasolidEntity51Record {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                sequence: record.sequence,
                definition_xmt: record.definition_xmt,
                leading_references: record.leading_references,
                trailing_references: record.trailing_references,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
    }
    records.sort_by(|first, second| first.id.cmp(&second.id));
    Ok(records)
}

pub(super) fn parasolid_entity_value_records(
    streams: &[Stream],
) -> Result<ParasolidEntityValueRecords, CodecError> {
    let mut records = ParasolidEntityValueRecords {
        integers: Vec::new(),
        doubles: Vec::new(),
        strings: Vec::new(),
        vectors: Vec::new(),
        axes: Vec::new(),
        tags: Vec::new(),
        unicode: Vec::new(),
    };
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        let ordinal = u32::try_from(stream_ordinal)
            .map_err(|_| CodecError::malformed("NX stream ordinal exceeds u32"))?;
        if !matches!(stream.kind(), StreamKind::Partition | StreamKind::Plain) {
            continue;
        }
        let owned_offsets = crate::parasolid::referenced_value_record_offsets(&stream.inflated);
        let values = crate::parasolid::value_records::entity_value_records_at(
            &stream.inflated,
            owned_offsets,
        )?;
        for record in values.integers {
            let id = parasolid_offset_record_id(
                stream_ordinal,
                "entity-52-integers",
                u32::from(record.xmt),
                record.offset,
            );
            records.integers.push(ParasolidEntity52IntegerRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                values: record.value,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
        for record in values.doubles {
            let id = parasolid_offset_record_id(
                stream_ordinal,
                "entity-53-doubles",
                u32::from(record.xmt),
                record.offset,
            );
            records.doubles.push(ParasolidEntity53DoubleRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                values: record.value,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
        for record in values.strings {
            let id = parasolid_offset_record_id(
                stream_ordinal,
                "entity-54-string",
                u32::from(record.xmt),
                record.offset,
            );
            let text = record.value.as_str().to_owned();
            let value = crate::printable_string::PrintableString::new(text)
                .map_err(|message| CodecError::Malformed(message.into()))?;
            records.strings.push(ParasolidEntity54StringRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                value,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
        let mut retain_vector = |kind, family: &'static str, xmt, offset, byte_len, values| {
            let id = parasolid_offset_record_id(stream_ordinal, family, u32::from(xmt), offset);
            records.vectors.push(ParasolidEntityVectorRecord {
                id,
                stream_ordinal: ordinal,
                kind,
                xmt,
                values,
                byte_len: cadmpeg_core::decode::u64_from_index(byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(offset),
            });
        };
        for record in values.points {
            retain_vector(
                ParasolidVectorValueKind::Points,
                "entity-55-points",
                record.xmt,
                record.offset,
                record.byte_len,
                record.value,
            );
        }
        for record in values.vectors {
            retain_vector(
                ParasolidVectorValueKind::Vectors,
                "entity-56-vectors",
                record.xmt,
                record.offset,
                record.byte_len,
                record.value,
            );
        }
        for record in values.directions {
            retain_vector(
                ParasolidVectorValueKind::Directions,
                "entity-59-directions",
                record.xmt,
                record.offset,
                record.byte_len,
                record.value,
            );
        }
        for record in values.axes {
            let id = parasolid_offset_record_id(
                stream_ordinal,
                "entity-57-axes",
                u32::from(record.xmt),
                record.offset,
            );
            records.axes.push(ParasolidEntity57AxisRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                values: record.value,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
        for record in values.tags {
            let id = parasolid_offset_record_id(
                stream_ordinal,
                "entity-58-tags",
                u32::from(record.xmt),
                record.offset,
            );
            records.tags.push(ParasolidEntity58TagRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                values: record.value,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
        for record in values.unicode {
            let id = parasolid_offset_record_id(
                stream_ordinal,
                "entity-62-unicode",
                u32::from(record.xmt),
                record.offset,
            );
            records.unicode.push(ParasolidEntity62UnicodeRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                value: record.value,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
    }
    records
        .integers
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .doubles
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .strings
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .vectors
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .axes
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .tags
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .unicode
        .sort_by(|first, second| first.id.cmp(&second.id));
    Ok(records)
}

/// Join type-81 reference slots to unique same-stream numeric value records.
fn insert_unique_value<T: Copy>(
    values: &mut BTreeMap<(u32, u32), Option<T>>,
    key: (u32, u32),
    value: T,
) {
    match values.entry(key) {
        std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() = None,
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(Some(value));
        }
    }
}

fn entity_51_use_id(
    stem: &'static str,
    entity: &ParasolidEntity51Record,
    reference_ordinal: u32,
) -> String {
    format!(
        "nx:s{}:{stem}#{}-{}-{reference_ordinal}",
        entity.stream_ordinal,
        u32::from(entity.xmt),
        entity.inflated_offset
    )
}

pub(super) fn parasolid_entity_51_numeric_uses(
    entities: &[ParasolidEntity51Record],
    integers: &[ParasolidEntity52IntegerRecord],
    doubles: &[ParasolidEntity53DoubleRecord],
) -> Vec<ParasolidEntity51NumericUse> {
    let mut values =
        BTreeMap::<(u32, u32), Option<(ParasolidEntity51NumericKind, NonNullXmt, &str)>>::new();
    for record in integers {
        insert_unique_value(
            &mut values,
            (record.stream_ordinal, u32::from(record.xmt)),
            (
                ParasolidEntity51NumericKind::UnsignedIntegers,
                record.xmt,
                &record.id,
            ),
        );
    }
    for record in doubles {
        insert_unique_value(
            &mut values,
            (record.stream_ordinal, u32::from(record.xmt)),
            (
                ParasolidEntity51NumericKind::Doubles,
                record.xmt,
                &record.id,
            ),
        );
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (position, &referenced_xmt) in entity.trailing_references.fields() {
            let reference_ordinal = position.reference_ordinal();
            let Some(Some((kind, target_xmt, value_record))) =
                values.get(&(entity.stream_ordinal, referenced_xmt))
            else {
                continue;
            };
            uses.push(ParasolidEntity51NumericUse {
                id: entity_51_use_id("entity-51-numeric-use", entity, reference_ordinal),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                position,
                referenced_xmt: *target_xmt,
                kind: *kind,
                value_record: (*value_record).to_owned(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|left, right| left.id.cmp(&right.id));
    uses
}

/// Join type-81 reference slots to unique same-stream type-84 strings.
pub(super) fn parasolid_entity_51_string_uses(
    entities: &[ParasolidEntity51Record],
    strings: &[ParasolidEntity54StringRecord],
) -> Vec<ParasolidEntity51StringUse> {
    let mut strings_by_identity =
        BTreeMap::<(u32, u32), Option<&ParasolidEntity54StringRecord>>::new();
    for string in strings {
        insert_unique_value(
            &mut strings_by_identity,
            (string.stream_ordinal, u32::from(string.xmt)),
            string,
        );
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (position, &referenced_xmt) in entity.trailing_references.fields() {
            let reference_ordinal = position.reference_ordinal();
            let Some(Some(string)) =
                strings_by_identity.get(&(entity.stream_ordinal, referenced_xmt))
            else {
                continue;
            };
            uses.push(ParasolidEntity51StringUse {
                id: entity_51_use_id("entity-51-string-use", entity, reference_ordinal),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                position,
                referenced_xmt: string.xmt,
                string_record: string.id.clone(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|left, right| left.id.cmp(&right.id));
    uses
}

/// Join type-81 reference slots to unique same-stream structured value records.
pub(super) fn parasolid_entity_51_structured_uses(
    entities: &[ParasolidEntity51Record],
    vectors: &[ParasolidEntityVectorRecord],
    axes: &[ParasolidEntity57AxisRecord],
    tags: &[ParasolidEntity58TagRecord],
    unicode: &[ParasolidEntity62UnicodeRecord],
) -> Vec<ParasolidEntity51StructuredUse> {
    let mut values = BTreeMap::<(u32, u32), Option<(StructuredValueKind, NonNullXmt, &str)>>::new();
    for record in vectors {
        let kind = match record.kind {
            ParasolidVectorValueKind::Points => StructuredValueKind::Points,
            ParasolidVectorValueKind::Vectors => StructuredValueKind::Vectors,
            ParasolidVectorValueKind::Directions => StructuredValueKind::Directions,
        };
        insert_unique_value(
            &mut values,
            (record.stream_ordinal, u32::from(record.xmt)),
            (kind, record.xmt, record.id.as_str()),
        );
    }
    for (kind, stream_ordinal, xmt, id) in axes
        .iter()
        .map(|record| {
            (
                StructuredValueKind::Axes,
                record.stream_ordinal,
                record.xmt,
                record.id.as_str(),
            )
        })
        .chain(tags.iter().map(|record| {
            (
                StructuredValueKind::Tags,
                record.stream_ordinal,
                record.xmt,
                record.id.as_str(),
            )
        }))
        .chain(unicode.iter().map(|record| {
            (
                StructuredValueKind::Unicode,
                record.stream_ordinal,
                record.xmt,
                record.id.as_str(),
            )
        }))
    {
        insert_unique_value(
            &mut values,
            (stream_ordinal, u32::from(xmt)),
            (kind, xmt, id),
        );
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (position, &referenced_xmt) in entity.trailing_references.fields() {
            let reference_ordinal = position.reference_ordinal();
            let Some(Some((kind, target_xmt, value_record))) =
                values.get(&(entity.stream_ordinal, referenced_xmt))
            else {
                continue;
            };
            uses.push(ParasolidEntity51StructuredUse {
                id: entity_51_use_id("entity-51-structured-use", entity, reference_ordinal),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                position,
                referenced_xmt: *target_xmt,
                kind: *kind,
                value_record: (*value_record).to_owned(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|left, right| left.id.cmp(&right.id));
    uses
}

/// Resolve topology-owned attribute instances through their type-80 definition.
pub(super) fn parasolid_topology_attribute_class_uses(
    topology_references: &[ParasolidTopologyAttributeListReference],
    entity_records: &[ParasolidEntity51Record],
    class_uses: &[ParasolidAttributeClassUse],
) -> Vec<ParasolidTopologyAttributeClassUse> {
    let mut records_by_identity = BTreeMap::<(u32, u32), Option<&ParasolidEntity51Record>>::new();
    for record in entity_records {
        insert_unique_value(
            &mut records_by_identity,
            (record.stream_ordinal, u32::from(record.xmt)),
            record,
        );
    }
    let mut records_by_owner = BTreeMap::<(u32, u32), Vec<&ParasolidEntity51Record>>::new();
    for record in entity_records {
        let owner_xmt = record.leading_references[0];
        if owner_xmt > 1 {
            let owner_key = (record.stream_ordinal, owner_xmt);

            let members = match records_by_owner.entry(owner_key) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Vec::new()),
            };
            members.push(record);
        }
    }
    let mut class_uses_by_entity = BTreeMap::<&str, Option<&ParasolidAttributeClassUse>>::new();
    for class_use in class_uses {
        let class_key = class_use.entity_51_record.as_str();

        match class_uses_by_entity.entry(class_key) {
            std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() = None,
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(class_use));
            }
        }
    }
    let mut uses = Vec::new();
    for reference in topology_references {
        let Some(entity_id) = reference.attribute_list_record.as_deref() else {
            continue;
        };
        let Some(Some(head)) =
            records_by_identity.get(&(reference.stream_ordinal, reference.attribute_list_xmt))
        else {
            continue;
        };
        if head.id != entity_id || head.leading_references[0] != reference.topology_xmt {
            continue;
        }

        let Some(members) =
            records_by_owner.get(&(reference.stream_ordinal, reference.topology_xmt))
        else {
            continue;
        };

        let mut member_xmt_counts = BTreeMap::<u32, usize>::new();
        for member in members {
            *member_xmt_counts.entry(u32::from(member.xmt)).or_default() += 1;
        }
        for member in members {
            let Some(Some(class_use)) = class_uses_by_entity.get(member.id.as_str()) else {
                continue;
            };
            let suffix = member.id != head.id;
            let offset_suffix = suffix && member_xmt_counts.get(&u32::from(member.xmt)) != Some(&1);
            let mut id = format!(
                "nx:s{}:topology-attribute-class-use#{}-{}",
                reference.stream_ordinal,
                reference.topology_type.code(),
                reference.topology_xmt
            );
            if suffix {
                id = format!("{id}-{}", u32::from(member.xmt));
            }
            if offset_suffix {
                id = format!("{id}-{}", member.inflated_offset);
            }
            uses.push(ParasolidTopologyAttributeClassUse {
                stream_ordinal: reference.stream_ordinal,
                inflated_offset: member.inflated_offset,
                id,
                topology_attribute_reference: reference.id.clone(),
                entity_51_record: class_use.entity_51_record.clone(),
                attribute_class_use: class_use.id.clone(),
                definition_xmt: class_use.definition_xmt,
                attribute_definition: class_use.attribute_definition.clone(),
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Resolve every type-81 attribute instance through its type-80 definition reference.
pub(super) fn parasolid_attribute_class_uses(
    entities: &[ParasolidEntity51Record],
    definitions: &[ParasolidAttributeDefinition],
) -> Vec<ParasolidAttributeClassUse> {
    let mut definitions_by_identity =
        BTreeMap::<(u32, u32), Option<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        insert_unique_value(
            &mut definitions_by_identity,
            (definition.stream_ordinal, u32::from(definition.xmt)),
            definition,
        );
    }
    let mut uses = Vec::new();
    for entity in entities {
        let Some(Some(definition)) =
            definitions_by_identity.get(&(entity.stream_ordinal, entity.definition_xmt))
        else {
            continue;
        };

        let id = format!(
            "nx:s{}:attribute-class-use#{}-{}",
            entity.stream_ordinal,
            u32::from(entity.xmt),
            entity.inflated_offset
        );
        uses.push(ParasolidAttributeClassUse {
            inflated_offset: entity.inflated_offset,
            id,
            stream_ordinal: entity.stream_ordinal,
            entity_51_record: entity.id.clone(),
            definition_xmt: definition.xmt,
            attribute_definition: definition.id.clone(),
        });
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Assign uniquely resolved attribute values to declared type-80 fields.
pub(super) fn parasolid_attribute_field_uses(
    class_uses: &[ParasolidAttributeClassUse],
    definitions: &[ParasolidAttributeDefinition],
    numeric_uses: &[ParasolidEntity51NumericUse],
    string_uses: &[ParasolidEntity51StringUse],
    structured_uses: &[ParasolidEntity51StructuredUse],
) -> Vec<ParasolidAttributeFieldUse> {
    let mut classes = BTreeMap::<&str, Vec<&ParasolidAttributeClassUse>>::new();
    for class_use in class_uses {
        classes
            .entry(class_use.entity_51_record.as_str())
            .or_default()
            .push(class_use);
    }
    let mut definitions_by_id = BTreeMap::<&str, Vec<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        definitions_by_id
            .entry(definition.id.as_str())
            .or_default()
            .push(definition);
    }
    let mut candidates = BTreeMap::<(&str, FieldPosition), Vec<_>>::new();
    let mut push_candidate = |key, candidate| {
        candidates.entry(key).or_default().push(candidate);
    };
    for numeric_use in numeric_uses {
        let value_kind = match numeric_use.kind {
            ParasolidEntity51NumericKind::UnsignedIntegers => {
                ParasolidAttributeFieldValueKind::UnsignedIntegers
            }
            ParasolidEntity51NumericKind::Doubles => ParasolidAttributeFieldValueKind::Doubles,
        };
        push_candidate(
            (numeric_use.entity_51_record.as_str(), numeric_use.position),
            (
                numeric_use.stream_ordinal,
                value_kind,
                numeric_use.id.as_str(),
                numeric_use.value_record.as_str(),
                numeric_use.inflated_offset,
            ),
        );
    }
    for string_use in string_uses {
        push_candidate(
            (string_use.entity_51_record.as_str(), string_use.position),
            (
                string_use.stream_ordinal,
                ParasolidAttributeFieldValueKind::String,
                string_use.id.as_str(),
                string_use.string_record.as_str(),
                string_use.inflated_offset,
            ),
        );
    }
    for structured_use in structured_uses {
        push_candidate(
            (
                structured_use.entity_51_record.as_str(),
                structured_use.position,
            ),
            (
                structured_use.stream_ordinal,
                structured_use.kind.into(),
                structured_use.id.as_str(),
                structured_use.value_record.as_str(),
                structured_use.inflated_offset,
            ),
        );
    }
    let mut uses = Vec::new();
    for ((entity_51_record, position), candidates) in candidates {
        let matched = (|| {
            let [(stream_ordinal, value_kind, value_use, value_record, inflated_offset)] =
                candidates.as_slice()
            else {
                return None;
            };
            let [class_use] = classes.get(entity_51_record)?.as_slice() else {
                return None;
            };
            if class_use.stream_ordinal != *stream_ordinal {
                return None;
            }
            let [definition] = definitions_by_id
                .get(class_use.attribute_definition.as_str())?
                .as_slice()
            else {
                return None;
            };
            let field_ordinal = position.field_ordinal();
            let field_code = *definition
                .field_codes
                .get(cadmpeg_core::decode::index_from_u32(field_ordinal))?;
            (field_code == value_kind.field_code()).then_some(())?;
            let (_, class_key) = class_use.id.rsplit_once('#')?;
            Some((
                *stream_ordinal,
                *value_kind,
                *value_use,
                *value_record,
                *inflated_offset,
                class_use,
                class_key,
                field_ordinal,
            ))
        })();
        let Some((
            stream_ordinal,
            value_kind,
            value_use,
            value_record,
            inflated_offset,
            class_use,
            class_key,
            field_ordinal,
        )) = matched
        else {
            continue;
        };
        let id = format!("nx:s{stream_ordinal}:attribute-field-use#{class_key}-{field_ordinal}");

        uses.push(ParasolidAttributeFieldUse {
            id,
            stream_ordinal,
            attribute_class_use: class_use.id.clone(),
            entity_51_record: entity_51_record.to_owned(),
            attribute_definition: class_use.attribute_definition.clone(),
            position,
            value_kind,
            value_use: value_use.to_owned(),
            value_record: value_record.to_owned(),
            inflated_offset,
        });
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Whether a concrete topology-owned attribute field lacks its exact value relation.
pub(super) fn parasolid_topology_attribute_fields_have_untransferred_values(
    definitions: &[ParasolidAttributeDefinition],
    entities: &[ParasolidEntity51Record],
    field_uses: &[ParasolidAttributeFieldUse],
    topology_class_uses: &[ParasolidTopologyAttributeClassUse],
) -> bool {
    let mut definitions_by_id = BTreeMap::<&str, Option<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        let definition_key = definition.id.as_str();

        match definitions_by_id.entry(definition_key) {
            std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() = None,
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(definition));
            }
        }
    }
    let mut fields_by_identity =
        BTreeMap::<(&str, u32), Option<&ParasolidAttributeFieldUse>>::new();
    for field_use in field_uses {
        let key = (
            field_use.entity_51_record.as_str(),
            field_use.position.field_ordinal(),
        );

        match fields_by_identity.entry(key) {
            std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() = None,
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(field_use));
            }
        }
    }

    let mut entities_by_id = BTreeMap::<&str, Option<&ParasolidEntity51Record>>::new();
    for entity in entities {
        let entity_key = entity.id.as_str();

        match entities_by_id.entry(entity_key) {
            std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() = None,
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(entity));
            }
        }
    }

    topology_class_uses.iter().any(|topology_class_use| {
        let entity_id = topology_class_use.entity_51_record.as_str();
        let Some(Some(entity)) = entities_by_id.get(entity_id) else {
            return true;
        };
        let Some(Some(definition)) =
            definitions_by_id.get(topology_class_use.attribute_definition.as_str())
        else {
            return true;
        };
        definition
            .field_codes
            .iter()
            .enumerate()
            .any(|(field_ordinal, field_code)| {
                // Field code 0 is ignored. Pointer fields (code 9) are always
                // transmitted empty and therefore have no value relation.
                if matches!(
                    field_code,
                    AttributeField::Ignored | AttributeField::Pointer
                ) {
                    return false;
                }
                let Some(&referenced_xmt) = entity.trailing_references.values().get(field_ordinal)
                else {
                    return true;
                };
                if referenced_xmt == 1 {
                    return false;
                }
                let Ok(field_ordinal) = u32::try_from(field_ordinal) else {
                    return true;
                };
                !matches!(
                    fields_by_identity.get(&(entity.id.as_str(), field_ordinal)),
                    Some(Some(_))
                )
            })
    })
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_region_type,
    crate::deltas::region::RegionType,
    "region_type"
);
cadmpeg_core::named_optional_field!(deserialize_group_selector, GroupSelector, "group_selector");
cadmpeg_core::named_optional_field!(
    deserialize_group_linked_reference_status,
    GroupReferenceStatus,
    "group_linked_reference_status"
);
cadmpeg_core::named_optional_field!(
    deserialize_attribute_list_record,
    String,
    "attribute_list_record"
);
