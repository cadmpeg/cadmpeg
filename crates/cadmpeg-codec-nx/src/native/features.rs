// SPDX-License-Identifier: Apache-2.0
//! Feature-history record extractors and their record types.

mod reference;
use reference::{ConstructionReference, NullableConstructionReference};

#[allow(clippy::wildcard_imports)]
use super::*;
use crate::printable_string::PrintableString;
use crate::native::om::{
    data_blocks, DataBlockColumnIndexTable, DataBlockIndexRow, DataBlockLinkedIndexRow,
    DataBlockReference, DataBlockRole, DataBlockTargetIndexRow, Expression, ExpressionDeclaration,
    OmOperationStateJournalGroup, OmSchemaRole,
};
use crate::native::segments::{segment_om_links, SegmentBodyBinding, SegmentOmLink};
use std::borrow::Cow;
use std::num::NonZeroU8;
use crate::om::swp104_state::Swp104StateLane;
use crate::om::scalar::{LocatedBinary64, PayloadScalarAtom, PayloadScalarEncoding, RepeatedScalar, ShiftedBinary32, ShiftedBinary64, ShiftedScalar};
use crate::om::branch_items::BranchItems;
use crate::om::nonempty::NonEmpty;
use crate::om::sketch_scalar::{SketchScaledAtom, SketchMixedScalars, SketchScalarLaneForm};
use crate::om::fixed::{Q155, Q155Atom, Q155Marker, Q155LaneFrame};
use crate::om::scalar_run::FramedScalarRun;
use crate::om::scalar_pair::{PairPosition, SketchPairForm, DatumPairForm, MixedPairForm};
mod pair_wire;
use crate::om::draft_identity::{DraftIdentityFrame, DraftIdentityForm};
use crate::om::plane_descriptor::PlaneDescriptor;
use crate::om::csys_descriptor::{CsysDescriptor, CsysDescriptorSlot, CsysIdentity, LocatedCsysDescriptor};
use crate::om::discriminators::DraftBinary32Branch;
use crate::om::pattern::{PatternRow, PatternRows, PatternScalarEncoding, PatternTerminal, PatternValue, PatternWideValues};
use crate::om::thru_curve_state::ThruCurveBranchItems;
use crate::om::thru_curve_controls::ThruCurveControls;
use crate::om::thru_curve_endings::{ThruCurveBranchSuffix, ThruCurveGroupTerminator};

pub(crate) mod datum_plane_header;
mod payload_content;
mod joined_payload;
use joined_payload::JoinedPayload;
use datum_plane_header::FeatureDatumPlaneHeader;
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
    /// Four object-index slots in header order.
    pub object_indices: [Option<u32>; 4],
    /// Exact serialized object-index tokens in header order.
    pub raw_object_indices: [Vec<u8>; 4],
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

/// Exactly bounded feature-history operation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based record order within the feature-history section.
    pub ordinal: u32,
    /// Exact record byte length.
    pub byte_len: u64,
    /// SHA-256 of the complete operation record.
    pub sha256: String,
    /// Exact serialized post-label payload length.
    pub payload_byte_len: u64,
    /// SHA-256 of the post-label serialized operation payload.
    pub payload_sha256: String,
    /// Record-order-independent header identity when the owning label has one
    /// after content-backed offset-store resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_identity: Option<String>,
    /// Absolute file offset of the first post-label payload byte.
    pub payload_source_offset: u64,
    /// Absolute file offset of the fixed operation-header marker.
    pub source_offset: u64,
}

/// Exactly bounded feature-history operation record without a label frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureUnlabeledOperationRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based order among all operation headers in the section.
    pub ordinal: u32,
    /// Four object-index slots in header order.
    pub object_indices: [Option<u32>; 4],
    /// Absolute source offsets of the four object-index tokens.
    pub object_index_source_offsets: [u64; 4],
    /// Exact record byte length.
    pub byte_len: u64,
    /// SHA-256 of the complete operation record.
    pub sha256: String,
    /// Exact serialized post-header payload length.
    pub payload_byte_len: u64,
    /// SHA-256 of the post-header serialized operation payload.
    pub payload_sha256: String,
    /// Absolute file offset of the first post-header payload byte.
    pub payload_source_offset: u64,
    /// Absolute file offset of the fixed operation-header marker.
    pub source_offset: u64,
}

/// Exact body-write frame retained from one feature operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationBodyWrite {
    /// Globally unique relation identity.
    pub id: String,
    /// Owning operation-label identity, absent for an unlabeled record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_label: Option<String>,
    /// Owning bounded operation-record identity.
    pub operation_record: String,
    /// Zero-based body-write order within the operation payload.
    pub ordinal: u32,
    /// Persistent identity of the body written by this operation.
    pub body_identity: u8,
    /// Partition-local Parasolid GROUP node owned by this feature.
    pub group_node: u32,
    /// Exact serialized GROUP-node token.
    pub raw_group_node: Vec<u8>,
    /// Absolute offset of the GROUP-node token.
    pub group_node_source_offset: u64,
    /// Tagged body-image field discriminator.
    pub endpoint_tag: u8,
    /// Offset-store object containing the body's serialized image.
    pub body_image_object_index: u32,
    /// Unambiguous offset-store block selected by the body-image object index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_image_data_block: Option<String>,
    /// Exact serialized body-image object token.
    pub raw_body_image_object_index: Vec<u8>,
    /// Absolute offset of the body-image object token.
    pub body_image_object_index_source_offset: u64,
    /// Exact serialized frame byte length.
    pub byte_len: u64,
    /// Absolute offset of the opening `01 02` marker.
    pub source_offset: u64,
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
pub struct FeatureOperationObjectReference {
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

/// Exactly framed common record in one bounded feature operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationCommonFrame {
    /// Globally unique common-frame identity.
    pub id: String,
    /// Owning bounded operation record.
    pub operation_record: String,
    /// Zero-based frame order within the operation payload.
    pub ordinal: u32,
    /// Three compact prefix indices.
    pub indices: [u32; 3],
    /// Exact compact-index tokens in order.
    pub raw_indices: [Vec<u8>; 3],
    /// Fixed marker selecting the index layout.
    pub marker: [u8; 3],
    /// Exact eight-byte state lane following the fixed state marker.
    ///
    /// The first three bytes remain an untyped operation-state prefix. The
    /// admitted field mappings begin at byte three; callers must not treat the
    /// prefix, or any other state byte, as feature suppression without the
    /// separate serialized owner and typed-value joins.
    pub state: [u8; 8],
    /// Whether legacy operation modules are inactive, when the stored field is boolean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_inactive_modules: Option<bool>,
    /// Whether the operation modifies Parasolid data, when the stored field is boolean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifies_parasolid_data: Option<bool>,
    /// Exact two-byte `m_splitTrackingData` representation.
    #[serde(default)]
    pub split_tracking_data: [u8; 2],
    /// Serialized operation group count.
    #[serde(default)]
    pub group_count: u8,
    /// Duplicated frame-local ordinal.
    pub local_ordinal: u32,
    /// Exact canonical token repeated for the local ordinal.
    pub raw_local_ordinal: Vec<u8>,
    /// Nullable object reference following the duplicated ordinal.
    pub object_index: Option<u32>,
    /// Exact canonical nullable object-reference token.
    pub raw_object_index: Vec<u8>,
    /// Unique target in the native offset-store data-block arena, when found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Exact serialized frame byte length.
    pub byte_len: u64,
    /// Absolute offset of the first compact index token.
    pub source_offset: u64,
    /// Absolute offsets of the compact prefix-index tokens.
    pub index_source_offsets: [u64; 3],
    /// Absolute offset of the first state byte.
    pub state_source_offset: u64,
    /// Absolute offset of the first local-ordinal token.
    pub local_ordinal_source_offset: u64,
    /// Absolute offset of the object-reference token.
    pub object_index_source_offset: u64,
}

/// Canonical terminal common-frame suffix of one feature operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationTerminalFrame {
    /// Globally unique frame identity.
    pub id: String,
    /// Owning bounded operation record.
    pub operation_record: String,
    /// Exact common frame when it occurs immediately before this suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub immediate_common_frame: Option<String>,
    /// Duplicated frame-local ordinal.
    pub local_ordinal: u32,
    /// Exact canonical token repeated for the local ordinal.
    pub raw_local_ordinal: Vec<u8>,
    /// Nullable object reference following the duplicated ordinal.
    pub object_index: Option<u32>,
    /// Exact canonical nullable object-reference token.
    pub raw_object_index: Vec<u8>,
    /// Unique target in the native offset-store data-block arena, when found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute offset of the first local-ordinal token.
    pub source_offset: u64,
    /// Absolute offset of the object-reference token.
    pub object_index_source_offset: u64,
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
    /// Terminal frame carrying the duplicated local ordinal.
    pub operation_terminal_frame: String,
    /// State-journal group containing the matching row.
    pub journal_group: String,
    /// Zero-based row order within the journal group.
    pub journal_row_ordinal: u32,
    /// Duplicated operation terminal ordinal.
    pub operation_local_ordinal: u32,
    /// Matching journal state ordinal.
    pub journal_state_ordinal: u32,
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

/// Exact text frame retained from a `SYMBOLIC_THREAD` operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FeatureSymbolicThreadTextFrameWire", into = "FeatureSymbolicThreadTextFrameWire")]
pub struct FeatureSymbolicThreadTextFrame {
    /// Globally unique text-frame identity.
    pub id: String,
    /// Owning typed `SYMBOLIC_THREAD` record.
    pub symbolic_thread: String,
    /// Zero-based order among the operation's type-`03` text frames.
    pub ordinal: u32,
    /// Exact UTF-8 text value.
    pub value: crate::payload_text::PayloadText<String>,
    /// Absolute file offset of the text-frame marker.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureSymbolicThreadTextFrameWire {
    id: String,
    symbolic_thread: String,
    ordinal: u32,
    marker: u8,
    value: crate::payload_text::PayloadText<String>,
    source_offset: u64,
}

impl From<FeatureSymbolicThreadTextFrame> for FeatureSymbolicThreadTextFrameWire {
    fn from(frame: FeatureSymbolicThreadTextFrame) -> Self {
        Self {
            id: frame.id,
            symbolic_thread: frame.symbolic_thread,
            ordinal: frame.ordinal,
            marker: 3,
            value: frame.value,
            source_offset: frame.source_offset,
        }
    }
}

impl TryFrom<FeatureSymbolicThreadTextFrameWire> for FeatureSymbolicThreadTextFrame {
    type Error = String;

    fn try_from(wire: FeatureSymbolicThreadTextFrameWire) -> Result<Self, Self::Error> {
        if wire.marker != 3 {
            return Err("marker must be 3 for a symbolic-thread text frame".to_owned());
        }
        Ok(Self {
            id: wire.id,
            symbolic_thread: wire.symbolic_thread,
            ordinal: wire.ordinal,
            value: wire.value,
            source_offset: wire.source_offset,
        })
    }
}

/// Typed text-frame payload retained from one `SYMBOLIC_THREAD` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSymbolicThread {
    /// Globally unique symbolic-thread identity.
    pub id: String,
    /// Owning `SYMBOLIC_THREAD` operation label.
    pub operation_label: String,
    /// Owning exact feature-operation record.
    pub operation_record: String,
    /// Ordered complete type-`03` text frames in the payload.
    pub text_frames: Vec<FeatureSymbolicThreadTextFrame>,
    /// Absolute file offset of the operation record's fixed header marker.
    pub source_offset: u64,
}

/// Typed operation template carried by a hole payload string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSimpleHoleTemplate {
    /// Globally unique template identity.
    pub id: String,
    /// Owning `SIMPLE HOLE`, `CBORE_HOLE`, or `CSUNK_HOLE` operation label.
    pub operation_label: String,
    /// Source string in the native payload-string arena.
    pub payload_string: String,
    /// Hole construction family token.
    pub family: SimpleHoleFamily,
    /// Hole cross-section token.
    pub form: SimpleHoleForm,
    /// Axial extent token.
    pub extent: SimpleHoleExtent,
    /// Entry treatment token.
    pub start_treatment: SimpleHoleEndTreatment,
    /// Exit treatment token.
    pub end_treatment: SimpleHoleEndTreatment,
}

/// Exact threaded-hole template retained from a `SIMPLE HOLE` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureThreadedHoleTemplate {
    /// Globally unique template identity.
    pub id: String,
    /// Owning `SIMPLE HOLE` operation label.
    pub operation_label: String,
    /// Source string in the native payload-string arena.
    pub payload_string: String,
    /// Thread-standard family token.
    pub family: ThreadedHoleFamily,
    /// Axial extent token.
    pub extent: SimpleHoleExtent,
    /// Absolute file offset of the template string marker.
    pub source_offset: u64,
}

/// Exact nonempty redundantly witnessed scalar lane in a simple-hole payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureSimpleHoleRepeatedScalarLaneWire",
    into = "FeatureSimpleHoleRepeatedScalarLaneWire"
)]
pub struct FeatureSimpleHoleRepeatedScalarLane {
    /// Globally unique repeated-lane identity.
    pub id: String,
    /// Owning `SIMPLE HOLE` operation label.
    pub operation_label: String,
    /// Ordered scalars with both source witnesses.
    pub values: NonEmpty<RepeatedScalar<u64>>,
}

#[derive(Serialize, Deserialize)]
struct FeatureSimpleHoleRepeatedScalarLaneWire {
    id: String,
    operation_label: String,
    values: Vec<f64>,
    raw_values: Vec<[u8; 8]>,
    first_witness_offsets: Vec<u64>,
    second_witness_offsets: Vec<u64>,
}

impl From<FeatureSimpleHoleRepeatedScalarLane> for FeatureSimpleHoleRepeatedScalarLaneWire {
    fn from(lane: FeatureSimpleHoleRepeatedScalarLane) -> Self {
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            values: lane.values.iter().map(|token| token.scalar.value()).collect(),
            raw_values: lane.values.iter().map(|token| token.scalar.raw()).collect(),
            first_witness_offsets: lane
                .values
                .iter()
                .map(|token| token.witness_offsets[0])
                .collect(),
            second_witness_offsets: lane
                .values
                .iter()
                .map(|token| token.witness_offsets[1])
                .collect(),
        }
    }
}

impl TryFrom<FeatureSimpleHoleRepeatedScalarLaneWire> for FeatureSimpleHoleRepeatedScalarLane {
    type Error = String;
    fn try_from(wire: FeatureSimpleHoleRepeatedScalarLaneWire) -> Result<Self, Self::Error> {
        let count = wire.values.len();
        if wire.raw_values.len() != count
            || wire.first_witness_offsets.len() != count
            || wire.second_witness_offsets.len() != count
        {
            return Err("simple-hole values, raw_values, first_witness_offsets, and second_witness_offsets must have equal lengths".into());
        }
        let values = wire.values.into_iter().zip(wire.raw_values)
            .zip(wire.first_witness_offsets).zip(wire.second_witness_offsets)
            .map(|(((value, raw), first), second)| Ok(RepeatedScalar {
                scalar: ShiftedBinary64::from_wire(value, raw)
                    .map_err(|error| format!("values/raw_values: {error}"))?,
                witness_offsets: [first, second],
            }))
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            values: NonEmpty::new(values).ok_or("values must contain a repeated scalar")?,
        })
    }
}

/// Offset-store blocks linked after both repeated scalar-lane witnesses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
    /// Globally unique reference-lane identity.
    pub id: String,
    /// Owning `SIMPLE HOLE` operation label.
    pub operation_label: String,
    /// Ordered blocks following the first scalar pair.
    pub first_data_blocks: [String; 2],
    /// Ordered blocks following the repeated scalar lane.
    pub second_data_blocks: [String; 2],
    /// Exact optional wrapper before the first reference pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_reference_prefix: Option<[u8; 8]>,
    /// Exact optional wrapper before the repeated reference pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_reference_prefix: Option<[u8; 8]>,
    /// Absolute offsets of the first pair of tagged-index tokens.
    pub first_reference_offsets: [u64; 2],
    /// Absolute offsets of the repeated pair of tagged-index tokens.
    pub second_reference_offsets: [u64; 2],
}

/// Distinct simple-hole operations sharing one four-block construction identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureSimpleHoleConstructionGroupWire",
    into = "FeatureSimpleHoleConstructionGroupWire"
)]
pub struct FeatureSimpleHoleConstructionGroup {
    /// Globally unique group identity.
    pub id: String,
    /// Shared first-witness block pair.
    pub first_data_blocks: [String; 2],
    /// Shared repeated-witness block pair.
    pub second_data_blocks: [String; 2],
    /// Operations and their construction lanes in feature-history order.
    pub members: SimpleHoleConstructionMembers,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSimpleHoleConstructionMember {
    pub operation_label: String,
    pub scalar_lane: String,
    pub block_reference: String,
}

/// At least two distinct operations in retained feature-history order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleHoleConstructionMembers(Vec<FeatureSimpleHoleConstructionMember>);

impl SimpleHoleConstructionMembers {
    pub fn new(members: Vec<FeatureSimpleHoleConstructionMember>) -> Result<Self, &'static str> {
        if members.len() < 2 {
            return Err("operation_labels must contain at least two members");
        }
        let mut labels = BTreeSet::new();
        if members.iter().any(|member| !labels.insert(member.operation_label.as_str())) {
            return Err("operation_labels must contain distinct members");
        }
        Ok(Self(members))
    }
}

impl std::ops::Deref for SimpleHoleConstructionMembers {
    type Target = [FeatureSimpleHoleConstructionMember];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Serialize, Deserialize)]
struct FeatureSimpleHoleConstructionGroupWire {
    id: String,
    first_data_blocks: [String; 2],
    second_data_blocks: [String; 2],
    operation_labels: Vec<String>,
    scalar_lanes: Vec<String>,
    block_references: Vec<String>,
}

impl From<FeatureSimpleHoleConstructionGroup> for FeatureSimpleHoleConstructionGroupWire {
    fn from(group: FeatureSimpleHoleConstructionGroup) -> Self {
        Self {
            id: group.id,
            first_data_blocks: group.first_data_blocks,
            second_data_blocks: group.second_data_blocks,
            operation_labels: group
                .members
                .iter()
                .map(|member| member.operation_label.clone())
                .collect(),
            scalar_lanes: group
                .members
                .iter()
                .map(|member| member.scalar_lane.clone())
                .collect(),
            block_references: group
                .members
                .iter()
                .map(|member| member.block_reference.clone())
                .collect(),
        }
    }
}

impl TryFrom<FeatureSimpleHoleConstructionGroupWire> for FeatureSimpleHoleConstructionGroup {
    type Error = String;
    fn try_from(wire: FeatureSimpleHoleConstructionGroupWire) -> Result<Self, Self::Error> {
        if wire.operation_labels.len() != wire.scalar_lanes.len()
            || wire.operation_labels.len() != wire.block_references.len()
        {
            return Err("simple-hole operation_labels, scalar_lanes, and block_references must have equal lengths".into());
        }
        Ok(Self {
            id: wire.id,
            first_data_blocks: wire.first_data_blocks,
            second_data_blocks: wire.second_data_blocks,
            members: SimpleHoleConstructionMembers::new(wire
                .operation_labels
                .into_iter()
                .zip(wire.scalar_lanes)
                .zip(wire.block_references)
                .map(|((operation_label, scalar_lane), block_reference)| {
                    FeatureSimpleHoleConstructionMember {
                        operation_label,
                        scalar_lane,
                        block_reference,
                    }
                })
                .collect()).map_err(str::to_owned)?,
        })
    }
}

/// Exact four-block construction-group lane carried by a `HOLE PACKAGE` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FeatureHolePackageConstructionGroupLaneWire", into = "FeatureHolePackageConstructionGroupLaneWire")]
pub struct FeatureHolePackageConstructionGroupLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning `HOLE PACKAGE` operation label.
    pub operation_label: String,
    /// Compact selector preceding the repeated branch byte.
    pub selector: NonZeroU8,
    /// Branch byte repeated between the two reference pairs.
    pub branch: NonZeroU8,
    /// Four checked references with their resolved targets and source offsets.
    pub references: [ConstructionReference<String>; 4],
    /// Payload-relative offset of the lane prefix.
    pub payload_offset: u64,
    /// Absolute file offset of the lane prefix.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureHolePackageConstructionGroupLaneWire {
    /// Globally unique lane identity.
    id: String,
    /// Owning `HOLE PACKAGE` operation label.
    operation_label: String,
    /// Compact selector preceding the repeated branch byte.
    selector: u8,
    /// Branch byte repeated between the two reference pairs.
    branch: u8,
    /// Ordered serialized offset-store block indices.
    object_indices: [u32; 4],
    /// Exact variable-width object-index tokens.
    raw_object_indices: [Vec<u8>; 4],
    /// Uniquely resolved offset-store blocks.
    data_blocks: [String; 4],
    /// Payload-relative offset of the lane prefix.
    payload_offset: u64,
    /// Absolute file offset of the lane prefix.
    source_offset: u64,
    /// Absolute file offsets of the four reference tokens.
    reference_source_offsets: [u64; 4],
}

impl From<FeatureHolePackageConstructionGroupLane> for FeatureHolePackageConstructionGroupLaneWire {
    fn from(value: FeatureHolePackageConstructionGroupLane) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            selector: value.selector.get(),
            branch: value.branch.get(),
            object_indices: value.references.each_ref().map(|reference| reference.token.value()),
            raw_object_indices: value.references.each_ref().map(|reference| reference.token.raw().to_vec()),
            data_blocks: value.references.each_ref().map(|reference| reference.data_block.clone()),
            payload_offset: value.payload_offset,
            source_offset: value.source_offset,
            reference_source_offsets: value.references.each_ref().map(|reference| reference.source_offset),
        }
    }
}

impl TryFrom<FeatureHolePackageConstructionGroupLaneWire> for FeatureHolePackageConstructionGroupLane {
    type Error = String;

    fn try_from(wire: FeatureHolePackageConstructionGroupLaneWire) -> Result<Self, Self::Error> {
        let [a, b, c, d] = [0, 1, 2, 3].map(|slot| {
            crate::om::reference_index::ReferenceIndexToken::from_wire(
                wire.object_indices[slot], &wire.raw_object_indices[slot],
            ).map_err(|error| format!("object_indices/raw_object_indices[{slot}]: {error}"))
            .map(|token| ConstructionReference {
                token,
                data_block: wire.data_blocks[slot].clone(),
                source_offset: wire.reference_source_offsets[slot],
            })
        });
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            selector: NonZeroU8::new(wire.selector).ok_or("selector: zero is not a construction selector")?,
            branch: NonZeroU8::new(wire.branch).ok_or("branch: zero is not a construction branch")?,
            references: [a?, b?, c?, d?],
            payload_offset: wire.payload_offset,
            source_offset: wire.source_offset,
        })
    }
}


/// Exact relation between one hole package and one simple-hole construction group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureHolePackageConstructionGroupUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Owning `HOLE PACKAGE` operation label.
    pub operation_label: String,
    /// Exact package lane carrying the group identity.
    pub construction_group_lane: String,
    /// Uniquely matched simple-hole construction group.
    pub simple_hole_construction_group: String,
    /// Absolute file offset of the package lane.
    pub source_offset: u64,
}

/// Construction family named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleHoleFamily {
    /// General-hole construction family.
    GeneralHole,
}

/// Thread-standard family named by a threaded-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadedHoleFamily {
    /// Metric profile family named by the `M Profile` token.
    MProfile,
    /// Unified National Coarse family named by the `UNC` token.
    Unc,
}

/// Cross-section named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleHoleForm {
    /// Plain cylindrical cross-section.
    Simple,
    /// Counterbored cross-section.
    Counterbored,
    /// Countersunk cross-section.
    Countersunk,
}

/// Axial termination named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleHoleExtent {
    /// Continue through all intersected material.
    Through,
    /// Stop at a blind termination whose distance is carried by another field.
    Blind,
}

/// End treatment named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleHoleEndTreatment {
    /// No separate end treatment is named.
    None,
    /// Chamfer the circular end edge.
    Chamfer,
}

/// Primary selection or ordered body-reference field in one feature operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBodyReference {
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
    pub input_slot: u8,
    /// Object index serialized in that slot.
    pub object_index: u32,
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
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
    pub input_slot: u8,
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureInputBlockIdentityGroupWire {
    id: String,
    data_block: String,
    input_blocks: Vec<String>,
    operation_labels: Vec<String>,
    input_slots: Vec<u8>,
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
    pub input_slot: u8,
    /// Serialized grammar of the referenced column row.
    pub row_kind: ColumnIndexRowKind,
    /// Native row identity in its grammar-specific arena.
    pub column_row: String,
    /// Unique complete composite table containing the row, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_table: Option<String>,
    /// Zero-based slot in the row's four-block lane.
    pub row_slot: u8,
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
    pub input_slot: u8,
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
    input_slot: u8,
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
    pub input_slot: u8,
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
#[serde(try_from = "FeatureDatumCsysConstructionWire", into = "FeatureDatumCsysConstructionWire")]
pub struct FeatureDatumCsysConstruction {
    /// Globally unique construction identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Payload control byte preceding the fixed construction header suffix.
    pub control: u8,
    /// Eight checked references with their resolved targets and source offsets.
    pub references: [ConstructionReference<String>; 8],
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
            control: value.control,
            object_indices: value.references.each_ref().map(|reference| reference.token.value()),
            raw_object_indices: value.references.each_ref().map(|reference| reference.token.raw().to_vec()),
            data_blocks: value.references.each_ref().map(|reference| reference.data_block.clone()),
            source_offsets: value.references.each_ref().map(|reference| reference.source_offset),
        }
    }
}

