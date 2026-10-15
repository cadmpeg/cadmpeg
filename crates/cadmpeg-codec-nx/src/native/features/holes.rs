// SPDX-License-Identifier: Apache-2.0
//! Holes construction records and extraction.

use cadmpeg_ir::native::bytes::NativeBytes;

use super::feature_input_blocks;
use super::format_feature_history_id;

use super::operation_record::FeatureOperationRecord;

use super::reference::ConstructionReference;
use super::unique_offset_data_block_id;
use super::visit_feature_history_operation_records;
use super::FeatureOperationLabel;
use super::FeaturePayloadString;
use crate::container::Container;

use crate::native::om::data_blocks;
use crate::om::nonempty::NonEmpty;

use crate::om::scalar::RepeatedScalar;
use crate::om::scalar::ShiftedBinary64;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::num::NonZeroU8;
mod borrowed_wires;

/// Exact text frame retained from a `SYMBOLIC_THREAD` operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureSymbolicThreadTextFrameWire")]
struct FeatureSymbolicThreadTextFrame {
    /// Globally unique text-frame identity.
    id: String,
    /// Owning typed `SYMBOLIC_THREAD` record.
    symbolic_thread: String,
    /// Zero-based order among the operation's type-`03` text frames.
    ordinal: u32,
    /// Exact UTF-8 text value.
    value: crate::payload_text::PayloadText<String>,
    /// Absolute file offset of the text-frame marker.
    source_offset: u64,
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

#[cfg(test)]
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
pub(in crate::native) struct FeatureSymbolicThread {
    /// Globally unique symbolic-thread identity.
    id: String,
    /// Owning `SYMBOLIC_THREAD` operation label.
    operation_label: String,
    /// Owning exact feature-operation record.
    operation_record: String,
    /// Ordered complete type-`03` text frames in the payload.
    text_frames: Vec<FeatureSymbolicThreadTextFrame>,
    /// Absolute file offset of the operation record's fixed header marker.
    source_offset: u64,
}

/// Typed operation template carried by a hole payload string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct FeatureSimpleHoleTemplate {
    /// Globally unique template identity.
    pub(in crate::native) id: String,
    /// Owning `SIMPLE HOLE`, `CBORE_HOLE`, or `CSUNK_HOLE` operation label.
    pub(in crate::native) operation_label: String,
    /// Source string in the native payload-string arena.
    pub(in crate::native) payload_string: String,
    /// Hole construction family token.
    pub(in crate::native) family: SimpleHoleFamily,
    /// Hole cross-section token.
    pub(in crate::native) form: SimpleHoleForm,
    /// Axial extent token.
    pub(in crate::native) extent: SimpleHoleExtent,
    /// Entry treatment token.
    pub(in crate::native) start_treatment: SimpleHoleEndTreatment,
    /// Exit treatment token.
    pub(in crate::native) end_treatment: SimpleHoleEndTreatment,
}

/// Exact threaded-hole template retained from a `SIMPLE HOLE` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct FeatureThreadedHoleTemplate {
    /// Globally unique template identity.
    id: String,
    /// Owning `SIMPLE HOLE` operation label.
    operation_label: String,
    /// Source string in the native payload-string arena.
    payload_string: String,
    /// Thread-standard family token.
    family: ThreadedHoleFamily,
    /// Axial extent token.
    extent: SimpleHoleExtent,
    /// Absolute file offset of the template string marker.
    source_offset: u64,
}

/// Exact nonempty redundantly witnessed scalar lane in a simple-hole payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "FeatureSimpleHoleRepeatedScalarLaneWire")]
pub(in crate::native) struct FeatureSimpleHoleRepeatedScalarLane {
    /// Globally unique repeated-lane identity.
    pub(in crate::native) id: String,
    /// Owning `SIMPLE HOLE` operation label.
    pub(in crate::native) operation_label: String,
    /// Ordered scalars with both source witnesses.
    pub(in crate::native) values: NonEmpty<RepeatedScalar<u64>>,
}

#[derive(Serialize, Deserialize)]
struct FeatureSimpleHoleRepeatedScalarLaneWire {
    id: String,
    operation_label: String,
    values: Vec<f64>,
    raw_values: Vec<NativeBytes<[u8; 8]>>,
    first_witness_offsets: Vec<u64>,
    second_witness_offsets: Vec<u64>,
}

