// SPDX-License-Identifier: Apache-2.0
//! Holes construction records and extraction.

use super::construction_records::{replace_operation_text, OperationInputStores};
use super::format_feature_history_id;
use super::payload_content::{block_store, operation_key};
use super::FeatureInputBlock;

use super::operation_record::FeatureOperationRecord;

use super::charged_unique_offset_data_block;
use super::reference::ConstructionReference;
use super::FeatureHistory;
use super::FeatureOperationLabel;
use super::FeaturePayloadString;

use crate::native::om::data_blocks;
use crate::om::nonempty::NonEmpty;

use crate::om::scalar::RepeatedScalar;
use crate::om::scalar::ShiftedBinary64;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
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
    raw_values: Vec<[u8; 8]>,
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
        let values = wire
            .values
            .into_iter()
            .zip(wire.raw_values)
            .zip(wire.first_witness_offsets)
            .zip(wire.second_witness_offsets)
            .map(|(((value, raw), first), second)| {
                Ok(RepeatedScalar {
                    scalar: ShiftedBinary64::from_wire(value, raw)
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
    first_reference_prefix: Option<[u8; 8]>,
    /// Exact optional wrapper before the repeated reference pair.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_reference_prefix"
    )]
    second_reference_prefix: Option<[u8; 8]>,
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
            first_reference_prefix: record
                .first
                .wrapped
                .then_some(crate::om::simple_hole_references::FIRST_PREFIX),
            second_reference_prefix: record
                .second
                .wrapped
                .then_some(crate::om::simple_hole_references::SECOND_PREFIX),
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

    /// Wrap members whose count and label distinctness the decoder has proved.
    fn from_distinct(members: Vec<FeatureSimpleHoleConstructionMember>) -> Self {
        Self(members)
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
        let members = SimpleHoleConstructionMembers::new(
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
        .map_err(str::to_owned)?;
        Ok(Self {
            id: wire.id,
            first_data_blocks: wire.first_data_blocks,
            second_data_blocks: wire.second_data_blocks,
            members,
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
            raw_object_indices: value
                .references
                .each_ref()
                .map(|reference| reference.token.raw().to_vec()),
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

fn symbolic_thread_text_frames<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: crate::om::operation_record::OperationPayload<'a>,
) -> Result<Option<Vec<crate::om::OperationPayloadTextFrame<'a>>>, cadmpeg_core::CodecError> {
    if record.name() != "SYMBOLIC_THREAD" {
        return Ok(None);
    }
    let frames = crate::om::operation_payload_text_frames(ctx, record)?;
    let text_frames = ctx
        .admit_iter(&frames, "count NX symbolic-thread text frames")?
        .filter(|frame| frame.marker == crate::om::OperationTextMarker::Text)
        .count();
    Ok((text_frames >= 2).then_some(frames))
}

fn owned_symbolic_thread(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    frames: Vec<crate::om::OperationPayloadTextFrame<'_>>,
    section_key: &str,
    entry_offset: u64,
    operation_ordinal: usize,
    record_offset: usize,
) -> Result<FeatureSymbolicThread, cadmpeg_core::CodecError> {
    let operation_label =
        format_feature_history_id(ctx, "operation-label", section_key, operation_ordinal, None)?;
    let operation_record = format_feature_history_id(
        ctx,
        "operation-record",
        section_key,
        operation_ordinal,
        None,
    )?;
    let id =
        format_feature_history_id(ctx, "symbolic-thread", section_key, operation_ordinal, None)?;

    let mut text_frames = Vec::new();
    let mut frames = frames.into_iter();
    let mut ordinal = 0usize;
    for _ in ctx.admit_iter(&(0..frames.len()), "build NX symbolic-thread text frames")? {
        let Some(frame) = frames.next() else {
            return Err(ctx.refuse_codec_limit("build NX symbolic-thread text frames", 0, 1));
        };
        if frame.marker != crate::om::OperationTextMarker::Text {
            continue;
        }
        let ordinal_u32 = u32::try_from(ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX symbolic thread text frame ordinal", 0, 1))?;
        let frame_id = format_feature_history_id(
            ctx,
            "symbolic-thread-text-frame",
            section_key,
            operation_ordinal,
            Some(ordinal),
        )?;
        let owner = ctx.copy_retained_text(&id, "NX symbolic thread text frame owner")?;
        let value =
            ctx.copy_retained_text(frame.value.as_str(), "NX symbolic thread text frame value")?;
        let value = crate::payload_text::PayloadText::new(value)
            .map_err(cadmpeg_core::CodecError::malformed)?;
        let source_offset = entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(frame.offset))
            .ok_or_else(|| ctx.refuse_codec_limit("NX symbolic thread text frame offset", 0, 1))?;
        ctx.reserve_vec(&mut text_frames, 1, "NX symbolic thread text frames")?;
        text_frames.push(FeatureSymbolicThreadTextFrame {
            id: frame_id,
            symbolic_thread: owner,
            ordinal: ordinal_u32,
            value,
            source_offset,
        });
        ordinal += 1;
    }
    Ok(FeatureSymbolicThread {
        id,
        operation_label,
        operation_record,
        text_frames,
        source_offset: entry_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(record_offset))
            .ok_or_else(|| ctx.refuse_codec_limit("NX symbolic thread offset", 0, 1))?,
    })
}

/// Decode complete typed text frames from symbolic-thread operations.
pub(in crate::native) fn feature_symbolic_threads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureSymbolicThread>, cadmpeg_core::CodecError> {
    let mut threads = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let Some(frames) = symbolic_thread_text_frames(ctx, record.payload_view())? else {
                continue;
            };
            let thread = owned_symbolic_thread(
                ctx,
                frames,
                section_key,
                entry_offset,
                operation_ordinal,
                record.offset(),
            )?;
            ctx.reserve_vec(&mut threads, 1, "NX symbolic threads")?;
            threads.push(thread);
        }
    }
    Ok(threads)
}

fn hole_template_candidates<T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    strings: &[FeaturePayloadString],
    eligible: impl Fn(&FeatureOperationLabel) -> bool,
    mut build: impl FnMut(
        &cadmpeg_core::decode::DecodeContext<'_>,
        &FeaturePayloadString,
        &FeatureOperationLabel,
    ) -> Result<Option<T>, cadmpeg_core::CodecError>,
) -> Result<Vec<T>, cadmpeg_core::CodecError> {
    let mut reservation = ctx.reserve_scoped(0, "NX hole template indexes")?;
    let mut labels_by_id = BTreeMap::<&str, &FeatureOperationLabel>::new();
    for label in labels {
        ctx.charge_work(1, "index NX hole template labels")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<(&str, &FeatureOperationLabel)>() * 4,
        ))?;
        ctx.insert_btree_map(
            &mut labels_by_id,
            label.id.as_str(),
            label,
            "NX hole template label index",
        )?;
    }
    let mut records_by_id = BTreeMap::<&str, &FeatureOperationRecord>::new();
    for record in records {
        ctx.charge_work(1, "index NX hole template records")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<(&str, &FeatureOperationRecord)>() * 4,
        ))?;
        ctx.insert_btree_map(
            &mut records_by_id,
            record.id.as_str(),
            record,
            "NX hole template record index",
        )?;
    }
    let mut candidates =
        BTreeMap::<&str, (usize, &FeaturePayloadString, &FeatureOperationLabel)>::new();
    for string in strings {
        ctx.charge_work(1, "join NX hole template strings")?;
        let Some(record) = ctx
            .get_btree_map(
                &records_by_id,
                string.operation_record.as_str(),
                "find NX hole template record",
            )?
            .copied()
        else {
            continue;
        };
        let Some(label) = ctx
            .get_btree_map(
                &labels_by_id,
                record.operation_label.as_str(),
                "find NX hole template label",
            )?
            .copied()
        else {
            continue;
        };
        if !eligible(label) || !string.value.as_str().starts_with("Hole_") {
            continue;
        }
        if let Some((count, _, _)) = ctx.get_mut_btree_map(
            &mut candidates,
            label.id.as_str(),
            "find NX hole template candidate",
        )? {
            *count = count
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit("count NX hole template candidates", 0, 1))?;
        } else {
            reservation.grow(cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<(&str, (usize, &FeaturePayloadString, &FeatureOperationLabel))>(
                ) * 4,
            ))?;
            ctx.insert_btree_map(
                &mut candidates,
                label.id.as_str(),
                (1, string, label),
                "NX hole template candidate index",
            )?;
        }
    }
    let mut output = Vec::new();
    for (_, (count, string, label)) in candidates {
        ctx.charge_work(1, "build NX hole templates")?;
        if count != 1 {
            continue;
        }
        let Some(item) = build(ctx, string, label)? else {
            continue;
        };
        ctx.reserve_vec(&mut output, 1, "NX hole templates")?;
        output.push(item);
    }
    Ok(output)
}