impl TryFrom<FeatureDatumCsysConstructionWire> for FeatureDatumCsysConstruction {
    type Error = String;

    fn try_from(wire: FeatureDatumCsysConstructionWire) -> Result<Self, Self::Error> {
        let [a, b, c, d, e, f, g, h] = [0, 1, 2, 3, 4, 5, 6, 7].map(|slot| {
            crate::om::reference_index::ReferenceIndexToken::from_wire(
                wire.object_indices[slot], &wire.raw_object_indices[slot],
            ).map_err(|error| format!("object_indices/raw_object_indices[{slot}]: {error}"))
            .map(|token| ConstructionReference {
                token,
                data_block: wire.data_blocks[slot].clone(),
                source_offset: wire.source_offsets[slot],
            })
        });
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            control: wire.control,
            references: [a?, b?, c?, d?, e?, f?, g?, h?],
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
    pub construction_slot: u8,
    /// Serialized grammar of the referenced column row.
    pub row_kind: ColumnIndexRowKind,
    /// Native row identity in its grammar-specific arena.
    pub column_row: String,
    /// Unique complete composite table containing the row, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_table: Option<String>,
    /// Zero-based slot in the row's four-block lane.
    pub row_slot: u8,
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
    /// Checked scalar atoms with payload and source offsets.
    pub values: [FeaturePayloadBinary64Token; 2],
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
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
    payload: FeatureScalarPairPayload,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeaturePayloadBinary64Token {
    pub scalar: ShiftedBinary64,
    pub payload_offset: u64,
    pub source_offset: u64,
}

fn resolved_payload_scalar_pair(
    values: [LocatedBinary64; 2],
    source_offset: impl Fn(usize) -> Option<u64>,
) -> Option<[FeaturePayloadBinary64Token; 2]> {
    let [first, second] = values.map(|value| {
        Some(FeaturePayloadBinary64Token {
            scalar: value.scalar,
            payload_offset: value.offset as u64,
            source_offset: source_offset(value.offset)?,
        })
    });
    Some([first?, second?])
}

impl TryFrom<FeaturePayloadScalarPairWire> for FeaturePayloadScalarPair {
    type Error = String;

    fn try_from(wire: FeaturePayloadScalarPairWire) -> Result<Self, Self::Error> {
        let [first, second] = std::array::from_fn::<_, 2, _>(|i| {
            ShiftedBinary64::from_wire(wire.values[i], wire.raw_values[i])
                .map(|scalar| FeaturePayloadBinary64Token {
                    scalar,
                    payload_offset: wire.value_payload_offsets[i],
                    source_offset: wire.value_source_offsets[i],
                })
                .map_err(|error| format!("values/raw_values[{i}]: {error}"))
        });
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            payload: wire.payload,
            ordinal: wire.ordinal,
            values: [first?, second?],
            payload_offset: wire.payload_offset,
            source_offset: wire.source_offset,
        })
    }
}

/// Payload identity and its scalar-pair branch discriminator.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum FeatureScalarPairPayload {
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

impl FeatureScalarPairPayload {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::DatumCsys {
                datum_csys_payload, ..
            } => datum_csys_payload,
            Self::DatumPlane {
                datum_plane_payload,
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
        let (payload_key, payload_id, discriminator) = match &self.payload {
            FeatureScalarPairPayload::DatumCsys {
                datum_csys_payload,
                discriminator,
            } => (
                "datum_csys_payload",
                datum_csys_payload,
                Some(discriminator),
            ),
            FeatureScalarPairPayload::DatumPlane {
                datum_plane_payload,
            } => ("datum_plane_payload", datum_plane_payload, None),
            FeatureScalarPairPayload::Construction {
                construction_payload,
                discriminator,
            } => (
                "construction_payload",
                construction_payload,
                Some(discriminator),
            ),
            FeatureScalarPairPayload::SurfaceConstruction {
                surface_construction_payload,
                discriminator,
            } => (
                "surface_construction_payload",
                surface_construction_payload,
                Some(discriminator),
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
        record.serialize_field("values", &self.values.map(|token| token.scalar.value()))?;
        record.serialize_field("raw_values", &self.values.map(|token| token.scalar.raw()))?;
        record.serialize_field("payload_offset", &self.payload_offset)?;
        record.serialize_field("value_payload_offsets", &self.values.map(|token| token.payload_offset))?;
        record.serialize_field("source_offset", &self.source_offset)?;
        record.serialize_field("value_source_offsets", &self.values.map(|token| token.source_offset))?;
        if let Some(discriminator) = discriminator {
            record.serialize_field("discriminator", discriminator)?;
        }
        record.end()
    }
}

/// One exactly framed signed Q1.55 pair in a reconstructed datum-CSYS payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "pair_wire::FeatureDatumCsysPayloadFixedPairWire", into = "pair_wire::FeatureDatumCsysPayloadFixedPairWire")]
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
#[serde(try_from = "FeaturePayloadScalarWire", into = "FeaturePayloadScalarWire")]
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
#[serde(try_from = "FeatureDatumCsysDescriptorWire", into = "FeatureDatumCsysDescriptorWire")]
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
        let descriptor = CsysDescriptor::from_wire(wire.prefix, identity, wire.suffix).map_err(str::to_owned)?;
        let descriptor = LocatedCsysDescriptor::new(descriptor, wire.source_offset).map_err(str::to_owned)?;
        if descriptor.identity_source_offset() != wire.identity_source_offset {
            return Err("identity_source_offset must equal source_offset plus prefix length".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            construction: wire.construction,
            reference_ordinal: CsysDescriptorSlot::try_from(wire.reference_ordinal).map_err(str::to_owned)?,
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

/// One compact index in a datum-plane terminal index lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureDatumPlaneIndexLaneEntry {
    pub value: u32,
    pub raw: Vec<u8>,
    pub offset: u64,
}

/// Unique terminal compact-index lane of a reconstructed datum-plane payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureDatumPlaneIndexLane {
    pub offset: u64,
    pub trailer: u32,
    pub entries: Vec<FeatureDatumPlaneIndexLaneEntry>,
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
    pub index_lane: Option<FeatureDatumPlaneIndexLane>,
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
                Some(lane.offset),
                Some(lane.entries.len() + 1),
                lane.entries.iter().map(|entry| entry.value).collect(),
                lane.entries.iter().map(|entry| entry.raw.clone()).collect(),
                lane.entries.iter().map(|entry| entry.offset).collect(),
                Some(lane.trailer),
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
        let index_lane =
            match (
                wire.index_lane_offset,
                wire.index_lane_declared_count,
                wire.index_lane_trailer,
                wire.index_lane_values.is_empty()
                    && wire.index_lane_raw_indices.is_empty()
                    && wire.index_lane_value_offsets.is_empty(),
            ) {
                (None, None, None, true) => None,
                (Some(offset), Some(declared_count), Some(trailer), _) => {
                    if !(2..=255).contains(&declared_count)
                        || declared_count != wire.index_lane_values.len() + 1
                    {
                        return Err("index_lane_declared_count must fit a byte and equal the nonempty entry count plus one".to_owned());
                    }
                    if wire.index_lane_values.len() != wire.index_lane_raw_indices.len()
                        || wire.index_lane_values.len() != wire.index_lane_value_offsets.len()
                    {
                        return Err("datum-plane index lane entry vectors disagree".to_owned());
                    }
                    Some(FeatureDatumPlaneIndexLane {
                        offset,
                        trailer,
                        entries: wire
                            .index_lane_values
                            .into_iter()
                            .zip(wire.index_lane_raw_indices)
                            .zip(wire.index_lane_value_offsets)
                            .map(|((value, raw), offset)| FeatureDatumPlaneIndexLaneEntry {
                                value,
                                raw,
                                offset,
                            })
                            .collect(),
                    })
                }
                _ => return Err(
                    "datum-plane index lane offset count trailer and entries are present together"
                        .to_owned(),
                ),
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
#[serde(try_from = "FeatureDatumPlaneDescriptorWire", into = "FeatureDatumPlaneDescriptorWire")]
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
        let descriptor = PlaneDescriptor::from_wire(wire.identity, &wire.suffix, wire.schema_index, &wire.label)
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
    pub input_slot: u8,
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
    pub reference_ordinal: u8,
    /// Shared offset-store block.
    pub data_block: String,
    /// Matching operation-header input binding.
    pub input_binding: String,
    /// Operation whose header addresses the shared block.
    pub input_operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: u8,
}

/// A construction reference paired with its uniquely resolved source block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureConstructionMember {
    pub reference: String,
    pub data_block: String,
}

/// Completely resolved counted-reference field of one sketch construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FeatureSketchConstructionInputsWire", into = "FeatureSketchConstructionInputsWire")]
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
        let (member_references, member_data_blocks) = value.members.into_iter()
            .map(|member| (member.reference, member.data_block)).unzip();
        Self { id: value.id, operation_label: value.operation_label, sketch_record: value.sketch_record,
            member_references, member_data_blocks, terminal_reference: value.terminal_reference,
            terminal_data_block: value.terminal_data_block }
    }
}

impl TryFrom<FeatureSketchConstructionInputsWire> for FeatureSketchConstructionInputs {
    type Error = String;
    fn try_from(wire: FeatureSketchConstructionInputsWire) -> Result<Self, Self::Error> {
        if wire.member_references.len() != wire.member_data_blocks.len() {
            return Err("member_references and member_data_blocks must have equal lengths".to_owned());
        }
        let members = wire.member_references.into_iter().zip(wire.member_data_blocks)
            .map(|(reference, data_block)| FeatureConstructionMember { reference, data_block })
            .collect::<Vec<_>>();
        Ok(Self { id: wire.id, operation_label: wire.operation_label, sketch_record: wire.sketch_record,
            members, terminal_reference: wire.terminal_reference,
            terminal_data_block: wire.terminal_data_block })
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
        reference_layout: FeaturePatternReferenceLayout,
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
#[serde(try_from = "pair_wire::FeatureSketchPayloadFixedPairWire", into = "pair_wire::FeatureSketchPayloadFixedPairWire")]
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
#[serde(try_from = "pair_wire::FeatureSketchPayloadMixedPairWire", into = "pair_wire::FeatureSketchPayloadMixedPairWire")]
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
            values: record.lane.iter().map(|(_, scalar, _)| scalar.value()).collect(),
            raw_values: record.lane.iter().map(|(_, scalar, _)| scalar.raw().to_vec()).collect(),
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
        let offset = wire.value_payload_offsets.first().copied()
            .ok_or("values must contain a sketch scalar atom")?
            .checked_sub(wire.discriminator.len() as u64)
            .ok_or("value_payload_offsets must follow the discriminator")?;
        let values = wire.values.into_iter().zip(wire.raw_values).zip(wire.value_source_offsets)
            .map(|((value, raw), source)| Ok((ShiftedScalar::from_wire(value, &raw)?, source)))
            .collect::<Result<Vec<_>, String>>()?;
        let lane = FramedScalarRun::new(form, offset, NonEmpty::new(values).ok_or("values must contain a sketch scalar atom")?)?;
        if !lane.iter().map(|(offset, _, _)| offset).eq(wire.value_payload_offsets) {
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

/// Compact type code on a reconstructed payload name that is not payload-leading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeaturePayloadTypeCode {
    /// Decoded compact type code following the `66` marker.
    pub value: u32,
    /// Exact compact type-code token.
    pub raw: Vec<u8>,
    /// Payload-relative offset of the compact type-code token.
    pub payload_offset: u64,
    /// Absolute source offset of the compact type-code token, when mapped.
    pub source_offset: Option<u64>,
}

fn feature_payload_type_code_from_wire(
    type_code: Option<u32>,
    raw_type_code: Option<Vec<u8>>,
    type_code_payload_offset: Option<u64>,
    type_code_source_offset: Option<u64>,
    payload_leading: bool,
) -> Result<Option<FeaturePayloadTypeCode>, String> {
    match (
        type_code,
        raw_type_code,
        type_code_payload_offset,
        type_code_source_offset,
        payload_leading,
    ) {
        (None, None, None, None, true) => Ok(None),
        (Some(value), Some(raw), Some(payload_offset), source_offset, false) => {
            Ok(Some(FeaturePayloadTypeCode {
                value,
                raw,
                payload_offset,
                source_offset,
            }))
        }
        _ => Err(
            "payload name type code is present exactly when payload_leading is false".to_owned(),
        ),
    }
}

fn feature_payload_type_code_to_wire(
    type_code: Option<FeaturePayloadTypeCode>,
) -> (Option<u32>, Option<Vec<u8>>, Option<u64>, Option<u64>, bool) {
    match type_code {
        None => (None, None, None, None, true),
        Some(code) => (
            Some(code.value),
            Some(code.raw),
            Some(code.payload_offset),
            code.source_offset,
            false,
        ),
    }
}

/// Exact framed name retained from one reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureSketchPayloadNameWire",
    into = "FeatureSketchPayloadNameWire"
)]
pub struct FeatureSketchPayloadName {
    /// Globally unique name-field identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying this field.
    pub construction_payload: String,
    /// Zero-based name-field order within the reconstructed payload.
    pub ordinal: u32,
    /// Compact type code, absent for the type-free payload-leading form.
    pub type_code: Option<FeaturePayloadTypeCode>,
    /// Exact printable field value.
    pub value: String,
    /// Byte offset of the opening `66` or payload-leading `03` marker.
    pub payload_offset: u64,
    /// Absolute file offset of the opening marker.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureSketchPayloadNameWire {
    id: String,
    operation_label: String,
    construction_payload: String,
    ordinal: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    type_code: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw_type_code: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    type_code_payload_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    type_code_source_offset: Option<u64>,
    payload_leading: bool,
    value: String,
    payload_offset: u64,
    source_offset: u64,
}

impl From<FeatureSketchPayloadName> for FeatureSketchPayloadNameWire {
    fn from(value: FeatureSketchPayloadName) -> Self {
        let (
            type_code,
            raw_type_code,
            type_code_payload_offset,
            type_code_source_offset,
            payload_leading,
        ) = feature_payload_type_code_to_wire(value.type_code);
        Self {
            id: value.id,
            operation_label: value.operation_label,
            construction_payload: value.construction_payload,
            ordinal: value.ordinal,
            type_code,
            raw_type_code,
            type_code_payload_offset,
            type_code_source_offset,
            payload_leading,
            value: value.value,
            payload_offset: value.payload_offset,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeatureSketchPayloadNameWire> for FeatureSketchPayloadName {
    type Error = String;

    fn try_from(wire: FeatureSketchPayloadNameWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            construction_payload: wire.construction_payload,
            ordinal: wire.ordinal,
            type_code: feature_payload_type_code_from_wire(
                wire.type_code,
                wire.raw_type_code,
                wire.type_code_payload_offset,
                wire.type_code_source_offset,
                wire.payload_leading,
            )?,
            value: wire.value,
            payload_offset: wire.payload_offset,
            source_offset: wire.source_offset,
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
#[serde(try_from = "OffsetStoreNamedPointWire", into = "OffsetStoreNamedPointWire")]
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
                .map(|scalar| FeatureBinary64ScalarToken { scalar, source_offset: wire.value_source_offsets[i] })
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
    /// Zero-based reference order in the counted field.
    pub ordinal: u32,
    /// Effective count encoded by the containing reference field.
    pub declared_count: u8,
    /// Whether this is the reference following the `00 00` separator.
    pub terminal: bool,
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
    pub token: crate::om::reference_index::ReferenceIndexToken,
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

/// Exact two-group object-reference graph carried by an `FSET` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FeatureFsetReferenceGraphWire", into = "FeatureFsetReferenceGraphWire")]
pub struct FeatureFsetReferenceGraph {
    /// Globally unique graph identity.
    pub id: String,
    /// Owning `FSET` operation label.
    pub operation_label: String,
    /// Exact nonempty printable selector preceding the first group.
    pub selector: String,
    /// Two references inside the byte-counted group.
    pub first: [ConstructionReference<Option<String>>; 2],
    /// Three references in the trailing group.
    pub second: [ConstructionReference<Option<String>>; 3],
    /// Absolute source offset of the graph's `01` marker.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureFsetReferenceGraphWire {
    /// Globally unique graph identity.
    id: String,
    /// Owning `FSET` operation label.
    operation_label: String,
    /// Exact nonempty printable selector preceding the first group.
    selector: String,
    /// Serialized object indices in the bounded first group.
    first_object_indices: [u32; 2],
    /// Exact variable-width object-index tokens in the first group.
    raw_first_object_indices: [Vec<u8>; 2],
    /// Unique native data-block targets for the first group.
    first_data_blocks: [Option<String>; 2],
    /// Serialized object indices in the trailing second group.
    second_object_indices: [u32; 3],
    /// Exact variable-width object-index tokens in the second group.
    raw_second_object_indices: [Vec<u8>; 3],
    /// Unique native data-block targets for the second group.
    second_data_blocks: [Option<String>; 3],
    /// Absolute source offset of the graph's `01` marker.
    source_offset: u64,
    /// Absolute source offsets of the first-group width markers.
    first_source_offsets: [u64; 2],
    /// Absolute source offsets of the second-group width markers.
    second_source_offsets: [u64; 3],
}

impl From<FeatureFsetReferenceGraph> for FeatureFsetReferenceGraphWire {
    fn from(value: FeatureFsetReferenceGraph) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            selector: value.selector,
            first_object_indices: value.first.each_ref().map(|reference| reference.token.value()),
            raw_first_object_indices: value.first.each_ref().map(|reference| reference.token.raw().to_vec()),
            first_data_blocks: value.first.each_ref().map(|reference| reference.data_block.clone()),
            second_object_indices: value.second.each_ref().map(|reference| reference.token.value()),
            raw_second_object_indices: value.second.each_ref().map(|reference| reference.token.raw().to_vec()),
            second_data_blocks: value.second.each_ref().map(|reference| reference.data_block.clone()),
            source_offset: value.source_offset,
            first_source_offsets: value.first.each_ref().map(|reference| reference.source_offset),
            second_source_offsets: value.second.each_ref().map(|reference| reference.source_offset),
        }
    }
}

impl TryFrom<FeatureFsetReferenceGraphWire> for FeatureFsetReferenceGraph {
    type Error = String;

    fn try_from(wire: FeatureFsetReferenceGraphWire) -> Result<Self, Self::Error> {
        let [a, b] = [0, 1].map(|slot| {
            crate::om::reference_index::ReferenceIndexToken::from_wire(
                wire.first_object_indices[slot], &wire.raw_first_object_indices[slot],
            ).map_err(|error| format!("first_object_indices/raw_first_object_indices[{slot}]: {error}"))
            .map(|token| ConstructionReference {
                token,
                data_block: wire.first_data_blocks[slot].clone(),
                source_offset: wire.first_source_offsets[slot],
            })
        });
        let [c, d, e] = [0, 1, 2].map(|slot| {
            crate::om::reference_index::ReferenceIndexToken::from_wire(
                wire.second_object_indices[slot], &wire.raw_second_object_indices[slot],
            ).map_err(|error| format!("second_object_indices/raw_second_object_indices[{slot}]: {error}"))
            .map(|token| ConstructionReference {
                token,
                data_block: wire.second_data_blocks[slot].clone(),
                source_offset: wire.second_source_offsets[slot],
            })
        });
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            selector: wire.selector,
            first: [a?, b?],
            second: [c?, d?, e?],
            source_offset: wire.source_offset,
        })
    }
}


/// Serialized reference group selecting one logical `FSET` construction payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureFsetReferenceGroup {
    /// Two-reference group inside the byte-counted angle-bracket frame.
    First,
    /// Three-reference group following the angle-bracket frame.
    Second,
}

/// Exact counted nullable reference field carried by a `DELETE` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "reference::DeleteReferenceFieldWire", into = "reference::DeleteReferenceFieldWire")]
pub struct FeatureDeleteReferenceField {
    /// Globally unique field identity.
    pub id: String,
    /// Owning `DELETE` operation label.
    pub operation_label: String,
    /// Leading operation-local control byte.
    pub control: u8,
    /// Five nullable references with their resolved targets and source positions.
    pub references: [NullableConstructionReference; 5],
    /// Absolute source offset of the leading control byte.
    pub source_offset: u64,
}

/// Exact logical payload reconstructed from a complete non-null `DELETE` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDeleteConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `DELETE` operation label.
    pub operation_label: String,
    /// Complete five-slot reference field selecting the source blocks.
    pub reference_field: String,
    /// Ordered source blocks and the hash of their concatenated bytes.
    #[serde(flatten)]
    pub content: FeaturePayloadContent<[FeaturePayloadBlock; 5]>,
}

/// Ordered construction reference carried by a bounded pattern payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturePatternReference {
    /// Globally unique pattern-reference identity.
    pub id: String,
    /// Owning pattern operation label.
    pub operation_label: String,
    /// Exact byte layout that framed the reference field.
    pub layout: FeaturePatternReferenceLayout,
    /// Zero-based non-null slot order in the exact reference field.
    pub ordinal: u32,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub token: crate::om::reference_index::ReferenceIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

fn deserialize_reference_lane_count<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<usize, D::Error> {
    u8::deserialize(deserializer).map(usize::from)
}

/// Exact counted reference lane carried by a bounded `Pattern Feature` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeaturePatternCountedReferenceLaneWire",
    into = "FeaturePatternCountedReferenceLaneWire"
)]
pub struct FeaturePatternCountedReferenceLane {
    pub id: String,
    pub operation_label: String,
    pub references: Vec<FeatureDataBlockToken<Vec<u8>>>,
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeaturePatternCountedReferenceLaneWire {
    /// Globally unique lane identity.
    id: String,
    /// Owning `Pattern Feature` operation label.
    operation_label: String,
    /// Serialized count including the implicit owner slot.
    #[serde(deserialize_with = "deserialize_reference_lane_count")]
    declared_count: usize,
    /// Ordered serialized object indices.
    object_indices: Vec<u32>,
    /// Exact variable-width object-index tokens in lane order.
    raw_object_indices: Vec<Vec<u8>>,
    /// Independently resolved offset-store blocks; unresolved entries are `None`.
    data_blocks: Vec<Option<String>>,
    /// Absolute source offset of the opening `01, count` field.
    source_offset: u64,
    /// Absolute source offsets of the object-index tokens.
    object_index_source_offsets: Vec<u64>,
}

impl From<FeaturePatternCountedReferenceLane> for FeaturePatternCountedReferenceLaneWire {
    fn from(value: FeaturePatternCountedReferenceLane) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            declared_count: value.references.len() + 1,
            source_offset: value.source_offset,
            object_indices: value
                .references
                .iter()
                .map(|reference| reference.value)
                .collect(),
            raw_object_indices: value
                .references
                .iter()
                .map(|reference| reference.raw.clone())
                .collect(),
            data_blocks: value
                .references
                .iter()
                .map(|reference| reference.data_block.clone())
                .collect(),
            object_index_source_offsets: value
                .references
                .iter()
                .map(|reference| reference.source_offset)
                .collect(),
        }
    }
}

impl TryFrom<FeaturePatternCountedReferenceLaneWire> for FeaturePatternCountedReferenceLane {
    type Error = String;
    fn try_from(wire: FeaturePatternCountedReferenceLaneWire) -> Result<Self, Self::Error> {
        let count = wire.object_indices.len();
        if wire.declared_count != count + 1 {
            return Err("declared_count must equal the reference count plus the implicit owner".into());
        }
        if wire.raw_object_indices.len() != count
            || wire.data_blocks.len() != count
            || wire.object_index_source_offsets.len() != count
        {
            return Err("object_indices, raw_object_indices, data_blocks, and object_index_source_offsets must have equal lengths".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            source_offset: wire.source_offset,
            references: wire
                .object_indices
                .into_iter()
                .zip(wire.raw_object_indices)
                .zip(wire.data_blocks)
                .zip(wire.object_index_source_offsets)
                .map(
                    |(((value, raw), data_block), source_offset)| FeatureDataBlockToken {
                        value,
                        raw,
                        data_block,
                        source_offset,
                    },
                )
                .collect(),
        })
    }
}

/// Byte layout selected by a pattern construction-reference field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeaturePatternReferenceLayout {
    /// The `61`/`ff 00 ff 01`/`ff 62` graph framing.
    CanonicalGraph,
    /// The `3b`/`ff 00 01`/`ff 3c` graph framing.
    CompactGraph,
    /// The one-reference `Geometry Instance` framing.
    GeometryInstance,
}

/// Canonical printable string in a reconstructed pattern payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturePatternConstructionString {
    /// Globally unique string identity.
    pub id: String,
    /// Owning `Pattern Feature` or `Pattern Geometry` operation label.
    pub operation_label: String,
    /// Reconstructed pattern payload carrying the string.
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

/// Complete signed Q1.55 lane in a reconstructed pattern payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeaturePatternConstructionFixedLaneWire",
    into = "FeaturePatternConstructionFixedLaneWire"
)]
pub struct FeaturePatternConstructionFixedLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning `Pattern Feature` or `Pattern Geometry` operation label.
    pub operation_label: String,
    /// Reconstructed pattern payload carrying the lane.
    pub construction_payload: String,
    /// Zero-based lane order within the payload.
    pub ordinal: u32,
    /// Framed scalar run with absolute source locations.
    pub lane: FramedScalarRun<Q155LaneFrame, u64>,
    /// Absolute source offset of the fixed discriminator.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeaturePatternConstructionFixedLaneWire {
    /// Globally unique lane identity.
    id: String,
    /// Owning `Pattern Feature` or `Pattern Geometry` operation label.
    operation_label: String,
    /// Reconstructed pattern payload carrying the lane.
    construction_payload: String,
    /// Zero-based lane order within the payload.
    ordinal: u32,
    /// Ordered dimensionless Q1.55 values.
    values: Vec<f64>,
    /// Exact atom markers in value order.
    markers: Vec<u8>,
    /// Exact seven-byte two's-complement payloads.
    raw_values: Vec<[u8; 7]>,
    /// Payload-relative offset of the fixed discriminator.
    payload_offset: u64,
    /// Payload-relative offsets of the atom markers.
    value_payload_offsets: Vec<u64>,
    /// Absolute source offset of the fixed discriminator.
    source_offset: u64,
    /// Absolute source offsets of the atom markers.
    value_source_offsets: Vec<u64>,
}