#[cfg(test)]
impl From<FeatureSimpleHoleRepeatedScalarLane> for FeatureSimpleHoleRepeatedScalarLaneWire {
    fn from(lane: FeatureSimpleHoleRepeatedScalarLane) -> Self {
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            values: lane
                .values
                .iter()
                .map(|token| token.scalar.value().get())
                .collect(),
            raw_values: lane
                .values
                .iter()
                .map(|token| token.scalar.raw())
                .map(Into::into)
                .collect(),
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
        let values = wire
            .values
            .into_iter()
            .zip(wire.raw_values)
            .zip(wire.first_witness_offsets)
            .zip(wire.second_witness_offsets)
            .map(|(((value, raw), first), second)| {
                Ok(RepeatedScalar {
                    scalar: ShiftedBinary64::from_wire(value, *raw)
                        .map_err(|error| format!("values/raw_values: {error}"))?,
                    witness_offsets: [first, second],
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            values: NonEmpty::from_admitted_vec(values)
                .ok_or("values must contain a repeated scalar")?,
        })
    }
}

/// Offset-store blocks linked after both repeated scalar-lane witnesses.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureSimpleHoleRepeatedScalarLaneBlockReferencesWire")]
pub(in crate::native) struct FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
    pub(in crate::native) id: String,
    pub(in crate::native) operation_label: String,
    pub(in crate::native) first: SimpleHoleReferencePair,
    pub(in crate::native) second: SimpleHoleReferencePair,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::native) struct SimpleHoleReferencePair {
    pub(in crate::native) references: [SimpleHoleBlockReference; 2],
    pub(in crate::native) wrapped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::native) struct SimpleHoleBlockReference {
    pub(in crate::native) data_block: String,
    pub(in crate::native) source_offset: u64,
}

/// Offset-store blocks linked after both repeated scalar-lane witnesses.
#[derive(Serialize, Deserialize)]
struct FeatureSimpleHoleRepeatedScalarLaneBlockReferencesWire {
    /// Globally unique reference-lane identity.
    id: String,
    /// Owning `SIMPLE HOLE` operation label.
    operation_label: String,
    /// Ordered blocks following the first scalar pair.
    first_data_blocks: [String; 2],
    /// Ordered blocks following the repeated scalar lane.
    second_data_blocks: [String; 2],
    /// Exact optional wrapper before the first reference pair.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_first_reference_prefix"
    )]
    first_reference_prefix: Option<NativeBytes<[u8; 8]>>,
    /// Exact optional wrapper before the repeated reference pair.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_reference_prefix"
    )]
    second_reference_prefix: Option<NativeBytes<[u8; 8]>>,
    /// Absolute offsets of the first pair of tagged-index tokens.
    first_reference_offsets: [u64; 2],
    /// Absolute offsets of the repeated pair of tagged-index tokens.
    second_reference_offsets: [u64; 2],
}

#[cfg(test)]
impl From<FeatureSimpleHoleRepeatedScalarLaneBlockReferences>
    for FeatureSimpleHoleRepeatedScalarLaneBlockReferencesWire
{
    fn from(record: FeatureSimpleHoleRepeatedScalarLaneBlockReferences) -> Self {
        Self {
            id: record.id,
            operation_label: record.operation_label,
            first_reference_offsets: record
                .first
                .references
                .each_ref()
                .map(|reference| reference.source_offset),
            second_reference_offsets: record
                .second
                .references
                .each_ref()
                .map(|reference| reference.source_offset),
            first_data_blocks: record
                .first
                .references
                .map(|reference| reference.data_block),
            second_data_blocks: record
                .second
                .references
                .map(|reference| reference.data_block),
            first_reference_prefix: (record
                .first
                .wrapped
                .then_some(crate::om::simple_hole_references::FIRST_PREFIX))
            .map(Into::into),
            second_reference_prefix: (record
                .second
                .wrapped
                .then_some(crate::om::simple_hole_references::SECOND_PREFIX))
            .map(Into::into),
        }
    }
}

