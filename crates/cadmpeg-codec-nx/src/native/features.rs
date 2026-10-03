// SPDX-License-Identifier: Apache-2.0
//! Feature-history record extractors and their record types.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::container::Container;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) mod construction_records;
use construction_records::{charged_unique_offset_data_block, replace_operation_text};

pub(super) mod delete;
pub(super) mod draft;
pub(super) mod extrude_32;
pub(super) mod fset;
use self::fset::FeatureFsetReferenceGroup;
pub(super) mod holes;
pub(super) mod pattern;
pub(super) mod payload_name;
pub(super) mod point_scalar_lane;
use payload_name::FeaturePayloadName;

mod reference;
use crate::om::column_row::ColumnRowSlot;
use crate::om::datum_csys::DatumCsysSlot;
use crate::om::header_references::HeaderSlot;
use reference::ConstructionReference;

pub(super) mod block_reference;
mod body_reference_wire;
pub(super) mod body_scalar_triple;
mod body_write_wire;
mod borrowed_wires;
mod common_frame_wire;
mod datum_plane_wire;
pub(super) mod object_frame;
pub(super) mod operation_record;
pub(super) mod surface_branches;
pub(super) mod swp104_branch;
pub(super) mod terminal_discriminator;
pub(super) mod thru_curve_branches;
pub(super) mod unlabeled_record;
use crate::native::om::column_row::{
    DataBlockIndexRow, DataBlockLinkedIndexRow, DataBlockTargetIndexRow,
};
use crate::native::om::journal_group::OmOperationStateJournalGroup;
use crate::native::om::{
    data_blocks, DataBlockColumnIndexTable, DataBlockReference, DataBlockRole, OmSchemaRole,
    ParameterFormula,
};
use crate::native::segments::{segment_om_links, SegmentBodyBinding, SegmentOmLink};
use crate::om::binary64_pair::{
    Binary64Pair, Binary64PairForm, DatumPlanePairForm, ObjectPairForm, SketchBinary64PairForm,
};
use crate::om::compact::CompactIndexAtom;
use crate::om::compact::CountedIndexMembers;
use crate::om::compact::LocatedCompactIndex;
use crate::om::fixed::Q155;
use crate::om::nonempty::NonEmpty;
use crate::om::reference_index::PayloadIndexToken;

use crate::om::scalar::ShiftedBinary64;
use crate::om::scalar::ShiftedScalar;
use crate::om::scalar_pair::{DatumPairForm, MixedPairForm, PairPosition, SketchPairForm};
use crate::om::scalar_run::FramedScalarRun;
use crate::om::sketch_scalar::{SketchMixedScalars, SketchScalarLaneForm, SketchScaledAtom};
use crate::printable_string::PrintableString;
use object_frame::DataBlockObjectFrame;
use operation_record::{FeatureOperationRecord, OperationRecordSpan};
use std::borrow::Cow;
use std::num::NonZeroU8;
use unlabeled_record::FeatureUnlabeledOperationRecord;
mod pair_wire;
use crate::om::csys_descriptor::{
    CsysDescriptor, CsysDescriptorSlot, CsysIdentity, LocatedCsysDescriptor,
};

use crate::om::plane_descriptor::PlaneDescriptor;
use crate::om::thru_curve_controls::ThruCurveControls;

pub(super) mod datum_plane_header;
mod joined_payload;
mod payload_content;
use datum_plane_header::FeatureDatumPlaneHeader;
use joined_payload::JoinedPayload;
use payload_content::{FeaturePayloadBlock, FeaturePayloadContent};

/// Ordered feature operation label from a feature-history record area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureOperationLabel {
    /// Globally unique label identity.
    pub(super) id: String,
    /// Link identifying the owning ordered OM section.
    pub(super) section_link: String,
    /// Zero-based order within the record area.
    pub(super) ordinal: u32,
    /// Exact printable operation name.
    pub(super) value: String,
    /// Four nullable references with their exact source encodings.
    #[serde(flatten)]
    pub(super) objects: crate::om::header_references::HeaderReferences,
    /// Record-order-independent header identity when every non-null slot
    /// resolves to a unique content-backed offset-store block and the tuple
    /// is unique across the feature-history sections.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stable_identity"
    )]
    pub(super) stable_identity: Option<String>,
    /// Absolute file offset of the `03` label tag.
    pub(super) source_offset: u64,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureOperationLabel {
    fn decode_cost(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, operation: &'static str) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(&(&self.section_link,self.source_offset), ctx, operation)
    }
}

/// Return operation labels in neutral construction-history order.
///
/// Each feature-history section stores its operation records newest first. The
/// native label arena retains that source order, but neutral dependencies and
/// feature ordinals use oldest-first construction order within each section.
pub(super) fn feature_operation_chronological_labels<'a>(
    ctx: &DecodeContext<'_>,
    labels: &'a [FeatureOperationLabel],
) -> Result<Vec<&'a FeatureOperationLabel>, CodecError> {
    let mut sections = BTreeMap::<&str, u64>::new();
    let mut section_reservation = ctx.reserve_scoped(0, "NX feature label sections")?;
    for label in labels {
        let section_key = label.section_link.as_str();
        section_reservation.with_storage(|| {
            ctx.admit_btree_entry(&sections, &section_key, "NX feature label sections")
        })?;

        sections
            .entry(section_key)
            .and_modify(|first| *first = (*first).min(label.source_offset))
            .or_insert(label.source_offset);
    }
    let mut ordered = ctx.collection_vec(labels.len(), "NX chronological feature labels")?;
    ordered.extend(labels);
    ctx.stable_sort_by(
        &mut ordered,
            |value| value,
            |left, right| {
            sections
                .get(left.section_link.as_str())
                .cmp(&sections.get(right.section_link.as_str()))
                .then_with(|| left.section_link.cmp(&right.section_link))
                .then_with(|| right.source_offset.cmp(&left.source_offset))
        },
        "sort NX chronological feature labels",
    )?;
    Ok(ordered)
}

/// Exact body-write frame retained from one feature operation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "body_write_wire::BodyWriteWire")]
pub(super) struct FeatureOperationBodyWrite {
    pub(super) id: String,
    pub(super) operation_label: Option<String>,
    pub(super) operation_record: String,
    pub(super) ordinal: u32,
    pub(super) frame: crate::om::body_write::BodyWriteFrame<u64>,
    pub(super) body_image_data_block: Option<String>,
}

/// Exact bridge from a body-write image block to one plain cached-body stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureOperationBodyImageSegmentUse {
    /// Globally unique bridge identity.
    pub(super) id: String,
    /// Body-write frame owning both the body identity and image block.
    pub(super) operation_body_write: String,
    /// Unambiguous offset-store block containing the serialized body image.
    pub(super) body_image_data_block: String,
    /// Plain cached-body tuple whose alias equals the body identity.
    pub(super) segment_body_binding: String,
}

/// Exact persistent body-identity match to one plain cached-body stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureOperationBodyIdentitySegmentUse {
    /// Globally unique bridge identity.
    pub(super) id: String,
    /// Body-write frame carrying the persistent body identity.
    pub(super) operation_body_write: String,
    /// Persistent body identity shared by the frame and plain-stream alias.
    pub(super) body_identity: u8,
    /// Unique plain cached-body tuple with the equal alias.
    pub(super) segment_body_binding: String,
}

/// Exact owning-partition scope for one body-write image and its GROUP node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureOperationBodyPartitionUse {
    /// Globally unique partition-use identity.
    pub(super) id: String,
    /// Body-write frame carrying the partition-local GROUP node.
    pub(super) operation_body_write: String,
    /// Exact body-image relation that selects the cached-body stream.
    pub(super) body_image_segment_use: String,
    /// Plain cached-body binding inside the partition's body-history run.
    pub(super) segment_body_binding: String,
    /// Partition stream that terminates the complete cached-body run.
    pub(super) partition_stream_ordinal: u32,
    /// Partition-local GROUP node carried by the body-write frame.
    pub(super) group_node: u32,
    /// Ordered GROUP record updates retained inside the owning partition scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) parasolid_group_records: Vec<String>,
    /// Current ordered GROUP members resolved inside the owning partition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) parasolid_group_members: Vec<String>,
}

/// Exact partition ownership of one labeled or unlabeled body-write GROUP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureBodyWriteGroupPartitionUse {
    /// Globally unique partition-use identity.
    pub(super) id: String,
    /// Labeled or unlabeled body-write frame carrying the GROUP node.
    pub(super) body_write: String,
    /// Persistent body identity carried by the frame.
    pub(super) body_identity: u8,
    /// Partition-local GROUP node carried by the frame.
    pub(super) group_node: u32,
    /// Unique partition namespace containing every matched GROUP record.
    pub(super) partition_stream_ordinal: u32,
    /// Ordered GROUP record updates retained inside the partition scope.
    pub(super) parasolid_group_records: Vec<String>,
    /// Current ordered GROUP members resolved inside the partition scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) parasolid_group_members: Vec<String>,
}

/// Exact direct object-reference field retained from one feature operation.
///
/// The optional tag and object identity are native evidence. They do not assign a
/// body, operand, input, or output role.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureOperationObjectReferenceWire")]
pub(super) struct FeatureOperationObjectReference {
    /// Globally unique reference identity.
    pub(super) id: String,
    /// Owning operation-label identity.
    operation_label: String,
    /// Owning bounded operation-record identity.
    operation_record: String,
    /// Zero-based reference order within the operation payload.
    ordinal: u32,
    pub(super) frame: crate::om::direct_reference::DirectReferenceFrame<u64>,
    /// Unique target in the native offset-store data-block arena, when found.
    data_block: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct FeatureOperationObjectReferenceWire {
    /// Globally unique reference identity.
    id: String,
    /// Owning operation-label identity.
    operation_label: String,
    /// Owning bounded operation-record identity.
    operation_record: String,
    /// Zero-based reference order within the operation payload.
    ordinal: u32,
    /// Byte between the opening `01 02` marker and the object index.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tag"
    )]
    tag: Option<u8>,
    /// Referenced feature object index.
    object_index: u32,
    /// Exact serialized object-index token.
    raw_object_index: Vec<u8>,
    /// Unique target in the native offset-store data-block arena, when found.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_data_block"
    )]
    data_block: Option<String>,
    /// Absolute offset of the object-index token.
    object_index_source_offset: u64,
    /// Exact serialized field byte length.
    byte_len: u64,
    /// Absolute offset of the opening `01 02` marker.
    source_offset: u64,
}

#[cfg(test)]
impl From<FeatureOperationObjectReference> for FeatureOperationObjectReferenceWire {
    fn from(value: FeatureOperationObjectReference) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            operation_record: value.operation_record,
            ordinal: value.ordinal,
            tag: value.frame.kind().tag(),
            object_index: value.frame.object().value(),
            raw_object_index: value.frame.object().raw().to_vec(),
            data_block: value.data_block,
            object_index_source_offset: value.frame.object_offset(),
            byte_len: u64::from(value.frame.byte_len()),
            source_offset: value.frame.offset(),
        }
    }
}

impl TryFrom<FeatureOperationObjectReferenceWire> for FeatureOperationObjectReference {
    type Error = String;
    fn try_from(value: FeatureOperationObjectReferenceWire) -> Result<Self, Self::Error> {
        let object = crate::om::reference_index::CanonicalFeatureReferenceToken::from_wire(
            value.object_index,
            &value.raw_object_index,
        )
        .map_err(|error| format!("object_index/raw_object_index: {error}"))?;
        let kind = crate::om::direct_reference::ReferenceFieldKind::from_tag(value.tag)?;
        let frame = crate::om::direct_reference::DirectReferenceFrame::<u64>::new(
            kind,
            object,
            value.source_offset,
        )
        .ok_or("source_offset: direct reference field end overflows")?;
        if value.byte_len != u64::from(frame.byte_len()) {
            return Err("byte_len disagrees with direct reference field".into());
        }
        if value.object_index_source_offset != frame.object_offset() {
            return Err("object_index_source_offset disagrees with direct reference field".into());
        }
        Ok(Self {
            id: value.id,
            operation_label: value.operation_label,
            operation_record: value.operation_record,
            ordinal: value.ordinal,
            frame,
            data_block: value.data_block,
        })
    }
}

/// Exactly framed common record in one bounded feature operation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "common_frame_wire::CommonFrameWire")]
pub(super) struct FeatureOperationCommonFrame {
    pub(super) id: String,
    pub(super) operation_record: String,
    pub(super) ordinal: u32,
    pub(super) frame: crate::om::common_frame::CommonFrame<u64, Option<String>>,
}

/// Canonical terminal common-frame suffix of one feature operation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "common_frame_wire::TerminalFrameWire")]
pub(super) struct FeatureOperationTerminalFrame {
    pub(super) id: String,
    pub(super) operation_record: String,
    pub(super) immediate_common_frame: Option<String>,
    pub(super) frame: crate::om::common_frame::TerminalFrame<u64, Option<String>>,
}

/// Exact join from an operation terminal ordinal to its state-journal row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureOperationStateJournalUse {
    /// Globally unique operation-to-journal relation identity.
    pub(super) id: String,
    /// Owning feature-history section.
    section_link: String,
    /// Owning operation label.
    operation_label: String,
    /// Owning bounded operation record.
    operation_record: String,
    /// Terminal frame carrying the local ordinal.
    operation_terminal_frame: String,
    /// State-journal group containing the matching row.
    journal_group: String,
    /// Zero-based row order within the journal group.
    pub(super) journal_row_ordinal: u32,
    /// Ordinal shared by the operation terminal frame and the journal row.
    state_ordinal: u32,
    /// Absolute source offset of the operation terminal frame.
    pub(super) operation_source_offset: u64,
    /// Absolute source offset of the matching journal row.
    journal_source_offset: u64,
}

/// Ordered length-framed string from one bounded feature-operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeaturePayloadString {
    /// Globally unique string identity.
    pub(super) id: String,
    /// Owning exact feature-operation record.
    pub(super) operation_record: String,
    /// Zero-based string order within the post-label payload.
    ordinal: u32,
    /// Exact UTF-8 string value.
    pub(super) value: crate::payload_text::PayloadText<String>,
    /// Absolute file offset of the `04` marker.
    pub(super) source_offset: u64,
}

/// Primary selection or ordered body-reference field in one feature operation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureBodyReferenceWire")]
pub(super) struct FeatureBodyReference {
    /// Globally unique reference identity.
    pub(super) id: String,
    /// Owning operation-label identity.
    pub(super) operation_label: String,
    /// Zero-based field order; absent for the primary body selection.
    pub(super) ordinal: Option<u32>,
    /// Serialized reference index interpreted through its resolved namespace.
    pub(super) body: crate::om::reference_index::FeatureReferenceToken,
    /// Absolute file offset of the object-index token.
    pub(super) source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureBodyReferenceWire {
    /// Globally unique reference identity.
    id: String,
    /// Owning operation-label identity.
    operation_label: String,
    /// Zero-based field order; absent for the primary body selection.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_ordinal"
    )]
    ordinal: Option<u32>,
    /// Serialized reference index interpreted through its resolved namespace.
    body_object_index: u32,
    /// Exact serialized variable-width object-index token.
    raw_body_object_index: Vec<u8>,
    /// Absolute file offset of the object-index token.
    source_offset: u64,
}

#[cfg(test)]
impl From<FeatureBodyReference> for FeatureBodyReferenceWire {
    fn from(value: FeatureBodyReference) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            ordinal: value.ordinal,
            body_object_index: value.body.value(),
            raw_body_object_index: value.body.raw().to_vec(),
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeatureBodyReferenceWire> for FeatureBodyReference {
    type Error = String;
    fn try_from(value: FeatureBodyReferenceWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            operation_label: value.operation_label,
            ordinal: value.ordinal,
            body: crate::om::reference_index::FeatureReferenceToken::from_wire(
                value.body_object_index,
                &value.raw_body_object_index,
            )
            .map_err(|error| format!("body_object_index/raw_body_object_index: {error}"))?,
            source_offset: value.source_offset,
        })
    }
}

/// Unambiguous reuse of one segment body image by a primary feature body field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureBodySegmentUse {
    /// Globally unique use identity.
    pub(super) id: String,
    /// Primary field in the native `feature_body_references` arena.
    pub(super) feature_body_reference: String,
    /// Segment image in the native `segment_body_bindings` arena.
    pub(super) segment_body_binding: String,
}

/// Primary feature body field resolved in its operation's offset-store namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureBodyDataBlockUse {
    /// Globally unique use identity.
    pub(super) id: String,
    /// Primary field in the native `feature_body_references` arena.
    pub(super) feature_body_reference: String,
    /// Target in the native `data_blocks` arena.
    pub(super) data_block: String,
}

/// Operation-header input resolved to one bounded offset-only OM data block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureInputBlock {
    /// Globally unique input-binding identity.
    pub(super) id: String,
    /// Owning operation-label identity.
    pub(super) operation_label: String,
    /// Zero-based operation-header input slot.
    pub(super) input_slot: HeaderSlot,
    /// Required reference with its exact source encoding.
    #[serde(flatten)]
    pub(super) object: crate::om::reference_index::FeatureReferenceToken,
    /// Target in the native `data_blocks` arena.
    pub(super) data_block: String,
    /// Absolute file offset of the object-index token.
    pub(super) source_offset: u64,
}

/// Input-block bindings from distinct operations that resolve to one data block.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureInputBlockIdentityGroupWire")]
pub(super) struct FeatureInputBlockIdentityGroup {
    /// Globally unique group identity.
    pub(super) id: String,
    /// Shared target in the native `data_blocks` arena.
    data_block: String,
    /// Input bindings in ascending source-offset order.
    pub(super) members: Vec<FeatureInputBlockIdentityMember>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::native) struct FeatureInputBlockIdentityMember {
    pub(super) input_block: String,
    operation_label: String,
    input_slot: HeaderSlot,
    pub(super) source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureInputBlockIdentityGroupWire {
    id: String,
    data_block: String,
    input_blocks: Vec<String>,
    operation_labels: Vec<String>,
    input_slots: Vec<HeaderSlot>,
    source_offsets: Vec<u64>,
}

#[cfg(test)]
impl From<FeatureInputBlockIdentityGroup> for FeatureInputBlockIdentityGroupWire {
    fn from(group: FeatureInputBlockIdentityGroup) -> Self {
        Self {
            id: group.id,
            data_block: group.data_block,
            input_blocks: group
                .members
                .iter()
                .map(|member| member.input_block.clone())
                .collect(),
            operation_labels: group
                .members
                .iter()
                .map(|member| member.operation_label.clone())
                .collect(),
            input_slots: group
                .members
                .iter()
                .map(|member| member.input_slot)
                .collect(),
            source_offsets: group
                .members
                .iter()
                .map(|member| member.source_offset)
                .collect(),
        }
    }
}

impl TryFrom<FeatureInputBlockIdentityGroupWire> for FeatureInputBlockIdentityGroup {
    type Error = String;
    fn try_from(wire: FeatureInputBlockIdentityGroupWire) -> Result<Self, Self::Error> {
        let count = wire.input_blocks.len();
        if wire.operation_labels.len() != count
            || wire.input_slots.len() != count
            || wire.source_offsets.len() != count
        {
            return Err("input_blocks, operation_labels, input_slots, and source_offsets must have equal lengths".into());
        }
        Ok(Self {
            id: wire.id,
            data_block: wire.data_block,
            members: wire
                .input_blocks
                .into_iter()
                .zip(wire.operation_labels)
                .zip(wire.input_slots)
                .zip(wire.source_offsets)
                .map(
                    |(((input_block, operation_label), input_slot), source_offset)| {
                        FeatureInputBlockIdentityMember {
                            input_block,
                            operation_label,
                            input_slot,
                            source_offset,
                        }
                    },
                )
                .collect(),
        })
    }
}

/// Serialized column-row grammar carrying a reused feature input block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ColumnIndexRowKind {
    /// `2d 02 0b ... 93 8a` index row.
    Index,
    /// `02 0b ... 93 8c` linked-index row.
    LinkedIndex,
    /// `02 01 01 01 16` target-index row.
    TargetIndex,
}

impl ColumnIndexRowKind {
    const fn id_component(self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::LinkedIndex => "linked-index",
            Self::TargetIndex => "target-index",
        }
    }
}

/// Exact reuse of one feature input block by any column-row slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureInputColumnRowUse {
    /// Globally unique use identity.
    pub(super) id: String,
    /// Feature input binding that resolves to the shared block.
    input_block: String,
    /// Owning feature operation label.
    pub(super) operation_label: String,
    /// Input slot in the operation header.
    input_slot: HeaderSlot,
    /// Serialized grammar of the referenced column row.
    row_kind: ColumnIndexRowKind,
    /// Native row identity in its grammar-specific arena.
    column_row: String,
    /// Unique complete composite table containing the row, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_column_table"
    )]
    column_table: Option<String>,
    /// Zero-based slot in the row's four-block lane.
    row_slot: ColumnRowSlot,
    /// Exact shared target in the native `data_blocks` arena.
    data_block: String,
    /// Absolute file offset of the row's compact block index.
    source_offset: u64,
}

/// Linked or target-index row whose slot zero is a feature input block.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FeatureInputColumnTargetRow {
    /// Linked-index row grammar.
    Linked {
        leading_index: u32,
        leading_index_source_offset: u64,
        discriminator: crate::om::discriminators::LinkedIndexDiscriminator,
        flag: crate::om::discriminators::LinkedIndexFlag,
    },
    /// Target-index row grammar.
    Target,
}

/// Unique composite-table target row for one feature input block.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureInputColumnTargetWire")]
pub(super) struct FeatureInputColumnTarget {
    /// Globally unique target identity.
    pub(super) id: String,
    /// Feature input binding that resolves to the target block.
    input_block: String,
    /// Owning feature operation label.
    pub(super) operation_label: String,
    /// Input slot in the operation header.
    input_slot: HeaderSlot,
    /// Linked or target-index row whose slot zero is the input block.
    column_row: String,
    /// Linked or target-index grammar of `column_row`.
    row: FeatureInputColumnTargetRow,
    /// Three compact values following the fixed row marker.
    field_indices: [u32; 3],
    /// Three same-section blocks addressed by `field_indices`.
    field_data_blocks: [String; 3],
    /// Absolute offsets of the three compact field values.
    field_source_offsets: [u64; 3],
    /// Serialized row mode.
    mode: crate::om::discriminators::IndexRowMode,
    /// Unique complete composite table containing `column_row`.
    column_table: String,
    /// Exact target in the native `data_blocks` arena.
    data_block: String,
    /// Absolute file offset of the row's target block index.
    source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureInputColumnTargetWire {
    id: String,
    input_block: String,
    operation_label: String,
    input_slot: HeaderSlot,
    column_row: String,
    row_kind: ColumnIndexRowKind,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_leading_index"
    )]
    leading_index: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_leading_index_source_offset"
    )]
    leading_index_source_offset: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_discriminator"
    )]
    discriminator: Option<crate::om::discriminators::LinkedIndexDiscriminator>,
    field_indices: [u32; 3],
    field_data_blocks: [String; 3],
    field_source_offsets: [u64; 3],
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_flag"
    )]
    flag: Option<crate::om::discriminators::LinkedIndexFlag>,
    mode: crate::om::discriminators::IndexRowMode,
    column_table: String,
    data_block: String,
    source_offset: u64,
}

#[cfg(test)]
impl From<FeatureInputColumnTarget> for FeatureInputColumnTargetWire {
    fn from(value: FeatureInputColumnTarget) -> Self {
        let (row_kind, leading_index, leading_index_source_offset, discriminator, flag) =
            match value.row {
                FeatureInputColumnTargetRow::Linked {
                    leading_index,
                    leading_index_source_offset,
                    discriminator,
                    flag,
                } => (
                    ColumnIndexRowKind::LinkedIndex,
                    Some(leading_index),
                    Some(leading_index_source_offset),
                    Some(discriminator),
                    Some(flag),
                ),
                FeatureInputColumnTargetRow::Target => {
                    (ColumnIndexRowKind::TargetIndex, None, None, None, None)
                }
            };
        Self {
            id: value.id,
            input_block: value.input_block,
            operation_label: value.operation_label,
            input_slot: value.input_slot,
            column_row: value.column_row,
            row_kind,
            leading_index,
            leading_index_source_offset,
            discriminator,
            field_indices: value.field_indices,
            field_data_blocks: value.field_data_blocks,
            field_source_offsets: value.field_source_offsets,
            flag,
            mode: value.mode,
            column_table: value.column_table,
            data_block: value.data_block,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeatureInputColumnTargetWire> for FeatureInputColumnTarget {
    type Error = String;

    fn try_from(wire: FeatureInputColumnTargetWire) -> Result<Self, Self::Error> {
        let row =
            match (
                wire.row_kind,
                wire.leading_index,
                wire.leading_index_source_offset,
                wire.discriminator,
                wire.flag,
            ) {
                (
                    ColumnIndexRowKind::LinkedIndex,
                    Some(leading_index),
                    Some(leading_index_source_offset),
                    Some(discriminator),
                    Some(flag),
                ) => FeatureInputColumnTargetRow::Linked {
                    leading_index,
                    leading_index_source_offset,
                    discriminator,
                    flag,
                },
                (ColumnIndexRowKind::TargetIndex, None, None, None, None) => {
                    FeatureInputColumnTargetRow::Target
                }
                _ => return Err(
                    "feature input column target linked fields are present exactly for LinkedIndex"
                        .to_owned(),
                ),
            };
        Ok(Self {
            id: wire.id,
            input_block: wire.input_block,
            operation_label: wire.operation_label,
            input_slot: wire.input_slot,
            column_row: wire.column_row,
            row,
            field_indices: wire.field_indices,
            field_data_blocks: wire.field_data_blocks,
            field_source_offsets: wire.field_source_offsets,
            mode: wire.mode,
            column_table: wire.column_table,
            data_block: wire.data_block,
            source_offset: wire.source_offset,
        })
    }
}

/// Ordered parameter declaration reached through one feature input block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureParameterBinding {
    /// Globally unique binding identity.
    pub(super) id: String,
    /// Owning operation-label identity.
    pub(super) operation_label: String,
    /// Zero-based operation-header input slot.
    pub(super) input_slot: HeaderSlot,
    /// Input block carrying the object-reference field.
    pub(super) input_block: String,
    /// Zero-based object-reference order within the input block.
    pub(super) reference_ordinal: u32,
    /// Target parameter declaration in the native expression arena.
    pub(super) expression_declaration: String,
    /// Exact numeric expression bound to the declaration, when unique.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_expression"
    )]
    pub(super) expression: Option<String>,
    /// Persistent OM object ID of the declaration.
    pub(super) object_id: u32,
    /// Absolute file offset of the object-index token.
    pub(super) source_offset: u64,
}

/// All binding occurrences by which one operation consumes one expression.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureParameterUseWire")]
pub(super) struct FeatureParameterUse {
    /// Globally unique use identity.
    pub(super) id: String,
    /// Consuming operation-label identity.
    pub(super) operation_label: String,
    /// Exact numeric expression consumed by the operation.
    pub(super) expression: String,
    /// Binding occurrences in ascending source-offset order.
    pub(super) bindings: Vec<FeatureParameterUseBinding>,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureParameterUse {
    fn decode_cost(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, operation: &'static str) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(&(self.bindings[0].source_offset,&self.operation_label,&self.expression), ctx, operation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FeatureParameterUseBinding {
    pub(super) binding: String,
    pub(super) source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureParameterUseWire {
    id: String,
    operation_label: String,
    expression: String,
    bindings: Vec<String>,
    source_offsets: Vec<u64>,
}

#[cfg(test)]
impl From<FeatureParameterUse> for FeatureParameterUseWire {
    fn from(value: FeatureParameterUse) -> Self {
        let (bindings, source_offsets) = value
            .bindings
            .into_iter()
            .map(|binding| (binding.binding, binding.source_offset))
            .unzip();
        Self {
            id: value.id,
            operation_label: value.operation_label,
            expression: value.expression,
            bindings,
            source_offsets,
        }
    }
}

impl TryFrom<FeatureParameterUseWire> for FeatureParameterUse {
    type Error = String;
    fn try_from(wire: FeatureParameterUseWire) -> Result<Self, Self::Error> {
        if wire.bindings.len() != wire.source_offsets.len() {
            return Err("bindings and source_offsets must have equal lengths".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            expression: wire.expression,
            bindings: wire
                .bindings
                .into_iter()
                .zip(wire.source_offsets)
                .map(|(binding, source_offset)| FeatureParameterUseBinding {
                    binding,
                    source_offset,
                })
                .collect(),
        })
    }
}

/// Ordered sketch-history record and its exact native input lanes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureSketchRecord {
    /// Globally unique sketch-record identity.
    pub(super) id: String,
    /// Owning `SKETCH` operation label.
    pub(super) operation_label: String,
    /// Zero-based order within the feature-history area.
    ordinal: u32,
    /// Exact bounded operation record.
    operation_record: String,
    /// Resolved input bindings in header-slot order.
    input_blocks: Vec<String>,
    /// Ordered references carried by the sketch payload.
    payload_references: Vec<String>,
    /// Absolute file offset of the operation label.
    pub(super) source_offset: u64,
}

/// Completely resolved native construction lane of a datum coordinate system.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureDatumCsysConstructionWire")]
pub(super) struct FeatureDatumCsysConstruction {
    pub(super) id: String,
    pub(super) operation_label: String,
    frame: crate::om::datum_csys::DatumCsysFrame<String>,
}

#[derive(Serialize, Deserialize)]
struct FeatureDatumCsysConstructionWire {
    id: String,
    operation_label: String,
    control: u8,
    object_indices: [u32; 8],
    raw_object_indices: [Vec<u8>; 8],
    data_blocks: [String; 8],
    source_offsets: [u64; 8],
}

#[cfg(test)]
impl From<FeatureDatumCsysConstruction> for FeatureDatumCsysConstructionWire {
    fn from(value: FeatureDatumCsysConstruction) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            control: value.frame.control(),
            object_indices: value
                .frame
                .members()
                .each_ref()
                .map(|(token, _)| token.value()),
            raw_object_indices: value
                .frame
                .members()
                .each_ref()
                .map(|(token, _)| token.raw().to_vec()),
            data_blocks: value
                .frame
                .members()
                .each_ref()
                .map(|(_, binding)| binding.clone()),
            source_offsets: value.frame.offsets(),
        }
    }
}

impl TryFrom<FeatureDatumCsysConstructionWire> for FeatureDatumCsysConstruction {
    type Error = String;
    fn try_from(wire: FeatureDatumCsysConstructionWire) -> Result<Self, Self::Error> {
        let origin = wire.source_offsets[0]
            .checked_sub(14)
            .ok_or("source_offsets[0]: datum-CSYS header precedes file")?;
        let [slot0, slot1, slot2, slot3, slot4, slot5, slot6, slot7] =
            std::array::from_fn::<_, 8, _>(|slot| {
                crate::om::reference_index::PayloadIndexToken::from_wire(
                    wire.object_indices[slot],
                    &wire.raw_object_indices[slot],
                )
                .map(|token| (token, wire.data_blocks[slot].clone()))
                .map_err(|error| format!("object_indices/raw_object_indices[{slot}]: {error}"))
            });
        let frame = crate::om::datum_csys::DatumCsysFrame::new(
            wire.control,
            origin,
            [
                slot0?, slot1?, slot2?, slot3?, slot4?, slot5?, slot6?, slot7?,
            ],
        )?;
        if frame.offsets() != wire.source_offsets {
            return Err("source_offsets: disagrees with datum-CSYS token widths".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            frame,
        })
    }
}

/// Exact reuse of one datum-CSYS construction block by a column-row slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureDatumCsysColumnRowUse {
    /// Globally unique use identity.
    pub(super) id: String,
    /// Owning datum-CSYS construction.
    construction: String,
    /// Owning `DATUM_CSYS` operation label.
    pub(super) operation_label: String,
    /// Zero-based slot in the construction's eight-block lane.
    construction_slot: DatumCsysSlot,
    /// Serialized grammar of the referenced column row.
    row_kind: ColumnIndexRowKind,
    /// Native row identity in its grammar-specific arena.
    column_row: String,
    /// Unique complete composite table containing the row, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_column_table"
    )]
    column_table: Option<String>,
    /// Zero-based slot in the row's four-block lane.
    row_slot: ColumnRowSlot,
    /// Exact shared target in the native `data_blocks` arena.
    data_block: String,
    /// Absolute file offset of the construction's object-index token.
    construction_source_offset: u64,
    /// Absolute file offset of the row's compact block index.
    row_source_offset: u64,
}

/// Exact logical payload reconstructed from the two leading datum-CSYS blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureDatumCsysPayload {
    /// Globally unique reconstructed-payload identity.
    pub(super) id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub(super) operation_label: String,
    /// Construction defining the ordered block lane.
    construction: String,
    /// Ordered source blocks and the hash of their concatenated bytes.
    #[serde(flatten)]
    content: FeaturePayloadContent<[FeaturePayloadBlock; 2]>,
}

/// One exactly framed scalar pair in a reconstructed feature payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "FeaturePayloadScalarPairWire")]
pub(super) struct FeaturePayloadScalarPair {
    /// Globally unique scalar-pair identity.
    pub(super) id: String,
    /// Owning operation label.
    pub(super) operation_label: String,
    /// Reconstructed payload carrying the frame.
    #[serde(flatten)]
    pub(super) payload: FeatureScalarPairPayload,
    /// Zero-based frame order within the payload.
    pub(super) ordinal: u32,
    /// Absolute source offsets of the scalar encodings across payload blocks.
    pub(super) value_source_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub(super) source_offset: u64,
}

