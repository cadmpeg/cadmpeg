// SPDX-License-Identifier: Apache-2.0
//! Draft construction records and extraction.

use super::joined_payload::JoinedPayload;
use super::payload_content::FeaturePayloadBlock;
use super::payload_content::FeaturePayloadContent;
use super::FeatureConstructionOwner;
use super::FeatureConstructionPayload;
use crate::container::Container;
use crate::om::compact::CompactIndexAtom;
use crate::om::compact::CountedIndexMembers;
use crate::om::compact::ExtendedCompactIndex;
#[cfg(test)]
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
use cadmpeg_core::CodecError;
use serde::Deserialize;

use crate::om::scalar::ShiftedBinary32;
use serde::Serialize;

use super::offset_data_block_bytes;

use super::construction_records::format_offset_data_block_id;
use super::construction_records::resolved_feature_payload_references;
use super::construction_records::unique_offset_data_store;

use super::format_feature_child_id;
use super::format_feature_history_id;
use super::replace_operation_text;
use super::visit_feature_history_operation_records;

use crate::om::draft_leading::DraftLeadingLane;
mod borrowed_wires;

/// Ordered construction reference carried by a bounded draft-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct FeatureDraftConstructionReference {
    /// Globally unique draft-construction-reference identity.
    pub(in crate::native) id: String,
    /// Owning `DRAFT` operation label.
    pub(in crate::native) operation_label: String,
    /// Zero-based slot order in the exact construction graph.
    pub(in crate::native) ordinal: u32,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub(in crate::native) token: crate::om::reference_index::PayloadIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_data_block"
    )]
    pub(in crate::native) data_block: Option<String>,
    /// Absolute file offset of the width marker.
    source_offset: u64,
}

/// Counted compact-index lane preceding a bounded draft construction graph.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureDraftConstructionIndexLaneWire")]
pub(in crate::native) struct FeatureDraftConstructionIndexLane {
    pub(in crate::native) id: String,
    pub(in crate::native) operation_label: String,
    indices: FeatureDraftConstructionIndices,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FeatureDraftConstructionIndices {
    Unresolved(DraftLeadingLane<(), u64>),
    Resolved(DraftLeadingLane<String, u64>),
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_data_blocks"
    )]
    data_blocks: Option<Vec<String>>,
    /// Absolute source offsets of the compact-index tokens.
    source_offsets: Vec<u64>,
}

