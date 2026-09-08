// SPDX-License-Identifier: Apache-2.0
//! Holes construction records and extraction.

use super::feature_input_blocks;
use super::feature_operation_chronological_labels;

use super::operation_record::FeatureOperationRecord;

use super::reference::ConstructionReference;
use super::unique_offset_data_block;
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

/// Exact text frame retained from a `SYMBOLIC_THREAD` operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureSymbolicThreadTextFrameWire",
    into = "FeatureSymbolicThreadTextFrameWire"
)]
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
            values: lane
                .values
                .iter()
                .map(|token| token.scalar.value())
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
            values: NonEmpty::new(values).ok_or("values must contain a repeated scalar")?,
        })
    }
}

/// Offset-store blocks linked after both repeated scalar-lane witnesses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureSimpleHoleRepeatedScalarLaneBlockReferencesWire",
    into = "FeatureSimpleHoleRepeatedScalarLaneBlockReferencesWire"
)]
pub struct FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
    pub id: String,
    pub operation_label: String,
    pub first: SimpleHoleReferencePair,
    pub second: SimpleHoleReferencePair,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleHoleReferencePair {
    pub references: [SimpleHoleBlockReference; 2],
    pub wrapped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleHoleBlockReference {
    pub data_block: String,
    pub source_offset: u64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_reference_prefix: Option<[u8; 8]>,
    /// Exact optional wrapper before the repeated reference pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    second_reference_prefix: Option<[u8; 8]>,
    /// Absolute offsets of the first pair of tagged-index tokens.
    first_reference_offsets: [u64; 2],
    /// Absolute offsets of the repeated pair of tagged-index tokens.
    second_reference_offsets: [u64; 2],
}

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
        if members
            .iter()
            .any(|member| !labels.insert(member.operation_label.as_str()))
        {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureHolePackageConstructionGroupLaneWire",
    into = "FeatureHolePackageConstructionGroupLaneWire"
)]
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

fn symbolic_thread_text_frames(
    record: crate::om::operation_record::OperationPayload<'_>,
) -> Option<Vec<crate::om::OperationPayloadTextFrame<'_>>> {
    if record.name() != "SYMBOLIC_THREAD" {
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
            let Some(frames) = symbolic_thread_text_frames(record.payload_view()) else {
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
            let Some(pair) = crate::om::simple_hole_repeated_scalar_lane(record.payload_view())
            else {
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
                crate::om::simple_hole_references::simple_hole_repeated_scalar_lane_block_references(record.payload_view())
            else {
                return;
            };
            let resolve = |pair: crate::om::simple_hole_references::ReferencePair| {
                let [first, second] = pair.references().map(|(token, offset)| {
                    let data_block = format!("{prefix}:block#{}", token.value());
                    blocks.contains(&data_block).then_some(())?;
                    Some(SimpleHoleBlockReference {
                        data_block,
                        source_offset: entry_offset.checked_add(offset as u64)?,
                    })
                });
                Some(SimpleHoleReferencePair {
                    references: [first?, second?],
                    wrapped: pair.wrapped(),
                })
            };
            let (Some(first), Some(second)) = (resolve(decoded[0]), resolve(decoded[1])) else {
                return;
            };
            references.push(FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
                id: format!(
                    "nx:feature-history:simple-hole-repeated-scalar-lane-block-references#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                first,
                second,
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
            reference
                .first
                .references
                .each_ref()
                .map(|reference| reference.data_block.clone()),
            reference
                .second
                .references
                .each_ref()
                .map(|reference| reference.data_block.clone()),
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
            let members = SimpleHoleConstructionMembers::new(
                members
                    .into_iter()
                    .map(|(reference, lane)| FeatureSimpleHoleConstructionMember {
                        operation_label: reference.operation_label.clone(),
                        scalar_lane: lane.id.clone(),
                        block_reference: reference.id.clone(),
                    })
                    .collect(),
            )
            .ok()?;
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
            let Some(lane) = crate::om::hole_package_construction_group_lane(record.payload_view())
            else {
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
                source_offset: entry_offset + record.payload_offset() as u64 + lane.offset as u64,
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
            .entry(
                lane.references
                    .each_ref()
                    .map(|reference| reference.data_block.clone()),
            )
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
    Some((form, extent, start_treatment, end_treatment))
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

#[cfg(test)]
mod tests;