#[derive(Deserialize)]
struct FeaturePayloadScalarPairWire {
    /// Globally unique scalar-pair identity.
    id: String,
    /// Owning operation label.
    operation_label: String,
    /// Reconstructed payload carrying the frame.
    #[serde(flatten)]
    payload: FeatureScalarPairPayloadWire,
    /// Zero-based frame order within the payload.
    ordinal: u32,
    /// Ordered finite shifted-IEEE values.
    values: [f64; 2],
    /// Exact shifted-binary64 encodings in value order.
    raw_values: [[u8; 8]; 2],
    /// Payload-relative offset of the discriminator.
    payload_offset: u64,
    /// Payload-relative scalar offsets.
    value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    source_offset: u64,
    /// Absolute source offsets of the scalar encodings.
    value_source_offsets: [u64; 2],
}

impl TryFrom<FeaturePayloadScalarPairWire> for FeaturePayloadScalarPair {
    type Error = String;

    fn try_from(wire: FeaturePayloadScalarPairWire) -> Result<Self, Self::Error> {
        fn frame<F: Binary64PairForm>(
            form: F,
            offset: u64,
            atoms: [ShiftedBinary64; 2],
            positions: [u64; 2],
        ) -> Result<Binary64Pair<F, u64>, String> {
            let frame = Binary64Pair::new(form, offset, atoms)
                .ok_or("payload_offset must leave room for the complete binary64 pair")?;
            if frame.value_offsets() != positions {
                return Err("value_payload_offsets must match the binary64 pair form".into());
            }
            Ok(frame)
        }

        let [first, second] = std::array::from_fn::<_, 2, _>(|i| {
            ShiftedBinary64::from_wire(wire.values[i], wire.raw_values[i])
                .map_err(|error| format!("values/raw_values[{i}]: {error}"))
        });
        let atoms = [first?, second?];

        let payload = match wire.payload {
            FeatureScalarPairPayloadWire::DatumCsys {
                datum_csys_payload,
                discriminator,
            } => FeatureScalarPairPayload::DatumCsys {
                datum_csys_payload,
                frame: frame(
                    ObjectPairForm::try_from(discriminator.as_slice())?,
                    wire.payload_offset,
                    atoms,
                    wire.value_payload_offsets,
                )?,
            },
            FeatureScalarPairPayloadWire::DatumPlane {
                datum_plane_payload,
            } => FeatureScalarPairPayload::DatumPlane {
                datum_plane_payload,
                frame: frame(
                    DatumPlanePairForm,
                    wire.payload_offset,
                    atoms,
                    wire.value_payload_offsets,
                )?,
            },
            FeatureScalarPairPayloadWire::Construction {
                construction_payload,
                discriminator,
            } => FeatureScalarPairPayload::Construction {
                construction_payload,
                frame: frame(
                    SketchBinary64PairForm::try_from(discriminator.as_slice())?,
                    wire.payload_offset,
                    atoms,
                    wire.value_payload_offsets,
                )?,
            },
            FeatureScalarPairPayloadWire::SurfaceConstruction {
                surface_construction_payload,
                discriminator,
            } => FeatureScalarPairPayload::SurfaceConstruction {
                surface_construction_payload,
                frame: frame(
                    ObjectPairForm::try_from(discriminator.as_slice())?,
                    wire.payload_offset,
                    atoms,
                    wire.value_payload_offsets,
                )?,
            },
        };
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            payload,
            ordinal: wire.ordinal,
            value_source_offsets: wire.value_source_offsets,
            source_offset: wire.source_offset,
        })
    }
}

/// Payload identity and its scalar-pair branch discriminator.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum FeatureScalarPairPayloadWire {
    DatumCsys {
        datum_csys_payload: String,
        discriminator: Vec<u8>,
    },
    DatumPlane {
        datum_plane_payload: String,
    },
    Construction {
        construction_payload: String,
        discriminator: Vec<u8>,
    },
    SurfaceConstruction {
        surface_construction_payload: String,
        discriminator: Vec<u8>,
    },
}

/// Payload identity and the exact scalar-pair frame for that owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FeatureScalarPairPayload {
    DatumCsys {
        datum_csys_payload: String,
        frame: Binary64Pair<ObjectPairForm, u64>,
    },
    DatumPlane {
        datum_plane_payload: String,
        frame: Binary64Pair<DatumPlanePairForm, u64>,
    },
    Construction {
        construction_payload: String,
        frame: Binary64Pair<SketchBinary64PairForm, u64>,
    },
    SurfaceConstruction {
        surface_construction_payload: String,
        frame: Binary64Pair<ObjectPairForm, u64>,
    },
}

impl FeatureScalarPairPayload {
    #[must_use]
    pub(super) fn id(&self) -> &str {
        match self {
            Self::DatumCsys {
                datum_csys_payload, ..
            } => datum_csys_payload,
            Self::DatumPlane {
                datum_plane_payload,
                ..
            } => datum_plane_payload,
            Self::Construction {
                construction_payload,
                ..
            } => construction_payload,
            Self::SurfaceConstruction {
                surface_construction_payload,
                ..
            } => surface_construction_payload,
        }
    }
}

impl Serialize for FeaturePayloadScalarPair {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let (payload_key, payload_id, discriminator, atoms, payload_offset, positions) =
            match &self.payload {
                FeatureScalarPairPayload::DatumCsys {
                    datum_csys_payload,
                    frame,
                } => (
                    "datum_csys_payload",
                    datum_csys_payload,
                    Some(frame.discriminator()),
                    frame.atoms(),
                    frame.offset(),
                    frame.value_offsets(),
                ),
                FeatureScalarPairPayload::DatumPlane {
                    datum_plane_payload,
                    frame,
                } => (
                    "datum_plane_payload",
                    datum_plane_payload,
                    None,
                    frame.atoms(),
                    frame.offset(),
                    frame.value_offsets(),
                ),
                FeatureScalarPairPayload::Construction {
                    construction_payload,
                    frame,
                } => (
                    "construction_payload",
                    construction_payload,
                    Some(frame.discriminator()),
                    frame.atoms(),
                    frame.offset(),
                    frame.value_offsets(),
                ),
                FeatureScalarPairPayload::SurfaceConstruction {
                    surface_construction_payload,
                    frame,
                } => (
                    "surface_construction_payload",
                    surface_construction_payload,
                    Some(frame.discriminator()),
                    frame.atoms(),
                    frame.offset(),
                    frame.value_offsets(),
                ),
            };
        let mut record = serializer.serialize_struct(
            "FeaturePayloadScalarPair",
            10 + usize::from(discriminator.is_some()),
        )?;
        record.serialize_field("id", &self.id)?;
        record.serialize_field("operation_label", &self.operation_label)?;
        record.serialize_field(payload_key, payload_id)?;
        record.serialize_field("ordinal", &self.ordinal)?;
        record.serialize_field("values", &atoms.map(|atom| atom.value().get()))?;
        record.serialize_field("raw_values", &atoms.map(ShiftedBinary64::raw))?;
        record.serialize_field("payload_offset", &payload_offset)?;
        record.serialize_field("value_payload_offsets", &positions)?;
        record.serialize_field("source_offset", &self.source_offset)?;
        record.serialize_field("value_source_offsets", &self.value_source_offsets)?;
        if let Some(discriminator) = discriminator {
            record.serialize_field("discriminator", &discriminator)?;
        }
        record.end()
    }
}

/// One exactly framed signed Q1.55 pair in a reconstructed datum-CSYS payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "pair_wire::FeatureDatumCsysPayloadFixedPairWire")]
pub(super) struct FeatureDatumCsysPayloadFixedPair {
    /// Globally unique fixed-pair identity.
    pub(super) id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub(super) operation_label: String,
    /// Reconstructed payload carrying the frame.
    datum_csys_payload: String,
    /// Zero-based frame order within the payload.
    ordinal: u32,
    /// Ordered dimensionless Q1.55 values.
    values: [Q155; 2],
    /// Closed pair framing and checked payload position.
    position: PairPosition<DatumPairForm>,
    /// Absolute source offset of the discriminator.
    source_offset: u64,
    /// Absolute source offsets of the two `30` atom markers.
    value_source_offsets: [u64; 2],
}

/// One exactly framed scalar field in a reconstructed feature payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "FeaturePayloadScalarWire")]
pub(super) struct FeaturePayloadScalar {
    /// Globally unique scalar-field identity.
    pub(super) id: String,
    /// Owning operation label.
    pub(super) operation_label: String,
    /// Reconstructed payload carrying the field.
    #[serde(flatten)]
    pub(super) payload: FeatureScalarPayload,
    /// Zero-based field order within the payload.
    pub(super) ordinal: u32,
    /// Serialized discriminator following the `50 59 66` marker.
    pub(super) field_code: u8,
    /// Checked shifted-binary64 atom.
    pub(super) scalar: ShiftedBinary64,
    /// Payload-relative offset of the field marker.
    pub(super) payload_offset: u64,
    /// Absolute source offset of the field marker.
    pub(super) source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeaturePayloadScalarWire {
    /// Globally unique scalar-field identity.
    id: String,
    /// Owning operation label.
    operation_label: String,
    /// Reconstructed payload carrying the field.
    #[serde(flatten)]
    payload: FeatureScalarPayload,
    /// Zero-based field order within the payload.
    ordinal: u32,
    /// Serialized discriminator following the `50 59 66` marker.
    field_code: u8,
    /// Finite shifted-IEEE binary64 value.
    value: f64,
    /// Exact shifted-binary64 encoding.
    raw_value: [u8; 8],
    /// Payload-relative offset of the field marker.
    payload_offset: u64,
    /// Absolute source offset of the field marker.
    source_offset: u64,
}

#[cfg(test)]
impl From<FeaturePayloadScalar> for FeaturePayloadScalarWire {
    fn from(value: FeaturePayloadScalar) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            payload: value.payload,
            ordinal: value.ordinal,
            field_code: value.field_code,
            value: value.scalar.value().get(),
            raw_value: value.scalar.raw(),
            payload_offset: value.payload_offset,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeaturePayloadScalarWire> for FeaturePayloadScalar {
    type Error = String;

    fn try_from(wire: FeaturePayloadScalarWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            payload: wire.payload,
            ordinal: wire.ordinal,
            field_code: wire.field_code,
            scalar: ShiftedBinary64::from_wire(wire.value, wire.raw_value)
                .map_err(|error| format!("value/raw_value: {error}"))?,
            payload_offset: wire.payload_offset,
            source_offset: wire.source_offset,
        })
    }
}

/// Payload identity under its native record field name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum FeatureScalarPayload {
    DatumCsys { datum_csys_payload: String },
    Construction { construction_payload: String },
}

impl FeatureScalarPayload {
    #[must_use]
    fn id(&self) -> &str {
        match self {
            Self::DatumCsys { datum_csys_payload } => datum_csys_payload,
            Self::Construction {
                construction_payload,
            } => construction_payload,
        }
    }
}

/// Typed descriptor from one of the final three datum-CSYS construction lanes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureDatumCsysDescriptorWire")]
pub(super) struct FeatureDatumCsysDescriptor {
    /// Globally unique descriptor identity.
    pub(super) id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub(super) operation_label: String,
    /// Construction carrying the descriptor lane.
    construction: String,
    /// Construction reference ordinal in the range 5–7.
    reference_ordinal: CsysDescriptorSlot,
    /// Resolved source block.
    data_block: String,
    /// Checked descriptor bytes and source position.
    descriptor: LocatedCsysDescriptor,
}

#[derive(Serialize, Deserialize)]
struct FeatureDatumCsysDescriptorWire {
    /// Globally unique descriptor identity.
    id: String,
    /// Owning `DATUM_CSYS` operation label.
    operation_label: String,
    /// Construction carrying the descriptor lane.
    construction: String,
    /// Construction reference ordinal in the range 5–7.
    reference_ordinal: u8,
    /// Resolved source block.
    data_block: String,
    /// Exact bytes preceding the hexadecimal identity.
    prefix: Vec<u8>,
    /// Lowercase 30–32 digit hexadecimal identity.
    identity: String,
    /// Exact bytes following the hexadecimal identity.
    suffix: Vec<u8>,
    /// Absolute source offset of the block.
    source_offset: u64,
    /// Absolute source offset of the identity.
    identity_source_offset: u64,
}

#[cfg(test)]
impl From<FeatureDatumCsysDescriptor> for FeatureDatumCsysDescriptorWire {
    fn from(value: FeatureDatumCsysDescriptor) -> Self {
        let identity_source_offset = value.descriptor.identity_source_offset();
        Self {
            id: value.id,
            operation_label: value.operation_label,
            construction: value.construction,
            reference_ordinal: value.reference_ordinal.into(),
            data_block: value.data_block,
            prefix: value.descriptor.descriptor().prefix().to_vec(),
            identity: value.descriptor.descriptor().identity().as_str().to_owned(),
            suffix: value.descriptor.descriptor().suffix().to_vec(),
            source_offset: value.descriptor.source_offset(),
            identity_source_offset,
        }
    }
}

impl TryFrom<FeatureDatumCsysDescriptorWire> for FeatureDatumCsysDescriptor {
    type Error = String;

    fn try_from(wire: FeatureDatumCsysDescriptorWire) -> Result<Self, Self::Error> {
        let identity = CsysIdentity::try_from(wire.identity).map_err(str::to_owned)?;
        let descriptor = CsysDescriptor::from_wire(&wire.prefix, &identity, &wire.suffix)
            .map_err(str::to_owned)?;
        let descriptor =
            LocatedCsysDescriptor::new(descriptor, wire.source_offset).map_err(str::to_owned)?;
        if descriptor.identity_source_offset() != wire.identity_source_offset {
            return Err(
                "identity_source_offset must equal source_offset plus prefix length".into(),
            );
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            construction: wire.construction,
            reference_ordinal: CsysDescriptorSlot::try_from(wire.reference_ordinal)
                .map_err(str::to_owned)?,
            data_block: wire.data_block,
            descriptor,
        })
    }
}

/// Exact shared descriptor identity between datum-plane and datum-CSYS history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureDatumPlaneCsysIdentityUse {
    /// Globally unique relation identity.
    pub(super) id: String,
    /// Shared lowercase hexadecimal identity.
    identity: CsysIdentity,
    /// Typed datum-plane descriptor.
    datum_plane_descriptor: String,
    /// Datum-plane operation carrying the descriptor.
    pub(super) datum_plane_operation_label: String,
    /// Typed datum-CSYS descriptor.
    datum_csys_descriptor: String,
    /// Datum-CSYS operation carrying the descriptor.
    pub(super) datum_csys_operation_label: String,
    /// Datum-CSYS construction reference ordinal.
    datum_csys_reference_ordinal: CsysDescriptorSlot,
}

/// Exact logical datum-plane object payload reconstructed in lane order.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureDatumPlanePayloadWire")]
pub(super) struct FeatureDatumPlanePayload {
    /// Globally unique reconstructed-payload identity.
    pub(super) id: String,
    /// Owning `DATUM_PLANE` operation label.
    pub(super) operation_label: String,
    /// Header defining the ordered object-block lane.
    datum_plane_header: String,
    /// Ordered source blocks and the hash of their concatenated bytes.
    content: FeaturePayloadContent<Vec<FeaturePayloadBlock>>,
    /// Unique terminal index lane, when the payload has exactly one.
    index_lane: Option<crate::om::datum_index::DatumIndexLane<u64>>,
}

#[derive(Serialize, Deserialize)]
struct FeatureDatumPlanePayloadWire {
    id: String,
    operation_label: String,
    datum_plane_header: String,
    #[serde(flatten)]
    content: FeaturePayloadContent<Vec<FeaturePayloadBlock>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_index_lane_offset"
    )]
    index_lane_offset: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_index_lane_declared_count"
    )]
    index_lane_declared_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    index_lane_values: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    index_lane_raw_indices: Vec<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    index_lane_value_offsets: Vec<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_index_lane_trailer"
    )]
    index_lane_trailer: Option<u32>,
}

#[cfg(test)]
impl From<FeatureDatumPlanePayload> for FeatureDatumPlanePayloadWire {
    fn from(value: FeatureDatumPlanePayload) -> Self {
        let (
            index_lane_offset,
            index_lane_declared_count,
            index_lane_values,
            index_lane_raw_indices,
            index_lane_value_offsets,
            index_lane_trailer,
        ) = match value.index_lane {
            None => (None, None, Vec::new(), Vec::new(), Vec::new(), None),
            Some(lane) => (
                Some(lane.offset()),
                Some(usize::from(lane.declared_count())),
                lane.indices().map(|entry| entry.atom.value()).collect(),
                lane.indices()
                    .map(|entry| entry.atom.raw().to_vec())
                    .collect(),
                lane.indices().map(|entry| entry.offset).collect(),
                Some(lane.trailer()),
            ),
        };
        Self {
            id: value.id,
            operation_label: value.operation_label,
            datum_plane_header: value.datum_plane_header,
            content: value.content,
            index_lane_offset,
            index_lane_declared_count,
            index_lane_values,
            index_lane_raw_indices,
            index_lane_value_offsets,
            index_lane_trailer,
        }
    }
}

impl TryFrom<FeatureDatumPlanePayloadWire> for FeatureDatumPlanePayload {
    type Error = String;

    fn try_from(wire: FeatureDatumPlanePayloadWire) -> Result<Self, Self::Error> {
        let index_lane = match (
            wire.index_lane_offset,
            wire.index_lane_declared_count,
            wire.index_lane_trailer,
            wire.index_lane_values.is_empty()
                && wire.index_lane_raw_indices.is_empty()
                && wire.index_lane_value_offsets.is_empty(),
        ) {
            (None, None, None, true) => None,
            (Some(offset), Some(declared_count), Some(trailer), _) => {
                if declared_count != wire.index_lane_values.len() + 1 {
                    return Err("index_lane_declared_count must fit a byte and equal the nonempty entry count plus one".to_owned());
                }
                if wire.index_lane_values.len() != wire.index_lane_raw_indices.len()
                    || wire.index_lane_values.len() != wire.index_lane_value_offsets.len()
                {
                    return Err("index_lane_values/index_lane_raw_indices/index_lane_value_offsets differ in length".to_owned());
                }
                let indices = wire
                    .index_lane_values
                    .into_iter()
                    .zip(wire.index_lane_raw_indices)
                    .enumerate()
                    .map(|(slot, (value, raw))| {
                        CompactIndexAtom::from_wire(value, &raw)
                            .map_err(|error| format!("index_lane_values[{slot}]: {error}"))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let lane = crate::om::datum_index::DatumIndexLane::<u64>::new(
                    CountedIndexMembers::new(indices)
                        .map_err(|error| format!("index_lane_declared_count: {error}"))?,
                    trailer,
                    offset,
                )
                .ok_or("index_lane_offset overflows the terminal frame")?;
                if !lane
                    .indices()
                    .map(|token| token.offset)
                    .eq(wire.index_lane_value_offsets)
                {
                    return Err("index_lane_value_offsets must follow the terminal frame".into());
                }
                Some(lane)
            }
            _ => {
                return Err(
                    "datum-plane index lane offset count trailer and entries are present together"
                        .to_owned(),
                )
            }
        };
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            datum_plane_header: wire.datum_plane_header,
            content: wire.content,
            index_lane,
        })
    }
}

/// Resolved typed descriptor of one datum-plane construction.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureDatumPlaneDescriptorWire")]
pub(super) struct FeatureDatumPlaneDescriptor {
    /// Globally unique descriptor identity.
    pub(super) id: String,
    /// Owning `DATUM_PLANE` operation label.
    pub(super) operation_label: String,
    /// Header carrying the descriptor reference.
    datum_plane_header: String,
    /// Zero-based descriptor-lane order.
    ordinal: u32,
    /// Resolved source block.
    data_block: String,
    /// Exact identity, schema token, and terminal label.
    descriptor: PlaneDescriptor,
    /// Absolute source offset of the descriptor block.
    source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureDatumPlaneDescriptorWire {
    /// Globally unique descriptor identity.
    id: String,
    /// Owning `DATUM_PLANE` operation label.
    operation_label: String,
    /// Header carrying the descriptor reference.
    datum_plane_header: String,
    /// Zero-based descriptor-lane order.
    ordinal: u32,
    /// Resolved source block.
    data_block: String,
    /// Lowercase hexadecimal identity preceding the delimiter.
    identity: String,
    /// Exact descriptor suffix beginning with `?`.
    suffix: Vec<u8>,
    /// Non-null compact schema index following `?A`.
    schema_index: u32,
    /// Nonempty printable terminal label.
    label: String,
    /// Absolute source offset of the descriptor block.
    source_offset: u64,
}

#[cfg(test)]
impl From<FeatureDatumPlaneDescriptor> for FeatureDatumPlaneDescriptorWire {
    fn from(value: FeatureDatumPlaneDescriptor) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            datum_plane_header: value.datum_plane_header,
            ordinal: value.ordinal,
            data_block: value.data_block,
            identity: value.descriptor.identity().to_owned(),
            suffix: value.descriptor.suffix(),
            schema_index: value.descriptor.schema_index(),
            label: value.descriptor.label().to_owned(),
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeatureDatumPlaneDescriptorWire> for FeatureDatumPlaneDescriptor {
    type Error = String;
    fn try_from(wire: FeatureDatumPlaneDescriptorWire) -> Result<Self, Self::Error> {
        let descriptor = PlaneDescriptor::from_wire(
            &wire.identity,
            &wire.suffix,
            wire.schema_index,
            &wire.label,
        )
        .map_err(str::to_owned)?;
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            datum_plane_header: wire.datum_plane_header,
            ordinal: wire.ordinal,
            data_block: wire.data_block,
            descriptor,
            source_offset: wire.source_offset,
        })
    }
}

/// Datum-plane construction lane containing a reused block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DatumPlaneBlockLane {
    /// Compact descriptor lane.
    Descriptor,
    /// Canonical object-reference lane.
    Object,
}

/// Exact reuse of one resolved datum-plane construction block by an operation input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureDatumPlaneBlockUse {
    /// Globally unique relation identity.
    pub(super) id: String,
    /// Owning datum-plane header.
    datum_plane_header: String,
    /// `DATUM_PLANE` operation owning the construction.
    pub(super) construction_operation_label: String,
    /// Construction lane containing the block.
    lane: DatumPlaneBlockLane,
    /// Zero-based position within the lane.
    reference_ordinal: u32,
    /// Shared offset-store block.
    data_block: String,
    /// Matching operation-header input binding.
    input_binding: String,
    /// Operation whose header addresses the shared block.
    pub(super) input_operation_label: String,
    /// Zero-based operation-header input slot.
    input_slot: HeaderSlot,
}

/// Exact reuse of one datum-coordinate-system construction block by an operation input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureDatumCsysBlockUse {
    /// Globally unique block-use identity.
    pub(super) id: String,
    /// Owning datum-coordinate-system construction.
    construction: String,
    /// `DATUM_CSYS` operation owning the construction lane.
    pub(super) construction_operation_label: String,
    /// Zero-based position in the eight-reference construction lane.
    reference_ordinal: DatumCsysSlot,
    /// Shared offset-store block.
    data_block: String,
    /// Matching operation-header input binding.
    input_binding: String,
    /// Operation whose header addresses the shared block.
    pub(super) input_operation_label: String,
    /// Zero-based operation-header input slot.
    input_slot: HeaderSlot,
}

/// A construction reference paired with its uniquely resolved source block.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FeatureConstructionMember {
    reference: String,
    data_block: String,
}

/// Completely resolved counted-reference field of one sketch construction.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureSketchConstructionInputsWire")]
pub(super) struct FeatureSketchConstructionInputs {
    /// Globally unique construction-input identity.
    pub(super) id: String,
    /// Owning `SKETCH` operation label.
    pub(super) operation_label: String,
    /// Joined typed sketch record.
    sketch_record: String,
    /// Ordered references and their uniquely resolved source blocks.
    members: Vec<FeatureConstructionMember>,
    /// Reference following the field separator.
    terminal_reference: String,
    /// Uniquely resolved terminal block.
    terminal_data_block: String,
}

#[derive(Serialize, Deserialize)]
struct FeatureSketchConstructionInputsWire {
    id: String,
    operation_label: String,
    sketch_record: String,
    member_references: Vec<String>,
    member_data_blocks: Vec<String>,
    terminal_reference: String,
    terminal_data_block: String,
}

#[cfg(test)]
impl From<FeatureSketchConstructionInputs> for FeatureSketchConstructionInputsWire {
    fn from(value: FeatureSketchConstructionInputs) -> Self {
        let (member_references, member_data_blocks) = value
            .members
            .into_iter()
            .map(|member| (member.reference, member.data_block))
            .unzip();
        Self {
            id: value.id,
            operation_label: value.operation_label,
            sketch_record: value.sketch_record,
            member_references,
            member_data_blocks,
            terminal_reference: value.terminal_reference,
            terminal_data_block: value.terminal_data_block,
        }
    }
}

impl TryFrom<FeatureSketchConstructionInputsWire> for FeatureSketchConstructionInputs {
    type Error = String;
    fn try_from(wire: FeatureSketchConstructionInputsWire) -> Result<Self, Self::Error> {
        if wire.member_references.len() != wire.member_data_blocks.len() {
            return Err(
                "member_references and member_data_blocks must have equal lengths".to_owned(),
            );
        }
        let members = wire
            .member_references
            .into_iter()
            .zip(wire.member_data_blocks)
            .map(|(reference, data_block)| FeatureConstructionMember {
                reference,
                data_block,
            })
            .collect::<Vec<_>>();
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            sketch_record: wire.sketch_record,
            members,
            terminal_reference: wire.terminal_reference,
            terminal_data_block: wire.terminal_data_block,
        })
    }
}

/// Exact logical payload reconstructed from ordered feature-construction blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub(super) id: String,
    /// Owning operation label.
    pub(super) operation_label: String,
    /// Construction records selecting the ordered source blocks.
    #[serde(flatten)]
    pub(super) owner: FeatureConstructionOwner,
    /// Ordered source blocks and the hash of their concatenated bytes.
    #[serde(flatten)]
    content: FeaturePayloadContent<Vec<FeaturePayloadBlock>>,
}

/// Construction-specific ownership fields of a reconstructed payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    untagged,
    expecting = "construction ownership fields with a valid operation_kind for the selected grammar"
)]
pub(super) enum FeatureConstructionOwner {
    Sketch {
        construction_inputs: String,
    },
    ProjectedCurve {
        operation_kind: FeatureProjectedCurveKind,
        construction_references: Vec<String>,
    },
    Fset {
        reference_graph: String,
        group: FeatureFsetReferenceGroup,
    },
    Pattern {
        operation_kind: FeaturePatternKind,
        reference_layout: PatternPayloadReferenceLayout,
        construction_references: Vec<String>,
    },
    Draft {
        index_lane: String,
    },
    Block {
        construction: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) enum FeatureProjectedCurveKind {
    #[serde(rename = "CPROJ")]
    Projected,
    #[serde(rename = "CPROJ_CMB")]
    Combined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) enum FeaturePatternKind {
    #[serde(rename = "Pattern Feature")]
    Feature,
    #[serde(rename = "Pattern Geometry")]
    Geometry,
}

/// One exactly framed scaled shifted-binary64 pair in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "pair_wire::FeatureSketchPayloadFixedPairWire")]
pub(super) struct FeatureSketchPayloadFixedPair {
    /// Globally unique fixed-pair identity.
    pub(super) id: String,
    /// Owning `SKETCH` operation label.
    pub(super) operation_label: String,
    /// Reconstructed sketch payload carrying the frame.
    construction_payload: String,
    /// Zero-based frame order within the payload.
    pub(super) ordinal: u32,
    /// Ordered values reconstructed from the `30` shifted-binary64 atoms and scaled by `1/4`.
    values: [SketchScaledAtom; 2],
    /// Closed pair framing and checked payload position.
    position: PairPosition<SketchPairForm>,
    /// Absolute source offset of the discriminator.
    pub(super) source_offset: u64,
    /// Absolute source offsets of the two atom markers.
    value_source_offsets: [u64; 2],
}

/// One exactly framed mixed scaled shifted-binary64/binary32 pair in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "pair_wire::FeatureSketchPayloadMixedPairWire")]
pub(super) struct FeatureSketchPayloadMixedPair {
    /// Globally unique mixed-pair identity.
    pub(super) id: String,
    /// Owning `SKETCH` operation label.
    pub(super) operation_label: String,
    /// Reconstructed sketch payload carrying the frame.
    construction_payload: String,
    /// Zero-based frame order within the payload.
    pub(super) ordinal: u32,
    /// Exact scaled binary64 and binary32 atoms.
    scalars: SketchMixedScalars,
    /// Closed pair framing and checked payload position.
    position: PairPosition<MixedPairForm>,
    /// Absolute source offset of the discriminator.
    pub(super) source_offset: u64,
    /// Absolute source offsets of the two atom markers.
    value_source_offsets: [u64; 2],
}

/// Exact scalar-vector frame retained from one reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "FeatureSketchPayloadScalarLaneWire")]
pub(super) struct FeatureSketchPayloadScalarLane {
    /// Globally unique scalar-lane identity.
    pub(super) id: String,
    /// Owning `SKETCH` operation label.
    pub(super) operation_label: String,
    /// Reconstructed sketch payload carrying this lane.
    construction_payload: String,
    /// Zero-based lane order within the reconstructed payload.
    pub(super) ordinal: u32,
    /// Typed lane form and contiguous atoms with their absolute source locations.
    lane: FramedScalarRun<SketchScalarLaneForm, u64>,
    /// Absolute source offset of the discriminator.
    source_offset: u64,
    /// Absolute source offset of the terminating zero atom.
    terminator_source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureSketchPayloadScalarLaneWire {
    /// Globally unique scalar-lane identity.
    id: String,
    /// Owning `SKETCH` operation label.
    operation_label: String,
    /// Reconstructed sketch payload carrying this lane.
    construction_payload: String,
    /// Zero-based lane order within the reconstructed payload.
    ordinal: u32,
    /// Exact discriminator selecting the lane form.
    discriminator: Vec<u8>,
    /// Ordered finite scalar values after the discriminator.
    values: Vec<f64>,
    /// Exact nonzero scalar atoms in serialized order.
    raw_values: Vec<Vec<u8>>,
    /// Payload-relative offsets of the scalar atoms.
    value_payload_offsets: Vec<u64>,
    /// Payload-relative offset of the terminating zero atom.
    terminator_payload_offset: u64,
    /// Absolute source offset of the discriminator.
    source_offset: u64,
    /// Absolute source offsets of the scalar atoms.
    value_source_offsets: Vec<u64>,
    /// Absolute source offset of the terminating zero atom.
    terminator_source_offset: u64,
}

#[cfg(test)]
impl From<FeatureSketchPayloadScalarLane> for FeatureSketchPayloadScalarLaneWire {
    fn from(record: FeatureSketchPayloadScalarLane) -> Self {
        Self {
            id: record.id,
            operation_label: record.operation_label,
            construction_payload: record.construction_payload,
            ordinal: record.ordinal,
            discriminator: record.lane.form().discriminator().to_vec(),
            values: record
                .lane
                .iter()
                .map(|(_, scalar, _)| scalar.value().get())
                .collect(),
            raw_values: record
                .lane
                .iter()
                .map(|(_, scalar, _)| scalar.raw().to_vec())
                .collect(),
            value_payload_offsets: record.lane.iter().map(|(offset, _, _)| offset).collect(),
            terminator_payload_offset: record.lane.end(),
            source_offset: record.source_offset,
            value_source_offsets: record.lane.iter().map(|(_, _, source)| *source).collect(),
            terminator_source_offset: record.terminator_source_offset,
        }
    }
}

impl TryFrom<FeatureSketchPayloadScalarLaneWire> for FeatureSketchPayloadScalarLane {
    type Error = String;
    fn try_from(wire: FeatureSketchPayloadScalarLaneWire) -> Result<Self, Self::Error> {
        let count = wire.values.len();
        if wire.raw_values.len() != count
            || wire.value_payload_offsets.len() != count
            || wire.value_source_offsets.len() != count
        {
            return Err("sketch values, raw_values, value_payload_offsets, and value_source_offsets must have equal lengths".into());
        }
        let form = SketchScalarLaneForm::from_discriminator(&wire.discriminator)?;
        let offset = wire
            .value_payload_offsets
            .first()
            .copied()
            .ok_or("values must contain a sketch scalar atom")?
            .checked_sub(cadmpeg_core::decode::u64_from_index(
                wire.discriminator.len(),
            ))
            .ok_or("value_payload_offsets must follow the discriminator")?;
        let values = wire
            .values
            .into_iter()
            .zip(wire.raw_values)
            .zip(wire.value_source_offsets)
            .map(|((value, raw), source)| Ok((ShiftedScalar::from_wire(value, &raw)?, source)))
            .collect::<Result<Vec<_>, String>>()?;
        let lane = FramedScalarRun::new(
            form,
            offset,
            NonEmpty::from_admitted_vec(values)
                .ok_or("values must contain a sketch scalar atom")?,
        )?;
        if !lane
            .iter()
            .map(|(offset, _, _)| offset)
            .eq(wire.value_payload_offsets)
        {
            return Err("value_payload_offsets must follow the contiguous scalar atoms".into());
        }
        if lane.end() != wire.terminator_payload_offset {
            return Err("terminator_payload_offset must follow the last scalar atom".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            construction_payload: wire.construction_payload,
            ordinal: wire.ordinal,
            lane,
            source_offset: wire.source_offset,
            terminator_source_offset: wire.terminator_source_offset,
        })
    }
}

/// Named sketch payload interval and its ordered framed numeric fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureSketchPayloadNamedRecord {
    /// Globally unique named-record identity.
    id: String,
    /// Owning `SKETCH` operation label.
    operation_label: String,
    /// Reconstructed sketch payload carrying this record.
    construction_payload: String,
    /// Name field opening the retained interval.
    name_field: String,
    /// Ordered scalar fields before the next complete name field.
    scalar_fields: Vec<String>,
    /// Ordered fixed-pair fields before the next complete name field.
    fixed_pairs: Vec<String>,
    /// Mixed scaled shifted-binary64/binary32 pairs contained by this interval in payload order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mixed_pairs: Vec<String>,
    /// Payload-relative offset of the opening name marker.
    payload_start_offset: u64,
    /// Payload-relative exclusive end at the next name or payload boundary.
    payload_end_offset: u64,
}

/// Complete named two-dimensional point in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct FeatureSketchPoint {
    /// Globally unique point identity.
    pub(super) id: String,
    /// Owning `SKETCH` operation label.
    pub(super) operation_label: String,
    /// Name-delimited payload record carrying the point.
    pub(super) named_record: String,
    /// Exact `Point<decimal>` source name.
    pub(super) name: String,
    /// Ordered scalar fields carrying the two coordinates.
    pub(super) scalar_fields: [String; 2],
    /// Ordered native coordinate values.
    pub(super) coordinates: cadmpeg_ir::units::FiniteVector<2>,
}

