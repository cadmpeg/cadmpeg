// SPDX-License-Identifier: Apache-2.0
//! Feature-history record extractors and their record types.

pub(crate) mod delete;
pub(crate) mod draft;
pub(crate) mod extrude_32;
pub(crate) mod fset;
use self::fset::FeatureFsetReferenceGroup;
use extrude_32::{FeatureExtrude32Construction, FeatureExtrudePayload32Branch};
pub(crate) mod holes;
pub(crate) mod pattern;
pub(crate) mod point_scalar_lane;
use point_scalar_lane::{FeaturePointConstructionScalarLane, PointScalarPositions};
pub(crate) mod payload_name;
use payload_name::FeaturePayloadName;

mod reference;
use crate::om::column_row::ColumnRowSlot;
use crate::om::datum_csys::DatumCsysSlot;
use crate::om::header_references::HeaderSlot;
use reference::ConstructionReference;

#[allow(clippy::wildcard_imports)]
use super::*;
pub(crate) mod block_reference;
pub(crate) mod body_scalar_triple;
use body_scalar_triple::FeatureOperationBodyScalarTriple;
mod body_write_wire;
mod common_frame_wire;
pub(crate) mod object_frame;
pub(crate) mod operation_record;
pub(crate) mod surface_branches;
pub(crate) mod swp104_branch;
pub(crate) mod terminal_discriminator;
pub(crate) mod thru_curve_branches;
use terminal_discriminator::FeatureOperationTerminalDiscriminator;
pub(crate) mod unlabeled_record;
use crate::native::om::column_row::{
    DataBlockIndexRow, DataBlockLinkedIndexRow, DataBlockTargetIndexRow,
};
use crate::native::om::journal_group::OmOperationStateJournalGroup;
use crate::native::om::{
    data_blocks, DataBlockColumnIndexTable, DataBlockReference, DataBlockRole, Expression,
    ExpressionDeclaration, OmSchemaRole,
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
use block_reference::{BlockReferencePosition, FeatureBlockConstructionReference};
use object_frame::DataBlockObjectFrame;
use operation_record::{FeatureOperationRecord, OperationRecordSpan};
use std::borrow::Cow;
use std::num::NonZeroU8;
use swp104_branch::FeatureSwp104LeadingBranch;
use unlabeled_record::FeatureUnlabeledOperationRecord;
mod pair_wire;
use crate::om::csys_descriptor::{
    CsysDescriptor, CsysDescriptorSlot, CsysIdentity, LocatedCsysDescriptor,
};

use crate::om::plane_descriptor::PlaneDescriptor;
use crate::om::thru_curve_controls::ThruCurveControls;

pub(crate) mod datum_plane_header;
mod joined_payload;
mod payload_content;
use datum_plane_header::FeatureDatumPlaneHeader;
use joined_payload::JoinedPayload;
use payload_content::{FeaturePayloadBlock, FeaturePayloadContent};

/// Ordered feature operation label from a feature-history record area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationLabel {
    /// Globally unique label identity.
    pub id: String,
    /// Link identifying the owning ordered OM section.
    pub section_link: String,
    /// Zero-based order within the record area.
    pub ordinal: u32,
    /// Exact printable operation name.
    pub value: String,
    /// Four nullable references with their exact source encodings.
    #[serde(flatten)]
    pub objects: crate::om::header_references::HeaderReferences,
    /// Record-order-independent header identity when every non-null slot
    /// resolves to a unique content-backed offset-store block and the tuple
    /// is unique across the feature-history sections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_identity: Option<String>,
    /// Absolute file offset of the `03` label tag.
    pub source_offset: u64,
}

/// Return operation labels in neutral construction-history order.
///
/// Each feature-history section stores its operation records newest first. The
/// native label arena retains that source order, but neutral dependencies and
/// feature ordinals use oldest-first construction order within each section.
pub(crate) fn feature_operation_chronological_labels(
    labels: &[FeatureOperationLabel],
) -> Vec<&FeatureOperationLabel> {
    let mut sections = Vec::<(&str, Vec<&FeatureOperationLabel>)>::new();
    for label in labels {
        if let Some((_, section)) = sections
            .iter_mut()
            .find(|(section_link, _)| *section_link == label.section_link)
        {
            section.push(label);
        } else {
            sections.push((label.section_link.as_str(), vec![label]));
        }
    }
    sections.sort_by(|(left_link, left), (right_link, right)| {
        left.iter()
            .map(|label| label.source_offset)
            .min()
            .cmp(&right.iter().map(|label| label.source_offset).min())
            .then_with(|| left_link.cmp(right_link))
    });
    sections
        .into_iter()
        .flat_map(|(_, mut section)| {
            section.sort_by_key(|label| std::cmp::Reverse(label.source_offset));
            section
        })
        .collect()
}

/// Exact body-write frame retained from one feature operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "body_write_wire::BodyWriteWire",
    into = "body_write_wire::BodyWriteWire"
)]
pub struct FeatureOperationBodyWrite {
    pub id: String,
    pub operation_label: Option<String>,
    pub operation_record: String,
    pub ordinal: u32,
    pub frame: crate::om::body_write::BodyWriteFrame<u64>,
    pub body_image_data_block: Option<String>,
}

/// Exact bridge from a body-write image block to one plain cached-body stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationBodyImageSegmentUse {
    /// Globally unique bridge identity.
    pub id: String,
    /// Body-write frame owning both the body identity and image block.
    pub operation_body_write: String,
    /// Unambiguous offset-store block containing the serialized body image.
    pub body_image_data_block: String,
    /// Plain cached-body tuple whose alias equals the body identity.
    pub segment_body_binding: String,
}

/// Exact persistent body-identity match to one plain cached-body stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationBodyIdentitySegmentUse {
    /// Globally unique bridge identity.
    pub id: String,
    /// Body-write frame carrying the persistent body identity.
    pub operation_body_write: String,
    /// Persistent body identity shared by the frame and plain-stream alias.
    pub body_identity: u8,
    /// Unique plain cached-body tuple with the equal alias.
    pub segment_body_binding: String,
}

/// Exact owning-partition scope for one body-write image and its GROUP node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationBodyPartitionUse {
    /// Globally unique partition-use identity.
    pub id: String,
    /// Body-write frame carrying the partition-local GROUP node.
    pub operation_body_write: String,
    /// Exact body-image relation that selects the cached-body stream.
    pub body_image_segment_use: String,
    /// Plain cached-body binding inside the partition's body-history run.
    pub segment_body_binding: String,
    /// Partition stream that terminates the complete cached-body run.
    pub partition_stream_ordinal: u32,
    /// Partition-local GROUP node carried by the body-write frame.
    pub group_node: u32,
    /// Ordered GROUP record updates retained inside the owning partition scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parasolid_group_records: Vec<String>,
    /// Current ordered GROUP members resolved inside the owning partition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parasolid_group_members: Vec<String>,
}

/// Exact partition ownership of one labeled or unlabeled body-write GROUP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBodyWriteGroupPartitionUse {
    /// Globally unique partition-use identity.
    pub id: String,
    /// Labeled or unlabeled body-write frame carrying the GROUP node.
    pub body_write: String,
    /// Persistent body identity carried by the frame.
    pub body_identity: u8,
    /// Partition-local GROUP node carried by the frame.
    pub group_node: u32,
    /// Unique partition namespace containing every matched GROUP record.
    pub partition_stream_ordinal: u32,
    /// Ordered GROUP record updates retained inside the partition scope.
    pub parasolid_group_records: Vec<String>,
    /// Current ordered GROUP members resolved inside the partition scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parasolid_group_members: Vec<String>,
}

/// Exact direct object-reference field retained from one feature operation.
///
/// The optional tag and object identity are native evidence. They do not assign a
/// body, operand, input, or output role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureOperationObjectReferenceWire",
    into = "FeatureOperationObjectReferenceWire"
)]
pub struct FeatureOperationObjectReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Owning bounded operation-record identity.
    pub operation_record: String,
    /// Zero-based reference order within the operation payload.
    pub ordinal: u32,
    pub frame: crate::om::direct_reference::DirectReferenceFrame<u64>,
    /// Unique target in the native offset-store data-block arena, when found.
    pub data_block: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct FeatureOperationObjectReferenceWire {
    /// Globally unique reference identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Owning bounded operation-record identity.
    pub operation_record: String,
    /// Zero-based reference order within the operation payload.
    pub ordinal: u32,
    /// Byte between the opening `01 02` marker and the object index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<u8>,
    /// Referenced feature object index.
    pub object_index: u32,
    /// Exact serialized object-index token.
    pub raw_object_index: Vec<u8>,
    /// Unique target in the native offset-store data-block arena, when found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute offset of the object-index token.
    pub object_index_source_offset: u64,
    /// Exact serialized field byte length.
    pub byte_len: u64,
    /// Absolute offset of the opening `01 02` marker.
    pub source_offset: u64,
}

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "common_frame_wire::CommonFrameWire",
    into = "common_frame_wire::CommonFrameWire"
)]
pub struct FeatureOperationCommonFrame {
    pub id: String,
    pub operation_record: String,
    pub ordinal: u32,
    pub frame: crate::om::common_frame::CommonFrame<u64, Option<String>>,
}

/// Canonical terminal common-frame suffix of one feature operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "common_frame_wire::TerminalFrameWire",
    into = "common_frame_wire::TerminalFrameWire"
)]
pub struct FeatureOperationTerminalFrame {
    pub id: String,
    pub operation_record: String,
    pub immediate_common_frame: Option<String>,
    pub frame: crate::om::common_frame::TerminalFrame<u64, Option<String>>,
}

/// Exact join from an operation terminal ordinal to its state-journal row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationStateJournalUse {
    /// Globally unique operation-to-journal relation identity.
    pub id: String,
    /// Owning feature-history section.
    pub section_link: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Owning bounded operation record.
    pub operation_record: String,
    /// Terminal frame carrying the local ordinal.
    pub operation_terminal_frame: String,
    /// State-journal group containing the matching row.
    pub journal_group: String,
    /// Zero-based row order within the journal group.
    pub journal_row_ordinal: u32,
    /// Ordinal shared by the operation terminal frame and the journal row.
    pub state_ordinal: u32,
    /// Absolute source offset of the operation terminal frame.
    pub operation_source_offset: u64,
    /// Absolute source offset of the matching journal row.
    pub journal_source_offset: u64,
}

/// Ordered length-framed string from one bounded feature-operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturePayloadString {
    /// Globally unique string identity.
    pub id: String,
    /// Owning exact feature-operation record.
    pub operation_record: String,
    /// Zero-based string order within the post-label payload.
    pub ordinal: u32,
    /// Exact UTF-8 string value.
    pub value: crate::payload_text::PayloadText<String>,
    /// Absolute file offset of the `04` marker.
    pub source_offset: u64,
}

/// Primary selection or ordered body-reference field in one feature operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureBodyReferenceWire",
    into = "FeatureBodyReferenceWire"
)]
pub struct FeatureBodyReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based field order; absent for the primary body selection.
    pub ordinal: Option<u32>,
    /// Serialized reference index interpreted through its resolved namespace.
    pub body: crate::om::reference_index::FeatureReferenceToken,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureBodyReferenceWire {
    /// Globally unique reference identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based field order; absent for the primary body selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<u32>,
    /// Serialized reference index interpreted through its resolved namespace.
    pub body_object_index: u32,
    /// Exact serialized variable-width object-index token.
    pub raw_body_object_index: Vec<u8>,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

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
pub struct FeatureBodySegmentUse {
    /// Globally unique use identity.
    pub id: String,
    /// Primary field in the native `feature_body_references` arena.
    pub feature_body_reference: String,
    /// Segment image in the native `segment_body_bindings` arena.
    pub segment_body_binding: String,
}

/// Primary feature body field resolved in its operation's offset-store namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBodyDataBlockUse {
    /// Globally unique use identity.
    pub id: String,
    /// Primary field in the native `feature_body_references` arena.
    pub feature_body_reference: String,
    /// Target in the native `data_blocks` arena.
    pub data_block: String,
}

/// Operation-header input resolved to one bounded offset-only OM data block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureInputBlock {
    /// Globally unique input-binding identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: HeaderSlot,
    /// Required reference with its exact source encoding.
    #[serde(flatten)]
    pub object: crate::om::reference_index::FeatureReferenceToken,
    /// Target in the native `data_blocks` arena.
    pub data_block: String,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// Input-block bindings from distinct operations that resolve to one data block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureInputBlockIdentityGroupWire",
    into = "FeatureInputBlockIdentityGroupWire"
)]
pub struct FeatureInputBlockIdentityGroup {
    /// Globally unique group identity.
    pub id: String,
    /// Shared target in the native `data_blocks` arena.
    pub data_block: String,
    /// Input bindings in ascending source-offset order.
    pub members: Vec<FeatureInputBlockIdentityMember>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureInputBlockIdentityMember {
    pub input_block: String,
    pub operation_label: String,
    pub input_slot: HeaderSlot,
    pub source_offset: u64,
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
pub enum ColumnIndexRowKind {
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
pub struct FeatureInputColumnRowUse {
    /// Globally unique use identity.
    pub id: String,
    /// Feature input binding that resolves to the shared block.
    pub input_block: String,
    /// Owning feature operation label.
    pub operation_label: String,
    /// Input slot in the operation header.
    pub input_slot: HeaderSlot,
    /// Serialized grammar of the referenced column row.
    pub row_kind: ColumnIndexRowKind,
    /// Native row identity in its grammar-specific arena.
    pub column_row: String,
    /// Unique complete composite table containing the row, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_table: Option<String>,
    /// Zero-based slot in the row's four-block lane.
    pub row_slot: ColumnRowSlot,
    /// Exact shared target in the native `data_blocks` arena.
    pub data_block: String,
    /// Absolute file offset of the row's compact block index.
    pub source_offset: u64,
}

/// Linked or target-index row whose slot zero is a feature input block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureInputColumnTargetRow {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureInputColumnTargetWire",
    into = "FeatureInputColumnTargetWire"
)]
pub struct FeatureInputColumnTarget {
    /// Globally unique target identity.
    pub id: String,
    /// Feature input binding that resolves to the target block.
    pub input_block: String,
    /// Owning feature operation label.
    pub operation_label: String,
    /// Input slot in the operation header.
    pub input_slot: HeaderSlot,
    /// Linked or target-index row whose slot zero is the input block.
    pub column_row: String,
    /// Linked or target-index grammar of `column_row`.
    pub row: FeatureInputColumnTargetRow,
    /// Three compact values following the fixed row marker.
    pub field_indices: [u32; 3],
    /// Three same-section blocks addressed by `field_indices`.
    pub field_data_blocks: [String; 3],
    /// Absolute offsets of the three compact field values.
    pub field_source_offsets: [u64; 3],
    /// Serialized row mode.
    pub mode: crate::om::discriminators::IndexRowMode,
    /// Unique complete composite table containing `column_row`.
    pub column_table: String,
    /// Exact target in the native `data_blocks` arena.
    pub data_block: String,
    /// Absolute file offset of the row's target block index.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureInputColumnTargetWire {
    id: String,
    input_block: String,
    operation_label: String,
    input_slot: HeaderSlot,
    column_row: String,
    row_kind: ColumnIndexRowKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    leading_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    leading_index_source_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    discriminator: Option<crate::om::discriminators::LinkedIndexDiscriminator>,
    field_indices: [u32; 3],
    field_data_blocks: [String; 3],
    field_source_offsets: [u64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    flag: Option<crate::om::discriminators::LinkedIndexFlag>,
    mode: crate::om::discriminators::IndexRowMode,
    column_table: String,
    data_block: String,
    source_offset: u64,
}

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
pub struct FeatureParameterBinding {
    /// Globally unique binding identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: HeaderSlot,
    /// Input block carrying the object-reference field.
    pub input_block: String,
    /// Zero-based object-reference order within the input block.
    pub reference_ordinal: u32,
    /// Target parameter declaration in the native expression arena.
    pub expression_declaration: String,
    /// Exact numeric expression bound to the declaration, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    /// Persistent OM object ID of the declaration.
    pub object_id: u32,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// All binding occurrences by which one operation consumes one expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FeatureParameterUseWire", into = "FeatureParameterUseWire")]
pub struct FeatureParameterUse {
    /// Globally unique use identity.
    pub id: String,
    /// Consuming operation-label identity.
    pub operation_label: String,
    /// Exact numeric expression consumed by the operation.
    pub expression: String,
    /// Binding occurrences in ascending source-offset order.
    pub bindings: Vec<FeatureParameterUseBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureParameterUseBinding {
    pub binding: String,
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureParameterUseWire {
    id: String,
    operation_label: String,
    expression: String,
    bindings: Vec<String>,
    source_offsets: Vec<u64>,
}

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
pub struct FeatureSketchRecord {
    /// Globally unique sketch-record identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Zero-based order within the feature-history area.
    pub ordinal: u32,
    /// Exact bounded operation record.
    pub operation_record: String,
    /// Resolved input bindings in header-slot order.
    pub input_blocks: Vec<String>,
    /// Ordered references carried by the sketch payload.
    pub payload_references: Vec<String>,
    /// Absolute file offset of the operation label.
    pub source_offset: u64,
}

/// Completely resolved native construction lane of a datum coordinate system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureDatumCsysConstructionWire",
    into = "FeatureDatumCsysConstructionWire"
)]
pub struct FeatureDatumCsysConstruction {
    pub id: String,
    pub operation_label: String,
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
pub struct FeatureDatumCsysColumnRowUse {
    /// Globally unique use identity.
    pub id: String,
    /// Owning datum-CSYS construction.
    pub construction: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Zero-based slot in the construction's eight-block lane.
    pub construction_slot: DatumCsysSlot,
    /// Serialized grammar of the referenced column row.
    pub row_kind: ColumnIndexRowKind,
    /// Native row identity in its grammar-specific arena.
    pub column_row: String,
    /// Unique complete composite table containing the row, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_table: Option<String>,
    /// Zero-based slot in the row's four-block lane.
    pub row_slot: ColumnRowSlot,
    /// Exact shared target in the native `data_blocks` arena.
    pub data_block: String,
    /// Absolute file offset of the construction's object-index token.
    pub construction_source_offset: u64,
    /// Absolute file offset of the row's compact block index.
    pub row_source_offset: u64,
}

/// Exact logical payload reconstructed from the two leading datum-CSYS blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumCsysPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Construction defining the ordered block lane.
    pub construction: String,
    /// Ordered source blocks and the hash of their concatenated bytes.
    #[serde(flatten)]
    pub content: FeaturePayloadContent<[FeaturePayloadBlock; 2]>,
}

/// One exactly framed scalar pair in a reconstructed feature payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "FeaturePayloadScalarPairWire")]
pub struct FeaturePayloadScalarPair {
    /// Globally unique scalar-pair identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the frame.
    #[serde(flatten)]
    pub payload: FeatureScalarPairPayload,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Absolute source offsets of the scalar encodings across payload blocks.
    pub value_source_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
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
pub enum FeatureScalarPairPayload {
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
    pub fn id(&self) -> &str {
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
        record.serialize_field("values", &atoms.map(ShiftedBinary64::value))?;
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "pair_wire::FeatureDatumCsysPayloadFixedPairWire",
    into = "pair_wire::FeatureDatumCsysPayloadFixedPairWire"
)]
pub struct FeatureDatumCsysPayloadFixedPair {
    /// Globally unique fixed-pair identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the frame.
    pub datum_csys_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered dimensionless Q1.55 values.
    pub values: [Q155; 2],
    /// Closed pair framing and checked payload position.
    pub position: PairPosition<DatumPairForm>,
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the two `30` atom markers.
    pub value_source_offsets: [u64; 2],
}

/// One exactly framed scalar field in a reconstructed feature payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeaturePayloadScalarWire",
    into = "FeaturePayloadScalarWire"
)]
pub struct FeaturePayloadScalar {
    /// Globally unique scalar-field identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the field.
    #[serde(flatten)]
    pub payload: FeatureScalarPayload,
    /// Zero-based field order within the payload.
    pub ordinal: u32,
    /// Serialized discriminator following the `50 59 66` marker.
    pub field_code: u8,
    /// Checked shifted-binary64 atom.
    pub scalar: ShiftedBinary64,
    /// Payload-relative offset of the field marker.
    pub payload_offset: u64,
    /// Absolute source offset of the field marker.
    pub source_offset: u64,
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

impl From<FeaturePayloadScalar> for FeaturePayloadScalarWire {
    fn from(value: FeaturePayloadScalar) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            payload: value.payload,
            ordinal: value.ordinal,
            field_code: value.field_code,
            value: value.scalar.value(),
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
pub enum FeatureScalarPayload {
    DatumCsys { datum_csys_payload: String },
    Construction { construction_payload: String },
}

impl FeatureScalarPayload {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::DatumCsys { datum_csys_payload } => datum_csys_payload,
            Self::Construction {
                construction_payload,
            } => construction_payload,
        }
    }
}

/// Typed descriptor from one of the final three datum-CSYS construction lanes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureDatumCsysDescriptorWire",
    into = "FeatureDatumCsysDescriptorWire"
)]
pub struct FeatureDatumCsysDescriptor {
    /// Globally unique descriptor identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Construction carrying the descriptor lane.
    pub construction: String,
    /// Construction reference ordinal in the range 5–7.
    pub reference_ordinal: CsysDescriptorSlot,
    /// Resolved source block.
    pub data_block: String,
    /// Checked descriptor bytes and source position.
    pub descriptor: LocatedCsysDescriptor,
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
        let descriptor =
            CsysDescriptor::from_wire(wire.prefix, identity, wire.suffix).map_err(str::to_owned)?;
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
pub struct FeatureDatumPlaneCsysIdentityUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Shared lowercase hexadecimal identity.
    pub identity: CsysIdentity,
    /// Typed datum-plane descriptor.
    pub datum_plane_descriptor: String,
    /// Datum-plane operation carrying the descriptor.
    pub datum_plane_operation_label: String,
    /// Typed datum-CSYS descriptor.
    pub datum_csys_descriptor: String,
    /// Datum-CSYS operation carrying the descriptor.
    pub datum_csys_operation_label: String,
    /// Datum-CSYS construction reference ordinal.
    pub datum_csys_reference_ordinal: CsysDescriptorSlot,
}