impl From<FeaturePatternConstructionFixedLane> for FeaturePatternConstructionFixedLaneWire {
    fn from(record: FeaturePatternConstructionFixedLane) -> Self {
        Self {
            id: record.id,
            operation_label: record.operation_label,
            construction_payload: record.construction_payload,
            ordinal: record.ordinal,
            values: record.lane.iter().map(|(_, atom, _)| atom.scalar.value()).collect(),
            markers: record.lane.iter().map(|(_, atom, _)| atom.marker.byte()).collect(),
            raw_values: record.lane.iter().map(|(_, atom, _)| atom.scalar.raw()).collect(),
            payload_offset: record.lane.offset(),
            value_payload_offsets: record.lane.iter().map(|(offset, _, _)| offset).collect(),
            source_offset: record.source_offset,
            value_source_offsets: record.lane.iter().map(|(_, _, source)| *source).collect(),
        }
    }
}

impl TryFrom<FeaturePatternConstructionFixedLaneWire> for FeaturePatternConstructionFixedLane {
    type Error = String;
    fn try_from(wire: FeaturePatternConstructionFixedLaneWire) -> Result<Self, Self::Error> {
        let count = wire.values.len();
        if wire.markers.len() != count
            || wire.raw_values.len() != count
            || wire.value_payload_offsets.len() != count
            || wire.value_source_offsets.len() != count
        {
            return Err(
                "FeaturePatternConstructionFixedLane scalar columns must have equal lengths".into(),
            );
        }
        let values = wire.values.into_iter().zip(wire.markers).zip(wire.raw_values).zip(wire.value_source_offsets)
            .map(|(((value, marker), raw), source)| Ok((Q155Atom {
                marker: Q155Marker::read(marker).ok_or("markers must contain 48 or 176")?,
                scalar: Q155::from_wire(value, raw)?,
            }, source))).collect::<Result<Vec<_>, String>>()?;
        let lane = FramedScalarRun::new(Q155LaneFrame, wire.payload_offset, NonEmpty::new(values).ok_or("values must contain a Q1.55 atom")?)?;
        if !lane.iter().map(|(offset, _, _)| offset).eq(wire.value_payload_offsets) {
            return Err("value_payload_offsets must follow the contiguous Q1.55 atoms".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            construction_payload: wire.construction_payload,
            ordinal: wire.ordinal,
            lane,
            source_offset: wire.source_offset,
        })
    }
}

/// Byte layout selected by one exact pattern-transform lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeaturePatternTransformLayout {
    /// One shifted scalar per row and terminal mode `01`.
    ScalarRows,
    /// Four shifted binary64 values and one terminal value per row, with terminal mode `02`.
    WideRows,
}

/// Exact counted transform lane carried by a bounded pattern payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeaturePatternTransformLaneWire",
    into = "FeaturePatternTransformLaneWire"
)]
pub struct FeaturePatternTransformLane {
    pub id: String,
    pub operation_label: String,
    pub row_schema_index: NonZeroU8,
    pub rows: PatternRows<FeatureIndexToken, u64>,
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeaturePatternTransformLaneWire {
    /// Globally unique transform-lane identity.
    id: String,
    /// Owning `Pattern Feature` or `Pattern Geometry` operation label.
    operation_label: String,
    /// Schema index framing every row in the lane.
    row_schema_index: NonZeroU8,
    /// Row byte layout selected by the terminal mode.
    layout: FeaturePatternTransformLayout,
    /// Count including the implicit seed row.
    #[serde(deserialize_with = "deserialize_reference_lane_count")]
    declared_count: usize,
    /// Scalar encodings selected independently in row order.
    encodings: Vec<PatternScalarEncoding>,
    /// Ordered finite row scalars.
    values: Vec<f64>,
    /// Exact scalar encodings in row order.
    raw_values: Vec<Vec<u8>>,
    /// Ordered non-null compact selectors.
    selectors: Vec<u32>,
    /// Exact compact-index selector tokens in row order.
    raw_selectors: Vec<Vec<u8>>,
    /// Absolute source offset of the opening `01, count` field.
    source_offset: u64,
    /// Absolute source offsets of the scalar encodings.
    value_source_offsets: Vec<u64>,
    /// Absolute source offsets of the selector tokens.
    selector_source_offsets: Vec<u64>,
}

impl FeaturePatternTransformLaneWire {
    fn push_scalar(&mut self, encoding: PatternScalarEncoding, value: f64, raw: &[u8], source_offset: u64) {
        self.encodings.push(encoding);
        self.values.push(value);
        self.raw_values.push(raw.to_vec());
        self.value_source_offsets.push(source_offset);
    }

    fn push_selector(&mut self, selector: FeatureIndexToken) {
        self.selectors.push(selector.value);
        self.raw_selectors.push(selector.raw);
        self.selector_source_offsets.push(selector.source_offset);
    }
}

impl From<FeaturePatternTransformLane> for FeaturePatternTransformLaneWire {
    fn from(lane: FeaturePatternTransformLane) -> Self {
        let layout = match &lane.rows {
            PatternRows::Scalar(_) => FeaturePatternTransformLayout::ScalarRows,
            PatternRows::Wide(_) => FeaturePatternTransformLayout::WideRows,
        };
        let mut wire = Self {
            id: lane.id,
            operation_label: lane.operation_label,
            row_schema_index: lane.row_schema_index,
            layout,
            declared_count: usize::from(lane.rows.declared_count()),
            encodings: Vec::new(),
            values: Vec::new(),
            raw_values: Vec::new(),
            selectors: Vec::new(),
            raw_selectors: Vec::new(),
            source_offset: lane.source_offset,
            value_source_offsets: Vec::new(),
            selector_source_offsets: Vec::new(),
        };
        match lane.rows {
            PatternRows::Scalar(rows) => {
                for row in rows.into_vec() {
                    let encoding = match row.values.scalar {
                        ShiftedScalar::Binary32(_) => PatternScalarEncoding::Binary32,
                        ShiftedScalar::Binary64(_) => PatternScalarEncoding::Binary64,
                    };
                    wire.push_scalar(encoding, row.values.scalar.value(), row.values.scalar.raw(), row.values.offset);
                    wire.push_selector(row.selector);
                }
            }
            PatternRows::Wide(rows) => {
                for row in rows.into_vec() {
                    for value in row.values.first {
                        wire.push_scalar(PatternScalarEncoding::Binary64, value.scalar.value(), value.scalar.as_bytes(), value.offset);
                    }
                    let terminal = row.values.terminal;
                    wire.push_scalar(terminal.scalar.encoding(), terminal.scalar.value(), terminal.scalar.raw(), terminal.offset);
                    wire.push_selector(row.selector);
                }
            }
        }
        wire
    }
}

struct PatternScalarWire {
    encoding: PatternScalarEncoding,
    value: f64,
    raw: Vec<u8>,
    source_offset: u64,
}

impl PatternScalarWire {
    fn shifted(&self) -> Result<PatternValue<ShiftedScalar, u64>, String> {
        let scalar = ShiftedScalar::read(&self.raw).ok_or("raw_values must contain shifted scalar atoms")?;
        let encoding = match scalar {
            ShiftedScalar::Binary32(_) => PatternScalarEncoding::Binary32,
            ShiftedScalar::Binary64(_) => PatternScalarEncoding::Binary64,
        };
        if self.encoding != encoding || self.raw.len() != scalar.raw().len() || self.value.to_bits() != scalar.value().to_bits() {
            return Err("encodings and values must match each exact raw_values atom".into());
        }
        Ok(PatternValue { scalar, offset: self.source_offset })
    }

    fn binary64(&self) -> Result<PatternValue<ShiftedBinary64, u64>, String> {
        if self.encoding != PatternScalarEncoding::Binary64 {
            return Err("encodings must select binary64 for the first four wide-row scalars".into());
        }
        let raw = self.raw.as_slice().try_into().map_err(|_| "raw_values wide-row scalars must contain eight bytes")?;
        let scalar = ShiftedBinary64::from_wire(self.value, raw)
            .map_err(|error| format!("values/raw_values: {error}"))?;
        Ok(PatternValue { scalar, offset: self.source_offset })
    }

    fn terminal(&self) -> Result<PatternValue<PatternTerminal, u64>, String> {
        let scalar = PatternTerminal::read(&self.raw).ok_or("raw_values wide-row terminal must contain exact one or binary32")?;
        if self.encoding != scalar.encoding() || self.raw.len() != scalar.raw().len() || self.value.to_bits() != scalar.value().to_bits() {
            return Err("encodings and values must match the exact raw_values terminal atom".into());
        }
        Ok(PatternValue { scalar, offset: self.source_offset })
    }
}

impl TryFrom<FeaturePatternTransformLaneWire> for FeaturePatternTransformLane {
    type Error = String;

    fn try_from(wire: FeaturePatternTransformLaneWire) -> Result<Self, Self::Error> {
        if wire.declared_count != wire.selectors.len() + 1 {
            return Err("declared_count must equal the row count plus the implicit seed".into());
        }
        let width = match wire.layout {
            FeaturePatternTransformLayout::ScalarRows => 1,
            FeaturePatternTransformLayout::WideRows => 5,
        };
        let count = wire.values.len();
        if wire.selectors.len().checked_mul(width) != Some(count)
            || wire.encodings.len() != count
            || wire.raw_values.len() != count
            || wire.value_source_offsets.len() != count
            || wire.raw_selectors.len() != wire.selectors.len()
            || wire.selector_source_offsets.len() != wire.selectors.len()
        {
            return Err("pattern transform columns must contain complete rows for the selected layout".into());
        }
        let values = wire.encodings.into_iter().zip(wire.values).zip(wire.raw_values).zip(wire.value_source_offsets)
            .map(|(((encoding, value), raw), source_offset)| PatternScalarWire { encoding, value, raw, source_offset })
            .collect::<Vec<_>>();
        let selectors = wire.selectors.into_iter().zip(wire.raw_selectors).zip(wire.selector_source_offsets)
            .map(|((value, raw), source_offset)| FeatureIndexToken { value, raw, source_offset });
        let rows = match wire.layout {
            FeaturePatternTransformLayout::ScalarRows => {
                let rows = values.as_chunks::<1>().0.iter().zip(selectors)
                    .map(|([value], selector)| Ok(PatternRow { values: value.shifted()?, selector }))
                    .collect::<Result<Vec<_>, String>>()?;
                PatternRows::Scalar(BranchItems::new(rows)?)
            }
            FeaturePatternTransformLayout::WideRows => {
                let rows = values.as_chunks::<5>().0.iter().zip(selectors)
                    .map(|([a, b, c, d, terminal], selector)| Ok(PatternRow {
                        values: PatternWideValues {
                            first: [a.binary64()?, b.binary64()?, c.binary64()?, d.binary64()?],
                            terminal: terminal.terminal()?,
                        },
                        selector,
                    }))
                    .collect::<Result<Vec<_>, String>>()?;
                PatternRows::Wide(BranchItems::new(rows)?)
            }
        };
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            row_schema_index: wire.row_schema_index,
            rows,
            source_offset: wire.source_offset,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureIndexToken {
    pub value: u32,
    pub raw: Vec<u8>,
    pub source_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureMultiInstanceOutputRow {
    pub value: u32,
    pub raw: Vec<u8>,
    pub ordinal: u8,
    pub source_offset: u64,
}

/// Exact counted instance-output lane carried by a bounded operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureMultiInstanceOutputLaneWire",
    into = "FeatureMultiInstanceOutputLaneWire"
)]
pub struct FeatureMultiInstanceOutputLane {
    /// Globally unique output-lane identity.
    pub id: String,
    /// Owning `Multi Instance Output` operation label.
    pub operation_label: String,
    /// Ordered complete source tokens.
    pub rows: Vec<FeatureMultiInstanceOutputRow>,
    /// Ordered complete source tokens.
    pub trailing_references: Vec<FeatureIndexToken>,
    /// Absolute source offset of the opening `25 01, count` field.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureMultiInstanceOutputLaneWire {
    /// Globally unique output-lane identity.
    id: String,
    /// Owning `Multi Instance Output` operation label.
    operation_label: String,
    /// Count including the implicit seed row.
    #[serde(deserialize_with = "deserialize_reference_lane_count")]
    declared_count: usize,
    /// Ordered non-null compact selectors.
    selectors: Vec<u32>,
    /// Exact compact-index selector tokens in row order.
    raw_selectors: Vec<Vec<u8>>,
    /// Ordered serialized instance ordinals.
    ordinals: Vec<u8>,
    /// Ordered serialized row indices.
    row_indices: Vec<usize>,
    /// Count including the implicit seed instance.
    #[serde(deserialize_with = "deserialize_reference_lane_count")]
    instance_count: usize,
    /// Ordered non-null trailing object indices.
    trailing_object_indices: Vec<u32>,
    /// Exact trailing object-index tokens in row order.
    raw_trailing_object_indices: Vec<Vec<u8>>,
    /// Absolute source offset of the opening `25 01, count` field.
    source_offset: u64,
    /// Absolute source offsets of the selector tokens.
    selector_source_offsets: Vec<u64>,
    /// Absolute source offsets of the trailing object-index tokens.
    trailing_object_index_source_offsets: Vec<u64>,
}

impl From<FeatureMultiInstanceOutputLane> for FeatureMultiInstanceOutputLaneWire {
    fn from(lane: FeatureMultiInstanceOutputLane) -> Self {
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            declared_count: lane.rows.len() + 1,
            instance_count: lane.trailing_references.len() + 1,
            source_offset: lane.source_offset,
            selectors: lane.rows.iter().map(|token| token.value).collect(),
            raw_selectors: lane.rows.iter().map(|token| token.raw.clone()).collect(),
            ordinals: lane.rows.iter().map(|token| token.ordinal).collect(),
            row_indices: (2..lane.rows.len() + 2).collect(),
            selector_source_offsets: lane.rows.iter().map(|token| token.source_offset).collect(),
            trailing_object_indices: lane
                .trailing_references
                .iter()
                .map(|token| token.value)
                .collect(),
            raw_trailing_object_indices: lane
                .trailing_references
                .iter()
                .map(|token| token.raw.clone())
                .collect(),
            trailing_object_index_source_offsets: lane
                .trailing_references
                .iter()
                .map(|token| token.source_offset)
                .collect(),
        }
    }
}

impl TryFrom<FeatureMultiInstanceOutputLaneWire> for FeatureMultiInstanceOutputLane {
    type Error = String;
    fn try_from(wire: FeatureMultiInstanceOutputLaneWire) -> Result<Self, Self::Error> {
        if wire.declared_count != wire.selectors.len() + 1 {
            return Err("declared_count must equal the row count plus the implicit seed".into());
        }
        if wire.row_indices.iter().copied().ne(2..wire.selectors.len() + 2) {
            return Err("row_indices must enumerate rows from two".into());
        }
        if wire.instance_count != wire.trailing_object_indices.len() + 1 {
            return Err("instance_count must equal the trailing reference count plus the implicit seed".into());
        }
        if wire.raw_selectors.len() != wire.selectors.len()
            || wire.ordinals.len() != wire.selectors.len()
            || wire.selector_source_offsets.len() != wire.selectors.len()
        {
            return Err(
                "FeatureMultiInstanceOutputLane rows columns must have equal lengths".into(),
            );
        }
        if wire.raw_trailing_object_indices.len() != wire.trailing_object_indices.len()
            || wire.trailing_object_index_source_offsets.len() != wire.trailing_object_indices.len()
        {
            return Err("FeatureMultiInstanceOutputLane trailing_references columns must have equal lengths".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            source_offset: wire.source_offset,
            rows: wire
                .selectors
                .into_iter()
                .zip(wire.raw_selectors)
                .zip(wire.ordinals)
                .zip(wire.selector_source_offsets)
                .map(|(((value, raw), ordinal), source_offset)| {
                    FeatureMultiInstanceOutputRow {
                        value,
                        raw,
                        ordinal,
                        source_offset,
                    }
                })
                .collect(),
            trailing_references: wire
                .trailing_object_indices
                .into_iter()
                .zip(wire.raw_trailing_object_indices)
                .zip(wire.trailing_object_index_source_offsets)
                .map(|((value, raw), source_offset)| FeatureIndexToken {
                    value,
                    raw,
                    source_offset,
                })
                .collect(),
        })
    }
}

/// Exact counted selector lane carried by an identical-instance output payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureIdenticalInstanceOutputLaneWire",
    into = "FeatureIdenticalInstanceOutputLaneWire"
)]
pub struct FeatureIdenticalInstanceOutputLane {
    /// Globally unique output-lane identity.
    pub id: String,
    /// Owning `IDENTICAL INSTANCE OUTPUT` operation label.
    pub operation_label: String,
    /// Schema index preceding the count field.
    pub leading_schema_index: u8,
    /// Schema index framing the serialized count.
    pub count_schema_index: crate::om::IdenticalInstanceSchemaIndex,
    /// Ordered complete source tokens.
    pub selectors: Vec<FeatureIndexToken>,
    /// Absolute source offset of the leading schema index.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureIdenticalInstanceOutputLaneWire {
    /// Globally unique output-lane identity.
    id: String,
    /// Owning `IDENTICAL INSTANCE OUTPUT` operation label.
    operation_label: String,
    /// Schema index preceding the count field.
    leading_schema_index: u8,
    /// Schema index framing the serialized count.
    count_schema_index: u8,
    /// Three consecutive schema indices framing every selector row.
    row_schema_indices: [u8; 3],
    /// Count including the implicit owner row.
    #[serde(deserialize_with = "deserialize_reference_lane_count")]
    declared_count: usize,
    /// Ordered non-null compact selectors.
    selectors: Vec<u32>,
    /// Exact compact-index selector tokens in row order.
    raw_selectors: Vec<Vec<u8>>,
    /// Absolute source offset of the leading schema index.
    source_offset: u64,
    /// Absolute source offsets of the selector tokens.
    selector_source_offsets: Vec<u64>,
}

impl From<FeatureIdenticalInstanceOutputLane> for FeatureIdenticalInstanceOutputLaneWire {
    fn from(lane: FeatureIdenticalInstanceOutputLane) -> Self {
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            leading_schema_index: lane.leading_schema_index,
            count_schema_index: lane.count_schema_index.value(),
            row_schema_indices: lane.count_schema_index.row_indices(),
            declared_count: lane.selectors.len() + 1,
            source_offset: lane.source_offset,
            selectors: lane.selectors.iter().map(|token| token.value).collect(),
            raw_selectors: lane
                .selectors
                .iter()
                .map(|token| token.raw.clone())
                .collect(),
            selector_source_offsets: lane
                .selectors
                .iter()
                .map(|token| token.source_offset)
                .collect(),
        }
    }
}

impl TryFrom<FeatureIdenticalInstanceOutputLaneWire> for FeatureIdenticalInstanceOutputLane {
    type Error = String;
    fn try_from(wire: FeatureIdenticalInstanceOutputLaneWire) -> Result<Self, Self::Error> {
        let count_schema_index = crate::om::IdenticalInstanceSchemaIndex::new(wire.count_schema_index)
            .ok_or("count_schema_index must leave room for three row schema indices")?;
        if wire.row_schema_indices != count_schema_index.row_indices() {
            return Err("row_schema_indices must follow count_schema_index consecutively".into());
        }
        if wire.declared_count != wire.selectors.len() + 1 {
            return Err("declared_count must equal the row count plus the implicit seed".into());
        }
        if wire.raw_selectors.len() != wire.selectors.len()
            || wire.selector_source_offsets.len() != wire.selectors.len()
        {
            return Err(
                "FeatureIdenticalInstanceOutputLane selectors columns must have equal lengths"
                    .into(),
            );
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            leading_schema_index: wire.leading_schema_index,
            count_schema_index,
            source_offset: wire.source_offset,
            selectors: wire
                .selectors
                .into_iter()
                .zip(wire.raw_selectors)
                .zip(wire.selector_source_offsets)
                .map(|((value, raw), source_offset)| FeatureIndexToken {
                    value,
                    raw,
                    source_offset,
                })
                .collect(),
        })
    }
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
    pub mode: u8,
    /// Absolute file offset of the reference width marker.
    pub source_offset: u64,
}

/// Exact cross-block scalar lane selected by a point-construction header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeaturePointConstructionScalarLaneWire",
    into = "FeaturePointConstructionScalarLaneWire"
)]
pub struct FeaturePointConstructionScalarLane {
    pub id: String,
    pub operation_label: String,
    pub construction_header: String,
    pub data_blocks: [String; 2],
    pub values: [FeatureBinary64ScalarToken; 6],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureBinary64ScalarToken {
    pub scalar: ShiftedBinary64,
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeaturePointConstructionScalarLaneWire {
    /// Globally unique point-construction scalar-lane identity.
    id: String,
    /// Owning `POINT` operation label.
    operation_label: String,
    /// Header selecting this lane.
    construction_header: String,
    /// Preceding and target data blocks in byte order.
    data_blocks: [String; 2],
    /// Six finite scalar values in byte order.
    values: [f64; 6],
    /// Exact shifted-binary64 encodings in byte order.
    raw_values: [[u8; 8]; 6],
    /// Absolute file offsets of the six scalar markers.
    source_offsets: [u64; 6],
}

impl From<FeaturePointConstructionScalarLane> for FeaturePointConstructionScalarLaneWire {
    fn from(lane: FeaturePointConstructionScalarLane) -> Self {
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            construction_header: lane.construction_header,
            data_blocks: lane.data_blocks,
            values: lane.values.map(|token| token.scalar.value()),
            raw_values: lane.values.map(|token| token.scalar.raw()),
            source_offsets: lane.values.map(|token| token.source_offset),
        }
    }
}

impl TryFrom<FeaturePointConstructionScalarLaneWire> for FeaturePointConstructionScalarLane {
    type Error = String;

    fn try_from(wire: FeaturePointConstructionScalarLaneWire) -> Result<Self, Self::Error> {
        let [a, b, c, d, e, f] = std::array::from_fn::<_, 6, _>(|i| {
            ShiftedBinary64::from_wire(wire.values[i], wire.raw_values[i])
                .map_err(|error| format!("values/raw_values[{i}]: {error}"))
        });
        let scalars = [a?, b?, c?, d?, e?, f?];
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            construction_header: wire.construction_header,
            data_blocks: wire.data_blocks,
            values: std::array::from_fn(|slot| FeatureBinary64ScalarToken {
                scalar: scalars[slot],
                source_offset: wire.source_offsets[slot],
            }),
        })
    }
}

/// Ordered construction reference carried by a bounded draft-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionReference {
    /// Globally unique draft-construction-reference identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Zero-based slot order in the exact construction graph.
    pub ordinal: u32,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub token: crate::om::reference_index::ReferenceIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Counted compact-index lane preceding a bounded draft construction graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureDraftConstructionIndexLaneWire",
    into = "FeatureDraftConstructionIndexLaneWire"
)]
pub struct FeatureDraftConstructionIndexLane {
    pub id: String,
    pub operation_label: String,
    pub indices: FeatureDraftConstructionIndices,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureDraftConstructionIndices {
    Unresolved(Vec<FeatureIndexToken>),
    Resolved(Vec<FeatureResolvedIndexToken>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureResolvedIndexToken {
    pub token: FeatureIndexToken,
    pub data_block: String,
}

#[derive(Serialize, Deserialize)]
struct FeatureDraftConstructionIndexLaneWire {
    /// Globally unique lane identity.
    id: String,
    /// Owning `DRAFT` operation label.
    operation_label: String,
    /// Serialized count including the omitted lane owner.
    #[serde(deserialize_with = "deserialize_reference_lane_count")]
    declared_count: usize,
    /// Non-null compact indices in serialized order.
    indices: Vec<u32>,
    /// Exact compact-index tokens in serialized order.
    raw_indices: Vec<Vec<u8>>,
    /// Same-store native blocks when the complete lane and graph select one store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data_blocks: Option<Vec<String>>,
    /// Absolute source offsets of the compact-index tokens.
    source_offsets: Vec<u64>,
}

impl From<FeatureDraftConstructionIndexLane> for FeatureDraftConstructionIndexLaneWire {
    fn from(lane: FeatureDraftConstructionIndexLane) -> Self {
        let (tokens, data_blocks): (Vec<_>, _) = match lane.indices {
            FeatureDraftConstructionIndices::Unresolved(tokens) => (tokens, None),
            FeatureDraftConstructionIndices::Resolved(tokens) => {
                let (tokens, blocks) = tokens
                    .into_iter()
                    .map(|row| (row.token, row.data_block))
                    .unzip();
                (tokens, Some(blocks))
            }
        };
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            declared_count: tokens.len() + 1,
            indices: tokens.iter().map(|token| token.value).collect(),
            raw_indices: tokens.iter().map(|token| token.raw.clone()).collect(),
            data_blocks,
            source_offsets: tokens.iter().map(|token| token.source_offset).collect(),
        }
    }
}

impl TryFrom<FeatureDraftConstructionIndexLaneWire> for FeatureDraftConstructionIndexLane {
    type Error = String;

    fn try_from(wire: FeatureDraftConstructionIndexLaneWire) -> Result<Self, Self::Error> {
        let count = wire.indices.len();
        if wire.declared_count != count + 1 {
            return Err("declared_count must equal the reference count plus the implicit owner".into());
        }
        if wire.raw_indices.len() != count
            || wire.source_offsets.len() != count
            || wire
                .data_blocks
                .as_ref()
                .is_some_and(|blocks| blocks.len() != count)
        {
            return Err("draft index lane columns must have equal lengths".into());
        }
        let tokens = wire
            .indices
            .into_iter()
            .zip(wire.raw_indices)
            .zip(wire.source_offsets)
            .map(|((value, raw), source_offset)| FeatureIndexToken {
                value,
                raw,
                source_offset,
            });
        let indices = match wire.data_blocks {
            None => FeatureDraftConstructionIndices::Unresolved(tokens.collect()),
            Some(blocks) => FeatureDraftConstructionIndices::Resolved(
                tokens
                    .zip(blocks)
                    .map(|(token, data_block)| FeatureResolvedIndexToken { token, data_block })
                    .collect(),
            ),
        };
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            indices,
        })
    }
}