#[cfg(test)]
impl From<FeatureDraftConstructionIndexLane> for FeatureDraftConstructionIndexLaneWire {
    fn from(lane: FeatureDraftConstructionIndexLane) -> Self {
        let (declared_count, tokens, data_blocks): (_, Vec<_>, _) = match lane.indices {
            FeatureDraftConstructionIndices::Unresolved(frame) => (
                usize::from(frame.declared_count()),
                frame
                    .indices()
                    .map(|token| LocatedCompactIndex {
                        atom: token.atom,
                        offset: token.offset,
                    })
                    .collect(),
                None,
            ),
            FeatureDraftConstructionIndices::Resolved(frame) => {
                let (tokens, blocks) = frame
                    .indices()
                    .map(|token| {
                        (
                            LocatedCompactIndex {
                                atom: token.atom,
                                offset: token.offset,
                            },
                            token.target.clone(),
                        )
                    })
                    .unzip();
                (usize::from(frame.declared_count()), tokens, Some(blocks))
            }
        };
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            declared_count,
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
        let origin = wire
            .source_offsets
            .first()
            .and_then(|offset| offset.checked_sub(24))
            .ok_or("source_offsets: missing draft frame prefix")?;
        let tokens = wire
            .indices
            .into_iter()
            .zip(wire.raw_indices)
            .enumerate()
            .map(|(slot, (value, raw))| {
                CompactIndexAtom::from_wire(value, &raw)
                    .map(Into::into)
                    .map_err(|error| format!("indices[{slot}]: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let tokens =
            CountedIndexMembers::new(tokens).map_err(|error| format!("indices: {error}"))?;
        let frame = DraftLeadingLane::<(), u64>::new(tokens, origin)
            .ok_or("source_offsets: draft frame extent overflows")?;
        if !frame
            .indices()
            .map(|token| token.offset)
            .eq(wire.source_offsets)
        {
            return Err("source_offsets: must follow the compact-index token widths".into());
        }
        let indices = match wire.data_blocks {
            None => FeatureDraftConstructionIndices::Unresolved(frame),
            Some(blocks) => {
                let mut blocks = blocks.into_iter();
                FeatureDraftConstructionIndices::Resolved(
                    frame
                        .try_resolve(|_| blocks.next())
                        .ok_or("data_blocks: missing draft lane target")?,
                )
            }
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
pub(in crate::native) struct FeatureDraftConstructionGraphPayload {
    /// Globally unique reconstructed-payload identity.
    pub(in crate::native) id: String,
    /// Owning `DRAFT` operation label.
    pub(in crate::native) operation_label: String,
    /// Counted index lane establishing the common offset store.
    index_lane: String,
    /// Ordered construction-reference records.
    construction_references: [String; 4],
    /// Ordered source blocks and the hash of their concatenated bytes.
    #[serde(flatten)]
    content: FeaturePayloadContent<[FeaturePayloadBlock; 4]>,
}

/// Complete signed Q1.55 lane in a reconstructed draft graph payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "FeatureDraftConstructionFixedLaneWire")]
pub(in crate::native) struct FeatureDraftConstructionFixedLane {
    /// Globally unique lane identity.
    pub(in crate::native) id: String,
    /// Owning `DRAFT` operation label.
    pub(in crate::native) operation_label: String,
    /// Reconstructed graph payload carrying the lane.
    graph_payload: String,
    /// Zero-based lane order in the reconstructed payload.
    pub(in crate::native) ordinal: u32,
    /// Framed scalar run with absolute source locations.
    lane: FramedScalarRun<Q155LaneFrame, u64>,
    /// Absolute source offset of the fixed discriminator.
    source_offset: u64,
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

#[cfg(test)]
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
            NonEmpty::from_vec(values).ok_or("values must contain a Q1.55 atom")?,
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "FeatureDraftConstructionBinary32LaneWire")]
pub(in crate::native) struct FeatureDraftConstructionBinary32Lane {
    /// Globally unique lane identity.
    pub(in crate::native) id: String,
    /// Owning `DRAFT` operation label.
    pub(in crate::native) operation_label: String,
    /// Reconstructed graph payload carrying the lane.
    graph_payload: String,
    /// Zero-based lane order in the reconstructed payload.
    pub(in crate::native) ordinal: u32,
    /// Typed branch and contiguous atoms with absolute source locations.
    lane: FramedScalarRun<DraftBinary32Branch, u64>,
    /// Absolute source offset of the discriminator.
    source_offset: u64,
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

#[cfg(test)]
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
                .map(|(_, scalar, _)| scalar.value().get())
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
            .map(|((value, raw), source)| Ok((ShiftedBinary32::from_wire(value, raw)?, source)))
            .collect::<Result<Vec<_>, String>>()?;
        let lane = FramedScalarRun::new(
            branch,
            wire.payload_offset,
            NonEmpty::from_vec(values).ok_or("values must contain a binary32 atom")?,
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
pub(in crate::native) struct FeatureDraftConstructionGraphString {
    /// Globally unique string identity.
    pub(in crate::native) id: String,
    /// Owning `DRAFT` operation label.
    pub(in crate::native) operation_label: String,
    /// Reconstructed graph payload carrying the string.
    graph_payload: String,
    /// Zero-based string order in the reconstructed payload.
    pub(in crate::native) ordinal: u32,
    /// Exact printable value.
    value: PrintableString<String>,
    /// Payload-relative offset of the `66 32 03` marker.
    payload_offset: u64,
    /// Absolute source offset of the marker.
    source_offset: u64,
}

/// Complete identity frame in a reconstructed draft construction payload.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureDraftConstructionIdentityFrameWire")]
pub(in crate::native) struct FeatureDraftConstructionIdentityFrame {
    /// Globally unique frame identity.
    pub(in crate::native) id: String,
    /// Owning `DRAFT` operation label.
    pub(in crate::native) operation_label: String,
    /// Reconstructed payload carrying the frame.
    draft_construction_payload: String,
    /// Zero-based frame order in the reconstructed payload.
    pub(in crate::native) ordinal: u32,
    /// Exact prefix tokens, identity, and bounded payload position.
    frame: DraftIdentityFrame,
    /// Absolute source offset of the opening marker.
    source_offset: u64,
    /// Absolute source offset of the identity.
    identity_source_offset: u64,
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

#[cfg(test)]
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureDraftConstructionTerminalLaneWire")]
pub(in crate::native) struct FeatureDraftConstructionTerminalLane {
    /// Globally unique lane identity.
    pub(in crate::native) id: String,
    /// Owning `DRAFT` operation label.
    pub(in crate::native) operation_label: String,
    /// Checked terminal indices and tail with absolute source offsets.
    lane: crate::om::draft_terminal::DraftTerminalLane<u64>,
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

#[cfg(test)]
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
            ExtendedCompactIndex::from_wire(wire.indices[slot], wire.raw_indices[slot])
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
pub(in crate::native) fn feature_draft_construction_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureDraftConstructionReference>, cadmpeg_core::CodecError> {
    let references = resolved_feature_payload_references(ctx, container, |record, base| {
        crate::om::draft_references::draft_feature_payload_references(record)
            .and_then(|field| field.relocate(base))
            .map(|field| field.references().into_iter().collect())
    })?;
    let mut output = Vec::new();
    for reference in references {
        let operation_label = format_feature_history_id(
            ctx,
            "operation-label",
            &reference.section_key,
            reference.operation_ordinal,
            None,
        )?;
        let id = format_feature_history_id(
            ctx,
            "draft-construction-reference",
            &reference.section_key,
            reference.operation_ordinal,
            Some(reference.ordinal),
        )?;
        ctx.reserve_vec(&mut output, 1, "NX draft construction references")?;
        output.push(FeatureDraftConstructionReference {
            id,
            operation_label,
            ordinal: u32::try_from(reference.ordinal).map_err(|_| {
                ctx.refuse_codec_limit("NX draft construction reference ordinal", 0, 1)
            })?,
            token: reference.token,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        });
    }
    Ok(output)
}

/// Decode exact counted compact-index lanes preceding draft construction graphs.
pub(in crate::native) fn feature_draft_construction_index_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureDraftConstructionIndexLane>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut lanes = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let lane = match crate::om::draft_leading::scan(ctx, record.payload_view()) {
                Ok(lane) => lane,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let Some(lane) = lane else {
                return;
            };
            let projected =
                (|| -> Result<Option<FeatureDraftConstructionIndexLane>, CodecError> {
                    let section_ordinal = if let Some(graph) =
                        crate::om::draft_references::draft_feature_payload_references(
                            record.payload_view(),
                        ) {
                        let count =
                            4usize.checked_add(lane.indices().count()).ok_or_else(|| {
                                ctx.refuse_codec_limit("NX draft complete reference indices", 0, 1)
                            })?;

                        let (mut complete_indices, _indices_reservation) =
                            ctx.temporary_vec(count, "NX draft complete reference indices")?;
                        complete_indices.extend(
                            graph
                                .references()
                                .into_iter()
                                .map(|(token, _)| token.value()),
                        );
                        complete_indices.extend(lane.indices().map(|token| token.atom.value()));
                        let work = indexed.len().checked_mul(count).ok_or_else(|| {
                            ctx.refuse_codec_limit("resolve NX draft reference store", 0, 1)
                        })?;
                        ctx.charge_work(
                            cadmpeg_core::decode::u64_from_index(work),
                            "resolve NX draft reference store",
                        )?;
                        unique_offset_data_store(&indexed, &complete_indices)
                    } else {
                        None
                    };
                    let Some(frame) = lane.into_absolute(entry_offset) else {
                        return Ok(None);
                    };
                    let indices = match section_ordinal {
                        None => FeatureDraftConstructionIndices::Unresolved(frame),
                        Some(section_ordinal) => FeatureDraftConstructionIndices::Resolved(
                            frame.resolve(ctx, |index| {
                                format_offset_data_block_id(ctx, section_ordinal, index)
                            })?,
                        ),
                    };
                    let id = format_feature_history_id(
                        ctx,
                        "draft-construction-index-lane",
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
                    ctx.reserve_vec(&mut lanes, 1, "NX draft construction index lanes")?;
                    Ok(Some(FeatureDraftConstructionIndexLane {
                        id,
                        operation_label,
                        indices,
                    }))
                })();
            match projected {
                Ok(Some(lane)) => lanes.push(lane),
                Ok(None) => {}
                Err(error) => failure = Some(error),
            }
        },
    )?;
    if let Some(error) = failure {
        Err(error)
    } else {
        Ok(lanes)
    }
}

/// Reconstruct ordered logical payloads from resolved draft index lanes.
pub(in crate::native) fn feature_draft_construction_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    lanes: &[FeatureDraftConstructionIndexLane],
) -> Result<Vec<FeatureConstructionPayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut output = Vec::new();
    for lane in lanes {
        let FeatureDraftConstructionIndices::Resolved(tokens) = &lane.indices else {
            continue;
        };
        let count = tokens.indices().count();
        let mut source_id_storage = ctx.reserve_scoped(0, "NX payload source identity headers")?;
        let mut data_blocks = source_id_storage
            .with_storage(|| ctx.collection_vec(count, "NX draft construction source blocks"))?;
        for row in tokens.indices() {
            data_blocks
                .push(ctx.copy_retained_text(row.target, "NX draft construction source block")?);
        }
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };
        let id = replace_operation_text(
            ctx,
            &lane.id,
            "draft-construction-index-lane#",
            "draft-construction-payload#",
            "NX draft construction payload identity",
        )?;
        let operation_label = ctx.copy_retained_text(
            &lane.operation_label,
            "NX draft construction operation label",
        )?;
        let index_lane = ctx.copy_retained_text(&lane.id, "NX draft construction index lane")?;
        ctx.reserve_vec(&mut output, 1, "NX draft construction payloads")?;
        output.push(FeatureConstructionPayload {
            id,
            operation_label,
            owner: FeatureConstructionOwner::Draft { index_lane },
            content,
        });
    }
    Ok(output)
}

/// Reconstruct ordered logical payloads from complete draft construction graphs.
pub(in crate::native) fn feature_draft_construction_graph_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    lanes: &[FeatureDraftConstructionIndexLane],
    references: &[FeatureDraftConstructionReference],
) -> Result<Vec<FeatureDraftConstructionGraphPayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut output = Vec::new();
    for lane in lanes {
        let FeatureDraftConstructionIndices::Resolved(tokens) = &lane.indices else {
            continue;
        };
        let Some(store) = tokens
            .indices()
            .next()
            .and_then(|row| row.target.rsplit_once(":block#").map(|(store, _)| store))
        else {
            continue;
        };
        let scan_work = references
            .len()
            .checked_mul(2)
            .ok_or_else(|| ctx.refuse_codec_limit("scan NX draft construction graph", 0, 1))?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(scan_work),
            "scan NX draft construction graph",
        )?;
        let count = references
            .iter()
            .filter(|reference| reference.operation_label == lane.operation_label)
            .count();

        let (mut graph, _graph_reservation) =
            ctx.temporary_vec(count, "NX draft construction graph")?;
        graph.extend(
            references
                .iter()
                .filter(|reference| reference.operation_label == lane.operation_label),
        );
        ctx.stable_sort_by(
            &mut graph,
            |left, right| left.ordinal.cmp(&right.ordinal),
            |_| 0,
            "sort NX draft construction graph",
        )?;
        if graph
            .iter()
            .enumerate()
            .any(|(ordinal, reference)| u32::try_from(ordinal) != Ok(reference.ordinal))
        {
            continue;
        }
        let Ok(graph): Result<[&FeatureDraftConstructionReference; 4], _> = graph.try_into() else {
            continue;
        };
        let mut source_id_storage = ctx.reserve_scoped(0, "NX payload source identity headers")?;
        let mut data_blocks = source_id_storage
            .with_storage(|| ctx.collection_vec(4, "NX draft graph source blocks"))?;
        for reference in graph {
            let Some(block) = reference.data_block.as_deref() else {
                break;
            };
            data_blocks.push(ctx.copy_retained_text(block, "NX draft graph source block")?);
        }
        if data_blocks.len() != 4 {
            continue;
        }
        if data_blocks.iter().any(|block| {
            block
                .rsplit_once(":block#")
                .is_none_or(|(prefix, _)| prefix != store)
        }) {
            continue;
        }
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };
        let Some((_, key)) = lane.id.rsplit_once('#') else {
            continue;
        };
        let prefix = "nx:feature-history:draft-construction-graph-payload#";
        let id_len = prefix
            .len()
            .checked_add(key.len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX draft graph payload identity", 0, 1))?;
        let mut id = ctx.retained_string(id_len, "NX draft graph payload identity")?;
        id.push_str(prefix);
        id.push_str(key);
        let mut construction_references: [String; 4] = std::array::from_fn(|_| String::new());
        for (slot, reference) in graph.into_iter().enumerate() {
            construction_references[slot] =
                ctx.copy_retained_text(&reference.id, "NX draft graph reference identity")?;
        }
        ctx.reserve_vec(&mut output, 1, "NX draft construction graph payloads")?;
        output.push(FeatureDraftConstructionGraphPayload {
            id,
            operation_label: ctx
                .copy_retained_text(&lane.operation_label, "NX draft graph operation label")?,
            index_lane: ctx.copy_retained_text(&lane.id, "NX draft graph index lane")?,
            construction_references,
            content,
        });
    }
    Ok(output)
}

/// Decode complete signed Q1.55 lanes from reconstructed draft graph payloads.
pub(in crate::native) fn feature_draft_construction_fixed_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureDraftConstructionGraphPayload],
) -> Result<Vec<FeatureDraftConstructionFixedLane>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut lanes = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(ctx, payload.content.block_ids(), &blocks)?
        else {
            continue;
        };
        for (ordinal, lane) in crate::om::draft_construction_fixed_lanes(ctx, joined.bytes())?
            .into_iter()
            .enumerate()
        {
            let payload_offset = lane.offset();
            let Some(lane) =
                lane.try_map_locations(ctx, |offset, ()| joined.source_offset(offset))?
            else {
                continue;
            };
            let Some(source_offset) = joined.source_offset(payload_offset) else {
                continue;
            };
            let id = format_feature_child_id(ctx, &payload.id, "-fixed-lane-", ordinal)?;
            let operation_label =
                ctx.copy_retained_text(&payload.operation_label, "NX draft fixed lane operation")?;
            let graph_payload = ctx.copy_retained_text(&payload.id, "NX draft fixed lane graph")?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX draft fixed lane ordinal", 0, 1))?;
            ctx.reserve_vec(&mut lanes, 1, "NX draft construction fixed lanes")?;
            lanes.push(FeatureDraftConstructionFixedLane {
                id,
                operation_label,
                graph_payload,
                ordinal,
                lane,
                source_offset,
            });
        }
    }
    Ok(lanes)
}