/// Exact logical datum-plane object payload reconstructed in lane order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureDatumPlanePayloadWire",
    into = "FeatureDatumPlanePayloadWire"
)]
pub struct FeatureDatumPlanePayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `DATUM_PLANE` operation label.
    pub operation_label: String,
    /// Header defining the ordered object-block lane.
    pub datum_plane_header: String,
    /// Ordered source blocks and the hash of their concatenated bytes.
    pub content: FeaturePayloadContent<Vec<FeaturePayloadBlock>>,
    /// Unique terminal index lane, when the payload has exactly one.
    pub index_lane: Option<crate::om::datum_index::DatumIndexLane<u64>>,
}

#[derive(Serialize, Deserialize)]
struct FeatureDatumPlanePayloadWire {
    id: String,
    operation_label: String,
    datum_plane_header: String,
    #[serde(flatten)]
    content: FeaturePayloadContent<Vec<FeaturePayloadBlock>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    index_lane_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    index_lane_declared_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    index_lane_values: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    index_lane_raw_indices: Vec<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    index_lane_value_offsets: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    index_lane_trailer: Option<u32>,
}

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureDatumPlaneDescriptorWire",
    into = "FeatureDatumPlaneDescriptorWire"
)]
pub struct FeatureDatumPlaneDescriptor {
    /// Globally unique descriptor identity.
    pub id: String,
    /// Owning `DATUM_PLANE` operation label.
    pub operation_label: String,
    /// Header carrying the descriptor reference.
    pub datum_plane_header: String,
    /// Zero-based descriptor-lane order.
    pub ordinal: u32,
    /// Resolved source block.
    pub data_block: String,
    /// Exact identity, schema token, and terminal label.
    pub descriptor: PlaneDescriptor,
    /// Absolute source offset of the descriptor block.
    pub source_offset: u64,
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
        let descriptor =
            PlaneDescriptor::from_wire(wire.identity, &wire.suffix, wire.schema_index, &wire.label)
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
pub enum DatumPlaneBlockLane {
    /// Compact descriptor lane.
    Descriptor,
    /// Canonical object-reference lane.
    Object,
}

/// Exact reuse of one resolved datum-plane construction block by an operation input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumPlaneBlockUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Owning datum-plane header.
    pub datum_plane_header: String,
    /// `DATUM_PLANE` operation owning the construction.
    pub construction_operation_label: String,
    /// Construction lane containing the block.
    pub lane: DatumPlaneBlockLane,
    /// Zero-based position within the lane.
    pub reference_ordinal: u32,
    /// Shared offset-store block.
    pub data_block: String,
    /// Matching operation-header input binding.
    pub input_binding: String,
    /// Operation whose header addresses the shared block.
    pub input_operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: HeaderSlot,
}

/// Exact reuse of one datum-coordinate-system construction block by an operation input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumCsysBlockUse {
    /// Globally unique block-use identity.
    pub id: String,
    /// Owning datum-coordinate-system construction.
    pub construction: String,
    /// `DATUM_CSYS` operation owning the construction lane.
    pub construction_operation_label: String,
    /// Zero-based position in the eight-reference construction lane.
    pub reference_ordinal: DatumCsysSlot,
    /// Shared offset-store block.
    pub data_block: String,
    /// Matching operation-header input binding.
    pub input_binding: String,
    /// Operation whose header addresses the shared block.
    pub input_operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: HeaderSlot,
}

/// A construction reference paired with its uniquely resolved source block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureConstructionMember {
    pub reference: String,
    pub data_block: String,
}

/// Completely resolved counted-reference field of one sketch construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureSketchConstructionInputsWire",
    into = "FeatureSketchConstructionInputsWire"
)]
pub struct FeatureSketchConstructionInputs {
    /// Globally unique construction-input identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Joined typed sketch record.
    pub sketch_record: String,
    /// Ordered references and their uniquely resolved source blocks.
    pub members: Vec<FeatureConstructionMember>,
    /// Reference following the field separator.
    pub terminal_reference: String,
    /// Uniquely resolved terminal block.
    pub terminal_data_block: String,
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
pub struct FeatureConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Construction records selecting the ordered source blocks.
    #[serde(flatten)]
    pub owner: FeatureConstructionOwner,
    /// Ordered source blocks and the hash of their concatenated bytes.
    #[serde(flatten)]
    pub content: FeaturePayloadContent<Vec<FeaturePayloadBlock>>,
}

/// Construction-specific ownership fields of a reconstructed payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    untagged,
    expecting = "construction ownership fields with a valid operation_kind for the selected grammar"
)]
pub enum FeatureConstructionOwner {
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
pub enum FeatureProjectedCurveKind {
    #[serde(rename = "CPROJ")]
    Projected,
    #[serde(rename = "CPROJ_CMB")]
    Combined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeaturePatternKind {
    #[serde(rename = "Pattern Feature")]
    Feature,
    #[serde(rename = "Pattern Geometry")]
    Geometry,
}

/// One exactly framed scaled shifted-binary64 pair in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "pair_wire::FeatureSketchPayloadFixedPairWire",
    into = "pair_wire::FeatureSketchPayloadFixedPairWire"
)]
pub struct FeatureSketchPayloadFixedPair {
    /// Globally unique fixed-pair identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying the frame.
    pub construction_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered values reconstructed from the `30` shifted-binary64 atoms and scaled by `1/4`.
    pub values: [SketchScaledAtom; 2],
    /// Closed pair framing and checked payload position.
    pub position: PairPosition<SketchPairForm>,
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the two atom markers.
    pub value_source_offsets: [u64; 2],
}

/// One exactly framed mixed scaled shifted-binary64/binary32 pair in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "pair_wire::FeatureSketchPayloadMixedPairWire",
    into = "pair_wire::FeatureSketchPayloadMixedPairWire"
)]
pub struct FeatureSketchPayloadMixedPair {
    /// Globally unique mixed-pair identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying the frame.
    pub construction_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Exact scaled binary64 and binary32 atoms.
    pub scalars: SketchMixedScalars,
    /// Closed pair framing and checked payload position.
    pub position: PairPosition<MixedPairForm>,
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the two atom markers.
    pub value_source_offsets: [u64; 2],
}

/// Exact scalar-vector frame retained from one reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureSketchPayloadScalarLaneWire",
    into = "FeatureSketchPayloadScalarLaneWire"
)]
pub struct FeatureSketchPayloadScalarLane {
    /// Globally unique scalar-lane identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying this lane.
    pub construction_payload: String,
    /// Zero-based lane order within the reconstructed payload.
    pub ordinal: u32,
    /// Typed lane form and contiguous atoms with their absolute source locations.
    pub lane: FramedScalarRun<SketchScalarLaneForm, u64>,
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offset of the terminating zero atom.
    pub terminator_source_offset: u64,
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
                .map(|(_, scalar, _)| scalar.value())
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
            .checked_sub(wire.discriminator.len() as u64)
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
            NonEmpty::new(values).ok_or("values must contain a sketch scalar atom")?,
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
pub struct FeatureSketchPayloadNamedRecord {
    /// Globally unique named-record identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying this record.
    pub construction_payload: String,
    /// Name field opening the retained interval.
    pub name_field: String,
    /// Ordered scalar fields before the next complete name field.
    pub scalar_fields: Vec<String>,
    /// Ordered fixed-pair fields before the next complete name field.
    pub fixed_pairs: Vec<String>,
    /// Mixed scaled shifted-binary64/binary32 pairs contained by this interval in payload order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mixed_pairs: Vec<String>,
    /// Payload-relative offset of the opening name marker.
    pub payload_start_offset: u64,
    /// Payload-relative exclusive end at the next name or payload boundary.
    pub payload_end_offset: u64,
}

/// Complete named two-dimensional point in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchPoint {
    /// Globally unique point identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Name-delimited payload record carrying the point.
    pub named_record: String,
    /// Exact `Point<decimal>` source name.
    pub name: String,
    /// Ordered scalar fields carrying the two coordinates.
    pub scalar_fields: [String; 2],
    /// Ordered finite native coordinate values.
    pub coordinates: [f64; 2],
}

/// Complete named scaled shifted-binary64 record in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchFixedPoint {
    /// Globally unique fixed-point identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Name-delimited payload record carrying the pair.
    pub named_record: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Exact fixed-pair field carrying the two values.
    pub fixed_pair: String,
    /// Ordered values reconstructed from the `30` shifted-binary64 atoms and scaled by `1/4`.
    pub values: [f64; 2],
    /// Absolute source offset of the fixed-pair discriminator.
    pub source_offset: u64,
}

/// Exact same-name point identity within one reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchPointGroup {
    /// Globally unique point-group identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Identical point records in payload order.
    pub points: Vec<String>,
    /// Bit-identical ordered coordinate values.
    pub coordinates: [f64; 2],
}

/// Named two-scalar point object spanning consecutive offset-store blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "OffsetStoreNamedPointWire",
    into = "OffsetStoreNamedPointWire"
)]
pub struct OffsetStoreNamedPoint {
    /// Globally unique point-object identity.
    pub id: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Minimal consecutive source-block span carrying the object.
    pub data_blocks: Vec<String>,
    /// Checked scalar atoms and their absolute frame offsets.
    pub values: [FeatureBinary64ScalarToken; 2],
    /// Absolute source offset of the name frame.
    pub source_offset: u64,
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

impl From<OffsetStoreNamedPoint> for OffsetStoreNamedPointWire {
    fn from(value: OffsetStoreNamedPoint) -> Self {
        Self {
            id: value.id,
            name: value.name,
            data_blocks: value.data_blocks,
            values: value.values.map(|token| token.scalar.value()),
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
pub struct FeatureSketchNamedPointBlockUse {
    /// Globally unique block-use identity.
    pub id: String,
    /// Sketch operation carrying the reference.
    pub operation_label: String,
    /// Typed sketch-reference occurrence.
    pub sketch_reference: String,
    /// Reference order within the sketch field.
    pub reference_ordinal: u32,
    /// Typed named-point object containing the block.
    pub named_point: String,
    /// Shared offset-store block.
    pub data_block: String,
    /// Block position within the named-point span.
    pub point_block_ordinal: u32,
    /// Absolute source offset of the sketch reference.
    pub source_offset: u64,
}

/// Exact predecessor relation between a named point and a sketch construction lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchPrecedingNamedPointUse {
    /// Globally unique predecessor-use identity.
    pub id: String,
    /// Sketch operation carrying the construction lane.
    pub operation_label: String,
    /// First typed sketch-reference occurrence.
    pub first_sketch_reference: String,
    /// Typed named-point object ending immediately before the construction lane.
    pub named_point: String,
    /// Complete ordered block span of the named point.
    pub point_data_blocks: Vec<String>,
    /// First construction block immediately following the point span.
    pub following_data_block: String,
    /// Absolute source offset of the first sketch reference.
    pub source_offset: u64,
}

/// Exact identity of one solved sketch point across its payload and reference lanes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureSketchPointUseWire",
    into = "FeatureSketchPointUseWire"
)]
pub struct FeatureSketchPointUse {
    pub id: String,
    pub operation_label: String,
    pub references: Vec<FeatureSketchPointUseReference>,
    pub sketch_point_group: String,
    pub named_point: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSketchPointUseReference {
    pub sketch_reference: String,
    pub block_use: String,
    pub source_offset: u64,
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
pub enum FeatureSketchDatumCsysBlockRelation {
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
pub struct FeatureSketchDatumCsysScalarAlias {
    /// Zero-based coordinate within the named sketch point.
    pub sketch_coordinate_ordinal: u8,
    /// Exact datum-CSYS scalar field occupying the same source bytes.
    pub datum_csys_scalar: String,
    /// Absolute source offset of the shared scalar field marker.
    pub value_source_offset: u64,
}

/// Exact ordered dependency from a sketch point to a datum coordinate system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchDatumCsysDependency {
    /// Globally unique dependency identity.
    pub id: String,
    /// Earlier sketch operation owning the point identity.
    pub sketch_operation_label: String,
    /// Later datum-coordinate-system operation consuming the point block.
    pub datum_csys_operation_label: String,
    /// Exact sketch-point identity witnessing ownership.
    pub sketch_point_use: String,
    /// Exact datum-coordinate-system construction witnessing consumption.
    pub datum_csys_construction: String,
    /// Exact block relation between the complete point span and construction.
    pub block_relation: FeatureSketchDatumCsysBlockRelation,
    /// Scalar encodings occupying the same source bytes in both typed records.
    pub scalar_aliases: Vec<FeatureSketchDatumCsysScalarAlias>,
    /// Absolute source offset of the first sketch reference witnessing the point identity.
    pub source_offset: u64,
}

/// Ordered object reference carried by a bounded sketch-operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchReference {
    /// Globally unique sketch-reference identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Checked position in the counted field; terminal status is derived.
    #[serde(flatten)]
    pub position: crate::om::sketch_references::SketchReferencePosition,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub token: crate::om::reference_index::ReferenceIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Ordered construction reference carried by a bounded projected-curve payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureProjectedCurveReference {
    /// Globally unique projected-curve reference identity.
    pub id: String,
    /// Owning `CPROJ` or `CPROJ_CMB` operation label.
    pub operation_label: String,
    /// Zero-based order among the field's non-repeated references.
    pub ordinal: u32,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub token: PayloadIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Canonical printable string in a reconstructed projected-curve payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureProjectedCurveConstructionString {
    /// Globally unique string identity.
    pub id: String,
    /// Owning `CPROJ` or `CPROJ_CMB` operation label.
    pub operation_label: String,
    /// Reconstructed projected-curve payload carrying the string.
    pub construction_payload: String,
    /// Zero-based string order within the payload.
    pub ordinal: u32,
    /// Exact printable value.
    pub value: PrintableString<String>,
    /// Payload-relative offset of the `66 32 03` marker.
    pub payload_offset: u64,
    /// Absolute source offset of the marker.
    pub source_offset: u64,
}

/// Exact leading construction header carried by a bounded point-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturePointConstructionHeader {
    /// Globally unique point-construction-header identity.
    pub id: String,
    /// Owning `POINT` operation label.
    pub operation_label: String,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub token: crate::om::reference_index::ReferenceIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Serialized header mode.
    pub mode: crate::om::discriminators::PointHeaderMode,
    /// Absolute file offset of the reference width marker.
    pub source_offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureBinary64ScalarToken {
    pub scalar: ShiftedBinary64,
    pub source_offset: u64,
}

/// Ordered construction reference carried by a bounded surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSurfaceConstructionReference {
    /// Globally unique surface-construction-reference identity.
    pub id: String,
    /// Owning `SKIN`, `Studio Surface`, or `THRU_CURVE` operation label.
    pub operation_label: String,
    /// Zero-based slot order in the exact common envelope.
    pub ordinal: u32,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub token: crate::om::reference_index::PayloadIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Exact leading construction envelope in a `THRU_CURVE` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureThruCurveConstructionEnvelope {
    /// Globally unique envelope identity.
    pub id: String,
    /// Owning `THRU_CURVE` operation label.
    pub operation_label: String,
    /// Nonzero construction discriminator.
    pub discriminator: NonZeroU8,
    /// Exact opaque controls between the reference groups.
    pub controls: ThruCurveControls,
    /// Nonzero control following the second reference group.
    pub trailing_control: NonZeroU8,
    /// Exact two-byte value selected by the `a0` marker.
    pub trailing_value: [u8; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
}

/// Exact logical payload reconstructed from an ordered surface-construction graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSurfaceConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `SKIN` or `Studio Surface` operation label.
    pub operation_label: String,
    /// Ordered construction-reference records.
    pub construction_references: [String; 14],
    /// Ordered source blocks and the hash of their concatenated bytes.
    #[serde(flatten)]
    pub content: FeaturePayloadContent<[FeaturePayloadBlock; 14]>,
}

/// One printable string frame in a reconstructed surface payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSurfaceConstructionString {
    /// Globally unique string identity.
    pub id: String,
    /// Owning `SKIN` or `Studio Surface` operation label.
    pub operation_label: String,
    /// Reconstructed surface payload carrying the frame.
    pub surface_construction_payload: String,
    /// Zero-based string order within the payload.
    pub ordinal: u32,
    /// Exact printable value.
    pub value: crate::payload_text::PayloadText<String>,
    /// Payload-relative offset of the `66 1b 03` marker.
    pub payload_offset: u64,
    /// Absolute source offset of the marker.
    pub source_offset: u64,
}

/// Ordered profile reference carried by a bounded extrusion payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureExtrudeProfileReference {
    /// Globally unique profile-reference identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Zero-based profile-reference order.
    pub ordinal: u32,
    /// Field tag serialized before the counted reference list.
    pub field_tag: u8,
    /// Absolute source offset of the matching duplicate-list index marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_source_offset: Option<u64>,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub token: crate::om::reference_index::PayloadIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Fixed shifted-IEEE scalar header from a bounded extrusion payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureExtrudePayloadHeaderWire",
    into = "FeatureExtrudePayloadHeaderWire"
)]
pub struct FeatureExtrudePayloadHeader {
    /// Globally unique header identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Ordered finite scalar values.
    pub scalars: [ShiftedBinary64; 2],
    /// Absolute file offset of the first shifted-IEEE scalar.
    pub source_offset: u64,
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

impl From<FeatureExtrudePayloadHeader> for FeatureExtrudePayloadHeaderWire {
    fn from(value: FeatureExtrudePayloadHeader) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            scalars: value.scalars.map(ShiftedBinary64::value),
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureOperationBodyMemberWire",
    into = "FeatureOperationBodyMemberWire"
)]
pub struct FeatureOperationBodyMember {
    /// Globally unique member identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Zero-based member order in the counted lane.
    pub ordinal: u32,
    /// Exact compact index and its absolute file position.
    pub member: LocatedCompactIndex<u64>,
}

#[derive(Serialize, Deserialize)]
struct FeatureOperationBodyMemberWire {
    /// Globally unique member identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Zero-based member order in the counted lane.
    pub ordinal: u32,
    /// Decoded compact index.
    pub member_index: u32,
    /// Exact compact-index token.
    pub raw_member_index: Vec<u8>,
    /// Absolute file offset of the compact-index marker.
    pub source_offset: u64,
}

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureOperationBodyOperandWire",
    into = "FeatureOperationBodyOperandWire"
)]
pub struct FeatureOperationBodyOperand {
    /// Globally unique operand identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Body clause containing the operand.
    pub body_object_index: u32,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Zero-based operand order in the wrapped member lane.
    pub ordinal: u32,
    /// Exact operand compact index and its absolute file position.
    pub operand: LocatedCompactIndex<u64>,
    /// Same-store offset data block named by the operand, when resolved.
    pub operand_data_block: Option<String>,
    /// Segment body bindings naming the same body image.
    pub segment_body_bindings: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct FeatureOperationBodyOperandWire {
    /// Globally unique operand identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Body clause containing the operand.
    pub body_object_index: u32,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Zero-based operand order in the wrapped member lane.
    pub ordinal: u32,
    /// Serialized operand body object index.
    pub operand_object_index: u32,
    /// Exact serialized compact-index token.
    pub raw_operand_object_index: Vec<u8>,
    /// Same-store offset data block named by the operand, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operand_data_block: Option<String>,
    /// Segment body bindings naming the same body image.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segment_body_bindings: Vec<String>,
    /// Absolute file offset of the compact-index marker.
    pub source_offset: u64,
}

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

impl FeatureOperationBodyOperand {
    pub(crate) fn source_property_key(&self) -> String {
        format!(
            "operation_body_operand.{}.{}",
            self.body_reference_ordinal, self.ordinal
        )
    }
}

/// Exact continuation following a `TRIM BODY` branch-`11` member lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "reference::Body11ContinuationWire",
    into = "reference::Body11ContinuationWire"
)]
pub struct FeatureOperationBody11Continuation {
    /// Globally unique continuation identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Exact compact continuation index and its absolute file offset.
    pub continuation: crate::om::compact::LocatedCompactIndex<u64>,
    /// Exact required terminal reference.
    pub terminal: crate::om::reference_index::ReferenceIndexToken,
    /// Absolute file offset of the terminal object-index marker.
    pub terminal_source_offset: u64,
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
pub enum FeatureOperationBodyReferences {
    CompactIndex(Vec<ConstructionReference<Option<String>, CompactIndexAtom>>),
    PayloadObjectIndex(Vec<ConstructionReference<Option<String>, PayloadIndexToken>>),
}

/// Counted reference lane following an operation body scalar clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureOperationBodyReferenceLaneWire",
    into = "FeatureOperationBodyReferenceLaneWire"
)]
pub struct FeatureOperationBodyReferenceLane {
    pub id: String,
    pub operation_label: String,
    pub body_reference_ordinal: u32,
    pub body_object_index: u32,
    pub branch: crate::om::discriminators::OperationBodyReferenceBranch,
    pub references: FeatureOperationBodyReferences,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureExtrudeConstructionProfileWire",
    into = "FeatureExtrudeConstructionProfileWire"
)]
pub struct FeatureExtrudeConstructionProfile {
    pub id: String,
    pub operation_label: String,
    pub references: Vec<FeatureExtrudeConstructionProfileReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureExtrudeConstructionProfileReference {
    pub object_index: u32,
    pub data_block: String,
    pub profile_source_offset: u64,
    pub witness_source_offset: u64,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureBlockConstructionWire",
    into = "FeatureBlockConstructionWire"
)]
pub struct FeatureBlockConstruction {
    /// Globally unique construction identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Payload control byte preceding the construction field.
    pub control: u8,
    /// Ordered references and their uniquely resolved source blocks.
    pub members: [FeatureConstructionMember; 18],
    /// Reference following the separator.
    pub terminal_reference: String,
    /// Uniquely resolved terminal block.
    pub terminal_data_block: String,
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
pub struct FeatureBlockPayloadNamedRecord {
    /// Globally unique interval identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Reconstructed payload containing the interval.
    pub construction_payload: String,
    /// Name field opening the interval.
    pub name_field: String,
    /// Complete scalar fields in payload order within the interval.
    pub scalar_fields: Vec<String>,
    /// Inclusive payload-relative start.
    pub payload_start_offset: u64,
    /// Exclusive payload-relative end.
    pub payload_end_offset: u64,
}

/// Exactly two-scalar `Point<positive decimal>` record in a `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureBlockPayloadPoint {
    /// Globally unique typed-point identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Name-delimited payload interval carrying the point.
    pub named_record: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Ordered scalar fields carrying the two coordinates.
    pub scalar_fields: [String; 2],
    /// Ordered finite native coordinate values.
    pub coordinates: [f64; 2],
}