/// Exact logical payload reconstructed from the ordered draft construction graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionGraphPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Counted index lane establishing the common offset store.
    pub index_lane: String,
    /// Ordered construction-reference records.
    pub construction_references: [String; 4],
    /// Ordered source blocks and the hash of their concatenated bytes.
    #[serde(flatten)]
    pub content: FeaturePayloadContent<[FeaturePayloadBlock; 4]>,
}

/// Complete signed Q1.55 lane in a reconstructed draft graph payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureDraftConstructionFixedLaneWire",
    into = "FeatureDraftConstructionFixedLaneWire"
)]
pub struct FeatureDraftConstructionFixedLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Reconstructed graph payload carrying the lane.
    pub graph_payload: String,
    /// Zero-based lane order in the reconstructed payload.
    pub ordinal: u32,
    /// Framed scalar run with absolute source locations.
    pub lane: FramedScalarRun<Q155LaneFrame, u64>,
    /// Absolute source offset of the fixed discriminator.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureDraftConstructionFixedLaneWire {
    /// Globally unique lane identity.
    id: String,
    /// Owning `DRAFT` operation label.
    operation_label: String,
    /// Reconstructed graph payload carrying the lane.
    graph_payload: String,
    /// Zero-based lane order in the reconstructed payload.
    ordinal: u32,
    /// Ordered dimensionless Q1.55 values.
    values: Vec<f64>,
    /// Exact atom markers in value order.
    markers: Vec<u8>,
    /// Exact seven-byte two's-complement payloads.
    raw_values: Vec<[u8; 7]>,
    /// Payload-relative offset of the fixed discriminator.
    payload_offset: u64,
    /// Payload-relative offsets of the atom markers.
    value_payload_offsets: Vec<u64>,
    /// Absolute source offset of the fixed discriminator.
    source_offset: u64,
    /// Absolute source offsets of the atom markers.
    value_source_offsets: Vec<u64>,
}

impl From<FeatureDraftConstructionFixedLane> for FeatureDraftConstructionFixedLaneWire {
    fn from(record: FeatureDraftConstructionFixedLane) -> Self {
        Self {
            id: record.id,
            operation_label: record.operation_label,
            graph_payload: record.graph_payload,
            ordinal: record.ordinal,
            values: record.lane.iter().map(|(_, atom, _)| atom.scalar.value()).collect(),
            markers: record.lane.iter().map(|(_, atom, _)| atom.marker.byte()).collect(),
            raw_values: record.lane.iter().map(|(_, atom, _)| atom.scalar.raw()).collect(),
            payload_offset: record.lane.offset(),
            value_payload_offsets: record.lane.iter().map(|(offset, _, _)| offset).collect(),
            source_offset: record.source_offset,
            value_source_offsets: record.lane.iter().map(|(_, _, source)| *source).collect(),
        }
    }
}

impl TryFrom<FeatureDraftConstructionFixedLaneWire> for FeatureDraftConstructionFixedLane {
    type Error = String;
    fn try_from(wire: FeatureDraftConstructionFixedLaneWire) -> Result<Self, Self::Error> {
        let count = wire.values.len();
        if wire.markers.len() != count
            || wire.raw_values.len() != count
            || wire.value_payload_offsets.len() != count
            || wire.value_source_offsets.len() != count
        {
            return Err(
                "FeatureDraftConstructionFixedLane scalar columns must have equal lengths".into(),
            );
        }
        let values = wire.values.into_iter().zip(wire.markers).zip(wire.raw_values).zip(wire.value_source_offsets)
            .map(|(((value, marker), raw), source)| Ok((Q155Atom {
                marker: Q155Marker::read(marker).ok_or("markers must contain 48 or 176")?,
                scalar: Q155::from_wire(value, raw)?,
            }, source))).collect::<Result<Vec<_>, String>>()?;
        let lane = FramedScalarRun::new(Q155LaneFrame, wire.payload_offset, NonEmpty::new(values).ok_or("values must contain a Q1.55 atom")?)?;
        if !lane.iter().map(|(offset, _, _)| offset).eq(wire.value_payload_offsets) {
            return Err("value_payload_offsets must follow the contiguous Q1.55 atoms".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            graph_payload: wire.graph_payload,
            ordinal: wire.ordinal,
            lane,
            source_offset: wire.source_offset,
        })
    }
}

/// Complete shifted-binary32 lane in a reconstructed draft graph payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureDraftConstructionBinary32LaneWire",
    into = "FeatureDraftConstructionBinary32LaneWire"
)]
pub struct FeatureDraftConstructionBinary32Lane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Reconstructed graph payload carrying the lane.
    pub graph_payload: String,
    /// Zero-based lane order in the reconstructed payload.
    pub ordinal: u32,
    /// Typed branch and contiguous atoms with absolute source locations.
    pub lane: FramedScalarRun<DraftBinary32Branch, u64>,
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureDraftConstructionBinary32LaneWire {
    /// Globally unique lane identity.
    id: String,
    /// Owning `DRAFT` operation label.
    operation_label: String,
    /// Reconstructed graph payload carrying the lane.
    graph_payload: String,
    /// Zero-based lane order in the reconstructed payload.
    ordinal: u32,
    /// Exact discriminator selecting the lane form.
    discriminator: [u8; 18],
    /// Exact `03` or `04` branch byte.
    branch: u8,
    /// Ordered finite shifted-IEEE binary32 values.
    values: Vec<f64>,
    /// Exact four-byte shifted encodings.
    raw_values: Vec<[u8; 4]>,
    /// Payload-relative offset of the discriminator.
    payload_offset: u64,
    /// Payload-relative offsets of the scalar encodings.
    value_payload_offsets: Vec<u64>,
    /// Absolute source offset of the discriminator.
    source_offset: u64,
    /// Absolute source offsets of the scalar encodings.
    value_source_offsets: Vec<u64>,
}

impl From<FeatureDraftConstructionBinary32Lane> for FeatureDraftConstructionBinary32LaneWire {
    fn from(record: FeatureDraftConstructionBinary32Lane) -> Self {
        Self {
            id: record.id,
            operation_label: record.operation_label,
            graph_payload: record.graph_payload,
            ordinal: record.ordinal,
            discriminator: record.lane.form().discriminator(),
            branch: u8::from(record.lane.form()),
            values: record.lane.iter().map(|(_, scalar, _)| scalar.value()).collect(),
            raw_values: record.lane.iter().map(|(_, scalar, _)| scalar.raw()).collect(),
            payload_offset: record.lane.offset(),
            value_payload_offsets: record.lane.iter().map(|(offset, _, _)| offset).collect(),
            source_offset: record.source_offset,
            value_source_offsets: record.lane.iter().map(|(_, _, source)| *source).collect(),
        }
    }
}

impl TryFrom<FeatureDraftConstructionBinary32LaneWire> for FeatureDraftConstructionBinary32Lane {
    type Error = String;
    fn try_from(wire: FeatureDraftConstructionBinary32LaneWire) -> Result<Self, Self::Error> {
        let branch = DraftBinary32Branch::try_from(wire.branch)?;
        if wire.discriminator != branch.discriminator() {
            return Err("discriminator must match branch".into());
        }
        let count = wire.values.len();
        if wire.raw_values.len() != count
            || wire.value_payload_offsets.len() != count
            || wire.value_source_offsets.len() != count
        {
            return Err(
                "FeatureDraftConstructionBinary32Lane scalar columns must have equal lengths"
                    .into(),
            );
        }
        let values = wire.values.into_iter().zip(wire.raw_values).zip(wire.value_source_offsets)
            .map(|((value, raw), source)| Ok((ShiftedBinary32::from_wire(value, &raw)?, source)))
            .collect::<Result<Vec<_>, String>>()?;
        let lane = FramedScalarRun::new(branch, wire.payload_offset, NonEmpty::new(values).ok_or("values must contain a binary32 atom")?)?;
        if !lane.iter().map(|(offset, _, _)| offset).eq(wire.value_payload_offsets) {
            return Err("value_payload_offsets must follow the contiguous binary32 atoms".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            graph_payload: wire.graph_payload,
            ordinal: wire.ordinal,
            lane,
            source_offset: wire.source_offset,
        })
    }
}

/// Canonical printable string in a reconstructed draft graph payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionGraphString {
    /// Globally unique string identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Reconstructed graph payload carrying the string.
    pub graph_payload: String,
    /// Zero-based string order in the reconstructed payload.
    pub ordinal: u32,
    /// Exact printable value.
    pub value: PrintableString<String>,
    /// Payload-relative offset of the `66 32 03` marker.
    pub payload_offset: u64,
    /// Absolute source offset of the marker.
    pub source_offset: u64,
}

/// Complete identity frame in a reconstructed draft construction payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FeatureDraftConstructionIdentityFrameWire", into = "FeatureDraftConstructionIdentityFrameWire")]
pub struct FeatureDraftConstructionIdentityFrame {
    /// Globally unique frame identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the frame.
    pub draft_construction_payload: String,
    /// Zero-based frame order in the reconstructed payload.
    pub ordinal: u32,
    /// Exact prefix tokens, identity, and bounded payload position.
    pub frame: DraftIdentityFrame,
    /// Absolute source offset of the opening marker.
    pub source_offset: u64,
    /// Absolute source offset of the identity.
    pub identity_source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureDraftConstructionIdentityFrameWire {
    /// Globally unique frame identity.
    id: String,
    /// Owning `DRAFT` operation label.
    operation_label: String,
    /// Reconstructed payload carrying the frame.
    draft_construction_payload: String,
    /// Zero-based frame order in the reconstructed payload.
    ordinal: u32,
    /// Exact bytes from the opening marker through the identity introducer.
    prefix: Vec<u8>,
    /// Typed frame form selected by the exact prefix.
    form: DraftIdentityForm,
    /// Nonempty lowercase hexadecimal identity.
    identity: String,
    /// Payload-relative offset of the opening marker.
    payload_offset: u64,
    /// Payload-relative identity offset.
    identity_payload_offset: u64,
    /// Absolute source offset of the opening marker.
    source_offset: u64,
    /// Absolute source offset of the identity.
    identity_source_offset: u64,
}

impl From<FeatureDraftConstructionIdentityFrame> for FeatureDraftConstructionIdentityFrameWire {
    fn from(value: FeatureDraftConstructionIdentityFrame) -> Self {
        let identity_payload_offset = value.frame.identity_offset();
        Self {
            id: value.id,
            operation_label: value.operation_label,
            draft_construction_payload: value.draft_construction_payload,
            ordinal: value.ordinal,
            prefix: value.frame.prefix(),
            form: value.frame.form(),
            identity: value.frame.identity().to_owned(),
            payload_offset: value.frame.offset(),
            identity_payload_offset,
            source_offset: value.source_offset,
            identity_source_offset: value.identity_source_offset,
        }
    }
}

impl TryFrom<FeatureDraftConstructionIdentityFrameWire> for FeatureDraftConstructionIdentityFrame {
    type Error = String;

    fn try_from(wire: FeatureDraftConstructionIdentityFrameWire) -> Result<Self, Self::Error> {
        let frame = DraftIdentityFrame::from_wire(&wire.prefix, wire.form, wire.identity, wire.payload_offset)
            .map_err(str::to_owned)?;
        if frame.identity_offset() != wire.identity_payload_offset {
            return Err("identity_payload_offset must equal payload_offset plus prefix length".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            draft_construction_payload: wire.draft_construction_payload,
            ordinal: wire.ordinal,
            frame,
            source_offset: wire.source_offset,
            identity_source_offset: wire.identity_source_offset,
        })
    }
}


/// End-anchored compact-index lane in a bounded draft construction payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureDraftConstructionTerminalLaneWire",
    into = "FeatureDraftConstructionTerminalLaneWire"
)]
pub struct FeatureDraftConstructionTerminalLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Two non-null compact indices in serialized order.
    pub indices: [u32; 2],
    /// Exact two-byte compact-index tokens in serialized order.
    pub raw_indices: [[u8; 2]; 2],
    /// Exact uninterpreted bytes preceding the terminal zero.
    pub tail: [u8; 3],
    /// Absolute source offsets of the compact-index tokens.
    pub index_source_offsets: [u64; 2],
}

#[derive(Serialize, Deserialize)]
struct FeatureDraftConstructionTerminalLaneWire {
    /// Globally unique lane identity.
    id: String,
    /// Owning `DRAFT` operation label.
    operation_label: String,
    /// Two non-null compact indices in serialized order.
    indices: [u32; 2],
    /// Exact two-byte compact-index tokens in serialized order.
    raw_indices: [[u8; 2]; 2],
    /// Exact uninterpreted bytes preceding the terminal zero.
    tail: [u8; 3],
    /// Absolute source offsets of the compact-index tokens.
    index_source_offsets: [u64; 2],
    /// Absolute source offset of the first compact-index token.
    source_offset: u64,
}

impl From<FeatureDraftConstructionTerminalLane> for FeatureDraftConstructionTerminalLaneWire {
    fn from(lane: FeatureDraftConstructionTerminalLane) -> Self {
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            indices: lane.indices,
            raw_indices: lane.raw_indices,
            tail: lane.tail,
            index_source_offsets: lane.index_source_offsets,
            source_offset: lane.index_source_offsets[0],
        }
    }
}

impl TryFrom<FeatureDraftConstructionTerminalLaneWire> for FeatureDraftConstructionTerminalLane {
    type Error = String;

    fn try_from(wire: FeatureDraftConstructionTerminalLaneWire) -> Result<Self, Self::Error> {
        if wire.source_offset != wire.index_source_offsets[0] {
            return Err("source_offset must equal the first index_source_offsets entry".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            indices: wire.indices,
            raw_indices: wire.raw_indices,
            tail: wire.tail,
            index_source_offsets: wire.index_source_offsets,
        })
    }
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
    pub token: crate::om::reference_index::ReferenceIndexToken,
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

/// One exact counted branch in a `THRU_CURVE` construction group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FeatureThruCurveConstructionBranchWire", into = "FeatureThruCurveConstructionBranchWire")]
pub struct FeatureThruCurveConstructionBranch {
    /// Zero-based branch order.
    pub ordinal: u32,
    /// Serialized nonzero branch mode.
    pub mode: NonZeroU8,
    /// Ordered nonterminal references.
    pub members: ThruCurveBranchItems<FeatureSurfaceBranchReference>,
    /// Terminal reference.
    pub terminal: FeatureSurfaceBranchReference,
    /// Exact two-byte branch suffix.
    pub suffix: ThruCurveBranchSuffix,
    /// Absolute source offset of the mode byte.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureThruCurveConstructionBranchWire {
    ordinal: u32,
    mode: NonZeroU8,
    declared_count: u8,
    state_lane: Vec<u8>,
    members: Vec<FeatureSurfaceBranchReference>,
    terminal: FeatureSurfaceBranchReference,
    suffix: ThruCurveBranchSuffix,
    source_offset: u64,
}

impl From<FeatureThruCurveConstructionBranch> for FeatureThruCurveConstructionBranchWire {
    fn from(value: FeatureThruCurveConstructionBranch) -> Self {
        Self {
            ordinal: value.ordinal,
            mode: value.mode,
            declared_count: value.members.declared_count(),
            state_lane: value.members.state_lane(),
            members: value.members.into_members(),
            terminal: value.terminal,
            suffix: value.suffix,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeatureThruCurveConstructionBranchWire> for FeatureThruCurveConstructionBranch {
    type Error = String;
    fn try_from(wire: FeatureThruCurveConstructionBranchWire) -> Result<Self, Self::Error> {
        if usize::from(wire.declared_count) != wire.members.len() + 1 {
            return Err("declared_count must equal members length plus one".to_owned());
        }
        Ok(Self {
            ordinal: wire.ordinal,
            mode: wire.mode,
            members: ThruCurveBranchItems::from_parts(wire.members, &wire.state_lane)?,
            terminal: wire.terminal,
            suffix: wire.suffix,
            source_offset: wire.source_offset,
        })
    }
}


/// Exact counted branch group after a `THRU_CURVE` construction envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FeatureThruCurveConstructionBranchGroupWire", into = "FeatureThruCurveConstructionBranchGroupWire")]
pub struct FeatureThruCurveConstructionBranchGroup {
    /// Globally unique branch-group identity.
    pub id: String,
    /// Owning `THRU_CURVE` operation label.
    pub operation_label: String,
    /// Ordered explicit branches.
    pub branches: BranchItems<FeatureThruCurveConstructionBranch>,
    /// Exact group terminator selected by the schema generation.
    pub terminator: ThruCurveGroupTerminator,
    /// Absolute source offset of the group count.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureThruCurveConstructionBranchGroupWire {
    id: String,
    operation_label: String,
    declared_count: u8,
    branches: BranchItems<FeatureThruCurveConstructionBranch>,
    terminator: ThruCurveGroupTerminator,
    source_offset: u64,
}

impl From<FeatureThruCurveConstructionBranchGroup> for FeatureThruCurveConstructionBranchGroupWire {
    fn from(value: FeatureThruCurveConstructionBranchGroup) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            declared_count: value.branches.declared_count(),
            branches: value.branches,
            terminator: value.terminator,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeatureThruCurveConstructionBranchGroupWire> for FeatureThruCurveConstructionBranchGroup {
    type Error = String;
    fn try_from(wire: FeatureThruCurveConstructionBranchGroupWire) -> Result<Self, Self::Error> {
        if wire.declared_count != wire.branches.declared_count() {
            return Err("declared_count must equal branches length plus one".to_owned());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            branches: wire.branches,
            terminator: wire.terminator,
            source_offset: wire.source_offset,
        })
    }
}


/// Exact leading construction branch in a `SWP104` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FeatureSwp104LeadingBranchWire", into = "FeatureSwp104LeadingBranchWire")]
pub struct FeatureSwp104LeadingBranch {
    /// Globally unique leading-branch identity.
    pub id: String,
    /// Owning `SWP104` operation label.
    pub operation_label: String,
    /// Nonzero construction discriminator.
    pub discriminator: NonZeroU8,
    /// Four finite shifted-binary64 values in serialized order.
    pub scalars: [ShiftedBinary64; 4],
    /// Whether one zero byte precedes the branch mode.
    pub leading_zero: bool,
    /// Serialized nonzero branch mode.
    pub mode: NonZeroU8,
    /// Exact state lane preceding the terminal marker.
    pub state_lane: Swp104StateLane,
    /// Ordered nonterminal references.
    pub members: BranchItems<FeatureSurfaceBranchReference>,
    /// Terminal reference.
    pub terminal: FeatureSurfaceBranchReference,
    /// Exact byte length through the terminal zero.
    pub byte_len: u64,
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureSwp104LeadingBranchWire {
    id: String,
    operation_label: String,
    discriminator: NonZeroU8,
    scalars: [f64; 4],
    raw_scalars: [[u8; 8]; 4],
    leading_zero: bool,
    mode: NonZeroU8,
    declared_count: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    witnessed_count: Option<u8>,
    state_lane: Vec<u8>,
    members: BranchItems<FeatureSurfaceBranchReference>,
    terminal: FeatureSurfaceBranchReference,
    byte_len: u64,
    source_offset: u64,
}

impl From<FeatureSwp104LeadingBranch> for FeatureSwp104LeadingBranchWire {
    fn from(value: FeatureSwp104LeadingBranch) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            discriminator: value.discriminator,
            scalars: value.scalars.map(ShiftedBinary64::value),
            raw_scalars: value.scalars.map(ShiftedBinary64::raw),
            leading_zero: value.leading_zero,
            mode: value.mode,
            declared_count: value.members.declared_count(),
            witnessed_count: value.state_lane.witnessed_count(),
            state_lane: value.state_lane.bytes().to_vec(),
            members: value.members,
            terminal: value.terminal,
            byte_len: value.byte_len,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeatureSwp104LeadingBranchWire> for FeatureSwp104LeadingBranch {
    type Error = String;
    fn try_from(wire: FeatureSwp104LeadingBranchWire) -> Result<Self, Self::Error> {
        if wire.declared_count != wire.members.declared_count() {
            return Err("declared_count must equal members length plus one".to_owned());
        }
        let [a, b, c, d] = std::array::from_fn::<_, 4, _>(|i| ShiftedBinary64::from_wire(wire.scalars[i], wire.raw_scalars[i]));
        let scalars = [a?, b?, c?, d?];
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            discriminator: wire.discriminator,
            scalars,
            leading_zero: wire.leading_zero,
            mode: wire.mode,
            state_lane: Swp104StateLane::from_parts(wire.witnessed_count, wire.state_lane)?,
            members: wire.members,
            terminal: wire.terminal,
            byte_len: wire.byte_len,
            source_offset: wire.source_offset,
        })
    }
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

/// One resolved reference within a surface-construction branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSurfaceBranchReference {
    /// Zero-based member order, or the declared count minus one for the terminal.
    pub ordinal: u32,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub token: crate::om::reference_index::ReferenceIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// One exact counted branch in a bounded surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FeatureSurfaceConstructionBranchWire", into = "FeatureSurfaceConstructionBranchWire")]
pub struct FeatureSurfaceConstructionBranch {
    /// Globally unique branch identity.
    pub id: String,
    /// Owning `SKIN` or `Studio Surface` operation label.
    pub operation_label: String,
    /// Zero-based branch order.
    pub ordinal: u32,
    /// Serialized construction family byte following `a0 5a`.
    pub family: u8,
    /// Serialized branch-group header code.
    pub header_code: u8,
    /// Serialized `16` or `40` branch mode.
    pub mode: crate::om::discriminators::SurfaceBranchMode,
    /// Whether the payload repeats the declared count before its zero lane.
    pub witnessed: bool,
    /// Ordered nonterminal references.
    pub members: BranchItems<FeatureSurfaceBranchReference>,
    /// Terminal reference.
    pub terminal: FeatureSurfaceBranchReference,
    /// Opaque bytes separating the terminal from the next branch or terminator.
    pub suffix: Vec<u8>,
    /// Absolute file offset of the branch mode byte.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureSurfaceConstructionBranchWire {
    id: String,
    operation_label: String,
    ordinal: u32,
    family: u8,
    header_code: u8,
    mode: crate::om::discriminators::SurfaceBranchMode,
    declared_count: u8,
    witnessed: bool,
    members: BranchItems<FeatureSurfaceBranchReference>,
    terminal: FeatureSurfaceBranchReference,
    suffix: Vec<u8>,
    source_offset: u64,
}

impl From<FeatureSurfaceConstructionBranch> for FeatureSurfaceConstructionBranchWire {
    fn from(value: FeatureSurfaceConstructionBranch) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            ordinal: value.ordinal,
            family: value.family,
            header_code: value.header_code,
            mode: value.mode,
            declared_count: value.members.declared_count(),
            witnessed: value.witnessed,
            members: value.members,
            terminal: value.terminal,
            suffix: value.suffix,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeatureSurfaceConstructionBranchWire> for FeatureSurfaceConstructionBranch {
    type Error = String;
    fn try_from(wire: FeatureSurfaceConstructionBranchWire) -> Result<Self, Self::Error> {
        if wire.declared_count != wire.members.declared_count() {
            return Err("declared_count must equal members length plus one".to_owned());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            ordinal: wire.ordinal,
            family: wire.family,
            header_code: wire.header_code,
            mode: wire.mode,
            witnessed: wire.witnessed,
            members: wire.members,
            terminal: wire.terminal,
            suffix: wire.suffix,
            source_offset: wire.source_offset,
        })
    }
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
    pub token: crate::om::reference_index::ReferenceIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Fixed shifted-IEEE scalar header from a bounded extrusion payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FeatureExtrudePayloadHeaderWire", into = "FeatureExtrudePayloadHeaderWire")]
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
        let [first, second] = std::array::from_fn::<_, 2, _>(|i| ShiftedBinary64::from_wire(wire.scalars[i], wire.raw_scalars[i]));
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            scalars: [first?, second?],
            source_offset: wire.source_offset,
        })
    }
}

/// Exact terminal discriminator lane from a bounded operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureOperationTerminalDiscriminatorWire",
    into = "FeatureOperationTerminalDiscriminatorWire"
)]
pub struct FeatureOperationTerminalDiscriminator {
    pub id: String,
    pub operation_label: String,
    pub type_indices: [FeatureIndexToken; 2],
    pub flags: [u8; 4],
    pub trailing_indices: Vec<FeatureIndexToken>,
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureOperationTerminalDiscriminatorWire {
    /// Globally unique lane identity.
    id: String,
    /// Owning operation label.
    operation_label: String,
    /// Two compact type indices following the footer prelude.
    type_indices: [u32; 2],
    /// Exact compact-index tokens for the two type indices.
    raw_type_indices: [Vec<u8>; 2],
    /// Absolute file offsets of the two type-index tokens.
    type_index_source_offsets: [u64; 2],
    /// Four serialized one-byte flags.
    flags: [u8; 4],
    /// Compact values preceding the payload terminator.
    trailing_indices: Vec<u32>,
    /// Exact compact-index tokens in the trailing lane.
    raw_trailing_indices: Vec<Vec<u8>>,
    /// Absolute file offsets of the trailing compact-index tokens.
    trailing_index_source_offsets: Vec<u64>,
    /// Absolute file offset of the footer prelude.
    source_offset: u64,
}

impl From<FeatureOperationTerminalDiscriminator> for FeatureOperationTerminalDiscriminatorWire {
    fn from(lane: FeatureOperationTerminalDiscriminator) -> Self {
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            type_indices: lane.type_indices.each_ref().map(|token| token.value),
            raw_type_indices: lane.type_indices.each_ref().map(|token| token.raw.clone()),
            type_index_source_offsets: lane
                .type_indices
                .each_ref()
                .map(|token| token.source_offset),
            flags: lane.flags,
            trailing_indices: lane
                .trailing_indices
                .iter()
                .map(|token| token.value)
                .collect(),
            raw_trailing_indices: lane
                .trailing_indices
                .iter()
                .map(|token| token.raw.clone())
                .collect(),
            trailing_index_source_offsets: lane
                .trailing_indices
                .iter()
                .map(|token| token.source_offset)
                .collect(),
            source_offset: lane.source_offset,
        }
    }
}