/// Decode complete shifted-binary32 lanes from reconstructed draft graph payloads.
pub(in crate::native) fn feature_draft_construction_binary32_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureDraftConstructionGraphPayload],
) -> Result<Vec<FeatureDraftConstructionBinary32Lane>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut lanes = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(ctx, payload.content.block_ids(), &blocks)?
        else {
            continue;
        };
        for (ordinal, lane) in crate::om::draft_construction_binary32_lanes(ctx, joined.bytes())?
            .into_iter()
            .enumerate()
        {
            let payload_offset = lane.offset();
            let Some(lane) =
                lane.try_map_locations(ctx, |offset, ()| joined.source_offset(offset))?
            else {
                continue;
            };
            let Some(source_offset) = joined.source_offset(payload_offset) else {
                continue;
            };
            let id = format_feature_child_id(ctx, &payload.id, "-binary32-lane-", ordinal)?;
            let operation_label = ctx
                .copy_retained_text(&payload.operation_label, "NX draft binary32 lane operation")?;
            let graph_payload =
                ctx.copy_retained_text(&payload.id, "NX draft binary32 lane graph")?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX draft binary32 lane ordinal", 0, 1))?;
            ctx.reserve_vec(&mut lanes, 1, "NX draft construction binary32 lanes")?;
            lanes.push(FeatureDraftConstructionBinary32Lane {
                id,
                operation_label,
                graph_payload,
                ordinal,
                lane,
                source_offset,
            });
        }
    }
    Ok(lanes)
}