/// Exact same-name point identity within one reconstructed `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureBlockPayloadPointGroup {
    /// Globally unique point-group identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Identical point records in payload order.
    pub points: Vec<String>,
    /// Bit-identical ordered coordinate values.
    pub coordinates: [f64; 2],
}

/// Ordered three-parameter dimension run of one `BLOCK` feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    from = "FeatureBlockDimensionsWire",
    into = "FeatureBlockDimensionsWire"
)]
pub struct FeatureBlockDimensions {
    pub id: String,
    pub operation_label: String,
    pub construction: String,
    pub anchor_bindings: Vec<String>,
    pub dimensions: [FeatureBlockDimension; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeatureBlockDimension {
    pub declaration: String,
    pub expression: String,
    pub value: f64,
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
    /// Ordered finite dimensions in model millimeters.
    values: [f64; 3],
}

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
pub enum FeatureBooleanKind {
    /// Add tool bodies to the target.
    Unite,
    /// Remove tool bodies from the target.
    Subtract,
    /// Retain target/tool intersections.
    Intersect,
}

/// Ordered target/tool binding from a feature-history Boolean operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureBooleanOperationWire",
    into = "FeatureBooleanOperationWire"
)]
pub struct FeatureBooleanOperation {
    pub id: String,
    pub operation_label: String,
    pub kind: FeatureBooleanKind,
    pub target:
        crate::om::PayloadObjectReference<crate::om::reference_index::ReferenceIndexToken, u64>,
    pub tools: Vec<
        crate::om::PayloadObjectReference<crate::om::reference_index::ReferenceIndexToken, u64>,
    >,
    pub source_offset: u64,
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

fn feature_history_sections(container: &Container) -> Vec<(usize, SegmentOmLink)> {
    canonical_feature_history_links(segment_om_links(container))
        .into_iter()
        .enumerate()
        .collect()
}

fn visit_feature_history_operation_records(
    container: &Container,
    mut visit: impl FnMut(
        &crate::om::Section<'_>,
        &str,
        u64,
        usize,
        crate::om::operation_record::OperationRecord<'_>,
    ),
) {
    let sections = container.om_sections();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span()
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.location.section_offset()
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            visit(
                section,
                &section_key,
                entry_offset,
                operation_ordinal,
                record,
            );
        }
    }
}

fn visit_feature_history_unlabeled_operation_records(
    container: &Container,
    mut visit: impl FnMut(
        &crate::om::Section<'_>,
        &str,
        u64,
        usize,
        crate::om::UnlabeledOperationRecord<'_>,
    ),
) {
    let sections = container.om_sections();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span()
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.location.section_offset()
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.unlabeled_operation_records_with_ordinals() {
            visit(
                section,
                &section_key,
                entry_offset,
                operation_ordinal,
                record,
            );
        }
    }
}

pub(crate) fn canonical_feature_history_links(
    links: impl IntoIterator<Item = SegmentOmLink>,
) -> Vec<SegmentOmLink> {
    let mut links = links
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
        .collect::<Vec<_>>();
    links.sort_by(|first, second| {
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
    });
    links.dedup_by_key(|link| link.location.section_offset());
    links
}

/// Return unique content-backed identities for the offset-store ordinals used
/// by operation-header slots.
///
/// A slot ordinal is local to one offset store and can drift when a record is
/// inserted. The map is usable only when the ordinal resolves to exactly one
/// column block across all offset stores and that block has a unique
/// content-backed identity. An ambiguous or duplicate block remains absent.
fn operation_header_block_identities(container: &Container) -> BTreeMap<u32, Option<String>> {
    let mut candidates = BTreeMap::<u32, Vec<Option<String>>>::new();
    for block in data_blocks(container) {
        if block.role == DataBlockRole::Column {
            candidates
                .entry(block.block_ordinal)
                .or_default()
                .push(block.stable_identity);
        }
    }
    candidates
        .into_iter()
        .map(|(ordinal, identities)| {
            let identity = match identities.as_slice() {
                [Some(identity)] => Some(identity.clone()),
                _ => None,
            };
            (ordinal, identity)
        })
        .collect()
}

/// Return the record-order-independent identity encoded by one operation
/// header.
///
/// Every non-null slot must resolve through the complete offset-store map.
/// An all-null tuple, an unresolved slot, or a duplicated content identity has
/// no operation identity witness.
fn operation_header_identity_key(
    object_indices: [Option<u32>; 4],
    block_identities: &BTreeMap<u32, Option<String>>,
) -> Option<String> {
    if !object_indices.iter().any(Option::is_some) {
        return None;
    }
    let slots = object_indices
        .iter()
        .map(|index| match index {
            None => Some("null".to_string()),
            Some(index) => block_identities.get(index)?.clone(),
        })
        .collect::<Option<Vec<_>>>()?;
    Some(format!(
        "nx:feature-history:operation-header-identity#content:{}",
        slots.join("-")
    ))
}

fn assign_operation_header_identities(
    labels: &mut [FeatureOperationLabel],
    block_identities: &BTreeMap<u32, Option<String>>,
) {
    let keys = labels
        .iter()
        .map(|label| operation_header_identity_key(label.objects.values(), block_identities))
        .collect::<Vec<_>>();
    let mut counts = BTreeMap::<String, usize>::new();
    for key in keys.iter().flatten() {
        *counts.entry(key.clone()).or_default() += 1;
    }
    for (label, key) in labels.iter_mut().zip(keys) {
        label.stable_identity = key.filter(|key| counts.get(key) == Some(&1));
    }
}

/// Decode ordered operation labels from feature-history record areas.
pub fn feature_operation_labels(container: &Container) -> Vec<FeatureOperationLabel> {
    let sections = container.om_sections();
    let block_identities = operation_header_block_identities(container);
    let mut labels = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span()
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.location.section_offset()
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        labels.extend(
            section
                .operation_records_with_label_ordinals()
                .into_iter()
                .map(|(ordinal, record)| {
                    let label = record.label();
                    FeatureOperationLabel {
                        id: format!(
                            "nx:feature-history:operation-label#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal: ordinal as u32,
                        value: label.value.to_string(),
                        objects: label.header.objects(),
                        stable_identity: None,
                        source_offset: entry_offset + label.header.end_offset() as u64,
                    }
                }),
        );
    }
    assign_operation_header_identities(&mut labels, &block_identities);
    labels
}

/// Decode ordered Boolean target/tool bindings from feature-history sections.
pub fn feature_boolean_operations(container: &Container) -> Vec<FeatureBooleanOperation> {
    let mut operations = Vec::new();
    visit_feature_history_operation_records(
        container,
        |section, section_key, entry_offset, operation_ordinal, record| {
            let Some(operation) = section
                .boolean_operations()
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
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            operations.push(FeatureBooleanOperation {
                id: format!("nx:feature-history:boolean#{section_key}-{operation_ordinal:010}"),
                operation_label,
                kind,
                target: crate::om::PayloadObjectReference {
                    token: operation.target.token,
                    offset: entry_offset + operation.target.offset as u64,
                },
                tools: operation
                    .tools
                    .into_iter()
                    .map(|tool| crate::om::PayloadObjectReference {
                        token: tool.token,
                        offset: entry_offset + tool.offset as u64,
                    })
                    .collect(),
                source_offset: entry_offset + operation.offset as u64,
            });
        },
    );
    operations
}

/// Decode exact feature-operation record boundaries and byte identities.
pub fn feature_operation_records(container: &Container) -> Vec<FeatureOperationRecord> {
    let block_identities = operation_header_block_identities(container);
    let mut identity_counts = BTreeMap::<String, usize>::new();
    let mut records = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let stable_identity = operation_header_identity_key(
                record.label().header.objects().values(),
                &block_identities,
            );
            let Some(span) = entry_offset
                .checked_add(record.offset() as u64)
                .zip(entry_offset.checked_add(record.payload_offset() as u64))
                .and_then(|(start, payload)| {
                    OperationRecordSpan::new(start, payload, record.payload().len() as u64)
                })
            else {
                return;
            };
            if let Some(key) = &stable_identity {
                *identity_counts.entry(key.clone()).or_default() += 1;
            }
            records.push((
                FeatureOperationRecord {
                    id: format!(
                        "nx:feature-history:operation-record#{section_key}-{operation_ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: operation_ordinal as u32,
                    sha256: cadmpeg_ir::hash::sha256_hex(record.bytes()),
                    payload_sha256: cadmpeg_ir::hash::sha256_hex(record.payload()),
                    stable_identity: None,
                    span,
                },
                stable_identity,
            ));
        },
    );
    records
        .into_iter()
        .map(|(mut record, stable_identity)| {
            record.stable_identity =
                stable_identity.filter(|key| identity_counts.get(key) == Some(&1));
            record
        })
        .collect()
}

/// Retain operation records whose validated headers have no complete label.
pub fn feature_unlabeled_operation_records(
    container: &Container,
) -> Vec<FeatureUnlabeledOperationRecord> {
    let mut records = Vec::new();
    visit_feature_history_unlabeled_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if let Some(record) = FeatureUnlabeledOperationRecord::from_source(
                format!("nx:feature-history:unlabeled-operation-record#{section_key}-{operation_ordinal:010}"),
                operation_ordinal as u32, entry_offset, record,
            ) {
                records.push(record);
            }
        },
    );
    records
}

/// Decode body-write frames owned by independently bounded unlabeled records.
pub fn feature_unlabeled_operation_body_writes(
    container: &Container,
) -> Vec<FeatureOperationBodyWrite> {
    let indexed = container.indexed_om_sections();
    let mut writes = Vec::new();
    visit_feature_history_unlabeled_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let operation_record = format!(
                "nx:feature-history:unlabeled-operation-record#{section_key}-{operation_ordinal:010}"
            );
            for (ordinal, write) in crate::om::unlabeled_operation_body_write_frames(record)
                .into_iter()
                .enumerate()
            {
                let Some(offset) = entry_offset.checked_add(write.offset() as u64) else {
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
                writes.push(FeatureOperationBodyWrite {
                    operation_label: None,
                    id: format!(
                        "nx:feature-history:unlabeled-operation-body-write#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_record: operation_record.clone(),
                    ordinal: ordinal as u32,
                    body_image_data_block: unique_offset_data_block(&indexed, frame.body_image().value()),
                    frame,
                });
            }
        },
    );
    writes
}

/// Decode exact body-write frames from bounded feature operations.
pub fn feature_operation_body_writes(container: &Container) -> Vec<FeatureOperationBodyWrite> {
    let indexed = container.indexed_om_sections();
    let mut writes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let operation_record = format!(
                "nx:feature-history:operation-record#{section_key}-{operation_ordinal:010}"
            );
            for (ordinal, write) in crate::om::operation_body_write_frames(record.payload_view())
                .into_iter()
                .enumerate()
            {
                let Some(offset) = entry_offset.checked_add(write.offset() as u64) else {
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
                writes.push(FeatureOperationBodyWrite {
                    id: format!(
                        "nx:feature-history:operation-body-write#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: Some(operation_label.clone()),
                    operation_record: operation_record.clone(),
                    ordinal: ordinal as u32,
                    body_image_data_block: unique_offset_data_block(&indexed, frame.body_image().value()),
                    frame,
                });
            }
        },
    );
    writes
}

/// Join body-write identities to unique plain cached-body aliases.
///
/// Partition aliases use a separate identity namespace and do not participate.
/// A missing image block, no plain alias, or duplicate plain alias leaves the
/// write unresolved.
pub fn feature_operation_body_image_segment_uses(
    writes: &[FeatureOperationBodyWrite],
    bindings: &[SegmentBodyBinding],
) -> Vec<FeatureOperationBodyImageSegmentUse> {
    writes
        .iter()
        .filter_map(|write| {
            let body_image_data_block = write.body_image_data_block.as_ref()?;
            let mut matches = bindings.iter().filter(|binding| {
                binding.stream_kind == crate::parasolid::StreamKind::Plain
                    && binding.body_alias_object_index == u32::from(write.frame.body_identity())
            });
            let binding = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            Some(FeatureOperationBodyImageSegmentUse {
                id: write.id.replacen(
                    "operation-body-write",
                    "operation-body-image-segment-use",
                    1,
                ),
                operation_body_write: write.id.clone(),
                body_image_data_block: body_image_data_block.clone(),
                segment_body_binding: binding.id.clone(),
            })
        })
        .collect()
}

/// Join persistent body identities to unique plain cached-body aliases.
///
/// This cross-store relation is independent of body-image block resolution.
/// Partition-stream aliases use another identity namespace and do not match.
pub fn feature_operation_body_identity_segment_uses(
    writes: &[FeatureOperationBodyWrite],
    bindings: &[SegmentBodyBinding],
) -> Vec<FeatureOperationBodyIdentitySegmentUse> {
    writes
        .iter()
        .filter_map(|write| {
            let mut matches = bindings.iter().filter(|binding| {
                binding.stream_kind == crate::parasolid::StreamKind::Plain
                    && binding.body_alias_object_index == u32::from(write.frame.body_identity())
            });
            let binding = matches.next()?;
            matches.next().is_none().then_some(())?;
            Some(FeatureOperationBodyIdentitySegmentUse {
                id: write.id.replacen(
                    "operation-body-write",
                    "operation-body-identity-segment-use",
                    1,
                ),
                operation_body_write: write.id.clone(),
                body_identity: write.frame.body_identity(),
                segment_body_binding: binding.id.clone(),
            })
        })
        .collect()
}

const BODY_HISTORY_TERMINAL_STREAM_ROLE: u32 = 16;

fn body_history_partition_stream(
    binding: &SegmentBodyBinding,
    bindings: &[SegmentBodyBinding],
    streams: &[crate::parasolid::Stream],
) -> Option<u32> {
    (binding.stream_kind == crate::parasolid::StreamKind::Plain).then_some(())?;
    let stream_ordinal = usize::try_from(binding.stream_ordinal).ok()?;
    (streams.get(stream_ordinal)?.kind() == crate::parasolid::StreamKind::Plain).then_some(())?;
    let partition_ordinal = streams
        .iter()
        .enumerate()
        .skip(stream_ordinal + 1)
        .find_map(|(ordinal, stream)| {
            (stream.kind() == crate::parasolid::StreamKind::Partition).then_some(ordinal)
        })?;
    let run_start = streams[..stream_ordinal]
        .iter()
        .rposition(|stream| stream.kind() != crate::parasolid::StreamKind::Plain)
        .map_or(0, |ordinal| ordinal + 1);
    let run_streams = streams.get(run_start..partition_ordinal)?;
    run_streams
        .iter()
        .all(|stream| stream.kind() == crate::parasolid::StreamKind::Plain)
        .then_some(())?;
    let mut run_bindings = Vec::with_capacity(run_streams.len());
    for ordinal in run_start..partition_ordinal {
        let mut matches = bindings.iter().filter(|candidate| {
            candidate.stream_kind == crate::parasolid::StreamKind::Plain
                && usize::try_from(candidate.stream_ordinal).ok() == Some(ordinal)
        });
        let candidate = matches.next()?;
        matches.next().is_none().then_some(())?;
        run_bindings.push(candidate);
    }
    let (terminal, preceding) = run_bindings.split_last()?;
    (terminal.stream_role == BODY_HISTORY_TERMINAL_STREAM_ROLE
        && preceding
            .iter()
            .all(|candidate| candidate.stream_role != BODY_HISTORY_TERMINAL_STREAM_ROLE))
    .then_some(())?;
    u32::try_from(partition_ordinal).ok()
}

/// Resolve body-write GROUP nodes only inside their complete body-history unit.
///
/// A plain cached-body binding belongs to the next partition only when every
/// intervening compressed stream is another completely bound plain image and
/// the run has one terminal role-16 binding. GROUP records from other
/// partition-local namespaces never participate, even when their node IDs are
/// equal.
pub fn feature_operation_body_partition_uses(
    writes: &[FeatureOperationBodyWrite],
    image_uses: &[FeatureOperationBodyImageSegmentUse],
    bindings: &[SegmentBodyBinding],
    streams: &[crate::parasolid::Stream],
    groups: &[crate::native::parasolid::ParasolidGroupRecord],
    group_members: &[crate::native::parasolid::ParasolidGroupMember],
) -> Vec<FeatureOperationBodyPartitionUse> {
    image_uses
        .iter()
        .filter_map(|image_use| {
            let mut matching_writes = writes
                .iter()
                .filter(|write| write.id == image_use.operation_body_write);
            let write = matching_writes.next()?;
            matching_writes.next().is_none().then_some(())?;
            let mut matching_bindings = bindings
                .iter()
                .filter(|binding| binding.id == image_use.segment_body_binding);
            let binding = matching_bindings.next()?;
            matching_bindings.next().is_none().then_some(())?;
            let partition_stream_ordinal =
                body_history_partition_stream(binding, bindings, streams)?;
            let parasolid_group_records = groups
                .iter()
                .filter(|group| {
                    group.origin.partition_stream_ordinal() == Some(partition_stream_ordinal)
                        && group.node_id == write.frame.group_node().value()
                })
                .map(|group| group.id.clone())
                .collect();
            let parasolid_group_members = group_members
                .iter()
                .filter(|member| {
                    member.partition_stream_ordinal == partition_stream_ordinal
                        && member.group_node_id == write.frame.group_node().value()
                })
                .map(|member| member.id.clone())
                .collect();
            Some(FeatureOperationBodyPartitionUse {
                id: write
                    .id
                    .replacen("operation-body-write", "operation-body-partition-use", 1),
                operation_body_write: write.id.clone(),
                body_image_segment_use: image_use.id.clone(),
                segment_body_binding: binding.id.clone(),
                partition_stream_ordinal,
                group_node: write.frame.group_node().value(),
                parasolid_group_records,
                parasolid_group_members,
            })
        })
        .collect()
}

/// Resolve body-write GROUP nodes directly through their partition ownership.
///
/// The relation requires at least one retained GROUP record and exactly one
/// partition namespace for that node. Labeled and independently bounded
/// unlabeled writes participate in the same persistent body-identity domain.
pub fn feature_body_write_group_partition_uses(
    writes: &[FeatureOperationBodyWrite],
    unlabeled_writes: &[FeatureOperationBodyWrite],
    groups: &[crate::native::parasolid::ParasolidGroupRecord],
    group_members: &[crate::native::parasolid::ParasolidGroupMember],
) -> Vec<FeatureBodyWriteGroupPartitionUse> {
    let candidates = writes.iter().chain(unlabeled_writes).map(|write| {
        (
            write.id.as_str(),
            write.frame.body_identity(),
            write.frame.group_node().value(),
        )
    });
    candidates
        .filter_map(|(id, body_identity, group_node)| {
            let matching_groups = groups
                .iter()
                .filter(|group| group.node_id == group_node)
                .collect::<Vec<_>>();
            (!matching_groups.is_empty()).then_some(())?;
            let partitions = matching_groups
                .iter()
                .filter_map(|group| group.origin.partition_stream_ordinal())
                .collect::<BTreeSet<_>>();
            let mut partitions = partitions.into_iter();
            let partition_stream_ordinal = partitions.next()?;
            partitions.next().is_none().then_some(())?;
            matching_groups
                .iter()
                .all(|group| {
                    group.origin.partition_stream_ordinal() == Some(partition_stream_ordinal)
                })
                .then_some(())?;
            Some(FeatureBodyWriteGroupPartitionUse {
                id: id.replacen("body-write", "body-write-group-partition-use", 1),
                body_write: id.to_string(),
                body_identity,
                group_node,
                partition_stream_ordinal,
                parasolid_group_records: matching_groups
                    .into_iter()
                    .map(|group| group.id.clone())
                    .collect(),
                parasolid_group_members: group_members
                    .iter()
                    .filter(|member| {
                        member.partition_stream_ordinal == partition_stream_ordinal
                            && member.group_node_id == group_node
                    })
                    .map(|member| member.id.clone())
                    .collect(),
            })
        })
        .collect()
}

/// Decode one direct-reference field family from bounded feature operations.
pub fn feature_operation_object_references(
    container: &Container,
    kind: crate::om::direct_reference::ReferenceFieldKind,
) -> Vec<FeatureOperationObjectReference> {
    let stem = match kind {
        crate::om::direct_reference::ReferenceFieldKind::Tagged17 => "operation-tagged-reference",
        crate::om::direct_reference::ReferenceFieldKind::DataBlock03 => {
            "operation-data-block-reference"
        }
    };
    let indexed = container.indexed_om_sections();
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let operation_record = format!(
                "nx:feature-history:operation-record#{section_key}-{operation_ordinal:010}"
            );
            for (ordinal, reference) in
                crate::om::direct_reference::operation_reference_fields(record.payload_view(), kind)
                    .into_iter()
                    .enumerate()
            {
                let Some(offset) = entry_offset.checked_add(reference.offset() as u64) else {
                    continue;
                };
                let Some(frame) = crate::om::direct_reference::DirectReferenceFrame::<u64>::new(
                    reference.kind(),
                    reference.object(),
                    offset,
                ) else {
                    continue;
                };
                references.push(FeatureOperationObjectReference {
                    id: format!(
                        "nx:feature-history:{stem}#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    operation_record: operation_record.clone(),
                    ordinal: ordinal as u32,
                    data_block: unique_offset_data_block(&indexed, frame.object().value()),
                    frame,
                });
            }
        },
    );
    references
}