/// Complete named scaled shifted-binary64 record in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct FeatureSketchFixedPoint {
    /// Globally unique fixed-point identity.
    pub(super) id: String,
    /// Owning `SKETCH` operation label.
    pub(super) operation_label: String,
    /// Name-delimited payload record carrying the pair.
    pub(super) named_record: String,
    /// Exact `Point<positive decimal>` source name.
    pub(super) name: String,
    /// Exact fixed-pair field carrying the two values.
    pub(super) fixed_pair: String,
    /// Ordered values reconstructed from the `30` shifted-binary64 atoms and scaled by `1/4`.
    pub(super) values: [cadmpeg_ir::scalar::FiniteReal; 2],
    /// Absolute source offset of the fixed-pair discriminator.
    pub(super) source_offset: u64,
}

/// Exact same-name point identity within one reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct FeatureSketchPointGroup {
    /// Globally unique point-group identity.
    pub(super) id: String,
    /// Owning `SKETCH` operation label.
    pub(super) operation_label: String,
    /// Exact `Point<positive decimal>` source name.
    pub(super) name: String,
    /// Identical point records in payload order.
    pub(super) points: Vec<String>,
    /// Bit-identical ordered coordinate values.
    pub(super) coordinates: cadmpeg_ir::units::FiniteVector<2>,
}

/// Named two-scalar point object spanning consecutive offset-store blocks.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "OffsetStoreNamedPointWire")]
pub(super) struct OffsetStoreNamedPoint {
    /// Globally unique point-object identity.
    pub(super) id: String,
    /// Exact `Point<positive decimal>` source name.
    name: String,
    /// Minimal consecutive source-block span carrying the object.
    data_blocks: Vec<String>,
    /// Checked scalar atoms and their absolute frame offsets.
    values: [FeatureBinary64ScalarToken; 2],
    /// Absolute source offset of the name frame.
    pub(super) source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct OffsetStoreNamedPointWire {
    /// Globally unique point-object identity.
    id: String,
    /// Exact `Point<positive decimal>` source name.
    name: String,
    /// Minimal consecutive source-block span carrying the object.
    data_blocks: Vec<String>,
    /// Ordered finite native scalar values.
    values: [f64; 2],
    /// Exact shifted-binary64 encodings in scalar order.
    raw_values: [[u8; 8]; 2],
    /// Absolute source offsets of the two scalar markers.
    value_source_offsets: [u64; 2],
    /// Absolute source offset of the name frame.
    source_offset: u64,
}

#[cfg(test)]
impl From<OffsetStoreNamedPoint> for OffsetStoreNamedPointWire {
    fn from(value: OffsetStoreNamedPoint) -> Self {
        Self {
            id: value.id,
            name: value.name,
            data_blocks: value.data_blocks,
            values: value.values.map(|token| token.scalar.value().get()),
            raw_values: value.values.map(|token| token.scalar.raw()),
            value_source_offsets: value.values.map(|token| token.source_offset),
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<OffsetStoreNamedPointWire> for OffsetStoreNamedPoint {
    type Error = String;

    fn try_from(wire: OffsetStoreNamedPointWire) -> Result<Self, Self::Error> {
        let [first, second] = std::array::from_fn::<_, 2, _>(|i| {
            ShiftedBinary64::from_wire(wire.values[i], wire.raw_values[i])
                .map(|scalar| FeatureBinary64ScalarToken {
                    scalar,
                    source_offset: wire.value_source_offsets[i],
                })
                .map_err(|error| format!("values/raw_values[{i}]: {error}"))
        });
        Ok(Self {
            id: wire.id,
            name: wire.name,
            data_blocks: wire.data_blocks,
            values: [first?, second?],
            source_offset: wire.source_offset,
        })
    }
}

/// Exact reuse of one named-point block by a sketch reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureSketchNamedPointBlockUse {
    /// Globally unique block-use identity.
    pub(super) id: String,
    /// Sketch operation carrying the reference.
    pub(super) operation_label: String,
    /// Typed sketch-reference occurrence.
    sketch_reference: String,
    /// Reference order within the sketch field.
    reference_ordinal: u32,
    /// Typed named-point object containing the block.
    named_point: String,
    /// Shared offset-store block.
    data_block: String,
    /// Block position within the named-point span.
    point_block_ordinal: u32,
    /// Absolute source offset of the sketch reference.
    pub(super) source_offset: u64,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSketchNamedPointBlockUse {
    fn decode_cost(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, operation: &'static str) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(&(self.reference_ordinal,self.source_offset,&self.id), ctx, operation)
    }
}

/// Exact predecessor relation between a named point and a sketch construction lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureSketchPrecedingNamedPointUse {
    /// Globally unique predecessor-use identity.
    pub(super) id: String,
    /// Sketch operation carrying the construction lane.
    pub(super) operation_label: String,
    /// First typed sketch-reference occurrence.
    first_sketch_reference: String,
    /// Typed named-point object ending immediately before the construction lane.
    named_point: String,
    /// Complete ordered block span of the named point.
    point_data_blocks: Vec<String>,
    /// First construction block immediately following the point span.
    following_data_block: String,
    /// Absolute source offset of the first sketch reference.
    pub(super) source_offset: u64,
}

/// Exact identity of one solved sketch point across its payload and reference lanes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureSketchPointUseWire")]
pub(super) struct FeatureSketchPointUse {
    pub(super) id: String,
    pub(super) operation_label: String,
    pub(super) references: Vec<FeatureSketchPointUseReference>,
    pub(super) sketch_point_group: String,
    pub(super) named_point: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FeatureSketchPointUseReference {
    pub(super) sketch_reference: String,
    pub(super) block_use: String,
    pub(super) source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureSketchPointUseWire {
    /// Globally unique point-use identity.
    id: String,
    /// Sketch operation carrying both point encodings.
    operation_label: String,
    /// Ordered sketch-reference occurrences addressing the named-point span.
    sketch_references: Vec<String>,
    /// Exact block-use witnesses corresponding to the sketch references.
    block_uses: Vec<String>,
    /// Exact same-name sketch-point group.
    sketch_point_group: String,
    /// Independently framed named-point object addressed by the reference.
    named_point: String,
    /// Absolute source offsets of the sketch references.
    source_offsets: Vec<u64>,
}

#[cfg(test)]
impl From<FeatureSketchPointUse> for FeatureSketchPointUseWire {
    fn from(value: FeatureSketchPointUse) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            sketch_references: value
                .references
                .iter()
                .map(|reference| reference.sketch_reference.clone())
                .collect(),
            block_uses: value
                .references
                .iter()
                .map(|reference| reference.block_use.clone())
                .collect(),
            sketch_point_group: value.sketch_point_group,
            named_point: value.named_point,
            source_offsets: value
                .references
                .iter()
                .map(|reference| reference.source_offset)
                .collect(),
        }
    }
}

impl TryFrom<FeatureSketchPointUseWire> for FeatureSketchPointUse {
    type Error = String;
    fn try_from(wire: FeatureSketchPointUseWire) -> Result<Self, Self::Error> {
        if wire.sketch_references.len() != wire.block_uses.len()
            || wire.sketch_references.len() != wire.source_offsets.len()
        {
            return Err(
                "sketch_references, block_uses, and source_offsets must have equal lengths".into(),
            );
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            sketch_point_group: wire.sketch_point_group,
            named_point: wire.named_point,
            references: wire
                .sketch_references
                .into_iter()
                .zip(wire.block_uses)
                .zip(wire.source_offsets)
                .map(|((sketch_reference, block_use), source_offset)| {
                    FeatureSketchPointUseReference {
                        sketch_reference,
                        block_use,
                        source_offset,
                    }
                })
                .collect(),
        })
    }
}

/// Exact ordered dependency from a sketch point to a datum coordinate system.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum FeatureSketchDatumCsysBlockRelation {
    /// The named-point span and coordinate-system construction address one block.
    Shared {
        /// Block addressed by both the named-point span and construction.
        data_block: String,
    },
    /// The coordinate-system construction begins immediately after the named-point span.
    Consecutive {
        /// Final block in the complete named-point span.
        point_data_block: String,
        /// First block in the coordinate-system construction.
        construction_data_block: String,
    },
}

/// One byte-identical scalar shared by a named sketch point and datum-CSYS payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct FeatureSketchDatumCsysScalarAlias {
    /// Zero-based coordinate within the named sketch point.
    pub(super) sketch_coordinate_ordinal: u8,
    /// Exact datum-CSYS scalar field occupying the same source bytes.
    pub(super) datum_csys_scalar: String,
    /// Absolute source offset of the shared scalar field marker.
    value_source_offset: u64,
}

/// Exact ordered dependency from a sketch point to a datum coordinate system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureSketchDatumCsysDependency {
    /// Globally unique dependency identity.
    pub(super) id: String,
    /// Earlier sketch operation owning the point identity.
    pub(super) sketch_operation_label: String,
    /// Later datum-coordinate-system operation consuming the point block.
    pub(super) datum_csys_operation_label: String,
    /// Exact sketch-point identity witnessing ownership.
    pub(super) sketch_point_use: String,
    /// Exact datum-coordinate-system construction witnessing consumption.
    datum_csys_construction: String,
    /// Exact block relation between the complete point span and construction.
    pub(super) block_relation: FeatureSketchDatumCsysBlockRelation,
    /// Scalar encodings occupying the same source bytes in both typed records.
    pub(super) scalar_aliases: Vec<FeatureSketchDatumCsysScalarAlias>,
    /// Absolute source offset of the first sketch reference witnessing the point identity.
    pub(super) source_offset: u64,
}

/// Ordered object reference carried by a bounded sketch-operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureSketchReference {
    /// Globally unique sketch-reference identity.
    pub(super) id: String,
    /// Owning `SKETCH` operation label.
    pub(super) operation_label: String,
    /// Checked position in the counted field; terminal status is derived.
    #[serde(flatten)]
    pub(super) position: crate::om::sketch_references::SketchReferencePosition,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub(super) token: crate::om::reference_index::ReferenceIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_data_block"
    )]
    pub(super) data_block: Option<String>,
    /// Absolute file offset of the width marker.
    source_offset: u64,
}

/// Ordered construction reference carried by a bounded projected-curve payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureProjectedCurveReference {
    /// Globally unique projected-curve reference identity.
    pub(super) id: String,
    /// Owning `CPROJ` or `CPROJ_CMB` operation label.
    pub(super) operation_label: String,
    /// Zero-based order among the field's non-repeated references.
    pub(super) ordinal: u32,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub(super) token: PayloadIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_data_block"
    )]
    pub(super) data_block: Option<String>,
    /// Absolute file offset of the width marker.
    source_offset: u64,
}

/// Canonical printable string in a reconstructed projected-curve payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureProjectedCurveConstructionString {
    /// Globally unique string identity.
    pub(super) id: String,
    /// Owning `CPROJ` or `CPROJ_CMB` operation label.
    pub(super) operation_label: String,
    /// Reconstructed projected-curve payload carrying the string.
    construction_payload: String,
    /// Zero-based string order within the payload.
    pub(super) ordinal: u32,
    /// Exact printable value.
    value: PrintableString<String>,
    /// Payload-relative offset of the `66 32 03` marker.
    payload_offset: u64,
    /// Absolute source offset of the marker.
    source_offset: u64,
}

/// Exact leading construction header carried by a bounded point-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeaturePointConstructionHeader {
    /// Globally unique point-construction-header identity.
    pub(super) id: String,
    /// Owning `POINT` operation label.
    pub(super) operation_label: String,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub(super) token: crate::om::reference_index::ReferenceIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_data_block"
    )]
    pub(super) data_block: Option<String>,
    /// Serialized header mode.
    pub(super) mode: crate::om::discriminators::PointHeaderMode,
    /// Absolute file offset of the reference width marker.
    source_offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FeatureBinary64ScalarToken {
    scalar: ShiftedBinary64,
    source_offset: u64,
}

/// Ordered construction reference carried by a bounded surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureSurfaceConstructionReference {
    /// Globally unique surface-construction-reference identity.
    pub(super) id: String,
    /// Owning `SKIN`, `Studio Surface`, or `THRU_CURVE` operation label.
    pub(super) operation_label: String,
    /// Zero-based slot order in the exact common envelope.
    pub(super) ordinal: u32,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub(super) token: crate::om::reference_index::PayloadIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_data_block"
    )]
    pub(super) data_block: Option<String>,
    /// Absolute file offset of the width marker.
    source_offset: u64,
}

/// Exact leading construction envelope in a `THRU_CURVE` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureThruCurveConstructionEnvelope {
    /// Globally unique envelope identity.
    id: String,
    /// Owning `THRU_CURVE` operation label.
    operation_label: String,
    /// Nonzero construction discriminator.
    discriminator: NonZeroU8,
    /// Exact opaque controls between the reference groups.
    controls: ThruCurveControls,
    /// Nonzero control following the second reference group.
    trailing_control: NonZeroU8,
    /// Exact two-byte value selected by the `a0` marker.
    trailing_value: [u8; 2],
    /// Absolute source offset of the discriminator.
    source_offset: u64,
}

/// Exact logical payload reconstructed from an ordered surface-construction graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureSurfaceConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub(super) id: String,
    /// Owning `SKIN` or `Studio Surface` operation label.
    pub(super) operation_label: String,
    /// Ordered construction-reference records.
    construction_references: [String; 14],
    /// Ordered source blocks and the hash of their concatenated bytes.
    #[serde(flatten)]
    content: FeaturePayloadContent<[FeaturePayloadBlock; 14]>,
}

/// One printable string frame in a reconstructed surface payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureSurfaceConstructionString {
    /// Globally unique string identity.
    pub(super) id: String,
    /// Owning `SKIN` or `Studio Surface` operation label.
    pub(super) operation_label: String,
    /// Reconstructed surface payload carrying the frame.
    surface_construction_payload: String,
    /// Zero-based string order within the payload.
    pub(super) ordinal: u32,
    /// Exact printable value.
    value: crate::payload_text::PayloadText<String>,
    /// Payload-relative offset of the `66 1b 03` marker.
    payload_offset: u64,
    /// Absolute source offset of the marker.
    source_offset: u64,
}

/// Ordered profile reference carried by a bounded extrusion payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureExtrudeProfileReference {
    /// Globally unique profile-reference identity.
    pub(super) id: String,
    /// Owning `EXTRUDE` operation label.
    pub(super) operation_label: String,
    /// Zero-based profile-reference order.
    pub(super) ordinal: u32,
    /// Field tag serialized before the counted reference list.
    field_tag: u8,
    /// Absolute source offset of the matching duplicate-list index marker.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_witness_source_offset"
    )]
    witness_source_offset: Option<u64>,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub(super) token: crate::om::reference_index::PayloadIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_data_block"
    )]
    pub(super) data_block: Option<String>,
    /// Absolute file offset of the width marker.
    source_offset: u64,
}

/// Fixed shifted-IEEE scalar header from a bounded extrusion payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "FeatureExtrudePayloadHeaderWire")]
pub(super) struct FeatureExtrudePayloadHeader {
    /// Globally unique header identity.
    pub(super) id: String,
    /// Owning `EXTRUDE` operation label.
    pub(super) operation_label: String,
    /// Ordered finite scalar values.
    scalars: [ShiftedBinary64; 2],
    /// Absolute file offset of the first shifted-IEEE scalar.
    source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureExtrudePayloadHeaderWire {
    /// Globally unique header identity.
    id: String,
    /// Owning `EXTRUDE` operation label.
    operation_label: String,
    /// Ordered finite scalar values.
    scalars: [f64; 2],
    /// Exact shifted-binary64 encodings in scalar order.
    raw_scalars: [[u8; 8]; 2],
    /// Absolute file offset of the first shifted-IEEE scalar.
    source_offset: u64,
}

#[cfg(test)]
impl From<FeatureExtrudePayloadHeader> for FeatureExtrudePayloadHeaderWire {
    fn from(value: FeatureExtrudePayloadHeader) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            scalars: value.scalars.map(|scalar| scalar.value().get()),
            raw_scalars: value.scalars.map(ShiftedBinary64::raw),
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeatureExtrudePayloadHeaderWire> for FeatureExtrudePayloadHeader {
    type Error = &'static str;

    fn try_from(wire: FeatureExtrudePayloadHeaderWire) -> Result<Self, Self::Error> {
        let [first, second] = std::array::from_fn::<_, 2, _>(|i| {
            ShiftedBinary64::from_wire(wire.scalars[i], wire.raw_scalars[i])
        });
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            scalars: [first?, second?],
            source_offset: wire.source_offset,
        })
    }
}

/// Ordered member index in a branch-`11` operation body clause.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureOperationBodyMemberWire")]
pub(super) struct FeatureOperationBodyMember {
    /// Globally unique member identity.
    pub(super) id: String,
    /// Owning operation label.
    pub(super) operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub(super) body_reference_ordinal: u32,
    /// Serialized body object index.
    body_object_index: u32,
    /// Zero-based member order in the counted lane.
    pub(super) ordinal: u32,
    /// Exact compact index and its absolute file position.
    member: LocatedCompactIndex<u64>,
}

#[derive(Serialize, Deserialize)]
struct FeatureOperationBodyMemberWire {
    /// Globally unique member identity.
    id: String,
    /// Owning operation label.
    operation_label: String,
    /// Zero-based body-reference occurrence order.
    body_reference_ordinal: u32,
    /// Serialized body object index.
    body_object_index: u32,
    /// Zero-based member order in the counted lane.
    ordinal: u32,
    /// Decoded compact index.
    member_index: u32,
    /// Exact compact-index token.
    raw_member_index: Vec<u8>,
    /// Absolute file offset of the compact-index marker.
    source_offset: u64,
}

#[cfg(test)]
impl From<FeatureOperationBodyMember> for FeatureOperationBodyMemberWire {
    fn from(value: FeatureOperationBodyMember) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            body_reference_ordinal: value.body_reference_ordinal,
            body_object_index: value.body_object_index,
            ordinal: value.ordinal,
            member_index: value.member.atom.value(),
            raw_member_index: value.member.atom.raw().to_vec(),
            source_offset: value.member.offset,
        }
    }
}

impl TryFrom<FeatureOperationBodyMemberWire> for FeatureOperationBodyMember {
    type Error = String;
    fn try_from(wire: FeatureOperationBodyMemberWire) -> Result<Self, Self::Error> {
        let atom = CompactIndexAtom::from_wire(wire.member_index, &wire.raw_member_index)
            .map_err(|error| format!("operation body member member_index: {error}"))?;
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            body_reference_ordinal: wire.body_reference_ordinal,
            body_object_index: wire.body_object_index,
            ordinal: wire.ordinal,
            member: LocatedCompactIndex {
                atom,
                offset: wire.source_offset,
            },
        })
    }
}

/// Wrapped operation member resolved in the feature-body identity namespace.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureOperationBodyOperandWire")]
pub(super) struct FeatureOperationBodyOperand {
    /// Globally unique operand identity.
    pub(super) id: String,
    /// Owning operation label.
    pub(super) operation_label: String,
    /// Body clause containing the operand.
    pub(super) body_object_index: u32,
    /// Zero-based body-reference occurrence order.
    pub(super) body_reference_ordinal: u32,
    /// Zero-based operand order in the wrapped member lane.
    pub(super) ordinal: u32,
    /// Exact operand compact index and its absolute file position.
    pub(super) operand: LocatedCompactIndex<u64>,
    /// Same-store offset data block named by the operand, when resolved.
    pub(super) operand_data_block: Option<String>,
    /// Segment body bindings naming the same body image.
    pub(super) segment_body_bindings: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct FeatureOperationBodyOperandWire {
    /// Globally unique operand identity.
    id: String,
    /// Owning operation label.
    operation_label: String,
    /// Body clause containing the operand.
    body_object_index: u32,
    /// Zero-based body-reference occurrence order.
    body_reference_ordinal: u32,
    /// Zero-based operand order in the wrapped member lane.
    ordinal: u32,
    /// Serialized operand body object index.
    operand_object_index: u32,
    /// Exact serialized compact-index token.
    raw_operand_object_index: Vec<u8>,
    /// Same-store offset data block named by the operand, when resolved.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_operand_data_block"
    )]
    operand_data_block: Option<String>,
    /// Segment body bindings naming the same body image.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    segment_body_bindings: Vec<String>,
    /// Absolute file offset of the compact-index marker.
    source_offset: u64,
}

#[cfg(test)]
impl From<FeatureOperationBodyOperand> for FeatureOperationBodyOperandWire {
    fn from(value: FeatureOperationBodyOperand) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            body_object_index: value.body_object_index,
            body_reference_ordinal: value.body_reference_ordinal,
            ordinal: value.ordinal,
            operand_object_index: value.operand.atom.value(),
            raw_operand_object_index: value.operand.atom.raw().to_vec(),
            operand_data_block: value.operand_data_block,
            segment_body_bindings: value.segment_body_bindings,
            source_offset: value.operand.offset,
        }
    }
}

impl TryFrom<FeatureOperationBodyOperandWire> for FeatureOperationBodyOperand {
    type Error = String;
    fn try_from(wire: FeatureOperationBodyOperandWire) -> Result<Self, Self::Error> {
        let atom =
            CompactIndexAtom::from_wire(wire.operand_object_index, &wire.raw_operand_object_index)
                .map_err(|error| format!("operation body operand operand_object_index: {error}"))?;
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            body_object_index: wire.body_object_index,
            body_reference_ordinal: wire.body_reference_ordinal,
            ordinal: wire.ordinal,
            operand: LocatedCompactIndex {
                atom,
                offset: wire.source_offset,
            },
            operand_data_block: wire.operand_data_block,
            segment_body_bindings: wire.segment_body_bindings,
        })
    }
}

/// Exact continuation following a `TRIM BODY` branch-`11` member lane.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "reference::Body11ContinuationWire")]
pub(super) struct FeatureOperationBody11Continuation {
    /// Globally unique continuation identity.
    pub(super) id: String,
    /// Owning operation label.
    pub(super) operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub(super) body_reference_ordinal: u32,
    /// Serialized body object index.
    body_object_index: u32,
    /// Exact compact continuation index and its absolute file offset.
    continuation: crate::om::compact::LocatedCompactIndex<u64>,
    /// Exact required terminal reference.
    terminal: crate::om::reference_index::ReferenceIndexToken,
    /// Absolute file offset of the terminal object-index marker.
    terminal_source_offset: u64,
}

/// Homogeneous value encoding in an operation body-reference lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FeatureOperationBodyReferenceLaneEncoding {
    /// NX OM compact-index encoding.
    CompactIndex,
    /// `f0`/`f1` payload object-index encoding.
    PayloadObjectIndex,
}

/// A homogeneous operation body lane with checked grammar-specific tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FeatureOperationBodyReferences {
    CompactIndex(Vec<ConstructionReference<Option<String>, CompactIndexAtom>>),
    PayloadObjectIndex(Vec<ConstructionReference<Option<String>, PayloadIndexToken>>),
}

/// Counted reference lane following an operation body scalar clause.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureOperationBodyReferenceLaneWire")]
pub(super) struct FeatureOperationBodyReferenceLane {
    pub(super) id: String,
    pub(super) operation_label: String,
    pub(super) body_reference_ordinal: u32,
    body_object_index: u32,
    branch: crate::om::discriminators::OperationBodyReferenceBranch,
    references: FeatureOperationBodyReferences,
}

#[derive(Serialize, Deserialize)]
struct FeatureOperationBodyReferenceLaneWire {
    /// Globally unique lane identity.
    id: String,
    /// Owning operation label.
    operation_label: String,
    /// Zero-based body-reference occurrence order.
    body_reference_ordinal: u32,
    /// Serialized body object index.
    body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    branch: crate::om::discriminators::OperationBodyReferenceBranch,
    /// Homogeneous encoding used by every lane value.
    encoding: FeatureOperationBodyReferenceLaneEncoding,
    /// Ordered decoded indices.
    object_indices: Vec<u32>,
    /// Exact encoded index tokens in lane order.
    raw_object_indices: Vec<Vec<u8>>,
    /// Unique offset-only data blocks addressed by the ordered indices.
    data_blocks: Vec<Option<String>>,
    /// Absolute file offsets of the encoded index markers.
    source_offsets: Vec<u64>,
}

#[cfg(test)]
impl From<FeatureOperationBodyReferenceLane> for FeatureOperationBodyReferenceLaneWire {
    fn from(value: FeatureOperationBodyReferenceLane) -> Self {
        let mut object_indices = Vec::new();
        let mut raw_object_indices = Vec::new();
        let mut data_blocks = Vec::new();
        let mut source_offsets = Vec::new();
        let mut push = |index, raw, data_block, offset| {
            object_indices.push(index);
            raw_object_indices.push(raw);
            data_blocks.push(data_block);
            source_offsets.push(offset);
        };
        let encoding = match value.references {
            FeatureOperationBodyReferences::CompactIndex(references) => {
                for reference in references {
                    push(
                        reference.token.value(),
                        reference.token.raw().to_vec(),
                        reference.data_block,
                        reference.source_offset,
                    );
                }
                FeatureOperationBodyReferenceLaneEncoding::CompactIndex
            }
            FeatureOperationBodyReferences::PayloadObjectIndex(references) => {
                for reference in references {
                    push(
                        reference.token.value(),
                        reference.token.raw().to_vec(),
                        reference.data_block,
                        reference.source_offset,
                    );
                }
                FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex
            }
        };
        Self {
            id: value.id,
            operation_label: value.operation_label,
            body_reference_ordinal: value.body_reference_ordinal,
            body_object_index: value.body_object_index,
            branch: value.branch,
            encoding,
            object_indices,
            raw_object_indices,
            data_blocks,
            source_offsets,
        }
    }
}

impl TryFrom<FeatureOperationBodyReferenceLaneWire> for FeatureOperationBodyReferenceLane {
    type Error = String;
    fn try_from(wire: FeatureOperationBodyReferenceLaneWire) -> Result<Self, Self::Error> {
        let count = wire.object_indices.len();
        if wire.raw_object_indices.len() != count
            || wire.data_blocks.len() != count
            || wire.source_offsets.len() != count
        {
            return Err("object_indices, raw_object_indices, data_blocks, and source_offsets must have equal lengths".into());
        }
        let entries = wire
            .object_indices
            .into_iter()
            .zip(wire.raw_object_indices)
            .zip(wire.data_blocks)
            .zip(wire.source_offsets)
            .enumerate();
        let references = match wire.encoding {
            FeatureOperationBodyReferenceLaneEncoding::CompactIndex => {
                FeatureOperationBodyReferences::CompactIndex(
                    entries
                        .map(|(slot, (((value, raw), data_block), source_offset))| {
                            Ok(ConstructionReference {
                                token: CompactIndexAtom::from_wire(value, &raw).map_err(
                                    |error| {
                                        format!(
                                            "object_indices/raw_object_indices[{slot}]: {error}"
                                        )
                                    },
                                )?,
                                data_block,
                                source_offset,
                            })
                        })
                        .collect::<Result<_, String>>()?,
                )
            }
            FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex => {
                FeatureOperationBodyReferences::PayloadObjectIndex(
                    entries
                        .map(|(slot, (((value, raw), data_block), source_offset))| {
                            Ok(ConstructionReference {
                                token: PayloadIndexToken::from_wire(value, &raw).map_err(
                                    |error| {
                                        format!(
                                            "object_indices/raw_object_indices[{slot}]: {error}"
                                        )
                                    },
                                )?,
                                data_block,
                                source_offset,
                            })
                        })
                        .collect::<Result<_, String>>()?,
                )
            }
        };
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            body_reference_ordinal: wire.body_reference_ordinal,
            body_object_index: wire.body_object_index,
            branch: wire.branch,
            references,
        })
    }
}

/// Atomically witnessed extrusion construction profile.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureExtrudeConstructionProfileWire")]
pub(super) struct FeatureExtrudeConstructionProfile {
    pub(super) id: String,
    pub(super) operation_label: String,
    references: Vec<FeatureExtrudeConstructionProfileReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FeatureExtrudeConstructionProfileReference {
    object_index: u32,
    data_block: String,
    profile_source_offset: u64,
    witness_source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureExtrudeConstructionProfileWire {
    /// Globally unique construction-profile identity.
    id: String,
    /// Owning `EXTRUDE` operation label.
    operation_label: String,
    /// Ordered serialized profile object indices.
    object_indices: Vec<u32>,
    /// Ordered uniquely resolved profile data blocks.
    data_blocks: Vec<String>,
    /// Source offsets from the independently encoded profile field.
    profile_source_offsets: Vec<u64>,
    /// Source offsets from the independently encoded duplicate list.
    witness_source_offsets: Vec<u64>,
}

#[cfg(test)]
impl From<FeatureExtrudeConstructionProfile> for FeatureExtrudeConstructionProfileWire {
    fn from(value: FeatureExtrudeConstructionProfile) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            object_indices: value
                .references
                .iter()
                .map(|reference| reference.object_index)
                .collect(),
            data_blocks: value
                .references
                .iter()
                .map(|reference| reference.data_block.clone())
                .collect(),
            profile_source_offsets: value
                .references
                .iter()
                .map(|reference| reference.profile_source_offset)
                .collect(),
            witness_source_offsets: value
                .references
                .iter()
                .map(|reference| reference.witness_source_offset)
                .collect(),
        }
    }
}

impl TryFrom<FeatureExtrudeConstructionProfileWire> for FeatureExtrudeConstructionProfile {
    type Error = String;
    fn try_from(wire: FeatureExtrudeConstructionProfileWire) -> Result<Self, Self::Error> {
        let count = wire.object_indices.len();
        if wire.data_blocks.len() != count
            || wire.profile_source_offsets.len() != count
            || wire.witness_source_offsets.len() != count
        {
            return Err("object_indices, data_blocks, profile_source_offsets, and witness_source_offsets must have equal lengths".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            references: wire
                .object_indices
                .into_iter()
                .zip(wire.data_blocks)
                .zip(wire.profile_source_offsets)
                .zip(wire.witness_source_offsets)
                .map(
                    |(
                        ((object_index, data_block), profile_source_offset),
                        witness_source_offset,
                    )| FeatureExtrudeConstructionProfileReference {
                        object_index,
                        data_block,
                        profile_source_offset,
                        witness_source_offset,
                    },
                )
                .collect(),
        })
    }
}

/// Completely resolved construction-reference field of one `BLOCK` feature.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureBlockConstructionWire")]
pub(super) struct FeatureBlockConstruction {
    /// Globally unique construction identity.
    pub(super) id: String,
    /// Owning `BLOCK` operation label.
    pub(super) operation_label: String,
    /// Payload control byte preceding the construction field.
    control: u8,
    /// Ordered references and their uniquely resolved source blocks.
    members: [FeatureConstructionMember; 18],
    /// Reference following the separator.
    terminal_reference: String,
    /// Uniquely resolved terminal block.
    terminal_data_block: String,
}

#[derive(Serialize, Deserialize)]
struct FeatureBlockConstructionWire {
    id: String,
    operation_label: String,
    control: u8,
    member_references: Vec<String>,
    member_data_blocks: Vec<String>,
    terminal_reference: String,
    terminal_data_block: String,
}

#[cfg(test)]
impl From<FeatureBlockConstruction> for FeatureBlockConstructionWire {
    fn from(value: FeatureBlockConstruction) -> Self {
        let (member_references, member_data_blocks) = value
            .members
            .into_iter()
            .map(|member| (member.reference, member.data_block))
            .unzip();
        Self {
            id: value.id,
            operation_label: value.operation_label,
            control: value.control,
            member_references,
            member_data_blocks,
            terminal_reference: value.terminal_reference,
            terminal_data_block: value.terminal_data_block,
        }
    }
}

impl TryFrom<FeatureBlockConstructionWire> for FeatureBlockConstruction {
    type Error = String;
    fn try_from(wire: FeatureBlockConstructionWire) -> Result<Self, Self::Error> {
        if wire.member_references.len() != wire.member_data_blocks.len() {
            return Err(
                "member_references and member_data_blocks must have equal lengths".to_owned(),
            );
        }
        let members = wire
            .member_references
            .into_iter()
            .zip(wire.member_data_blocks)
            .map(|(reference, data_block)| FeatureConstructionMember {
                reference,
                data_block,
            })
            .collect::<Vec<_>>();
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            control: wire.control,
            members: members
                .try_into()
                .map_err(|_| "member_references must contain eighteen entries".to_owned())?,
            terminal_reference: wire.terminal_reference,
            terminal_data_block: wire.terminal_data_block,
        })
    }
}

/// Name-delimited interval in a reconstructed `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FeatureBlockPayloadNamedRecord {
    /// Globally unique interval identity.
    id: String,
    /// Owning `BLOCK` operation label.
    operation_label: String,
    /// Reconstructed payload containing the interval.
    construction_payload: String,
    /// Name field opening the interval.
    name_field: String,
    /// Complete scalar fields in payload order within the interval.
    scalar_fields: Vec<String>,
    /// Inclusive payload-relative start.
    payload_start_offset: u64,
    /// Exclusive payload-relative end.
    payload_end_offset: u64,
}