impl TryFrom<FeatureSimpleHoleRepeatedScalarLaneBlockReferencesWire>
    for FeatureSimpleHoleRepeatedScalarLaneBlockReferences
{
    type Error = String;

    fn try_from(
        wire: FeatureSimpleHoleRepeatedScalarLaneBlockReferencesWire,
    ) -> Result<Self, Self::Error> {
        if wire
            .first_reference_prefix
            .is_some_and(|prefix| prefix != crate::om::simple_hole_references::FIRST_PREFIX)
        {
            return Err("first_reference_prefix: invalid first-witness wrapper".into());
        }
        if wire
            .second_reference_prefix
            .is_some_and(|prefix| prefix != crate::om::simple_hole_references::SECOND_PREFIX)
        {
            return Err("second_reference_prefix: invalid second-witness wrapper".into());
        }
        let [first_a, first_b] = wire.first_data_blocks;
        let [second_a, second_b] = wire.second_data_blocks;
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            first: SimpleHoleReferencePair {
                references: [
                    SimpleHoleBlockReference {
                        data_block: first_a,
                        source_offset: wire.first_reference_offsets[0],
                    },
                    SimpleHoleBlockReference {
                        data_block: first_b,
                        source_offset: wire.first_reference_offsets[1],
                    },
                ],
                wrapped: wire.first_reference_prefix.is_some(),
            },
            second: SimpleHoleReferencePair {
                references: [
                    SimpleHoleBlockReference {
                        data_block: second_a,
                        source_offset: wire.second_reference_offsets[0],
                    },
                    SimpleHoleBlockReference {
                        data_block: second_b,
                        source_offset: wire.second_reference_offsets[1],
                    },
                ],
                wrapped: wire.second_reference_prefix.is_some(),
            },
        })
    }
}

/// Distinct simple-hole operations sharing one four-block construction identity.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureSimpleHoleConstructionGroupWire")]
pub(in crate::native) struct FeatureSimpleHoleConstructionGroup {
    /// Globally unique group identity.
    pub(in crate::native) id: String,
    /// Shared first-witness block pair.
    pub(in crate::native) first_data_blocks: [String; 2],
    /// Shared repeated-witness block pair.
    pub(in crate::native) second_data_blocks: [String; 2],
    /// Operations and their construction lanes in feature-history order.
    pub(in crate::native) members: SimpleHoleConstructionMembers,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::native) struct FeatureSimpleHoleConstructionMember {
    pub(in crate::native) operation_label: String,
    pub(in crate::native) scalar_lane: String,
    pub(in crate::native) block_reference: String,
}

/// At least two distinct operations in retained feature-history order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::native) struct SimpleHoleConstructionMembers(
    Vec<FeatureSimpleHoleConstructionMember>,
);

impl SimpleHoleConstructionMembers {
    pub(in crate::native) fn new(
        members: Vec<FeatureSimpleHoleConstructionMember>,
    ) -> Result<Self, &'static str> {
        if members.len() < 2 {
            return Err("operation_labels must contain at least two members");
        }
        if members.iter().enumerate().any(|(index, member)| {
            members[..index]
                .iter()
                .any(|other| other.operation_label == member.operation_label)
        }) {
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

#[cfg(test)]
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
            members: SimpleHoleConstructionMembers::new(
                wire.operation_labels
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
                    .collect(),
            )
            .map_err(str::to_owned)?,
        })
    }
}

/// Exact four-block construction-group lane carried by a `HOLE PACKAGE` operation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureHolePackageConstructionGroupLaneWire")]
pub(in crate::native) struct FeatureHolePackageConstructionGroupLane {
    /// Globally unique lane identity.
    pub(in crate::native) id: String,
    /// Owning `HOLE PACKAGE` operation label.
    pub(in crate::native) operation_label: String,
    /// Compact selector preceding the repeated branch byte.
    selector: NonZeroU8,
    /// Branch byte repeated between the two reference pairs.
    branch: NonZeroU8,
    /// Four checked references with their resolved targets and source offsets.
    references: [ConstructionReference<String>; 4],
    /// Payload-relative offset of the lane prefix.
    payload_offset: u64,
    /// Absolute file offset of the lane prefix.
    source_offset: u64,
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
    raw_object_indices: [NativeBytes<Vec<u8>>; 4],
    /// Uniquely resolved offset-store blocks.
    data_blocks: [String; 4],
    /// Payload-relative offset of the lane prefix.
    payload_offset: u64,
    /// Absolute file offset of the lane prefix.
    source_offset: u64,
    /// Absolute file offsets of the four reference tokens.
    reference_source_offsets: [u64; 4],
}

#[cfg(test)]
impl From<FeatureHolePackageConstructionGroupLane> for FeatureHolePackageConstructionGroupLaneWire {
    fn from(value: FeatureHolePackageConstructionGroupLane) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            selector: value.selector.get(),
            branch: value.branch.get(),
            object_indices: value
                .references
                .each_ref()
                .map(|reference| reference.token.value()),
            raw_object_indices: (value
                .references
                .each_ref()
                .map(|reference| reference.token.raw().to_vec()))
            .map(Into::into),
            data_blocks: value
                .references
                .each_ref()
                .map(|reference| reference.data_block.clone()),
            payload_offset: value.payload_offset,
            source_offset: value.source_offset,
            reference_source_offsets: value
                .references
                .each_ref()
                .map(|reference| reference.source_offset),
        }
    }
}