/// Decode every exact common frame from bounded feature operations.
pub fn feature_operation_common_frames(container: &Container) -> Vec<FeatureOperationCommonFrame> {
    let indexed = container.indexed_om_sections();
    let mut frames = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let operation_record = format!(
                "nx:feature-history:operation-record#{section_key}-{operation_ordinal:010}"
            );
            for (ordinal, frame) in crate::om::operation_common_frames(record.payload_view())
                .into_iter()
                .enumerate()
            {
                let Some(offset) = entry_offset.checked_add(frame.offset() as u64) else {
                    continue;
                };
                let Some(frame) = crate::om::common_frame::CommonFrame::<u64, Option<String>>::new(
                    frame.prefix(),
                    frame.state(),
                    (*frame.suffix())
                        .map_target(|index, ()| unique_offset_data_block(&indexed, index)),
                    offset,
                ) else {
                    continue;
                };
                frames.push(FeatureOperationCommonFrame {
                    id: format!(
                        "nx:feature-history:operation-common-frame#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_record: operation_record.clone(), ordinal: ordinal as u32,
                    frame,
                });
            }
        },
    );
    frames
}

/// Decode canonical terminal common-frame suffixes from bounded operations.
pub fn feature_operation_terminal_frames(
    container: &Container,
    common_frames: &[FeatureOperationCommonFrame],
) -> Vec<FeatureOperationTerminalFrame> {
    let indexed = container.indexed_om_sections();
    let mut frames = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(frame) = crate::om::operation_terminal_frame(record.payload_view()) else {
                return;
            };
            let operation_record = format!(
                "nx:feature-history:operation-record#{section_key}-{operation_ordinal:010}"
            );
            let immediate_common_frame = frame.immediate_common_frame_offset.and_then(|offset| {
                let offset = entry_offset.checked_add(offset as u64)?;
                let mut matches = common_frames.iter().filter(|common| {
                    common.operation_record == operation_record && common.frame.offset() == offset
                });
                let common = matches.next()?;
                matches.next().is_none().then(|| common.id.clone())
            });
            let Some(offset) = entry_offset.checked_add(frame.frame.offset() as u64) else {
                return;
            };
            let Some(frame) = crate::om::common_frame::TerminalFrame::<u64, Option<String>>::new(
                (*frame.frame.suffix())
                    .map_target(|index, ()| unique_offset_data_block(&indexed, index)),
                offset,
            ) else {
                return;
            };
            frames.push(FeatureOperationTerminalFrame {
                id: format!(
                    "nx:feature-history:operation-terminal-frame#{section_key}-{operation_ordinal:010}"
                ),
                operation_record, immediate_common_frame,
                frame,
            });
        },
    );
    frames
}

/// Join operation terminal ordinals to exact rows in the owning state journal.
pub fn feature_operation_state_journal_uses(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    terminal_frames: &[FeatureOperationTerminalFrame],
    journal_groups: &[OmOperationStateJournalGroup],
) -> Vec<FeatureOperationStateJournalUse> {
    let labels_by_id = labels
        .iter()
        .map(|label| (label.id.as_str(), label))
        .collect::<BTreeMap<_, _>>();
    let record_labels = records
        .iter()
        .filter_map(|record| {
            let label = labels_by_id.get(record.operation_label.as_str())?;
            Some((record.id.as_str(), *label))
        })
        .collect::<BTreeMap<_, _>>();
    let mut journal_rows = BTreeMap::new();
    for group in journal_groups {
        for (row_ordinal, row) in group.frame.rows().iter().enumerate() {
            let Some(row_ordinal) = u32::try_from(row_ordinal).ok() else {
                continue;
            };
            let key = (group.section_link.as_str(), row.ordinal().value());
            match journal_rows.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Some((group, row_ordinal, row)));
                }
                std::collections::btree_map::Entry::Occupied(entry) => {
                    *entry.into_mut() = None;
                }
            }
        }
    }
    let mut uses = Vec::new();
    for frame in terminal_frames {
        let Some(label) = record_labels.get(frame.operation_record.as_str()) else {
            continue;
        };
        let Some(Some((group, journal_row_ordinal, row))) = journal_rows.get(&(
            label.section_link.as_str(),
            frame.frame.suffix().local_ordinal(),
        )) else {
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
        uses.push(FeatureOperationStateJournalUse {
            id: format!(
                "nx:feature-history:operation-state-journal-use#{operation_key}-{journal_key}-{journal_row_ordinal:010}"
            ),
            section_link: label.section_link.clone(),
            operation_label: label.id.clone(),
            operation_record: frame.operation_record.clone(),
            operation_terminal_frame: frame.id.clone(),
            journal_group: group.id.clone(),
            journal_row_ordinal: *journal_row_ordinal,
            state_ordinal: row.ordinal().value(),
            operation_source_offset: frame.frame.offset(),
            journal_source_offset: row.offset(),
        });
    }
    uses
}

/// Decode ordered self-framed strings from feature-operation payloads.
pub fn feature_payload_strings(container: &Container) -> Vec<FeaturePayloadString> {
    let mut strings = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let operation_record = format!(
                "nx:feature-history:operation-record#{section_key}-{operation_ordinal:010}"
            );
            strings.extend(
                crate::om::operation_payload_strings(record.payload_view())
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, value)| FeaturePayloadString {
                        id: format!(
                            "nx:feature-history:payload-string#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                        ),
                        operation_record: operation_record.clone(),
                        ordinal: ordinal as u32,
                        value: value.value.into_owned(),
                        source_offset: entry_offset + value.offset as u64,
                    }),
            );
        },
    );
    strings
}

/// Decode complete body-reference fields from feature-history operations.
pub fn feature_body_references(container: &Container) -> Vec<FeatureBodyReference> {
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(reference) = crate::om::operation_body_reference(record.body_view()) else {
                return;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            references.push(FeatureBodyReference {
                ordinal: None,
                id: format!(
                    "nx:feature-history:body-reference#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                body: reference.object_index,
                source_offset: entry_offset + reference.offset as u64,
            });
        },
    );
    references
}

/// Return the one body-reference field owned by each operation that has
/// exactly one such field.
pub(crate) fn unique_feature_body_references(
    references: &[FeatureBodyReference],
) -> BTreeMap<&str, &FeatureBodyReference> {
    let mut by_operation = BTreeMap::<&str, Vec<&FeatureBodyReference>>::new();
    for reference in references {
        by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    by_operation
        .into_iter()
        .filter_map(|(operation, references)| {
            let [reference] = references.as_slice() else {
                return None;
            };
            Some((operation, *reference))
        })
        .collect()
}

/// Decode every ordered body-reference field from bounded feature operations.
pub fn feature_body_reference_occurrences(container: &Container) -> Vec<FeatureBodyReference> {
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            references.extend(
                crate::om::operation_body_references(record.body_view())
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, reference)| FeatureBodyReference {
                        id: format!(
                            "nx:feature-history:body-reference-occurrence#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                        ),
                        operation_label: operation_label.clone(),
                        ordinal: Some(ordinal as u32),
                        body: reference.object_index,
                        source_offset: entry_offset + reference.offset as u64,
                    }),
            );
        },
    );
    references
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

pub fn feature_body_segment_uses(
    references: &[FeatureBodyReference],
    data_block_uses: &[FeatureBodyDataBlockUse],
    inputs: &[FeatureInputBlock],
    blocks: &[crate::native::om::DataBlock],
    bindings: &[SegmentBodyBinding],
    object_frames: &[DataBlockObjectFrame],
) -> Vec<FeatureBodySegmentUse> {
    let unique_references = unique_feature_body_references(references);
    let offset_store_reference_counts =
        data_block_uses
            .iter()
            .fold(BTreeMap::<&str, usize>::new(), |mut counts, use_| {
                *counts
                    .entry(use_.feature_body_reference.as_str())
                    .or_default() += 1;
                counts
            });
    let offset_store_operations = feature_input_store_operations(inputs, blocks);
    let single_offset_store_operations = feature_input_store_sections(inputs, blocks)
        .into_iter()
        .filter_map(|(operation_label, sections)| (sections.len() == 1).then_some(operation_label))
        .collect::<BTreeSet<_>>();
    references
        .iter()
        .filter(|reference| {
            unique_references
                .get(reference.operation_label.as_str())
                .is_some_and(|unique| unique.id == reference.id)
        })
        .filter_map(|reference| {
            let offset_store_reference_count = offset_store_reference_counts
                .get(reference.id.as_str())
                .copied();
            let has_offset_store_reference = offset_store_reference_count.is_some();
            let is_offset_store_operation =
                offset_store_operations.contains(reference.operation_label.as_str());
            if is_offset_store_operation && !has_offset_store_reference {
                return None;
            }
            if has_offset_store_reference
                && (offset_store_reference_count != Some(1)
                    || !single_offset_store_operations.contains(reference.operation_label.as_str()))
            {
                return None;
            }
            let binding = if has_offset_store_reference {
                let data_block_use = data_block_uses
                    .iter()
                    .find(|use_| use_.feature_body_reference == reference.id)?;
                unique_offset_store_body_frame(reference, data_block_use, object_frames)?;
                crate::native::segments::unique_segment_body_alias_binding(
                    reference.body.value(),
                    bindings,
                )?
            } else {
                crate::native::segments::unique_segment_body_binding(
                    reference.body.value(),
                    bindings,
                )?
            };
            Some(FeatureBodySegmentUse {
                id: reference
                    .id
                    .replacen("body-reference", "body-segment-use", 1),
                feature_body_reference: reference.id.clone(),
                segment_body_binding: binding.id.clone(),
            })
        })
        .collect()
}

/// Return operations with at least one resolved input field in an offset store.
///
/// This set identifies operations whose body fields must be resolved in the
/// offset-store namespace. The one-store requirement for a segment bridge is
/// checked separately from this broader namespace classification.
pub(crate) fn feature_input_store_operations(
    inputs: &[FeatureInputBlock],
    blocks: &[crate::native::om::DataBlock],
) -> BTreeSet<String> {
    feature_input_store_sections(inputs, blocks)
        .into_iter()
        .filter_map(|(operation_label, sections)| (!sections.is_empty()).then_some(operation_label))
        .collect()
}

/// Group resolved operation-header inputs by their indexed offset-store section.
pub(crate) fn feature_input_store_sections(
    inputs: &[FeatureInputBlock],
    blocks: &[crate::native::om::DataBlock],
) -> BTreeMap<String, BTreeSet<u32>> {
    let blocks_by_id = blocks
        .iter()
        .map(|block| (block.id.as_str(), block))
        .collect::<BTreeMap<_, _>>();
    let mut sections_by_operation = BTreeMap::<String, BTreeSet<u32>>::new();
    for input in inputs {
        let Some(block) = blocks_by_id.get(input.data_block.as_str()) else {
            continue;
        };
        sections_by_operation
            .entry(input.operation_label.clone())
            .or_default()
            .insert(block.section_ordinal);
    }
    sections_by_operation
}

/// Resolve primary feature body fields in an unambiguous operation input store.
pub fn feature_body_data_block_uses(
    references: &[FeatureBodyReference],
    inputs: &[FeatureInputBlock],
    blocks: &[crate::native::om::DataBlock],
) -> Vec<FeatureBodyDataBlockUse> {
    let unique_references = unique_feature_body_references(references);
    let store_sections = feature_input_store_sections(inputs, blocks);
    references
        .iter()
        .filter_map(|reference| {
            if unique_references
                .get(reference.operation_label.as_str())
                .is_none_or(|unique| unique.id != reference.id)
            {
                return None;
            }
            let section_ordinals = store_sections
                .get(reference.operation_label.as_str())
                .map(|sections| sections.iter().copied().collect::<Vec<_>>())
                .unwrap_or_default();
            let [section_ordinal] = section_ordinals.as_slice() else {
                return None;
            };
            let matches = blocks
                .iter()
                .filter(|block| {
                    block.section_ordinal == *section_ordinal
                        && block.block_ordinal == reference.body.value()
                })
                .collect::<Vec<_>>();
            let [block] = matches.as_slice() else {
                return None;
            };
            Some(FeatureBodyDataBlockUse {
                id: reference
                    .id
                    .replacen("body-reference", "body-data-block-use", 1),
                feature_body_reference: reference.id.clone(),
                data_block: block.id.clone(),
            })
        })
        .collect()
}

/// Resolve operation-header object indices to unique offset-only data blocks.
pub fn feature_input_blocks(container: &Container) -> Vec<FeatureInputBlock> {
    let indexed = container.indexed_om_sections();
    let mut inputs = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let label = record.label();
            for (input_slot, object) in HeaderSlot::ALL.into_iter().zip(label.header.objects().0) {
                let Some(object) = object else {
                    continue;
                };
                let Some(data_block) = unique_offset_data_block(&indexed, object.value()) else {
                    continue;
                };
                let operation_label = format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                );
                inputs.push(FeatureInputBlock {
                    id: format!(
                        "nx:feature-history:input-block#{section_key}-{operation_ordinal:010}-{input_slot:010}"
                    ),
                    operation_label,
                    input_slot,
                    object,
                    data_block,
                    source_offset: entry_offset + label.header.object_offsets()[input_slot.index()] as u64,
                });
            }
        },
    );
    inputs
}

/// Group bindings from distinct operations by exact resolved data-block identity.
pub fn feature_input_block_identity_groups(
    inputs: &[FeatureInputBlock],
) -> Vec<FeatureInputBlockIdentityGroup> {
    let mut by_block = BTreeMap::<&str, Vec<&FeatureInputBlock>>::new();
    for input in inputs {
        by_block.entry(&input.data_block).or_default().push(input);
    }
    let mut groups = by_block
        .into_iter()
        .filter_map(|(data_block, mut members)| {
            if members
                .iter()
                .map(|member| member.operation_label.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                < 2
            {
                return None;
            }
            members.sort_by_key(|member| member.source_offset);
            Some((data_block, members))
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|(_, members)| members[0].source_offset);
    groups
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (data_block, members))| FeatureInputBlockIdentityGroup {
                id: format!("nx:feature-history:input-block-identity-group#{ordinal:010}"),
                data_block: data_block.to_string(),
                members: members
                    .into_iter()
                    .map(|member| FeatureInputBlockIdentityMember {
                        input_block: member.id.clone(),
                        operation_label: member.operation_label.clone(),
                        input_slot: member.input_slot,
                        source_offset: member.source_offset,
                    })
                    .collect(),
            },
        )
        .collect()
}

type ColumnTableByRow<'a> = BTreeMap<&'a str, Option<&'a str>>;
type ColumnSlotsByBlock<'a> =
    BTreeMap<&'a str, Vec<(&'a str, ColumnIndexRowKind, ColumnRowSlot, u64)>>;

fn column_relations_by_block<'a>(
    index_rows: &'a [DataBlockIndexRow],
    linked_rows: &'a [DataBlockLinkedIndexRow],
    target_rows: &'a [DataBlockTargetIndexRow],
    tables: &'a [DataBlockColumnIndexTable],
) -> (ColumnTableByRow<'a>, ColumnSlotsByBlock<'a>) {
    let mut table_by_row = ColumnTableByRow::new();
    for table in tables {
        for row in std::iter::once(table.opening_linked_row.as_str())
            .chain(table.rows.target_rows().iter().map(String::as_str))
            .chain(table.rows.linked_rows().iter().map(String::as_str))
        {
            table_by_row
                .entry(row)
                .and_modify(|value| *value = None)
                .or_insert(Some(table.id.as_str()));
        }
    }
    let mut slots_by_block = ColumnSlotsByBlock::new();
    for row in index_rows {
        for (slot, token) in ColumnRowSlot::ALL.into_iter().zip(row.frame.indices()) {
            slots_by_block.entry(token.target).or_default().push((
                row.id.as_str(),
                ColumnIndexRowKind::Index,
                slot,
                token.offset,
            ));
        }
    }
    for row in linked_rows {
        for (slot, token) in ColumnRowSlot::ALL
            .into_iter()
            .zip(std::iter::once(row.frame.target_index()).chain(row.frame.indices()))
        {
            slots_by_block.entry(token.target).or_default().push((
                row.id.as_str(),
                ColumnIndexRowKind::LinkedIndex,
                slot,
                token.offset,
            ));
        }
    }
    for row in target_rows {
        for (slot, token) in ColumnRowSlot::ALL
            .into_iter()
            .zip(std::iter::once(row.frame.target_index()).chain(row.frame.indices()))
        {
            slots_by_block.entry(token.target).or_default().push((
                row.id.as_str(),
                ColumnIndexRowKind::TargetIndex,
                slot,
                token.offset,
            ));
        }
    }
    (table_by_row, slots_by_block)
}