/// Exactly two-scalar `Point<positive decimal>` record in a `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct FeatureBlockPayloadPoint {
    /// Globally unique typed-point identity.
    pub(super) id: String,
    /// Owning `BLOCK` operation label.
    pub(super) operation_label: String,
    /// Name-delimited payload interval carrying the point.
    named_record: String,
    /// Exact `Point<positive decimal>` source name.
    name: String,
    /// Ordered scalar fields carrying the two coordinates.
    scalar_fields: [String; 2],
    /// Ordered native coordinate values.
    coordinates: cadmpeg_ir::units::FiniteVector<2>,
}

/// Exact same-name point identity within one reconstructed `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct FeatureBlockPayloadPointGroup {
    /// Globally unique point-group identity.
    pub(super) id: String,
    /// Owning `BLOCK` operation label.
    pub(super) operation_label: String,
    /// Exact `Point<positive decimal>` source name.
    name: String,
    /// Identical point records in payload order.
    points: Vec<String>,
    /// Bit-identical ordered coordinate values.
    coordinates: cadmpeg_ir::units::FiniteVector<2>,
}

/// Ordered three-parameter dimension run of one `BLOCK` feature.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(from = "FeatureBlockDimensionsWire")]
pub(super) struct FeatureBlockDimensions {
    pub(super) id: String,
    pub(super) operation_label: String,
    pub(super) construction: String,
    pub(super) anchor_bindings: Vec<String>,
    pub(super) dimensions: [FeatureBlockDimension; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct FeatureBlockDimension {
    pub(super) declaration: String,
    pub(super) expression: String,
    pub(super) value: cadmpeg_ir::scalar::FiniteReal,
}

#[derive(Serialize, Deserialize)]
struct FeatureBlockDimensionsWire {
    /// Globally unique dimension-set identity.
    id: String,
    /// Owning `BLOCK` operation label.
    operation_label: String,
    /// Complete resolved block construction.
    construction: String,
    /// Bindings selecting the first parameter declaration.
    anchor_bindings: Vec<String>,
    /// Ordered consecutive parameter declarations.
    declarations: [String; 3],
    /// Ordered exact numeric expression records.
    expressions: [String; 3],
    /// Ordered dimensions in model millimeters.
    values: [cadmpeg_ir::scalar::FiniteReal; 3],
}

#[cfg(test)]
impl From<FeatureBlockDimensions> for FeatureBlockDimensionsWire {
    fn from(dimensions: FeatureBlockDimensions) -> Self {
        Self {
            id: dimensions.id,
            operation_label: dimensions.operation_label,
            construction: dimensions.construction,
            anchor_bindings: dimensions.anchor_bindings,
            declarations: dimensions
                .dimensions
                .each_ref()
                .map(|dimension| dimension.declaration.clone()),
            expressions: dimensions
                .dimensions
                .each_ref()
                .map(|dimension| dimension.expression.clone()),
            values: dimensions
                .dimensions
                .each_ref()
                .map(|dimension| dimension.value),
        }
    }
}

impl From<FeatureBlockDimensionsWire> for FeatureBlockDimensions {
    fn from(wire: FeatureBlockDimensionsWire) -> Self {
        Self {
            id: wire.id,
            operation_label: wire.operation_label,
            construction: wire.construction,
            anchor_bindings: wire.anchor_bindings,
            dimensions: std::array::from_fn(|slot| FeatureBlockDimension {
                declaration: wire.declarations[slot].clone(),
                expression: wire.expressions[slot].clone(),
                value: wire.values[slot],
            }),
        }
    }
}

/// Feature-history Boolean operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FeatureBooleanKind {
    /// Add tool bodies to the target.
    Unite,
    /// Remove tool bodies from the target.
    Subtract,
    /// Retain target/tool intersections.
    Intersect,
}

/// Ordered target/tool binding from a feature-history Boolean operation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureBooleanOperationWire")]
pub(super) struct FeatureBooleanOperation {
    pub(super) id: String,
    pub(super) operation_label: String,
    pub(super) kind: FeatureBooleanKind,
    pub(super) target:
        crate::om::PayloadObjectReference<crate::om::reference_index::ReferenceIndexToken, u64>,
    pub(super) tools: Vec<
        crate::om::PayloadObjectReference<crate::om::reference_index::ReferenceIndexToken, u64>,
    >,
    pub(super) source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureBooleanOperationWire {
    /// Globally unique Boolean identity.
    id: String,
    /// Owning operation-label identity.
    operation_label: String,
    /// Boolean operation kind.
    kind: FeatureBooleanKind,
    /// Object index of the target body.
    target_object_index: u32,
    /// Exact serialized target object-index token.
    raw_target_object_index: Vec<u8>,
    /// Absolute file offset of the target object-index token.
    target_source_offset: u64,
    /// Ordered object indices of the tool bodies.
    tool_object_indices: Vec<u32>,
    /// Exact serialized tool object-index tokens in tool order.
    raw_tool_object_indices: Vec<Vec<u8>>,
    /// Absolute file offsets of the tool object-index tokens in tool order.
    tool_source_offsets: Vec<u64>,
    /// Absolute file offset of the operation label tag.
    source_offset: u64,
}

#[cfg(test)]
impl From<FeatureBooleanOperation> for FeatureBooleanOperationWire {
    fn from(operation: FeatureBooleanOperation) -> Self {
        Self {
            id: operation.id,
            operation_label: operation.operation_label,
            kind: operation.kind,
            target_object_index: operation.target.token.value(),
            raw_target_object_index: operation.target.token.raw().to_vec(),
            target_source_offset: operation.target.offset,
            tool_object_indices: operation
                .tools
                .iter()
                .map(|token| token.token.value())
                .collect(),
            raw_tool_object_indices: operation
                .tools
                .iter()
                .map(|token| token.token.raw().to_vec())
                .collect(),
            tool_source_offsets: operation.tools.iter().map(|token| token.offset).collect(),
            source_offset: operation.source_offset,
        }
    }
}

impl TryFrom<FeatureBooleanOperationWire> for FeatureBooleanOperation {
    type Error = String;

    fn try_from(wire: FeatureBooleanOperationWire) -> Result<Self, Self::Error> {
        if wire.tool_object_indices.len() != wire.raw_tool_object_indices.len()
            || wire.tool_object_indices.len() != wire.tool_source_offsets.len()
        {
            return Err("Boolean tool columns must have equal lengths".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            kind: wire.kind,
            target: crate::om::PayloadObjectReference {
                token: crate::om::reference_index::ReferenceIndexToken::from_wire(
                    wire.target_object_index,
                    &wire.raw_target_object_index,
                )
                .map_err(|error| format!("target_object_index: {error}"))?,
                offset: wire.target_source_offset,
            },
            tools: wire
                .tool_object_indices
                .into_iter()
                .zip(wire.raw_tool_object_indices)
                .zip(wire.tool_source_offsets)
                .map(|((value, raw), offset)| {
                    Ok(crate::om::PayloadObjectReference {
                        token: crate::om::reference_index::ReferenceIndexToken::from_wire(
                            value, &raw,
                        )
                        .map_err(|error| format!("tool_object_indices: {error}"))?,
                        offset,
                    })
                })
                .collect::<Result<_, String>>()?,
            source_offset: wire.source_offset,
        })
    }
}

fn feature_history_sections(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<SegmentOmLink>, CodecError> {
    canonical_feature_history_links(ctx, segment_om_links(ctx, container)?)
}

fn visit_feature_history_operation_records(
    ctx: &DecodeContext<'_>,
    container: &Container,
    mut visit: impl FnMut(
        &crate::om::Section<'_>,
        &str,
        u64,
        usize,
        crate::om::operation_record::OperationRecord<'_>,
    ),
) -> Result<(), CodecError> {
    visit_feature_history_sections(ctx, container, |section, key, entry_offset| {
        for (ordinal, record) in section.operation_records_with_label_ordinals(ctx)? {
            visit(section, key, entry_offset, ordinal, record);
        }
        Ok(())
    })
}

fn visit_feature_history_unlabeled_operation_records(
    ctx: &DecodeContext<'_>,
    container: &Container,
    mut visit: impl FnMut(
        &crate::om::Section<'_>,
        &str,
        u64,
        usize,
        crate::om::UnlabeledOperationRecord<'_>,
    ) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    visit_feature_history_sections(ctx, container, |section, key, entry_offset| {
        for (ordinal, record) in section.unlabeled_operation_records_with_ordinals(ctx)? {
            visit(section, key, entry_offset, ordinal, record)?;
        }
        Ok(())
    })
}

pub(super) fn canonical_feature_history_links(
    ctx: &DecodeContext<'_>,
    mut links: Vec<SegmentOmLink>,
) -> Result<Vec<SegmentOmLink>, CodecError> {
    links.retain(|link| link.schema_role == OmSchemaRole::FeatureHistory);
    ctx.stable_sort_by(
        &mut links,
            |value| value,
            |first, second| {
            first
                .location
                .section_offset()
                .cmp(&second.location.section_offset())
                .then_with(|| {
                    first
                        .location
                        .source_offset()
                        .cmp(&second.location.source_offset())
                })
                .then_with(|| first.id.cmp(&second.id))
        },
        "sort NX feature history links",
    )?;
    links.dedup_by_key(|link| link.location.section_offset());
    Ok(links)
}

/// Return unique content-backed identities for the offset-store ordinals used
/// by operation-header slots.
///
/// A slot ordinal is local to one offset store and can drift when a record is
/// inserted. The map is usable only when the ordinal resolves to exactly one
/// column block across all offset stores and that block has a unique
/// content-backed identity. An ambiguous or duplicate block remains absent.
fn operation_header_block_identities(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<BTreeMap<u32, Option<String>>, CodecError> {
    let mut identities = BTreeMap::<u32, Option<String>>::new();
    for block in data_blocks(ctx, container)? {
        if block.role == DataBlockRole::Column {
            ctx.admit_btree_entry(
                &identities,
                &block.block_ordinal,
                "NX operation block identity ordinals",
            )?;
            match identities.entry(block.block_ordinal) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.charge_retained(
                        cadmpeg_core::decode::u64_from_index(
                            std::mem::size_of::<(u32, Option<String>)>() * 4,
                        ),
                        "retain NX operation block identity ordinals",
                    )?;
                    entry.insert(block.stable_identity);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
    }
    Ok(identities)
}

/// Return the record-order-independent identity encoded by one operation
/// header.
///
/// Every non-null slot must resolve through the complete offset-store map.
/// An all-null tuple, an unresolved slot, or a duplicated content identity has
/// no operation identity witness.
fn operation_header_identity_key(
    ctx: &DecodeContext<'_>,
    object_indices: [Option<u32>; 4],
    block_identities: &BTreeMap<u32, Option<String>>,
) -> Result<Option<String>, CodecError> {
    if !object_indices.iter().any(Option::is_some) {
        return Ok(None);
    }
    let mut slots = ["null"; 4];
    for (slot, index) in slots.iter_mut().zip(object_indices) {
        if let Some(index) = index {
            let Some(Some(identity)) = block_identities.get(&index) else {
                return Ok(None);
            };
            *slot = identity;
        }
    }
    let prefix = "nx:feature-history:operation-header-identity#content:";
    let length = slots
        .iter()
        .try_fold(prefix.len() + 3, |length, slot| {
            length.checked_add(slot.len())
        })
        .ok_or_else(|| ctx.refuse_codec_limit("retain NX operation header identity", 0, 1))?;
    let mut identity = ctx.retained_string(length, "retain NX operation header identity")?;
    identity.push_str(prefix);
    identity.push_str(slots[0]);
    for slot in slots.iter().skip(1) {
        identity.push('-');
        identity.push_str(slot);
    }
    Ok(Some(identity))
}

fn assign_operation_header_identities(
    ctx: &DecodeContext<'_>,
    labels: &mut [FeatureOperationLabel],
    block_identities: &BTreeMap<u32, Option<String>>,
) -> Result<(), CodecError> {
    let mut keys_guard = ctx.reserve_scoped(0, "reserve NX operation header keys")?;
    let mut keys =
        keys_guard.with_storage(|| ctx.collection_vec(labels.len(), "NX operation header keys"))?;
    for label in labels.iter() {
        keys.push(operation_header_identity_key(
            ctx,
            label.objects.values(),
            block_identities,
        )?);
    }

    let mut counts_guard = ctx.reserve_scoped(0, "reserve NX operation header counts")?;
    let mut counts = BTreeMap::<String, usize>::new();
    for key in keys.iter().flatten() {
        if !counts.contains_key(key.as_str()) {
            let mut copy = String::new();
            counts_guard.with_storage(|| {
                ctx.try_reserve_retained_text(
                    &mut copy,
                    key.len(),
                    "allocate NX operation header count key",
                )
            })?;
            copy.push_str(key);
            counts_guard.with_storage(|| {
                ctx.insert_btree_map(&mut counts, copy, 0, "NX operation header counts")
            })?;
        }
        let count = counts
            .get_mut(key.as_str())
            .ok_or_else(|| ctx.refuse_codec_limit("NX operation header count key", 0, 1))?;
        *count = count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("count NX operation headers", 0, 1))?;
    }
    for (label, key) in labels.iter_mut().zip(keys) {
        label.stable_identity = key.filter(|key| counts.get(key.as_str()) == Some(&1));
    }
    Ok(())
}

fn format_feature_history_id(
    ctx: &DecodeContext<'_>,
    kind: &'static str,
    section_key: &str,
    operation_ordinal: usize,
    subordinal: Option<usize>,
) -> Result<String, CodecError> {
    fn decimal_width(mut value: usize) -> usize {
        let mut digits = 1;
        while value >= 10 {
            value /= 10;
            digits += 1;
        }
        digits.max(10)
    }
    let prefix = "nx:feature-history:";
    let mut length = prefix
        .len()
        .checked_add(kind.len())
        .and_then(|length| length.checked_add(1))
        .and_then(|length| length.checked_add(section_key.len()))
        .and_then(|length| length.checked_add(1))
        .and_then(|length| length.checked_add(decimal_width(operation_ordinal)))
        .ok_or_else(|| ctx.refuse_codec_limit("retain NX feature history identity", 0, 1))?;
    if let Some(subordinal) = subordinal {
        length = length
            .checked_add(1)
            .and_then(|length| length.checked_add(decimal_width(subordinal)))
            .ok_or_else(|| ctx.refuse_codec_limit("retain NX feature history identity", 0, 1))?;
    }
    let mut id = ctx.retained_string(length, "retain NX feature history identity")?;
    write!(
        &mut id,
        "{prefix}{kind}#{section_key}-{operation_ordinal:010}"
    )
    .map_err(|_| ctx.refuse_codec_limit("format NX feature history identity", 0, 1))?;
    if let Some(subordinal) = subordinal {
        write!(&mut id, "-{subordinal:010}")
            .map_err(|_| ctx.refuse_codec_limit("format NX feature history identity", 0, 1))?;
    }
    Ok(id)
}

fn format_feature_child_id(
    ctx: &DecodeContext<'_>,
    parent: &str,
    suffix: &'static str,
    ordinal: usize,
) -> Result<String, CodecError> {
    let digits = match ordinal.checked_ilog10() {
        Some(digits) => usize::try_from(digits)
            .map_err(|_| ctx.refuse_codec_limit("NX feature child identity width", 0, 1))?
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("NX feature child identity width", 0, 1))?,
        None => 1,
    }
    .max(10);
    let length = parent
        .len()
        .checked_add(suffix.len())
        .and_then(|length| length.checked_add(digits))
        .ok_or_else(|| ctx.refuse_codec_limit("NX feature child identity", 0, 1))?;
    let mut id = ctx.retained_string(length, "NX feature child identity")?;
    write!(&mut id, "{parent}{suffix}{ordinal:010}")
        .map_err(|_| ctx.refuse_codec_limit("write NX feature child identity", 0, 1))?;
    Ok(id)
}

/// Decode ordered operation labels from feature-history record areas.
pub(super) fn feature_operation_labels(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureOperationLabel>, cadmpeg_core::CodecError> {
    let sections = container.om_sections(ctx)?;
    let block_identities = operation_header_block_identities(ctx, container)?;
    let mut labels = Vec::new();
    for (section_ordinal, link) in feature_history_sections(ctx, container)?
        .into_iter()
        .enumerate()
    {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(sections.len()),
            "match NX feature label section",
        )?;
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry.file_span().map_or(
                Some(cadmpeg_core::decode::u64_from_index(section.offset)),
                |(offset, _)| {
                    offset.checked_add(cadmpeg_core::decode::u64_from_index(section.offset))
                },
            ) == Some(link.location.section_offset())
        }) else {
            continue;
        };
        let section_key_len = section_ordinal
            .checked_ilog10()
            .map_or(1, |digits| cadmpeg_core::decode::index_from_u32(digits) + 1)
            .max(10);
        let mut section_key_guard = ctx.reserve_scoped(0, "NX feature label section key")?;
        let mut section_key = String::new();
        section_key_guard.with_storage(|| {
            ctx.try_reserve_retained_text(
                &mut section_key,
                section_key_len,
                "allocate NX feature label section key",
            )
        })?;
        write!(&mut section_key, "{section_ordinal:010}")
            .map_err(|_| ctx.refuse_codec_limit("write NX feature label section key", 0, 1))?;
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let records = section.operation_records_with_label_ordinals(ctx)?;
        ctx.reserve_vec(&mut labels, records.len(), "NX feature operation labels")?;
        for (ordinal, record) in records {
            let label = record.label();
            let ordinal_u32 = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX feature operation ordinal", 0, 1))?;
            let id =
                format_feature_history_id(ctx, "operation-label", &section_key, ordinal, None)?;
            let source_offset = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(
                    label.header.end_offset(),
                ))
                .ok_or_else(|| ctx.refuse_codec_limit("NX feature operation label offset", 0, 1))?;
            labels.push(FeatureOperationLabel {
                id,
                section_link: ctx
                    .copy_retained_text(&link.id, "retain NX feature operation section link")?,
                ordinal: ordinal_u32,
                value: ctx
                    .copy_retained_text(label.value, "retain NX feature operation label text")?,
                objects: label.header.objects(),
                stable_identity: None,
                source_offset,
            });
        }
    }
    assign_operation_header_identities(ctx, &mut labels, &block_identities)?;
    Ok(labels)
}

/// Decode ordered Boolean target/tool bindings from feature-history sections.
pub(super) fn feature_boolean_operations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureBooleanOperation>, cadmpeg_core::CodecError> {
    let mut operations = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let candidates = match section.boolean_operations(ctx) {
                Ok(candidates) => candidates,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let Some(operation) = candidates
                .into_iter()
                .find(|operation| operation.offset == record.label().header.end_offset())
            else {
                return;
            };
            let kind = match operation.kind {
                crate::om::BooleanOperationKind::Unite => FeatureBooleanKind::Unite,
                crate::om::BooleanOperationKind::Subtract => FeatureBooleanKind::Subtract,
                crate::om::BooleanOperationKind::Intersect => FeatureBooleanKind::Intersect,
            };
            let item = (|| -> Result<FeatureBooleanOperation, CodecError> {
                let mut tools = Vec::new();
                for tool in operation.tools {
                    ctx.reserve_vec(&mut tools, 1, "NX Boolean tool references")?;
                    tools.push(crate::om::PayloadObjectReference {
                        token: tool.token,
                        offset: entry_offset + cadmpeg_core::decode::u64_from_index(tool.offset),
                    });
                }
                Ok(FeatureBooleanOperation {
                    id: format_feature_history_id(
                        ctx,
                        "boolean",
                        section_key,
                        operation_ordinal,
                        None,
                    )?,
                    operation_label: format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?,
                    kind,
                    target: crate::om::PayloadObjectReference {
                        token: operation.target.token,
                        offset: entry_offset
                            + cadmpeg_core::decode::u64_from_index(operation.target.offset),
                    },
                    tools,
                    source_offset: entry_offset
                        + cadmpeg_core::decode::u64_from_index(operation.offset),
                })
            })();
            let item = match item {
                Ok(item) => item,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            if let Err(error) = ctx
                .charge_collection_items(1, "NX Boolean operations")
                .and_then(|()| {
                    ctx.reserve_capacity(&mut operations, 1, "allocate NX Boolean operations")
                })
            {
                failure = Some(error);
                return;
            }
            operations.push(item);
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(operations)
}

/// Decode exact feature-operation record boundaries and byte identities.
pub(super) fn feature_operation_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureOperationRecord>, cadmpeg_core::CodecError> {
    let block_identities = operation_header_block_identities(ctx, container)?;
    let mut identity_counts = BTreeMap::<String, usize>::new();
    let mut counts_reservation = ctx.reserve_scoped(0, "NX operation record identity counts")?;
    let mut records = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let stable_identity = match operation_header_identity_key(
                ctx,
                record.label().header.objects().values(),
                &block_identities,
            ) {
                Ok(identity) => identity,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let Some(span) = entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(record.offset()))
                .zip(
                    entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(
                        record.payload_offset(),
                    )),
                )
                .and_then(|(start, payload)| {
                    OperationRecordSpan::new(
                        start,
                        payload,
                        cadmpeg_core::decode::u64_from_index(record.payload().len()),
                    )
                })
            else {
                return;
            };
            let item = (|| -> Result<FeatureOperationRecord, CodecError> {
                if let Some(key) = &stable_identity {
                    if let Some(count) = identity_counts.get_mut(key.as_str()) {
                        *count = count.checked_add(1).ok_or_else(|| {
                            ctx.refuse_codec_limit("count NX operation record identities", 0, 1)
                        })?;
                    } else {
                        let mut copy = String::new();
                        counts_reservation.with_storage(|| {
                            ctx.try_reserve_retained_text(
                                &mut copy,
                                key.len(),
                                "allocate NX operation record identity key",
                            )
                        })?;
                        copy.push_str(key);
                        counts_reservation.with_storage(|| {
                            ctx.insert_btree_map(
                                &mut identity_counts,
                                copy,
                                1,
                                "NX operation record identity counts",
                            )
                        })?;
                    }
                }
                let ordinal = u32::try_from(operation_ordinal)
                    .map_err(|_| ctx.refuse_codec_limit("NX operation record ordinal", 0, 1))?;
                Ok(FeatureOperationRecord {
                    id: format_feature_history_id(
                        ctx,
                        "operation-record",
                        section_key,
                        operation_ordinal,
                        None,
                    )?,
                    operation_label: format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?,
                    ordinal,
                    sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                        ctx,
                        record.bytes(),
                        "NX retained record digest",
                    )?,
                    payload_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                        ctx,
                        record.payload(),
                        "NX retained record digest",
                    )?,
                    stable_identity,
                    span,
                })
            })();
            let item = match item {
                Ok(item) => item,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            if let Err(error) = ctx
                .charge_collection_items(1, "NX feature operation records")
                .and_then(|()| {
                    ctx.reserve_capacity(&mut records, 1, "allocate NX feature operation records")
                })
            {
                failure = Some(error);
                return;
            }
            records.push(item);
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    for record in &mut records {
        if record
            .stable_identity
            .as_ref()
            .is_some_and(|key| identity_counts.get(key.as_str()) != Some(&1))
        {
            record.stable_identity = None;
        }
    }
    Ok(records)
}

/// Retain operation records whose validated headers have no complete label.
pub(super) fn feature_unlabeled_operation_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureUnlabeledOperationRecord>, cadmpeg_core::CodecError> {
    let mut records = Vec::new();
    visit_feature_history_unlabeled_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let id = format_feature_history_id(
                ctx,
                "unlabeled-operation-record",
                section_key,
                operation_ordinal,
                None,
            )?;
            let ordinal = u32::try_from(operation_ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX unlabeled operation ordinal", 0, 1))?;
            if let Some(record) = FeatureUnlabeledOperationRecord::from_source(
                ctx,
                id,
                ordinal,
                entry_offset,
                record,
            )? {
                ctx.charge_entities(1, "NX unlabeled operation record")?;
                ctx.reserve_vec(&mut records, 1, "NX unlabeled operation records")?;
                records.push(record);
            }
            Ok(())
        },
    )?;
    Ok(records)
}

/// Decode body-write frames owned by independently bounded unlabeled records.
pub(super) fn feature_unlabeled_operation_body_writes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureOperationBodyWrite>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut writes = Vec::new();
    visit_feature_history_unlabeled_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let decoded = crate::om::unlabeled_operation_body_write_frames(ctx, record)?;
            for (ordinal, write) in decoded.into_iter().enumerate() {
                let Some(offset) =
                    entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(write.offset()))
                else {
                    continue;
                };
                let Some(frame) = crate::om::body_write::BodyWriteFrame::<u64>::new(
                    write.body_identity(),
                    write.group_node(),
                    write.endpoint_tag(),
                    write.body_image(),
                    offset,
                ) else {
                    continue;
                };
                let id = format_feature_history_id(
                    ctx,
                    "unlabeled-operation-body-write",
                    section_key,
                    operation_ordinal,
                    Some(ordinal),
                )?;
                let operation_record = format_feature_history_id(
                    ctx,
                    "unlabeled-operation-record",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                let ordinal = u32::try_from(ordinal)
                    .map_err(|_| ctx.refuse_codec_limit("NX unlabeled body write ordinal", 0, 1))?;
                ctx.charge_entities(1, "NX unlabeled operation body write")?;
                ctx.reserve_vec(&mut writes, 1, "NX unlabeled operation body writes")?;
                writes.push(FeatureOperationBodyWrite {
                    operation_label: None,
                    id,
                    operation_record,
                    ordinal,
                    body_image_data_block: charged_unique_offset_data_block(
                        ctx,
                        &indexed,
                        frame.body_image().value(),
                    )?,
                    frame,
                });
            }
            Ok(())
        },
    )?;
    Ok(writes)
}

/// Decode exact body-write frames from bounded feature operations.
pub(super) fn feature_operation_body_writes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureOperationBodyWrite>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut writes = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let decoded = match crate::om::operation_body_write_frames(ctx, record.payload_view()) {
                Ok(decoded) => decoded,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            for (ordinal, write) in decoded.into_iter().enumerate() {
                let Some(offset) =
                    entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(write.offset()))
                else {
                    continue;
                };
                let Some(frame) = crate::om::body_write::BodyWriteFrame::<u64>::new(
                    write.body_identity(),
                    write.group_node(),
                    write.endpoint_tag(),
                    write.body_image(),
                    offset,
                ) else {
                    continue;
                };
                let item = (|| -> Result<FeatureOperationBodyWrite, CodecError> {
                    let ordinal_u32 = u32::try_from(ordinal).map_err(|_| {
                        ctx.refuse_codec_limit("NX operation body write ordinal", 0, 1)
                    })?;
                    Ok(FeatureOperationBodyWrite {
                        id: format_feature_history_id(
                            ctx,
                            "operation-body-write",
                            section_key,
                            operation_ordinal,
                            Some(ordinal),
                        )?,
                        operation_label: Some(format_feature_history_id(
                            ctx,
                            "operation-label",
                            section_key,
                            operation_ordinal,
                            None,
                        )?),
                        operation_record: format_feature_history_id(
                            ctx,
                            "operation-record",
                            section_key,
                            operation_ordinal,
                            None,
                        )?,
                        ordinal: ordinal_u32,
                        body_image_data_block: charged_unique_offset_data_block(
                            ctx,
                            &indexed,
                            frame.body_image().value(),
                        )?,
                        frame,
                    })
                })();
                let item = match item {
                    Ok(item) => item,
                    Err(error) => {
                        failure = Some(error);
                        return;
                    }
                };
                if let Err(error) = ctx
                    .charge_collection_items(1, "NX operation body writes")
                    .and_then(|()| {
                        ctx.reserve_capacity(&mut writes, 1, "allocate NX operation body writes")
                    })
                {
                    failure = Some(error);
                    return;
                }
                writes.push(item);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(writes)
}