impl TryFrom<FeatureHolePackageConstructionGroupLaneWire>
    for FeatureHolePackageConstructionGroupLane
{
    type Error = String;

    fn try_from(wire: FeatureHolePackageConstructionGroupLaneWire) -> Result<Self, Self::Error> {
        let [a, b, c, d] = [0, 1, 2, 3].map(|slot| {
            crate::om::reference_index::ReferenceIndexToken::from_wire(
                wire.object_indices[slot],
                &wire.raw_object_indices[slot],
            )
            .map_err(|error| format!("object_indices/raw_object_indices[{slot}]: {error}"))
            .map(|token| ConstructionReference {
                token,
                data_block: wire.data_blocks[slot].clone(),
                source_offset: wire.reference_source_offsets[slot],
            })
        });
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            selector: NonZeroU8::new(wire.selector)
                .ok_or("selector: zero is not a construction selector")?,
            branch: NonZeroU8::new(wire.branch)
                .ok_or("branch: zero is not a construction branch")?,
            references: [a?, b?, c?, d?],
            payload_offset: wire.payload_offset,
            source_offset: wire.source_offset,
        })
    }
}

/// Exact relation between one hole package and one simple-hole construction group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct FeatureHolePackageConstructionGroupUse {
    /// Globally unique relation identity.
    pub(in crate::native) id: String,
    /// Owning `HOLE PACKAGE` operation label.
    pub(in crate::native) operation_label: String,
    /// Exact package lane carrying the group identity.
    pub(in crate::native) construction_group_lane: String,
    /// Uniquely matched simple-hole construction group.
    pub(in crate::native) simple_hole_construction_group: String,
    /// Absolute file offset of the package lane.
    pub(in crate::native) source_offset: u64,
}

/// Construction family named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::native) enum SimpleHoleFamily {
    /// General-hole construction family.
    GeneralHole,
}

/// Thread-standard family named by a threaded-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ThreadedHoleFamily {
    /// Metric profile family named by the `M Profile` token.
    MProfile,
    /// Unified National Coarse family named by the `UNC` token.
    Unc,
}

/// Cross-section named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::native) enum SimpleHoleForm {
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
pub(in crate::native) enum SimpleHoleExtent {
    /// Continue through all intersected material.
    Through,
    /// Stop at a blind termination whose distance is carried by another field.
    Blind,
}

/// End treatment named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::native) enum SimpleHoleEndTreatment {
    /// No separate end treatment is named.
    None,
    /// Chamfer the circular end edge.
    Chamfer,
}

fn symbolic_thread_text_frames(
    record: crate::om::operation_record::OperationPayload<'_>,
) -> Option<Vec<crate::om::OperationPayloadTextFrame<'_>>> {
    if record.name() != "SYMBOLIC_THREAD" {
        return None;
    }
    let mut frames = crate::om::operation_payload_text_frames(record);
    frames.retain(|frame| frame.marker == crate::om::OperationTextMarker::Text);
    (frames.len() >= 2).then_some(frames)
}

fn owned_symbolic_thread(
    frames: Vec<crate::om::OperationPayloadTextFrame<'_>>,
    section_key: &str,
    entry_offset: u64,
    operation_ordinal: usize,
    record_offset: usize,
) -> Result<FeatureSymbolicThread, cadmpeg_core::CodecError> {
    let operation_label =
        format_feature_history_id("operation-label", section_key, operation_ordinal, None);
    let operation_record =
        format_feature_history_id("operation-record", section_key, operation_ordinal, None);
    let id = format_feature_history_id("symbolic-thread", section_key, operation_ordinal, None);

    let mut text_frames = Vec::new();
    for (ordinal, frame) in frames.into_iter().enumerate() {
        let ordinal_u32 = u32::try_from(ordinal).map_err(|_| {
            cadmpeg_core::CodecError::malformed("NX symbolic thread text frame ordinal exceeds u32")
        })?;
        let frame_id = format_feature_history_id(
            "symbolic-thread-text-frame",
            section_key,
            operation_ordinal,
            Some(ordinal),
        );
        let owner = id.clone();
        let value = frame.value.as_str().to_owned();
        text_frames.push(FeatureSymbolicThreadTextFrame {
            id: frame_id,
            symbolic_thread: owner,
            ordinal: ordinal_u32,
            value: crate::payload_text::PayloadText::new(value)
                .map_err(|error| cadmpeg_core::CodecError::Malformed(error.to_owned()))?,
            source_offset: entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(frame.offset))
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "NX symbolic thread text frame offset overflows",
                    )
                })?,
        });
    }
    Ok(FeatureSymbolicThread {
        id,
        operation_label,
        operation_record,
        text_frames,
        source_offset: entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(record_offset))
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("NX symbolic thread offset overflows")
            })?,
    })
}