/// Join feature inputs to every column-row slot addressing the same block.
pub fn feature_input_column_row_uses(
    inputs: &[FeatureInputBlock],
    index_rows: &[DataBlockIndexRow],
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
    tables: &[DataBlockColumnIndexTable],
) -> Vec<FeatureInputColumnRowUse> {
    let (table_by_row, slots_by_block) =
        column_relations_by_block(index_rows, linked_rows, target_rows, tables);
    inputs
        .iter()
        .flat_map(|input| {
            slots_by_block
                .get(input.data_block.as_str())
                .into_iter()
                .flatten()
                .enumerate()
                .map(
                    |(ordinal, (row, row_kind, slot, source_offset))| FeatureInputColumnRowUse {
                        id: format!(
                            "nx:feature-history:input-column-row-use#{}-{}-{ordinal:010}",
                            input.id.rsplit_once('#').map_or("unknown", |(_, key)| key),
                            row_kind.id_component(),
                        ),
                        input_block: input.id.clone(),
                        operation_label: input.operation_label.clone(),
                        input_slot: input.input_slot,
                        row_kind: *row_kind,
                        column_row: (*row).to_string(),
                        column_table: table_by_row
                            .get(row)
                            .and_then(|table| *table)
                            .map(str::to_string),
                        row_slot: *slot,
                        data_block: input.data_block.clone(),
                        source_offset: *source_offset,
                    },
                )
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Join every datum-CSYS construction lane to column-row slots addressing the
/// same block. The relation assigns no geometric role to either lane.
pub fn feature_datum_csys_column_row_uses(
    constructions: &[FeatureDatumCsysConstruction],
    index_rows: &[DataBlockIndexRow],
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
    tables: &[DataBlockColumnIndexTable],
) -> Vec<FeatureDatumCsysColumnRowUse> {
    let (table_by_row, slots_by_block) =
        column_relations_by_block(index_rows, linked_rows, target_rows, tables);
    let table_by_row = &table_by_row;
    let slots_by_block = &slots_by_block;
    constructions
        .iter()
        .flat_map(|construction| {
            DatumCsysSlot::ALL.into_iter().zip(construction.frame.references())
                .flat_map(|(construction_slot, (_, data_block, source_offset))| {
                    slots_by_block
                        .get(data_block.as_str())
                        .into_iter()
                        .flatten()
                        .enumerate()
                        .map(move |(ordinal, (row, row_kind, row_slot, row_source_offset))| {
                            FeatureDatumCsysColumnRowUse {
                                id: format!(
                                    "nx:feature-history:datum-csys-column-row-use#{}-{construction_slot:010}-{}-{ordinal:010}",
                                    construction
                                        .id
                                        .rsplit_once('#')
                                        .map_or("unknown", |(_, key)| key),
                                    row_kind.id_component(),
                                ),
                                construction: construction.id.clone(),
                                operation_label: construction.operation_label.clone(),
                                construction_slot,
                                row_kind: *row_kind,
                                column_row: (*row).to_string(),
                                column_table: table_by_row
                                    .get(row)
                                    .and_then(|table| *table)
                                    .map(str::to_string),
                                row_slot: *row_slot,
                                data_block: data_block.clone(),
                                construction_source_offset: source_offset,
                                row_source_offset: *row_source_offset,
                            }
                        })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Retain inputs having exactly one slot-zero use in one complete column table.
pub fn feature_input_column_targets(
    inputs: &[FeatureInputBlock],
    uses: &[FeatureInputColumnRowUse],
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
) -> Vec<FeatureInputColumnTarget> {
    inputs
        .iter()
        .filter_map(|input| {
            let targets = uses
                .iter()
                .filter(|use_| {
                    use_.input_block == input.id
                        && use_.row_slot == ColumnRowSlot::Zero
                        && use_.row_kind != ColumnIndexRowKind::Index
                })
                .filter_map(|use_| Some((use_, use_.column_table.as_ref()?)))
                .collect::<Vec<_>>();
            let [(target, column_table)] = targets.as_slice() else {
                return None;
            };
            let (row, field_indices, field_data_blocks, field_source_offsets, mode) =
                match target.row_kind {
                    ColumnIndexRowKind::LinkedIndex => {
                        let rows = linked_rows
                            .iter()
                            .filter(|row| row.id == target.column_row)
                            .collect::<Vec<_>>();
                        let [row] = rows.as_slice() else {
                            return None;
                        };
                        (
                            FeatureInputColumnTargetRow::Linked {
                                leading_index: row.frame.first_index().atom.value(),
                                leading_index_source_offset: row.frame.first_index().offset,
                                discriminator: row.frame.discriminator(),
                                flag: row.frame.flag(),
                            },
                            row.frame.indices().map(|token| token.atom.value()),
                            row.frame.indices().map(|token| token.target.clone()),
                            row.frame.indices().map(|token| token.offset),
                            row.frame.mode(),
                        )
                    }
                    ColumnIndexRowKind::TargetIndex => {
                        let rows = target_rows
                            .iter()
                            .filter(|row| row.id == target.column_row)
                            .collect::<Vec<_>>();
                        let [row] = rows.as_slice() else {
                            return None;
                        };
                        (
                            FeatureInputColumnTargetRow::Target,
                            row.frame.indices().map(|token| token.atom.value()),
                            row.frame.indices().map(|token| token.target.clone()),
                            row.frame.indices().map(|token| token.offset),
                            row.frame.mode(),
                        )
                    }
                    ColumnIndexRowKind::Index => return None,
                };
            Some(FeatureInputColumnTarget {
                id: format!(
                    "nx:feature-history:input-column-target#{}",
                    input.id.rsplit_once('#').map_or("unknown", |(_, key)| key)
                ),
                input_block: input.id.clone(),
                operation_label: input.operation_label.clone(),
                input_slot: input.input_slot,
                column_row: target.column_row.clone(),
                row,
                field_indices,
                field_data_blocks,
                field_source_offsets,
                mode,
                column_table: (*column_table).clone(),
                data_block: input.data_block.clone(),
                source_offset: target.source_offset,
            })
        })
        .collect()
}

/// Decode and atomically resolve datum coordinate-system construction lanes
/// through the offset store selected by each operation header.
pub fn feature_datum_csys_constructions(
    container: &Container,
) -> Vec<FeatureDatumCsysConstruction> {
    let indexed = container.indexed_om_sections();
    let inputs = feature_input_blocks(container);
    let mut constructions = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(field) = crate::om::datum_csys::datum_csys_references(record.payload_view())
            else {
                return;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let input_prefixes = inputs
                .iter()
                .filter(|input| input.operation_label == operation_label)
                .filter_map(|input| {
                    input
                        .data_block
                        .rsplit_once(":block#")
                        .map(|(prefix, _)| prefix)
                })
                .collect::<BTreeSet<_>>();
            let mut input_prefixes = input_prefixes.into_iter();
            let (Some(input_prefix), None) = (input_prefixes.next(), input_prefixes.next()) else {
                return;
            };
            let Some(frame) = field.relocate(entry_offset).and_then(|field| {
                field.resolve(|index| {
                    let data_block = unique_offset_data_block(&indexed, index)?;
                    (data_block.rsplit_once(":block#")?.0 == input_prefix).then_some(data_block)
                })
            }) else {
                return;
            };
            constructions.push(FeatureDatumCsysConstruction {
                id: format!(
                    "nx:feature-history:datum-csys-construction#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                frame,
            });
        },
    );
    constructions
}

/// Reconstruct datum-plane object payloads across ordered store blocks.
pub fn feature_datum_plane_payloads(
    container: &Container,
    headers: &[FeatureDatumPlaneHeader],
) -> Vec<FeatureDatumPlanePayload> {
    let blocks = offset_data_block_bytes(container);
    headers
        .iter()
        .filter(|header| {
            header
                .resolved_data_blocks(DatumPlaneBlockLane::Object)
                .next()
                .is_some()
        })
        .filter_map(|header| {
            let data_blocks = header
                .resolved_data_blocks(DatumPlaneBlockLane::Object)
                .cloned()
                .collect::<Vec<_>>();
            let (payload, content) = FeaturePayloadContent::from_source(data_blocks, &blocks)?;
            let lanes = crate::om::datum_index::scan(&payload);
            let lane = <[_; 1]>::try_from(lanes).ok().map(|[lane]| lane);
            let key = header.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
            Some(FeatureDatumPlanePayload {
                id: format!("nx:feature-history:datum-plane-payload#{key}"),
                operation_label: header.operation_label.clone(),
                datum_plane_header: header.id.clone(),
                content,
                index_lane: lane.map(crate::om::datum_index::DatumIndexLane::into_u64),
            })
        })
        .collect()
}

/// Reconstruct the two leading object blocks of each datum coordinate system.
pub fn feature_datum_csys_payloads(
    container: &Container,
    constructions: &[FeatureDatumCsysConstruction],
) -> Vec<FeatureDatumCsysPayload> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .filter_map(|construction| {
            let data_blocks = [
                construction.frame.members()[0].1.clone(),
                construction.frame.members()[1].1.clone(),
            ];
            let (_, content) = FeaturePayloadContent::from_source(data_blocks, &blocks)?;
            Some(FeatureDatumCsysPayload {
                id: construction
                    .id
                    .replacen("datum-csys-construction", "datum-csys-payload", 1),
                operation_label: construction.operation_label.clone(),
                construction: construction.id.clone(),
                content,
            })
        })
        .collect()
}

/// Shared body for construction-payload frame extractors. Reconstruct each
/// payload's concatenated bytes, build the payload-relative-to-source-offset
/// mapper once, scan the bytes, and let each family build its record, dropping
/// frames whose offsets fall outside a source block. Extractors differ only in
/// their payload block lane, scanner, and output record.
fn construction_payload_frames<P, S, R>(
    container: &Container,
    payloads: &[P],
    data_blocks: impl Fn(&P) -> &[FeaturePayloadBlock],
    scan: impl Fn(&[u8]) -> Vec<S>,
    build: impl Fn(&P, usize, S, &dyn Fn(usize) -> Option<u64>) -> Option<R>,
) -> Vec<R> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) = JoinedPayload::from_source(
                data_blocks(payload).iter().map(|block| &block.id),
                &blocks,
            ) else {
                return Vec::new();
            };
            let source_offset = |relative: usize| joined.source_offset(relative as u64);
            scan(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, row)| build(payload, ordinal, row, &source_offset))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode exact scalar-pair frames from reconstructed datum-CSYS payloads.
pub fn feature_datum_csys_payload_scalar_pairs(
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Vec<FeaturePayloadScalarPair> {
    construction_payload_frames(
        container,
        payloads,
        |payload| payload.content.blocks(),
        crate::om::binary64_pair::object_pairs,
        |payload, ordinal, pair, source_offset| {
            Some(FeaturePayloadScalarPair {
                id: format!("{}-scalar-pair-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                payload: FeatureScalarPairPayload::DatumCsys {
                    datum_csys_payload: payload.id.clone(),
                    frame: pair.into_wire_frame()?,
                },
                ordinal: ordinal as u32,
                value_source_offsets: [
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                ],
                source_offset: source_offset(pair.offset())?,
            })
        },
    )
}

/// Decode complete signed Q1.55 pair frames from reconstructed datum-CSYS payloads.
pub fn feature_datum_csys_payload_fixed_pairs(
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Vec<FeatureDatumCsysPayloadFixedPair> {
    construction_payload_frames(
        container,
        payloads,
        |payload| payload.content.blocks(),
        crate::om::datum_csys_payload_fixed_pairs,
        |payload, ordinal, pair, source_offset| {
            Some(FeatureDatumCsysPayloadFixedPair {
                id: format!("{}-fixed-pair-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                datum_csys_payload: payload.id.clone(),
                ordinal: ordinal as u32,
                values: pair.values,
                position: PairPosition::new(pair.form, pair.offset as u64)?,
                source_offset: source_offset(pair.offset)?,
                value_source_offsets: [
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                ],
            })
        },
    )
}

/// Decode complete shifted-binary64 fields from reconstructed datum-CSYS payloads.
pub fn feature_datum_csys_payload_scalars(
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Vec<FeaturePayloadScalar> {
    construction_payload_frames(
        container,
        payloads,
        |payload| payload.content.blocks(),
        crate::om::construction_payload_scalar_fields,
        |payload, ordinal, scalar, source_offset| {
            Some(FeaturePayloadScalar {
                id: format!("{}-scalar-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                payload: FeatureScalarPayload::DatumCsys {
                    datum_csys_payload: payload.id.clone(),
                },
                ordinal: ordinal as u32,
                field_code: scalar.field_code,
                scalar: scalar.scalar,
                payload_offset: scalar.offset as u64,
                source_offset: source_offset(scalar.offset)?,
            })
        },
    )
}

/// Decode the final three descriptor lanes of datum coordinate systems.
pub fn feature_datum_csys_descriptors(
    container: &Container,
    constructions: &[FeatureDatumCsysConstruction],
) -> Vec<FeatureDatumCsysDescriptor> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .flat_map(|construction| {
            [
                CsysDescriptorSlot::Five,
                CsysDescriptorSlot::Six,
                CsysDescriptorSlot::Seven,
            ]
            .into_iter()
            .filter_map(|slot| {
                let reference_ordinal = u8::from(slot);
                let data_block = &construction.frame.members()[usize::from(reference_ordinal)].1;
                let &(bytes, source_offset) = blocks.get(data_block)?;
                let descriptor = crate::om::datum_csys_descriptor_block(bytes)?;
                Some(FeatureDatumCsysDescriptor {
                    id: format!("{}-descriptor-{reference_ordinal}", construction.id),
                    operation_label: construction.operation_label.clone(),
                    construction: construction.id.clone(),
                    reference_ordinal: slot,
                    data_block: data_block.clone(),
                    descriptor: LocatedCsysDescriptor::new(descriptor, source_offset).ok()?,
                })
            })
            .collect::<Vec<_>>()
        })
        .collect()
}

/// Join equal typed descriptor identities across datum-plane and datum-CSYS history.
pub fn feature_datum_plane_csys_identity_uses(
    plane_descriptors: &[FeatureDatumPlaneDescriptor],
    csys_descriptors: &[FeatureDatumCsysDescriptor],
) -> Vec<FeatureDatumPlaneCsysIdentityUse> {
    plane_descriptors
        .iter()
        .flat_map(|plane| {
            csys_descriptors
                .iter()
                .filter(|csys| csys.descriptor.descriptor().identity().as_str() == plane.descriptor.identity())
                .map(|csys| {
                    let plane_key = plane.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
                    let csys_key = csys.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
                    FeatureDatumPlaneCsysIdentityUse {
                        id: format!(
                            "nx:feature-history:datum-plane-csys-identity-use#{plane_key}-{csys_key}"
                        ),
                        identity: csys.descriptor.descriptor().identity().clone(),
                        datum_plane_descriptor: plane.id.clone(),
                        datum_plane_operation_label: plane.operation_label.clone(),
                        datum_csys_descriptor: csys.id.clone(),
                        datum_csys_operation_label: csys.operation_label.clone(),
                        datum_csys_reference_ordinal: csys.reference_ordinal,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode exact scalar-pair frames from reconstructed datum-plane payloads.
pub fn feature_datum_plane_payload_scalar_pairs(
    container: &Container,
    payloads: &[FeatureDatumPlanePayload],
) -> Vec<FeaturePayloadScalarPair> {
    construction_payload_frames(
        container,
        payloads,
        |payload| payload.content.blocks(),
        crate::om::binary64_pair::datum_plane_pairs,
        |payload, ordinal, pair, source_offset| {
            Some(FeaturePayloadScalarPair {
                id: format!("{}-scalar-pair-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                payload: FeatureScalarPairPayload::DatumPlane {
                    datum_plane_payload: payload.id.clone(),
                    frame: pair.into_wire_frame()?,
                },
                ordinal: ordinal as u32,
                value_source_offsets: [
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                ],
                source_offset: source_offset(pair.offset())?,
            })
        },
    )
}

/// Decode atomically resolved datum-plane descriptor blocks.
pub fn feature_datum_plane_descriptors(
    container: &Container,
    headers: &[FeatureDatumPlaneHeader],
) -> Vec<FeatureDatumPlaneDescriptor> {
    let blocks = offset_data_block_bytes(container);
    headers
        .iter()
        .flat_map(|header| {
            header
                .resolved_data_blocks(DatumPlaneBlockLane::Descriptor)
                .enumerate()
                .filter_map(|(ordinal, data_block)| {
                    let (bytes, source_offset) = blocks.get(data_block)?.to_owned();
                    let descriptor = crate::om::datum_plane_descriptor_block(bytes)?;
                    Some(FeatureDatumPlaneDescriptor {
                        id: format!("{}-descriptor-{ordinal:010}", header.id),
                        operation_label: header.operation_label.clone(),
                        datum_plane_header: header.id.clone(),
                        ordinal: ordinal as u32,
                        data_block: data_block.clone(),
                        descriptor,
                        source_offset,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Join resolved datum-plane blocks to operation inputs addressing the same block.
pub fn feature_datum_plane_block_uses(
    headers: &[FeatureDatumPlaneHeader],
    inputs: &[FeatureInputBlock],
) -> Vec<FeatureDatumPlaneBlockUse> {
    let mut uses = Vec::new();
    for header in headers {
        let construction_key = header
            .operation_label
            .rsplit_once('#')
            .map_or(header.operation_label.as_str(), |(_, key)| key);
        for lane in [DatumPlaneBlockLane::Descriptor, DatumPlaneBlockLane::Object] {
            for (reference_ordinal, data_block) in header.resolved_data_blocks(lane).enumerate() {
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
                    uses.push(FeatureDatumPlaneBlockUse {
                        id: format!(
                            "nx:feature-history:datum-plane-block-use#{construction_key}-{lane_key}-{reference_ordinal}-{input_key}-{}",
                            input.input_slot
                        ),
                        datum_plane_header: header.id.clone(),
                        construction_operation_label: header.operation_label.clone(),
                        lane,
                        reference_ordinal: reference_ordinal as u32,
                        data_block: data_block.clone(),
                        input_binding: input.id.clone(),
                        input_operation_label: input.operation_label.clone(),
                        input_slot: input.input_slot,
                    });
                }
            }
        }
    }
    uses
}

/// Join resolved datum-coordinate-system blocks to every exact operation input
/// addressing the same native block.
pub fn feature_datum_csys_block_uses(
    constructions: &[FeatureDatumCsysConstruction],
    inputs: &[FeatureInputBlock],
) -> Vec<FeatureDatumCsysBlockUse> {
    let mut uses = Vec::new();
    for construction in constructions {
        for (reference_ordinal, reference) in DatumCsysSlot::ALL
            .into_iter()
            .zip(construction.frame.members())
        {
            let data_block = &reference.1;
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
                uses.push(FeatureDatumCsysBlockUse {
                    id: format!(
                        "nx:feature-history:datum-csys-block-use#{construction_key}-{reference_ordinal}-{input_key}-{}",
                        input.input_slot
                    ),
                    construction: construction.id.clone(),
                    construction_operation_label: construction.operation_label.clone(),
                    reference_ordinal,
                    data_block: data_block.clone(),
                    input_binding: input.id.clone(),
                    input_operation_label: input.operation_label.clone(),
                    input_slot: input.input_slot,
                });
            }
        }
    }
    uses
}

/// Join each sketch operation to its bounded record and ordered input blocks.
pub fn feature_sketch_records(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    inputs: &[FeatureInputBlock],
    references: &[FeatureSketchReference],
) -> Vec<FeatureSketchRecord> {
    labels
        .iter()
        .filter(|label| label.value == "SKETCH")
        .filter_map(|label| {
            let mut operation_records = records
                .iter()
                .filter(|record| record.operation_label == label.id);
            let record = operation_records.next()?;
            if operation_records.next().is_some() {
                return None;
            }
            let mut input_blocks = inputs
                .iter()
                .filter(|input| input.operation_label == label.id)
                .collect::<Vec<_>>();
            input_blocks.sort_by_key(|input| input.input_slot);
            let mut payload_references = references
                .iter()
                .filter(|reference| reference.operation_label == label.id)
                .collect::<Vec<_>>();
            payload_references.sort_by_key(|reference| reference.position.ordinal());
            Some(FeatureSketchRecord {
                id: label.id.replacen("operation-label", "sketch-record", 1),
                operation_label: label.id.clone(),
                ordinal: label.ordinal,
                operation_record: record.id.clone(),
                input_blocks: input_blocks
                    .into_iter()
                    .map(|input| input.id.clone())
                    .collect(),
                payload_references: payload_references
                    .into_iter()
                    .map(|reference| reference.id.clone())
                    .collect(),
                source_offset: label.source_offset,
            })
        })
        .collect()
}

/// Join complete, uniquely resolved sketch construction-reference fields.
pub fn feature_sketch_construction_inputs(
    sketches: &[FeatureSketchRecord],
    references: &[FeatureSketchReference],
) -> Vec<FeatureSketchConstructionInputs> {
    let mut inputs = Vec::new();
    for sketch in sketches {
        let mut field = references
            .iter()
            .filter(|reference| reference.operation_label == sketch.operation_label)
            .collect::<Vec<_>>();
        field.sort_by_key(|reference| reference.position.ordinal());
        let Some((terminal, members)) = field.split_last() else {
            continue;
        };
        let expected_len = usize::from(terminal.position.declared_count().max(1));
        if field.len() != expected_len
            || field.iter().enumerate().any(|(ordinal, reference)| {
                reference.position.declared_count() != terminal.position.declared_count()
                    || reference.position.ordinal() != ordinal as u32
            })
        {
            continue;
        }
        let Some(members) = members
            .iter()
            .map(|reference| {
                Some(FeatureConstructionMember {
                    reference: reference.id.clone(),
                    data_block: reference.data_block.clone()?,
                })
            })
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(terminal_data_block) = terminal.data_block.clone() else {
            continue;
        };
        inputs.push(FeatureSketchConstructionInputs {
            id: sketch
                .id
                .replacen("sketch-record", "sketch-construction-inputs", 1),
            operation_label: sketch.operation_label.clone(),
            sketch_record: sketch.id.clone(),
            members,
            terminal_reference: terminal.id.clone(),
            terminal_data_block,
        });
    }
    inputs
}

/// Reconstruct exact sketch payloads across offset-store block boundaries.
pub fn feature_sketch_construction_payloads(
    container: &Container,
    constructions: &[FeatureSketchConstructionInputs],
) -> Vec<FeatureConstructionPayload> {
    let blocks = offset_data_block_bytes(container);

    constructions
        .iter()
        .filter_map(|construction| {
            let mut data_blocks = construction
                .members
                .iter()
                .map(|member| member.data_block.clone())
                .collect::<Vec<_>>();
            data_blocks.push(construction.terminal_data_block.clone());
            let (_, content) = FeaturePayloadContent::from_source(data_blocks, &blocks)?;
            Some(FeatureConstructionPayload {
                id: construction.id.replacen(
                    "sketch-construction-inputs",
                    "sketch-construction-payload",
                    1,
                ),
                operation_label: construction.operation_label.clone(),
                owner: FeatureConstructionOwner::Sketch {
                    construction_inputs: construction.id.clone(),
                },
                content,
            })
        })
        .collect()
}

/// Decode exact coordinate-pair frames from reconstructed sketch payloads.
pub fn feature_sketch_payload_coordinate_pairs(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Vec<FeaturePayloadScalarPair> {
    construction_payload_frames(
        container,
        payloads,
        |payload| payload.content.blocks(),
        crate::om::binary64_pair::sketch_pairs,
        |payload, ordinal, pair, source_offset| {
            Some(FeaturePayloadScalarPair {
                id: format!("{}-coordinate-pair-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                payload: FeatureScalarPairPayload::Construction {
                    construction_payload: payload.id.clone(),
                    frame: pair.into_wire_frame()?,
                },
                ordinal: ordinal as u32,
                value_source_offsets: [
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                ],
                source_offset: source_offset(pair.offset())?,
            })
        },
    )
}

/// Decode exact scaled shifted-binary64 pair frames from reconstructed sketch payloads.
pub fn feature_sketch_payload_fixed_pairs(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Vec<FeatureSketchPayloadFixedPair> {
    construction_payload_frames(
        container,
        payloads,
        |payload| payload.content.blocks(),
        crate::om::sketch_payload_fixed_pairs,
        |payload, ordinal, pair, source_offset| {
            Some(FeatureSketchPayloadFixedPair {
                id: format!("{}-fixed-pair-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                construction_payload: payload.id.clone(),
                ordinal: ordinal as u32,
                values: pair.values,
                position: PairPosition::new(pair.form, pair.offset as u64)?,
                source_offset: source_offset(pair.offset)?,
                value_source_offsets: [
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                ],
            })
        },
    )
}

/// Decode exact mixed scaled shifted-binary64/binary32 pair frames from reconstructed sketch payloads.
pub fn feature_sketch_payload_mixed_pairs(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Vec<FeatureSketchPayloadMixedPair> {
    construction_payload_frames(
        container,
        payloads,
        |payload| payload.content.blocks(),
        crate::om::sketch_payload_mixed_pairs,
        |payload, ordinal, pair, source_offset| {
            Some(FeatureSketchPayloadMixedPair {
                id: format!("{}-mixed-pair-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                construction_payload: payload.id.clone(),
                ordinal: ordinal as u32,
                scalars: pair.scalars,
                position: PairPosition::new(MixedPairForm, pair.offset as u64)?,
                source_offset: source_offset(pair.offset)?,
                value_source_offsets: [
                    source_offset(pair.value_offsets()[0])?,
                    source_offset(pair.value_offsets()[1])?,
                ],
            })
        },
    )
}

pub(crate) fn offset_data_block_bytes_for_section<'a>(
    section_ordinal: usize,
    entry_offset: u64,
    control: &crate::om::EntityRecord<'a>,
    records: &[crate::om::EntityRecord<'a>],
) -> BTreeMap<String, (&'a [u8], u64)> {
    let mut blocks = BTreeMap::new();
    blocks.insert(
        format!("nx:om-data-blocks-{section_ordinal}:block#0"),
        (control.bytes, entry_offset + control.offset as u64),
    );
    for (record_ordinal, block) in records.iter().enumerate() {
        blocks.insert(
            format!(
                "nx:om-data-blocks-{section_ordinal}:block#{}",
                record_ordinal + 1
            ),
            (block.bytes, entry_offset + block.offset as u64),
        );
    }
    blocks
}

fn offset_data_block_bytes<'a>(
    container: &'a Container<'_>,
) -> Cow<'a, BTreeMap<String, (&'a [u8], u64)>> {
    if let Some(blocks) = container.cached_offset_data_block_bytes() {
        return Cow::Borrowed(blocks);
    }
    let indexed = container.indexed_om_sections();
    if let Some(blocks) = container.cached_offset_data_block_bytes() {
        return Cow::Borrowed(blocks);
    }
    let mut blocks = BTreeMap::new();
    for (section_ordinal, (entry, section)) in indexed.into_iter().enumerate() {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        blocks.extend(offset_data_block_bytes_for_section(
            section_ordinal,
            entry_offset,
            control,
            records,
        ));
    }
    Cow::Owned(blocks)
}

/// Decode exact framed scalar fields across reconstructed sketch payloads.
pub fn feature_sketch_payload_scalars(
    container: &Container,
    constructions: &[FeatureSketchConstructionInputs],
) -> Vec<FeaturePayloadScalar> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .filter_map(|construction| {
            let mut data_blocks = construction
                .members
                .iter()
                .map(|member| member.data_block.clone())
                .collect::<Vec<_>>();
            data_blocks.push(construction.terminal_data_block.clone());
            let joined = JoinedPayload::from_source(data_blocks.iter(), &blocks)?;
            let construction_payload = construction.id.replacen(
                "sketch-construction-inputs",
                "sketch-construction-payload",
                1,
            );
            Some(
                crate::om::construction_payload_scalar_fields(joined.bytes())
                    .into_iter()
                    .enumerate()
                    .filter_map(|(ordinal, field)| {
                        let source_offset = joined.source_offset(field.offset as u64)?;
                        Some(FeaturePayloadScalar {
                            id: format!(
                                "nx:feature-history:sketch-payload-scalar#{}-{ordinal:010}",
                                construction_payload
                                    .rsplit_once('#')
                                    .map_or("unknown", |(_, key)| key)
                            ),
                            operation_label: construction.operation_label.clone(),
                            payload: FeatureScalarPayload::Construction {
                                construction_payload: construction_payload.clone(),
                            },
                            ordinal: ordinal as u32,
                            field_code: field.field_code,
                            scalar: field.scalar,
                            payload_offset: field.offset as u64,
                            source_offset,
                        })
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect()
}

/// Decode exact scalar-vector frames across reconstructed sketch payloads.
pub fn feature_sketch_payload_scalar_lanes(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Vec<FeatureSketchPayloadScalarLane> {
    construction_payload_frames(
        container,
        payloads,
        |payload| payload.content.blocks(),
        crate::om::sketch_payload_scalar_lanes,
        |payload, ordinal, lane, source_offset| {
            let header_source = source_offset(lane.offset() as usize)?;
            let terminator_source = source_offset(lane.end() as usize)?;
            let lane = lane.try_map_locations(|offset, ()| source_offset(offset as usize))?;
            Some(FeatureSketchPayloadScalarLane {
                id: format!("{}-scalar-lane-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                construction_payload: payload.id.clone(),
                ordinal: ordinal as u32,
                lane,
                source_offset: header_source,
                terminator_source_offset: terminator_source,
            })
        },
    )
}

/// Decode exact compact-code name fields across reconstructed sketch payloads.
pub fn feature_sketch_payload_names(
    container: &Container,
    constructions: &[FeatureSketchConstructionInputs],
) -> Vec<FeaturePayloadName> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .flat_map(|construction| {
            let mut data_blocks = construction
                .members
                .iter()
                .map(|member| member.data_block.clone())
                .collect::<Vec<_>>();
            data_blocks.push(construction.terminal_data_block.clone());
            let Some(joined) = JoinedPayload::from_source(data_blocks.iter(), &blocks) else {
                return Vec::new();
            };
            let construction_payload = construction.id.replacen(
                "sketch-construction-inputs",
                "sketch-construction-payload",
                1,
            );
            crate::om::name_field::scan(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, field)| {
                    let relative = field.offset() as u64;
                    let source_offset = joined.source_offset(relative)?;
                    Some(FeaturePayloadName {
                        id: format!(
                            "nx:feature-history:sketch-payload-name#{}-{ordinal:010}",
                            construction_payload
                                .rsplit_once('#')
                                .map_or("unknown", |(_, key)| key)
                        ),
                        operation_label: construction.operation_label.clone(),
                        construction_payload: construction_payload.clone(),
                        ordinal: ordinal as u32,
                        frame: field.into_native(|offset| joined.source_offset(offset))?,
                        source_offset,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Join complete name-delimited intervals to their framed scalar fields.
pub fn feature_sketch_payload_named_records(
    payloads: &[FeatureConstructionPayload],
    names: &[FeaturePayloadName],
    scalars: &[FeaturePayloadScalar],
    fixed_pairs: &[FeatureSketchPayloadFixedPair],
    mixed_pairs: &[FeatureSketchPayloadMixedPair],
) -> Vec<FeatureSketchPayloadNamedRecord> {
    let mut records = Vec::new();
    for payload in payloads {
        let mut payload_names = names
            .iter()
            .filter(|name| name.construction_payload == payload.id)
            .collect::<Vec<_>>();
        payload_names.sort_by_key(|name| name.frame.offset());
        for (ordinal, name) in payload_names.iter().enumerate() {
            let end = payload_names
                .get(ordinal + 1)
                .map_or(payload.content.byte_len(), |next| next.frame.offset());
            let mut scalar_fields = scalars
                .iter()
                .filter(|scalar| {
                    scalar.payload.id() == payload.id
                        && scalar.payload_offset > name.frame.offset()
                        && scalar.payload_offset < end
                })
                .collect::<Vec<_>>();
            scalar_fields.sort_by_key(|scalar| scalar.payload_offset);
            let mut record_fixed_pairs = fixed_pairs
                .iter()
                .filter(|pair| {
                    pair.construction_payload == payload.id
                        && pair.position.offset() > name.frame.offset()
                        && pair.position.offset() < end
                })
                .collect::<Vec<_>>();
            record_fixed_pairs.sort_by_key(|pair| pair.position.offset());
            let mut record_mixed_pairs = mixed_pairs
                .iter()
                .filter(|pair| {
                    pair.construction_payload == payload.id
                        && pair.position.offset() > name.frame.offset()
                        && pair.position.offset() < end
                })
                .collect::<Vec<_>>();
            record_mixed_pairs.sort_by_key(|pair| pair.position.offset());
            records.push(FeatureSketchPayloadNamedRecord {
                id: format!(
                    "nx:feature-history:sketch-payload-record#{}-{ordinal:010}",
                    payload
                        .id
                        .rsplit_once('#')
                        .map_or("unknown", |(_, key)| key)
                ),
                operation_label: payload.operation_label.clone(),
                construction_payload: payload.id.clone(),
                name_field: name.id.clone(),
                scalar_fields: scalar_fields
                    .into_iter()
                    .map(|scalar| scalar.id.clone())
                    .collect(),
                fixed_pairs: record_fixed_pairs
                    .into_iter()
                    .map(|pair| pair.id.clone())
                    .collect(),
                mixed_pairs: record_mixed_pairs
                    .into_iter()
                    .map(|pair| pair.id.clone())
                    .collect(),
                payload_start_offset: name.frame.offset(),
                payload_end_offset: end,
            });
        }
    }
    records
}

/// Decode complete `Point<decimal>` records with exactly two scalar fields.
pub fn feature_sketch_points(
    records: &[FeatureSketchPayloadNamedRecord],
    names: &[FeaturePayloadName],
    scalars: &[FeaturePayloadScalar],
) -> Vec<FeatureSketchPoint> {
    let names = names
        .iter()
        .map(|name| (name.id.as_str(), name))
        .collect::<BTreeMap<_, _>>();
    let scalars = scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<BTreeMap<_, _>>();
    records
        .iter()
        .filter_map(|record| {
            let name = names.get(record.name_field.as_str())?;
            if name.operation_label != record.operation_label
                || name.construction_payload != record.construction_payload
            {
                return None;
            }
            parse_sketch_point_name(name.frame.value())?;
            let [first_id, second_id] = record.scalar_fields.as_slice() else {
                return None;
            };
            let first = scalars.get(first_id.as_str())?;
            let second = scalars.get(second_id.as_str())?;
            if [first, second].into_iter().any(|scalar| {
                scalar.operation_label != record.operation_label
                    || scalar.payload.id() != record.construction_payload
            }) {
                return None;
            }
            Some(FeatureSketchPoint {
                id: format!(
                    "nx:feature-history:sketch-point#{}",
                    record.id.rsplit_once('#').map_or("unknown", |(_, key)| key)
                ),
                operation_label: record.operation_label.clone(),
                named_record: record.id.clone(),
                name: name.frame.value().to_owned(),
                scalar_fields: [first.id.clone(), second.id.clone()],
                coordinates: [first.scalar.value(), second.scalar.value()],
            })
        })
        .collect()
}

/// Decode `Point<positive decimal>` records containing exactly one fixed pair.
pub fn feature_sketch_fixed_points(
    records: &[FeatureSketchPayloadNamedRecord],
    names: &[FeaturePayloadName],
    fixed_pairs: &[FeatureSketchPayloadFixedPair],
) -> Vec<FeatureSketchFixedPoint> {
    let names = names
        .iter()
        .map(|name| (name.id.as_str(), name))
        .collect::<BTreeMap<_, _>>();
    let fixed_pairs = fixed_pairs
        .iter()
        .map(|pair| (pair.id.as_str(), pair))
        .collect::<BTreeMap<_, _>>();
    records
        .iter()
        .filter_map(|record| {
            if !record.scalar_fields.is_empty() || !record.mixed_pairs.is_empty() {
                return None;
            }
            let point_pairs = record
                .fixed_pairs
                .iter()
                .filter(|fixed_pair_id| {
                    fixed_pairs.get(fixed_pair_id.as_str()).is_some_and(|pair| {
                        matches!(
                            pair.position.form(),
                            SketchPairForm::Legacy
                                | SketchPairForm::Short
                                | SketchPairForm::Extended
                        )
                    })
                })
                .collect::<Vec<_>>();
            let [fixed_pair_id] = point_pairs.as_slice() else {
                return None;
            };
            let name = names.get(record.name_field.as_str())?;
            if name.operation_label != record.operation_label
                || name.construction_payload != record.construction_payload
            {
                return None;
            }
            parse_sketch_point_name(name.frame.value())?;
            let pair = fixed_pairs.get(fixed_pair_id.as_str())?;
            if pair.operation_label != record.operation_label
                || pair.construction_payload != record.construction_payload
            {
                return None;
            }
            Some(FeatureSketchFixedPoint {
                id: record
                    .id
                    .replacen("sketch-payload-record", "sketch-fixed-point", 1),
                operation_label: record.operation_label.clone(),
                named_record: record.id.clone(),
                name: name.frame.value().to_owned(),
                fixed_pair: pair.id.clone(),
                values: pair.values.map(SketchScaledAtom::value),
                source_offset: pair.source_offset,
            })
        })
        .collect()
}

/// Group every bit-identical same-name sketch-point witness.
pub fn feature_sketch_point_groups(points: &[FeatureSketchPoint]) -> Vec<FeatureSketchPointGroup> {
    let mut grouped = BTreeSet::new();
    let mut groups = Vec::new();
    for point in points {
        let key = (point.operation_label.as_str(), point.name.as_str());
        if !grouped.insert(key) {
            continue;
        }
        let witnesses = points
            .iter()
            .filter(|candidate| {
                candidate.operation_label == point.operation_label && candidate.name == point.name
            })
            .collect::<Vec<_>>();
        if witnesses.iter().any(|candidate| {
            candidate
                .coordinates
                .iter()
                .zip(point.coordinates)
                .any(|(first, second)| first.to_bits() != second.to_bits())
        }) {
            continue;
        }
        groups.push(FeatureSketchPointGroup {
            id: format!(
                "nx:feature-history:sketch-point-group#{}",
                point.id.rsplit_once('#').map_or("unknown", |(_, key)| key)
            ),
            operation_label: point.operation_label.clone(),
            name: point.name.clone(),
            points: witnesses
                .into_iter()
                .map(|point| point.id.clone())
                .collect(),
            coordinates: point.coordinates,
        });
    }
    groups
}

/// Decode exact named point objects across consecutive offset-store blocks.
pub fn offset_store_named_points(container: &Container) -> Vec<OffsetStoreNamedPoint> {
    let mut points = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        let Some((_, _, records)) = section.as_offset_only() else {
            continue;
        };
        let section_key = format!("nx:om-data-blocks-{section_ordinal}");
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        for ordinal in 0..records.len() {
            let Some(point) = crate::om::offset_store_named_point(
                records[ordinal..].iter().map(|record| record.bytes),
            ) else {
                continue;
            };
            let records = &records[ordinal..ordinal + point.block_count];
            let first_source = entry_offset + records[0].offset as u64;
            let value_source_offset = |payload_offset: usize| {
                let mut relative = payload_offset;
                for record in records {
                    if relative < record.bytes.len() {
                        return Some(entry_offset + record.offset as u64 + relative as u64);
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
            points.push(OffsetStoreNamedPoint {
                id: format!(
                    "nx:offset-store:named-point#{section_ordinal}-{}",
                    ordinal + 1
                ),
                name: point.name,
                data_blocks: (0..point.block_count)
                    .map(|relative| format!("{section_key}:block#{}", ordinal + relative + 1))
                    .collect(),
                values: [first, second],
                source_offset: first_source,
            });
        }
    }
    points
}

/// Join sketch references to named points through exact shared block identity.
pub fn feature_sketch_named_point_block_uses(
    references: &[FeatureSketchReference],
    points: &[OffsetStoreNamedPoint],
) -> Vec<FeatureSketchNamedPointBlockUse> {
    let mut uses = Vec::new();
    for reference in references {
        let Some(data_block) = reference.data_block.as_deref() else {
            continue;
        };
        for point in points {
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
            uses.push(FeatureSketchNamedPointBlockUse {
                id: format!(
                    "nx:feature-history:sketch-named-point-block-use#{operation_key}-{}-{point_key}-{point_block_ordinal}",
                    reference.position.ordinal()
                ),
                operation_label: reference.operation_label.clone(),
                sketch_reference: reference.id.clone(),
                reference_ordinal: reference.position.ordinal(),
                named_point: point.id.clone(),
                data_block: data_block.to_string(),
                point_block_ordinal: point_block_ordinal as u32,
                source_offset: reference.source_offset,
            });
        }
    }
    uses
}

/// Join one named point to a complete sketch lane through unique consecutive block adjacency.
pub fn feature_sketch_preceding_named_point_uses(
    references: &[FeatureSketchReference],
    points: &[OffsetStoreNamedPoint],
) -> Vec<FeatureSketchPrecedingNamedPointUse> {
    fn block_key(block: &str) -> Option<(&str, u32)> {
        let (store, ordinal) = block.rsplit_once(":block#")?;
        Some((store, ordinal.parse().ok()?))
    }

    let mut references_by_operation = BTreeMap::<&str, Vec<&FeatureSketchReference>>::new();
    for reference in references {
        references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut uses = Vec::new();
    for (operation_label, mut operation_references) in references_by_operation {
        operation_references.sort_by_key(|reference| reference.position.ordinal());
        let Some((first_reference, first_block)) = operation_references
            .first()
            .and_then(|reference| Some((*reference, reference.data_block.as_deref()?)))
        else {
            continue;
        };
        let complete_lane = operation_references
            .iter()
            .enumerate()
            .all(|(ordinal, reference)| {
                reference.position.ordinal() == ordinal as u32
                    && usize::from(reference.position.declared_count())
                        == operation_references.len()
                    && reference.data_block.is_some()
            });
        if !complete_lane {
            continue;
        }
        let Some((first_store, first_ordinal)) = block_key(first_block) else {
            continue;
        };
        let candidates = points
            .iter()
            .filter(|point| {
                let Some(last_block) = point.data_blocks.last() else {
                    return false;
                };
                let Some((point_store, point_ordinal)) = block_key(last_block) else {
                    return false;
                };
                point_store == first_store && point_ordinal.checked_add(1) == Some(first_ordinal)
            })
            .collect::<Vec<_>>();
        let [point] = candidates.as_slice() else {
            continue;
        };
        let operation_key = operation_label
            .rsplit_once('#')
            .map_or(operation_label, |(_, key)| key);
        let point_key = point
            .id
            .rsplit_once('#')
            .map_or(point.id.as_str(), |(_, key)| key);
        uses.push(FeatureSketchPrecedingNamedPointUse {
            id: format!(
                "nx:feature-history:sketch-preceding-named-point-use#{operation_key}-{point_key}"
            ),
            operation_label: operation_label.to_string(),
            first_sketch_reference: first_reference.id.clone(),
            named_point: point.id.clone(),
            point_data_blocks: point.data_blocks.clone(),
            following_data_block: first_block.to_string(),
            source_offset: first_reference.source_offset,
        });
    }
    uses
}

/// Join the two exact encodings of a solved sketch point.
pub fn feature_sketch_point_uses(
    point_groups: &[FeatureSketchPointGroup],
    named_points: &[OffsetStoreNamedPoint],
    block_uses: &[FeatureSketchNamedPointBlockUse],
) -> Vec<FeatureSketchPointUse> {
    let named_points = named_points
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<BTreeMap<_, _>>();
    let mut uses = Vec::new();
    let mut joined = BTreeSet::new();
    for block_use in block_uses {
        let key = (
            block_use.operation_label.as_str(),
            block_use.named_point.as_str(),
        );
        if !joined.insert(key) {
            continue;
        }
        let Some(named_point) = named_points.get(block_use.named_point.as_str()) else {
            continue;
        };
        let mut point_block_uses = block_uses
            .iter()
            .filter(|candidate| {
                candidate.operation_label == block_use.operation_label
                    && candidate.named_point == block_use.named_point
            })
            .collect::<Vec<_>>();
        point_block_uses.sort_by_key(|block_use| {
            (
                block_use.reference_ordinal,
                block_use.source_offset,
                block_use.id.as_str(),
            )
        });
        let candidates = point_groups
            .iter()
            .filter(|group| {
                group.operation_label == block_use.operation_label && group.name == named_point.name
            })
            .collect::<Vec<_>>();
        let [point_group] = candidates.as_slice() else {
            continue;
        };
        if point_group
            .coordinates
            .iter()
            .zip(named_point.values.map(|token| token.scalar.value()))
            .any(|(first, second)| first.to_bits() != second.to_bits())
        {
            continue;
        }
        uses.push(FeatureSketchPointUse {
            id: point_block_uses[0].id.replacen(
                "sketch-named-point-block-use",
                "sketch-point-use",
                1,
            ),
            operation_label: block_use.operation_label.clone(),
            references: point_block_uses
                .into_iter()
                .map(|block_use| FeatureSketchPointUseReference {
                    sketch_reference: block_use.sketch_reference.clone(),
                    block_use: block_use.id.clone(),
                    source_offset: block_use.source_offset,
                })
                .collect(),
            sketch_point_group: point_group.id.clone(),
            named_point: named_point.id.clone(),
        });
    }
    uses
}

/// Join one uniquely sketch-owned named-point block to a later datum-CSYS construction.
pub fn feature_sketch_datum_csys_dependencies(
    labels: &[FeatureOperationLabel],
    named_points: &[OffsetStoreNamedPoint],
    point_uses: &[FeatureSketchPointUse],
    constructions: &[FeatureDatumCsysConstruction],
    scalars: &[FeaturePayloadScalar],
) -> Vec<FeatureSketchDatumCsysDependency> {
    fn block_key(block: &str) -> Option<(&str, u32)> {
        let (store, ordinal) = block.rsplit_once(":block#")?;
        Some((store, ordinal.parse().ok()?))
    }

    let positions = feature_operation_chronological_labels(labels)
        .into_iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let points = named_points
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<BTreeMap<_, _>>();
    let mut dependencies = Vec::new();
    for construction in constructions {
        let Some(consumer_position) = positions.get(construction.operation_label.as_str()) else {
            continue;
        };
        let mut candidate: Option<(usize, FeatureSketchDatumCsysBlockRelation)> = None;
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
            for shared_block in construction
                .frame
                .members()
                .iter()
                .map(|(_, binding)| binding)
                .filter(|block| point.data_blocks.contains(block))
            {
                let relation = FeatureSketchDatumCsysBlockRelation::Shared {
                    data_block: shared_block.clone(),
                };
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
                    let relation = FeatureSketchDatumCsysBlockRelation::Consecutive {
                        point_data_block: point_last_block.clone(),
                        construction_data_block: construction_first_block.clone(),
                    };
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
        let point = points[point_use.named_point.as_str()];
        let scalar_aliases = point
            .values
            .iter()
            .enumerate()
            .flat_map(|(coordinate_ordinal, value)| {
                let value_source_offset = value.source_offset;
                scalars
                    .iter()
                    .filter(move |scalar| {
                        scalar.operation_label == construction.operation_label
                            && scalar.source_offset == value_source_offset
                    })
                    .map(move |scalar| FeatureSketchDatumCsysScalarAlias {
                        sketch_coordinate_ordinal: coordinate_ordinal as u8,
                        datum_csys_scalar: scalar.id.clone(),
                        value_source_offset,
                    })
            })
            .collect();
        dependencies.push(FeatureSketchDatumCsysDependency {
            id: construction.id.replacen(
                "datum-csys-construction",
                "sketch-datum-csys-dependency",
                1,
            ),
            sketch_operation_label: point_use.operation_label.clone(),
            datum_csys_operation_label: construction.operation_label.clone(),
            sketch_point_use: point_use.id.clone(),
            datum_csys_construction: construction.id.clone(),
            block_relation: block_relation.clone(),
            scalar_aliases,
            source_offset: point_use.references[0].source_offset,
        });
    }
    dependencies.sort_by(|left, right| left.id.cmp(&right.id));
    dependencies
}

pub(crate) fn parse_sketch_point_name(value: &str) -> Option<u32> {
    let suffix = value.strip_prefix("Point")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let ordinal = suffix.parse::<u32>().ok()?;
    (ordinal != 0).then_some(ordinal)
}

/// Decode and resolve the ordered counted-reference field in sketch payloads.
pub fn feature_sketch_references(container: &Container) -> Vec<FeatureSketchReference> {
    let indexed = container.indexed_om_sections();
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(decoded) = crate::om::sketch_payload_references(record.payload_view()) else {
                return;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            references.extend(decoded.into_positioned().map(|(position, reference)| {
                let ordinal = position.ordinal();
                let data_block = unique_offset_data_block(&indexed, reference.token.value());
                FeatureSketchReference {
                    id: format!(
                        "nx:feature-history:sketch-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    position,
                    token: reference.token,
                    data_block,
                    source_offset: entry_offset + reference.offset as u64,
                }
            }));
        },
    );
    references
}

struct ResolvedFeaturePayloadReference {
    section_key: String,
    operation_ordinal: usize,
    ordinal: usize,
    token: PayloadIndexToken,
    data_block: Option<String>,
    source_offset: u64,
}

fn resolved_feature_payload_references(
    container: &Container,
    decode: impl Fn(
        crate::om::operation_record::OperationPayload<'_>,
        u64,
    ) -> Option<Vec<(PayloadIndexToken, u64)>>,
) -> Vec<ResolvedFeaturePayloadReference> {
    let indexed = container.indexed_om_sections();
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(decoded) = decode(record.payload_view(), entry_offset) else {
                return;
            };
            references.extend(decoded.into_iter().enumerate().map(
                |(ordinal, (token, source_offset))| ResolvedFeaturePayloadReference {
                    section_key: section_key.to_string(),
                    operation_ordinal,
                    ordinal,
                    token,
                    data_block: unique_offset_data_block(&indexed, token.value()),
                    source_offset,
                },
            ));
        },
    );
    references
}

/// Decode and resolve the exact ordered construction-reference field in
/// projected-curve payloads without assigning semantic roles to its slots.
pub fn feature_projected_curve_references(
    container: &Container,
) -> Vec<FeatureProjectedCurveReference> {
    resolved_feature_payload_references(container, |record, base| {
        crate::om::projected_references::ProjectedCurveReferences::read(record).and_then(|field| {
            field
                .into_references()
                .into_iter()
                .map(|reference| {
                    Some((reference.token, base.checked_add(reference.offset as u64)?))
                })
                .collect()
        })
    })
    .into_iter()
    .map(|reference| {
        let operation_label = format!(
            "nx:feature-history:operation-label#{}-{:010}",
            reference.section_key, reference.operation_ordinal
        );
        FeatureProjectedCurveReference {
            id: format!(
                "nx:feature-history:projected-curve-reference#{}-{:010}-{:010}",
                reference.section_key, reference.operation_ordinal, reference.ordinal
            ),
            operation_label,
            ordinal: reference.ordinal as u32,
            token: reference.token,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        }
    })
    .collect()
}

/// Reconstruct ordered logical payloads from projected-curve reference fields.
pub fn feature_projected_curve_construction_payloads(
    container: &Container,
    labels: &[FeatureOperationLabel],
    references: &[FeatureProjectedCurveReference],
) -> Vec<FeatureConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    let kinds = labels
        .iter()
        .map(|label| (label.id.as_str(), label.value.as_str()))
        .collect::<BTreeMap<_, _>>();
    references
        .iter()
        .map(|reference| reference.operation_label.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|operation_label| {
            let (operation_kind, expected_len) = match *kinds.get(operation_label)? {
                "CPROJ" => (FeatureProjectedCurveKind::Projected, 3),
                "CPROJ_CMB" => (FeatureProjectedCurveKind::Combined, 8),
                _ => return None,
            };
            let mut field = references
                .iter()
                .filter(|reference| reference.operation_label == operation_label)
                .collect::<Vec<_>>();
            field.sort_by_key(|reference| reference.ordinal);
            if field.len() != expected_len
                || field
                    .iter()
                    .enumerate()
                    .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
            {
                return None;
            }
            let data_blocks = field
                .iter()
                .map(|reference| reference.data_block.clone())
                .collect::<Option<Vec<_>>>()?;
            let store = data_blocks.first()?.rsplit_once(":block#")?.0;
            if data_blocks.iter().any(|block| {
                block
                    .rsplit_once(":block#")
                    .is_none_or(|(prefix, _)| prefix != store)
            }) {
                return None;
            }
            let (_, content) = FeaturePayloadContent::from_source(data_blocks, &blocks)?;
            let (_, operation_key) = operation_label.rsplit_once('#')?;
            Some(FeatureConstructionPayload {
                id: format!(
                    "nx:feature-history:projected-curve-construction-payload#{operation_key}"
                ),
                operation_label: operation_label.to_string(),
                owner: FeatureConstructionOwner::ProjectedCurve {
                    operation_kind,
                    construction_references: field
                        .iter()
                        .map(|reference| reference.id.clone())
                        .collect(),
                },
                content,
            })
        })
        .collect()
}

/// Decode canonical printable strings from reconstructed projected-curve payloads.
pub fn feature_projected_curve_construction_strings(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Vec<FeatureProjectedCurveConstructionString> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::string_values(joined.bytes(), 0)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, value)| {
                    let payload_offset = value.offset as u64;
                    Some(FeatureProjectedCurveConstructionString {
                        id: format!("{}-string-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        value: value.value.into_owned(),
                        payload_offset,
                        source_offset: joined.source_offset(payload_offset)?,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode exact point-feature construction headers without assigning coordinate semantics.
pub fn feature_point_construction_headers(
    container: &Container,
) -> Vec<FeaturePointConstructionHeader> {
    let indexed = container.indexed_om_sections();
    let mut headers = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(header) = crate::om::point_feature_payload_header(record.payload_view())
            else {
                return;
            };
            headers.push(FeaturePointConstructionHeader {
                id: format!(
                    "nx:feature-history:point-construction-header#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                token: header.reference.token,
                data_block: unique_offset_data_block(&indexed, header.reference.token.value()),
                mode: header.mode,
                source_offset: entry_offset + header.reference.offset as u64,
            });
        },
    );
    headers
}

/// Decode exact scalar lanes selected by uniquely resolved point-feature headers.
pub fn feature_point_construction_scalar_lanes(
    container: &Container,
    headers: &[FeaturePointConstructionHeader],
) -> Vec<FeaturePointConstructionScalarLane> {
    let indexed = container.indexed_om_sections();
    let mut lanes = Vec::new();
    for header in headers {
        let Some(expected_target) = header.data_block.as_deref() else {
            continue;
        };
        let Ok(target_ordinal) = usize::try_from(header.token.value()) else {
            continue;
        };
        let candidates = indexed
            .iter()
            .enumerate()
            .filter_map(|(section_ordinal, (entry, section))| {
                let records = section.as_offset_only()?.2;
                if target_ordinal < 2 {
                    return None;
                }
                let target_id =
                    format!("nx:om-data-blocks-{section_ordinal}:block#{target_ordinal}");
                if target_id != expected_target {
                    return None;
                }
                let preceding = records.get(target_ordinal - 2)?;
                let target = records.get(target_ordinal - 1)?;
                let lane = crate::om::point_feature_scalar_lane(preceding.bytes, target.bytes)?;
                Some((section_ordinal, *entry, preceding, target, lane))
            })
            .collect::<Vec<_>>();
        let [(section_ordinal, entry, preceding, target, lane)] = candidates.as_slice() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let Some(first_source_offset) = entry_offset
            .checked_add(preceding.offset as u64)
            .and_then(|base| base.checked_add(lane.value_offsets()[0] as u64))
        else {
            continue;
        };
        let Some(target_source_offset) = entry_offset.checked_add(target.offset as u64) else {
            continue;
        };
        let Ok(positions) = PointScalarPositions::new(first_source_offset, target_source_offset)
        else {
            continue;
        };
        lanes.push(FeaturePointConstructionScalarLane {
            id: header.id.replacen(
                "point-construction-header#",
                "point-construction-scalar-lane#",
                1,
            ),
            operation_label: header.operation_label.clone(),
            construction_header: header.id.clone(),
            data_blocks: [
                format!(
                    "nx:om-data-blocks-{section_ordinal}:block#{}",
                    target_ordinal - 1
                ),
                format!("nx:om-data-blocks-{section_ordinal}:block#{target_ordinal}"),
            ],
            scalars: lane.values,
            positions,
        });
    }
    lanes
}

/// Decode and resolve the exact common reference envelope in surface-feature
/// payloads without assigning section or guide semantics to its slots.
pub fn feature_surface_construction_references(
    container: &Container,
) -> Vec<FeatureSurfaceConstructionReference> {
    resolved_feature_payload_references(container, |record, base| {
        crate::om::surface_envelope::surface_feature_payload_references(record)
            .and_then(|field| field.relocate(base))
            .map(|field| field.references().into_iter().collect())
            .or_else(|| {
                crate::om::surface_envelope::thru_curve_payload_references(record)
                    .and_then(|field| field.relocate(base))
                    .map(|field| field.references().into_iter().collect())
            })
    })
    .into_iter()
    .map(|reference| {
        let operation_label = format!(
            "nx:feature-history:operation-label#{}-{:010}",
            reference.section_key, reference.operation_ordinal
        );
        FeatureSurfaceConstructionReference {
            id: format!(
                "nx:feature-history:surface-construction-reference#{}-{:010}-{:010}",
                reference.section_key, reference.operation_ordinal, reference.ordinal
            ),
            operation_label,
            ordinal: reference.ordinal as u32,
            token: reference.token,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        }
    })
    .collect()
}

/// Decode the exact leading construction envelope in each `THRU_CURVE` payload.
pub fn feature_thru_curve_construction_envelopes(
    container: &Container,
) -> Vec<FeatureThruCurveConstructionEnvelope> {
    let mut envelopes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(field) =
                crate::om::surface_envelope::thru_curve_payload_references(record.payload_view())
                    .and_then(|field| field.relocate(entry_offset))
            else {
                return;
            };
            let operation_key = format!("{section_key}-{operation_ordinal:010}");
            envelopes.push(FeatureThruCurveConstructionEnvelope {
                id: format!("nx:feature-history:thru-curve-construction-envelope#{operation_key}"),
                operation_label: format!("nx:feature-history:operation-label#{operation_key}"),
                discriminator: field.discriminator,
                controls: field.controls,
                trailing_control: field.trailing_control,
                trailing_value: field.trailing_value,
                source_offset: field.origin(),
            });
        },
    );
    envelopes
}

/// Decode and resolve each exact leading `SWP104` construction branch.
pub fn feature_swp104_leading_branches(container: &Container) -> Vec<FeatureSwp104LeadingBranch> {
    let indexed = container.indexed_om_sections();
    let mut branches = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(branch) = crate::om::swp104_payload_leading_branch(record.payload_view())
            else {
                return;
            };
            let operation_key = format!("{section_key}-{operation_ordinal:010}");
            let Some(source_offset) = entry_offset.checked_add(record.payload_offset() as u64)
            else {
                return;
            };
            if let Some(branch) = FeatureSwp104LeadingBranch::from_source(
                format!("nx:feature-history:swp104-leading-branch#{operation_key}"),
                format!("nx:feature-history:operation-label#{operation_key}"),
                source_offset,
                branch,
                |token| unique_offset_data_block(&indexed, token.value()),
            ) {
                branches.push(branch);
            }
        },
    );
    branches
}

/// Reconstruct ordered logical payloads from complete surface-construction graphs.
pub fn feature_surface_construction_payloads(
    container: &Container,
    references: &[FeatureSurfaceConstructionReference],
) -> Vec<FeatureSurfaceConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    references
        .iter()
        .map(|reference| reference.operation_label.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|operation_label| {
            let mut graph = references
                .iter()
                .filter(|reference| reference.operation_label == operation_label)
                .collect::<Vec<_>>();
            graph.sort_by_key(|reference| reference.ordinal);
            if graph
                .iter()
                .enumerate()
                .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
            {
                return None;
            }
            let graph: [&FeatureSurfaceConstructionReference; 14] = graph.try_into().ok()?;
            let data_blocks = graph
                .each_ref()
                .map(|reference| reference.data_block.clone())
                .into_iter()
                .collect::<Option<Vec<_>>>()?;
            let store = data_blocks.first()?.rsplit_once(":block#")?.0;
            if data_blocks.iter().any(|block| {
                block
                    .rsplit_once(":block#")
                    .is_none_or(|(prefix, _)| prefix != store)
            }) {
                return None;
            }
            let (_, content) = FeaturePayloadContent::from_source(data_blocks, &blocks)?;
            let (_, operation_key) = operation_label.rsplit_once('#')?;
            Some(FeatureSurfaceConstructionPayload {
                id: format!("nx:feature-history:surface-construction-payload#{operation_key}"),
                operation_label: operation_label.to_string(),
                construction_references: graph.each_ref().map(|reference| reference.id.clone()),
                content,
            })
        })
        .collect()
}

/// Decode exact scalar-pair frames from reconstructed surface payloads.
pub fn feature_surface_construction_scalar_pairs(
    container: &Container,
    payloads: &[FeatureSurfaceConstructionPayload],
) -> Vec<FeaturePayloadScalarPair> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::binary64_pair::object_pairs(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, pair)| {
                    Some(FeaturePayloadScalarPair {
                        id: format!("{}-scalar-pair-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        payload: FeatureScalarPairPayload::SurfaceConstruction {
                            surface_construction_payload: payload.id.clone(),
                            frame: pair.into_wire_frame()?,
                        },
                        ordinal: ordinal as u32,
                        value_source_offsets: [
                            joined.source_offset(pair.value_offsets()[0] as u64)?,
                            joined.source_offset(pair.value_offsets()[1] as u64)?,
                        ],
                        source_offset: joined.source_offset(pair.offset() as u64)?,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode exact printable string frames from reconstructed surface payloads.
pub fn feature_surface_construction_strings(
    container: &Container,
    payloads: &[FeatureSurfaceConstructionPayload],
) -> Vec<FeatureSurfaceConstructionString> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::surface_payload_strings(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, value)| {
                    let payload_offset = value.offset as u64;
                    Some(FeatureSurfaceConstructionString {
                        id: format!("{}-string-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        surface_construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        value: value.value.into_owned(),
                        payload_offset,
                        source_offset: joined.source_offset(payload_offset)?,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode and resolve the witnessed ordered profile list in extrusion payloads.
pub fn feature_extrude_profile_references(
    container: &Container,
) -> Vec<FeatureExtrudeProfileReference> {
    let indexed = container.indexed_om_sections();
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(decoded) =
                crate::om::extrude_profile::extrude_profile_references(record.payload_view())
                    .and_then(|field| field.relocate(entry_offset))
            else {
                return;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            references.extend(decoded.references().enumerate().map(|(ordinal, (token, source_offset, witness_source_offset))| {
                FeatureExtrudeProfileReference {
                    id: format!(
                        "nx:feature-history:extrude-profile-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: ordinal as u32,
                    field_tag: decoded.field_tag(),
                    witness_source_offset,
                    token,
                    data_block: unique_offset_data_block(&indexed, token.value()),
                    source_offset,
                }
            }));
        },
    );
    references
}

/// Decode fixed scalar headers from bounded extrusion payloads.
pub fn feature_extrude_payload_headers(container: &Container) -> Vec<FeatureExtrudePayloadHeader> {
    let mut headers = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(header) = crate::om::extrude_payload_header(record.payload_view()) else {
                return;
            };
            headers.push(FeatureExtrudePayloadHeader {
                id: format!(
                    "nx:feature-history:extrude-payload-header#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                scalars: header.scalars,
                source_offset: entry_offset + header.offset as u64,
            });
        },
    );
    headers
}

/// Decode exact terminal discriminator lanes from bounded operation payloads.
pub fn feature_operation_terminal_discriminators(
    container: &Container,
) -> Vec<FeatureOperationTerminalDiscriminator> {
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(frame) = crate::om::terminal_discriminator::operation_terminal_discriminator(
                record.payload_view(),
            )
            .and_then(|frame| frame.relocate(entry_offset)) else {
                return;
            };
            lanes.push(FeatureOperationTerminalDiscriminator {
                id: format!("nx:feature-history:operation-terminal-discriminator#{section_key}-{operation_ordinal:010}"),
                operation_label: format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"),
                frame,
            });
        },
    );
    lanes
}

/// Decode typed scalar clauses anchored to operation body-reference fields.
pub fn feature_operation_body_scalar_triples(
    container: &Container,
) -> Vec<FeatureOperationBodyScalarTriple> {
    let mut triples = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            for triple in
                crate::om::body_scalar_triple::operation_body_scalar_triples(record.body_view())
            {
                let Some(scalars) = triple.scalars.relocate(entry_offset) else {
                    continue;
                };
                triples.push(FeatureOperationBodyScalarTriple {
                    id: format!(
                        "nx:feature-history:operation-body-scalar-triple#{section_key}-{operation_ordinal:010}-{}",
                        triple.body_reference_ordinal
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                    ),
                    body_reference_ordinal: triple.body_reference_ordinal,
                    body_object_index: triple.body_object_index,
                    branch: triple.branch,
                    scalars,
                });
            }
        },
    );
    triples
}

/// Decode ordered member lanes following branch-`11` operation body clauses.
pub fn feature_operation_body_members(container: &Container) -> Vec<FeatureOperationBodyMember> {
    let mut members = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            members.extend(
                crate::om::operation_body_members(record.body_view())
                    .into_iter()
                    .flat_map(|group| group.members.into_iter().enumerate().map(move |(ordinal, member)| FeatureOperationBodyMember {
                        id: format!(
                            "nx:feature-history:operation-body-member#{section_key}-{operation_ordinal:010}-{}-{}",
                            group.body_reference_ordinal, ordinal as u32
                        ),
                        operation_label: format!(
                            "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                        ),
                        body_reference_ordinal: group.body_reference_ordinal,
                        body_object_index: group.body_object_index,
                        ordinal: ordinal as u32,
                        member: LocatedCompactIndex { atom: member.atom, offset: entry_offset + member.offset as u64 },
                    })),
            );
        },
    );
    members
}

/// Resolve wrapped operation members that name known feature-body identities.
pub fn feature_operation_body_operands(
    members: &[FeatureOperationBodyMember],
    references: &[FeatureBodyReference],
    inputs: &[FeatureInputBlock],
    blocks: &[crate::native::om::DataBlock],
    bindings: &[SegmentBodyBinding],
) -> Vec<FeatureOperationBodyOperand> {
    let input_operations = inputs
        .iter()
        .map(|input| input.operation_label.as_str())
        .collect::<BTreeSet<_>>();
    let mut stores_by_operation = BTreeMap::<&str, BTreeSet<&str>>::new();
    for input in inputs {
        let Some((store, _)) = input.data_block.rsplit_once(":block#") else {
            continue;
        };
        stores_by_operation
            .entry(input.operation_label.as_str())
            .or_default()
            .insert(store);
    }
    let unique_stores = stores_by_operation
        .into_iter()
        .filter_map(|(operation, stores)| {
            let stores = stores.into_iter().collect::<Vec<_>>();
            let [store] = stores.as_slice() else {
                return None;
            };
            Some((operation, *store))
        })
        .collect::<BTreeMap<_, _>>();
    let block_ids = blocks
        .iter()
        .map(|block| block.id.as_str())
        .collect::<BTreeSet<_>>();

    members
        .iter()
        .filter_map(|member| {
            if member.member.atom.value() == member.body_object_index {
                return None;
            }
            let member_store = unique_stores.get(member.operation_label.as_str()).copied();
            if member_store.is_none() && input_operations.contains(member.operation_label.as_str())
            {
                return None;
            }
            let operand_data_block = member_store.and_then(|store| {
                let id = format!("{store}:block#{}", member.member.atom.value());
                block_ids.contains(id.as_str()).then_some(id)
            });
            let same_namespace_reference = references.iter().any(|reference| match member_store {
                Some(store) => {
                    unique_stores
                        .get(reference.operation_label.as_str())
                        .copied()
                        == Some(store)
                }
                None => !input_operations.contains(reference.operation_label.as_str()),
            });
            let segment_body_bindings = if member_store.is_none() {
                bindings
                    .iter()
                    .filter(|binding| {
                        binding.body_object_index == member.member.atom.value()
                            || binding.body_alias_object_index == member.member.atom.value()
                    })
                    .map(|binding| binding.id.clone())
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            if member_store.is_some() && operand_data_block.is_none() {
                return None;
            }
            if !same_namespace_reference && segment_body_bindings.is_empty() {
                return None;
            }
            Some(FeatureOperationBodyOperand {
                id: member
                    .id
                    .replacen("operation-body-member", "operation-body-operand", 1),
                operation_label: member.operation_label.clone(),
                body_object_index: member.body_object_index,
                body_reference_ordinal: member.body_reference_ordinal,
                ordinal: member.ordinal,
                operand: member.member,
                operand_data_block,
                segment_body_bindings,
            })
        })
        .collect()
}

/// Decode exact continuations following `TRIM BODY` branch-`11` member lanes.
pub fn feature_operation_body_11_continuations(
    container: &Container,
) -> Vec<FeatureOperationBody11Continuation> {
    let mut continuations = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            continuations.extend(
                crate::om::operation_body_11_continuations(record.body_view())
                    .into_iter()
                    .map(|continuation| FeatureOperationBody11Continuation {
                        id: format!(
                            "nx:feature-history:trim-body-11-continuation#{section_key}-{operation_ordinal:010}-{}",
                            continuation.body_reference_ordinal
                        ),
                        operation_label: format!(
                            "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                        ),
                        body_reference_ordinal: continuation.body_reference_ordinal,
                        body_object_index: continuation.body_object_index,
                        continuation: crate::om::compact::LocatedCompactIndex {
                            atom: continuation.continuation.atom,
                            offset: entry_offset + continuation.continuation.offset as u64,
                        },
                        terminal: continuation.terminal.token,
                        terminal_source_offset: entry_offset + continuation.terminal.offset as u64,
                    }),
            );
        },
    );
    continuations
}

/// Decode complete unwrapped counted reference lanes following body scalar clauses.
pub fn feature_operation_body_reference_lanes(
    container: &Container,
) -> Vec<FeatureOperationBodyReferenceLane> {
    let indexed = container.indexed_om_sections();
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            for lane in crate::om::operation_body_reference_lanes(record.body_view()) {
                let references = match lane.values {
                    crate::om::OperationBodyReferenceLaneValues::CompactIndex(values) => {
                        FeatureOperationBodyReferences::CompactIndex(
                            values
                                .into_iter()
                                .map(|value| ConstructionReference {
                                    token: value.atom,
                                    data_block: unique_offset_data_block(
                                        &indexed,
                                        value.atom.value(),
                                    ),
                                    source_offset: entry_offset + value.offset as u64,
                                })
                                .collect(),
                        )
                    }
                    crate::om::OperationBodyReferenceLaneValues::PayloadObjectIndex(values) => {
                        FeatureOperationBodyReferences::PayloadObjectIndex(
                            values
                                .into_iter()
                                .map(|value| ConstructionReference {
                                    token: value.token,
                                    data_block: unique_offset_data_block(
                                        &indexed,
                                        value.token.value(),
                                    ),
                                    source_offset: entry_offset + value.offset as u64,
                                })
                                .collect(),
                        )
                    }
                };
                lanes.push(FeatureOperationBodyReferenceLane {
                    id: format!(
                        "nx:feature-history:operation-body-reference-lane#{section_key}-{operation_ordinal:010}-{}",
                        lane.body_reference_ordinal
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                    ),
                    body_reference_ordinal: lane.body_reference_ordinal,
                    body_object_index: lane.body_object_index,
                    branch: lane.branch,
                    references,
                });
            }
        },
    );
    lanes
}

/// Join the two exact encodings of an extrusion construction profile.
pub fn feature_extrude_construction_profiles(
    references: &[FeatureExtrudeProfileReference],
) -> Vec<FeatureExtrudeConstructionProfile> {
    let mut references_by_operation = BTreeMap::<&str, Vec<&FeatureExtrudeProfileReference>>::new();
    for reference in references {
        references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut profiles = Vec::new();
    for (operation_label, mut operation_references) in references_by_operation {
        operation_references.sort_by_key(|reference| reference.ordinal);
        if operation_references
            .iter()
            .enumerate()
            .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
        {
            continue;
        }
        let Some(references) = operation_references
            .iter()
            .map(|reference| {
                Some(FeatureExtrudeConstructionProfileReference {
                    object_index: reference.token.value(),
                    data_block: reference.data_block.clone()?,
                    profile_source_offset: reference.source_offset,
                    witness_source_offset: reference.witness_source_offset?,
                })
            })
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        profiles.push(FeatureExtrudeConstructionProfile {
            id: operation_label.replacen("operation-label", "extrude-construction-profile", 1),
            operation_label: operation_label.to_string(),
            references,
        });
    }
    profiles
}

/// Decode structured `32` branches following extrusion body-reference fields.
pub fn feature_extrude_payload_32_branches(
    container: &Container,
) -> Vec<FeatureExtrudePayload32Branch> {
    let indexed = container.indexed_om_sections();
    let mut branches = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(frame) = crate::om::extrude_32::extrude_payload_32_branch(record.body_view())
                .and_then(|frame| frame.relocate(entry_offset))
            else {
                return;
            };
            branches.push(FeatureExtrudePayload32Branch {
                id: format!("nx:feature-history:extrude-payload-32-branch#{section_key}-{operation_ordinal:010}"),
                operation_label: format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"),
                frame: frame.map_bindings(|index, ()| unique_offset_data_block(&indexed, index)),
            });
        },
    );
    branches
}

/// Join exact profile fields to self-witnessed structured extrusion branches.
pub fn feature_extrude_32_constructions(
    references: &[FeatureExtrudeProfileReference],
    branches: &[FeatureExtrudePayload32Branch],
) -> Vec<FeatureExtrude32Construction> {
    let mut branches_by_operation = BTreeMap::<&str, Vec<&FeatureExtrudePayload32Branch>>::new();
    for branch in branches {
        branches_by_operation
            .entry(branch.operation_label.as_str())
            .or_default()
            .push(branch);
    }
    let mut constructions = Vec::new();
    for (operation_label, operation_branches) in branches_by_operation {
        let [branch] = operation_branches.as_slice() else {
            continue;
        };
        let mut profile = references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
            .collect::<Vec<_>>();
        profile.sort_by_key(|reference| reference.ordinal);
        let Ok(profile) = crate::om::branch_items::BranchItems::new(profile) else {
            continue;
        };
        if profile
            .as_slice()
            .iter()
            .enumerate()
            .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
        {
            continue;
        }
        let Some(profiles) = profile
            .map_indexed(|_, reference| {
                Some(FeatureConstructionMember {
                    reference: reference.id.clone(),
                    data_block: reference.data_block.clone()?,
                })
            })
            .transpose()
        else {
            continue;
        };
        let Some(atom_data_blocks) = branch
            .frame
            .atom_members()
            .clone()
            .map_indexed(|_, (_, binding)| binding)
            .transpose()
        else {
            continue;
        };
        let Some(first_data_blocks) = branch
            .frame
            .first_members()
            .clone()
            .map_indexed(|_, (_, binding)| binding)
            .transpose()
        else {
            continue;
        };
        let Some(second_data_blocks) = branch
            .frame
            .second_members()
            .clone()
            .map_indexed(|_, (_, binding)| binding)
            .transpose()
        else {
            continue;
        };
        constructions.push(FeatureExtrude32Construction {
            id: branch
                .id
                .replacen("extrude-payload-32-branch", "extrude-32-construction", 1),
            operation_label: branch.operation_label.clone(),
            branch: branch.id.clone(),
            body_object_index: branch.frame.terminal().value(),
            profiles,
            atom_data_blocks,
            first_data_blocks,
            second_data_blocks,
        });
    }
    constructions
}

/// Decode and resolve ordered construction references in `BLOCK` payloads.
pub fn feature_block_construction_references(
    container: &Container,
) -> Vec<FeatureBlockConstructionReference> {
    let indexed = container.indexed_om_sections();
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(field) =
                crate::om::block_construction::block_construction_references(record.payload_view())
                    .and_then(|field| field.relocate(entry_offset))
            else {
                return;
            };
            references.extend(BlockReferencePosition::enumerate(field.references()).map(
                |(position, (token, source_offset))| FeatureBlockConstructionReference {
                    id: format!(
                        "nx:feature-history:block-construction-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}", ordinal = position.ordinal()
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                    ),
                    control: field.control(),
                    position,
                    token,
                    data_block: unique_offset_data_block(&indexed, token.value()),
                    source_offset,
                },
            ));
        },
    );
    references
}

/// Join complete, uniquely resolved `BLOCK` construction-reference fields.
// Names follow the ordered source slots in this fixed-width lane.
#[allow(clippy::many_single_char_names)]
pub fn feature_block_constructions(
    references: &[FeatureBlockConstructionReference],
) -> Vec<FeatureBlockConstruction> {
    let mut by_operation = BTreeMap::<&str, Vec<&FeatureBlockConstructionReference>>::new();
    for reference in references {
        by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut constructions = Vec::new();
    for (operation_label, mut field) in by_operation {
        field.sort_by_key(|reference| reference.position.ordinal());
        let Ok(field): Result<[_; 19], _> = field.try_into() else {
            continue;
        };
        if field.iter().enumerate().any(|(ordinal, reference)| {
            reference.position.ordinal() != ordinal as u32 || reference.control != field[0].control
        }) {
            continue;
        }
        let [members @ .., terminal] = &field;
        let resolved = members.map(|reference| {
            reference
                .data_block
                .as_ref()
                .map(|data_block| FeatureConstructionMember {
                    reference: reference.id.clone(),
                    data_block: data_block.clone(),
                })
        });
        let [Some(a), Some(b), Some(c), Some(d), Some(e), Some(f), Some(g), Some(h), Some(i), Some(j), Some(k), Some(l), Some(m), Some(n), Some(o), Some(p), Some(q), Some(r)] =
            resolved
        else {
            continue;
        };
        let members = [a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r];
        let Some(terminal_data_block) = terminal.data_block.clone() else {
            continue;
        };
        constructions.push(FeatureBlockConstruction {
            id: operation_label.replacen("operation-label", "block-construction", 1),
            operation_label: operation_label.to_string(),
            control: field[0].control,
            members,
            terminal_reference: terminal.id.clone(),
            terminal_data_block,
        });
    }
    constructions
}

/// Reconstruct complete `BLOCK` construction payloads in reference order.
pub fn feature_block_construction_payloads(
    container: &Container,
    constructions: &[FeatureBlockConstruction],
) -> Vec<FeatureConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .filter_map(|construction| {
            let mut data_blocks = construction
                .members
                .iter()
                .map(|member| member.data_block.clone())
                .collect::<Vec<_>>();
            data_blocks.push(construction.terminal_data_block.clone());
            let (_, content) = FeaturePayloadContent::from_source(data_blocks, &blocks)?;
            Some(FeatureConstructionPayload {
                id: construction
                    .id
                    .replacen("block-construction", "block-construction-payload", 1),
                operation_label: construction.operation_label.clone(),
                owner: FeatureConstructionOwner::Block {
                    construction: construction.id.clone(),
                },
                content,
            })
        })
        .collect()
}

/// Decode exact framed scalar fields across reconstructed `BLOCK` payloads.
pub fn feature_block_payload_scalars(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Vec<FeaturePayloadScalar> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::construction_payload_scalar_fields(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, field)| {
                    let source_offset = joined.source_offset(field.offset as u64)?;
                    Some(FeaturePayloadScalar {
                        id: format!("{}-scalar-{ordinal}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        payload: FeatureScalarPayload::Construction {
                            construction_payload: payload.id.clone(),
                        },
                        ordinal: ordinal as u32,
                        field_code: field.field_code,
                        scalar: field.scalar,
                        payload_offset: field.offset as u64,
                        source_offset,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode exact compact-code name fields across reconstructed `BLOCK` payloads.
pub fn feature_block_payload_names(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Vec<FeaturePayloadName> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::name_field::scan(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, field)| {
                    let source_offset = joined.source_offset(field.offset() as u64)?;
                    Some(FeaturePayloadName {
                        id: format!("{}-name-{ordinal}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        frame: field.into_native(|offset| joined.source_offset(offset))?,
                        source_offset,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Join complete `BLOCK` payload names to scalar fields in their intervals.
pub fn feature_block_payload_named_records(
    payloads: &[FeatureConstructionPayload],
    names: &[FeaturePayloadName],
    scalars: &[FeaturePayloadScalar],
) -> Vec<FeatureBlockPayloadNamedRecord> {
    let mut records = Vec::new();
    for payload in payloads {
        let mut payload_names = names
            .iter()
            .filter(|name| name.construction_payload == payload.id)
            .collect::<Vec<_>>();
        payload_names.sort_by_key(|name| name.frame.offset());
        for (ordinal, name) in payload_names.iter().enumerate() {
            let end = payload_names
                .get(ordinal + 1)
                .map_or(payload.content.byte_len(), |next| next.frame.offset());
            let mut scalar_fields = scalars
                .iter()
                .filter(|scalar| {
                    scalar.payload.id() == payload.id
                        && scalar.payload_offset > name.frame.offset()
                        && scalar.payload_offset < end
                })
                .collect::<Vec<_>>();
            scalar_fields.sort_by_key(|scalar| scalar.payload_offset);
            records.push(FeatureBlockPayloadNamedRecord {
                id: format!("{}-record-{ordinal}", payload.id),
                operation_label: payload.operation_label.clone(),
                construction_payload: payload.id.clone(),
                name_field: name.id.clone(),
                scalar_fields: scalar_fields
                    .into_iter()
                    .map(|scalar| scalar.id.clone())
                    .collect(),
                payload_start_offset: name.frame.offset(),
                payload_end_offset: end,
            });
        }
    }
    records
}

/// Type exact two-scalar `Point<positive decimal>` `BLOCK` payload intervals.
pub fn feature_block_payload_points(
    records: &[FeatureBlockPayloadNamedRecord],
    names: &[FeaturePayloadName],
    scalars: &[FeaturePayloadScalar],
) -> Vec<FeatureBlockPayloadPoint> {
    let names = names
        .iter()
        .map(|name| (name.id.as_str(), name))
        .collect::<BTreeMap<_, _>>();
    let scalars = scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<BTreeMap<_, _>>();
    records
        .iter()
        .filter_map(|record| {
            let name = names.get(record.name_field.as_str())?;
            parse_sketch_point_name(name.frame.value())?;
            let [first_id, second_id] = record.scalar_fields.as_slice() else {
                return None;
            };
            let first = scalars.get(first_id.as_str())?;
            let second = scalars.get(second_id.as_str())?;
            Some(FeatureBlockPayloadPoint {
                id: format!("{}-point", record.id),
                operation_label: record.operation_label.clone(),
                named_record: record.id.clone(),
                name: name.frame.value().to_owned(),
                scalar_fields: [first.id.clone(), second.id.clone()],
                coordinates: [first.scalar.value(), second.scalar.value()],
            })
        })
        .collect()
}

/// Group every bit-identical same-name `BLOCK` construction-point witness.
pub fn feature_block_payload_point_groups(
    points: &[FeatureBlockPayloadPoint],
) -> Vec<FeatureBlockPayloadPointGroup> {
    let mut grouped = BTreeSet::new();
    let mut groups = Vec::new();
    for point in points {
        let key = (point.operation_label.as_str(), point.name.as_str());
        if !grouped.insert(key) {
            continue;
        }
        let witnesses = points
            .iter()
            .filter(|candidate| {
                candidate.operation_label == point.operation_label && candidate.name == point.name
            })
            .collect::<Vec<_>>();
        if witnesses.iter().any(|candidate| {
            candidate
                .coordinates
                .iter()
                .zip(point.coordinates)
                .any(|(first, second)| first.to_bits() != second.to_bits())
        }) {
            continue;
        }
        groups.push(FeatureBlockPayloadPointGroup {
            id: format!("{}-group", point.id),
            operation_label: point.operation_label.clone(),
            name: point.name.clone(),
            points: witnesses
                .into_iter()
                .map(|point| point.id.clone())
                .collect(),
            coordinates: point.coordinates,
        });
    }
    groups
}

/// Resolve the consecutive three-parameter dimension run of `BLOCK` features.
pub fn feature_block_dimensions(
    constructions: &[FeatureBlockConstruction],
    bindings: &[FeatureParameterBinding],
    declarations: &[ExpressionDeclaration],
    expressions: &[Expression],
) -> Vec<FeatureBlockDimensions> {
    constructions
        .iter()
        .filter_map(|construction| {
            let mut operation_bindings = bindings
                .iter()
                .filter(|binding| binding.operation_label == construction.operation_label)
                .collect::<Vec<_>>();
            operation_bindings
                .sort_by_key(|binding| (binding.input_slot, binding.reference_ordinal));
            let mut anchors = operation_bindings
                .iter()
                .map(|binding| binding.expression_declaration.as_str())
                .collect::<Vec<_>>();
            anchors.sort_unstable();
            anchors.dedup();
            let [anchor] = anchors.as_slice() else {
                return None;
            };
            let start = declarations
                .iter()
                .position(|declaration| declaration.id == *anchor)?;
            let run: [&ExpressionDeclaration; 3] = declarations
                .get(start..start + 3)?
                .iter()
                .collect::<Vec<_>>()
                .try_into()
                .ok()?;
            let first = run[0].name.index();
            if run.iter().enumerate().any(|(ordinal, declaration)| {
                declaration.record.split_once(":entry#").map(|pair| pair.0)
                    != run[0].record.split_once(":entry#").map(|pair| pair.0)
                    || declaration.source_entry != run[0].source_entry
                    || Some(declaration.name.index()) != first.checked_add(ordinal as u32)
                    || declaration.name.as_str() != format!("p{}", declaration.name.index())
            }) {
                return None;
            }
            let resolved: [(&Expression, f64); 3] = run
                .iter()
                .map(|declaration| {
                    let mut matches = expressions.iter().filter(|expression| {
                        expression.declaration.as_deref() == Some(&declaration.id)
                    });
                    let expression = matches.next()?;
                    if matches.next().is_some() {
                        return None;
                    }
                    Some((
                        expression,
                        crate::native::expression_length_in_millimeters(
                            &expression.unit,
                            expression.value?.get(),
                        )?,
                    ))
                })
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()?;
            if resolved
                .iter()
                .zip(run)
                .any(|((expression, _), declaration)| {
                    expression.source_entry != declaration.source_entry
                        || expression.source_table != resolved[0].0.source_table
                })
            {
                return None;
            }
            Some(FeatureBlockDimensions {
                id: construction
                    .id
                    .replacen("block-construction", "block-dimensions", 1),
                operation_label: construction.operation_label.clone(),
                construction: construction.id.clone(),
                anchor_bindings: operation_bindings
                    .into_iter()
                    .map(|binding| binding.id.clone())
                    .collect(),
                dimensions: std::array::from_fn(|slot| FeatureBlockDimension {
                    declaration: run[slot].id.clone(),
                    expression: resolved[slot].0.id.clone(),
                    value: resolved[slot].1,
                }),
            })
        })
        .collect()
}

/// Decode persistent object frames from bounded offset-store blocks.
pub fn data_block_object_frames(container: &Container) -> Vec<DataBlockObjectFrame> {
    let blocks = offset_data_block_bytes(container);
    blocks
        .iter()
        .flat_map(|(data_block, (bytes, source_offset))| {
            crate::om::data_block_object_frames(bytes)
                .into_iter()
                .enumerate()
                .map(|(ordinal, frame)| DataBlockObjectFrame {
                    id: data_block_object_frame_id(data_block, ordinal),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    object: LocatedCompactIndex {
                        atom: frame.atom,
                        offset: source_offset + frame.offset as u64,
                    },
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(crate) fn data_block_object_frame_id(data_block: &str, ordinal: usize) -> String {
    format!(
        "{}-{ordinal}",
        data_block
            .replacen("nx:om-data-blocks-", "nx:om-data-block-object-frames-", 1)
            .replacen(":block#", ":block-frame#", 1)
    )
}

fn unique_offset_data_block(
    indexed: &[(
        crate::container::entry_ref::EntryRef<'_>,
        crate::om::IndexedSection<'_>,
    )],
    object_index: u32,
) -> Option<String> {
    let section_ordinal = unique_offset_data_store(indexed, &[object_index])?;
    Some(format!(
        "nx:om-data-blocks-{section_ordinal}:block#{object_index}"
    ))
}

fn unique_offset_data_store(
    indexed: &[(
        crate::container::entry_ref::EntryRef<'_>,
        crate::om::IndexedSection<'_>,
    )],
    object_indices: &[u32],
) -> Option<usize> {
    if object_indices.is_empty() || object_indices.contains(&0) {
        return None;
    }
    let mut unique = None;
    for (section_ordinal, (_, candidate)) in indexed.iter().enumerate() {
        let matches = candidate.as_offset_only().is_some_and(|(_, _, records)| {
            object_indices.iter().all(|object_index| {
                usize::try_from(*object_index)
                    .ok()
                    .is_some_and(|ordinal| ordinal <= records.len())
            })
        });
        if !matches {
            continue;
        }
        if unique.replace(section_ordinal).is_some() {
            return None;
        }
    }
    unique
}

/// Join operation input lanes to uniquely resolved parameter declarations.
pub fn feature_parameter_bindings(
    inputs: &[FeatureInputBlock],
    references: &[DataBlockReference],
    expressions: &[Expression],
) -> Vec<FeatureParameterBinding> {
    let mut expressions_by_declaration = BTreeMap::<&str, Vec<&str>>::new();
    for expression in expressions {
        if let Some(declaration) = expression.declaration.as_deref() {
            expressions_by_declaration
                .entry(declaration)
                .or_default()
                .push(expression.id.as_str());
        }
    }
    let mut bindings = Vec::new();
    for input in inputs {
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
            bindings.push(FeatureParameterBinding {
                id: format!(
                    "nx:feature-history:parameter-binding#{operation_key}-{}-{}",
                    input.input_slot, reference.ordinal
                ),
                operation_label: input.operation_label.clone(),
                input_slot: input.input_slot,
                input_block: input.data_block.clone(),
                reference_ordinal: reference.ordinal,
                expression_declaration: expression_declaration.clone(),
                expression: expressions_by_declaration
                    .get(expression_declaration.as_str())
                    .and_then(|matches| matches.as_slice().first().filter(|_| matches.len() == 1))
                    .map(|expression| (*expression).to_string()),
                object_id: reference.object.value(),
                source_offset: reference.source_offset,
            });
        }
    }
    bindings
}

/// Group exact expression bindings by consuming operation and expression.
pub fn feature_parameter_uses(bindings: &[FeatureParameterBinding]) -> Vec<FeatureParameterUse> {
    let mut grouped = BTreeMap::<(&str, &str), Vec<&FeatureParameterBinding>>::new();
    for binding in bindings {
        if let Some(expression) = binding.expression.as_deref() {
            grouped
                .entry((binding.operation_label.as_str(), expression))
                .or_default()
                .push(binding);
        }
    }
    let mut uses = grouped
        .into_iter()
        .map(|((operation_label, expression), mut bindings)| {
            bindings.sort_by_key(|binding| binding.source_offset);
            (operation_label, expression, bindings)
        })
        .collect::<Vec<_>>();
    uses.sort_by_key(|(_, _, bindings)| bindings[0].source_offset);
    uses.into_iter()
        .map(|(operation_label, expression, bindings)| {
            let operation_key = operation_label
                .rsplit_once('#')
                .map_or(operation_label, |(_, key)| key);
            let expression_key = expression
                .rsplit_once('#')
                .map_or(expression, |(_, key)| key);
            FeatureParameterUse {
                id: format!("nx:feature-history:parameter-use#{operation_key}-{expression_key}"),
                operation_label: operation_label.to_string(),
                expression: expression.to_string(),
                bindings: bindings
                    .into_iter()
                    .map(|binding| FeatureParameterUseBinding {
                        binding: binding.id.clone(),
                        source_offset: binding.source_offset,
                    })
                    .collect(),
            }
        })
        .collect()
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