/// Join body-write identities to unique plain cached-body aliases.
///
/// Partition aliases use a separate identity namespace and do not participate.
/// A missing image block, no plain alias, or duplicate plain alias leaves the
/// write unresolved.
pub(super) fn feature_operation_body_image_segment_uses(
    ctx: &DecodeContext<'_>,
    writes: &[FeatureOperationBodyWrite],
    bindings: &[SegmentBodyBinding],
) -> Result<Vec<FeatureOperationBodyImageSegmentUse>, CodecError> {
    let work = writes
        .len()
        .checked_mul(bindings.len())
        .and_then(|count| count.checked_mul(2))
        .ok_or_else(|| ctx.refuse_codec_limit("join NX body-image segment uses", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "join NX body-image segment uses",
    )?;
    let mut output = Vec::new();
    for write in writes {
        let Some(body_image_data_block) = write.body_image_data_block.as_ref() else {
            continue;
        };
        let mut matches = bindings.iter().filter(|binding| {
            binding.stream_kind == crate::parasolid::StreamKind::Plain
                && binding.body_alias_object_index == u32::from(write.frame.body_identity())
        });
        let Some(binding) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let item = FeatureOperationBodyImageSegmentUse {
            id: replace_operation_text(
                ctx,
                &write.id,
                "operation-body-write",
                "operation-body-image-segment-use",
                "NX body-image segment use identity",
            )?,
            operation_body_write: ctx
                .copy_retained_text(&write.id, "NX body-image write identity")?,
            body_image_data_block: ctx
                .copy_retained_text(body_image_data_block, "NX body-image data block identity")?,
            segment_body_binding: ctx
                .copy_retained_text(&binding.id, "NX body-image segment binding identity")?,
        };
        ctx.reserve_vec(&mut output, 1, "NX body-image segment uses")?;
        output.push(item);
    }
    Ok(output)
}

/// Join persistent body identities to unique plain cached-body aliases.
///
/// This cross-store relation is independent of body-image block resolution.
/// Partition-stream aliases use another identity namespace and do not match.
pub(super) fn feature_operation_body_identity_segment_uses(
    ctx: &DecodeContext<'_>,
    writes: &[FeatureOperationBodyWrite],
    bindings: &[SegmentBodyBinding],
) -> Result<Vec<FeatureOperationBodyIdentitySegmentUse>, CodecError> {
    let work = writes
        .len()
        .checked_mul(bindings.len())
        .and_then(|count| count.checked_mul(2))
        .ok_or_else(|| ctx.refuse_codec_limit("join NX body-identity segment uses", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "join NX body-identity segment uses",
    )?;
    let mut output = Vec::new();
    for write in writes {
        let mut matches = bindings.iter().filter(|binding| {
            binding.stream_kind == crate::parasolid::StreamKind::Plain
                && binding.body_alias_object_index == u32::from(write.frame.body_identity())
        });
        let Some(binding) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let item = FeatureOperationBodyIdentitySegmentUse {
            id: replace_operation_text(
                ctx,
                &write.id,
                "operation-body-write",
                "operation-body-identity-segment-use",
                "NX body-identity segment use identity",
            )?,
            operation_body_write: ctx
                .copy_retained_text(&write.id, "NX body-identity write identity")?,
            body_identity: write.frame.body_identity(),
            segment_body_binding: ctx
                .copy_retained_text(&binding.id, "NX body-identity segment binding identity")?,
        };
        ctx.reserve_vec(&mut output, 1, "NX body-identity segment uses")?;
        output.push(item);
    }
    Ok(output)
}

const BODY_HISTORY_TERMINAL_STREAM_ROLE: u32 = 16;

fn body_history_partition_stream(
    ctx: &DecodeContext<'_>,
    binding: &SegmentBodyBinding,
    bindings: &[SegmentBodyBinding],
    streams: &[crate::parasolid::Stream],
) -> Result<Option<u32>, CodecError> {
    let stream_work = streams
        .len()
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit("scan NX body-history partition", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(stream_work),
        "scan NX body-history partition",
    )?;
    if binding.stream_kind != crate::parasolid::StreamKind::Plain {
        return Ok(None);
    }
    let Ok(stream_ordinal) = usize::try_from(binding.stream_ordinal) else {
        return Ok(None);
    };
    if streams
        .get(stream_ordinal)
        .is_none_or(|stream| stream.kind() != crate::parasolid::StreamKind::Plain)
    {
        return Ok(None);
    }
    let Some(partition_ordinal) = streams
        .iter()
        .enumerate()
        .skip(stream_ordinal + 1)
        .find_map(|(ordinal, stream)| {
            (stream.kind() == crate::parasolid::StreamKind::Partition).then_some(ordinal)
        })
    else {
        return Ok(None);
    };
    let run_start = streams[..stream_ordinal]
        .iter()
        .rposition(|stream| stream.kind() != crate::parasolid::StreamKind::Plain)
        .map_or(0, |ordinal| ordinal + 1);
    let Some(run_streams) = streams.get(run_start..partition_ordinal) else {
        return Ok(None);
    };
    if !run_streams
        .iter()
        .all(|stream| stream.kind() == crate::parasolid::StreamKind::Plain)
    {
        return Ok(None);
    }
    let work = run_streams
        .len()
        .checked_mul(bindings.len())
        .ok_or_else(|| ctx.refuse_codec_limit("scan NX body-history partition", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "scan NX body-history partition",
    )?;
    let mut terminal_role = None;
    for ordinal in run_start..partition_ordinal {
        let mut matches = bindings.iter().filter(|candidate| {
            candidate.stream_kind == crate::parasolid::StreamKind::Plain
                && usize::try_from(candidate.stream_ordinal).ok() == Some(ordinal)
        });
        let Some(candidate) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() || terminal_role == Some(BODY_HISTORY_TERMINAL_STREAM_ROLE) {
            return Ok(None);
        }
        terminal_role = Some(candidate.stream_role);
    }
    if terminal_role != Some(BODY_HISTORY_TERMINAL_STREAM_ROLE) {
        return Ok(None);
    }
    Ok(u32::try_from(partition_ordinal).ok())
}

/// Resolve body-write GROUP nodes only inside their complete body-history unit.
///
/// A plain cached-body binding belongs to the next partition only when every
/// intervening compressed stream is another completely bound plain image and
/// the run has one terminal role-16 binding. GROUP records from other
/// partition-local namespaces never participate, even when their node IDs are
/// equal.
pub(super) fn feature_operation_body_partition_uses(
    ctx: &DecodeContext<'_>,
    writes: &[FeatureOperationBodyWrite],
    image_uses: &[FeatureOperationBodyImageSegmentUse],
    bindings: &[SegmentBodyBinding],
    streams: &[crate::parasolid::Stream],
    groups: &[crate::native::parasolid::ParasolidGroupRecord],
    group_members: &[crate::native::parasolid::ParasolidGroupMember],
) -> Result<Vec<FeatureOperationBodyPartitionUse>, CodecError> {
    let scan_width = writes
        .len()
        .checked_add(bindings.len())
        .and_then(|count| count.checked_add(groups.len()))
        .and_then(|count| count.checked_add(group_members.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("join NX body partition uses", 0, 1))?;
    let work = image_uses
        .len()
        .checked_mul(scan_width)
        .ok_or_else(|| ctx.refuse_codec_limit("join NX body partition uses", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "join NX body partition uses",
    )?;
    let mut output = Vec::new();
    for image_use in image_uses {
        let mut matching_writes = writes
            .iter()
            .filter(|write| write.id == image_use.operation_body_write);
        let Some(write) = matching_writes.next() else {
            continue;
        };
        if matching_writes.next().is_some() {
            continue;
        }
        let mut matching_bindings = bindings
            .iter()
            .filter(|binding| binding.id == image_use.segment_body_binding);
        let Some(binding) = matching_bindings.next() else {
            continue;
        };
        if matching_bindings.next().is_some() {
            continue;
        }
        let Some(partition_stream_ordinal) =
            body_history_partition_stream(ctx, binding, bindings, streams)?
        else {
            continue;
        };
        let mut parasolid_group_records = Vec::new();
        for group in groups.iter().filter(|group| {
            group.origin.partition_stream_ordinal() == Some(partition_stream_ordinal)
                && group.node_id == write.frame.group_node().value()
        }) {
            ctx.reserve_vec(
                &mut parasolid_group_records,
                1,
                "NX body partition group records",
            )?;
            parasolid_group_records.push(
                ctx.copy_retained_text(&group.id, "NX body partition group record identity")?,
            );
        }
        let mut parasolid_group_members = Vec::new();
        for member in group_members.iter().filter(|member| {
            member.partition_stream_ordinal == partition_stream_ordinal
                && member.group_node_id == write.frame.group_node().value()
        }) {
            ctx.reserve_vec(
                &mut parasolid_group_members,
                1,
                "NX body partition group members",
            )?;
            parasolid_group_members.push(
                ctx.copy_retained_text(&member.id, "NX body partition group member identity")?,
            );
        }
        let item = FeatureOperationBodyPartitionUse {
            id: replace_operation_text(
                ctx,
                &write.id,
                "operation-body-write",
                "operation-body-partition-use",
                "NX body partition use identity",
            )?,
            operation_body_write: ctx
                .copy_retained_text(&write.id, "NX body partition write identity")?,
            body_image_segment_use: ctx
                .copy_retained_text(&image_use.id, "NX body partition image use identity")?,
            segment_body_binding: ctx
                .copy_retained_text(&binding.id, "NX body partition binding identity")?,
            partition_stream_ordinal,
            group_node: write.frame.group_node().value(),
            parasolid_group_records,
            parasolid_group_members,
        };
        ctx.reserve_vec(&mut output, 1, "NX body partition uses")?;
        output.push(item);
    }
    Ok(output)
}

/// Resolve body-write GROUP nodes directly through their partition ownership.
///
/// The relation requires at least one retained GROUP record and exactly one
/// partition namespace for that node. Labeled and independently bounded
/// unlabeled writes participate in the same persistent body-identity domain.
pub(super) fn feature_body_write_group_partition_uses(
    ctx: &DecodeContext<'_>,
    writes: &[FeatureOperationBodyWrite],
    unlabeled_writes: &[FeatureOperationBodyWrite],
    groups: &[crate::native::parasolid::ParasolidGroupRecord],
    group_members: &[crate::native::parasolid::ParasolidGroupMember],
) -> Result<Vec<FeatureBodyWriteGroupPartitionUse>, CodecError> {
    let candidates = writes
        .len()
        .checked_add(unlabeled_writes.len())
        .ok_or_else(|| ctx.refuse_codec_limit("join NX body-write group partitions", 0, 1))?;
    let scan_width = groups
        .len()
        .checked_mul(2)
        .and_then(|count| count.checked_add(group_members.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("join NX body-write group partitions", 0, 1))?;
    let work = candidates
        .checked_mul(scan_width)
        .ok_or_else(|| ctx.refuse_codec_limit("join NX body-write group partitions", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "join NX body-write group partitions",
    )?;
    let mut output = Vec::new();
    for write in writes.iter().chain(unlabeled_writes) {
        let id = write.id.as_str();
        let group_node = write.frame.group_node().value();
        let mut partition = None;
        let mut valid = true;
        for group in groups.iter().filter(|group| group.node_id == group_node) {
            match (partition, group.origin.partition_stream_ordinal()) {
                (None, Some(ordinal)) => partition = Some(ordinal),
                (Some(expected), Some(actual)) if expected == actual => {}
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        let Some(partition_stream_ordinal) = partition else {
            continue;
        };
        if !valid {
            continue;
        }
        let mut parasolid_group_records = Vec::new();
        for group in groups.iter().filter(|group| group.node_id == group_node) {
            ctx.reserve_vec(
                &mut parasolid_group_records,
                1,
                "NX body-write group records",
            )?;
            parasolid_group_records
                .push(ctx.copy_retained_text(&group.id, "NX body-write group record identity")?);
        }
        let mut parasolid_group_members = Vec::new();
        for member in group_members.iter().filter(|member| {
            member.partition_stream_ordinal == partition_stream_ordinal
                && member.group_node_id == group_node
        }) {
            ctx.reserve_vec(
                &mut parasolid_group_members,
                1,
                "NX body-write group members",
            )?;
            parasolid_group_members
                .push(ctx.copy_retained_text(&member.id, "NX body-write group member identity")?);
        }
        let use_record = FeatureBodyWriteGroupPartitionUse {
            id: replace_operation_text(
                ctx,
                id,
                "body-write",
                "body-write-group-partition-use",
                "NX body-write group use identity",
            )?,
            body_write: ctx.copy_retained_text(id, "NX body-write group write identity")?,
            body_identity: write.frame.body_identity(),
            group_node,
            partition_stream_ordinal,
            parasolid_group_records,
            parasolid_group_members,
        };
        ctx.reserve_vec(&mut output, 1, "NX body-write group partition uses")?;
        output.push(use_record);
    }
    Ok(output)
}

/// Decode one direct-reference field family from bounded feature operations.
pub(super) fn feature_operation_object_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    kind: crate::om::direct_reference::ReferenceFieldKind,
) -> Result<Vec<FeatureOperationObjectReference>, cadmpeg_core::CodecError> {
    let stem = match kind {
        crate::om::direct_reference::ReferenceFieldKind::Tagged17 => "operation-tagged-reference",
        crate::om::direct_reference::ReferenceFieldKind::DataBlock03 => {
            "operation-data-block-reference"
        }
    };
    let indexed = container.indexed_om_sections(ctx)?;
    let mut references = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let result = (|| -> Result<(), CodecError> {
                let fields = crate::om::direct_reference::operation_reference_fields(
                    ctx,
                    record.payload_view(),
                    kind,
                )?;
                for (ordinal, reference) in fields.into_iter().enumerate() {
                    let Some(offset) = u64::try_from(reference.offset())
                        .ok()
                        .and_then(|offset| entry_offset.checked_add(offset))
                    else {
                        continue;
                    };
                    let Some(frame) = crate::om::direct_reference::DirectReferenceFrame::<u64>::new(
                        reference.kind(),
                        reference.object(),
                        offset,
                    ) else {
                        continue;
                    };
                    let Some(ordinal_u32) = u32::try_from(ordinal).ok() else {
                        continue;
                    };
                    ctx.charge_work(1, "resolve NX operation object reference")?;
                    ctx.reserve_vec(&mut references, 1, "NX operation object references")?;
                    references.push(FeatureOperationObjectReference {
                        id: format_feature_history_id(
                            ctx,
                            stem,
                            section_key,
                            operation_ordinal,
                            Some(ordinal),
                        )?,
                        operation_label: format_feature_history_id(
                            ctx,
                            "operation-label",
                            section_key,
                            operation_ordinal,
                            None,
                        )?,
                        operation_record: format_feature_history_id(
                            ctx,
                            "operation-record",
                            section_key,
                            operation_ordinal,
                            None,
                        )?,
                        ordinal: ordinal_u32,
                        data_block: charged_unique_offset_data_block(
                            ctx,
                            &indexed,
                            frame.object().value(),
                        )?,
                        frame,
                    });
                }
                Ok(())
            })();
            if let Err(error) = result {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(references)
}

/// Decode every exact common frame from bounded feature operations.
pub(super) fn feature_operation_common_frames(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureOperationCommonFrame>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut frames = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let result = (|| -> Result<(), CodecError> {
                let decoded = crate::om::operation_common_frames(ctx, record.payload_view())?;
                for (ordinal, frame) in decoded.into_iter().enumerate() {
                    let Some(offset) = u64::try_from(frame.offset())
                        .ok()
                        .and_then(|offset| entry_offset.checked_add(offset))
                    else {
                        continue;
                    };
                    let Some(ordinal_u32) = u32::try_from(ordinal).ok() else {
                        continue;
                    };
                    ctx.charge_work(1, "resolve NX operation common frame")?;
                    let target = match frame.suffix().object_index() {
                        Some(index) => charged_unique_offset_data_block(ctx, &indexed, index)?,
                        None => None,
                    };
                    let Some(frame) =
                        crate::om::common_frame::CommonFrame::<u64, Option<String>>::new(
                            frame.prefix(),
                            frame.state(),
                            (*frame.suffix()).map_target(|_, ()| target),
                            offset,
                        )
                    else {
                        continue;
                    };
                    ctx.reserve_vec(&mut frames, 1, "NX operation common frames")?;
                    frames.push(FeatureOperationCommonFrame {
                        id: format_feature_history_id(
                            ctx,
                            "operation-common-frame",
                            section_key,
                            operation_ordinal,
                            Some(ordinal),
                        )?,
                        operation_record: format_feature_history_id(
                            ctx,
                            "operation-record",
                            section_key,
                            operation_ordinal,
                            None,
                        )?,
                        ordinal: ordinal_u32,
                        frame,
                    });
                }
                Ok(())
            })();
            if let Err(error) = result {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(frames)
}

/// Decode canonical terminal common-frame suffixes from bounded operations.
pub(super) fn feature_operation_terminal_frames(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    common_frames: &[FeatureOperationCommonFrame],
) -> Result<Vec<FeatureOperationTerminalFrame>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut frames = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let result = (|| -> Result<(), CodecError> {
                let Some(decoded) =
                    crate::om::operation_terminal_frame(ctx, record.payload_view())?
                else {
                    return Ok(());
                };
                let operation_record = format_feature_history_id(
                    ctx,
                    "operation-record",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(common_frames.len()),
                    "resolve NX operation terminal common frame",
                )?;
                let immediate_common_frame = decoded
                    .immediate_common_frame_offset
                    .and_then(|offset| u64::try_from(offset).ok())
                    .and_then(|offset| entry_offset.checked_add(offset))
                    .and_then(|offset| {
                        let mut matches = common_frames.iter().filter(|common| {
                            common.operation_record == operation_record
                                && common.frame.offset() == offset
                        });
                        let common = matches.next()?;
                        matches.next().is_none().then_some(common.id.as_str())
                    });
                let Some(offset) = u64::try_from(decoded.frame.offset())
                    .ok()
                    .and_then(|offset| entry_offset.checked_add(offset))
                else {
                    return Ok(());
                };
                let target = match decoded.frame.suffix().object_index() {
                    Some(index) => charged_unique_offset_data_block(ctx, &indexed, index)?,
                    None => None,
                };
                let Some(frame) =
                    crate::om::common_frame::TerminalFrame::<u64, Option<String>>::new(
                        (*decoded.frame.suffix()).map_target(|_, ()| target),
                        offset,
                    )
                else {
                    return Ok(());
                };
                ctx.reserve_vec(&mut frames, 1, "NX operation terminal frames")?;
                frames.push(FeatureOperationTerminalFrame {
                    id: format_feature_history_id(
                        ctx,
                        "operation-terminal-frame",
                        section_key,
                        operation_ordinal,
                        None,
                    )?,
                    operation_record,
                    immediate_common_frame: immediate_common_frame
                        .map(|id| {
                            ctx.copy_retained_text(
                                id,
                                "NX operation immediate common frame identity",
                            )
                        })
                        .transpose()?,
                    frame,
                });
                Ok(())
            })();
            if let Err(error) = result {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(frames)
}

/// Join operation terminal ordinals to exact rows in the owning state journal.
pub(super) fn feature_operation_state_journal_uses(
    ctx: &DecodeContext<'_>,
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    terminal_frames: &[FeatureOperationTerminalFrame],
    journal_groups: &[OmOperationStateJournalGroup],
) -> Result<Vec<FeatureOperationStateJournalUse>, CodecError> {
    let row_count = journal_groups.iter().try_fold(0usize, |count, group| {
        count
            .checked_add(group.frame.rows().len())
            .ok_or_else(|| ctx.refuse_codec_limit("scan NX operation journal rows", 0, 1))
    })?;
    let scan_width = labels
        .len()
        .checked_mul(records.len())
        .and_then(|count| count.checked_add(row_count))
        .and_then(|count| count.checked_add(journal_groups.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("join NX operation journal rows", 0, 1))?;
    let work = terminal_frames
        .len()
        .checked_mul(scan_width)
        .and_then(|count| count.checked_add(row_count))
        .ok_or_else(|| ctx.refuse_codec_limit("join NX operation journal rows", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "join NX operation journal rows",
    )?;
    let mut uses = Vec::new();
    for frame in terminal_frames {
        let Some(label) = records
            .iter()
            .rev()
            .filter(|record| record.id == frame.operation_record)
            .find_map(|record| {
                labels
                    .iter()
                    .rev()
                    .find(|label| label.id == record.operation_label)
            })
        else {
            continue;
        };
        let mut matching_row = None;
        let mut ambiguous = false;
        for group in journal_groups
            .iter()
            .filter(|group| group.section_link == label.section_link)
        {
            for (row_ordinal, row) in group.frame.rows().iter().enumerate() {
                let Some(row_ordinal) = u32::try_from(row_ordinal).ok() else {
                    continue;
                };
                if row.ordinal().value() != frame.frame.suffix().local_ordinal() {
                    continue;
                }
                if matching_row.is_some() {
                    ambiguous = true;
                    break;
                }
                matching_row = Some((group, row_ordinal, row));
            }
            if ambiguous {
                break;
            }
        }
        if ambiguous {
            continue;
        }
        let Some((group, journal_row_ordinal, row)) = matching_row else {
            continue;
        };
        let operation_key = frame
            .operation_record
            .strip_prefix("nx:feature-history:operation-record#")
            .unwrap_or(frame.operation_record.as_str());
        let journal_key = group
            .id
            .strip_prefix("nx:feature-history:operation-state-journal-group#")
            .unwrap_or(group.id.as_str());
        let prefix = "nx:feature-history:operation-state-journal-use#";
        let id_len = prefix
            .len()
            .checked_add(operation_key.len())
            .and_then(|length| length.checked_add(journal_key.len()))
            .and_then(|length| length.checked_add(12))
            .ok_or_else(|| {
                ctx.refuse_codec_limit("format NX operation journal use identity", 0, 1)
            })?;
        let mut id = ctx.retained_string(id_len, "NX operation journal use identity")?;
        write!(
            &mut id,
            "{prefix}{operation_key}-{journal_key}-{journal_row_ordinal:010}"
        )
        .map_err(|_| ctx.refuse_codec_limit("format NX operation journal use identity", 0, 1))?;
        ctx.reserve_vec(&mut uses, 1, "NX operation journal uses")?;
        uses.push(FeatureOperationStateJournalUse {
            id,
            section_link: ctx
                .copy_retained_text(&label.section_link, "NX operation journal section link")?,
            operation_label: ctx
                .copy_retained_text(&label.id, "NX operation journal label identity")?,
            operation_record: ctx.copy_retained_text(
                &frame.operation_record,
                "NX operation journal record identity",
            )?,
            operation_terminal_frame: ctx
                .copy_retained_text(&frame.id, "NX operation journal terminal identity")?,
            journal_group: ctx
                .copy_retained_text(&group.id, "NX operation journal group identity")?,
            journal_row_ordinal,
            state_ordinal: row.ordinal().value(),
            operation_source_offset: frame.frame.offset(),
            journal_source_offset: row.offset(),
        });
    }
    Ok(uses)
}

/// Decode ordered self-framed strings from feature-operation payloads.
pub(super) fn feature_payload_strings(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeaturePayloadString>, cadmpeg_core::CodecError> {
    let mut strings = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let result = (|| -> Result<(), CodecError> {
                let values = crate::om::operation_payload_strings(ctx, record.payload_view())?;
                for (ordinal, value) in values.into_iter().enumerate() {
                    let Some(ordinal_u32) = u32::try_from(ordinal).ok() else {
                        continue;
                    };
                    let Some(source_offset) = u64::try_from(value.offset)
                        .ok()
                        .and_then(|offset| entry_offset.checked_add(offset))
                    else {
                        continue;
                    };
                    ctx.reserve_vec(&mut strings, 1, "NX feature payload strings")?;
                    let text = ctx.copy_retained_text(
                        value.value.as_str(),
                        "NX feature payload string text",
                    )?;
                    strings.push(FeaturePayloadString {
                        id: format_feature_history_id(
                            ctx,
                            "payload-string",
                            section_key,
                            operation_ordinal,
                            Some(ordinal),
                        )?,
                        operation_record: format_feature_history_id(
                            ctx,
                            "operation-record",
                            section_key,
                            operation_ordinal,
                            None,
                        )?,
                        ordinal: ordinal_u32,
                        value: crate::payload_text::PayloadText::new(text)
                            .map_err(|error| CodecError::InvalidInput(error.to_string()))?,
                        source_offset,
                    });
                }
                Ok(())
            })();
            if let Err(error) = result {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(strings)
}

/// Decode complete body-reference fields from feature-history operations.
pub(super) fn feature_body_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureBodyReference>, cadmpeg_core::CodecError> {
    let mut references = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let Some(reference) = crate::om::operation_body_reference(record.body_view()) else {
                return;
            };
            let result = (|| -> Result<(), CodecError> {
                let Some(source_offset) = u64::try_from(reference.offset)
                    .ok()
                    .and_then(|offset| entry_offset.checked_add(offset))
                else {
                    return Ok(());
                };
                ctx.charge_work(1, "resolve NX feature body reference")?;
                ctx.reserve_vec(&mut references, 1, "NX feature body references")?;
                references.push(FeatureBodyReference {
                    ordinal: None,
                    id: format_feature_history_id(
                        ctx,
                        "body-reference",
                        section_key,
                        operation_ordinal,
                        None,
                    )?,
                    operation_label: format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?,
                    body: reference.object_index,
                    source_offset,
                });
                Ok(())
            })();
            if let Err(error) = result {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(references)
}

/// Return the one body-reference field owned by each operation that has
/// exactly one such field.
pub(super) fn unique_feature_body_references<'a>(
    ctx: &DecodeContext<'_>,
    references: &'a [FeatureBodyReference],
) -> Result<BTreeMap<&'a str, &'a FeatureBodyReference>, CodecError> {
    let work = references
        .len()
        .checked_mul(references.len())
        .ok_or_else(|| ctx.refuse_codec_limit("index NX body references", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "index NX body references",
    )?;
    let mut unique = BTreeMap::<&str, &FeatureBodyReference>::new();
    let mut ambiguous = BTreeSet::<&str>::new();
    let mut ambiguous_reservation = ctx.reserve_scoped(0, "NX ambiguous body references")?;
    for reference in references {
        let key = reference.operation_label.as_str();
        if ambiguous.contains(key) {
            continue;
        }
        if unique.remove(key).is_some() {
            ambiguous_reservation.with_storage(|| {
                ctx.insert_btree_set(&mut ambiguous, key, "NX ambiguous body references")
            })?;
            continue;
        }

        ctx.insert_btree_map(&mut unique, key, reference, "NX unique body references")?;
    }
    Ok(unique)
}

/// Decode every ordered body-reference field from bounded feature operations.
pub(super) fn feature_body_reference_occurrences(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureBodyReference>, cadmpeg_core::CodecError> {
    let mut references = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let result = (|| -> Result<(), CodecError> {
                let rows = crate::om::operation_body_references(ctx, record.body_view())?;
                for (ordinal, reference) in rows.into_iter().enumerate() {
                    let Some(ordinal_u32) = u32::try_from(ordinal).ok() else {
                        continue;
                    };
                    let Some(source_offset) = u64::try_from(reference.offset)
                        .ok()
                        .and_then(|offset| entry_offset.checked_add(offset))
                    else {
                        continue;
                    };
                    ctx.charge_work(1, "resolve NX body reference occurrence")?;
                    ctx.reserve_vec(&mut references, 1, "NX body reference occurrences")?;
                    references.push(FeatureBodyReference {
                        id: format_feature_history_id(
                            ctx,
                            "body-reference-occurrence",
                            section_key,
                            operation_ordinal,
                            Some(ordinal),
                        )?,
                        operation_label: format_feature_history_id(
                            ctx,
                            "operation-label",
                            section_key,
                            operation_ordinal,
                            None,
                        )?,
                        ordinal: Some(ordinal_u32),
                        body: reference.object_index,
                        source_offset,
                    });
                }
                Ok(())
            })();
            if let Err(error) = result {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(references)
}

/// Join primary body fields to exactly one segment body alias pair.
///
/// A field resolved in an offset store enters the segment namespace only when
/// its operation has one resolved offset store, the field has one retained
/// data-block use, and that block contains one object frame carrying the
/// field's persistent identity. A primary-index match, a missing or duplicate
/// data-block use, a missing or ambiguous store, a missing or duplicate object
/// frame, or a duplicate segment alias remains unresolved.
fn unique_offset_store_body_frame<'a>(
    reference: &FeatureBodyReference,
    data_block_use: &FeatureBodyDataBlockUse,
    object_frames: &'a [DataBlockObjectFrame],
) -> Option<&'a DataBlockObjectFrame> {
    let mut matches = object_frames.iter().filter(|frame| {
        frame.data_block == data_block_use.data_block
            && frame.object.atom.value() == reference.body.value()
    });
    let frame = matches.next()?;
    matches.next().is_none().then_some(frame)
}

pub(super) fn feature_body_segment_uses(
    ctx: &DecodeContext<'_>,
    references: &[FeatureBodyReference],
    data_block_uses: &[FeatureBodyDataBlockUse],
    inputs: &[FeatureInputBlock],
    blocks: &[crate::native::om::DataBlock],
    bindings: &[SegmentBodyBinding],
    object_frames: &[DataBlockObjectFrame],
) -> Result<Vec<FeatureBodySegmentUse>, CodecError> {
    let (unique_references, _unique_references_storage) = ctx
        .with_scoped_storage("NX local unique_references storage", || {
            unique_feature_body_references(ctx, references)
        })?;
    let mut counts_reservation = ctx.reserve_scoped(0, "NX body offset-store reference counts")?;
    let mut offset_store_reference_counts = BTreeMap::<&str, usize>::new();
    for use_ in data_block_uses {
        let reference_key = use_.feature_body_reference.as_str();
        counts_reservation.with_storage(|| {
            ctx.admit_btree_entry(
                &offset_store_reference_counts,
                &reference_key,
                "NX body offset-store reference counts",
            )
        })?;

        let count = offset_store_reference_counts
            .entry(reference_key)
            .or_default();
        *count = count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("count NX body offset-store references", 0, 1))?;
    }
    let (offset_store_operations, _offset_store_operations_storage) = ctx
        .with_scoped_storage("NX local offset_store_operations storage", || {
            feature_input_store_operations(ctx, inputs, blocks)
        })?;
    let (store_sections, _store_sections_storage) = ctx
        .with_scoped_storage("NX local store_sections storage", || {
            feature_input_store_sections(ctx, inputs, blocks)
        })?;
    let scan_work = references
        .len()
        .checked_mul(data_block_uses.len())
        .and_then(|count| count.checked_add(references.len().checked_mul(bindings.len())?))
        .and_then(|count| count.checked_add(references.len().checked_mul(object_frames.len())?))
        .ok_or_else(|| ctx.refuse_codec_limit("join NX body segment uses", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(scan_work),
        "join NX body segment uses",
    )?;
    let mut output = Vec::new();
    for reference in references {
        if unique_references
            .get(reference.operation_label.as_str())
            .is_none_or(|unique| unique.id != reference.id)
        {
            continue;
        }
        let count = offset_store_reference_counts
            .get(reference.id.as_str())
            .copied();
        let has_offset_store_reference = count.is_some();
        if offset_store_operations.contains(reference.operation_label.as_str())
            && !has_offset_store_reference
        {
            continue;
        }
        if has_offset_store_reference
            && (count != Some(1)
                || store_sections
                    .get(reference.operation_label.as_str())
                    .is_none_or(|sections| sections.len() != 1))
        {
            continue;
        }
        let binding = if has_offset_store_reference {
            let Some(data_block_use) = data_block_uses
                .iter()
                .find(|use_| use_.feature_body_reference == reference.id)
            else {
                continue;
            };
            if unique_offset_store_body_frame(reference, data_block_use, object_frames).is_none() {
                continue;
            }
            let Some(binding) = crate::native::segments::unique_segment_body_alias_binding(
                reference.body.value(),
                bindings,
            ) else {
                continue;
            };
            binding
        } else {
            let Some(binding) = crate::native::segments::unique_segment_body_binding(
                reference.body.value(),
                bindings,
            ) else {
                continue;
            };
            binding
        };
        let replace = reference.id.split_once("body-reference");
        let (prefix, suffix, replacement) = replace
            .map_or((reference.id.as_str(), "", ""), |(prefix, suffix)| {
                (prefix, suffix, "body-segment-use")
            });
        let id_len = prefix
            .len()
            .checked_add(suffix.len())
            .and_then(|count| count.checked_add(replacement.len()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX body segment use identity", 0, 1))?;
        let mut id = ctx.retained_string(id_len, "NX body segment use identity")?;
        id.push_str(prefix);
        id.push_str(replacement);
        id.push_str(suffix);
        let copy = |source: &str, operation: &'static str| -> Result<String, CodecError> {
            let mut value = ctx.retained_string(source.len(), operation)?;
            value.push_str(source);
            Ok(value)
        };
        let feature_body_reference = copy(&reference.id, "NX body segment reference identity")?;
        let segment_body_binding = copy(&binding.id, "NX body segment binding identity")?;
        ctx.reserve_vec(&mut output, 1, "NX body segment uses")?;
        output.push(FeatureBodySegmentUse {
            id,
            feature_body_reference,
            segment_body_binding,
        });
    }
    Ok(output)
}

/// Return operations with at least one resolved input field in an offset store.
///
/// This set identifies operations whose body fields must be resolved in the
/// offset-store namespace. The one-store requirement for a segment bridge is
/// checked separately from this broader namespace classification.
pub(super) fn feature_input_store_operations(
    ctx: &DecodeContext<'_>,
    inputs: &[FeatureInputBlock],
    blocks: &[crate::native::om::DataBlock],
) -> Result<BTreeSet<String>, CodecError> {
    let (sections, _sections_storage) = ctx
        .with_scoped_storage("NX local sections storage", || {
            feature_input_store_sections(ctx, inputs, blocks)
        })?;
    let mut operations = BTreeSet::new();
    for (label, sections) in sections {
        if sections.is_empty() {
            continue;
        }

        let label = ctx.copy_retained_text(&label, "NX offset-store operation label")?;
        ctx.insert_btree_set(&mut operations, label, "NX offset-store operations")?;
    }
    Ok(operations)
}

/// Group resolved operation-header inputs by their indexed offset-store section.
fn feature_input_store_sections(
    ctx: &DecodeContext<'_>,
    inputs: &[FeatureInputBlock],
    blocks: &[crate::native::om::DataBlock],
) -> Result<BTreeMap<String, BTreeSet<u32>>, CodecError> {
    let mut block_reservation = ctx.reserve_scoped(0, "NX input-store block index")?;
    let mut blocks_by_id = BTreeMap::new();
    for block in blocks {
        block_reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut blocks_by_id,
                block.id.as_str(),
                block,
                "NX input-store block index",
            )
        })?;
    }
    let work = inputs
        .len()
        .checked_mul(blocks.len())
        .and_then(|count| count.checked_add(inputs.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("join NX input-store sections", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "join NX input-store sections",
    )?;
    let mut sections_by_operation = BTreeMap::<String, BTreeSet<u32>>::new();
    for input in inputs {
        let Some(block) = blocks_by_id.get(input.data_block.as_str()) else {
            continue;
        };
        ctx.charge_collection_items(1, "NX input-store section identities")?;

        if let Some(sections) = sections_by_operation.get_mut(input.operation_label.as_str()) {
            if !sections.contains(&block.section_ordinal) {
                ctx.admit_btree_node_storage::<u32, ()>(
                    sections.len(),
                    "NX input-store section identities",
                )?;
            }
            sections.insert(block.section_ordinal);
            continue;
        }
        let key_len = input.operation_label.len();

        let mut label = String::new();
        ctx.try_reserve_retained_text(
            &mut label,
            key_len,
            "allocate NX input-store operation label",
        )?;
        label.push_str(&input.operation_label);
        let mut sections = BTreeSet::new();
        ctx.admit_btree_node_storage::<u32, ()>(0, "NX input-store section identities")?;
        sections.insert(block.section_ordinal);
        ctx.insert_btree_map(
            &mut sections_by_operation,
            label,
            sections,
            "NX input-store operation groups",
        )?;
    }
    Ok(sections_by_operation)
}

/// Resolve primary feature body fields in an unambiguous operation input store.
pub(super) fn feature_body_data_block_uses(
    ctx: &DecodeContext<'_>,
    references: &[FeatureBodyReference],
    inputs: &[FeatureInputBlock],
    blocks: &[crate::native::om::DataBlock],
) -> Result<Vec<FeatureBodyDataBlockUse>, CodecError> {
    let work = references
        .len()
        .checked_mul(references.len())
        .and_then(|work| {
            references
                .len()
                .checked_mul(inputs.len())?
                .checked_mul(blocks.len())?
                .checked_add(work)
        })
        .ok_or_else(|| ctx.refuse_codec_limit("scan NX feature body block uses", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "scan NX feature body block uses",
    )?;
    let mut uses = Vec::new();
    for reference in references {
        if references
            .iter()
            .filter(|candidate| candidate.operation_label == reference.operation_label)
            .take(2)
            .count()
            != 1
        {
            continue;
        }
        let mut section_ordinal = None;
        let mut ambiguous_section = false;
        for input in inputs
            .iter()
            .filter(|input| input.operation_label == reference.operation_label)
        {
            let Some(block) = blocks
                .iter()
                .rev()
                .find(|block| block.id == input.data_block)
            else {
                continue;
            };
            match section_ordinal {
                None => section_ordinal = Some(block.section_ordinal),
                Some(section) if section != block.section_ordinal => ambiguous_section = true,
                Some(_) => {}
            }
        }
        let Some(section_ordinal) = section_ordinal.filter(|_| !ambiguous_section) else {
            continue;
        };
        let mut matches = blocks.iter().filter(|block| {
            block.section_ordinal == section_ordinal
                && block.block_ordinal == reference.body.value()
        });
        let Some(block) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let old = "body-reference";
        let new = "body-data-block-use";
        let (prefix, suffix) = if let Some(position) = reference.id.find(old) {
            (
                &reference.id[..position],
                &reference.id[position + old.len()..],
            )
        } else {
            (reference.id.as_str(), "")
        };
        let id_len = prefix
            .len()
            .checked_add(suffix.len())
            .and_then(|length| {
                length.checked_add(if prefix.len() == reference.id.len() {
                    0
                } else {
                    new.len()
                })
            })
            .ok_or_else(|| ctx.refuse_codec_limit("retain NX feature body block use id", 0, 1))?;
        let mut id = ctx.retained_string(id_len, "retain NX feature body block use id")?;
        id.push_str(prefix);
        if prefix.len() != reference.id.len() {
            id.push_str(new);
        }
        id.push_str(suffix);
        ctx.reserve_vec(&mut uses, 1, "NX feature body block uses")?;
        uses.push(FeatureBodyDataBlockUse {
            id,
            feature_body_reference: ctx
                .copy_retained_text(&reference.id, "retain NX feature body reference id")?,
            data_block: ctx.copy_retained_text(&block.id, "retain NX feature body block id")?,
        });
    }
    Ok(uses)
}

/// Resolve operation-header object indices to unique offset-only data blocks.
pub(super) fn feature_input_blocks(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureInputBlock>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut inputs = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let label = record.label();
            for (input_slot, object) in HeaderSlot::ALL.into_iter().zip(label.header.objects().0) {
                let Some(object) = object else {
                    continue;
                };
                let result = (|| -> Result<(), CodecError> {
                    let Some(data_block) =
                        charged_unique_offset_data_block(ctx, &indexed, object.value())?
                    else {
                        return Ok(());
                    };
                    let Some(source_offset) =
                        u64::try_from(label.header.object_offsets()[input_slot.index()])
                            .ok()
                            .and_then(|offset| entry_offset.checked_add(offset))
                    else {
                        return Ok(());
                    };
                    ctx.reserve_vec(&mut inputs, 1, "NX feature input blocks")?;
                    inputs.push(FeatureInputBlock {
                        id: format_feature_history_id(
                            ctx,
                            "input-block",
                            section_key,
                            operation_ordinal,
                            Some(input_slot.index()),
                        )?,
                        operation_label: format_feature_history_id(
                            ctx,
                            "operation-label",
                            section_key,
                            operation_ordinal,
                            None,
                        )?,
                        input_slot,
                        object,
                        data_block,
                        source_offset,
                    });
                    Ok(())
                })();
                if let Err(error) = result {
                    failure = Some(error);
                    return;
                }
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(inputs)
}

/// Group bindings from distinct operations by exact resolved data-block identity.
pub(super) fn feature_input_block_identity_groups(
    ctx: &DecodeContext<'_>,
    inputs: &[FeatureInputBlock],
) -> Result<Vec<FeatureInputBlockIdentityGroup>, CodecError> {
    let work = inputs
        .len()
        .checked_mul(inputs.len())
        .and_then(|count| count.checked_mul(3))
        .ok_or_else(|| ctx.refuse_codec_limit("group NX feature input blocks", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "group NX feature input blocks",
    )?;
    let mut map_reservation = ctx.reserve_scoped(0, "NX input block group index")?;
    let mut member_reservation = ctx.reserve_scoped(0, "NX input block group members")?;
    let mut by_block = BTreeMap::<&str, Vec<&FeatureInputBlock>>::new();
    for input in inputs {
        if !by_block.contains_key(input.data_block.as_str()) {
            map_reservation.with_storage(|| {
                ctx.admit_btree_entry(
                    &by_block,
                    &input.data_block.as_str(),
                    "NX input block group index",
                )
            })?;
        }
        let members = by_block.entry(input.data_block.as_str()).or_default();
        ctx.reserve_scoped_vec(
            &mut member_reservation,
            members,
            1,
            "NX input block group members",
        )?;
        members.push(input);
    }
    let mut group_reservation = ctx.reserve_scoped(0, "NX input block group order")?;
    let mut groups = Vec::new();
    for (data_block, mut members) in by_block {
        let Some(first) = members.first() else {
            continue;
        };
        if !members
            .iter()
            .any(|member| member.operation_label != first.operation_label)
        {
            continue;
        }
        ctx.stable_sort_by(
            &mut members,
            |value| &value.source_offset,
            Ord::cmp,
            "sort NX input block group members",
        )?;
        ctx.reserve_scoped_vec(
            &mut group_reservation,
            &mut groups,
            1,
            "NX input block group order",
        )?;
        groups.push((data_block, members));
    }
    drop(map_reservation);
    ctx.stable_sort_by_key(
        &mut groups,
            |value| { let (_, left) = value; left[0].source_offset },
            Ord::cmp,
        "sort NX input block groups",
    )?;
    let mut output = Vec::new();
    for (ordinal, (data_block, members)) in groups.into_iter().enumerate() {
        let mut retained_members = Vec::new();
        for member in members {
            ctx.reserve_vec(&mut retained_members, 1, "NX input block identity members")?;
            retained_members.push(FeatureInputBlockIdentityMember {
                input_block: ctx
                    .copy_retained_text(&member.id, "NX input block identity member")?,
                operation_label: ctx.copy_retained_text(
                    &member.operation_label,
                    "NX input block member operation label",
                )?,
                input_slot: member.input_slot,
                source_offset: member.source_offset,
            });
        }
        let prefix = "nx:feature-history:input-block-identity-group#";
        let mut value = ordinal;
        let mut digits = 1usize;
        while value >= 10 {
            value /= 10;
            digits += 1;
        }
        let id_len = prefix
            .len()
            .checked_add(digits.max(10))
            .ok_or_else(|| ctx.refuse_codec_limit("NX input block identity group id", 0, 1))?;
        let mut id = ctx.retained_string(id_len, "NX input block identity group id")?;
        write!(&mut id, "{prefix}{ordinal:010}")
            .map_err(|_| ctx.refuse_codec_limit("format NX input block identity group id", 0, 1))?;
        ctx.reserve_vec(&mut output, 1, "NX input block identity groups")?;
        output.push(FeatureInputBlockIdentityGroup {
            id,
            data_block: ctx.copy_retained_text(data_block, "NX grouped input data block")?,
            members: retained_members,
        });
    }
    Ok(output)
}

fn visit_column_slots<'a>(
    ctx: &DecodeContext<'_>,
    index_rows: &'a [DataBlockIndexRow],
    linked_rows: &'a [DataBlockLinkedIndexRow],
    target_rows: &'a [DataBlockTargetIndexRow],
    mut visit: impl FnMut(
        &'a str,
        &'a str,
        ColumnIndexRowKind,
        ColumnRowSlot,
        u64,
    ) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    for row in index_rows {
        for (slot, token) in ColumnRowSlot::ALL.into_iter().zip(row.frame.indices()) {
            ctx.charge_work(1, "scan NX column row slots")?;
            visit(
                token.target,
                &row.id,
                ColumnIndexRowKind::Index,
                slot,
                token.offset,
            )?;
        }
    }
    for row in linked_rows {
        for (slot, token) in ColumnRowSlot::ALL
            .into_iter()
            .zip(std::iter::once(row.frame.target_index()).chain(row.frame.indices()))
        {
            ctx.charge_work(1, "scan NX column row slots")?;
            visit(
                token.target,
                &row.id,
                ColumnIndexRowKind::LinkedIndex,
                slot,
                token.offset,
            )?;
        }
    }
    for row in target_rows {
        for (slot, token) in ColumnRowSlot::ALL
            .into_iter()
            .zip(std::iter::once(row.frame.target_index()).chain(row.frame.indices()))
        {
            ctx.charge_work(1, "scan NX column row slots")?;
            visit(
                token.target,
                &row.id,
                ColumnIndexRowKind::TargetIndex,
                slot,
                token.offset,
            )?;
        }
    }
    Ok(())
}

fn unique_column_table<'a>(
    ctx: &DecodeContext<'_>,
    row: &str,
    tables: &'a [DataBlockColumnIndexTable],
) -> Result<Option<&'a str>, CodecError> {
    let mut unique = None;
    for table in tables {
        for candidate in std::iter::once(table.opening_linked_row.as_str())
            .chain(table.rows.target_rows().iter().map(String::as_str))
            .chain(table.rows.linked_rows().iter().map(String::as_str))
        {
            ctx.charge_work(1, "resolve NX column row table")?;
            if candidate != row {
                continue;
            }
            if unique.is_some() {
                return Ok(None);
            }
            unique = Some(table.id.as_str());
        }
    }
    Ok(unique)
}

fn format_column_relation_id(
    ctx: &DecodeContext<'_>,
    prefix: &'static str,
    key: &str,
    construction_slot: Option<DatumCsysSlot>,
    row_kind: ColumnIndexRowKind,
    ordinal: usize,
) -> Result<String, CodecError> {
    let mut value = ordinal;
    let mut digits = 1usize;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    let kind = row_kind.id_component();
    let length = prefix
        .len()
        .checked_add(key.len())
        .and_then(|length| length.checked_add(kind.len()))
        .and_then(|length| length.checked_add(digits.max(10)))
        .and_then(|length| length.checked_add(if construction_slot.is_some() { 13 } else { 2 }))
        .ok_or_else(|| ctx.refuse_codec_limit("format NX column relation identity", 0, 1))?;
    let mut id = ctx.retained_string(length, "NX column relation identity")?;
    if let Some(slot) = construction_slot {
        write!(&mut id, "{prefix}{key}-{slot:010}-{kind}-{ordinal:010}")
            .map_err(|_| ctx.refuse_codec_limit("format NX column relation identity", 0, 1))?;
    } else {
        write!(&mut id, "{prefix}{key}-{kind}-{ordinal:010}")
            .map_err(|_| ctx.refuse_codec_limit("format NX column relation identity", 0, 1))?;
    }
    Ok(id)
}

/// Join feature inputs to every column-row slot addressing the same block.
pub(super) fn feature_input_column_row_uses(
    ctx: &DecodeContext<'_>,
    inputs: &[FeatureInputBlock],
    index_rows: &[DataBlockIndexRow],
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
    tables: &[DataBlockColumnIndexTable],
) -> Result<Vec<FeatureInputColumnRowUse>, CodecError> {
    let mut output = Vec::new();
    for input in inputs {
        let mut ordinal = 0usize;
        visit_column_slots(
            ctx,
            index_rows,
            linked_rows,
            target_rows,
            |data_block, row, row_kind, slot, source_offset| {
                if data_block != input.data_block {
                    return Ok(());
                }
                let table = unique_column_table(ctx, row, tables)?;
                ctx.reserve_vec(&mut output, 1, "NX input column row uses")?;
                output.push(FeatureInputColumnRowUse {
                    id: format_column_relation_id(
                        ctx,
                        "nx:feature-history:input-column-row-use#",
                        input.id.rsplit_once('#').map_or("unknown", |(_, key)| key),
                        None,
                        row_kind,
                        ordinal,
                    )?,
                    input_block: ctx
                        .copy_retained_text(&input.id, "NX input column row input identity")?,
                    operation_label: ctx.copy_retained_text(
                        &input.operation_label,
                        "NX input column row operation label",
                    )?,
                    input_slot: input.input_slot,
                    row_kind,
                    column_row: ctx.copy_retained_text(row, "NX input column row identity")?,
                    column_table: table
                        .map(|id| ctx.copy_retained_text(id, "NX input column table identity"))
                        .transpose()?,
                    row_slot: slot,
                    data_block: ctx
                        .copy_retained_text(&input.data_block, "NX input column data block")?,
                    source_offset,
                });
                ordinal = ordinal.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("count NX input column row uses", 0, 1)
                })?;
                Ok(())
            },
        )?;
    }
    Ok(output)
}

/// Join every datum-CSYS construction lane to column-row slots addressing the
/// same block. The relation assigns no geometric role to either lane.
pub(super) fn feature_datum_csys_column_row_uses(
    ctx: &DecodeContext<'_>,
    constructions: &[FeatureDatumCsysConstruction],
    index_rows: &[DataBlockIndexRow],
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
    tables: &[DataBlockColumnIndexTable],
) -> Result<Vec<FeatureDatumCsysColumnRowUse>, CodecError> {
    let mut output = Vec::new();
    for construction in constructions {
        for (construction_slot, (_, data_block, source_offset)) in DatumCsysSlot::ALL
            .into_iter()
            .zip(construction.frame.references())
        {
            let mut ordinal = 0usize;
            visit_column_slots(
                ctx,
                index_rows,
                linked_rows,
                target_rows,
                |target, row, row_kind, row_slot, row_source_offset| {
                    if target != data_block {
                        return Ok(());
                    }
                    let table = unique_column_table(ctx, row, tables)?;
                    ctx.reserve_vec(&mut output, 1, "NX datum CSYS column row uses")?;
                    output.push(FeatureDatumCsysColumnRowUse {
                        id: format_column_relation_id(
                            ctx,
                            "nx:feature-history:datum-csys-column-row-use#",
                            construction
                                .id
                                .rsplit_once('#')
                                .map_or("unknown", |(_, key)| key),
                            Some(construction_slot),
                            row_kind,
                            ordinal,
                        )?,
                        construction: ctx.copy_retained_text(
                            &construction.id,
                            "NX datum CSYS column construction identity",
                        )?,
                        operation_label: ctx.copy_retained_text(
                            &construction.operation_label,
                            "NX datum CSYS column operation label",
                        )?,
                        construction_slot,
                        row_kind,
                        column_row: ctx
                            .copy_retained_text(row, "NX datum CSYS column row identity")?,
                        column_table: table
                            .map(|id| {
                                ctx.copy_retained_text(id, "NX datum CSYS column table identity")
                            })
                            .transpose()?,
                        row_slot,
                        data_block: ctx
                            .copy_retained_text(data_block, "NX datum CSYS column data block")?,
                        construction_source_offset: source_offset,
                        row_source_offset,
                    });
                    ordinal = ordinal.checked_add(1).ok_or_else(|| {
                        ctx.refuse_codec_limit("count NX datum CSYS column row uses", 0, 1)
                    })?;
                    Ok(())
                },
            )?;
        }
    }
    Ok(output)
}

/// Retain inputs having exactly one slot-zero use in one complete column table.
pub(super) fn feature_input_column_targets(
    ctx: &DecodeContext<'_>,
    inputs: &[FeatureInputBlock],
    uses: &[FeatureInputColumnRowUse],
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
) -> Result<Vec<FeatureInputColumnTarget>, CodecError> {
    let scan_width = uses
        .len()
        .checked_add(linked_rows.len())
        .and_then(|count| count.checked_add(target_rows.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("resolve NX input column targets", 0, 1))?;
    let work = inputs
        .len()
        .checked_mul(scan_width)
        .ok_or_else(|| ctx.refuse_codec_limit("resolve NX input column targets", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "resolve NX input column targets",
    )?;
    let mut output = Vec::new();
    for input in inputs {
        let mut targets = uses.iter().filter(|use_| {
            use_.input_block == input.id
                && use_.row_slot == ColumnRowSlot::Zero
                && use_.row_kind != ColumnIndexRowKind::Index
                && use_.column_table.is_some()
        });
        let Some(target) = targets.next() else {
            continue;
        };
        if targets.next().is_some() {
            continue;
        }
        let Some(column_table) = target.column_table.as_ref() else {
            continue;
        };
        let (row, field_indices, field_data_blocks, field_source_offsets, mode) = match target
            .row_kind
        {
            ColumnIndexRowKind::LinkedIndex => {
                let mut rows = linked_rows.iter().filter(|row| row.id == target.column_row);
                let Some(row) = rows.next() else {
                    continue;
                };
                if rows.next().is_some() {
                    continue;
                }
                let [first, second, third] = row.frame.indices().map(|token| {
                    ctx.copy_retained_text(token.target, "NX input column target field data block")
                });
                (
                    FeatureInputColumnTargetRow::Linked {
                        leading_index: row.frame.first_index().atom.value(),
                        leading_index_source_offset: row.frame.first_index().offset,
                        discriminator: row.frame.discriminator(),
                        flag: row.frame.flag(),
                    },
                    row.frame.indices().map(|token| token.atom.value()),
                    [first?, second?, third?],
                    row.frame.indices().map(|token| token.offset),
                    row.frame.mode(),
                )
            }
            ColumnIndexRowKind::TargetIndex => {
                let mut rows = target_rows.iter().filter(|row| row.id == target.column_row);
                let Some(row) = rows.next() else {
                    continue;
                };
                if rows.next().is_some() {
                    continue;
                }
                let [first, second, third] = row.frame.indices().map(|token| {
                    ctx.copy_retained_text(token.target, "NX input column target field data block")
                });
                (
                    FeatureInputColumnTargetRow::Target,
                    row.frame.indices().map(|token| token.atom.value()),
                    [first?, second?, third?],
                    row.frame.indices().map(|token| token.offset),
                    row.frame.mode(),
                )
            }
            ColumnIndexRowKind::Index => continue,
        };
        let key = input.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let prefix = "nx:feature-history:input-column-target#";
        let id_len = prefix
            .len()
            .checked_add(key.len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX input column target identity", 0, 1))?;
        let mut id = ctx.retained_string(id_len, "NX input column target identity")?;
        id.push_str(prefix);
        id.push_str(key);
        ctx.reserve_vec(&mut output, 1, "NX input column targets")?;
        output.push(FeatureInputColumnTarget {
            id,
            input_block: ctx
                .copy_retained_text(&input.id, "NX input column target input identity")?,
            operation_label: ctx.copy_retained_text(
                &input.operation_label,
                "NX input column target operation label",
            )?,
            input_slot: input.input_slot,
            column_row: ctx
                .copy_retained_text(&target.column_row, "NX input column target row identity")?,
            row,
            field_indices,
            field_data_blocks,
            field_source_offsets,
            mode,
            column_table: ctx
                .copy_retained_text(column_table, "NX input column target table identity")?,
            data_block: ctx
                .copy_retained_text(&input.data_block, "NX input column target data block")?,
            source_offset: target.source_offset,
        });
    }
    Ok(output)
}

/// Decode and atomically resolve datum coordinate-system construction lanes
/// through the offset store selected by each operation header.
pub(super) fn feature_datum_csys_constructions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureDatumCsysConstruction>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let inputs = feature_input_blocks(ctx, container)?;
    let mut constructions = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let Some(field) = crate::om::datum_csys::datum_csys_references(record.payload_view())
            else {
                return;
            };
            let result = (|| -> Result<(), CodecError> {
                let operation_label = format_feature_history_id(
                    ctx,
                    "operation-label",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(inputs.len()),
                    "resolve NX datum CSYS input prefix",
                )?;
                let mut input_prefix = None;
                for input in &inputs {
                    if input.operation_label != operation_label {
                        continue;
                    }
                    let Some((prefix, _)) = input.data_block.rsplit_once(":block#") else {
                        continue;
                    };
                    match input_prefix {
                        None => input_prefix = Some(prefix),
                        Some(existing) if existing == prefix => {}
                        Some(_) => return Ok(()),
                    }
                }
                let Some(input_prefix) = input_prefix else {
                    return Ok(());
                };
                let Some(field) = field.relocate(entry_offset) else {
                    return Ok(());
                };
                let Some(frame) = field.resolve(|index| {
                    let Some(data_block) = charged_unique_offset_data_block(ctx, &indexed, index)?
                    else {
                        return Ok(None);
                    };
                    Ok((data_block.rsplit_once(":block#").map(|(prefix, _)| prefix)
                        == Some(input_prefix))
                    .then_some(data_block))
                })?
                else {
                    return Ok(());
                };
                let id = format_feature_history_id(
                    ctx,
                    "datum-csys-construction",
                    section_key,
                    operation_ordinal,
                    None,
                )?;
                ctx.reserve_vec(&mut constructions, 1, "NX datum CSYS constructions")?;
                constructions.push(FeatureDatumCsysConstruction {
                    id,
                    operation_label,
                    frame,
                });
                Ok(())
            })();
            if let Err(error) = result {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(constructions)
}

/// Reconstruct datum-plane object payloads across ordered store blocks.
pub(super) fn feature_datum_plane_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    headers: &[FeatureDatumPlaneHeader],
) -> Result<Vec<FeatureDatumPlanePayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut output = Vec::new();
    for header in headers {
        if header
            .resolved_data_blocks(DatumPlaneBlockLane::Object)
            .next()
            .is_none()
        {
            continue;
        }
        let mut reservation = ctx.reserve_scoped(0, "copy NX datum plane source blocks")?;
        let mut data_blocks = Vec::new();
        for source in header
            .resolved_data_blocks(DatumPlaneBlockLane::Object)
            .map(String::as_str)
        {
            ctx.charge_work(1, "copy NX datum plane source blocks")?;
            let owned = ctx.copy_retained_text(source, "copy NX datum plane source blocks")?;
            reservation.with_storage(|| {
                ctx.push_vec(&mut data_blocks, owned, "copy NX datum plane source blocks")
            })?;
        }
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };
        drop(reservation);
        let Some(joined) = JoinedPayload::from_source(ctx, content.block_ids(), &blocks)? else {
            continue;
        };
        let lanes = crate::om::datum_index::scan(ctx, joined.bytes())?;
        let lane = <[_; 1]>::try_from(lanes).ok().map(|[lane]| lane);
        let key = header.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let prefix = "nx:feature-history:datum-plane-payload#";
        let id_len = prefix
            .len()
            .checked_add(key.len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX datum plane payload identity", 0, 1))?;
        let mut id = ctx.retained_string(id_len, "NX datum plane payload identity")?;
        id.push_str(prefix);
        id.push_str(key);
        let operation_label = ctx.copy_retained_text(
            &header.operation_label,
            "NX datum plane payload operation label",
        )?;
        let datum_plane_header =
            ctx.copy_retained_text(&header.id, "NX datum plane payload header identity")?;
        ctx.reserve_vec(&mut output, 1, "NX datum plane payloads")?;
        output.push(FeatureDatumPlanePayload {
            id,
            operation_label,
            datum_plane_header,
            content,
            index_lane: lane.map(crate::om::datum_index::DatumIndexLane::into_u64),
        });
    }
    Ok(output)
}

/// Reconstruct the two leading object blocks of each datum coordinate system.
pub(super) fn feature_datum_csys_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    constructions: &[FeatureDatumCsysConstruction],
) -> Result<Vec<FeatureDatumCsysPayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut output = Vec::new();
    for construction in constructions {
        let first = &construction.frame.members()[0].1;
        let second = &construction.frame.members()[1].1;

        let copy = |value: &str| -> Result<String, CodecError> {
            let mut id = String::new();
            ctx.try_reserve_retained_text(
                &mut id,
                value.len(),
                "allocate NX datum CSYS source block identity",
            )?;
            id.push_str(value);
            Ok(id)
        };
        let data_blocks = [copy(first)?, copy(second)?];
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };

        let id = replace_operation_text(
            ctx,
            &construction.id,
            "datum-csys-construction",
            "datum-csys-payload",
            "NX datum CSYS payload identity",
        )?;
        let operation_label = ctx.copy_retained_text(
            &construction.operation_label,
            "NX datum CSYS payload operation label",
        )?;
        let construction_id = ctx.copy_retained_text(
            &construction.id,
            "NX datum CSYS payload construction identity",
        )?;
        ctx.reserve_vec(&mut output, 1, "NX datum CSYS payloads")?;
        output.push(FeatureDatumCsysPayload {
            id,
            operation_label,
            construction: construction_id,
            content,
        });
    }
    Ok(output)
}

/// Shared body for construction-payload frame extractors. Reconstruct each
/// payload's concatenated bytes, build the payload-relative-to-source-offset
/// mapper once, scan the bytes, and let each family build its record, dropping
/// frames whose offsets fall outside a source block. Extractors differ only in
/// their payload block lane, scanner, and output record.
fn construction_payload_frames<P, S, R>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    payloads: &[P],
    data_blocks: impl Fn(&P) -> &[FeaturePayloadBlock],
    scan: impl Fn(&[u8]) -> Result<Vec<S>, CodecError>,
    mut build: impl FnMut(&P, usize, S, &dyn Fn(usize) -> Option<u64>) -> Result<Option<R>, CodecError>,
) -> Result<Vec<R>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut output = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(
            ctx,
            data_blocks(payload).iter().map(|block| &block.id),
            &blocks,
        )?
        else {
            continue;
        };
        let source_offset =
            |relative: usize| joined.source_offset(cadmpeg_core::decode::u64_from_index(relative));
        for (ordinal, row) in scan(joined.bytes())?.into_iter().enumerate() {
            let Some(record) = build(payload, ordinal, row, &source_offset)? else {
                continue;
            };
            ctx.reserve_vec(&mut output, 1, "NX construction payload frames")?;
            output.push(record);
        }
    }
    Ok(output)
}

/// Decode exact scalar-pair frames from reconstructed datum-CSYS payloads.
pub(super) fn feature_datum_csys_payload_scalar_pairs(
    ctx: &DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Result<Vec<FeaturePayloadScalarPair>, CodecError> {
    construction_payload_frames(
        ctx,
        container,
        payloads,
        |payload| payload.content.blocks(),
        |bytes| crate::om::binary64_pair::object_pairs(ctx, bytes),
        |payload, ordinal, pair, source_offset| {
            let Some((frame, first, second, source)) = (|| {
                Some((
                    pair.into_wire_frame()?,
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                    source_offset(pair.offset())?,
                ))
            })() else {
                return Ok(None);
            };
            let id = format_feature_child_id(ctx, &payload.id, "-scalar-pair-", ordinal)?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX datum CSYS scalar pair ordinal", 0, 1))?;
            Ok(Some(FeaturePayloadScalarPair {
                id,
                operation_label: ctx.copy_retained_text(
                    &payload.operation_label,
                    "NX datum CSYS scalar pair label",
                )?,
                payload: FeatureScalarPairPayload::DatumCsys {
                    datum_csys_payload: ctx
                        .copy_retained_text(&payload.id, "NX datum CSYS scalar pair payload")?,
                    frame,
                },
                ordinal,
                value_source_offsets: [first, second],
                source_offset: source,
            }))
        },
    )
}

/// Decode complete signed Q1.55 pair frames from reconstructed datum-CSYS payloads.
pub(super) fn feature_datum_csys_payload_fixed_pairs(
    ctx: &DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Result<Vec<FeatureDatumCsysPayloadFixedPair>, CodecError> {
    construction_payload_frames(
        ctx,
        container,
        payloads,
        |payload| payload.content.blocks(),
        |bytes| crate::om::datum_csys_payload_fixed_pairs(ctx, bytes),
        |payload, ordinal, pair, source_offset| {
            let Some((position, source, first, second)) = (|| {
                Some((
                    PairPosition::new(
                        pair.form,
                        cadmpeg_core::decode::u64_from_index(pair.offset),
                    )?,
                    source_offset(pair.offset)?,
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                ))
            })() else {
                return Ok(None);
            };
            let id = format_feature_child_id(ctx, &payload.id, "-fixed-pair-", ordinal)?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX datum CSYS fixed pair ordinal", 0, 1))?;
            Ok(Some(FeatureDatumCsysPayloadFixedPair {
                id,
                operation_label: ctx.copy_retained_text(
                    &payload.operation_label,
                    "NX datum CSYS fixed pair label",
                )?,
                datum_csys_payload: ctx
                    .copy_retained_text(&payload.id, "NX datum CSYS fixed pair payload")?,
                ordinal,
                values: pair.values,
                position,
                source_offset: source,
                value_source_offsets: [first, second],
            }))
        },
    )
}

/// Decode complete shifted-binary64 fields from reconstructed datum-CSYS payloads.
pub(super) fn feature_datum_csys_payload_scalars(
    ctx: &DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Result<Vec<FeaturePayloadScalar>, CodecError> {
    construction_payload_frames(
        ctx,
        container,
        payloads,
        |payload| payload.content.blocks(),
        |bytes| crate::om::construction_payload_scalar_fields(ctx, bytes),
        |payload, ordinal, scalar, source_offset| {
            let Some(source) = source_offset(scalar.offset) else {
                return Ok(None);
            };
            let id = format_feature_child_id(ctx, &payload.id, "-scalar-", ordinal)?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX datum CSYS scalar ordinal", 0, 1))?;
            Ok(Some(FeaturePayloadScalar {
                id,
                operation_label: ctx
                    .copy_retained_text(&payload.operation_label, "NX datum CSYS scalar label")?,
                payload: FeatureScalarPayload::DatumCsys {
                    datum_csys_payload: ctx
                        .copy_retained_text(&payload.id, "NX datum CSYS scalar payload")?,
                },
                ordinal,
                field_code: scalar.field_code,
                scalar: scalar.scalar,
                payload_offset: cadmpeg_core::decode::u64_from_index(scalar.offset),
                source_offset: source,
            }))
        },
    )
}

/// Decode the final three descriptor lanes of datum coordinate systems.
pub(super) fn feature_datum_csys_descriptors(
    ctx: &DecodeContext<'_>,
    container: &Container,
    constructions: &[FeatureDatumCsysConstruction],
) -> Result<Vec<FeatureDatumCsysDescriptor>, CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut descriptors = Vec::new();
    for construction in constructions {
        for slot in [
            CsysDescriptorSlot::Five,
            CsysDescriptorSlot::Six,
            CsysDescriptorSlot::Seven,
        ] {
            let reference_ordinal = u8::from(slot);
            let data_block = &construction.frame.members()[usize::from(reference_ordinal)].1;
            let Some(&(bytes, source_offset)) = blocks.get(data_block) else {
                continue;
            };
            let Some(descriptor) = crate::om::datum_csys_descriptor_block(ctx, bytes)? else {
                continue;
            };
            let Some(descriptor) = LocatedCsysDescriptor::new(descriptor, source_offset).ok()
            else {
                continue;
            };
            let id = ctx.format_retained(
                format_args!("{}-descriptor-{reference_ordinal}", construction.id),
                "NX datum CSYS descriptor identity",
            )?;
            let operation_label = ctx.copy_retained_text(
                &construction.operation_label,
                "NX datum CSYS descriptor operation label",
            )?;
            let construction_id = ctx.copy_retained_text(
                &construction.id,
                "NX datum CSYS descriptor construction identity",
            )?;
            let data_block =
                ctx.copy_retained_text(data_block, "NX datum CSYS descriptor data block")?;
            ctx.reserve_vec(&mut descriptors, 1, "NX datum CSYS descriptors")?;
            descriptors.push(FeatureDatumCsysDescriptor {
                id,
                operation_label,
                construction: construction_id,
                reference_ordinal: slot,
                data_block,
                descriptor,
            });
        }
    }
    Ok(descriptors)
}

/// Join equal typed descriptor identities across datum-plane and datum-CSYS history.
pub(super) fn feature_datum_plane_csys_identity_uses(
    ctx: &DecodeContext<'_>,
    plane_descriptors: &[FeatureDatumPlaneDescriptor],
    csys_descriptors: &[FeatureDatumCsysDescriptor],
) -> Result<Vec<FeatureDatumPlaneCsysIdentityUse>, CodecError> {
    let work = plane_descriptors
        .len()
        .checked_mul(csys_descriptors.len())
        .ok_or_else(|| ctx.refuse_codec_limit("join NX datum descriptor identities", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "join NX datum descriptor identities",
    )?;
    let mut uses = Vec::new();
    for plane in plane_descriptors {
        for csys in csys_descriptors {
            if csys.descriptor.descriptor().identity().as_str() != plane.descriptor.identity() {
                continue;
            }
            let plane_key = plane.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
            let csys_key = csys.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
            let id = ctx.format_retained(
                format_args!(
                    "nx:feature-history:datum-plane-csys-identity-use#{plane_key}-{csys_key}"
                ),
                "NX datum descriptor identity use",
            )?;
            let identity_text = ctx.copy_retained_text(
                csys.descriptor.descriptor().identity().as_str(),
                "NX datum descriptor shared identity",
            )?;
            let identity = CsysIdentity::try_from(identity_text)
                .map_err(|error| CodecError::Malformed(error.to_owned()))?;
            let datum_plane_descriptor =
                ctx.copy_retained_text(&plane.id, "NX datum identity plane descriptor")?;
            let datum_plane_operation_label =
                ctx.copy_retained_text(&plane.operation_label, "NX datum identity plane label")?;
            let datum_csys_descriptor =
                ctx.copy_retained_text(&csys.id, "NX datum identity CSYS descriptor")?;
            let datum_csys_operation_label =
                ctx.copy_retained_text(&csys.operation_label, "NX datum identity CSYS label")?;
            ctx.reserve_vec(&mut uses, 1, "NX datum descriptor identity uses")?;
            uses.push(FeatureDatumPlaneCsysIdentityUse {
                id,
                identity,
                datum_plane_descriptor,
                datum_plane_operation_label,
                datum_csys_descriptor,
                datum_csys_operation_label,
                datum_csys_reference_ordinal: csys.reference_ordinal,
            });
        }
    }
    Ok(uses)
}

/// Decode exact scalar-pair frames from reconstructed datum-plane payloads.
pub(super) fn feature_datum_plane_payload_scalar_pairs(
    ctx: &DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureDatumPlanePayload],
) -> Result<Vec<FeaturePayloadScalarPair>, CodecError> {
    construction_payload_frames(
        ctx,
        container,
        payloads,
        |payload| payload.content.blocks(),
        |bytes| crate::om::binary64_pair::datum_plane_pairs(ctx, bytes),
        |payload, ordinal, pair, source_offset| {
            let Some((frame, first, second, source)) = (|| {
                Some((
                    pair.into_wire_frame()?,
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                    source_offset(pair.offset())?,
                ))
            })() else {
                return Ok(None);
            };
            let id = format_feature_child_id(ctx, &payload.id, "-scalar-pair-", ordinal)?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX datum plane scalar pair ordinal", 0, 1))?;
            Ok(Some(FeaturePayloadScalarPair {
                id,
                operation_label: ctx.copy_retained_text(
                    &payload.operation_label,
                    "NX datum plane scalar pair label",
                )?,
                payload: FeatureScalarPairPayload::DatumPlane {
                    datum_plane_payload: ctx
                        .copy_retained_text(&payload.id, "NX datum plane scalar pair payload")?,
                    frame,
                },
                ordinal,
                value_source_offsets: [first, second],
                source_offset: source,
            }))
        },
    )
}

/// Decode atomically resolved datum-plane descriptor blocks.
pub(super) fn feature_datum_plane_descriptors(
    ctx: &DecodeContext<'_>,
    container: &Container,
    headers: &[FeatureDatumPlaneHeader],
) -> Result<Vec<FeatureDatumPlaneDescriptor>, CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut descriptors = Vec::new();
    for header in headers {
        for (ordinal, data_block) in header
            .resolved_data_blocks(DatumPlaneBlockLane::Descriptor)
            .enumerate()
        {
            let Some(&(bytes, source_offset)) = blocks.get(data_block) else {
                continue;
            };
            let Some(descriptor) = crate::om::datum_plane_descriptor_block(ctx, bytes)? else {
                continue;
            };
            let id = ctx.format_retained(
                format_args!("{}-descriptor-{ordinal:010}", header.id),
                "NX datum plane descriptor identity",
            )?;
            let operation_label = ctx.copy_retained_text(
                &header.operation_label,
                "NX datum plane descriptor operation label",
            )?;
            let datum_plane_header =
                ctx.copy_retained_text(&header.id, "NX datum plane descriptor header identity")?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX datum plane descriptor ordinal", 0, 1))?;
            let data_block =
                ctx.copy_retained_text(data_block, "NX datum plane descriptor data block")?;
            ctx.reserve_vec(&mut descriptors, 1, "NX datum plane descriptors")?;
            descriptors.push(FeatureDatumPlaneDescriptor {
                id,
                operation_label,
                datum_plane_header,
                ordinal,
                data_block,
                descriptor,
                source_offset,
            });
        }
    }
    Ok(descriptors)
}

/// Join resolved datum-plane blocks to operation inputs addressing the same block.
pub(super) fn feature_datum_plane_block_uses(
    ctx: &DecodeContext<'_>,
    headers: &[FeatureDatumPlaneHeader],
    inputs: &[FeatureInputBlock],
) -> Result<Vec<FeatureDatumPlaneBlockUse>, CodecError> {
    let mut uses = Vec::new();
    for header in headers {
        let construction_key = header
            .operation_label
            .rsplit_once('#')
            .map_or(header.operation_label.as_str(), |(_, key)| key);
        for lane in [DatumPlaneBlockLane::Descriptor, DatumPlaneBlockLane::Object] {
            for (reference_ordinal, data_block) in header.resolved_data_blocks(lane).enumerate() {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(inputs.len()),
                    "join NX datum plane input blocks",
                )?;
                for input in inputs
                    .iter()
                    .filter(|input| input.data_block == *data_block)
                {
                    let input_key = input
                        .operation_label
                        .rsplit_once('#')
                        .map_or(input.operation_label.as_str(), |(_, key)| key);
                    let lane_key = match lane {
                        DatumPlaneBlockLane::Descriptor => "descriptor",
                        DatumPlaneBlockLane::Object => "object",
                    };
                    let id = ctx.format_retained(format_args!(
                        "nx:feature-history:datum-plane-block-use#{construction_key}-{lane_key}-{reference_ordinal}-{input_key}-{}",
                        input.input_slot), "NX datum plane block use identity")?;
                    let datum_plane_header =
                        ctx.copy_retained_text(&header.id, "NX datum plane block use header")?;
                    let construction_operation_label = ctx.copy_retained_text(
                        &header.operation_label,
                        "NX datum plane block use construction label",
                    )?;
                    let reference_ordinal = u32::try_from(reference_ordinal).map_err(|_| {
                        ctx.refuse_codec_limit("NX datum plane block use ordinal", 0, 1)
                    })?;
                    let data_block =
                        ctx.copy_retained_text(data_block, "NX datum plane block use data block")?;
                    let input_binding =
                        ctx.copy_retained_text(&input.id, "NX datum plane block use input")?;
                    let input_operation_label = ctx.copy_retained_text(
                        &input.operation_label,
                        "NX datum plane block use input label",
                    )?;
                    ctx.reserve_vec(&mut uses, 1, "NX datum plane block uses")?;
                    uses.push(FeatureDatumPlaneBlockUse {
                        id,
                        datum_plane_header,
                        construction_operation_label,
                        lane,
                        reference_ordinal,
                        data_block,
                        input_binding,
                        input_operation_label,
                        input_slot: input.input_slot,
                    });
                }
            }
        }
    }
    Ok(uses)
}

/// Join resolved datum-coordinate-system blocks to every exact operation input
/// addressing the same native block.
pub(super) fn feature_datum_csys_block_uses(
    ctx: &DecodeContext<'_>,
    constructions: &[FeatureDatumCsysConstruction],
    inputs: &[FeatureInputBlock],
) -> Result<Vec<FeatureDatumCsysBlockUse>, CodecError> {
    let mut uses = Vec::new();
    for construction in constructions {
        for (reference_ordinal, reference) in DatumCsysSlot::ALL
            .into_iter()
            .zip(construction.frame.members())
        {
            let data_block = &reference.1;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(inputs.len()),
                "join NX datum CSYS input blocks",
            )?;
            for input in inputs
                .iter()
                .filter(|input| input.data_block == *data_block)
            {
                let construction_key = construction
                    .operation_label
                    .rsplit_once('#')
                    .map_or(construction.operation_label.as_str(), |(_, key)| key);
                let input_key = input
                    .operation_label
                    .rsplit_once('#')
                    .map_or(input.operation_label.as_str(), |(_, key)| key);
                let id = ctx.format_retained(format_args!(
                    "nx:feature-history:datum-csys-block-use#{construction_key}-{reference_ordinal}-{input_key}-{}",
                    input.input_slot), "NX datum CSYS block use identity")?;
                let construction_id = ctx
                    .copy_retained_text(&construction.id, "NX datum CSYS block use construction")?;
                let construction_operation_label = ctx.copy_retained_text(
                    &construction.operation_label,
                    "NX datum CSYS block use construction label",
                )?;
                let data_block =
                    ctx.copy_retained_text(data_block, "NX datum CSYS block use data block")?;
                let input_binding =
                    ctx.copy_retained_text(&input.id, "NX datum CSYS block use input")?;
                let input_operation_label = ctx.copy_retained_text(
                    &input.operation_label,
                    "NX datum CSYS block use input label",
                )?;
                ctx.reserve_vec(&mut uses, 1, "NX datum CSYS block uses")?;
                uses.push(FeatureDatumCsysBlockUse {
                    id,
                    construction: construction_id,
                    construction_operation_label,
                    reference_ordinal,
                    data_block,
                    input_binding,
                    input_operation_label,
                    input_slot: input.input_slot,
                });
            }
        }
    }
    Ok(uses)
}

/// Join each sketch operation to its bounded record and ordered input blocks.
pub(super) fn feature_sketch_records(
    ctx: &DecodeContext<'_>,
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    inputs: &[FeatureInputBlock],
    references: &[FeatureSketchReference],
) -> Result<Vec<FeatureSketchRecord>, CodecError> {
    let mut sketches = Vec::new();
    for label in labels.iter().filter(|label| label.value == "SKETCH") {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(records.len()),
            "resolve NX sketch operation record",
        )?;
        let mut operation_records = records
            .iter()
            .filter(|record| record.operation_label == label.id);
        let Some(record) = operation_records.next() else {
            continue;
        };
        if operation_records.next().is_some() {
            continue;
        }

        let mut input_blocks = Vec::new();
        let mut input_reservation = ctx.reserve_scoped(0, "sort NX sketch input blocks")?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(inputs.len()),
            "resolve NX sketch input blocks",
        )?;
        for input in inputs
            .iter()
            .filter(|input| input.operation_label == label.id)
        {
            input_reservation.with_storage(|| {
                ctx.reserve_vec(&mut input_blocks, 1, "NX sketch input block order")
            })?;
            input_blocks.push(input);
        }
        ctx.stable_sort_by(
            &mut input_blocks,
            |value| &value.input_slot,
            Ord::cmp,
            "sort NX sketch input blocks",
        )?;

        let mut payload_references = Vec::new();
        let mut reference_reservation = ctx.reserve_scoped(0, "sort NX sketch references")?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(references.len()),
            "resolve NX sketch references",
        )?;
        for reference in references
            .iter()
            .filter(|reference| reference.operation_label == label.id)
        {
            reference_reservation.with_storage(|| {
                ctx.reserve_vec(&mut payload_references, 1, "NX sketch reference order")
            })?;
            payload_references.push(reference);
        }
        ctx.stable_sort_by_key(
            &mut payload_references,
            |value| value.position.ordinal(),
            Ord::cmp,
            "sort NX sketch references",
        )?;

        let mut input_ids = Vec::new();
        for input in input_blocks {
            let id = ctx.copy_retained_text(&input.id, "NX sketch input identity")?;
            ctx.reserve_vec(&mut input_ids, 1, "NX sketch input identities")?;
            input_ids.push(id);
        }
        drop(input_reservation);
        let mut reference_ids = Vec::new();
        for reference in payload_references {
            let id = ctx.copy_retained_text(&reference.id, "NX sketch reference identity")?;
            ctx.reserve_vec(&mut reference_ids, 1, "NX sketch reference identities")?;
            reference_ids.push(id);
        }
        drop(reference_reservation);
        let id = replace_operation_text(
            ctx,
            &label.id,
            "operation-label",
            "sketch-record",
            "NX sketch record identity",
        )?;
        let operation_label =
            ctx.copy_retained_text(&label.id, "NX sketch record operation label")?;
        let operation_record =
            ctx.copy_retained_text(&record.id, "NX sketch operation record identity")?;
        ctx.reserve_vec(&mut sketches, 1, "NX sketch records")?;
        sketches.push(FeatureSketchRecord {
            id,
            operation_label,
            ordinal: label.ordinal,
            operation_record,
            input_blocks: input_ids,
            payload_references: reference_ids,
            source_offset: label.source_offset,
        });
    }
    Ok(sketches)
}

/// Join complete, uniquely resolved sketch construction-reference fields.
pub(super) fn feature_sketch_construction_inputs(
    ctx: &DecodeContext<'_>,
    sketches: &[FeatureSketchRecord],
    references: &[FeatureSketchReference],
) -> Result<Vec<FeatureSketchConstructionInputs>, CodecError> {
    let mut inputs = Vec::new();
    for sketch in sketches {
        let mut field = Vec::new();
        let mut field_reservation =
            ctx.reserve_scoped(0, "sort NX sketch construction references")?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(references.len()),
            "resolve NX sketch construction references",
        )?;
        for reference in references
            .iter()
            .filter(|reference| reference.operation_label == sketch.operation_label)
        {
            field_reservation.with_storage(|| {
                ctx.reserve_vec(&mut field, 1, "NX sketch construction reference order")
            })?;
            field.push(reference);
        }
        ctx.stable_sort_by_key(
            &mut field,
            |value| value.position.ordinal(),
            Ord::cmp,
            "sort NX sketch construction references",
        )?;
        let Some((terminal, members)) = field.split_last() else {
            continue;
        };
        let expected_len = usize::from(terminal.position.declared_count().effective().get());
        if field.len() != expected_len {
            continue;
        }
        let mut valid = true;
        for (ordinal, reference) in field.iter().enumerate() {
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                ctx.refuse_codec_limit("NX sketch construction reference ordinal", 0, 1)
            })?;
            if reference.position.declared_count() != terminal.position.declared_count()
                || reference.position.ordinal() != ordinal
            {
                valid = false;
                break;
            }
        }
        if !valid
            || members
                .iter()
                .any(|reference| reference.data_block.is_none())
        {
            continue;
        }
        let Some(terminal_data_block) = terminal.data_block.as_ref() else {
            continue;
        };
        let mut member_rows = Vec::new();
        for reference in members {
            let reference_id =
                ctx.copy_retained_text(&reference.id, "NX sketch construction member reference")?;
            let Some(data_block) = reference.data_block.as_ref() else {
                continue;
            };
            let data_block =
                ctx.copy_retained_text(data_block, "NX sketch construction member block")?;
            ctx.reserve_vec(&mut member_rows, 1, "NX sketch construction members")?;
            member_rows.push(FeatureConstructionMember {
                reference: reference_id,
                data_block,
            });
        }
        let id = replace_operation_text(
            ctx,
            &sketch.id,
            "sketch-record",
            "sketch-construction-inputs",
            "NX sketch construction input identity",
        )?;
        let operation_label = ctx.copy_retained_text(
            &sketch.operation_label,
            "NX sketch construction input label",
        )?;
        let sketch_record =
            ctx.copy_retained_text(&sketch.id, "NX sketch construction sketch record")?;
        let terminal_reference =
            ctx.copy_retained_text(&terminal.id, "NX sketch construction terminal reference")?;
        let terminal_data_block =
            ctx.copy_retained_text(terminal_data_block, "NX sketch construction terminal block")?;
        drop(field);
        drop(field_reservation);
        ctx.reserve_vec(&mut inputs, 1, "NX sketch construction inputs")?;
        inputs.push(FeatureSketchConstructionInputs {
            id,
            operation_label,
            sketch_record,
            members: member_rows,
            terminal_reference,
            terminal_data_block,
        });
    }
    Ok(inputs)
}

/// Reconstruct exact sketch payloads across offset-store block boundaries.
pub(super) fn feature_sketch_construction_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    constructions: &[FeatureSketchConstructionInputs],
) -> Result<Vec<FeatureConstructionPayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;

    let mut output = Vec::new();
    for construction in constructions {
        let source_ids = construction
            .members
            .iter()
            .map(|member| member.data_block.as_str())
            .chain(std::iter::once(construction.terminal_data_block.as_str()));
        let mut reservation = ctx.reserve_scoped(0, "copy NX sketch construction source blocks")?;
        let mut data_blocks = Vec::new();
        for source in source_ids {
            ctx.charge_work(1, "copy NX sketch construction source blocks")?;
            let owned =
                ctx.copy_retained_text(source, "copy NX sketch construction source blocks")?;
            reservation.with_storage(|| {
                ctx.push_vec(
                    &mut data_blocks,
                    owned,
                    "copy NX sketch construction source blocks",
                )
            })?;
        }
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };
        drop(reservation);
        let id = replace_operation_text(
            ctx,
            &construction.id,
            "sketch-construction-inputs",
            "sketch-construction-payload",
            "NX sketch construction payload identity",
        )?;
        let operation_label = ctx.copy_retained_text(
            &construction.operation_label,
            "NX sketch construction payload label",
        )?;
        let construction_inputs = ctx.copy_retained_text(
            &construction.id,
            "NX sketch construction payload input identity",
        )?;
        ctx.reserve_vec(&mut output, 1, "NX sketch construction payloads")?;
        output.push(FeatureConstructionPayload {
            id,
            operation_label,
            owner: FeatureConstructionOwner::Sketch {
                construction_inputs,
            },
            content,
        });
    }
    Ok(output)
}