/// Decode canonical printable strings from reconstructed draft graph payloads.
pub(in crate::native) fn feature_draft_construction_graph_strings(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureDraftConstructionGraphPayload],
) -> Result<Vec<FeatureDraftConstructionGraphString>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut strings = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(ctx, payload.content.block_ids(), &blocks)?
        else {
            continue;
        };
        for (ordinal, value) in crate::om::string_values(ctx, joined.bytes(), 0)?
            .into_iter()
            .enumerate()
        {
            let payload_offset = cadmpeg_core::decode::u64_from_index(value.offset);
            let Some(source_offset) = joined.source_offset(payload_offset) else {
                continue;
            };
            let id = format_feature_child_id(ctx, &payload.id, "-string-", ordinal)?;
            let operation_label =
                ctx.copy_retained_text(&payload.operation_label, "NX draft string operation")?;
            let graph_payload = ctx.copy_retained_text(&payload.id, "NX draft string graph")?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX draft string ordinal", 0, 1))?;
            let value = PrintableString::new(
                ctx.copy_retained_text(value.value.as_str(), "NX draft construction string")?,
            )
            .map_err(|reason| CodecError::Malformed(reason.into()))?;
            ctx.reserve_vec(&mut strings, 1, "NX draft construction graph strings")?;
            strings.push(FeatureDraftConstructionGraphString {
                id,
                operation_label,
                graph_payload,
                ordinal,
                value,
                payload_offset,
                source_offset,
            });
        }
    }
    Ok(strings)
}