/// Join exact hole payload templates to their operation identities.
pub(in crate::native) fn feature_simple_hole_templates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    strings: &[FeaturePayloadString],
) -> Result<Vec<FeatureSimpleHoleTemplate>, cadmpeg_core::CodecError> {
    hole_template_candidates(
        ctx,
        labels,
        records,
        strings,
        |label| {
            matches!(
                label.value.as_str(),
                "SIMPLE HOLE" | "CBORE_HOLE" | "CSUNK_HOLE"
            )
        },
        |ctx, string, label| {
            let Some((form, extent, start_treatment, end_treatment)) =
                parse_simple_hole_template(string.value.as_str())
            else {
                return Ok(None);
            };
            Ok(Some(FeatureSimpleHoleTemplate {
                id: replace_operation_text(
                    ctx,
                    &string.id,
                    "payload-string",
                    "simple-hole-template",
                    "NX simple hole template identity",
                )?,
                operation_label: ctx
                    .copy_retained_text(&label.id, "NX simple hole template label")?,
                payload_string: ctx
                    .copy_retained_text(&string.id, "NX simple hole template source")?,
                family: SimpleHoleFamily::GeneralHole,
                form,
                extent,
                start_treatment,
                end_treatment,
            }))
        },
    )
}

/// Join exact threaded-hole payload templates to their operation identities.
pub(in crate::native) fn feature_threaded_hole_templates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    strings: &[FeaturePayloadString],
) -> Result<Vec<FeatureThreadedHoleTemplate>, cadmpeg_core::CodecError> {
    hole_template_candidates(
        ctx,
        labels,
        records,
        strings,
        |label| label.value == "SIMPLE HOLE",
        |ctx, string, label| {
            let Some((family, extent)) = parse_threaded_hole_template(string.value.as_str()) else {
                return Ok(None);
            };
            Ok(Some(FeatureThreadedHoleTemplate {
                id: replace_operation_text(
                    ctx,
                    &string.id,
                    "payload-string",
                    "threaded-hole-template",
                    "NX threaded hole template identity",
                )?,
                operation_label: ctx
                    .copy_retained_text(&label.id, "NX threaded hole template label")?,
                payload_string: ctx
                    .copy_retained_text(&string.id, "NX threaded hole template source")?,
                family,
                extent,
                source_offset: string.source_offset,
            }))
        },
    )
}