/// Decode complete typed text frames from symbolic-thread operations.
pub(in crate::native) fn feature_symbolic_threads(
    container: &Container,
) -> Result<Vec<FeatureSymbolicThread>, cadmpeg_core::CodecError> {
    let mut threads = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let Some(frames) = symbolic_thread_text_frames(record.payload_view()) else {
                return;
            };
            let thread = match owned_symbolic_thread(
                frames,
                section_key,
                entry_offset,
                operation_ordinal,
                record.offset(),
            ) {
                Ok(thread) => thread,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            threads.push(thread);
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(threads)
}

fn hole_template_candidates<T>(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    strings: &[FeaturePayloadString],
    eligible: impl Fn(&FeatureOperationLabel) -> bool,
    mut build: impl FnMut(&FeaturePayloadString, &FeatureOperationLabel) -> Option<T>,
) -> Vec<T> {
    let labels_by_id = labels
        .iter()
        .map(|label| (label.id.as_str(), label))
        .collect::<BTreeMap<_, _>>();
    let records_by_id = records
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let mut candidates =
        BTreeMap::<&str, (usize, &FeaturePayloadString, &FeatureOperationLabel)>::new();
    for string in strings {
        let Some(record) = records_by_id.get(string.operation_record.as_str()) else {
            continue;
        };
        let Some(label) = labels_by_id.get(record.operation_label.as_str()) else {
            continue;
        };
        if !eligible(label) || !string.value.as_str().starts_with("Hole_") {
            continue;
        }
        candidates
            .entry(label.id.as_str())
            .or_insert((0, string, label))
            .0 += 1;
    }
    candidates
        .into_values()
        .filter(|(count, _, _)| *count == 1)
        .filter_map(|(_, string, label)| build(string, label))
        .collect()
}

/// Join exact hole payload templates to their operation identities.
pub(in crate::native) fn feature_simple_hole_templates(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    strings: &[FeaturePayloadString],
) -> Vec<FeatureSimpleHoleTemplate> {
    hole_template_candidates(
        labels,
        records,
        strings,
        |label| {
            matches!(
                label.value.as_str(),
                "SIMPLE HOLE" | "CBORE_HOLE" | "CSUNK_HOLE"
            )
        },
        |string, label| {
            let (form, extent, start_treatment, end_treatment) =
                parse_simple_hole_template(string.value.as_str())?;
            Some(FeatureSimpleHoleTemplate {
                id: string
                    .id
                    .replacen("payload-string", "simple-hole-template", 1),
                operation_label: label.id.clone(),
                payload_string: string.id.clone(),
                family: SimpleHoleFamily::GeneralHole,
                form,
                extent,
                start_treatment,
                end_treatment,
            })
        },
    )
}

/// Join exact threaded-hole payload templates to their operation identities.
pub(in crate::native) fn feature_threaded_hole_templates(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    strings: &[FeaturePayloadString],
) -> Vec<FeatureThreadedHoleTemplate> {
    hole_template_candidates(
        labels,
        records,
        strings,
        |label| label.value == "SIMPLE HOLE",
        |string, label| {
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
        },
    )
}

/// Decode exact nonempty duplicated scalar lanes from simple-hole operations.
pub(in crate::native) fn feature_simple_hole_repeated_scalar_lanes(
    container: &Container,
) -> Result<Vec<FeatureSimpleHoleRepeatedScalarLane>, cadmpeg_core::CodecError> {
    let mut pairs = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(pair) = crate::om::simple_hole_repeated_scalar_lane(record.payload_view())
            else {
                return;
            };
            let values = pair.map(|token| RepeatedScalar {
                scalar: token.scalar,
                witness_offsets: token
                    .witness_offsets
                    .map(|offset| entry_offset + cadmpeg_core::decode::u64_from_index(offset)),
            });
            let id = format_feature_history_id(
                "simple-hole-repeated-scalar-lane",
                section_key,
                operation_ordinal,
                None,
            );
            let operation_label =
                format_feature_history_id("operation-label", section_key, operation_ordinal, None);
            pairs.push(FeatureSimpleHoleRepeatedScalarLane {
                id,
                operation_label,
                values,
            });
        },
    )?;
    Ok(pairs)
}