/// Decode exact coordinate-pair frames from reconstructed sketch payloads.
pub(super) fn feature_sketch_payload_coordinate_pairs(
    ctx: &DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeaturePayloadScalarPair>, CodecError> {
    construction_payload_frames(
        ctx,
        container,
        payloads,
        |payload| payload.content.blocks(),
        |bytes| crate::om::binary64_pair::sketch_pairs(ctx, bytes),
        |payload, ordinal, pair, source_offset| {
            let Some((frame, first, second, source)) = (|| {
                Some((
                    pair.into_wire_frame()?,
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                    source_offset(pair.offset())?,
                ))
            })() else {
                return Ok(None);
            };
            let id = format_feature_child_id(ctx, &payload.id, "-coordinate-pair-", ordinal)?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX sketch coordinate pair ordinal", 0, 1))?;
            Ok(Some(FeaturePayloadScalarPair {
                id,
                operation_label: ctx.copy_retained_text(
                    &payload.operation_label,
                    "NX sketch coordinate pair label",
                )?,
                payload: FeatureScalarPairPayload::Construction {
                    construction_payload: ctx
                        .copy_retained_text(&payload.id, "NX sketch coordinate pair payload")?,
                    frame,
                },
                ordinal,
                value_source_offsets: [first, second],
                source_offset: source,
            }))
        },
    )
}

/// Decode exact scaled shifted-binary64 pair frames from reconstructed sketch payloads.
pub(super) fn feature_sketch_payload_fixed_pairs(
    ctx: &DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeatureSketchPayloadFixedPair>, CodecError> {
    construction_payload_frames(
        ctx,
        container,
        payloads,
        |payload| payload.content.blocks(),
        |bytes| crate::om::sketch_payload_fixed_pairs(ctx, bytes),
        |payload, ordinal, pair, source_offset| {
            let Some((position, source, first, second)) = (|| {
                Some((
                    PairPosition::new(
                        pair.form,
                        cadmpeg_core::decode::u64_from_index(pair.offset),
                    )?,
                    source_offset(pair.offset)?,
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                ))
            })() else {
                return Ok(None);
            };
            let id = format_feature_child_id(ctx, &payload.id, "-fixed-pair-", ordinal)?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX sketch fixed pair ordinal", 0, 1))?;
            Ok(Some(FeatureSketchPayloadFixedPair {
                id,
                operation_label: ctx
                    .copy_retained_text(&payload.operation_label, "NX sketch fixed pair label")?,
                construction_payload: ctx
                    .copy_retained_text(&payload.id, "NX sketch fixed pair payload")?,
                ordinal,
                values: pair.values,
                position,
                source_offset: source,
                value_source_offsets: [first, second],
            }))
        },
    )
}

/// Decode exact mixed scaled shifted-binary64/binary32 pair frames from reconstructed sketch payloads.
pub(super) fn feature_sketch_payload_mixed_pairs(
    ctx: &DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeatureSketchPayloadMixedPair>, CodecError> {
    construction_payload_frames(
        ctx,
        container,
        payloads,
        |payload| payload.content.blocks(),
        |bytes| crate::om::sketch_payload_mixed_pairs(ctx, bytes),
        |payload, ordinal, pair, source_offset| {
            let Some((position, source, first, second)) = (|| {
                Some((
                    PairPosition::new(
                        MixedPairForm,
                        cadmpeg_core::decode::u64_from_index(pair.offset),
                    )?,
                    source_offset(pair.offset)?,
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                ))
            })() else {
                return Ok(None);
            };
            let id = format_feature_child_id(ctx, &payload.id, "-mixed-pair-", ordinal)?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX sketch mixed pair ordinal", 0, 1))?;
            Ok(Some(FeatureSketchPayloadMixedPair {
                id,
                operation_label: ctx
                    .copy_retained_text(&payload.operation_label, "NX sketch mixed pair label")?,
                construction_payload: ctx
                    .copy_retained_text(&payload.id, "NX sketch mixed pair payload")?,
                ordinal,
                scalars: pair.scalars,
                position,
                source_offset: source,
                value_source_offsets: [first, second],
            }))
        },
    )
}

type OffsetDataBlocks<'a> = BTreeMap<String, (&'a [u8], u64)>;

struct OffsetDataBlockView<'blocks, 'ctx> {
    blocks: Cow<'blocks, OffsetDataBlocks<'blocks>>,
    _reservation: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

impl<'blocks> std::ops::Deref for OffsetDataBlockView<'blocks, '_> {
    type Target = OffsetDataBlocks<'blocks>;

    fn deref(&self) -> &Self::Target {
        &self.blocks
    }
}

fn offset_data_block_bytes_for_section<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    blocks: &mut OffsetDataBlocks<'a>,
    section_ordinal: usize,
    entry_offset: u64,
    control: &crate::om::EntityRecord<'a>,
    records: &[crate::om::EntityRecord<'a>],
) -> Result<(), cadmpeg_core::CodecError> {
    use std::fmt::Write;

    for (block_ordinal, block) in std::iter::once(control).chain(records.iter()).enumerate() {
        let prefix = "nx:om-data-blocks-";
        let infix = ":block#";
        let length = prefix
            .len()
            .checked_add(
                section_ordinal
                    .checked_ilog10()
                    .map_or(1, |digits| cadmpeg_core::decode::index_from_u32(digits) + 1),
            )
            .and_then(|length| length.checked_add(infix.len()))
            .and_then(|length| {
                length.checked_add(
                    block_ordinal
                        .checked_ilog10()
                        .map_or(1, |digits| cadmpeg_core::decode::index_from_u32(digits) + 1),
                )
            })
            .ok_or_else(|| ctx.refuse_codec_limit("NX offset block view key length", 0, 1))?;

        ctx.charge_work(
            u64::from(usize::BITS - blocks.len().leading_zeros()),
            "index NX offset block view",
        )?;

        let mut key = String::new();
        reservation.with_storage(|| {
            ctx.try_reserve_retained_text(&mut key, length, "NX offset block view storage")
        })?;
        write!(&mut key, "{prefix}{section_ordinal}{infix}{block_ordinal}")
            .map_err(|_| ctx.refuse_codec_limit("format NX offset block view key", 0, 1))?;
        let offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(block.offset))
            .ok_or_else(|| ctx.refuse_codec_limit("NX offset block view source offset", 0, 1))?;
        reservation.with_storage(|| {
            ctx.insert_btree_map(
                blocks,
                key,
                (block.bytes, offset),
                "NX offset block view entries",
            )
        })?;
    }
    Ok(())
}

fn offset_data_block_bytes<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    container: &'a Container<'_>,
) -> Result<OffsetDataBlockView<'a, 'ctx>, cadmpeg_core::CodecError> {
    if let Some(blocks) = container.cached_offset_data_block_bytes() {
        return Ok(OffsetDataBlockView {
            blocks: Cow::Borrowed(blocks),
            _reservation: None,
        });
    }
    let indexed = container.indexed_om_sections(ctx)?;
    if let Some(blocks) = container.cached_offset_data_block_bytes() {
        return Ok(OffsetDataBlockView {
            blocks: Cow::Borrowed(blocks),
            _reservation: None,
        });
    }
    let mut reservation = ctx.reserve_scoped(0, "NX offset block view storage")?;
    let mut blocks = BTreeMap::new();
    for (section_ordinal, (entry, section)) in indexed.into_iter().enumerate() {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        offset_data_block_bytes_for_section(
            ctx,
            &mut reservation,
            &mut blocks,
            section_ordinal,
            entry_offset,
            control,
            records,
        )?;
    }
    Ok(OffsetDataBlockView {
        blocks: Cow::Owned(blocks),
        _reservation: Some(reservation),
    })
}