/// Decode complete identity frames from reconstructed draft construction payloads.
pub(in crate::native) fn feature_draft_construction_identity_frames(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeatureDraftConstructionIdentityFrame>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut output = Vec::new();
    for payload in payloads {
        let Some(joined) = JoinedPayload::from_source(ctx, payload.content.block_ids(), &blocks)?
        else {
            continue;
        };
        for (ordinal, frame) in crate::om::draft_construction_identity_frames(ctx, joined.bytes())?
            .into_iter()
            .enumerate()
        {
            let payload_offset = frame.offset();
            let identity_payload_offset = frame.identity_offset();
            let (Some(source_offset), Some(identity_source_offset)) = (
                joined.source_offset(payload_offset),
                joined.source_offset(identity_payload_offset),
            ) else {
                continue;
            };
            let id = format_feature_child_id(ctx, &payload.id, "-identity-frame-", ordinal)?;
            let operation_label =
                ctx.copy_retained_text(&payload.operation_label, "NX draft identity operation")?;
            let draft_construction_payload =
                ctx.copy_retained_text(&payload.id, "NX draft identity payload")?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX draft identity ordinal", 0, 1))?;
            ctx.reserve_vec(&mut output, 1, "NX draft construction identity frames")?;
            output.push(FeatureDraftConstructionIdentityFrame {
                id,
                operation_label,
                draft_construction_payload,
                ordinal,
                frame,
                source_offset,
                identity_source_offset,
            });
        }
    }
    Ok(output)
}

/// Decode complete end-anchored terminal lanes from draft construction payloads.
pub(in crate::native) fn feature_draft_construction_terminal_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureDraftConstructionTerminalLane>, cadmpeg_core::CodecError> {
    let mut lanes = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let Some(lane) = crate::om::draft_terminal::scan(record.payload_view())
                .and_then(|lane| lane.into_absolute(entry_offset))
            else {
                return;
            };
            let projected = (|| -> Result<_, CodecError> {
                let id = format_feature_history_id(
                    ctx,
                    "draft-construction-terminal-lane",
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
                ctx.reserve_vec(&mut lanes, 1, "NX draft construction terminal lanes")?;
                Ok(FeatureDraftConstructionTerminalLane {
                    id,
                    operation_label,
                    lane,
                })
            })();
            match projected {
                Ok(lane) => lanes.push(lane),
                Err(error) => failure = Some(error),
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(lanes)
}

use super::deserialize_reference_lane_count;

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_data_block, String, "data_block");
cadmpeg_core::named_optional_field!(deserialize_data_blocks, Vec<String>, "data_blocks");