/// Decode exact nonempty duplicated scalar lanes from simple-hole operations.
pub(in crate::native) fn feature_simple_hole_repeated_scalar_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureSimpleHoleRepeatedScalarLane>, cadmpeg_core::CodecError> {
    let mut pairs = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let Some(pair) =
                crate::om::simple_hole_repeated_scalar_lane(ctx, record.payload_view())?
            else {
                continue;
            };
            let values = pair.map_charged(ctx, |token| RepeatedScalar {
                scalar: token.scalar,
                witness_offsets: token
                    .witness_offsets
                    .map(|offset| entry_offset + cadmpeg_core::decode::u64_from_index(offset)),
            })?;
            let id = format_feature_history_id(
                ctx,
                "simple-hole-repeated-scalar-lane",
                section_key,
                operation_ordinal,
                None,
            )?;
            let operation_label = format_feature_history_id(
                ctx,
                "operation-label",
                section_key,
                operation_ordinal,
                None,
            )?;
            ctx.reserve_vec(&mut pairs, 1, "NX simple hole repeated scalar lanes")?;
            pairs.push(FeatureSimpleHoleRepeatedScalarLane {
                id,
                operation_label,
                values,
            });
        }
    }
    Ok(pairs)
}

/// Resolve the tagged block-index pairs following both repeated scalar-lane
/// witnesses through the unique offset store that owns the operation inputs.
/// First data block of each (owner store, block ordinal).
type DataBlockIndex<'b> = BTreeMap<(&'b str, u32), &'b crate::native::om::DataBlock>;