/// Decode exact framed scalar fields across reconstructed sketch payloads.
pub(super) fn feature_sketch_payload_scalars(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    constructions: &[FeatureSketchConstructionInputs],
) -> Result<Vec<FeaturePayloadScalar>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut output = Vec::new();
    for construction in constructions {
        let ids = construction
            .members
            .iter()
            .map(|member| &member.data_block)
            .chain(std::iter::once(&construction.terminal_data_block));
        let Some(joined) = JoinedPayload::from_source(ctx, ids, &blocks)? else {
            continue;
        };
        for (ordinal, field) in crate::om::construction_payload_scalar_fields(ctx, joined.bytes())?
            .into_iter()
            .enumerate()
        {
            let payload_offset = cadmpeg_core::decode::u64_from_index(field.offset);
            let Some(source_offset) = joined.source_offset(payload_offset) else {
                continue;
            };
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX sketch payload scalar ordinal", 0, 1))?;
            let construction_payload = replace_operation_text(
                ctx,
                &construction.id,
                "sketch-construction-inputs",
                "sketch-construction-payload",
                "NX sketch scalar construction payload",
            )?;
            let key = construction_payload
                .rsplit_once('#')
                .map_or("unknown", |(_, key)| key);
            let id = ctx.format_retained(
                format_args!("nx:feature-history:sketch-payload-scalar#{key}-{ordinal:010}"),
                "NX sketch payload scalar identity",
            )?;
            let operation_label = ctx.copy_retained_text(
                &construction.operation_label,
                "NX sketch payload scalar operation label",
            )?;
            ctx.reserve_vec(&mut output, 1, "NX sketch payload scalars")?;
            output.push(FeaturePayloadScalar {
                id,
                operation_label,
                payload: FeatureScalarPayload::Construction {
                    construction_payload,
                },
                ordinal,
                field_code: field.field_code,
                scalar: field.scalar,
                payload_offset,
                source_offset,
            });
        }
    }
    Ok(output)
}

/// Decode exact scalar-vector frames across reconstructed sketch payloads.
pub(super) fn feature_sketch_payload_scalar_lanes(
    ctx: &DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeatureSketchPayloadScalarLane>, CodecError> {
    construction_payload_frames(
        ctx,
        container,
        payloads,
        |payload| payload.content.blocks(),
        |bytes| crate::om::sketch_payload_scalar_lanes(ctx, bytes),
        |payload, ordinal, lane, source_offset| {
            let Some(header_source) = usize::try_from(lane.offset()).ok().and_then(source_offset)
            else {
                return Ok(None);
            };
            let Some(terminator_source) = usize::try_from(lane.end()).ok().and_then(source_offset)
            else {
                return Ok(None);
            };
            let Some(lane) = lane.try_map_locations(ctx, |offset, ()| {
                usize::try_from(offset).ok().and_then(source_offset)
            })?
            else {
                return Ok(None);
            };
            let id = format_feature_child_id(ctx, &payload.id, "-scalar-lane-", ordinal)?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX sketch scalar lane ordinal", 0, 1))?;
            Ok(Some(FeatureSketchPayloadScalarLane {
                id,
                operation_label: ctx
                    .copy_retained_text(&payload.operation_label, "NX sketch scalar lane label")?,
                construction_payload: ctx
                    .copy_retained_text(&payload.id, "NX sketch scalar lane payload")?,
                ordinal,
                lane,
                source_offset: header_source,
                terminator_source_offset: terminator_source,
            }))
        },
    )
}

/// Decode exact compact-code name fields across reconstructed sketch payloads.
pub(super) fn feature_sketch_payload_names(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    constructions: &[FeatureSketchConstructionInputs],
) -> Result<Vec<FeaturePayloadName>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut names = Vec::new();
    for construction in constructions {
        let count = construction
            .members
            .len()
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("NX sketch name source blocks", 0, 1))?;

        let (mut ids, _ids_reservation) =
            ctx.temporary_vec(count, "NX sketch name source blocks")?;
        ids.extend(construction.members.iter().map(|member| &member.data_block));
        ids.push(&construction.terminal_data_block);
        let Some(joined) = JoinedPayload::from_source(ctx, ids.iter().copied(), &blocks)? else {
            continue;
        };
        let old = "sketch-construction-inputs";
        let new = "sketch-construction-payload";
        let construction_payload = if let Some(start) = construction.id.find(old) {
            let end = start
                .checked_add(old.len())
                .ok_or_else(|| ctx.refuse_codec_limit("NX sketch payload identity", 0, 1))?;
            let length = construction
                .id
                .len()
                .checked_sub(old.len())
                .and_then(|length| length.checked_add(new.len()))
                .ok_or_else(|| ctx.refuse_codec_limit("NX sketch payload identity", 0, 1))?;
            let mut id = ctx.retained_string(length, "NX sketch payload identity")?;
            id.push_str(&construction.id[..start]);
            id.push_str(new);
            id.push_str(&construction.id[end..]);
            id
        } else {
            ctx.copy_retained_text(&construction.id, "NX sketch payload identity")?
        };
        let key = construction_payload
            .rsplit_once('#')
            .map_or("unknown", |(_, key)| key);
        for (ordinal, field) in crate::om::name_field::scan(ctx, joined.bytes())?
            .into_iter()
            .enumerate()
        {
            let relative = cadmpeg_core::decode::u64_from_index(field.offset());
            let Some(source_offset) = joined.source_offset(relative) else {
                continue;
            };
            let Some(frame) = field.into_native(ctx, |offset| joined.source_offset(offset))? else {
                continue;
            };
            let ordinal_u32 = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX sketch payload name ordinal", 0, 1))?;
            let id = format_feature_history_id(ctx, "sketch-payload-name", key, ordinal, None)?;
            ctx.reserve_vec(&mut names, 1, "NX sketch payload names")?;
            names.push(FeaturePayloadName {
                id,
                operation_label: ctx.copy_retained_text(
                    &construction.operation_label,
                    "NX sketch payload name label",
                )?,
                construction_payload: ctx
                    .copy_retained_text(&construction_payload, "NX sketch payload name owner")?,
                ordinal: ordinal_u32,
                frame,
                source_offset,
            });
        }
    }
    Ok(names)
}

/// Join complete name-delimited intervals to their framed scalar fields.
fn sorted_payload_refs<'ctx, 'a, T>(
    ctx: &'ctx DecodeContext<'_>,
    source: &'a [T],
    include: impl Fn(&T) -> bool,
    key: impl Fn(&T) -> u64,
) -> Result<(Vec<&'a T>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
    let scan_work = cadmpeg_core::decode::u64_from_index(source.len())
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit("scan NX sketch payload records", 0, 1))?;
    ctx.charge_work(scan_work, "scan NX sketch payload records")?;
    let count = source.iter().filter(|record| include(record)).count();

    let (mut references, reservation) =
        ctx.temporary_vec(count, "NX sketch payload record references")?;
    references.extend(source.iter().filter(|record| include(record)));
    ctx.stable_sort_by_key(
        &mut references,
            |value| key(value),
            Ord::cmp,
        "sort NX sketch payload record references",
    )?;
    Ok((references, reservation))
}

pub(super) fn feature_sketch_payload_named_records(
    ctx: &DecodeContext<'_>,
    payloads: &[FeatureConstructionPayload],
    names: &[FeaturePayloadName],
    scalars: &[FeaturePayloadScalar],
    fixed_pairs: &[FeatureSketchPayloadFixedPair],
    mixed_pairs: &[FeatureSketchPayloadMixedPair],
) -> Result<Vec<FeatureSketchPayloadNamedRecord>, CodecError> {
    let mut records = Vec::new();
    for payload in payloads {
        let (payload_names, _names_reservation) = sorted_payload_refs(
            ctx,
            names,
            |name| name.construction_payload == payload.id,
            |name| name.frame.offset(),
        )?;
        for (ordinal, name) in payload_names.iter().enumerate() {
            let end = payload_names
                .get(ordinal + 1)
                .map_or(payload.content.byte_len(), |next| next.frame.offset());
            let (scalar_fields, _scalars_reservation) = sorted_payload_refs(
                ctx,
                scalars,
                |scalar| {
                    scalar.payload.id() == payload.id
                        && scalar.payload_offset > name.frame.offset()
                        && scalar.payload_offset < end
                },
                |scalar| scalar.payload_offset,
            )?;
            let (record_fixed_pairs, _fixed_reservation) = sorted_payload_refs(
                ctx,
                fixed_pairs,
                |pair| {
                    pair.construction_payload == payload.id
                        && pair.position.offset() > name.frame.offset()
                        && pair.position.offset() < end
                },
                |pair| pair.position.offset(),
            )?;
            let (record_mixed_pairs, _mixed_reservation) = sorted_payload_refs(
                ctx,
                mixed_pairs,
                |pair| {
                    pair.construction_payload == payload.id
                        && pair.position.offset() > name.frame.offset()
                        && pair.position.offset() < end
                },
                |pair| pair.position.offset(),
            )?;
            let key = payload
                .id
                .rsplit_once('#')
                .map_or("unknown", |(_, key)| key);
            let id = format_feature_history_id(ctx, "sketch-payload-record", key, ordinal, None)?;
            let scalar_fields = ctx.collect_retained_texts(
                scalar_fields.iter().map(|scalar| scalar.id.as_str()),
                "NX sketch payload record IDs",
            )?;
            let fixed_pairs = ctx.collect_retained_texts(
                record_fixed_pairs.iter().map(|pair| pair.id.as_str()),
                "NX sketch payload record IDs",
            )?;
            let mixed_pairs = ctx.collect_retained_texts(
                record_mixed_pairs.iter().map(|pair| pair.id.as_str()),
                "NX sketch payload record IDs",
            )?;
            ctx.reserve_vec(&mut records, 1, "NX sketch payload named records")?;
            records.push(FeatureSketchPayloadNamedRecord {
                id,
                operation_label: ctx.copy_retained_text(
                    &payload.operation_label,
                    "NX sketch payload record label",
                )?,
                construction_payload: ctx
                    .copy_retained_text(&payload.id, "NX sketch payload record owner")?,
                name_field: ctx.copy_retained_text(&name.id, "NX sketch payload record name")?,
                scalar_fields,
                fixed_pairs,
                mixed_pairs,
                payload_start_offset: name.frame.offset(),
                payload_end_offset: end,
            });
        }
    }
    Ok(records)
}

/// Decode complete `Point<decimal>` records with exactly two scalar fields.
pub(super) fn feature_sketch_points(
    ctx: &DecodeContext<'_>,
    records: &[FeatureSketchPayloadNamedRecord],
    names: &[FeaturePayloadName],
    scalars: &[FeaturePayloadScalar],
) -> Result<Vec<FeatureSketchPoint>, CodecError> {
    let mut points = Vec::new();
    for record in records {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(names.len()),
            "resolve NX sketch point name",
        )?;
        let Some(name) = names.iter().rev().find(|name| name.id == record.name_field) else {
            continue;
        };
        if name.operation_label != record.operation_label
            || name.construction_payload != record.construction_payload
        {
            continue;
        }
        if parse_sketch_point_name(name.frame.value()).is_none() {
            continue;
        }
        let [first_id, second_id] = record.scalar_fields.as_slice() else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(scalars.len())
                .checked_mul(2)
                .ok_or_else(|| ctx.refuse_codec_limit("resolve NX sketch point scalars", 0, 1))?,
            "resolve NX sketch point scalars",
        )?;
        let Some(first) = scalars.iter().rev().find(|scalar| scalar.id == *first_id) else {
            continue;
        };
        let Some(second) = scalars.iter().rev().find(|scalar| scalar.id == *second_id) else {
            continue;
        };
        if [first, second].into_iter().any(|scalar| {
            scalar.operation_label != record.operation_label
                || scalar.payload.id() != record.construction_payload
        }) {
            continue;
        }
        let key = record.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let id = ctx.format_retained(
            format_args!("nx:feature-history:sketch-point#{key}"),
            "NX sketch point identity",
        )?;
        let operation_label =
            ctx.copy_retained_text(&record.operation_label, "NX sketch point operation label")?;
        let named_record = ctx.copy_retained_text(&record.id, "NX sketch point named record")?;
        let name = ctx.copy_retained_text(name.frame.value(), "NX sketch point name")?;
        let first_id = ctx.copy_retained_text(&first.id, "NX sketch point first scalar")?;
        let second_id = ctx.copy_retained_text(&second.id, "NX sketch point second scalar")?;
        ctx.reserve_vec(&mut points, 1, "NX sketch points")?;
        points.push(FeatureSketchPoint {
            id,
            operation_label,
            named_record,
            name,
            scalar_fields: [first_id, second_id],
            coordinates: [first.scalar.value(), second.scalar.value()].into(),
        });
    }
    Ok(points)
}