/// Resolve the tagged block-index pairs following both repeated scalar-lane
/// witnesses through the unique offset store that owns the operation inputs.
fn simple_hole_block_reference(
    blocks: &[crate::native::om::DataBlock],
    prefix: &str,
    entry_offset: u64,
    (token, offset): (crate::om::reference_index::PayloadIndexToken, usize),
) -> Option<SimpleHoleBlockReference> {
    let block = blocks.iter().find(|block| {
        block.block_ordinal == token.value()
            && block
                .id
                .rsplit_once(":block#")
                .is_some_and(|(owner, _)| owner == prefix)
    })?;
    let source_offset = entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(offset))?;
    Some(SimpleHoleBlockReference {
        data_block: block.id.clone(),
        source_offset,
    })
}

fn simple_hole_reference_pair(
    blocks: &[crate::native::om::DataBlock],
    prefix: &str,
    entry_offset: u64,
    pair: crate::om::simple_hole_references::ReferencePair,
) -> Option<SimpleHoleReferencePair> {
    let [first, second] = pair.references();
    Some(SimpleHoleReferencePair {
        references: [
            simple_hole_block_reference(blocks, prefix, entry_offset, first)?,
            simple_hole_block_reference(blocks, prefix, entry_offset, second)?,
        ],
        wrapped: pair.wrapped(),
    })
}

pub(in crate::native) fn feature_simple_hole_repeated_scalar_lane_block_references(
    container: &Container,
) -> Result<Vec<FeatureSimpleHoleRepeatedScalarLaneBlockReferences>, cadmpeg_core::CodecError> {
    let inputs = feature_input_blocks(container)?;
    let blocks = data_blocks(container)?;
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let operation_label =
                format_feature_history_id("operation-label", section_key, operation_ordinal, None);
            let mut prefix = None;
            for input in &inputs {
                if input.operation_label != operation_label {
                    continue;
                }
                let Some(candidate) = input
                    .data_block
                    .rsplit_once(":block#")
                    .map(|(owner, _)| owner)
                else {
                    continue;
                };
                if prefix.is_some_and(|first| first != candidate) {
                    return;
                }
                prefix = Some(candidate);
            }
            let Some(prefix) = prefix else {
                return;
            };
            let Some(decoded) = crate::om::simple_hole_references::simple_hole_repeated_scalar_lane_block_references(record.payload_view()) else {
                return;
            };
            let Some(first) = simple_hole_reference_pair(&blocks, prefix, entry_offset, decoded[0])
            else {
                return;
            };
            let Some(second) =
                simple_hole_reference_pair(&blocks, prefix, entry_offset, decoded[1])
            else {
                return;
            };
            let id = format_feature_history_id(
                "simple-hole-repeated-scalar-lane-block-references",
                section_key,
                operation_ordinal,
                None,
            );
            references.push(FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
                id,
                operation_label,
                first,
                second,
            });
        },
    )?;
    Ok(references)
}