fn simple_hole_block_reference(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    blocks: &DataBlockIndex<'_>,
    prefix: &str,
    entry_offset: u64,
    (token, offset): (crate::om::reference_index::PayloadIndexToken, usize),
) -> Result<Option<SimpleHoleBlockReference>, cadmpeg_core::CodecError> {
    let Some(block) = ctx
        .get_btree_map(
            blocks,
            &(prefix, token.value()),
            "resolve NX simple hole block reference",
        )?
        .copied()
    else {
        return Ok(None);
    };
    let Some(source_offset) =
        entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(offset))
    else {
        return Ok(None);
    };
    Ok(Some(SimpleHoleBlockReference {
        data_block: ctx.copy_retained_text(&block.id, "NX simple hole block reference")?,
        source_offset,
    }))
}

fn simple_hole_reference_pair(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    blocks: &DataBlockIndex<'_>,
    prefix: &str,
    entry_offset: u64,
    pair: crate::om::simple_hole_references::ReferencePair,
) -> Result<Option<SimpleHoleReferencePair>, cadmpeg_core::CodecError> {
    let [first, second] = pair.references();
    let Some(first) = simple_hole_block_reference(ctx, blocks, prefix, entry_offset, first)? else {
        return Ok(None);
    };
    let Some(second) = simple_hole_block_reference(ctx, blocks, prefix, entry_offset, second)?
    else {
        return Ok(None);
    };
    Ok(Some(SimpleHoleReferencePair {
        references: [first, second],
        wrapped: pair.wrapped(),
    }))
}

pub(in crate::native) fn feature_simple_hole_repeated_scalar_lane_block_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
    inputs: &[FeatureInputBlock],
) -> Result<Vec<FeatureSimpleHoleRepeatedScalarLaneBlockReferences>, cadmpeg_core::CodecError> {
    let input_stores = OperationInputStores::new(ctx, inputs)?;
    let blocks = data_blocks(ctx, history.container())?;
    let mut block_index = DataBlockIndex::new();
    let mut block_index_storage = ctx.reserve_scoped(0, "NX simple hole data block index")?;
    for block in ctx.admit_iter(&blocks, "index NX simple hole data blocks")? {
        let Some(owner) = block_store(ctx, &block.id, "find NX simple hole data block owner")?
        else {
            continue;
        };
        let key = (owner, block.block_ordinal);
        if ctx.contains_key_btree_map(&block_index, &key, "find NX simple hole data block")? {
            continue;
        }
        block_index_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut block_index,
                key,
                block,
                "NX simple hole data block index",
            )
        })?;
    }
    let mut references = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let operation_label = format_feature_history_id(
                ctx,
                "operation-label",
                section_key,
                operation_ordinal,
                None,
            )?;
            let (_, Some(prefix)) = input_stores.get(ctx, &operation_label)? else {
                continue;
            };
            let Some(decoded) = crate::om::simple_hole_references::simple_hole_repeated_scalar_lane_block_references(ctx, record.payload_view())? else {
continue;
};
            let first = match simple_hole_reference_pair(
                ctx,
                &block_index,
                prefix,
                entry_offset,
                decoded[0],
            ) {
                Ok(Some(first)) => first,
                Ok(None) => continue,
                Err(error) => {
                    return Err(error);
                }
            };
            let second = match simple_hole_reference_pair(
                ctx,
                &block_index,
                prefix,
                entry_offset,
                decoded[1],
            ) {
                Ok(Some(second)) => second,
                Ok(None) => continue,
                Err(error) => {
                    return Err(error);
                }
            };
            let id = format_feature_history_id(
                ctx,
                "simple-hole-repeated-scalar-lane-block-references",
                section_key,
                operation_ordinal,
                None,
            )?;
            ctx.reserve_vec(&mut references, 1, "NX simple hole block reference lanes")?;
            references.push(FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
                id,
                operation_label,
                first,
                second,
            });
        }
    }
    Ok(references)
}