/// Decode `Point<positive decimal>` records containing exactly one fixed pair.
pub(super) fn feature_sketch_fixed_points(
    ctx: &DecodeContext<'_>,
    records: &[FeatureSketchPayloadNamedRecord],
    names: &[FeaturePayloadName],
    fixed_pairs: &[FeatureSketchPayloadFixedPair],
) -> Result<Vec<FeatureSketchFixedPoint>, CodecError> {
    let mut points = Vec::new();
    for record in records {
        if !record.scalar_fields.is_empty() || !record.mixed_pairs.is_empty() {
            continue;
        }
        let mut point_pair = None;
        for fixed_pair_id in &record.fixed_pairs {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(fixed_pairs.len()),
                "resolve NX sketch fixed pair",
            )?;
            let Some(pair) = fixed_pairs
                .iter()
                .rev()
                .find(|pair| pair.id == *fixed_pair_id)
            else {
                continue;
            };
            if !matches!(
                pair.position.form(),
                SketchPairForm::Legacy | SketchPairForm::Short | SketchPairForm::Extended
            ) {
                continue;
            }
            if point_pair.replace(pair).is_some() {
                point_pair = None;
                break;
            }
        }
        let Some(pair) = point_pair else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(names.len()),
            "resolve NX sketch fixed point name",
        )?;
        let Some(name) = names.iter().rev().find(|name| name.id == record.name_field) else {
            continue;
        };
        if name.operation_label != record.operation_label
            || name.construction_payload != record.construction_payload
        {
            continue;
        }
        if parse_sketch_point_name(name.frame.value()).is_none() {
            continue;
        }
        if pair.operation_label != record.operation_label
            || pair.construction_payload != record.construction_payload
        {
            continue;
        }
        let [Some(first), Some(second)] = pair
            .values
            .map(SketchScaledAtom::value)
            .map(cadmpeg_ir::scalar::FiniteReal::new)
        else {
            continue;
        };
        let id = replace_operation_text(
            ctx,
            &record.id,
            "sketch-payload-record",
            "sketch-fixed-point",
            "NX sketch fixed point identity",
        )?;
        let operation_label = ctx.copy_retained_text(
            &record.operation_label,
            "NX sketch fixed point operation label",
        )?;
        let named_record =
            ctx.copy_retained_text(&record.id, "NX sketch fixed point named record")?;
        let name = ctx.copy_retained_text(name.frame.value(), "NX sketch fixed point name")?;
        let fixed_pair = ctx.copy_retained_text(&pair.id, "NX sketch fixed point pair")?;
        ctx.reserve_vec(&mut points, 1, "NX sketch fixed points")?;
        points.push(FeatureSketchFixedPoint {
            id,
            operation_label,
            named_record,
            name,
            fixed_pair,
            values: [first, second],
            source_offset: pair.source_offset,
        });
    }
    Ok(points)
}

/// Group every bit-identical same-name sketch-point witness.
pub(super) fn feature_sketch_point_groups(
    ctx: &DecodeContext<'_>,
    points: &[FeatureSketchPoint],
) -> Result<Vec<FeatureSketchPointGroup>, CodecError> {
    let work = cadmpeg_core::decode::u64_from_index(points.len())
        .checked_mul(cadmpeg_core::decode::u64_from_index(points.len()))
        .and_then(|work| work.checked_mul(4))
        .ok_or_else(|| ctx.refuse_codec_limit("group NX sketch points", 0, 1))?;
    ctx.charge_work(work, "group NX sketch points")?;
    let mut grouped = BTreeSet::new();
    let mut grouped_reservation = ctx.reserve_scoped(0, "index NX sketch point groups")?;
    let mut groups = Vec::new();
    for point in points {
        let key = (point.operation_label.as_str(), point.name.as_str());
        if grouped.contains(&key) {
            continue;
        }

        grouped_reservation.with_storage(|| {
            ctx.insert_btree_set(&mut grouped, key, "NX sketch point group keys")
        })?;
        let matches = |candidate: &&FeatureSketchPoint| {
            candidate.operation_label == point.operation_label && candidate.name == point.name
        };
        let count = points.iter().filter(&matches).count();
        if points.iter().filter(&matches).any(|candidate| {
            candidate
                .coordinates
                .iter()
                .zip(point.coordinates)
                .any(|(first, second)| first.to_bits() != second.to_bits())
        }) {
            continue;
        }

        let mut members = ctx.collection_vec(count, "NX sketch point group members")?;
        for witness in points.iter().filter(&matches) {
            members.push(ctx.copy_retained_text(&witness.id, "NX sketch point group member")?);
        }
        let prefix = "nx:feature-history:sketch-point-group#";
        let key = point.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let length = prefix
            .len()
            .checked_add(key.len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX sketch point group identity", 0, 1))?;
        let mut id = ctx.retained_string(length, "NX sketch point group identity")?;
        id.push_str(prefix);
        id.push_str(key);
        ctx.reserve_vec(&mut groups, 1, "NX sketch point groups")?;
        groups.push(FeatureSketchPointGroup {
            id,
            operation_label: ctx
                .copy_retained_text(&point.operation_label, "NX sketch point group label")?,
            name: ctx.copy_retained_text(&point.name, "NX sketch point group name")?,
            points: members,
            coordinates: point.coordinates,
        });
    }
    Ok(groups)
}

/// Decode exact named point objects across consecutive offset-store blocks.
pub(super) fn offset_store_named_points(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<OffsetStoreNamedPoint>, cadmpeg_core::CodecError> {
    let mut points = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
    {
        let Some((_, _, records)) = section.as_offset_only() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for ordinal in 0..records.len() {
            let Some(point) = crate::om::offset_store_named_point(
                ctx,
                records[ordinal..].iter().map(|record| record.bytes),
            )?
            else {
                continue;
            };
            let records = &records[ordinal..ordinal + point.block_count];
            let first_source =
                entry_offset + cadmpeg_core::decode::u64_from_index(records[0].offset);
            let value_source_offset = |payload_offset: usize| {
                let mut relative = payload_offset;
                for record in records {
                    if relative < record.bytes.len() {
                        return Some(
                            entry_offset
                                + cadmpeg_core::decode::u64_from_index(record.offset)
                                + cadmpeg_core::decode::u64_from_index(relative),
                        );
                    }
                    relative -= record.bytes.len();
                }
                None
            };
            let [Some(first), Some(second)] = point.values.map(|value| {
                value_source_offset(value.offset).map(|source_offset| FeatureBinary64ScalarToken {
                    scalar: value.scalar,
                    source_offset,
                })
            }) else {
                continue;
            };
            let point_ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit("NX named point ordinal", 0, 1))?;
            let id = ctx.format_retained(
                format_args!("nx:offset-store:named-point#{section_ordinal}-{point_ordinal}"),
                "NX named point identity",
            )?;
            let mut data_blocks = Vec::new();
            for relative in 0..point.block_count {
                let block_ordinal = ordinal
                    .checked_add(relative)
                    .and_then(|ordinal| ordinal.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit("NX named point block ordinal", 0, 1))?;
                let block_id = ctx.format_retained(
                    format_args!("nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"),
                    "NX named point data block identity",
                )?;
                ctx.reserve_vec(&mut data_blocks, 1, "NX named point data blocks")?;
                data_blocks.push(block_id);
            }
            ctx.reserve_vec(&mut points, 1, "NX named points")?;
            points.push(OffsetStoreNamedPoint {
                id,
                name: point.name,
                data_blocks,
                values: [first, second],
                source_offset: first_source,
            });
        }
    }
    Ok(points)
}

/// Join sketch references to named points through exact shared block identity.
pub(super) fn feature_sketch_named_point_block_uses(
    ctx: &DecodeContext<'_>,
    references: &[FeatureSketchReference],
    points: &[OffsetStoreNamedPoint],
) -> Result<Vec<FeatureSketchNamedPointBlockUse>, CodecError> {
    let mut uses = Vec::new();
    for reference in references {
        let Some(data_block) = reference.data_block.as_deref() else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(points.len()),
            "join NX sketch named points",
        )?;
        for point in points {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(point.data_blocks.len()),
                "match NX named point block",
            )?;
            let Some(point_block_ordinal) = point
                .data_blocks
                .iter()
                .position(|block| block == data_block)
            else {
                continue;
            };
            let operation_key = reference
                .operation_label
                .rsplit_once('#')
                .map_or(reference.operation_label.as_str(), |(_, key)| key);
            let point_key = point
                .id
                .rsplit_once('#')
                .map_or(point.id.as_str(), |(_, key)| key);
            let id = ctx.format_retained(format_args!(
                "nx:feature-history:sketch-named-point-block-use#{operation_key}-{}-{point_key}-{point_block_ordinal}",
                reference.position.ordinal()), "NX sketch named point block use identity")?;
            let operation_label = ctx.copy_retained_text(
                &reference.operation_label,
                "NX sketch named point block use label",
            )?;
            let sketch_reference =
                ctx.copy_retained_text(&reference.id, "NX sketch named point block use reference")?;
            let named_point =
                ctx.copy_retained_text(&point.id, "NX sketch named point block use point")?;
            let data_block =
                ctx.copy_retained_text(data_block, "NX sketch named point block use data block")?;
            let point_block_ordinal = u32::try_from(point_block_ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX named point block ordinal", 0, 1))?;
            ctx.reserve_vec(&mut uses, 1, "NX sketch named point block uses")?;
            uses.push(FeatureSketchNamedPointBlockUse {
                id,
                operation_label,
                sketch_reference,
                reference_ordinal: reference.position.ordinal(),
                named_point,
                data_block,
                point_block_ordinal,
                source_offset: reference.source_offset,
            });
        }
    }
    Ok(uses)
}

/// Split a data-block id into its offset-store id and block ordinal.
fn block_key(block: &str) -> Option<(&str, u32)> {
    let (store, ordinal) = block.rsplit_once(":block#")?;
    Some((store, ordinal.parse().ok()?))
}

/// Join one named point to a complete sketch lane through unique consecutive block adjacency.
pub(super) fn feature_sketch_preceding_named_point_uses(
    ctx: &DecodeContext<'_>,
    references: &[FeatureSketchReference],
    points: &[OffsetStoreNamedPoint],
) -> Result<Vec<FeatureSketchPrecedingNamedPointUse>, CodecError> {
    use std::collections::btree_map::Entry;

    let mut references_by_operation = BTreeMap::<&str, Vec<&FeatureSketchReference>>::new();
    let mut index_reservation =
        ctx.reserve_scoped(0, "index NX preceding named-point references")?;
    for reference in references {
        ctx.charge_work(
            u64::from(usize::BITS - references_by_operation.len().leading_zeros()),
            "index NX preceding named-point references",
        )?;
        let operation_key = reference.operation_label.as_str();
        index_reservation.with_storage(|| {
            ctx.admit_btree_entry(
                &references_by_operation,
                &operation_key,
                "NX preceding named-point operation index",
            )
        })?;
        let group = match references_by_operation.entry(operation_key) {
            Entry::Vacant(entry) => entry.insert(Vec::new()),
            Entry::Occupied(entry) => entry.into_mut(),
        };

        index_reservation
            .with_storage(|| ctx.reserve_vec(group, 1, "NX preceding named-point references"))?;
        group.push(reference);
    }
    let mut uses = Vec::new();
    for (operation_label, operation_references) in &mut references_by_operation {
        ctx.stable_sort_by_key(
            operation_references,
            |value| value.position.ordinal(),
            Ord::cmp,
            "sort NX preceding named-point references",
        )?;
        let Some((first_reference, first_block)) = operation_references
            .first()
            .and_then(|reference| Some((*reference, reference.data_block.as_deref()?)))
        else {
            continue;
        };
        let mut complete_lane = true;
        for (ordinal, reference) in operation_references.iter().enumerate() {
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                ctx.refuse_codec_limit("NX preceding named-point reference ordinal", 0, 1)
            })?;
            if reference.position.ordinal() != ordinal
                || !matches!(reference.position.declared_count(),
                    crate::om::sketch_references::SketchReferenceCount::Declared(count)
                        if usize::from(count.get()) == operation_references.len())
                || reference.data_block.is_none()
            {
                complete_lane = false;
                break;
            }
        }
        if !complete_lane {
            continue;
        }
        let Some((first_store, first_ordinal)) = block_key(first_block) else {
            continue;
        };
        let mut candidate = None;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(points.len()),
            "match NX preceding named point",
        )?;
        for point in points {
            let Some(last_block) = point.data_blocks.last() else {
                continue;
            };
            let Some((point_store, point_ordinal)) = block_key(last_block) else {
                continue;
            };
            if point_store == first_store
                && point_ordinal.checked_add(1) == Some(first_ordinal)
                && candidate.replace(point).is_some()
            {
                candidate = None;
                break;
            }
        }
        let Some(point) = candidate else {
            continue;
        };
        let operation_key = (*operation_label)
            .rsplit_once('#')
            .map_or(*operation_label, |(_, key)| key);
        let point_key = point
            .id
            .rsplit_once('#')
            .map_or(point.id.as_str(), |(_, key)| key);
        let id = ctx.format_retained(
            format_args!(
                "nx:feature-history:sketch-preceding-named-point-use#{operation_key}-{point_key}"
            ),
            "NX preceding named-point use identity",
        )?;
        let operation_label =
            ctx.copy_retained_text(operation_label, "NX preceding named-point operation label")?;
        let first_sketch_reference = ctx.copy_retained_text(
            &first_reference.id,
            "NX preceding named-point first reference",
        )?;
        let named_point = ctx.copy_retained_text(&point.id, "NX preceding named-point identity")?;
        let following_data_block =
            ctx.copy_retained_text(first_block, "NX preceding named-point following block")?;
        let mut point_data_blocks = Vec::new();
        for block in &point.data_blocks {
            let block = ctx.copy_retained_text(block, "NX preceding named-point source block")?;
            ctx.reserve_vec(
                &mut point_data_blocks,
                1,
                "NX preceding named-point source blocks",
            )?;
            point_data_blocks.push(block);
        }
        ctx.reserve_vec(&mut uses, 1, "NX preceding named-point uses")?;
        uses.push(FeatureSketchPrecedingNamedPointUse {
            id,
            operation_label,
            first_sketch_reference,
            named_point,
            point_data_blocks,
            following_data_block,
            source_offset: first_reference.source_offset,
        });
    }
    drop(references_by_operation);
    drop(index_reservation);
    Ok(uses)
}

/// Join the two exact encodings of a solved sketch point.
pub(super) fn feature_sketch_point_uses(
    ctx: &DecodeContext<'_>,
    point_groups: &[FeatureSketchPointGroup],
    named_points: &[OffsetStoreNamedPoint],
    block_uses: &[FeatureSketchNamedPointBlockUse],
) -> Result<Vec<FeatureSketchPointUse>, CodecError> {
    let mut uses = Vec::new();
    for (block_use_index, block_use) in block_uses.iter().enumerate() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(block_use_index),
            "deduplicate NX sketch point uses",
        )?;
        if block_uses[..block_use_index].iter().any(|earlier| {
            earlier.operation_label == block_use.operation_label
                && earlier.named_point == block_use.named_point
        }) {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(named_points.len()),
            "resolve NX sketch named point",
        )?;
        let Some(named_point) = named_points
            .iter()
            .rev()
            .find(|point| point.id == block_use.named_point)
        else {
            continue;
        };
        let mut point_block_uses = Vec::new();
        let mut order_reservation = ctx.reserve_scoped(0, "sort NX sketch point block uses")?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(block_uses.len()),
            "collect NX sketch point block uses",
        )?;
        for candidate in block_uses.iter().filter(|candidate| {
            candidate.operation_label == block_use.operation_label
                && candidate.named_point == block_use.named_point
        }) {
            order_reservation.with_storage(|| {
                ctx.reserve_vec(&mut point_block_uses, 1, "NX sketch point block use order")
            })?;
            point_block_uses.push(candidate);
        }
        ctx.stable_sort_by(
            &mut point_block_uses,
            |value| value,
            |left, right| {
                (left.reference_ordinal, left.source_offset, left.id.as_str()).cmp(&(
                    right.reference_ordinal,
                    right.source_offset,
                    right.id.as_str(),
                ))
            },
            "sort NX sketch point block uses",
        )?;
        let mut point_group = None;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(point_groups.len()),
            "resolve NX sketch point group",
        )?;
        for group in point_groups.iter().filter(|group| {
            group.operation_label == block_use.operation_label && group.name == named_point.name
        }) {
            if point_group.replace(group).is_some() {
                point_group = None;
                break;
            }
        }
        let Some(point_group) = point_group else {
            continue;
        };
        if point_group
            .coordinates
            .iter()
            .zip(named_point.values.map(|token| token.scalar.value()))
            .any(|(first, second)| first.to_bits() != second.get().to_bits())
        {
            continue;
        }
        let Some(first) = point_block_uses.first() else {
            continue;
        };
        let id = replace_operation_text(
            ctx,
            &first.id,
            "sketch-named-point-block-use",
            "sketch-point-use",
            "NX sketch point use identity",
        )?;
        let operation_label = ctx.copy_retained_text(
            &block_use.operation_label,
            "NX sketch point use operation label",
        )?;
        let mut references = Vec::new();
        for block_use in &point_block_uses {
            let sketch_reference = ctx
                .copy_retained_text(&block_use.sketch_reference, "NX sketch point use reference")?;
            let block_use_id =
                ctx.copy_retained_text(&block_use.id, "NX sketch point use block identity")?;
            ctx.reserve_vec(&mut references, 1, "NX sketch point use references")?;
            references.push(FeatureSketchPointUseReference {
                sketch_reference,
                block_use: block_use_id,
                source_offset: block_use.source_offset,
            });
        }
        drop(point_block_uses);
        drop(order_reservation);
        let sketch_point_group =
            ctx.copy_retained_text(&point_group.id, "NX sketch point use group identity")?;
        let named_point =
            ctx.copy_retained_text(&named_point.id, "NX sketch point use named point")?;
        ctx.reserve_vec(&mut uses, 1, "NX sketch point uses")?;
        uses.push(FeatureSketchPointUse {
            id,
            operation_label,
            references,
            sketch_point_group,
            named_point,
        });
    }
    Ok(uses)
}

/// Join one uniquely sketch-owned named-point block to a later datum-CSYS construction.
pub(super) fn feature_sketch_datum_csys_dependencies(
    ctx: &DecodeContext<'_>,
    labels: &[FeatureOperationLabel],
    named_points: &[OffsetStoreNamedPoint],
    point_uses: &[FeatureSketchPointUse],
    constructions: &[FeatureDatumCsysConstruction],
    scalars: &[FeaturePayloadScalar],
) -> Result<Vec<FeatureSketchDatumCsysDependency>, CodecError> {
    #[derive(PartialEq, Eq)]
    enum BorrowedRelation<'a> {
        Shared(&'a str),
        Consecutive(&'a str, &'a str),
    }
    let (ordered_labels, _ordered_labels_storage) = ctx
        .with_scoped_storage("NX local ordered_labels storage", || {
            feature_operation_chronological_labels(ctx, labels)
        })?;
    let mut positions = BTreeMap::new();
    let mut position_reservation = ctx.reserve_scoped(0, "NX sketch datum label positions")?;
    for (position, label) in ordered_labels.iter().enumerate() {
        position_reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut positions,
                label.id.as_str(),
                position,
                "NX sketch datum label positions",
            )
        })?;
    }
    let mut points = BTreeMap::new();
    let mut point_reservation = ctx.reserve_scoped(0, "NX sketch datum named points")?;
    for point in named_points {
        point_reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut points,
                point.id.as_str(),
                point,
                "NX sketch datum named points",
            )
        })?;
    }
    let mut dependencies = Vec::new();
    for construction in constructions {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(point_uses.len()),
            "match NX sketch datum dependencies",
        )?;
        let Some(consumer_position) = positions.get(construction.operation_label.as_str()) else {
            continue;
        };
        let mut candidate: Option<(usize, BorrowedRelation<'_>)> = None;
        let mut ambiguous = false;
        'point_uses: for (point_use_index, point_use) in point_uses.iter().enumerate() {
            let Some(producer_position) = positions.get(point_use.operation_label.as_str()) else {
                continue;
            };
            if producer_position >= consumer_position {
                continue;
            }
            let Some(point) = points.get(point_use.named_point.as_str()) else {
                continue;
            };
            let comparisons = point
                .data_blocks
                .len()
                .checked_mul(construction.frame.members().len())
                .ok_or_else(|| ctx.refuse_codec_limit("match NX sketch datum blocks", 0, 1))?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(comparisons),
                "match NX sketch datum blocks",
            )?;
            for shared_block in construction
                .frame
                .members()
                .iter()
                .map(|(_, binding)| binding)
                .filter(|block| point.data_blocks.contains(block))
            {
                let relation = BorrowedRelation::Shared(shared_block);
                let is_new_ambiguity =
                    candidate
                        .as_ref()
                        .is_some_and(|(existing_index, existing_relation)| {
                            point_uses[*existing_index].id != point_use.id
                                || *existing_relation != relation
                        });
                if is_new_ambiguity {
                    ambiguous = true;
                    break 'point_uses;
                }
                candidate.get_or_insert((point_use_index, relation));
            }
            let Some(point_last_block) = point.data_blocks.last() else {
                continue;
            };
            let construction_first_block = &construction.frame.members()[0].1;
            if let (
                Some((point_store, point_ordinal)),
                Some((construction_store, construction_ordinal)),
            ) = (
                block_key(point_last_block),
                block_key(construction_first_block),
            ) {
                if point_store == construction_store
                    && point_ordinal.checked_add(1) == Some(construction_ordinal)
                {
                    let relation =
                        BorrowedRelation::Consecutive(point_last_block, construction_first_block);
                    let is_new_ambiguity =
                        candidate
                            .as_ref()
                            .is_some_and(|(existing_index, existing_relation)| {
                                point_uses[*existing_index].id != point_use.id
                                    || *existing_relation != relation
                            });
                    if is_new_ambiguity {
                        ambiguous = true;
                    } else {
                        candidate.get_or_insert((point_use_index, relation));
                    }
                }
            }
            if ambiguous {
                break;
            }
        }
        if ambiguous {
            continue;
        }
        let Some((point_use_index, block_relation)) = candidate else {
            continue;
        };
        let point_use = &point_uses[point_use_index];
        let Some(reference) = point_use.references.first() else {
            continue;
        };
        let point = points[point_use.named_point.as_str()];
        let mut scalar_aliases = Vec::new();
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(scalars.len())
                .checked_mul(cadmpeg_core::decode::u64_from_index(point.values.len()))
                .ok_or_else(|| ctx.refuse_codec_limit("match NX sketch datum scalars", 0, 1))?,
            "match NX sketch datum scalars",
        )?;
        for (coordinate_ordinal, value) in point.values.iter().enumerate() {
            for scalar in scalars.iter().filter(|scalar| {
                scalar.operation_label == construction.operation_label
                    && scalar.source_offset == value.source_offset
            }) {
                let datum_csys_scalar =
                    ctx.copy_retained_text(&scalar.id, "NX sketch datum scalar identity")?;
                ctx.reserve_vec(&mut scalar_aliases, 1, "NX sketch datum scalar aliases")?;
                scalar_aliases.push(FeatureSketchDatumCsysScalarAlias {
                    sketch_coordinate_ordinal: u8::try_from(coordinate_ordinal).map_err(|_| {
                        ctx.refuse_codec_limit("NX sketch datum coordinate ordinal", 0, 1)
                    })?,
                    datum_csys_scalar,
                    value_source_offset: value.source_offset,
                });
            }
        }
        let block_relation = match block_relation {
            BorrowedRelation::Shared(data_block) => FeatureSketchDatumCsysBlockRelation::Shared {
                data_block: ctx.copy_retained_text(data_block, "NX sketch datum shared block")?,
            },
            BorrowedRelation::Consecutive(point_data_block, construction_data_block) => {
                FeatureSketchDatumCsysBlockRelation::Consecutive {
                    point_data_block: ctx
                        .copy_retained_text(point_data_block, "NX sketch datum point block")?,
                    construction_data_block: ctx.copy_retained_text(
                        construction_data_block,
                        "NX sketch datum construction block",
                    )?,
                }
            }
        };
        let id = replace_operation_text(
            ctx,
            &construction.id,
            "datum-csys-construction",
            "sketch-datum-csys-dependency",
            "NX sketch datum dependency identity",
        )?;
        let sketch_operation_label =
            ctx.copy_retained_text(&point_use.operation_label, "NX sketch datum producer label")?;
        let datum_csys_operation_label = ctx.copy_retained_text(
            &construction.operation_label,
            "NX sketch datum consumer label",
        )?;
        let sketch_point_use =
            ctx.copy_retained_text(&point_use.id, "NX sketch datum point use")?;
        let datum_csys_construction =
            ctx.copy_retained_text(&construction.id, "NX sketch datum construction")?;
        ctx.reserve_vec(&mut dependencies, 1, "NX sketch datum dependencies")?;
        dependencies.push(FeatureSketchDatumCsysDependency {
            id,
            sketch_operation_label,
            datum_csys_operation_label,
            sketch_point_use,
            datum_csys_construction,
            block_relation,
            scalar_aliases,
            source_offset: reference.source_offset,
        });
    }
    ctx.stable_sort_by(
        &mut dependencies,
            |value| &value.id,
            Ord::cmp,
        "sort NX sketch datum dependencies",
    )?;
    Ok(dependencies)
}

fn parse_sketch_point_name(value: &str) -> Option<u32> {
    let suffix = value.strip_prefix("Point")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let ordinal = suffix.parse::<u32>().ok()?;
    (ordinal != 0).then_some(ordinal)
}

/// Decode and resolve the ordered counted-reference field in sketch payloads.
pub(super) fn feature_sketch_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureSketchReference>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut references = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let decoded = match crate::om::sketch_payload_references(ctx, record.payload_view()) {
                Ok(Some(decoded)) => decoded,
                Ok(None) => return,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let projected = (|| -> Result<(), CodecError> {
                for (position, reference) in decoded.into_positioned() {
                    let data_block =
                        charged_unique_offset_data_block(ctx, &indexed, reference.token.value())?;
                    let ordinal = usize::try_from(position.ordinal())
                        .map_err(|_| ctx.refuse_codec_limit("NX sketch reference ordinal", 0, 1))?;
                    let id = format_feature_history_id(
                        ctx,
                        "sketch-reference",
                        section_key,
                        operation_ordinal,
                        Some(ordinal),
                    )?;
                    let operation_label = format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?;
                    let source_offset = entry_offset
                        .checked_add(cadmpeg_core::decode::u64_from_index(reference.offset))
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit("NX sketch reference source offset", 0, 1)
                        })?;
                    ctx.reserve_vec(&mut references, 1, "NX sketch references")?;
                    references.push(FeatureSketchReference {
                        id,
                        operation_label,
                        position,
                        token: reference.token,
                        data_block,
                        source_offset,
                    });
                }
                Ok(())
            })();
            if let Err(error) = projected {
                failure = Some(error);
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(references)
}

/// Join operation input lanes to uniquely resolved parameter declarations.
pub(super) fn feature_parameter_bindings(
    ctx: &DecodeContext<'_>,
    inputs: &[FeatureInputBlock],
    references: &[DataBlockReference],
    expressions: &[ParameterFormula],
) -> Result<Vec<FeatureParameterBinding>, CodecError> {
    let mut bindings = Vec::new();
    for input in inputs {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(references.len()),
            "scan NX parameter binding references",
        )?;
        for reference in references
            .iter()
            .filter(|reference| reference.data_block == input.data_block)
        {
            let Some(expression_declaration) = &reference.target_expression_declaration else {
                continue;
            };
            let operation_key = input
                .operation_label
                .rsplit_once('#')
                .map_or(input.operation_label.as_str(), |(_, key)| key);
            let id = ctx.format_retained(
                format_args!(
                    "nx:feature-history:parameter-binding#{operation_key}-{}-{}",
                    input.input_slot, reference.ordinal
                ),
                "NX parameter binding identity",
            )?;
            let operation_label =
                ctx.copy_retained_text(&input.operation_label, "NX parameter binding operation")?;
            let input_block =
                ctx.copy_retained_text(&input.data_block, "NX parameter binding input block")?;
            let declaration =
                ctx.copy_retained_text(expression_declaration, "NX parameter binding declaration")?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(expressions.len()),
                "scan NX parameter binding expressions",
            )?;
            let mut matches = expressions.iter().filter(|expression| {
                expression.declaration.as_deref() == Some(expression_declaration.as_str())
            });
            let expression = match (matches.next(), matches.next()) {
                (Some(expression), None) => {
                    Some(ctx.copy_retained_text(&expression.id, "NX parameter binding expression")?)
                }
                _ => None,
            };
            ctx.reserve_vec(&mut bindings, 1, "NX parameter bindings")?;
            bindings.push(FeatureParameterBinding {
                id,
                operation_label,
                input_slot: input.input_slot,
                input_block,
                reference_ordinal: reference.ordinal,
                expression_declaration: declaration,
                expression,
                object_id: reference.object.value(),
                source_offset: reference.source_offset,
            });
        }
    }
    Ok(bindings)
}

/// Group exact expression bindings by consuming operation and expression.
pub(super) fn feature_parameter_uses(
    ctx: &DecodeContext<'_>,
    bindings: &[FeatureParameterBinding],
) -> Result<Vec<FeatureParameterUse>, CodecError> {
    let mut uses = Vec::new();
    let scan_work = bindings
        .len()
        .checked_mul(bindings.len())
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| ctx.refuse_codec_limit("group NX parameter uses", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(scan_work),
        "group NX parameter uses",
    )?;
    for (ordinal, binding) in bindings.iter().enumerate() {
        let Some(expression) = binding.expression.as_deref() else {
            continue;
        };
        if bindings[..ordinal].iter().any(|candidate| {
            candidate.operation_label == binding.operation_label
                && candidate.expression.as_deref() == Some(expression)
        }) {
            continue;
        }
        let operation_key = binding
            .operation_label
            .rsplit_once('#')
            .map_or(binding.operation_label.as_str(), |(_, key)| key);
        let expression_key = expression
            .rsplit_once('#')
            .map_or(expression, |(_, key)| key);
        let id = ctx.format_retained(
            format_args!("nx:feature-history:parameter-use#{operation_key}-{expression_key}"),
            "NX parameter use identity",
        )?;
        let operation_label =
            ctx.copy_retained_text(&binding.operation_label, "NX parameter use operation")?;
        let expression_id = ctx.copy_retained_text(expression, "NX parameter use expression")?;
        let mut occurrences = Vec::new();
        for candidate in bindings.iter().filter(|candidate| {
            candidate.operation_label == binding.operation_label
                && candidate.expression.as_deref() == Some(expression)
        }) {
            let binding_id = ctx.copy_retained_text(&candidate.id, "NX parameter use binding")?;
            ctx.reserve_vec(&mut occurrences, 1, "NX parameter use bindings")?;
            occurrences.push(FeatureParameterUseBinding {
                binding: binding_id,
                source_offset: candidate.source_offset,
            });
        }
        ctx.stable_sort_by(
            &mut occurrences,
            |value| &value.source_offset,
            Ord::cmp,
            "sort NX parameter use bindings",
        )?;
        ctx.reserve_vec(&mut uses, 1, "NX parameter uses")?;
        uses.push(FeatureParameterUse {
            id,
            operation_label,
            expression: expression_id,
            bindings: occurrences,
        });
    }
    ctx.stable_sort_by(
        &mut uses,
            |value| value,
            |left, right| {
            left.bindings[0]
                .source_offset
                .cmp(&right.bindings[0].source_offset)
                .then_with(|| left.operation_label.cmp(&right.operation_label))
                .then_with(|| left.expression.cmp(&right.expression))
        },
        "sort NX parameter uses",
    )?;
    Ok(uses)
}

fn visit_feature_history_sections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    mut visit: impl FnMut(&crate::om::Section<'_>, &str, u64) -> Result<(), cadmpeg_core::CodecError>,
) -> Result<(), cadmpeg_core::CodecError> {
    let sections = container.om_sections(ctx)?;
    for (section_ordinal, link) in feature_history_sections(ctx, container)?
        .into_iter()
        .enumerate()
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry.file_span().map_or(
                cadmpeg_core::decode::u64_from_index(section.offset),
                |(offset, _)| offset + cadmpeg_core::decode::u64_from_index(section.offset),
            ) == link.location.section_offset()
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        visit(section, &section_key, entry_offset)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

use crate::om::pattern_references::PatternPayloadReferenceLayout;

fn deserialize_reference_lane_count<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<usize, D::Error> {
    u8::deserialize(deserializer).map(usize::from)
}

#[cfg(test)]
mod test_support;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_stable_identity, String, "stable_identity");
cadmpeg_core::named_optional_field!(deserialize_tag, u8, "tag");
cadmpeg_core::named_optional_field!(deserialize_data_block, String, "data_block");
cadmpeg_core::named_optional_field!(deserialize_ordinal, u32, "ordinal");
cadmpeg_core::named_optional_field!(deserialize_column_table, String, "column_table");
cadmpeg_core::named_optional_field!(deserialize_leading_index, u32, "leading_index");
cadmpeg_core::named_optional_field!(
    deserialize_leading_index_source_offset,
    u64,
    "leading_index_source_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_discriminator,
    crate::om::discriminators::LinkedIndexDiscriminator,
    "discriminator"
);
cadmpeg_core::named_optional_field!(
    deserialize_flag,
    crate::om::discriminators::LinkedIndexFlag,
    "flag"
);
cadmpeg_core::named_optional_field!(deserialize_expression, String, "expression");
cadmpeg_core::named_optional_field!(deserialize_index_lane_offset, u64, "index_lane_offset");
cadmpeg_core::named_optional_field!(
    deserialize_index_lane_declared_count,
    usize,
    "index_lane_declared_count"
);
cadmpeg_core::named_optional_field!(deserialize_index_lane_trailer, u32, "index_lane_trailer");
cadmpeg_core::named_optional_field!(
    deserialize_witness_source_offset,
    u64,
    "witness_source_offset"
);
cadmpeg_core::named_optional_field!(deserialize_operand_data_block, String, "operand_data_block");