/// Group distinct simple-hole operations that address the same four construction blocks.
pub(in crate::native) fn feature_simple_hole_construction_groups(
    labels: &[FeatureOperationLabel],
    lanes: &[FeatureSimpleHoleRepeatedScalarLane],
    references: &[FeatureSimpleHoleRepeatedScalarLaneBlockReferences],
) -> Result<Vec<FeatureSimpleHoleConstructionGroup>, cadmpeg_core::CodecError> {
    let mut chronology = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        let first_offset = labels
            .iter()
            .filter(|other| other.section_link == label.section_link)
            .map(|other| other.source_offset)
            .fold(label.source_offset, u64::min);
        chronology.push((index, label, first_offset));
    }
    chronology.sort_unstable_by(
        |(left_index, left, left_first), (right_index, right, right_first)| {
            left_first
                .cmp(right_first)
                .then_with(|| left.section_link.cmp(&right.section_link))
                .then_with(|| right.source_offset.cmp(&left.source_offset))
                .then_with(|| left_index.cmp(right_index))
        },
    );

    let mut grouped = BTreeMap::<
        ([&str; 2], [&str; 2]),
        Vec<(
            &FeatureSimpleHoleRepeatedScalarLaneBlockReferences,
            &FeatureSimpleHoleRepeatedScalarLane,
        )>,
    >::new();
    let mut ambiguous_groups = BTreeSet::<([&str; 2], [&str; 2])>::new();
    for reference in references {
        let key = (
            reference
                .first
                .references
                .each_ref()
                .map(|reference| reference.data_block.as_str()),
            reference
                .second
                .references
                .each_ref()
                .map(|reference| reference.data_block.as_str()),
        );
        let mut matching_lanes = lanes
            .iter()
            .filter(|lane| lane.operation_label == reference.operation_label);
        let lane = match (matching_lanes.next(), matching_lanes.next()) {
            (Some(lane), None) => lane,
            (Some(_), Some(_)) => {
                ambiguous_groups.insert(key);
                continue;
            }
            _ => continue,
        };
        grouped.entry(key).or_default().push((reference, lane));
    }
    let mut groups = Vec::new();
    for (key, candidates) in grouped {
        if ambiguous_groups.contains(&key) {
            continue;
        }
        let mut positioned = Vec::new();
        let mut missing = false;
        for (index, (reference, lane)) in candidates.into_iter().enumerate() {
            let Some(position) = chronology
                .iter()
                .rposition(|(_, label, _)| label.id == reference.operation_label)
            else {
                missing = true;
                break;
            };
            positioned.push((position, index, reference, lane));
        }
        if missing {
            continue;
        }
        positioned.sort_unstable_by(
            |(left_pos, left_index, left, _), (right_pos, right_index, right, _)| {
                left_pos
                    .cmp(right_pos)
                    .then_with(|| left.operation_label.cmp(&right.operation_label))
                    .then_with(|| left_index.cmp(right_index))
            },
        );
        if positioned.len() < 2
            || positioned
                .iter()
                .enumerate()
                .any(|(index, (_, _, reference, _))| {
                    positioned[..index]
                        .iter()
                        .any(|(_, _, other, _)| other.operation_label == reference.operation_label)
                })
        {
            continue;
        }
        let mut members = Vec::new();
        for (_, _, reference, lane) in positioned {
            let operation_label = reference.operation_label.clone();
            let scalar_lane = lane.id.clone();
            let block_reference = reference.id.clone();

            members.push(FeatureSimpleHoleConstructionMember {
                operation_label,
                scalar_lane,
                block_reference,
            });
        }
        let id_anchor = members.iter().fold(&members[0], |first, second| {
            if first.operation_label <= second.operation_label {
                first
            } else {
                second
            }
        });
        let id_key = id_anchor
            .operation_label
            .rsplit_once('#')
            .map_or("unknown", |(_, key)| key);
        let id = format!("nx:feature-history:simple-hole-construction-group#{id_key}");
        let members = SimpleHoleConstructionMembers::new(members)
            .map_err(|error| cadmpeg_core::CodecError::Malformed(error.to_owned()))?;
        let first_data_blocks = [key.0[0].to_owned(), key.0[1].to_owned()];
        let second_data_blocks = [key.1[0].to_owned(), key.1[1].to_owned()];

        groups.push(FeatureSimpleHoleConstructionGroup {
            id,
            first_data_blocks,
            second_data_blocks,
            members,
        });
    }
    Ok(groups)
}

/// Decode and resolve exact four-block lanes from `HOLE PACKAGE` operations.
fn package_lane_references(
    indexed: &[(
        crate::container::entry_ref::EntryRef<'_>,
        crate::om::IndexedSection<'_>,
    )],
    entry_offset: u64,
    lane: &crate::om::HolePackageConstructionGroupLane,
) -> Option<[ConstructionReference<String>; 4]> {
    let mut resolved = [None, None, None, None];
    for (slot, reference) in lane.references.iter().enumerate() {
        resolved[slot] = Some(ConstructionReference {
            token: reference.token,
            data_block: unique_offset_data_block_id(indexed, reference.token.value())?,
            source_offset: entry_offset
                .checked_add(cadmpeg_core::decode::u64_from_index(reference.offset))?,
        });
    }
    let [Some(a), Some(b), Some(c), Some(d)] = resolved else {
        return None;
    };
    Some([a, b, c, d])
}

pub(in crate::native) fn feature_hole_package_construction_group_lanes(
    container: &Container,
) -> Result<Vec<FeatureHolePackageConstructionGroupLane>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections()?;
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(lane) = crate::om::hole_package_construction_group_lane(record.payload_view())
            else {
                return;
            };
            let Some(references) = package_lane_references(&indexed, entry_offset, &lane) else {
                return;
            };
            let operation_label =
                format_feature_history_id("operation-label", section_key, operation_ordinal, None);
            let id = format_feature_history_id(
                "hole-package-construction-group-lane",
                section_key,
                operation_ordinal,
                None,
            );
            lanes.push(FeatureHolePackageConstructionGroupLane {
                id,
                operation_label,
                selector: lane.selector,
                branch: lane.branch,
                references,
                payload_offset: cadmpeg_core::decode::u64_from_index(lane.offset),
                source_offset: entry_offset
                    + cadmpeg_core::decode::u64_from_index(record.payload_offset())
                    + cadmpeg_core::decode::u64_from_index(lane.offset),
            });
        },
    )?;
    Ok(lanes)
}