/// Group distinct simple-hole operations that address the same four construction blocks.
pub(in crate::native) fn feature_simple_hole_construction_groups(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    labels: &[FeatureOperationLabel],
    lanes: &[FeatureSimpleHoleRepeatedScalarLane],
    references: &[FeatureSimpleHoleRepeatedScalarLaneBlockReferences],
) -> Result<Vec<FeatureSimpleHoleConstructionGroup>, cadmpeg_core::CodecError> {
    let mut chronology_reservation = ctx.reserve_scoped(0, "NX hole operation chronology")?;
    let mut section_starts = BTreeMap::<&str, u64>::new();
    for label in ctx.admit_iter(labels, "find NX hole section starts")? {
        let section = label.section_link.as_str();
        if let Some(start) =
            ctx.get_mut_btree_map(&mut section_starts, &section, "find NX hole section start")?
        {
            *start = (*start).min(label.source_offset);
            continue;
        }
        chronology_reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut section_starts,
                section,
                label.source_offset,
                "NX hole section starts",
            )
        })?;
    }
    let mut chronology = Vec::new();
    for (index, label) in ctx
        .admit_iter(labels, "build NX hole operation chronology")?
        .enumerate()
    {
        let first_offset = ctx
            .get_btree_map(
                &section_starts,
                &label.section_link.as_str(),
                "find NX hole section start",
            )?
            .copied()
            .unwrap_or(label.source_offset);
        ctx.reserve_scoped_vec(
            &mut chronology_reservation,
            &mut chronology,
            1,
            "NX hole operation chronology",
        )?;
        chronology.push((index, label, first_offset));
    }
    ctx.sort_unstable_by_key(
        &mut chronology,
        |value| {
            let record = value.1;
            (
                value.2,
                record.section_link.as_str(),
                std::cmp::Reverse(record.source_offset),
                value.0,
            )
        },
        Ord::cmp,
        "sort NX hole operation chronology",
    )?;
    let mut position_of = BTreeMap::<&str, usize>::new();
    let mut position_storage = ctx.reserve_scoped(0, "NX hole chronology positions")?;
    for (position, (_, label, _)) in ctx
        .admit_iter(chronology.as_slice(), "index NX simple-hole chronology")?
        .enumerate()
    {
        position_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut position_of,
                label.id.as_str(),
                position,
                "NX hole chronology positions",
            )
        })?;
    }
    let (lane_groups, _lane_groups_reservation) = ctx.collect_scoped_btree_groups(
        lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "group NX simple-hole reference lanes",
    )?;
    let mut grouped_reservation = ctx.reserve_scoped(0, "NX simple hole group index")?;
    let mut grouped = BTreeMap::<
        ([&str; 2], [&str; 2]),
        Vec<(
            &FeatureSimpleHoleRepeatedScalarLaneBlockReferences,
            &FeatureSimpleHoleRepeatedScalarLane,
        )>,
    >::new();
    let mut ambiguous_groups = BTreeMap::<([&str; 2], [&str; 2]), usize>::new();
    for reference in ctx.admit_iter(references, "group NX simple-hole references")? {
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
        let Some(matching_lanes) = ctx.get_btree_map(
            &lane_groups,
            reference.operation_label.as_str(),
            "match NX simple-hole reference lanes",
        )?
        else {
            continue;
        };
        let lane = match matching_lanes.as_slice() {
            [lane] => *lane,
            [_, _, ..] => {
                grouped_reservation.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut ambiguous_groups,
                        key,
                        matching_lanes.len(),
                        "NX ambiguous simple hole groups",
                    )
                })?;
                continue;
            }
            [] => continue,
        };
        ctx.push_scoped_btree_group(
            &mut grouped_reservation,
            &mut grouped,
            key,
            || (reference, lane),
            0,
            "NX simple hole group candidates",
        )?;
    }
    let mut groups = Vec::new();
    let mut grouped = grouped.into_iter();
    for _ in ctx.admit_iter(&(0..grouped.len()), "visit NX simple-hole groups")? {
        let Some((key, candidates)) = grouped.next() else {
            return Err(ctx.refuse_codec_limit("visit NX simple-hole groups", 0, 1));
        };
        if ctx.contains_key_btree_map(
            &ambiguous_groups,
            &key,
            "check NX ambiguous simple hole group",
        )? {
            continue;
        }
        let mut positions_reservation = ctx.reserve_scoped(0, "NX simple hole group positions")?;
        let mut positioned = Vec::new();
        let mut missing = false;
        let mut candidates = candidates.into_iter();
        for index in ctx.admit_iter(
            &(0..candidates.len()),
            "position NX simple-hole group candidates",
        )? {
            let Some((reference, lane)) = candidates.next() else {
                return Err(ctx.refuse_codec_limit(
                    "position NX simple-hole group candidates",
                    0,
                    1,
                ));
            };
            let Some(&position) = ctx.get_btree_map(
                &position_of,
                reference.operation_label.as_str(),
                "find NX simple-hole chronology position",
            )?
            else {
                missing = true;
                break;
            };
            ctx.reserve_scoped_vec(
                &mut positions_reservation,
                &mut positioned,
                1,
                "NX simple hole group positions",
            )?;
            positioned.push((position, index, reference, lane));
        }
        if missing {
            continue;
        }
        ctx.sort_unstable_by_key(
            &mut positioned,
            |value| {
                let reference = value.2;
                (value.0, reference.operation_label.as_str(), value.1)
            },
            Ord::cmp,
            "sort NX simple hole group members",
        )?;
        if positioned.len() < 2 {
            continue;
        }
        let mut seen_labels = BTreeMap::<&str, usize>::new();
        let mut has_duplicate = false;
        for (position, (_, _, reference, _)) in ctx
            .admit_iter(positioned.as_slice(), "check NX simple-hole group labels")?
            .enumerate()
        {
            let label = reference.operation_label.as_str();
            if ctx.contains_key_btree_map(
                &seen_labels,
                label,
                "check NX simple-hole group label uniqueness",
            )? {
                has_duplicate = true;
                break;
            }
            positions_reservation.with_storage(|| {
                ctx.insert_btree_map(
                    &mut seen_labels,
                    label,
                    position,
                    "NX simple hole group label index",
                )
            })?;
        }
        if has_duplicate {
            continue;
        }
        let mut members = Vec::new();
        let mut positioned = positioned.into_iter();
        for _ in ctx.admit_iter(&(0..positioned.len()), "copy NX simple-hole group members")? {
            let Some((_, _, reference, lane)) = positioned.next() else {
                return Err(ctx.refuse_codec_limit("copy NX simple-hole group members", 0, 1));
            };
            let operation_label = ctx
                .copy_retained_text(&reference.operation_label, "NX simple hole group operation")?;
            let scalar_lane =
                ctx.copy_retained_text(&lane.id, "NX simple hole group scalar lane")?;
            let block_reference =
                ctx.copy_retained_text(&reference.id, "NX simple hole group block reference")?;
            ctx.reserve_vec(&mut members, 1, "NX simple hole group members")?;
            members.push(FeatureSimpleHoleConstructionMember {
                operation_label,
                scalar_lane,
                block_reference,
            });
        }
        let mut id_anchor = &members[0];
        for member in ctx.admit_iter(members.as_slice(), "choose NX simple-hole group identity")? {
            if ctx.compare(
                &id_anchor.operation_label,
                &member.operation_label,
                "compare NX simple-hole group identity labels",
            )? == std::cmp::Ordering::Greater
            {
                id_anchor = member;
            }
        }
        let id_key = operation_key(
            ctx,
            &id_anchor.operation_label,
            "find NX simple-hole group identity key",
        )?
        .unwrap_or("unknown");
        let id = ctx.format_retained(
            format_args!("nx:feature-history:simple-hole-construction-group#{id_key}"),
            "NX simple hole group identity",
        )?;
        // Two or more positioned members with distinct labels, checked above.
        let members = SimpleHoleConstructionMembers::from_distinct(members);
        let first_data_blocks = [
            ctx.copy_retained_text(key.0[0], "NX simple hole first block")?,
            ctx.copy_retained_text(key.0[1], "NX simple hole first block")?,
        ];
        let second_data_blocks = [
            ctx.copy_retained_text(key.1[0], "NX simple hole second block")?,
            ctx.copy_retained_text(key.1[1], "NX simple hole second block")?,
        ];
        ctx.reserve_vec(&mut groups, 1, "NX simple hole construction groups")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    indexed: &[(
        crate::container::entry_ref::EntryRef<'_>,
        crate::om::IndexedSection<'_>,
    )],
    entry_offset: u64,
    lane: &crate::om::HolePackageConstructionGroupLane,
) -> Result<Option<[ConstructionReference<String>; 4]>, cadmpeg_core::CodecError> {
    let mut resolved = [None, None, None, None];
    for (slot, reference) in lane.references.iter().enumerate() {
        let Some(data_block) =
            charged_unique_offset_data_block(ctx, indexed, reference.token.value())?
        else {
            return Ok(None);
        };
        let Some(source_offset) =
            entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(reference.offset))
        else {
            return Ok(None);
        };
        resolved[slot] = Some(ConstructionReference {
            token: reference.token,
            data_block,
            source_offset,
        });
    }
    let [Some(a), Some(b), Some(c), Some(d)] = resolved else {
        return Ok(None);
    };
    Ok(Some([a, b, c, d]))
}