impl TryFrom<FeatureOperationTerminalDiscriminatorWire> for FeatureOperationTerminalDiscriminator {
    type Error = String;

    fn try_from(wire: FeatureOperationTerminalDiscriminatorWire) -> Result<Self, Self::Error> {
        if wire.trailing_indices.len() != wire.raw_trailing_indices.len()
            || wire.trailing_indices.len() != wire.trailing_index_source_offsets.len()
        {
            return Err(
                "terminal discriminator trailing token columns must have equal lengths".into(),
            );
        }
        let mut slot = 0;
        let type_indices = wire.raw_type_indices.map(|raw| {
            let token = FeatureIndexToken {
                value: wire.type_indices[slot],
                raw,
                source_offset: wire.type_index_source_offsets[slot],
            };
            slot += 1;
            token
        });
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            type_indices,
            flags: wire.flags,
            trailing_indices: wire
                .trailing_indices
                .into_iter()
                .zip(wire.raw_trailing_indices)
                .zip(wire.trailing_index_source_offsets)
                .map(|((value, raw), source_offset)| FeatureIndexToken {
                    value,
                    raw,
                    source_offset,
                })
                .collect(),
            source_offset: wire.source_offset,
        })
    }
}

/// Three typed scalars anchored to an ordered operation body reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FeatureOperationBodyScalarTripleWire", into = "FeatureOperationBodyScalarTripleWire")]
pub struct FeatureOperationBodyScalarTriple {
    /// Globally unique scalar-clause identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub branch: u8,
    /// Three checked scalar atoms and their absolute source offsets.
    pub values: [FeatureBodyScalarToken; 3],
}

#[derive(Serialize, Deserialize)]
struct FeatureOperationBodyScalarTripleWire {
    /// Globally unique scalar-clause identity.
    id: String,
    /// Owning operation label.
    operation_label: String,
    /// Zero-based body-reference occurrence order.
    body_reference_ordinal: u32,
    /// Serialized body object index.
    body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    branch: u8,
    /// Ordered finite scalar values.
    values: [f64; 3],
    /// Ordered serialized width forms.
    encodings: [PayloadScalarEncoding; 3],
    /// Exact serialized scalar atoms in value order.
    raw_values: [Vec<u8>; 3],
    /// Absolute file offsets of the three scalar markers.
    source_offsets: [u64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureBodyScalarToken {
    pub atom: PayloadScalarAtom,
    pub source_offset: u64,
}

impl From<FeatureOperationBodyScalarTriple> for FeatureOperationBodyScalarTripleWire {
    fn from(value: FeatureOperationBodyScalarTriple) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            body_reference_ordinal: value.body_reference_ordinal,
            body_object_index: value.body_object_index,
            branch: value.branch,
            values: value.values.map(|token| token.atom.value()),
            encodings: value.values.map(|token| token.atom.encoding()),
            raw_values: value.values.map(|token| token.atom.raw().to_vec()),
            source_offsets: value.values.map(|token| token.source_offset),
        }
    }
}

impl TryFrom<FeatureOperationBodyScalarTripleWire> for FeatureOperationBodyScalarTriple {
    type Error = String;

    fn try_from(wire: FeatureOperationBodyScalarTripleWire) -> Result<Self, Self::Error> {
        let [a, b, c] = std::array::from_fn::<_, 3, _>(|i| {
            PayloadScalarAtom::from_wire(wire.values[i], wire.encodings[i], &wire.raw_values[i])
                .map(|atom| FeatureBodyScalarToken { atom, source_offset: wire.source_offsets[i] })
                .map_err(|error| format!("scalar[{i}]: {error}"))
        });
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            body_reference_ordinal: wire.body_reference_ordinal,
            body_object_index: wire.body_object_index,
            branch: wire.branch,
            values: [a?, b?, c?],
        })
    }
}

/// Ordered member index in a branch-`11` operation body clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Decoded compact index.
    pub member_index: u32,
    /// Exact compact-index token.
    pub raw_member_index: Vec<u8>,
    /// Absolute file offset of the compact-index marker.
    pub source_offset: u64,
}

/// Wrapped operation member resolved in the feature-body identity namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct FeatureOperationBody11Continuation {
    /// Globally unique continuation identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Compact index in the single-entry continuation lane.
    pub continuation_index: u32,
    /// Exact compact-index token in the continuation lane.
    pub raw_continuation_index: Vec<u8>,
    /// Absolute file offset of the continuation compact-index marker.
    pub continuation_source_offset: u64,
    /// Object index in the terminal field.
    pub terminal_object_index: u32,
    /// Exact serialized terminal object-index token.
    pub raw_terminal_object_index: Vec<u8>,
    /// Absolute file offset of the terminal object-index marker.
    pub terminal_source_offset: u64,
}

/// Homogeneous value encoding in an operation body-reference lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureOperationBodyReferenceLaneEncoding {
    /// NX OM compact-index encoding.
    CompactIndex,
    /// `f0`/`f1` payload object-index encoding.
    PayloadObjectIndex,
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
    pub branch: u8,
    pub encoding: FeatureOperationBodyReferenceLaneEncoding,
    pub references: Vec<FeatureDataBlockToken<Vec<u8>>>,
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
    branch: u8,
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
        Self {
            id: value.id,
            operation_label: value.operation_label,
            body_reference_ordinal: value.body_reference_ordinal,
            body_object_index: value.body_object_index,
            branch: value.branch,
            encoding: value.encoding,
            object_indices: value
                .references
                .iter()
                .map(|reference| reference.value)
                .collect(),
            raw_object_indices: value
                .references
                .iter()
                .map(|reference| reference.raw.clone())
                .collect(),
            data_blocks: value
                .references
                .iter()
                .map(|reference| reference.data_block.clone())
                .collect(),
            source_offsets: value
                .references
                .iter()
                .map(|reference| reference.source_offset)
                .collect(),
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
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            body_reference_ordinal: wire.body_reference_ordinal,
            body_object_index: wire.body_object_index,
            branch: wire.branch,
            encoding: wire.encoding,
            references: wire
                .object_indices
                .into_iter()
                .zip(wire.raw_object_indices)
                .zip(wire.data_blocks)
                .zip(wire.source_offsets)
                .map(
                    |(((value, raw), data_block), source_offset)| FeatureDataBlockToken {
                        value,
                        raw,
                        data_block,
                        source_offset,
                    },
                )
                .collect(),
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

/// Structured `32` branch following an extrusion body-reference field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureExtrudePayload32BranchWire",
    into = "FeatureExtrudePayload32BranchWire"
)]
pub struct FeatureExtrudePayload32Branch {
    pub id: String,
    pub operation_label: String,
    pub scalar: ShiftedBinary64,
    pub atoms: Vec<FeatureDataBlockToken<u32>>,
    pub first_indices: Vec<FeatureDataBlockToken<Vec<u8>>>,
    pub second_indices: Vec<FeatureDataBlockToken<Vec<u8>>>,
    pub terminal: FeatureIndexToken,
    pub source_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureDataBlockToken<R> {
    pub value: u32,
    pub raw: R,
    pub source_offset: u64,
    pub data_block: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct FeatureExtrudePayload32BranchWire {
    /// Globally unique branch identity.
    id: String,
    /// Owning `EXTRUDE` operation label.
    operation_label: String,
    /// Body object index anchoring the branch.
    body_object_index: u32,
    /// Finite shifted-IEEE scalar following the branch marker.
    scalar: f64,
    /// Exact shifted-binary64 scalar encoding.
    raw_scalar: [u8; 8],
    /// Ordered fixed-width big-endian atoms in the first counted lane.
    atoms_be: Vec<u32>,
    /// Absolute source offsets of the fixed-width atoms in lane order.
    atom_source_offsets: Vec<u64>,
    /// Compact indices wrapped by the fixed-width atoms.
    atom_indices: Vec<u32>,
    /// Unique offset-only data blocks addressed by the atom indices.
    atom_data_blocks: Vec<Option<String>>,
    /// Ordered values in the first compact-index lane.
    first_indices: Vec<u32>,
    /// Exact compact-index tokens in the first lane.
    raw_first_indices: Vec<Vec<u8>>,
    /// Absolute source offsets of the first-lane tokens.
    first_index_source_offsets: Vec<u64>,
    /// Unique offset-only data blocks addressed by the first lane.
    first_data_blocks: Vec<Option<String>>,
    /// Ordered values in the second compact-index lane.
    second_indices: Vec<u32>,
    /// Exact compact-index tokens in the second lane.
    raw_second_indices: Vec<Vec<u8>>,
    /// Absolute source offsets of the second-lane tokens.
    second_index_source_offsets: Vec<u64>,
    /// Unique offset-only data blocks addressed by the second lane.
    second_data_blocks: Vec<Option<String>>,
    /// Object index in the terminal field.
    terminal_object_index: u32,
    /// Exact serialized terminal object-index token.
    raw_terminal_object_index: Vec<u8>,
    /// Absolute file offset of the terminal object-index token.
    terminal_source_offset: u64,
    /// Absolute file offset of the `32` branch marker.
    source_offset: u64,
}

impl From<FeatureExtrudePayload32Branch> for FeatureExtrudePayload32BranchWire {
    fn from(branch: FeatureExtrudePayload32Branch) -> Self {
        Self {
            id: branch.id,
            operation_label: branch.operation_label,
            body_object_index: branch.terminal.value,
            scalar: branch.scalar.value(),
            raw_scalar: branch.scalar.raw(),
            atom_indices: branch.atoms.iter().map(|token| token.value).collect(),
            atoms_be: branch.atoms.iter().map(|token| token.raw).collect(),
            atom_source_offsets: branch
                .atoms
                .iter()
                .map(|token| token.source_offset)
                .collect(),
            atom_data_blocks: branch
                .atoms
                .iter()
                .map(|token| token.data_block.clone())
                .collect(),
            first_indices: branch
                .first_indices
                .iter()
                .map(|token| token.value)
                .collect(),
            raw_first_indices: branch
                .first_indices
                .iter()
                .map(|token| token.raw.clone())
                .collect(),
            first_index_source_offsets: branch
                .first_indices
                .iter()
                .map(|token| token.source_offset)
                .collect(),
            first_data_blocks: branch
                .first_indices
                .iter()
                .map(|token| token.data_block.clone())
                .collect(),
            second_indices: branch
                .second_indices
                .iter()
                .map(|token| token.value)
                .collect(),
            raw_second_indices: branch
                .second_indices
                .iter()
                .map(|token| token.raw.clone())
                .collect(),
            second_index_source_offsets: branch
                .second_indices
                .iter()
                .map(|token| token.source_offset)
                .collect(),
            second_data_blocks: branch
                .second_indices
                .iter()
                .map(|token| token.data_block.clone())
                .collect(),
            terminal_object_index: branch.terminal.value,
            raw_terminal_object_index: branch.terminal.raw,
            terminal_source_offset: branch.terminal.source_offset,
            source_offset: branch.source_offset,
        }
    }
}

impl TryFrom<FeatureExtrudePayload32BranchWire> for FeatureExtrudePayload32Branch {
    type Error = String;

    fn try_from(wire: FeatureExtrudePayload32BranchWire) -> Result<Self, Self::Error> {
        if wire.atom_indices.len() != wire.atoms_be.len()
            || wire.atom_indices.len() != wire.atom_source_offsets.len()
            || wire.atom_indices.len() != wire.atom_data_blocks.len()
        {
            return Err("extrusion atoms columns must have equal lengths".into());
        }
        if wire.first_indices.len() != wire.raw_first_indices.len()
            || wire.first_indices.len() != wire.first_index_source_offsets.len()
            || wire.first_indices.len() != wire.first_data_blocks.len()
        {
            return Err("extrusion first_indices columns must have equal lengths".into());
        }
        if wire.second_indices.len() != wire.raw_second_indices.len()
            || wire.second_indices.len() != wire.second_index_source_offsets.len()
            || wire.second_indices.len() != wire.second_data_blocks.len()
        {
            return Err("extrusion second_indices columns must have equal lengths".into());
        }
        if wire.body_object_index != wire.terminal_object_index {
            return Err("body_object_index must match terminal_object_index".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            scalar: ShiftedBinary64::from_wire(wire.scalar, wire.raw_scalar)
                .map_err(|error| format!("scalar/raw_scalar: {error}"))?,
            atoms: wire
                .atom_indices
                .into_iter()
                .zip(wire.atoms_be)
                .zip(wire.atom_source_offsets)
                .zip(wire.atom_data_blocks)
                .map(
                    |(((value, raw), source_offset), data_block)| FeatureDataBlockToken {
                        value,
                        raw,
                        source_offset,
                        data_block,
                    },
                )
                .collect(),
            first_indices: wire
                .first_indices
                .into_iter()
                .zip(wire.raw_first_indices)
                .zip(wire.first_index_source_offsets)
                .zip(wire.first_data_blocks)
                .map(
                    |(((value, raw), source_offset), data_block)| FeatureDataBlockToken {
                        value,
                        raw,
                        source_offset,
                        data_block,
                    },
                )
                .collect(),
            second_indices: wire
                .second_indices
                .into_iter()
                .zip(wire.raw_second_indices)
                .zip(wire.second_index_source_offsets)
                .zip(wire.second_data_blocks)
                .map(
                    |(((value, raw), source_offset), data_block)| FeatureDataBlockToken {
                        value,
                        raw,
                        source_offset,
                        data_block,
                    },
                )
                .collect(),
            terminal: FeatureIndexToken {
                value: wire.terminal_object_index,
                raw: wire.raw_terminal_object_index,
                source_offset: wire.terminal_source_offset,
            },
            source_offset: wire.source_offset,
        })
    }
}

/// Complete alternate extrusion construction using the structured `32` branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureExtrude32Construction {
    /// Globally unique construction identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Structured branch supplying the body-anchored construction lanes.
    pub branch: String,
    /// Body object index witnessed at both ends of the structured branch.
    pub body_object_index: u32,
    /// Ordered profile-reference identities.
    pub profile_references: Vec<String>,
    /// Ordered uniquely resolved profile blocks.
    pub profile_data_blocks: Vec<String>,
    /// Ordered uniquely resolved blocks from the fixed-atom lane.
    pub atom_data_blocks: Vec<String>,
    /// Ordered uniquely resolved blocks from the first compact-index lane.
    pub first_data_blocks: Vec<String>,
    /// Ordered uniquely resolved blocks from the second compact-index lane.
    pub second_data_blocks: Vec<String>,
}

/// Ordered construction reference carried by a bounded `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBlockConstructionReference {
    /// Globally unique construction-reference identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Payload control byte preceding the construction field.
    pub control: u8,
    /// Zero-based reference order across the complete field.
    pub ordinal: u32,
    /// Whether this is the reference following the separator byte.
    pub terminal: bool,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub token: crate::om::reference_index::ReferenceIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Completely resolved construction-reference field of one `BLOCK` feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FeatureBlockConstructionWire", into = "FeatureBlockConstructionWire")]
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
        let (member_references, member_data_blocks) = value.members.into_iter()
            .map(|member| (member.reference, member.data_block)).unzip();
        Self { id: value.id, operation_label: value.operation_label, control: value.control,
            member_references, member_data_blocks, terminal_reference: value.terminal_reference,
            terminal_data_block: value.terminal_data_block }
    }
}

impl TryFrom<FeatureBlockConstructionWire> for FeatureBlockConstruction {
    type Error = String;
    fn try_from(wire: FeatureBlockConstructionWire) -> Result<Self, Self::Error> {
        if wire.member_references.len() != wire.member_data_blocks.len() {
            return Err("member_references and member_data_blocks must have equal lengths".to_owned());
        }
        let members = wire.member_references.into_iter().zip(wire.member_data_blocks)
            .map(|(reference, data_block)| FeatureConstructionMember { reference, data_block })
            .collect::<Vec<_>>();
        Ok(Self { id: wire.id, operation_label: wire.operation_label, control: wire.control,
            members: members.try_into().map_err(|_| "member_references must contain eighteen entries".to_owned())?, terminal_reference: wire.terminal_reference,
            terminal_data_block: wire.terminal_data_block })
    }
}


/// One complete compact-code name field in a reconstructed `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureBlockPayloadNameWire",
    into = "FeatureBlockPayloadNameWire"
)]
pub struct FeatureBlockPayloadName {
    /// Globally unique name-field identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Reconstructed payload containing the field.
    pub construction_payload: String,
    /// Zero-based name order in the reconstructed payload.
    pub ordinal: u32,
    /// Compact type code, absent for the type-free payload-leading form.
    pub type_code: Option<FeaturePayloadTypeCode>,
    /// Exact printable field value.
    pub value: String,
    /// Payload-relative opening `66` or payload-leading `03` marker offset.
    pub payload_offset: u64,
    /// Absolute source offset of the opening marker.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeatureBlockPayloadNameWire {
    id: String,
    operation_label: String,
    construction_payload: String,
    ordinal: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    type_code: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw_type_code: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    type_code_payload_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    type_code_source_offset: Option<u64>,
    payload_leading: bool,
    value: String,
    payload_offset: u64,
    source_offset: u64,
}