/// Join one package lane to one simple-hole group only by exact four-block identity.
fn simple_hole_group_key(group: &FeatureSimpleHoleConstructionGroup) -> [&str; 4] {
    [
        &group.first_data_blocks[0],
        &group.first_data_blocks[1],
        &group.second_data_blocks[0],
        &group.second_data_blocks[1],
    ]
}

fn hole_package_lane_key(lane: &FeatureHolePackageConstructionGroupLane) -> [&str; 4] {
    lane.references
        .each_ref()
        .map(|reference| reference.data_block.as_str())
}

pub(in crate::native) fn feature_hole_package_construction_group_uses(
    lanes: &[FeatureHolePackageConstructionGroupLane],
    groups: &[FeatureSimpleHoleConstructionGroup],
) -> Vec<FeatureHolePackageConstructionGroupUse> {
    let mut matches = Vec::new();
    for group in groups {
        let key = simple_hole_group_key(group);
        if groups
            .iter()
            .filter(|other| simple_hole_group_key(other) == key)
            .take(2)
            .count()
            != 1
        {
            continue;
        }
        let mut matching_lanes = lanes
            .iter()
            .filter(|lane| hole_package_lane_key(lane) == key);
        let (Some(lane), None) = (matching_lanes.next(), matching_lanes.next()) else {
            continue;
        };
        matches.push((group, lane));
    }
    matches.sort_unstable_by(|(left, _), (right, _)| {
        simple_hole_group_key(left).cmp(&simple_hole_group_key(right))
    });
    let mut uses = Vec::new();
    for (group, lane) in matches {
        let id = lane.id.replacen(
            "hole-package-construction-group-lane",
            "hole-package-construction-group-use",
            1,
        );
        let operation_label = lane.operation_label.clone();
        let construction_group_lane = lane.id.clone();
        let simple_hole_construction_group = group.id.clone();

        uses.push(FeatureHolePackageConstructionGroupUse {
            id,
            operation_label,
            construction_group_lane,
            simple_hole_construction_group,
            source_offset: lane.source_offset,
        });
    }
    uses
}

pub(in crate::native) fn parse_simple_hole_template(
    value: &str,
) -> Option<(
    SimpleHoleForm,
    SimpleHoleExtent,
    SimpleHoleEndTreatment,
    SimpleHoleEndTreatment,
)> {
    let mut tokens = value.split('_');
    if tokens.next() != Some("Hole") || tokens.next() != Some("GeneralHole") {
        return None;
    }
    let form = match tokens.next()? {
        "Simple" => SimpleHoleForm::Simple,
        "Counterbored" => SimpleHoleForm::Counterbored,
        "Countersunk" => SimpleHoleForm::Countersunk,
        _ => return None,
    };
    let extent = match tokens.next()? {
        "Through" => SimpleHoleExtent::Through,
        "Blind" => SimpleHoleExtent::Blind,
        _ => return None,
    };
    let (start_treatment, end_treatment) =
        match (form, extent, tokens.next(), tokens.next(), tokens.next()) {
            (
                SimpleHoleForm::Simple,
                SimpleHoleExtent::Through,
                Some("StartChamfer"),
                Some("EndChamfer"),
                None,
            ) => (
                SimpleHoleEndTreatment::Chamfer,
                SimpleHoleEndTreatment::Chamfer,
            ),
            (
                SimpleHoleForm::Counterbored | SimpleHoleForm::Countersunk,
                SimpleHoleExtent::Through,
                None,
                None,
                None,
            )
            | (
                SimpleHoleForm::Simple | SimpleHoleForm::Countersunk,
                SimpleHoleExtent::Blind,
                None,
                None,
                None,
            ) => (SimpleHoleEndTreatment::None, SimpleHoleEndTreatment::None),
            _ => return None,
        };
    Some((form, extent, start_treatment, end_treatment))
}

fn parse_threaded_hole_template(value: &str) -> Option<(ThreadedHoleFamily, SimpleHoleExtent)> {
    match value {
        "Hole_ThreadedHole_M Profile_Blind" => {
            Some((ThreadedHoleFamily::MProfile, SimpleHoleExtent::Blind))
        }
        "Hole_ThreadedHole_UNC_Blind" => Some((ThreadedHoleFamily::Unc, SimpleHoleExtent::Blind)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_first_reference_prefix,
    cadmpeg_ir::native::bytes::NativeBytes<[u8; 8]>,
    "first_reference_prefix"
);
cadmpeg_core::named_optional_field!(
    deserialize_second_reference_prefix,
    cadmpeg_ir::native::bytes::NativeBytes<[u8; 8]>,
    "second_reference_prefix"
);