pub(in crate::native) fn feature_hole_package_construction_group_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureHolePackageConstructionGroupLane>, cadmpeg_core::CodecError> {
    let indexed = history.container().indexed_om_sections(ctx)?;
    let mut lanes = Vec::new();
    for history_section in
        ctx.admit_iter(history.sections(), "visit NX feature history sections")?
    {
        let section_key = history_section.key.as_str();
        let entry_offset = history_section.entry_offset;
        for &(operation_ordinal, record) in ctx.admit_iter(
            &history_section.records,
            "visit NX feature operation records",
        )? {
            let Some(lane) = crate::om::hole_package_construction_group_lane(record.payload_view())
            else {
                continue;
            };
            let Some(references) = package_lane_references(ctx, &indexed, entry_offset, &lane)?
            else {
                continue;
            };
            let operation_label = format_feature_history_id(
                ctx,
                "operation-label",
                section_key,
                operation_ordinal,
                None,
            )?;
            let id = format_feature_history_id(
                ctx,
                "hole-package-construction-group-lane",
                section_key,
                operation_ordinal,
                None,
            )?;
            ctx.reserve_vec(&mut lanes, 1, "NX hole package lanes")?;
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
        }
    }
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lanes: &[FeatureHolePackageConstructionGroupLane],
    groups: &[FeatureSimpleHoleConstructionGroup],
) -> Result<Vec<FeatureHolePackageConstructionGroupUse>, cadmpeg_core::CodecError> {
    let mut reservation = ctx.reserve_scoped(0, "NX hole package group candidates")?;
    let (group_keys, _group_keys_reservation) = ctx.collect_scoped_btree_groups(
        groups
            .iter()
            .map(|group| (simple_hole_group_key(group), group)),
        "group NX simple-hole group keys",
    )?;
    let (lane_keys, _lane_keys_reservation) = ctx.collect_scoped_btree_groups(
        lanes.iter().map(|lane| (hole_package_lane_key(lane), lane)),
        "group NX hole package lane keys",
    )?;
    let mut matches = Vec::new();
    for group in ctx.admit_iter(groups, "join NX hole package groups")? {
        let key = simple_hole_group_key(group);
        let Some([_]) = ctx
            .get_btree_map(&group_keys, &key, "check NX unique simple-hole group")?
            .map(Vec::as_slice)
        else {
            continue;
        };
        let Some([lane]) = ctx
            .get_btree_map(&lane_keys, &key, "match NX hole package group lanes")?
            .map(Vec::as_slice)
        else {
            continue;
        };
        let lane = *lane;
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut matches,
            1,
            "NX hole package group candidates",
        )?;
        matches.push((group, lane));
    }
    ctx.sort_unstable_by_key(
        &mut matches,
        |value| {
            let (left, _) = value;
            simple_hole_group_key(left)
        },
        Ord::cmp,
        "sort NX hole package groups",
    )?;
    let mut uses = Vec::new();
    let mut matches = matches.into_iter();
    for _ in ctx.admit_iter(&(0..matches.len()), "build NX hole package group uses")? {
        let Some((group, lane)) = matches.next() else {
            return Err(ctx.refuse_codec_limit("build NX hole package group uses", 0, 1));
        };
        let id = replace_operation_text(
            ctx,
            &lane.id,
            "hole-package-construction-group-lane",
            "hole-package-construction-group-use",
            "NX hole package group use identity",
        )?;
        let operation_label =
            ctx.copy_retained_text(&lane.operation_label, "NX hole package group use label")?;
        let construction_group_lane =
            ctx.copy_retained_text(&lane.id, "NX hole package group use lane")?;
        let simple_hole_construction_group =
            ctx.copy_retained_text(&group.id, "NX hole package group use group")?;
        ctx.reserve_vec(&mut uses, 1, "NX hole package group uses")?;
        uses.push(FeatureHolePackageConstructionGroupUse {
            id,
            operation_label,
            construction_group_lane,
            simple_hole_construction_group,
            source_offset: lane.source_offset,
        });
    }
    Ok(uses)
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
    [u8; 8],
    "first_reference_prefix"
);
cadmpeg_core::named_optional_field!(
    deserialize_second_reference_prefix,
    [u8; 8],
    "second_reference_prefix"
);
