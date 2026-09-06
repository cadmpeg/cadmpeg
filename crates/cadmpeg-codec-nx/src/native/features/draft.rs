// SPDX-License-Identifier: Apache-2.0
//! Draft construction records and extraction.

use super::joined_payload::JoinedPayload;
use super::payload_content::FeaturePayloadBlock;
use super::payload_content::FeaturePayloadContent;
use super::reference::ConstructionReference;
use super::FeatureConstructionOwner;
use super::FeatureConstructionPayload;
use crate::container::Container;
use crate::om::compact::CompactIndexAtom;
use crate::om::compact::CountedIndexMembers;
use crate::om::compact::ExtendedCompactIndex;
use crate::om::compact::LocatedCompactIndex;
use crate::om::discriminators::DraftBinary32Branch;
use crate::om::draft_identity::DraftIdentityForm;
use crate::om::draft_identity::DraftIdentityFrame;
use crate::om::fixed::Q155Atom;
use crate::om::fixed::Q155LaneFrame;
use crate::om::fixed::Q155Marker;
use crate::om::fixed::Q155;
use crate::om::nonempty::NonEmpty;
use crate::om::scalar_run::FramedScalarRun;
use crate::printable_string::PrintableString;
use serde::Deserialize;

use crate::om::scalar::ShiftedBinary32;
use serde::Serialize;

use super::offset_data_block_bytes;

use super::resolved_feature_payload_references;
use super::unique_offset_data_store;
use super::visit_feature_history_operation_records;

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
    Unresolved(CountedIndexMembers<LocatedCompactIndex<u64>, 1>),
    Resolved(CountedIndexMembers<ConstructionReference<String, CompactIndexAtom>, 1>),
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
            FeatureDraftConstructionIndices::Unresolved(tokens) => {
                (tokens.into_iter().collect(), None)
            }
            FeatureDraftConstructionIndices::Resolved(tokens) => {
                let (tokens, blocks) = tokens
                    .into_iter()
                    .map(|row| {
                        (
                            LocatedCompactIndex {
                                atom: row.token,
                                offset: row.source_offset,
                            },
                            row.data_block,
                        )
                    })
                    .unzip();
                (tokens, Some(blocks))
            }
        };
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            declared_count: tokens.len() + 1,
            indices: tokens.iter().map(|token| token.atom.value()).collect(),
            raw_indices: tokens
                .iter()
                .map(|token| token.atom.raw().to_vec())
                .collect(),
            data_blocks,
            source_offsets: tokens.iter().map(|token| token.offset).collect(),
        }
    }
}

impl TryFrom<FeatureDraftConstructionIndexLaneWire> for FeatureDraftConstructionIndexLane {
    type Error = String;