impl From<FeatureBlockPayloadName> for FeatureBlockPayloadNameWire {
    fn from(value: FeatureBlockPayloadName) -> Self {
        let (
            type_code,
            raw_type_code,
            type_code_payload_offset,
            type_code_source_offset,
            payload_leading,
        ) = feature_payload_type_code_to_wire(value.type_code);
        Self {
            id: value.id,
            operation_label: value.operation_label,
            construction_payload: value.construction_payload,
            ordinal: value.ordinal,
            type_code,
            raw_type_code,
            type_code_payload_offset,
            type_code_source_offset,
            payload_leading,
            value: value.value,
            payload_offset: value.payload_offset,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeatureBlockPayloadNameWire> for FeatureBlockPayloadName {
    type Error = String;

    fn try_from(wire: FeatureBlockPayloadNameWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            construction_payload: wire.construction_payload,
            ordinal: wire.ordinal,
            type_code: feature_payload_type_code_from_wire(
                wire.type_code,
                wire.raw_type_code,
                wire.type_code_payload_offset,
                wire.type_code_source_offset,
                wire.payload_leading,
            )?,
            value: wire.value,
            payload_offset: wire.payload_offset,
            source_offset: wire.source_offset,
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

/// Persistent object frame carried by one bounded offset-store block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockObjectFrame {
    /// Globally unique relation identity.
    pub id: String,
    /// Source block carrying the object frame.
    pub data_block: String,
    /// Zero-based frame order within the block.
    pub ordinal: u32,
    /// Serialized persistent object ID.
    pub object_id: u32,
    /// Exact serialized compact object-index token.
    pub raw_object_id: Vec<u8>,
    /// Absolute source offset of the compact object index.
    pub source_offset: u64,
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
    pub target: FeatureIndexToken,
    pub tools: Vec<FeatureIndexToken>,
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
            target_object_index: operation.target.value,
            raw_target_object_index: operation.target.raw,
            target_source_offset: operation.target.source_offset,
            tool_object_indices: operation.tools.iter().map(|token| token.value).collect(),
            raw_tool_object_indices: operation
                .tools
                .iter()
                .map(|token| token.raw.clone())
                .collect(),
            tool_source_offsets: operation
                .tools
                .iter()
                .map(|token| token.source_offset)
                .collect(),
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
            target: FeatureIndexToken {
                value: wire.target_object_index,
                raw: wire.raw_target_object_index,
                source_offset: wire.target_source_offset,
            },
            tools: wire
                .tool_object_indices
                .into_iter()
                .zip(wire.raw_tool_object_indices)
                .zip(wire.tool_source_offsets)
                .map(|((value, raw), source_offset)| FeatureIndexToken {
                    value,
                    raw,
                    source_offset,
                })
                .collect(),
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
    mut visit: impl FnMut(&crate::om::Section<'_>, &str, u64, usize, crate::om::OperationRecord<'_>),
) {
    let sections = container.om_sections();
    for (section_ordinal, link) in feature_history_sections(container) {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
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
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
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
            .section_offset
            .cmp(&second.section_offset)
            .then_with(|| first.source_offset.cmp(&second.source_offset))
            .then_with(|| first.id.cmp(&second.id))
    });
    links.dedup_by_key(|link| link.section_offset);
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
        .map(|label| operation_header_identity_key(label.object_indices, block_identities))
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
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let Some(record_area) = section.record_area else {
            continue;
        };
        let record_area_offset = record_area.offset;
        let record_area = record_area.bytes;
        labels.extend(
            section
                .operation_records_with_label_ordinals()
                .into_iter()
                .filter_map(|(ordinal, record)| {
                    let label = record.label;
                    let raw_object_indices: [Option<Vec<u8>>; 4] = std::array::from_fn(|slot| {
                        let start = label.object_index_offsets[slot] - record_area_offset;
                        let end = if slot + 1 < label.object_index_offsets.len() {
                            label.object_index_offsets[slot + 1] - record_area_offset
                        } else {
                            label.offset - record_area_offset
                        };
                        record_area.get(start..end).map(<[u8]>::to_vec)
                    });
                    let raw_object_indices = raw_object_indices
                        .into_iter()
                        .collect::<Option<Vec<_>>>()?
                        .try_into()
                        .ok()?;
                    Some(FeatureOperationLabel {
                        id: format!(
                            "nx:feature-history:operation-label#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal: ordinal as u32,
                        value: label.value.to_string(),
                        object_indices: label.object_indices,
                        raw_object_indices,
                        stable_identity: None,
                        source_offset: entry_offset + label.offset as u64,
                    })
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
                .find(|operation| operation.offset == record.label.offset)
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
                target: FeatureIndexToken {
                    value: operation.target.token.value(),
                    raw: operation.target.token.raw().to_vec(),
                    source_offset: entry_offset + operation.target.offset as u64,
                },
                tools: operation
                    .tools
                    .into_iter()
                    .map(|tool| FeatureIndexToken {
                        value: tool.token.value(),
                        raw: tool.token.raw().to_vec(),
                        source_offset: entry_offset + tool.offset as u64,
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
            let stable_identity =
                operation_header_identity_key(record.label.object_indices, &block_identities);
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
                    byte_len: record.bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(record.bytes),
                    payload_byte_len: record.payload.len() as u64,
                    payload_sha256: cadmpeg_ir::hash::sha256_hex(record.payload),
                    stable_identity: None,
                    payload_source_offset: entry_offset + record.payload_offset as u64,
                    source_offset: entry_offset + record.offset() as u64,
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
            records.push(FeatureUnlabeledOperationRecord {
                id: format!(
                    "nx:feature-history:unlabeled-operation-record#{section_key}-{operation_ordinal:010}"
                ),
                ordinal: operation_ordinal as u32,
                object_indices: record.object_indices,
                object_index_source_offsets: record
                    .object_index_offsets
                    .map(|offset| entry_offset + offset as u64),
                byte_len: record.bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(record.bytes),
                payload_byte_len: record.payload.len() as u64,
                payload_sha256: cadmpeg_ir::hash::sha256_hex(record.payload),
                payload_source_offset: entry_offset + record.payload_offset as u64,
                source_offset: entry_offset + record.offset as u64,
            });
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
                writes.push(FeatureOperationBodyWrite {
                    operation_label: None,
                    id: format!(
                        "nx:feature-history:unlabeled-operation-body-write#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_record: operation_record.clone(),
                    ordinal: ordinal as u32,
                    body_identity: write.body_identity,
                    group_node: write.group_node,
                    raw_group_node: write.raw_group_node,
                    group_node_source_offset: entry_offset + write.group_node_offset as u64,
                    endpoint_tag: write.endpoint_tag,
                    body_image_object_index: write.body_image_object_index,
                    body_image_data_block: unique_offset_data_block(
                        &indexed,
                        write.body_image_object_index,
                    ),
                    raw_body_image_object_index: write.raw_body_image_object_index,
                    body_image_object_index_source_offset: entry_offset
                        + write.body_image_object_index_offset as u64,
                    byte_len: (write.end_offset - write.offset) as u64,
                    source_offset: entry_offset + write.offset as u64,
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
            for (ordinal, write) in crate::om::operation_body_write_frames(record)
                .into_iter()
                .enumerate()
            {
                writes.push(FeatureOperationBodyWrite {
                    id: format!(
                        "nx:feature-history:operation-body-write#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: Some(operation_label.clone()),
                    operation_record: operation_record.clone(),
                    ordinal: ordinal as u32,
                    body_identity: write.body_identity,
                    group_node: write.group_node,
                    raw_group_node: write.raw_group_node,
                    group_node_source_offset: entry_offset
                        + write.group_node_offset as u64,
                    endpoint_tag: write.endpoint_tag,
                    body_image_object_index: write.body_image_object_index,
                    body_image_data_block: unique_offset_data_block(
                        &indexed,
                        write.body_image_object_index,
                    ),
                    raw_body_image_object_index: write.raw_body_image_object_index,
                    body_image_object_index_source_offset: entry_offset
                        + write.body_image_object_index_offset as u64,
                    byte_len: (write.end_offset - write.offset) as u64,
                    source_offset: entry_offset + write.offset as u64,
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
                    && binding.body_alias_object_index == u32::from(write.body_identity)
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
                    && binding.body_alias_object_index == u32::from(write.body_identity)
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
                body_identity: write.body_identity,
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
    (streams.get(stream_ordinal)?.kind == crate::parasolid::StreamKind::Plain).then_some(())?;
    let partition_ordinal = streams
        .iter()
        .enumerate()
        .skip(stream_ordinal + 1)
        .find_map(|(ordinal, stream)| {
            (stream.kind == crate::parasolid::StreamKind::Partition).then_some(ordinal)
        })?;
    let run_start = streams[..stream_ordinal]
        .iter()
        .rposition(|stream| stream.kind != crate::parasolid::StreamKind::Plain)
        .map_or(0, |ordinal| ordinal + 1);
    let run_streams = streams.get(run_start..partition_ordinal)?;
    run_streams
        .iter()
        .all(|stream| stream.kind == crate::parasolid::StreamKind::Plain)
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
                        && group.node_id == write.group_node
                })
                .map(|group| group.id.clone())
                .collect();
            let parasolid_group_members = group_members
                .iter()
                .filter(|member| {
                    member.partition_stream_ordinal == partition_stream_ordinal
                        && member.group_node_id == write.group_node
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
                group_node: write.group_node,
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
    let candidates = writes
        .iter()
        .chain(unlabeled_writes)
        .map(|write| (write.id.as_str(), write.body_identity, write.group_node));
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
                .all(|group| group.origin.partition_stream_ordinal() == Some(partition_stream_ordinal))
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

/// Decode exact direct tagged-reference fields from bounded feature operations.
pub fn feature_operation_tagged_references(
    container: &Container,
) -> Vec<FeatureOperationObjectReference> {
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
            for (ordinal, reference) in crate::om::operation_tagged_references(record)
                .into_iter()
                .enumerate()
            {
                references.push(FeatureOperationObjectReference {
                    id: format!(
                        "nx:feature-history:operation-tagged-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    operation_record: operation_record.clone(),
                    ordinal: ordinal as u32,
                    tag: Some(reference.tag),
                    object_index: reference.object_index,
                    raw_object_index: reference.raw_object_index,
                    data_block: unique_offset_data_block(&indexed, reference.object_index),
                    object_index_source_offset: entry_offset
                        + reference.object_index_offset as u64,
                    byte_len: (reference.end_offset - reference.offset) as u64,
                    source_offset: entry_offset + reference.offset as u64,
                });
            }
        },
    );
    references
}

/// Decode exact direct operation data-block reference fields from bounded
/// feature operations.
pub fn feature_operation_data_block_references(
    container: &Container,
) -> Vec<FeatureOperationObjectReference> {
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
            for (ordinal, reference) in crate::om::operation_data_block_references(record)
                .into_iter()
                .enumerate()
            {
                references.push(FeatureOperationObjectReference {
                    tag: None,
                    id: format!(
                        "nx:feature-history:operation-data-block-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    operation_record: operation_record.clone(),
                    ordinal: ordinal as u32,
                    object_index: reference.object_index,
                    raw_object_index: reference.raw_object_index,
                    data_block: unique_offset_data_block(&indexed, reference.object_index),
                    object_index_source_offset: entry_offset
                        + reference.object_index_offset as u64,
                    byte_len: (reference.end_offset - reference.offset) as u64,
                    source_offset: entry_offset + reference.offset as u64,
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
            for (ordinal, frame) in crate::om::operation_common_frames(record)
                .into_iter()
                .enumerate()
            {
                frames.push(FeatureOperationCommonFrame {
                    id: format!(
                        "nx:feature-history:operation-common-frame#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_record: operation_record.clone(),
                    ordinal: ordinal as u32,
                    indices: frame.indices,
                    raw_indices: frame.raw_indices,
                    marker: frame.marker,
                    state: frame.state,
                    legacy_inactive_modules: operation_legacy_inactive_modules(frame.state),
                    modifies_parasolid_data: operation_modifies_parasolid_data(frame.state),
                    split_tracking_data: operation_split_tracking_data(frame.state),
                    group_count: frame.state[7],
                    local_ordinal: frame.local_ordinal,
                    raw_local_ordinal: frame.raw_local_ordinal,
                    object_index: frame.object_index,
                    raw_object_index: frame.raw_object_index,
                    data_block: frame
                        .object_index
                        .and_then(|object_index| unique_offset_data_block(&indexed, object_index)),
                    byte_len: (frame.end_offset - frame.offset) as u64,
                    source_offset: entry_offset + frame.offset as u64,
                    index_source_offsets: frame
                        .index_offsets
                        .map(|offset| entry_offset + offset as u64),
                    state_source_offset: entry_offset + frame.state_offset as u64,
                    local_ordinal_source_offset: entry_offset
                        + frame.local_ordinal_offset as u64,
                    object_index_source_offset: entry_offset + frame.object_index_offset as u64,
                });
            }
        },
    );
    frames
}

fn operation_legacy_inactive_modules(state: [u8; 8]) -> Option<bool> {
    match state[3] {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn operation_modifies_parasolid_data(state: [u8; 8]) -> Option<bool> {
    match state[4] {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn operation_split_tracking_data(state: [u8; 8]) -> [u8; 2] {
    [state[5], state[6]]
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
            let Some(frame) = crate::om::operation_terminal_frame(record) else {
                return;
            };
            let operation_record = format!(
                "nx:feature-history:operation-record#{section_key}-{operation_ordinal:010}"
            );
            let immediate_common_frame = frame.immediate_common_frame_offset.and_then(|offset| {
                let matches = common_frames
                    .iter()
                    .filter(|common| {
                        common.operation_record == operation_record
                            && common.source_offset == entry_offset + offset as u64
                    })
                    .collect::<Vec<_>>();
                let [common] = matches.as_slice() else {
                    return None;
                };
                Some(common.id.clone())
            });
            frames.push(FeatureOperationTerminalFrame {
                id: format!(
                    "nx:feature-history:operation-terminal-frame#{section_key}-{operation_ordinal:010}"
                ),
                operation_record,
                immediate_common_frame,
                local_ordinal: frame.local_ordinal,
                raw_local_ordinal: frame.raw_local_ordinal,
                object_index: frame.object_index,
                raw_object_index: frame.raw_object_index,
                data_block: frame
                    .object_index
                    .and_then(|object_index| unique_offset_data_block(&indexed, object_index)),
                source_offset: entry_offset + frame.offset as u64,
                object_index_source_offset: entry_offset + frame.object_index_offset as u64,
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
        for (row_ordinal, row) in group.rows.iter().enumerate() {
            let Some(row_ordinal) = u32::try_from(row_ordinal).ok() else {
                continue;
            };
            let key = (group.section_link.as_str(), row.state_ordinal.value());
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
        let Some(Some((group, journal_row_ordinal, row))) =
            journal_rows.get(&(label.section_link.as_str(), frame.local_ordinal))
        else {
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
            operation_local_ordinal: frame.local_ordinal,
            journal_state_ordinal: row.state_ordinal.value(),
            operation_source_offset: frame.source_offset,
            journal_source_offset: row.source_offset,
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
                crate::om::operation_payload_strings(record)
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

fn symbolic_thread_text_frames(
    record: crate::om::OperationRecord<'_>,
) -> Option<Vec<crate::om::OperationPayloadTextFrame<'_>>> {
    if record.label.value != "SYMBOLIC_THREAD" {
        return None;
    }
    let frames = crate::om::operation_payload_text_frames(record)
        .into_iter()
        .filter(|frame| frame.marker == crate::om::OperationTextMarker::Text)
        .collect::<Vec<_>>();
    (frames.len() >= 2).then_some(frames)
}

/// Decode complete typed text frames from symbolic-thread operations.
pub fn feature_symbolic_threads(container: &Container) -> Vec<FeatureSymbolicThread> {
    let mut threads = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(frames) = symbolic_thread_text_frames(record) else {
                return;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let operation_record = format!(
                "nx:feature-history:operation-record#{section_key}-{operation_ordinal:010}"
            );
            let id =
                format!("nx:feature-history:symbolic-thread#{section_key}-{operation_ordinal:010}");
            let text_frames = frames
                .into_iter()
                .enumerate()
                .map(|(ordinal, frame)| FeatureSymbolicThreadTextFrame {
                    id: format!(
                        "nx:feature-history:symbolic-thread-text-frame#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    symbolic_thread: id.clone(),
                    ordinal: ordinal as u32,
                    value: frame.value.into_owned(),
                    source_offset: entry_offset + frame.offset as u64,
                })
                .collect();
            threads.push(FeatureSymbolicThread {
                id,
                operation_label,
                operation_record,
                text_frames,
                source_offset: entry_offset + record.offset() as u64,
            });
        },
    );
    threads
}

/// Join exact hole payload templates to their operation identities.
pub fn feature_simple_hole_templates(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    strings: &[FeaturePayloadString],
) -> Vec<FeatureSimpleHoleTemplate> {
    let labels_by_id = labels
        .iter()
        .map(|label| (label.id.as_str(), label))
        .collect::<BTreeMap<_, _>>();
    let records_by_id = records
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let mut templates_by_operation = BTreeMap::<String, Vec<_>>::new();
    for string in strings {
        let Some(record) = records_by_id.get(string.operation_record.as_str()) else {
            continue;
        };
        let Some(label) = labels_by_id.get(record.operation_label.as_str()) else {
            continue;
        };
        if !matches!(
            label.value.as_str(),
            "SIMPLE HOLE" | "CBORE_HOLE" | "CSUNK_HOLE"
        ) || !string.value.as_str().starts_with("Hole_")
        {
            continue;
        }
        templates_by_operation
            .entry(label.id.clone())
            .or_default()
            .push((string, *label));
    }
    templates_by_operation
        .into_values()
        .filter_map(|candidates| {
            let [(string, label)] = candidates.as_slice() else {
                return None;
            };
            let (family, form, extent, start_treatment, end_treatment) =
                parse_simple_hole_template(string.value.as_str())?;
            Some(FeatureSimpleHoleTemplate {
                id: string
                    .id
                    .replacen("payload-string", "simple-hole-template", 1),
                operation_label: label.id.clone(),
                payload_string: string.id.clone(),
                family,
                form,
                extent,
                start_treatment,
                end_treatment,
            })
        })
        .collect()
}

/// Join exact threaded-hole payload templates to their operation identities.
pub fn feature_threaded_hole_templates(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    strings: &[FeaturePayloadString],
) -> Vec<FeatureThreadedHoleTemplate> {
    let labels_by_id = labels
        .iter()
        .map(|label| (label.id.as_str(), label))
        .collect::<BTreeMap<_, _>>();
    let records_by_id = records
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let mut templates_by_operation = BTreeMap::<String, Vec<_>>::new();
    for string in strings {
        let Some(record) = records_by_id.get(string.operation_record.as_str()) else {
            continue;
        };
        let Some(label) = labels_by_id.get(record.operation_label.as_str()) else {
            continue;
        };
        if label.value != "SIMPLE HOLE" || !string.value.as_str().starts_with("Hole_") {
            continue;
        }
        templates_by_operation
            .entry(label.id.clone())
            .or_default()
            .push((string, *label));
    }
    templates_by_operation
        .into_values()
        .filter_map(|candidates| {
            let [(string, label)] = candidates.as_slice() else {
                return None;
            };
            let (family, extent) = parse_threaded_hole_template(string.value.as_str())?;
            Some(FeatureThreadedHoleTemplate {
                id: string
                    .id
                    .replacen("payload-string", "threaded-hole-template", 1),
                operation_label: label.id.clone(),
                payload_string: string.id.clone(),
                family,
                extent,
                source_offset: string.source_offset,
            })
        })
        .collect()
}

/// Decode exact nonempty duplicated scalar lanes from simple-hole operations.
pub fn feature_simple_hole_repeated_scalar_lanes(
    container: &Container,
) -> Vec<FeatureSimpleHoleRepeatedScalarLane> {
    let mut pairs = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(pair) = crate::om::simple_hole_repeated_scalar_lane(record) else {
                return;
            };
            pairs.push(FeatureSimpleHoleRepeatedScalarLane {
                id: format!(
                    "nx:feature-history:simple-hole-repeated-scalar-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                values: pair.map(|token| RepeatedScalar {
                    scalar: token.scalar,
                    witness_offsets: token.witness_offsets.map(|offset| entry_offset + offset as u64),
                }),
            });
        },
    );
    pairs
}

/// Resolve the tagged block-index pairs following both repeated scalar-lane
/// witnesses through the unique offset store that owns the operation inputs.
pub fn feature_simple_hole_repeated_scalar_lane_block_references(
    container: &Container,
) -> Vec<FeatureSimpleHoleRepeatedScalarLaneBlockReferences> {
    let inputs = feature_input_blocks(container);
    let blocks = data_blocks(container)
        .into_iter()
        .map(|block| block.id)
        .collect::<BTreeSet<_>>();
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let prefixes = inputs
                .iter()
                .filter(|input| input.operation_label == operation_label)
                .filter_map(|input| {
                    input
                        .data_block
                        .rsplit_once(":block#")
                        .map(|(prefix, _)| prefix)
                })
                .collect::<BTreeSet<_>>();
            let mut prefixes = prefixes.into_iter();
            let (Some(prefix), None) = (prefixes.next(), prefixes.next()) else {
                return;
            };
            let Some(decoded) =
                crate::om::simple_hole_repeated_scalar_lane_block_references(record)
            else {
                return;
            };
            let resolve = |indices: [u32; 2]| {
                let targets = indices.map(|index| format!("{prefix}:block#{index}"));
                targets
                    .iter()
                    .all(|target| blocks.contains(target))
                    .then_some(targets)
            };
            let (Some(first_data_blocks), Some(second_data_blocks)) =
                (resolve(decoded.first), resolve(decoded.second))
            else {
                return;
            };
            references.push(FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
                id: format!(
                    "nx:feature-history:simple-hole-repeated-scalar-lane-block-references#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                first_data_blocks,
                second_data_blocks,
                first_reference_prefix: decoded.prefixes[0],
                second_reference_prefix: decoded.prefixes[1],
                first_reference_offsets: decoded.offsets[0].map(|offset| entry_offset + offset as u64),
                second_reference_offsets: decoded.offsets[1].map(|offset| entry_offset + offset as u64),
            });
        },
    );
    references
}

/// Group distinct simple-hole operations that address the same four construction blocks.
pub fn feature_simple_hole_construction_groups(
    labels: &[FeatureOperationLabel],
    lanes: &[FeatureSimpleHoleRepeatedScalarLane],
    references: &[FeatureSimpleHoleRepeatedScalarLaneBlockReferences],
) -> Vec<FeatureSimpleHoleConstructionGroup> {
    let chronological_positions = feature_operation_chronological_labels(labels)
        .into_iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let mut lanes_by_operation = BTreeMap::<&str, Vec<_>>::new();
    for lane in lanes {
        lanes_by_operation
            .entry(lane.operation_label.as_str())
            .or_default()
            .push(lane);
    }
    let mut grouped = BTreeMap::<([String; 2], [String; 2]), Vec<_>>::new();
    let mut ambiguous_groups = BTreeSet::new();
    for reference in references {
        let key = (
            reference.first_data_blocks.clone(),
            reference.second_data_blocks.clone(),
        );
        let lane = match lanes_by_operation
            .get(reference.operation_label.as_str())
            .map(Vec::as_slice)
        {
            Some([lane]) => *lane,
            Some(_) => {
                ambiguous_groups.insert(key);
                continue;
            }
            None => continue,
        };
        grouped.entry(key).or_default().push((reference, lane));
    }
    grouped
        .into_iter()
        .filter_map(|(key, mut members)| {
            if ambiguous_groups.contains(&key) {
                return None;
            }
            let operation_position =
                |reference: &FeatureSimpleHoleRepeatedScalarLaneBlockReferences| {
                    chronological_positions
                        .get(reference.operation_label.as_str())
                        .copied()
                };
            if members
                .iter()
                .any(|(reference, _)| operation_position(reference).is_none())
            {
                return None;
            }
            members.sort_by(|(first, _), (second, _)| {
                operation_position(first)
                    .cmp(&operation_position(second))
                    .then_with(|| first.operation_label.cmp(&second.operation_label))
            });
            let members = SimpleHoleConstructionMembers::new(members
                .into_iter()
                .map(|(reference, lane)| FeatureSimpleHoleConstructionMember {
                    operation_label: reference.operation_label.clone(),
                    scalar_lane: lane.id.clone(),
                    block_reference: reference.id.clone(),
                })
                .collect()).ok()?;
            let id_anchor = members.iter().fold(&members[0], |first, second| {
                if first.operation_label <= second.operation_label { first } else { second }
            });
            let id_key = id_anchor
                .operation_label
                .rsplit_once('#')
                .map_or("unknown", |(_, key)| key);
            Some(FeatureSimpleHoleConstructionGroup {
                id: format!("nx:feature-history:simple-hole-construction-group#{id_key}"),
                first_data_blocks: key.0,
                second_data_blocks: key.1,
                members,
            })
        })
        .collect()
}

/// Decode and resolve exact four-block lanes from `HOLE PACKAGE` operations.
pub fn feature_hole_package_construction_group_lanes(
    container: &Container,
) -> Vec<FeatureHolePackageConstructionGroupLane> {
    let indexed = container.indexed_om_sections();
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(lane) = crate::om::hole_package_construction_group_lane(record) else {
                return;
            };
            let [a, b, c, d] = lane.references.map(|reference| {
                Some(ConstructionReference {
                    token: reference.token,
                    data_block: unique_offset_data_block(&indexed, reference.token.value())?,
                    source_offset: entry_offset + reference.offset as u64,
                })
            });
            let [Some(a), Some(b), Some(c), Some(d)] = [a, b, c, d] else {
                return;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            lanes.push(FeatureHolePackageConstructionGroupLane {
                id: format!(
                    "nx:feature-history:hole-package-construction-group-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                selector: lane.selector,
                branch: lane.branch,
                references: [a, b, c, d],
                payload_offset: lane.offset as u64,
                source_offset: entry_offset + record.payload_offset as u64 + lane.offset as u64,
            });
        },
    );
    lanes
}

/// Join one package lane to one simple-hole group only by exact four-block identity.
pub fn feature_hole_package_construction_group_uses(
    lanes: &[FeatureHolePackageConstructionGroupLane],
    groups: &[FeatureSimpleHoleConstructionGroup],
) -> Vec<FeatureHolePackageConstructionGroupUse> {
    let group_key = |group: &FeatureSimpleHoleConstructionGroup| {
        [
            group.first_data_blocks[0].clone(),
            group.first_data_blocks[1].clone(),
            group.second_data_blocks[0].clone(),
            group.second_data_blocks[1].clone(),
        ]
    };
    let mut groups_by_blocks = BTreeMap::<[String; 4], Vec<_>>::new();
    for group in groups {
        groups_by_blocks
            .entry(group_key(group))
            .or_default()
            .push(group);
    }
    let mut lanes_by_blocks = BTreeMap::<[String; 4], Vec<_>>::new();
    for lane in lanes {
        lanes_by_blocks
            .entry(lane.references.each_ref().map(|reference| reference.data_block.clone()))
            .or_default()
            .push(lane);
    }
    groups_by_blocks
        .into_iter()
        .filter_map(|(blocks, groups)| {
            let [group] = groups.as_slice() else {
                return None;
            };
            let [lane] = lanes_by_blocks.get(&blocks)?.as_slice() else {
                return None;
            };
            Some(FeatureHolePackageConstructionGroupUse {
                id: lane.id.replacen(
                    "hole-package-construction-group-lane",
                    "hole-package-construction-group-use",
                    1,
                ),
                operation_label: lane.operation_label.clone(),
                construction_group_lane: lane.id.clone(),
                simple_hole_construction_group: group.id.clone(),
                source_offset: lane.source_offset,
            })
        })
        .collect()
}

pub(crate) fn parse_simple_hole_template(
    value: &str,
) -> Option<(
    SimpleHoleFamily,
    SimpleHoleForm,
    SimpleHoleExtent,
    SimpleHoleEndTreatment,
    SimpleHoleEndTreatment,
)> {
    let tokens = value.split('_').collect::<Vec<_>>();
    if tokens.len() < 4 || tokens[0] != "Hole" || tokens[1] != "GeneralHole" {
        return None;
    }
    let form = match tokens[2] {
        "Simple" => SimpleHoleForm::Simple,
        "Counterbored" => SimpleHoleForm::Counterbored,
        "Countersunk" => SimpleHoleForm::Countersunk,
        _ => return None,
    };
    let extent = match tokens[3] {
        "Through" => SimpleHoleExtent::Through,
        "Blind" => SimpleHoleExtent::Blind,
        _ => return None,
    };
    let (start_treatment, end_treatment) = match (form, extent, &tokens[4..]) {
        (SimpleHoleForm::Simple, SimpleHoleExtent::Through, ["StartChamfer", "EndChamfer"]) => (
            SimpleHoleEndTreatment::Chamfer,
            SimpleHoleEndTreatment::Chamfer,
        ),
        (
            SimpleHoleForm::Counterbored | SimpleHoleForm::Countersunk,
            SimpleHoleExtent::Through,
            [],
        )
        | (SimpleHoleForm::Simple | SimpleHoleForm::Countersunk, SimpleHoleExtent::Blind, []) => {
            (SimpleHoleEndTreatment::None, SimpleHoleEndTreatment::None)
        }
        _ => return None,
    };
    Some((
        SimpleHoleFamily::GeneralHole,
        form,
        extent,
        start_treatment,
        end_treatment,
    ))
}

pub(crate) fn parse_threaded_hole_template(
    value: &str,
) -> Option<(ThreadedHoleFamily, SimpleHoleExtent)> {
    match value {
        "Hole_ThreadedHole_M Profile_Blind" => {
            Some((ThreadedHoleFamily::MProfile, SimpleHoleExtent::Blind))
        }
        "Hole_ThreadedHole_UNC_Blind" => Some((ThreadedHoleFamily::Unc, SimpleHoleExtent::Blind)),
        _ => None,
    }
}

/// Decode complete body-reference fields from feature-history operations.
pub fn feature_body_references(container: &Container) -> Vec<FeatureBodyReference> {
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(reference) = crate::om::operation_body_reference(record) else {
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
                body_object_index: reference.object_index,
                raw_body_object_index: reference.raw_object_index,
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
                crate::om::operation_body_references(record)
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, reference)| FeatureBodyReference {
                        id: format!(
                            "nx:feature-history:body-reference-occurrence#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                        ),
                        operation_label: operation_label.clone(),
                        ordinal: Some(ordinal as u32),
                        body_object_index: reference.object_index,
                        raw_body_object_index: reference.raw_object_index,
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
            && frame.object_id == reference.body_object_index
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
                    reference.body_object_index,
                    bindings,
                )?
            } else {
                crate::native::segments::unique_segment_body_binding(
                    reference.body_object_index,
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
                        && block.block_ordinal == reference.body_object_index
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
        |section, section_key, entry_offset, operation_ordinal, record| {
            let label = record.label;
            for (input_slot, object_index) in label.object_indices.into_iter().enumerate() {
                let Some(object_index) = object_index else {
                    continue;
                };
                let Some(data_block) = unique_offset_data_block(&indexed, object_index) else {
                    continue;
                };
                let Some(record_area) = section.record_area else {
                    continue;
                };
                let record_area_offset = record_area.offset;
                let token_offset = label.object_index_offsets[input_slot];
                let token_end = label
                    .object_index_offsets
                    .get(input_slot + 1)
                    .copied()
                    .unwrap_or(label.offset);
                let Some(start) = token_offset.checked_sub(record_area_offset) else {
                    continue;
                };
                let Some(end) = token_end.checked_sub(record_area_offset) else {
                    continue;
                };
                let Some(raw_object_index) = record_area.bytes.get(start..end).map(<[u8]>::to_vec)
                else {
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
                    input_slot: input_slot as u8,
                    object_index,
                    raw_object_index,
                    data_block,
                    source_offset: entry_offset + label.object_index_offsets[input_slot] as u64,
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
type ColumnSlotsByBlock<'a> = BTreeMap<&'a str, Vec<(&'a str, ColumnIndexRowKind, usize, u64)>>;

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
        for (slot, token) in row.indices.iter().enumerate() {
            slots_by_block.entry(&token.target.data_block).or_default().push((row.id.as_str(), ColumnIndexRowKind::Index, slot, token.source_offset));
        }
    }
    for row in linked_rows {
        for (slot, token) in std::iter::once(&row.target).chain(&row.indices).enumerate() {
            slots_by_block.entry(&token.target.data_block).or_default().push((row.id.as_str(), ColumnIndexRowKind::LinkedIndex, slot, token.source_offset));
        }
    }
    for row in target_rows {
        for (slot, token) in std::iter::once(&row.target).chain(&row.indices).enumerate() {
            slots_by_block.entry(&token.target.data_block).or_default().push((row.id.as_str(), ColumnIndexRowKind::TargetIndex, slot, token.source_offset));
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
                        row_slot: *slot as u8,
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
            construction
                .references
                .iter()
                .enumerate()
                .flat_map(|(construction_slot, reference)| {
                    let data_block = &reference.data_block;
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
                                construction_slot: construction_slot as u8,
                                row_kind: *row_kind,
                                column_row: (*row).to_string(),
                                column_table: table_by_row
                                    .get(row)
                                    .and_then(|table| *table)
                                    .map(str::to_string),
                                row_slot: *row_slot as u8,
                                data_block: data_block.clone(),
                                construction_source_offset: reference.source_offset,
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
                        && use_.row_slot == 0
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
                                leading_index: row.first_index.atom.value(),
                                leading_index_source_offset: row.first_index.offset,
                                discriminator: row.discriminator,
                                flag: row.flag,
                            },
                            row.indices.each_ref().map(|token| token.target.atom.value()),
                            row.indices.each_ref().map(|token| token.target.data_block.clone()),
                            row.indices.each_ref().map(|token| token.source_offset),
                            row.mode,
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
                            row.indices.each_ref().map(|token| token.target.atom.value()),
                            row.indices.each_ref().map(|token| token.target.data_block.clone()),
                            row.indices.each_ref().map(|token| token.source_offset),
                            row.mode,
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
            let Some(field) = crate::om::datum_csys_references(record) else {
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
            let resolved = field.references.map(|reference| {
                unique_offset_data_block(&indexed, reference.token.value()).map(|data_block| {
                    ConstructionReference {
                        token: reference.token,
                        data_block,
                        source_offset: entry_offset + reference.offset as u64,
                    }
                })
            });
            let [Some(a), Some(b), Some(c), Some(d), Some(e), Some(f), Some(g), Some(h)] = resolved else {
                return;
            };
            let resolved = [a, b, c, d, e, f, g, h];
            if resolved.iter().any(|reference| {
                reference.data_block
                    .rsplit_once(":block#")
                    .is_none_or(|(prefix, _)| prefix != input_prefix)
            }) {
                return;
            }
            constructions.push(FeatureDatumCsysConstruction {
                id: format!(
                    "nx:feature-history:datum-csys-construction#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                control: field.control,
                references: resolved,
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
            !header
                .resolved_references(DatumPlaneBlockLane::Object)
                .is_empty()
        })
        .filter_map(|header| {
            let data_blocks = header
                .resolved_references(DatumPlaneBlockLane::Object)
                .iter()
                .map(|reference| reference.data_block.clone())
                .collect::<Vec<_>>();
            let (payload, content) = FeaturePayloadContent::from_source(data_blocks, &blocks)?;
            let lanes = crate::om::datum_plane_object_index_lanes(&payload);
            let lane = match lanes.as_slice() {
                [lane] => Some(lane),
                _ => None,
            };
            let key = header.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
            Some(FeatureDatumPlanePayload {
                id: format!("nx:feature-history:datum-plane-payload#{key}"),
                operation_label: header.operation_label.clone(),
                datum_plane_header: header.id.clone(),
                content,
                index_lane: lane.map(|lane| FeatureDatumPlaneIndexLane {
                    offset: lane.offset as u64,
                    trailer: lane.trailer,
                    entries: lane
                        .indices
                        .iter()
                        .map(|token| FeatureDatumPlaneIndexLaneEntry {
                            value: token.value,
                            raw: token.raw.clone(),
                            offset: token.offset as u64,
                        })
                        .collect(),
                }),
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
                construction.references[0].data_block.clone(),
                construction.references[1].data_block.clone(),
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
            let Some(joined) =
                JoinedPayload::from_source(data_blocks(payload).iter().map(|block| &block.id), &blocks)
            else {
                return Vec::new();
            };
            let source_offset = |relative: usize| {
                joined.source_offset(relative as u64)
            };
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
        crate::om::object_payload_scalar_pairs,
        |payload, ordinal, pair, source_offset| {
            Some(FeaturePayloadScalarPair {
                id: format!("{}-scalar-pair-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                payload: FeatureScalarPairPayload::DatumCsys {
                    datum_csys_payload: payload.id.clone(),
                    discriminator: pair.discriminator,
                },
                ordinal: ordinal as u32,
                values: resolved_payload_scalar_pair(pair.values, source_offset)?,
                payload_offset: pair.offset as u64,
                source_offset: source_offset(pair.offset)?,
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
            [CsysDescriptorSlot::Five, CsysDescriptorSlot::Six, CsysDescriptorSlot::Seven]
                .into_iter()
                .filter_map(|slot| {
                    let reference_ordinal = u8::from(slot);
                    let data_block = &construction.references[usize::from(reference_ordinal)].data_block;
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
        crate::om::datum_plane_object_scalar_pairs,
        |payload, ordinal, pair, source_offset| {
            Some(FeaturePayloadScalarPair {
                id: format!("{}-scalar-pair-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                payload: FeatureScalarPairPayload::DatumPlane {
                    datum_plane_payload: payload.id.clone(),
                },
                ordinal: ordinal as u32,
                values: resolved_payload_scalar_pair(pair.values, source_offset)?,
                payload_offset: pair.offset as u64,
                source_offset: source_offset(pair.offset)?,
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
                .resolved_references(DatumPlaneBlockLane::Descriptor)
                .iter()
                .enumerate()
                .filter_map(|(ordinal, reference)| {
                    let data_block = &reference.data_block;
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
            for (reference_ordinal, reference) in
                header.resolved_references(lane).iter().enumerate()
            {
                let data_block = &reference.data_block;
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
        for (reference_ordinal, reference) in construction.references.iter().enumerate() {
            let data_block = &reference.data_block;
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
                    reference_ordinal: reference_ordinal as u8,
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
            payload_references.sort_by_key(|reference| reference.ordinal);
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
        field.sort_by_key(|reference| reference.ordinal);
        let Some(first) = field.first() else {
            continue;
        };
        let expected_len = usize::from(first.declared_count.max(1));
        if field.len() != expected_len
            || field.iter().enumerate().any(|(ordinal, reference)| {
                reference.declared_count != first.declared_count
                    || reference.ordinal != ordinal as u32
                    || reference.terminal != (ordinal + 1 == expected_len)
            })
        {
            continue;
        }
        let Some((terminal, members)) = field.split_last() else {
            continue;
        };
        let Some(members) = members
            .iter()
            .map(|reference| Some(FeatureConstructionMember {
                reference: reference.id.clone(), data_block: reference.data_block.clone()?,
            }))
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
            let mut data_blocks = construction.members.iter().map(|member| member.data_block.clone()).collect::<Vec<_>>();
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
        crate::om::sketch_payload_scalar_pairs,
        |payload, ordinal, pair, source_offset| {
            Some(FeaturePayloadScalarPair {
                id: format!("{}-coordinate-pair-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                payload: FeatureScalarPairPayload::Construction {
                    construction_payload: payload.id.clone(),
                    discriminator: pair.discriminator,
                },
                ordinal: ordinal as u32,
                values: resolved_payload_scalar_pair(pair.values, source_offset)?,
                payload_offset: pair.offset as u64,
                source_offset: source_offset(pair.offset)?,
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
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
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
            let mut data_blocks = construction.members.iter().map(|member| member.data_block.clone()).collect::<Vec<_>>();
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
) -> Vec<FeatureSketchPayloadName> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .flat_map(|construction| {
            let mut data_blocks = construction.members.iter().map(|member| member.data_block.clone()).collect::<Vec<_>>();
            data_blocks.push(construction.terminal_data_block.clone());
            let Some(joined) = JoinedPayload::from_source(data_blocks.iter(), &blocks)
            else {
                return Vec::new();
            };
            let construction_payload = construction.id.replacen(
                "sketch-construction-inputs",
                "sketch-construction-payload",
                1,
            );
            crate::om::construction_payload_named_fields(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, field)| {
                    let relative = field.offset as u64;
                    let source_offset = joined.source_offset(relative)?;
                    Some(FeatureSketchPayloadName {
                        id: format!(
                            "nx:feature-history:sketch-payload-name#{}-{ordinal:010}",
                            construction_payload
                                .rsplit_once('#')
                                .map_or("unknown", |(_, key)| key)
                        ),
                        operation_label: construction.operation_label.clone(),
                        construction_payload: construction_payload.clone(),
                        ordinal: ordinal as u32,
                        type_code: field.type_code.map(|code| FeaturePayloadTypeCode {
                            value: code.value,
                            raw: code.raw,
                            payload_offset: code.offset as u64,
                            source_offset: joined.source_offset(code.offset as u64),
                        }),
                        value: field.value.to_string(),
                        payload_offset: relative,
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
    names: &[FeatureSketchPayloadName],
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
        payload_names.sort_by_key(|name| name.payload_offset);
        for (ordinal, name) in payload_names.iter().enumerate() {
            let end = payload_names
                .get(ordinal + 1)
                .map_or(payload.content.byte_len(), |next| next.payload_offset);
            let mut scalar_fields = scalars
                .iter()
                .filter(|scalar| {
                    scalar.payload.id() == payload.id
                        && scalar.payload_offset > name.payload_offset
                        && scalar.payload_offset < end
                })
                .collect::<Vec<_>>();
            scalar_fields.sort_by_key(|scalar| scalar.payload_offset);
            let mut record_fixed_pairs = fixed_pairs
                .iter()
                .filter(|pair| {
                    pair.construction_payload == payload.id
                        && pair.position.offset() > name.payload_offset
                        && pair.position.offset() < end
                })
                .collect::<Vec<_>>();
            record_fixed_pairs.sort_by_key(|pair| pair.position.offset());
            let mut record_mixed_pairs = mixed_pairs
                .iter()
                .filter(|pair| {
                    pair.construction_payload == payload.id
                        && pair.position.offset() > name.payload_offset
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
                payload_start_offset: name.payload_offset,
                payload_end_offset: end,
            });
        }
    }
    records
}

/// Decode complete `Point<decimal>` records with exactly two scalar fields.
pub fn feature_sketch_points(
    records: &[FeatureSketchPayloadNamedRecord],
    names: &[FeatureSketchPayloadName],
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
            parse_sketch_point_name(&name.value)?;
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
                name: name.value.clone(),
                scalar_fields: [first.id.clone(), second.id.clone()],
                coordinates: [first.scalar.value(), second.scalar.value()],
            })
        })
        .collect()
}

/// Decode `Point<positive decimal>` records containing exactly one fixed pair.
pub fn feature_sketch_fixed_points(
    records: &[FeatureSketchPayloadNamedRecord],
    names: &[FeatureSketchPayloadName],
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
                    fixed_pairs
                        .get(fixed_pair_id.as_str())
                        .is_some_and(|pair| matches!(pair.position.form(), SketchPairForm::Legacy | SketchPairForm::Short | SketchPairForm::Extended))
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
            parse_sketch_point_name(&name.value)?;
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
                name: name.value.clone(),
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
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
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
                    reference.ordinal
                ),
                operation_label: reference.operation_label.clone(),
                sketch_reference: reference.id.clone(),
                reference_ordinal: reference.ordinal,
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
        operation_references.sort_by_key(|reference| reference.ordinal);
        let Some((first_reference, first_block)) = operation_references.first()
            .and_then(|reference| Some((*reference, reference.data_block.as_deref()?))) else {
            continue;
        };
        let complete_lane = operation_references
                .iter()
                .enumerate()
                .all(|(ordinal, reference)| {
                    reference.ordinal == ordinal as u32
                        && usize::from(reference.declared_count) == operation_references.len()
                        && reference.data_block.is_some()
                        && reference.terminal == (ordinal + 1 == operation_references.len())
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
                .references
                .iter()
                .map(|reference| &reference.data_block)
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
            let construction_first_block = &construction.references[0].data_block;
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
            let Some(decoded) = crate::om::sketch_payload_references(record) else {
                return;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let declared_count = decoded.declared_count;
            let terminal_ordinal = decoded.references.len() - 1;
            references.extend(decoded.references.into_iter().enumerate().map(|(ordinal, reference)| {
                let data_block = unique_offset_data_block(&indexed, reference.token.value());
                FeatureSketchReference {
                    id: format!(
                        "nx:feature-history:sketch-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: ordinal as u32,
                    declared_count,
                    terminal: ordinal == terminal_ordinal,
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
    token: crate::om::reference_index::ReferenceIndexToken,
    data_block: Option<String>,
    source_offset: u64,
}

fn resolved_feature_payload_references(
    container: &Container,
    decode: impl Fn(crate::om::OperationRecord<'_>) -> Option<Vec<crate::om::PayloadObjectReference>>,
) -> Vec<ResolvedFeaturePayloadReference> {
    let indexed = container.indexed_om_sections();
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(decoded) = decode(record) else {
                return;
            };
            references.extend(decoded.into_iter().enumerate().map(|(ordinal, reference)| {
                ResolvedFeaturePayloadReference {
                    section_key: section_key.to_string(),
                    operation_ordinal,
                    ordinal,
                    token: reference.token,
                    data_block: unique_offset_data_block(&indexed, reference.token.value()),
                    source_offset: entry_offset + reference.offset as u64,
                }
            }));
        },
    );
    references
}

/// Decode and resolve the exact ordered construction-reference field in
/// projected-curve payloads without assigning semantic roles to its slots.
pub fn feature_projected_curve_references(
    container: &Container,
) -> Vec<FeatureProjectedCurveReference> {
    resolved_feature_payload_references(container, |record| {
        crate::om::projected_curve_payload_references(record).map(|field| field.references)
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

/// Decode and resolve exact `FSET` reference graphs without assigning semantic
/// roles to either reference group.
pub fn feature_fset_reference_graphs(container: &Container) -> Vec<FeatureFsetReferenceGraph> {
    let indexed = container.indexed_om_sections();
    let mut graphs = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(graph) = crate::om::fset_payload_reference_graph(record) else {
                return;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            graphs.push(FeatureFsetReferenceGraph {
                id: format!(
                    "nx:feature-history:fset-reference-graph#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                selector: graph.selector,
                first: graph.first.map(|reference| ConstructionReference {
                    token: reference.token,
                    data_block: unique_offset_data_block(&indexed, reference.token.value()),
                    source_offset: entry_offset + reference.offset as u64,
                }),
                second: graph.second.map(|reference| ConstructionReference {
                    token: reference.token,
                    data_block: unique_offset_data_block(&indexed, reference.token.value()),
                    source_offset: entry_offset + reference.offset as u64,
                }),
                source_offset: entry_offset + graph.offset as u64,
            });
        },
    );
    graphs
}

/// Reconstruct the two ordered logical payloads selected by each complete
/// same-store `FSET` reference graph.
pub fn feature_fset_construction_payloads(
    container: &Container,
    graphs: &[FeatureFsetReferenceGraph],
) -> Vec<FeatureConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    graphs
        .iter()
        .flat_map(|graph| {
            let blocks = &blocks;
            [
                (
                    FeatureFsetReferenceGroup::First,
                    graph.first.iter(),
                ),
                (
                    FeatureFsetReferenceGroup::Second,
                    graph.second.iter(),
                ),
            ]
            .into_iter()
            .filter_map(move |(group, source_blocks)| {
                let data_blocks = source_blocks.map(|reference| reference.data_block.clone()).collect::<Option<Vec<_>>>()?;
                let store = data_blocks.first()?.rsplit_once(":block#")?.0;
                if data_blocks.iter().any(|block| {
                    block
                        .rsplit_once(":block#")
                        .is_none_or(|(prefix, _)| prefix != store)
                }) {
                    return None;
                }
                let (_, content) = FeaturePayloadContent::from_source(data_blocks, blocks)?;
                let group_name = match group {
                    FeatureFsetReferenceGroup::First => "first",
                    FeatureFsetReferenceGroup::Second => "second",
                };
                let operation_key = graph
                    .operation_label
                    .strip_prefix("nx:feature-history:operation-label#")?;
                Some(FeatureConstructionPayload {
                    id: format!(
                        "nx:feature-history:fset-construction-payload#{operation_key}-{group_name}"
                    ),
                    operation_label: graph.operation_label.clone(),
                    owner: FeatureConstructionOwner::Fset {
                        reference_graph: graph.id.clone(),
                        group,
                    },
                    content,
                })
            })
        })
        .collect()
}

/// Decode exact `DELETE` payload reference fields and independently resolve
/// their non-null slots without assigning a target object family.
pub fn feature_delete_reference_fields(container: &Container) -> Vec<FeatureDeleteReferenceField> {
    let indexed = container.indexed_om_sections();
    let mut fields = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(field) = crate::om::delete_payload_references(record) else {
                return;
            };
            fields.push(FeatureDeleteReferenceField {
                id: format!(
                    "nx:feature-history:delete-reference-field#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                control: field.control,
                references: field.references.map(|reference| NullableConstructionReference {
                    target: reference.token.map(|token| {
                        let data_block = unique_offset_data_block(&indexed, token.value());
                        (token, data_block)
                    }),
                    source_offset: entry_offset + reference.offset as u64,
                }),
                source_offset: entry_offset + field.offset as u64,
            });
        },
    );
    fields
}

/// Reconstruct one ordered logical payload from each complete same-store
/// non-null `DELETE` reference field.
pub fn feature_delete_construction_payloads(
    container: &Container,
    fields: &[FeatureDeleteReferenceField],
) -> Vec<FeatureDeleteConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    fields
        .iter()
        .filter_map(|field| {
            let data_blocks = field
                .references
                .iter()
                .map(|reference| reference.target.as_ref()?.1.clone())
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
            let operation_key = field
                .operation_label
                .strip_prefix("nx:feature-history:operation-label#")?;
            Some(FeatureDeleteConstructionPayload {
                id: format!("nx:feature-history:delete-construction-payload#{operation_key}"),
                operation_label: field.operation_label.clone(),
                reference_field: field.id.clone(),
                content,
            })
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
            let Some(joined) =
                JoinedPayload::from_source(payload.content.block_ids(), &blocks)
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

/// Decode and resolve exact ordered construction references in pattern
/// payloads without assigning seed or transform semantics to their slots.
pub fn feature_pattern_references(container: &Container) -> Vec<FeaturePatternReference> {
    let indexed = container.indexed_om_sections();
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(decoded) = crate::om::pattern_payload_references(record) else {
                return;
            };
            let layout = match decoded.layout {
                crate::om::PatternPayloadReferenceLayout::CanonicalGraph => {
                    FeaturePatternReferenceLayout::CanonicalGraph
                }
                crate::om::PatternPayloadReferenceLayout::CompactGraph => {
                    FeaturePatternReferenceLayout::CompactGraph
                }
                crate::om::PatternPayloadReferenceLayout::GeometryInstance => {
                    FeaturePatternReferenceLayout::GeometryInstance
                }
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            references.extend(decoded.references.into_iter().enumerate().map(|(ordinal, reference)| {
                FeaturePatternReference {
                    id: format!(
                        "nx:feature-history:pattern-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    layout,
                    ordinal: ordinal as u32,
                    token: reference.token,
                    data_block: unique_offset_data_block(&indexed, reference.token.value()),
                    source_offset: entry_offset + reference.offset as u64,
                }
            }));
        },
    );
    references
}

/// Decode and resolve the exact counted reference lane in `Pattern Feature`
/// payloads without assigning roles to its references.
pub fn feature_pattern_counted_reference_lanes(
    container: &Container,
) -> Vec<FeaturePatternCountedReferenceLane> {
    let indexed = container.indexed_om_sections();
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(lane) = crate::om::pattern_payload_counted_reference_lane(record) else {
                return;
            };
            lanes.push(FeaturePatternCountedReferenceLane {
                id: format!(
                    "nx:feature-history:pattern-counted-reference-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                references: lane.references.into_iter().map(|reference| FeatureDataBlockToken {
                    value: reference.token.value(),
                    raw: reference.token.raw().to_vec(),
                    data_block: unique_offset_data_block(&indexed, reference.token.value()),
                    source_offset: entry_offset + reference.offset as u64,
                }).collect(),
                source_offset: entry_offset + lane.offset as u64,
            });
        },
    );
    lanes
}

/// Reconstruct ordered logical payloads from complete pattern-reference graphs.
pub fn feature_pattern_construction_payloads(
    container: &Container,
    labels: &[FeatureOperationLabel],
    references: &[FeaturePatternReference],
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
            let operation_kind = match *kinds.get(operation_label)? {
                "Pattern Feature" => FeaturePatternKind::Feature,
                "Pattern Geometry" => FeaturePatternKind::Geometry,
                _ => return None,
            };
            let mut graph = references
                .iter()
                .filter(|reference| reference.operation_label == operation_label)
                .collect::<Vec<_>>();
            graph.sort_by_key(|reference| reference.ordinal);
            if !matches!(graph.len(), 9 | 10)
                || graph
                    .iter()
                    .enumerate()
                    .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
                || graph
                    .iter()
                    .any(|reference| reference.layout != graph[0].layout)
            {
                return None;
            }
            let data_blocks = graph
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
                id: format!("nx:feature-history:pattern-construction-payload#{operation_key}"),
                operation_label: operation_label.to_string(),
                owner: FeatureConstructionOwner::Pattern {
                    operation_kind,
                    reference_layout: graph[0].layout,
                    construction_references: graph
                        .iter()
                        .map(|reference| reference.id.clone())
                        .collect(),
                },
                content,
            })
        })
        .collect()
}

/// Decode canonical printable strings from reconstructed pattern payloads.
pub fn feature_pattern_construction_strings(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Vec<FeaturePatternConstructionString> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) =
                JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::string_values(joined.bytes(), 0)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, value)| {
                    let payload_offset = value.offset as u64;
                    Some(FeaturePatternConstructionString {
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

/// Decode complete signed Q1.55 lanes from reconstructed pattern payloads.
pub fn feature_pattern_construction_fixed_lanes(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Vec<FeaturePatternConstructionFixedLane> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) =
                JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::draft_construction_fixed_lanes(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, lane)| {
                    let payload_offset = lane.offset();
                    let lane = lane.try_map_locations(|offset, ()| joined.source_offset(offset))?;
                    Some(FeaturePatternConstructionFixedLane {
                        id: format!("{}-fixed-lane-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        lane,
                        source_offset: joined.source_offset(payload_offset)?,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode exact counted transform lanes from bounded pattern payloads.
pub fn feature_pattern_transform_lanes(container: &Container) -> Vec<FeaturePatternTransformLane> {
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(lane) = crate::om::pattern_payload_transform_lane(record) else {
                return;
            };
            lanes.push(FeaturePatternTransformLane {
                id: format!(
                    "nx:feature-history:pattern-transform-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                row_schema_index: lane.row_schema_index,
                rows: lane.rows.map(
                    |selector| FeatureIndexToken {
                        value: selector.value,
                        raw: selector.raw,
                        source_offset: entry_offset + selector.offset as u64,
                    },
                    |offset| entry_offset + offset as u64,
                ),
                source_offset: entry_offset + lane.offset as u64,
            });
        },
    );
    lanes
}

/// Decode exact counted output lanes from bounded multi-instance payloads.
pub fn feature_multi_instance_output_lanes(
    container: &Container,
) -> Vec<FeatureMultiInstanceOutputLane> {
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(lane) = crate::om::multi_instance_output_payload_lane(record) else {
                return;
            };
            lanes.push(FeatureMultiInstanceOutputLane {
                id: format!(
                    "nx:feature-history:multi-instance-output-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                rows: lane.rows.into_iter().map(|row| FeatureMultiInstanceOutputRow {
                    value: row.selector.value,
                    raw: row.selector.raw,
                    ordinal: row.ordinal,
                    source_offset: entry_offset + row.selector.offset as u64,
                }).collect(),
                trailing_references: lane.trailing_references.into_iter().map(|reference| FeatureIndexToken {
                    value: reference.token.value(),
                    raw: reference.token.raw().to_vec(),
                    source_offset: entry_offset + reference.offset as u64,
                }).collect(),
                source_offset: entry_offset + lane.offset as u64,
            });
        },
    );
    lanes
}

/// Decode exact counted selector lanes from bounded identical-instance output
/// payloads.
pub fn feature_identical_instance_output_lanes(
    container: &Container,
) -> Vec<FeatureIdenticalInstanceOutputLane> {
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(lane) = crate::om::identical_instance_output_payload_lane(record) else {
                return;
            };
            lanes.push(FeatureIdenticalInstanceOutputLane {
                id: format!(
                    "nx:feature-history:identical-instance-output-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                leading_schema_index: lane.leading_schema_index,
                count_schema_index: lane.count_schema_index,
                selectors: lane.selectors.into_iter().map(|token| FeatureIndexToken {
                    value: token.value,
                    raw: token.raw,
                    source_offset: entry_offset + token.offset as u64,
                }).collect(),
                source_offset: entry_offset + lane.offset as u64,
            });
        },
    );
    lanes
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
            let Some(header) = crate::om::point_feature_payload_header(record) else {
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
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let source_offsets = lane.value_offsets().map(|offset| {
            if offset < preceding.bytes.len() {
                entry_offset + preceding.offset as u64 + offset as u64
            } else {
                entry_offset + target.offset as u64 + (offset - preceding.bytes.len()) as u64
            }
        });
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
            values: std::array::from_fn(|slot| FeatureBinary64ScalarToken {
                scalar: lane.values[slot],
                source_offset: source_offsets[slot],
            }),
        });
    }
    lanes
}

/// Decode exact ordered draft construction references without assigning semantic roles.
pub fn feature_draft_construction_references(
    container: &Container,
) -> Vec<FeatureDraftConstructionReference> {
    resolved_feature_payload_references(container, |record| {
        crate::om::draft_feature_payload_references(record)
            .map(|field| field.references.into_iter().collect())
    })
    .into_iter()
    .map(|reference| {
        let operation_label = format!(
            "nx:feature-history:operation-label#{}-{:010}",
            reference.section_key, reference.operation_ordinal
        );
        FeatureDraftConstructionReference {
            id: format!(
                "nx:feature-history:draft-construction-reference#{}-{:010}-{:010}",
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

/// Decode exact counted compact-index lanes preceding draft construction graphs.
pub fn feature_draft_construction_index_lanes(
    container: &Container,
) -> Vec<FeatureDraftConstructionIndexLane> {
    let indexed = container.indexed_om_sections();
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(lane) = crate::om::draft_feature_leading_index_lane(record) else {
                return;
            };
            let section_ordinal =
                crate::om::draft_feature_payload_references(record).and_then(|graph| {
                    let complete_indices = graph
                        .references
                        .iter()
                        .map(|reference| reference.token.value())
                        .chain(lane.indices.iter().map(|token| token.value))
                        .collect::<Vec<_>>();
                    unique_offset_data_store(&indexed, &complete_indices)
                });
            let tokens = lane.indices.into_iter().map(|token| FeatureIndexToken {
                value: token.value,
                raw: token.raw,
                source_offset: entry_offset + token.offset as u64,
            });
            let indices = match section_ordinal {
                None => FeatureDraftConstructionIndices::Unresolved(tokens.collect()),
                Some(section_ordinal) => FeatureDraftConstructionIndices::Resolved(
                    tokens
                        .map(|token| {
                            let data_block = format!(
                                "nx:om-data-blocks-{section_ordinal}:block#{}",
                                token.value
                            );
                            FeatureResolvedIndexToken { token, data_block }
                        })
                        .collect(),
                ),
            };
            lanes.push(FeatureDraftConstructionIndexLane {
                id: format!(
                    "nx:feature-history:draft-construction-index-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                indices,
            });
        },
    );
    lanes
}

/// Reconstruct ordered logical payloads from resolved draft index lanes.
pub fn feature_draft_construction_payloads(
    container: &Container,
    lanes: &[FeatureDraftConstructionIndexLane],
) -> Vec<FeatureConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    lanes
        .iter()
        .filter_map(|lane| {
            let FeatureDraftConstructionIndices::Resolved(tokens) = &lane.indices else {
                return None;
            };
            let data_blocks = tokens
                .iter()
                .map(|row| row.data_block.clone())
                .collect::<Vec<_>>();
            let (_, content) = FeaturePayloadContent::from_source(data_blocks, &blocks)?;
            Some(FeatureConstructionPayload {
                id: lane.id.replacen(
                    "draft-construction-index-lane#",
                    "draft-construction-payload#",
                    1,
                ),
                operation_label: lane.operation_label.clone(),
                owner: FeatureConstructionOwner::Draft {
                    index_lane: lane.id.clone(),
                },
                content,
            })
        })
        .collect()
}

/// Reconstruct ordered logical payloads from complete draft construction graphs.
pub fn feature_draft_construction_graph_payloads(
    container: &Container,
    lanes: &[FeatureDraftConstructionIndexLane],
    references: &[FeatureDraftConstructionReference],
) -> Vec<FeatureDraftConstructionGraphPayload> {
    let blocks = offset_data_block_bytes(container);
    lanes
        .iter()
        .filter_map(|lane| {
            let FeatureDraftConstructionIndices::Resolved(tokens) = &lane.indices else {
                return None;
            };
            let store = tokens.first()?.data_block.rsplit_once(":block#")?.0;
            let mut graph = references
                .iter()
                .filter(|reference| reference.operation_label == lane.operation_label)
                .collect::<Vec<_>>();
            graph.sort_by_key(|reference| reference.ordinal);
            if graph
                .iter()
                .enumerate()
                .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
            {
                return None;
            }
            let graph: [&FeatureDraftConstructionReference; 4] = graph.try_into().ok()?;
            let data_blocks = graph
                .each_ref()
                .map(|reference| reference.data_block.clone())
                .into_iter()
                .collect::<Option<Vec<_>>>()?;
            if data_blocks.iter().any(|block| {
                block
                    .rsplit_once(":block#")
                    .is_none_or(|(prefix, _)| prefix != store)
            }) {
                return None;
            }
            let (_, content) = FeaturePayloadContent::from_source(data_blocks, &blocks)?;
            let (_, key) = lane.id.rsplit_once('#')?;
            Some(FeatureDraftConstructionGraphPayload {
                id: format!("nx:feature-history:draft-construction-graph-payload#{key}"),
                operation_label: lane.operation_label.clone(),
                index_lane: lane.id.clone(),
                construction_references: graph.each_ref().map(|reference| reference.id.clone()),
                content,
            })
        })
        .collect()
}

/// Decode complete signed Q1.55 lanes from reconstructed draft graph payloads.
pub fn feature_draft_construction_fixed_lanes(
    container: &Container,
    payloads: &[FeatureDraftConstructionGraphPayload],
) -> Vec<FeatureDraftConstructionFixedLane> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) =
                JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::draft_construction_fixed_lanes(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, lane)| {
                    let payload_offset = lane.offset();
                    let lane = lane.try_map_locations(|offset, ()| joined.source_offset(offset))?;
                    Some(FeatureDraftConstructionFixedLane {
                        id: format!("{}-fixed-lane-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        graph_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        lane,
                        source_offset: joined.source_offset(payload_offset)?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode complete shifted-binary32 lanes from reconstructed draft graph payloads.
pub fn feature_draft_construction_binary32_lanes(
    container: &Container,
    payloads: &[FeatureDraftConstructionGraphPayload],
) -> Vec<FeatureDraftConstructionBinary32Lane> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) =
                JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::draft_construction_binary32_lanes(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, lane)| {
                    let payload_offset = lane.offset();
                    let lane = lane.try_map_locations(|offset, ()| joined.source_offset(offset))?;
                    Some(FeatureDraftConstructionBinary32Lane {
                        id: format!("{}-binary32-lane-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        graph_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        lane,
                        source_offset: joined.source_offset(payload_offset)?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode canonical printable strings from reconstructed draft graph payloads.
pub fn feature_draft_construction_graph_strings(
    container: &Container,
    payloads: &[FeatureDraftConstructionGraphPayload],
) -> Vec<FeatureDraftConstructionGraphString> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) =
                JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::string_values(joined.bytes(), 0)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, value)| {
                    let payload_offset = value.offset as u64;
                    Some(FeatureDraftConstructionGraphString {
                        id: format!("{}-string-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        graph_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        value: value.value.into_owned(),
                        payload_offset,
                        source_offset: joined.source_offset(payload_offset)?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode complete identity frames from reconstructed draft construction payloads.
pub fn feature_draft_construction_identity_frames(
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Vec<FeatureDraftConstructionIdentityFrame> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) =
                JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::draft_construction_identity_frames(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, frame)| {
                    let payload_offset = frame.offset();
                    let identity_payload_offset = frame.identity_offset();
                    Some(FeatureDraftConstructionIdentityFrame {
                        id: format!("{}-identity-frame-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        draft_construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        frame,
                        source_offset: joined.source_offset(payload_offset)?,
                        identity_source_offset: joined.source_offset(identity_payload_offset)?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode complete end-anchored terminal lanes from draft construction payloads.
pub fn feature_draft_construction_terminal_lanes(
    container: &Container,
) -> Vec<FeatureDraftConstructionTerminalLane> {
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(lane) = crate::om::draft_feature_terminal_lane(record) else {
                return;
            };
            lanes.push(FeatureDraftConstructionTerminalLane {
                id: format!(
                    "nx:feature-history:draft-construction-terminal-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                indices: lane.indices,
                raw_indices: lane.raw_indices,
                tail: lane.tail,
                index_source_offsets: lane
                    .index_offsets
                    .map(|offset| entry_offset + offset as u64),
            });
        },
    );
    lanes
}

/// Decode and resolve the exact common reference envelope in surface-feature
/// payloads without assigning section or guide semantics to its slots.
pub fn feature_surface_construction_references(
    container: &Container,
) -> Vec<FeatureSurfaceConstructionReference> {
    resolved_feature_payload_references(container, |record| {
        crate::om::surface_feature_payload_references(record)
            .map(|field| field.references.into_iter().collect())
            .or_else(|| {
                crate::om::thru_curve_payload_references(record)
                    .map(|field| field.references.into_iter().collect())
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
            let Some(field) = crate::om::thru_curve_payload_references(record) else {
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
                source_offset: entry_offset + record.payload_offset as u64,
            });
        },
    );
    envelopes
}

/// Decode and resolve exact counted `THRU_CURVE` construction branches.
pub fn feature_thru_curve_construction_branch_groups(
    container: &Container,
) -> Vec<FeatureThruCurveConstructionBranchGroup> {
    let indexed = container.indexed_om_sections();
    let mut groups = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(group) = crate::om::thru_curve_payload_branch_group(record) else {
                return;
            };
            let operation_key = format!("{section_key}-{operation_ordinal:010}");
            let resolve = |ordinal: usize, reference: crate::om::PayloadObjectReference| {
                FeatureSurfaceBranchReference {
                    ordinal: ordinal as u32,
                    token: reference.token,
                    data_block: unique_offset_data_block(&indexed, reference.token.value()),
                    source_offset: entry_offset + reference.offset as u64,
                }
            };
            let branches = group.branches.map_indexed(|ordinal, branch| {
                    let members = branch.members.map_indexed(&resolve);
                    let terminal = resolve(members.len(), branch.terminal);
                    FeatureThruCurveConstructionBranch {
                        ordinal: ordinal as u32,
                        mode: branch.mode,
                        members,
                        terminal,
                        suffix: branch.suffix,
                        source_offset: entry_offset + branch.offset as u64,
                    }
                });
            groups.push(FeatureThruCurveConstructionBranchGroup {
                id: format!(
                    "nx:feature-history:thru-curve-construction-branch-group#{operation_key}"
                ),
                operation_label: format!("nx:feature-history:operation-label#{operation_key}"),
                branches,
                terminator: group.terminator,
                source_offset: entry_offset + group.offset as u64,
            });
        },
    );
    groups
}

/// Decode and resolve each exact leading `SWP104` construction branch.
pub fn feature_swp104_leading_branches(container: &Container) -> Vec<FeatureSwp104LeadingBranch> {
    let indexed = container.indexed_om_sections();
    let mut branches = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(branch) = crate::om::swp104_payload_leading_branch(record) else {
                return;
            };
            let operation_key = format!("{section_key}-{operation_ordinal:010}");
            let resolve = |ordinal: usize, reference: crate::om::PayloadObjectReference| {
                FeatureSurfaceBranchReference {
                    ordinal: ordinal as u32,
                    token: reference.token,
                    data_block: unique_offset_data_block(&indexed, reference.token.value()),
                    source_offset: entry_offset + reference.offset as u64,
                }
            };
            let members = branch.members.map_indexed(&resolve);
            let terminal = resolve(members.len(), branch.terminal);
            branches.push(FeatureSwp104LeadingBranch {
                id: format!("nx:feature-history:swp104-leading-branch#{operation_key}"),
                operation_label: format!("nx:feature-history:operation-label#{operation_key}"),
                discriminator: branch.discriminator,
                scalars: branch.scalars,
                leading_zero: branch.leading_zero,
                mode: branch.mode,
                state_lane: branch.state_lane,
                members,
                terminal,
                byte_len: (branch.end_offset - record.payload_offset) as u64,
                source_offset: entry_offset + record.payload_offset as u64,
            });
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
            let Some(joined) =
                JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::object_payload_scalar_pairs(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, pair)| {
                    Some(FeaturePayloadScalarPair {
                        id: format!("{}-scalar-pair-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        payload: FeatureScalarPairPayload::SurfaceConstruction {
                            surface_construction_payload: payload.id.clone(),
                            discriminator: pair.discriminator,
                        },
                        ordinal: ordinal as u32,
                        values: resolved_payload_scalar_pair(pair.values, |offset| joined.source_offset(offset as u64))?,
                        payload_offset: pair.offset as u64,
                        source_offset: joined.source_offset(pair.offset as u64)?,
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
            let Some(joined) =
                JoinedPayload::from_source(payload.content.block_ids(), &blocks)
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

/// Decode and resolve exact counted surface-construction branches without
/// assigning section or guide semantics to their members.
pub fn feature_surface_construction_branches(
    container: &Container,
) -> Vec<FeatureSurfaceConstructionBranch> {
    let indexed = container.indexed_om_sections();
    let mut branches = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(group) = crate::om::surface_feature_payload_branches(record) else {
                return;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            branches.extend(group.branches.into_iter().enumerate().map(|(ordinal, branch)| {
                let resolve = |ordinal: usize, reference: crate::om::PayloadObjectReference| {
                    FeatureSurfaceBranchReference {
                        ordinal: ordinal as u32,
                        token: reference.token,
                        data_block: unique_offset_data_block(&indexed, reference.token.value()),
                        source_offset: entry_offset + reference.offset as u64,
                    }
                };
                let members = branch.members.map_indexed(&resolve);
                let terminal = resolve(members.len(), branch.terminal);
                FeatureSurfaceConstructionBranch {
                    id: format!(
                        "nx:feature-history:surface-construction-branch#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: ordinal as u32,
                    family: group.family,
                    header_code: group.header_code,
                    mode: branch.mode,
                    witnessed: branch.witnessed,
                    members,
                    terminal,
                    suffix: branch.suffix,
                    source_offset: entry_offset + branch.offset as u64,
                }
            }));
        },
    );
    branches
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
            let Some(decoded) = crate::om::extrude_profile_references(record) else {
                return;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            references.extend(decoded.references.into_iter().enumerate().map(|(ordinal, row)| {
                let reference = row.reference;
                FeatureExtrudeProfileReference {
                    id: format!(
                        "nx:feature-history:extrude-profile-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: ordinal as u32,
                    field_tag: decoded.field_tag,
                    witness_source_offset: row.witness_offset.map(|offset| entry_offset + offset as u64),
                    token: reference.token,
                    data_block: unique_offset_data_block(&indexed, reference.token.value()),
                    source_offset: entry_offset + reference.offset as u64,
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
            let Some(header) = crate::om::extrude_payload_header(record) else {
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
            let Some(lane) = crate::om::operation_terminal_discriminator(record) else {
                return;
            };
            lanes.push(FeatureOperationTerminalDiscriminator {
                id: format!(
                    "nx:feature-history:operation-terminal-discriminator#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                type_indices: lane.type_indices.map(|token| FeatureIndexToken {
                    value: token.value,
                    raw: token.raw,
                    source_offset: entry_offset + token.offset as u64,
                }),
                flags: lane.flags,
                trailing_indices: lane.trailing_indices.into_iter().map(|token| FeatureIndexToken {
                    value: token.value,
                    raw: token.raw,
                    source_offset: entry_offset + token.offset as u64,
                }).collect(),
                source_offset: entry_offset + lane.offset as u64,
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
            for triple in crate::om::operation_body_scalar_triples(record) {
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
                    values: triple.scalars.map(|scalar| FeatureBodyScalarToken {
                        atom: scalar.atom,
                        source_offset: entry_offset + scalar.offset as u64,
                    }),
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
                crate::om::operation_body_members(record)
                    .into_iter()
                    .map(|member| FeatureOperationBodyMember {
                        id: format!(
                            "nx:feature-history:operation-body-member#{section_key}-{operation_ordinal:010}-{}-{}",
                            member.body_reference_ordinal, member.ordinal
                        ),
                        operation_label: format!(
                            "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                        ),
                        body_reference_ordinal: member.body_reference_ordinal,
                        body_object_index: member.body_object_index,
                        ordinal: member.ordinal,
                        member_index: member.member_index,
                        raw_member_index: member.raw_member_index,
                        source_offset: entry_offset + member.offset as u64,
                    }),
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
            if member.member_index == member.body_object_index {
                return None;
            }
            let member_store = unique_stores.get(member.operation_label.as_str()).copied();
            if member_store.is_none() && input_operations.contains(member.operation_label.as_str())
            {
                return None;
            }
            let operand_data_block = member_store.and_then(|store| {
                let id = format!("{store}:block#{}", member.member_index);
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
                        binding.body_object_index == member.member_index
                            || binding.body_alias_object_index == member.member_index
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
                operand_object_index: member.member_index,
                raw_operand_object_index: member.raw_member_index.clone(),
                operand_data_block,
                segment_body_bindings,
                source_offset: member.source_offset,
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
                crate::om::operation_body_11_continuations(record)
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
                        continuation_index: continuation.continuation_index,
                        raw_continuation_index: continuation.raw_continuation_index,
                        continuation_source_offset: entry_offset
                            + continuation.continuation_offset as u64,
                        terminal_object_index: continuation.terminal_object_index,
                        raw_terminal_object_index: continuation.raw_terminal_object_index,
                        terminal_source_offset: entry_offset + continuation.terminal_offset as u64,
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
            for lane in crate::om::operation_body_reference_lanes(record) {
                let encoding = match lane.encoding {
                    crate::om::OperationBodyReferenceLaneEncoding::CompactIndex => {
                        FeatureOperationBodyReferenceLaneEncoding::CompactIndex
                    }
                    crate::om::OperationBodyReferenceLaneEncoding::PayloadObjectIndex => {
                        FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex
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
                    encoding,
                    references: lane.values.into_iter().map(|value| FeatureDataBlockToken {
                        value: value.object_index,
                        raw: value.raw_value,
                        data_block: unique_offset_data_block(&indexed, value.object_index),
                        source_offset: entry_offset + value.offset as u64,
                    }).collect(),
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
        if operation_references.is_empty()
            || operation_references
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
            let Some(branch) = crate::om::extrude_payload_32_branch(record) else {
                return;
            };
            branches.push(FeatureExtrudePayload32Branch {
                id: format!(
                    "nx:feature-history:extrude-payload-32-branch#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                scalar: branch.scalar,
                atoms: branch.atoms.into_iter().map(|token| FeatureDataBlockToken {
                    value: token.value,
                    raw: token.raw,
                    source_offset: entry_offset + token.offset as u64,
                    data_block: unique_offset_data_block(&indexed, token.value),
                }).collect(),
                first_indices: branch.first_indices.into_iter().map(|token| FeatureDataBlockToken {
                    value: token.value,
                    raw: token.raw,
                    source_offset: entry_offset + token.offset as u64,
                    data_block: unique_offset_data_block(&indexed, token.value),
                }).collect(),
                second_indices: branch.second_indices.into_iter().map(|token| FeatureDataBlockToken {
                    value: token.value,
                    raw: token.raw,
                    source_offset: entry_offset + token.offset as u64,
                    data_block: unique_offset_data_block(&indexed, token.value),
                }).collect(),
                terminal: FeatureIndexToken {
                    value: branch.terminal_object_index,
                    raw: branch.raw_terminal_object_index,
                    source_offset: entry_offset + branch.terminal_offset as u64,
                },
                source_offset: entry_offset + branch.offset as u64,
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
        if profile.is_empty()
            || profile
                .iter()
                .enumerate()
                .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
        {
            continue;
        }
        let Some(profile_data_blocks) = profile
            .iter()
            .map(|reference| reference.data_block.clone())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(atom_data_blocks) = branch
            .atoms
            .iter()
            .map(|token| token.data_block.clone())
            .collect()
        else {
            continue;
        };
        let Some(first_data_blocks) = branch
            .first_indices
            .iter()
            .map(|token| token.data_block.clone())
            .collect()
        else {
            continue;
        };
        let Some(second_data_blocks) = branch
            .second_indices
            .iter()
            .map(|token| token.data_block.clone())
            .collect()
        else {
            continue;
        };
        constructions.push(FeatureExtrude32Construction {
            id: branch
                .id
                .replacen("extrude-payload-32-branch", "extrude-32-construction", 1),
            operation_label: branch.operation_label.clone(),
            branch: branch.id.clone(),
            body_object_index: branch.terminal.value,
            profile_references: profile
                .iter()
                .map(|reference| reference.id.clone())
                .collect(),
            profile_data_blocks,
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
            let Some(field) = crate::om::block_construction_references(record) else {
                return;
            };
            let terminal_ordinal = field.references.len() - 1;
            references.extend(field.references.into_iter().enumerate().map(
                |(ordinal, reference)| FeatureBlockConstructionReference {
                    id: format!(
                        "nx:feature-history:block-construction-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                    ),
                    control: field.control,
                    ordinal: ordinal as u32,
                    terminal: ordinal == terminal_ordinal,
                    token: reference.token,
                    data_block: unique_offset_data_block(&indexed, reference.token.value()),
                    source_offset: entry_offset + reference.offset as u64,
                },
            ));
        },
    );
    references
}

/// Join complete, uniquely resolved `BLOCK` construction-reference fields.
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
        field.sort_by_key(|reference| reference.ordinal);
        let Ok(field): Result<[_; 19], _> = field.try_into() else { continue; };
        if field.iter().enumerate().any(|(ordinal, reference)| {
                reference.ordinal != ordinal as u32
                    || reference.control != field[0].control
                    || reference.terminal != (ordinal == 18)
            })
        {
            continue;
        }
        let [members @ .., terminal] = &field;
        let Some(members) = members
            .iter()
            .map(|reference| Some(FeatureConstructionMember {
                reference: reference.id.clone(), data_block: reference.data_block.clone()?,
            }))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(terminal_data_block) = terminal.data_block.clone() else {
            continue;
        };
        let Ok(members) = members.try_into() else { continue; };
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
            let mut data_blocks = construction.members.iter().map(|member| member.data_block.clone()).collect::<Vec<_>>();
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
            let Some(joined) =
                JoinedPayload::from_source(payload.content.block_ids(), &blocks)
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
) -> Vec<FeatureBlockPayloadName> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some(joined) =
                JoinedPayload::from_source(payload.content.block_ids(), &blocks)
            else {
                return Vec::new();
            };
            crate::om::construction_payload_named_fields(joined.bytes())
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, field)| {
                    let source_offset = joined.source_offset(field.offset as u64)?;
                    Some(FeatureBlockPayloadName {
                        id: format!("{}-name-{ordinal}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        type_code: field.type_code.map(|code| FeaturePayloadTypeCode {
                            value: code.value,
                            raw: code.raw,
                            payload_offset: code.offset as u64,
                            source_offset: joined.source_offset(code.offset as u64),
                        }),
                        value: field.value.to_string(),
                        payload_offset: field.offset as u64,
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
    names: &[FeatureBlockPayloadName],
    scalars: &[FeaturePayloadScalar],
) -> Vec<FeatureBlockPayloadNamedRecord> {
    let mut records = Vec::new();
    for payload in payloads {
        let mut payload_names = names
            .iter()
            .filter(|name| name.construction_payload == payload.id)
            .collect::<Vec<_>>();
        payload_names.sort_by_key(|name| name.payload_offset);
        for (ordinal, name) in payload_names.iter().enumerate() {
            let end = payload_names
                .get(ordinal + 1)
                .map_or(payload.content.byte_len(), |next| next.payload_offset);
            let mut scalar_fields = scalars
                .iter()
                .filter(|scalar| {
                    scalar.payload.id() == payload.id
                        && scalar.payload_offset > name.payload_offset
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
                payload_start_offset: name.payload_offset,
                payload_end_offset: end,
            });
        }
    }
    records
}

/// Type exact two-scalar `Point<positive decimal>` `BLOCK` payload intervals.
pub fn feature_block_payload_points(
    records: &[FeatureBlockPayloadNamedRecord],
    names: &[FeatureBlockPayloadName],
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
            parse_sketch_point_name(&name.value)?;
            let [first_id, second_id] = record.scalar_fields.as_slice() else {
                return None;
            };
            let first = scalars.get(first_id.as_str())?;
            let second = scalars.get(second_id.as_str())?;
            Some(FeatureBlockPayloadPoint {
                id: format!("{}-point", record.id),
                operation_label: record.operation_label.clone(),
                named_record: record.id.clone(),
                name: name.value.clone(),
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
                            expression.value.filter(|value| value.is_finite())?,
                        )?,
                    ))
                })
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()?;
            if resolved[0].0.source_table.is_empty()
                || resolved
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
                    object_id: frame.object_id,
                    raw_object_id: frame.raw_object_id,
                    source_offset: source_offset + frame.offset as u64,
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
    indexed: &[(crate::container::entry_ref::EntryRef<'_>, crate::om::IndexedSection<'_>)],
    object_index: u32,
) -> Option<String> {
    let section_ordinal = unique_offset_data_store(indexed, &[object_index])?;
    Some(format!(
        "nx:om-data-blocks-{section_ordinal}:block#{object_index}"
    ))
}

fn unique_offset_data_store(
    indexed: &[(crate::container::entry_ref::EntryRef<'_>, crate::om::IndexedSection<'_>)],
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
                object_id: reference.object_id,
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