    fn try_from(wire: FeatureDraftConstructionIndexLaneWire) -> Result<Self, Self::Error> {
        let count = wire.indices.len();
        if wire.declared_count != count + 1 {
            return Err(
                "declared_count must equal the reference count plus the implicit owner".into(),
            );
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
            .enumerate()
            .map(|(slot, ((value, raw), offset))| {
                Ok(LocatedCompactIndex {
                    atom: CompactIndexAtom::from_wire(value, &raw)
                        .map_err(|error| format!("indices[{slot}]: {error}"))?,
                    offset,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let tokens = tokens.into_iter();
        let indices = match wire.data_blocks {
            None => FeatureDraftConstructionIndices::Unresolved(
                CountedIndexMembers::new(tokens.collect())
                    .map_err(|error| format!("indices: {error}"))?,
            ),
            Some(blocks) => FeatureDraftConstructionIndices::Resolved(
                CountedIndexMembers::new(
                    tokens
                        .zip(blocks)
                        .map(|(token, data_block)| ConstructionReference {
                            token: token.atom,
                            source_offset: token.offset,
                            data_block,
                        })
                        .collect(),
                )
                .map_err(|error| format!("indices: {error}"))?,
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
            values: record
                .lane
                .iter()
                .map(|(_, atom, _)| atom.scalar.value())
                .collect(),
            markers: record
                .lane
                .iter()
                .map(|(_, atom, _)| atom.marker.byte())
                .collect(),
            raw_values: record
                .lane
                .iter()
                .map(|(_, atom, _)| atom.scalar.raw())
                .collect(),
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
        let values = wire
            .values
            .into_iter()
            .zip(wire.markers)
            .zip(wire.raw_values)
            .zip(wire.value_source_offsets)
            .map(|(((value, marker), raw), source)| {
                Ok((
                    Q155Atom {
                        marker: Q155Marker::read(marker).ok_or("markers must contain 48 or 176")?,
                        scalar: Q155::from_wire(value, raw)?,
                    },
                    source,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let lane = FramedScalarRun::new(
            Q155LaneFrame,
            wire.payload_offset,
            NonEmpty::new(values).ok_or("values must contain a Q1.55 atom")?,
        )?;
        if !lane
            .iter()
            .map(|(offset, _, _)| offset)
            .eq(wire.value_payload_offsets)
        {
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
            values: record
                .lane
                .iter()
                .map(|(_, scalar, _)| scalar.value())
                .collect(),
            raw_values: record
                .lane
                .iter()
                .map(|(_, scalar, _)| scalar.raw())
                .collect(),
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
        let values = wire
            .values
            .into_iter()
            .zip(wire.raw_values)
            .zip(wire.value_source_offsets)
            .map(|((value, raw), source)| Ok((ShiftedBinary32::from_wire(value, &raw)?, source)))
            .collect::<Result<Vec<_>, String>>()?;
        let lane = FramedScalarRun::new(
            branch,
            wire.payload_offset,
            NonEmpty::new(values).ok_or("values must contain a binary32 atom")?,
        )?;
        if !lane
            .iter()
            .map(|(offset, _, _)| offset)
            .eq(wire.value_payload_offsets)
        {
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
#[serde(
    try_from = "FeatureDraftConstructionIdentityFrameWire",
    into = "FeatureDraftConstructionIdentityFrameWire"
)]
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
        let frame = DraftIdentityFrame::from_wire(
            &wire.prefix,
            wire.form,
            wire.identity,
            wire.payload_offset,
        )
        .map_err(str::to_owned)?;
        if frame.identity_offset() != wire.identity_payload_offset {
            return Err(
                "identity_payload_offset must equal payload_offset plus prefix length".into(),
            );
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
    /// Checked terminal indices and tail with absolute source offsets.
    pub lane: crate::om::draft_terminal::DraftTerminalLane<u64>,
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
            indices: lane.lane.indices().map(|token| token.atom.value()),
            raw_indices: lane.lane.indices().map(|token| *token.atom.raw()),
            tail: lane.lane.tail(),
            index_source_offsets: lane.lane.indices().map(|token| token.offset),
            source_offset: lane.lane.offset(),
        }
    }
}

impl TryFrom<FeatureDraftConstructionTerminalLaneWire> for FeatureDraftConstructionTerminalLane {
    type Error = String;

    fn try_from(wire: FeatureDraftConstructionTerminalLaneWire) -> Result<Self, Self::Error> {
        let [first, second] = [0, 1].map(|slot| {
            ExtendedCompactIndex::from_wire(wire.indices[slot], &wire.raw_indices[slot])
                .map_err(|error| format!("indices[{slot}]: {error}"))
        });
        let lane = crate::om::draft_terminal::DraftTerminalLane::<u64>::new(
            [first?, second?],
            wire.tail,
            wire.source_offset,
        )
        .ok_or("source_offset overflows the terminal frame")?;
        if lane.indices().map(|token| token.offset) != wire.index_source_offsets {
            return Err(
                "index_source_offsets must follow source_offset in the terminal frame".into(),
            );
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            lane,
        })
    }
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
            let Some(lane) = crate::om::draft_feature_leading_index_lane(record.payload_view())
            else {
                return;
            };
            let section_ordinal = crate::om::draft_feature_payload_references(
                record.payload_view(),
            )
            .and_then(|graph| {
                let complete_indices = graph
                    .references
                    .iter()
                    .map(|reference| reference.token.value())
                    .chain(
                        lane.indices
                            .as_slice()
                            .iter()
                            .map(|token| token.atom.value()),
                    )
                    .collect::<Vec<_>>();
                unique_offset_data_store(&indexed, &complete_indices)
            });
            let tokens = lane.indices.map(|token| LocatedCompactIndex {
                atom: token.atom,
                offset: entry_offset + token.offset as u64,
            });
            let indices = match section_ordinal {
                None => FeatureDraftConstructionIndices::Unresolved(tokens),
                Some(section_ordinal) => {
                    FeatureDraftConstructionIndices::Resolved(tokens.map(|token| {
                        let data_block = format!(
                            "nx:om-data-blocks-{section_ordinal}:block#{}",
                            token.atom.value()
                        );
                        ConstructionReference {
                            token: token.atom,
                            source_offset: token.offset,
                            data_block,
                        }
                    }))
                }
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
                .as_slice()
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
            let store = tokens
                .as_slice()
                .first()?
                .data_block
                .rsplit_once(":block#")?
                .0;
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
            let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks)
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
            let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks)
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
            let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks)
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
            let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks)
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
            let Some(lane) = crate::om::draft_terminal::scan(record.payload_view())
                .and_then(|lane| lane.into_absolute(entry_offset))
            else {
                return;
            };
            lanes.push(FeatureDraftConstructionTerminalLane {
                id: format!(
                    "nx:feature-history:draft-construction-terminal-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                lane,
            });
        },
    );
    lanes
}

use super::deserialize_reference_lane_count;

#[cfg(test)]
mod tests;
