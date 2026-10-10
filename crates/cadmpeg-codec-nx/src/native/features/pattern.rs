// SPDX-License-Identifier: Apache-2.0
//! Pattern construction records and extraction.

use crate::om::branch_items::BranchItems;
use crate::om::counted_pattern_references::CountedPatternReferences;
use crate::om::pattern_references::{PatternPayloadReferenceLayout, PatternReferences};
use crate::om::reference_index::PayloadIndexToken;
use std::collections::BTreeMap;

use crate::container::Container;

use super::joined_payload::JoinedPayload;
use super::payload_content::{
    copy_block_ids, operation_key, shared_block_store, FeaturePayloadContent,
};
use super::FeatureConstructionOwner;
use super::FeatureConstructionPayload;
use super::FeatureOperationLabel;
use super::FeaturePatternKind;
use crate::om::scalar_run::FramedScalarRun;
use serde::Deserialize;

use crate::om::fixed::Q155Atom;
use crate::om::fixed::Q155LaneFrame;
use crate::om::fixed::Q155Marker;
use crate::om::fixed::Q155;
use crate::om::nonempty::NonEmpty;
use crate::om::pattern::PatternRow;
use crate::om::pattern::PatternRows;
use crate::om::pattern::PatternScalarEncoding;
use crate::om::pattern::PatternTerminal;
use crate::om::pattern::PatternValue;
use crate::om::pattern::PatternWideValues;
use crate::om::scalar::ShiftedBinary64;
use crate::om::scalar::ShiftedScalar;
use crate::printable_string::PrintableString;
use serde::Serialize;
use std::num::NonZeroU8;

mod borrowed_wires;

use super::format_feature_child_id;
use super::format_feature_history_id;
use super::offset_data_block_bytes;

use super::charged_unique_offset_data_block;
use super::FeatureHistory;

/// Ordered construction reference carried by a bounded pattern payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct FeaturePatternReference {
    /// Globally unique pattern-reference identity.
    pub(in crate::native) id: String,
    /// Owning pattern operation label.
    pub(in crate::native) operation_label: String,
    /// Exact byte layout that framed the reference field.
    layout: PatternPayloadReferenceLayout,
    /// Zero-based non-null slot order in the exact reference field.
    pub(in crate::native) ordinal: u32,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub(in crate::native) token: PayloadIndexToken,
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

/// Exact counted reference lane carried by a bounded `Pattern Feature` payload.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeaturePatternCountedReferenceLaneWire")]
pub(in crate::native) struct FeaturePatternCountedReferenceLane {
    pub(in crate::native) id: String,
    pub(in crate::native) operation_label: String,
    references: CountedPatternReferences<Option<String>>,
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

#[cfg(test)]
impl From<FeaturePatternCountedReferenceLane> for FeaturePatternCountedReferenceLaneWire {
    fn from(value: FeaturePatternCountedReferenceLane) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            declared_count: usize::from(value.references.declared_count()),
            source_offset: value.references.offset(),
            object_indices: value
                .references
                .iter()
                .map(|(_, token, _)| token.value())
                .collect(),
            raw_object_indices: value
                .references
                .iter()
                .map(|(_, token, _)| token.raw().to_vec())
                .collect(),
            data_blocks: value
                .references
                .iter()
                .map(|(_, _, target)| target.clone())
                .collect(),
            object_index_source_offsets: value
                .references
                .iter()
                .map(|(offset, _, _)| offset)
                .collect(),
        }
    }
}

impl TryFrom<FeaturePatternCountedReferenceLaneWire> for FeaturePatternCountedReferenceLane {
    type Error = String;
    fn try_from(wire: FeaturePatternCountedReferenceLaneWire) -> Result<Self, Self::Error> {
        let count = wire.object_indices.len();
        if wire.declared_count != count + 1 {
            return Err(
                "declared_count must equal the reference count plus the implicit owner".into(),
            );
        }
        if wire.raw_object_indices.len() != count
            || wire.data_blocks.len() != count
            || wire.object_index_source_offsets.len() != count
        {
            return Err("object_indices, raw_object_indices, data_blocks, and object_index_source_offsets must have equal lengths".into());
        }
        let entries = wire
            .object_indices
            .into_iter()
            .zip(wire.raw_object_indices)
            .zip(wire.data_blocks)
            .map(|((value, raw), target)| {
                let token = PayloadIndexToken::from_wire(value, &raw)
                    .map_err(|error| format!("object_indices/raw_object_indices: {error}"))?;
                Ok((token, target))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let references = CountedPatternReferences::new(
            wire.source_offset,
            BranchItems::new(entries).map_err(|error| format!("declared_count: {error}"))?,
        )?;
        if references
            .iter()
            .map(|(offset, _, _)| offset)
            .ne(wire.object_index_source_offsets)
        {
            return Err(
                "object_index_source_offsets: must follow the counted reference frame".into(),
            );
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            references,
        })
    }
}

/// Canonical printable string in a reconstructed pattern payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct FeaturePatternConstructionString {
    /// Globally unique string identity.
    pub(in crate::native) id: String,
    /// Owning `Pattern Feature` or `Pattern Geometry` operation label.
    pub(in crate::native) operation_label: String,
    /// Reconstructed pattern payload carrying the string.
    construction_payload: String,
    /// Zero-based string order within the payload.
    pub(in crate::native) ordinal: u32,
    /// Exact printable value.
    value: PrintableString<String>,
    /// Payload-relative offset of the `66 32 03` marker.
    payload_offset: u64,
    /// Absolute source offset of the marker.
    source_offset: u64,
}

/// Complete signed Q1.55 lane in a reconstructed pattern payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "FeaturePatternConstructionFixedLaneWire")]
pub(in crate::native) struct FeaturePatternConstructionFixedLane {
    /// Globally unique lane identity.
    pub(in crate::native) id: String,
    /// Owning `Pattern Feature` or `Pattern Geometry` operation label.
    pub(in crate::native) operation_label: String,
    /// Reconstructed pattern payload carrying the lane.
    construction_payload: String,
    /// Zero-based lane order within the payload.
    pub(in crate::native) ordinal: u32,
    /// Framed scalar run with absolute source locations.
    lane: FramedScalarRun<Q155LaneFrame, u64>,
    /// Absolute source offset of the fixed discriminator.
    source_offset: u64,
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

#[cfg(test)]
impl From<FeaturePatternConstructionFixedLane> for FeaturePatternConstructionFixedLaneWire {
    fn from(record: FeaturePatternConstructionFixedLane) -> Self {
        Self {
            id: record.id,
            operation_label: record.operation_label,
            construction_payload: record.construction_payload,
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
            NonEmpty::from_admitted_vec(values).ok_or("values must contain a Q1.55 atom")?,
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
enum FeaturePatternTransformLayout {
    /// One shifted scalar per row and terminal mode `01`.
    ScalarRows,
    /// Four shifted binary64 values and one terminal value per row, with terminal mode `02`.
    WideRows,
}

/// Exact counted transform lane carried by a bounded pattern payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "FeaturePatternTransformLaneWire")]
pub(in crate::native) struct FeaturePatternTransformLane {
    pub(in crate::native) id: String,
    pub(in crate::native) operation_label: String,
    row_schema_index: NonZeroU8,
    rows: PatternRows<crate::om::compact::LocatedCompactIndex<u64>, u64>,
    source_offset: u64,
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

#[cfg(test)]
impl FeaturePatternTransformLaneWire {
    fn push_scalar(
        &mut self,
        encoding: PatternScalarEncoding,
        value: f64,
        raw: &[u8],
        source_offset: u64,
    ) {
        self.encodings.push(encoding);
        self.values.push(value);
        self.raw_values.push(raw.to_vec());
        self.value_source_offsets.push(source_offset);
    }

    fn push_selector(&mut self, selector: crate::om::compact::LocatedCompactIndex<u64>) {
        self.selectors.push(selector.atom.value());
        self.raw_selectors.push(selector.atom.raw().to_vec());
        self.selector_source_offsets.push(selector.offset);
    }
}

#[cfg(test)]
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
                    wire.push_scalar(
                        encoding,
                        row.values.scalar.value().get(),
                        row.values.scalar.raw(),
                        row.values.offset,
                    );
                    wire.push_selector(row.selector);
                }
            }
            PatternRows::Wide(rows) => {
                for row in rows.into_vec() {
                    for value in row.values.first {
                        wire.push_scalar(
                            PatternScalarEncoding::Binary64,
                            value.scalar.value().get(),
                            value.scalar.as_bytes(),
                            value.offset,
                        );
                    }
                    let terminal = row.values.terminal;
                    wire.push_scalar(
                        terminal.scalar.encoding(),
                        terminal.scalar.value().get(),
                        terminal.scalar.raw(),
                        terminal.offset,
                    );
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
        let scalar =
            ShiftedScalar::read(&self.raw).ok_or("raw_values must contain shifted scalar atoms")?;
        let encoding = match scalar {
            ShiftedScalar::Binary32(_) => PatternScalarEncoding::Binary32,
            ShiftedScalar::Binary64(_) => PatternScalarEncoding::Binary64,
        };
        if self.encoding != encoding
            || self.raw.len() != scalar.raw().len()
            || self.value.to_bits() != scalar.value().get().to_bits()
        {
            return Err("encodings and values must match each exact raw_values atom".into());
        }
        Ok(PatternValue {
            scalar,
            offset: self.source_offset,
        })
    }

    fn binary64(&self) -> Result<PatternValue<ShiftedBinary64, u64>, String> {
        if self.encoding != PatternScalarEncoding::Binary64 {
            return Err(
                "encodings must select binary64 for the first four wide-row scalars".into(),
            );
        }
        let raw = self
            .raw
            .as_slice()
            .try_into()
            .map_err(|_| "raw_values wide-row scalars must contain eight bytes")?;
        let scalar = ShiftedBinary64::from_wire(self.value, raw)
            .map_err(|error| format!("values/raw_values: {error}"))?;
        Ok(PatternValue {
            scalar,
            offset: self.source_offset,
        })
    }

    fn terminal(&self) -> Result<PatternValue<PatternTerminal, u64>, String> {
        let scalar = PatternTerminal::read(&self.raw)
            .ok_or("raw_values wide-row terminal must contain exact one or binary32")?;
        if self.encoding != scalar.encoding()
            || self.raw.len() != scalar.raw().len()
            || self.value.to_bits() != scalar.value().get().to_bits()
        {
            return Err(
                "encodings and values must match the exact raw_values terminal atom".into(),
            );
        }
        Ok(PatternValue {
            scalar,
            offset: self.source_offset,
        })
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
            return Err(
                "pattern transform columns must contain complete rows for the selected layout"
                    .into(),
            );
        }
        let values = wire
            .encodings
            .into_iter()
            .zip(wire.values)
            .zip(wire.raw_values)
            .zip(wire.value_source_offsets)
            .map(
                |(((encoding, value), raw), source_offset)| PatternScalarWire {
                    encoding,
                    value,
                    raw,
                    source_offset,
                },
            )
            .collect::<Vec<_>>();
        let selectors = wire
            .selectors
            .into_iter()
            .zip(wire.raw_selectors)
            .zip(wire.selector_source_offsets)
            .map(|((value, raw), offset)| {
                Ok(crate::om::compact::LocatedCompactIndex {
                    atom: crate::om::compact::CompactIndexAtom::from_wire(value, &raw)
                        .map_err(|error| format!("selectors/raw_selectors: {error}"))?,
                    offset,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let rows = match wire.layout {
            FeaturePatternTransformLayout::ScalarRows => {
                let rows = values
                    .as_chunks::<1>()
                    .0
                    .iter()
                    .zip(selectors)
                    .map(|([value], selector)| {
                        Ok(PatternRow {
                            values: value.shifted()?,
                            selector,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                PatternRows::Scalar(BranchItems::new(rows)?)
            }
            FeaturePatternTransformLayout::WideRows => {
                let rows = values
                    .as_chunks::<5>()
                    .0
                    .iter()
                    .zip(selectors)
                    .map(|([a, b, c, d, terminal], selector)| {
                        Ok(PatternRow {
                            values: PatternWideValues {
                                first: [a.binary64()?, b.binary64()?, c.binary64()?, d.binary64()?],
                                terminal: terminal.terminal()?,
                            },
                            selector,
                        })
                    })
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

/// Exact counted instance-output lane carried by a bounded operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureMultiInstanceOutputLaneWire")]
pub(in crate::native) struct FeatureMultiInstanceOutputLane {
    /// Globally unique output-lane identity.
    pub(in crate::native) id: String,
    /// Owning `Multi Instance Output` operation label.
    pub(in crate::native) operation_label: String,
    /// Complete selector groups and their trailing references.
    outputs: crate::om::instances::MultiInstanceOutputs<u64>,
    /// Absolute source offset of the opening `25 01, count` field.
    source_offset: u64,
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

#[cfg(test)]
impl From<FeatureMultiInstanceOutputLane> for FeatureMultiInstanceOutputLaneWire {
    fn from(lane: FeatureMultiInstanceOutputLane) -> Self {
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            declared_count: lane.outputs.selectors().len() + 1,
            instance_count: lane.outputs.references().len() + 1,
            source_offset: lane.source_offset,
            selectors: lane
                .outputs
                .selectors()
                .iter()
                .map(|token| token.atom.value())
                .collect(),
            raw_selectors: lane
                .outputs
                .selectors()
                .iter()
                .map(|token| token.atom.raw().to_vec())
                .collect(),
            ordinals: lane.outputs.ordinals().collect(),
            row_indices: (2..lane.outputs.selectors().len() + 2).collect(),
            selector_source_offsets: lane
                .outputs
                .selectors()
                .iter()
                .map(|token| token.offset)
                .collect(),
            trailing_object_indices: lane
                .outputs
                .references()
                .iter()
                .map(|token| token.token.value())
                .collect(),
            raw_trailing_object_indices: lane
                .outputs
                .references()
                .iter()
                .map(|token| token.token.raw().to_vec())
                .collect(),
            trailing_object_index_source_offsets: lane
                .outputs
                .references()
                .iter()
                .map(|token| token.offset)
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
        if wire
            .row_indices
            .iter()
            .copied()
            .ne(2..wire.selectors.len() + 2)
        {
            return Err("row_indices must enumerate rows from two".into());
        }
        if wire.instance_count != wire.trailing_object_indices.len() + 1 {
            return Err(
                "instance_count must equal the trailing reference count plus the implicit seed"
                    .into(),
            );
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
        let rows = wire
            .selectors
            .into_iter()
            .zip(wire.raw_selectors)
            .zip(wire.ordinals)
            .zip(wire.selector_source_offsets)
            .map(|(((value, raw), ordinal), offset)| {
                Ok((
                    crate::om::compact::LocatedCompactIndex {
                        atom: crate::om::compact::CompactIndexAtom::from_wire(value, &raw)
                            .map_err(|error| format!("selectors/raw_selectors: {error}"))?,
                        offset,
                    },
                    ordinal,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let references = wire
            .trailing_object_indices
            .into_iter()
            .zip(wire.raw_trailing_object_indices)
            .zip(wire.trailing_object_index_source_offsets)
            .map(|((value, raw), offset)| {
                Ok(crate::om::PayloadObjectReference {
                    token: crate::om::reference_index::FeatureReferenceToken::from_wire(
                        value, &raw,
                    )
                    .map_err(|error| format!("trailing_object_indices: {error}"))?,
                    offset,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            source_offset: wire.source_offset,
            outputs: crate::om::instances::MultiInstanceOutputs::new(rows, references)?,
        })
    }
}

/// Exact counted selector lane carried by an identical-instance output payload.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureIdenticalInstanceOutputLaneWire")]
pub(in crate::native) struct FeatureIdenticalInstanceOutputLane {
    /// Globally unique output-lane identity.
    pub(in crate::native) id: String,
    /// Owning `IDENTICAL INSTANCE OUTPUT` operation label.
    pub(in crate::native) operation_label: String,
    /// Schema index preceding the count field.
    leading_schema_index: u8,
    /// Schema index framing the serialized count.
    count_schema_index: crate::om::IdenticalInstanceSchemaIndex,
    /// Ordered complete source tokens.
    selectors:
        crate::om::compact::CountedIndexMembers<crate::om::compact::LocatedCompactIndex<u64>>,
    /// Absolute source offset of the leading schema index.
    source_offset: u64,
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

#[cfg(test)]
impl From<FeatureIdenticalInstanceOutputLane> for FeatureIdenticalInstanceOutputLaneWire {
    fn from(lane: FeatureIdenticalInstanceOutputLane) -> Self {
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            leading_schema_index: lane.leading_schema_index,
            count_schema_index: lane.count_schema_index.value(),
            row_schema_indices: lane.count_schema_index.row_indices(),
            declared_count: usize::from(lane.selectors.declared_count()) - 1,
            source_offset: lane.source_offset,
            selectors: lane
                .selectors
                .as_slice()
                .iter()
                .map(|token| token.atom.value())
                .collect(),
            raw_selectors: lane
                .selectors
                .as_slice()
                .iter()
                .map(|token| token.atom.raw().to_vec())
                .collect(),
            selector_source_offsets: lane
                .selectors
                .as_slice()
                .iter()
                .map(|token| token.offset)
                .collect(),
        }
    }
}

impl TryFrom<FeatureIdenticalInstanceOutputLaneWire> for FeatureIdenticalInstanceOutputLane {
    type Error = String;
    fn try_from(wire: FeatureIdenticalInstanceOutputLaneWire) -> Result<Self, Self::Error> {
        let count_schema_index =
            crate::om::IdenticalInstanceSchemaIndex::new(wire.count_schema_index)
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
            selectors: crate::om::compact::CountedIndexMembers::new(
                wire.selectors
                    .into_iter()
                    .zip(wire.raw_selectors)
                    .zip(wire.selector_source_offsets)
                    .map(|((value, raw), offset)| {
                        Ok(crate::om::compact::LocatedCompactIndex {
                            atom: crate::om::compact::CompactIndexAtom::from_wire(value, &raw)
                                .map_err(|error| format!("selectors/raw_selectors: {error}"))?,
                            offset,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            )
            .map_err(|error| format!("selectors: {error}"))?,
        })
    }
}

/// Decode and resolve exact ordered construction references in pattern
/// payloads without assigning seed or transform semantics to their slots.
pub(in crate::native) fn feature_pattern_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeaturePatternReference>, cadmpeg_core::CodecError> {
    let (indexed, _indexed_storage) = history.container().indexed_om_sections(ctx)?;
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
            let Some(decoded) = PatternReferences::read(ctx, record.payload_view())? else {
                continue;
            };
            let layout = decoded.layout();
            let mut decoded_references = decoded.into_references().into_iter();
            let ordinals = ctx.admit_iter(
                &(0..decoded_references.len()),
                "visit NX pattern references",
            )?;
            for ordinal in ordinals {
                let Some(reference) = decoded_references.next() else {
                    return Err(ctx.refuse_codec_limit("visit NX pattern references", 0, 1));
                };
                let ordinal_u32 = u32::try_from(ordinal)
                    .map_err(|_| ctx.refuse_codec_limit("NX pattern reference ordinal", 0, 1))?;
                let item = FeaturePatternReference {
                    id: format_feature_history_id(
                        ctx,
                        "pattern-reference",
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
                    layout,
                    ordinal: ordinal_u32,
                    token: reference.token,
                    data_block: charged_unique_offset_data_block(
                        ctx,
                        &indexed,
                        reference.token.value(),
                    )?,
                    source_offset: entry_offset
                        + cadmpeg_core::decode::u64_from_index(reference.offset),
                };
                ctx.reserve_vec(&mut references, 1, "NX pattern references")?;
                references.push(item);
            }
        }
    }
    Ok(references)
}

/// Decode and resolve the exact counted reference lane in `Pattern Feature`
/// payloads without assigning roles to its references.
pub(in crate::native) fn feature_pattern_counted_reference_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeaturePatternCountedReferenceLane>, cadmpeg_core::CodecError> {
    let (indexed, _indexed_storage) = history.container().indexed_om_sections(ctx)?;
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
            let (lane, _lane_storage) =
                match CountedPatternReferences::read(ctx, record.payload_view()) {
                    Ok((Some(lane), storage)) => (lane, storage),
                    Ok((None, _)) => continue,
                    Err(error) => {
                        return Err(error);
                    }
                };
            let references = match lane.resolve(ctx, entry_offset, |token| {
                charged_unique_offset_data_block(ctx, &indexed, token.value())
            }) {
                Ok(Some(references)) => references,
                Ok(None) => continue,
                Err(error) => {
                    return Err(error);
                }
            };
            let id = format_feature_history_id(
                ctx,
                "pattern-counted-reference-lane",
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
            ctx.reserve_vec(&mut lanes, 1, "NX counted pattern reference lanes")?;
            lanes.push(FeaturePatternCountedReferenceLane {
                id,
                operation_label,
                references,
            });
        }
    }
    Ok(lanes)
}

/// Reconstruct ordered logical payloads from complete pattern-reference graphs.
pub(in crate::native) fn feature_pattern_construction_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    labels: &[FeatureOperationLabel],
    references: &[FeaturePatternReference],
) -> Result<Vec<FeatureConstructionPayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut index_reservation = ctx.reserve_scoped(0, "NX pattern construction indexes")?;
    let mut kinds = BTreeMap::<&str, &str>::new();
    for label in ctx.admit_iter(labels, "index NX pattern construction labels")? {
        index_reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut kinds,
                label.id.as_str(),
                label.value.as_str(),
                "NX pattern construction labels",
            )
        })?;
    }
    let (groups, _groups_reservation) = ctx.collect_scoped_btree_groups(
        references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
        "group NX pattern construction references",
    )?;
    let mut output = Vec::new();
    for (operation_label, mut graph) in
        ctx.admit_iter(groups, "visit NX pattern construction operations")?
    {
        let Some(kind) = ctx.get_btree_map(
            &kinds,
            &operation_label,
            "find NX pattern construction kind",
        )?
        else {
            continue;
        };
        let operation_kind = match *kind {
            "Pattern Feature" => FeaturePatternKind::Feature,
            "Pattern Geometry" => FeaturePatternKind::Geometry,
            _ => continue,
        };
        if !matches!(graph.len(), 9 | 10) {
            continue;
        }
        ctx.stable_sort_by(
            &mut graph,
            |value| &value.ordinal,
            Ord::cmp,
            "sort NX pattern construction graph",
        )?;
        if graph
            .iter()
            .enumerate()
            .any(|(ordinal, reference)| u32::try_from(ordinal) != Ok(reference.ordinal))
            || graph
                .iter()
                .any(|reference| reference.layout != graph[0].layout)
        {
            continue;
        }
        let Some((data_blocks, _source_id_storage)) = copy_block_ids(
            ctx,
            graph
                .iter()
                .map(|reference| reference.data_block.as_deref()),
            "NX pattern construction block IDs",
        )?
        else {
            continue;
        };
        if shared_block_store(
            ctx,
            &data_blocks,
            "validate NX pattern construction block owners",
        )?
        .is_none()
        {
            continue;
        }
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };
        let Some(operation_key) = operation_key(
            ctx,
            operation_label,
            "find NX pattern construction operation key",
        )?
        else {
            continue;
        };
        let id = ctx.format_retained(
            format_args!("nx:feature-history:pattern-construction-payload#{operation_key}"),
            "NX pattern construction payload identity",
        )?;
        let mut construction_references = Vec::new();
        for reference in graph.iter().copied() {
            ctx.reserve_vec(
                &mut construction_references,
                1,
                "NX pattern construction reference IDs",
            )?;
            construction_references.push(
                ctx.copy_retained_text(&reference.id, "NX pattern construction reference ID")?,
            );
        }
        let record = FeatureConstructionPayload {
            id,
            operation_label: ctx
                .copy_retained_text(operation_label, "NX pattern construction operation label")?,
            owner: FeatureConstructionOwner::Pattern {
                operation_kind,
                reference_layout: graph[0].layout,
                construction_references,
            },
            content,
        };
        ctx.reserve_vec(&mut output, 1, "NX pattern construction payloads")?;
        output.push(record);
    }
    Ok(output)
}

/// Decode canonical printable strings from reconstructed pattern payloads.
pub(in crate::native) fn feature_pattern_construction_strings(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeaturePatternConstructionString>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut strings = Vec::new();
    for payload in ctx.admit_iter(payloads, "scan NX pattern string payloads")? {
        let Some(joined) = JoinedPayload::from_source(
            ctx,
            payload.content.block_ids(),
            payload.content.blocks().len(),
            &blocks,
        )?
        else {
            continue;
        };
        for (ordinal, value) in ctx
            .admit_iter(
                crate::om::string_values(ctx, joined.bytes(), 0)?,
                "visit NX pattern payload strings",
            )?
            .enumerate()
        {
            let payload_offset = cadmpeg_core::decode::u64_from_index(value.offset);
            let Some(source_offset) = joined.source_offset(ctx, payload_offset)? else {
                continue;
            };
            let ordinal_u32 = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX pattern string ordinal", 0, 1))?;
            let id = format_feature_child_id(ctx, &payload.id, "-string-", ordinal)?;
            let value = ctx
                .copy_retained_text(value.value.as_str(), "NX pattern construction string value")?;
            let value = PrintableString::new(value).map_err(cadmpeg_core::CodecError::malformed)?;
            let operation_label = ctx.copy_retained_text(
                &payload.operation_label,
                "NX pattern construction string label",
            )?;
            let construction_payload =
                ctx.copy_retained_text(&payload.id, "NX pattern construction string payload")?;
            ctx.reserve_vec(&mut strings, 1, "NX pattern construction strings")?;
            strings.push(FeaturePatternConstructionString {
                id,
                operation_label,
                construction_payload,
                ordinal: ordinal_u32,
                value,
                payload_offset,
                source_offset,
            });
        }
    }
    Ok(strings)
}

/// Decode complete signed Q1.55 lanes from reconstructed pattern payloads.
pub(in crate::native) fn feature_pattern_construction_fixed_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    payloads: &[FeatureConstructionPayload],
) -> Result<Vec<FeaturePatternConstructionFixedLane>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut lanes = Vec::new();
    for payload in ctx.admit_iter(payloads, "scan NX pattern fixed-lane payloads")? {
        let Some(joined) = JoinedPayload::from_source(
            ctx,
            payload.content.block_ids(),
            payload.content.blocks().len(),
            &blocks,
        )?
        else {
            continue;
        };
        for (ordinal, lane) in ctx
            .admit_iter(
                crate::om::draft_construction_fixed_lanes(ctx, joined.bytes())?,
                "visit NX pattern fixed lanes",
            )?
            .enumerate()
        {
            let payload_offset = lane.offset();
            let Some(lane) =
                lane.try_map_locations(ctx, |offset, ()| joined.source_offset(ctx, offset))?
            else {
                continue;
            };
            let Some(source_offset) = joined.source_offset(ctx, payload_offset)? else {
                continue;
            };
            let ordinal_u32 = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX pattern fixed lane ordinal", 0, 1))?;
            let id = format_feature_child_id(ctx, &payload.id, "-fixed-lane-", ordinal)?;
            let operation_label =
                ctx.copy_retained_text(&payload.operation_label, "NX pattern fixed lane label")?;
            let construction_payload =
                ctx.copy_retained_text(&payload.id, "NX pattern fixed lane payload")?;
            ctx.reserve_vec(&mut lanes, 1, "NX pattern construction fixed lanes")?;
            lanes.push(FeaturePatternConstructionFixedLane {
                id,
                operation_label,
                construction_payload,
                ordinal: ordinal_u32,
                lane,
                source_offset,
            });
        }
    }
    Ok(lanes)
}

/// Decode exact counted transform lanes from bounded pattern payloads.
pub(in crate::native) fn feature_pattern_transform_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeaturePatternTransformLane>, cadmpeg_core::CodecError> {
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
            let Some(lane) = crate::om::pattern_payload_transform_lane(ctx, record.payload_view())?
            else {
                continue;
            };
            let rows = lane.rows.map_charged(
                ctx,
                |selector| crate::om::compact::LocatedCompactIndex {
                    atom: selector.atom,
                    offset: entry_offset + cadmpeg_core::decode::u64_from_index(selector.offset),
                },
                |offset| entry_offset + cadmpeg_core::decode::u64_from_index(offset),
            )?;
            let id = format_feature_history_id(
                ctx,
                "pattern-transform-lane",
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
            ctx.reserve_vec(&mut lanes, 1, "NX pattern transform lanes")?;
            lanes.push(FeaturePatternTransformLane {
                id,
                operation_label,
                row_schema_index: lane.row_schema_index,
                rows,
                source_offset: entry_offset + cadmpeg_core::decode::u64_from_index(lane.offset),
            });
        }
    }
    Ok(lanes)
}

/// Decode exact counted output lanes from bounded multi-instance payloads.
pub(in crate::native) fn feature_multi_instance_output_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureMultiInstanceOutputLane>, cadmpeg_core::CodecError> {
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
            let lane =
                match crate::om::multi_instance_output_payload_lane(ctx, record.payload_view()) {
                    Ok(Some(lane)) => lane,
                    Ok(None) => continue,
                    Err(error) => {
                        return Err(error);
                    }
                };
            let outputs = lane.outputs.map_offsets(ctx, |offset| {
                entry_offset + cadmpeg_core::decode::u64_from_index(offset)
            })?;
            let id = format_feature_history_id(
                ctx,
                "multi-instance-output-lane",
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
            ctx.reserve_vec(&mut lanes, 1, "NX multi-instance output lanes")?;
            lanes.push(FeatureMultiInstanceOutputLane {
                id,
                operation_label,
                outputs,
                source_offset: entry_offset + cadmpeg_core::decode::u64_from_index(lane.offset),
            });
        }
    }
    Ok(lanes)
}

/// Decode exact counted selector lanes from bounded identical-instance output
/// payloads.
pub(in crate::native) fn feature_identical_instance_output_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &FeatureHistory<'_, '_, '_>,
) -> Result<Vec<FeatureIdenticalInstanceOutputLane>, cadmpeg_core::CodecError> {
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
            let lane =
                crate::om::identical_instance_output_payload_lane(ctx, record.payload_view())?;
            let Some(lane) = lane else {
                continue;
            };
            let selectors = lane.selectors.map_charged(ctx, |token| {
                Ok(crate::om::compact::LocatedCompactIndex {
                    atom: token.atom,
                    offset: entry_offset + cadmpeg_core::decode::u64_from_index(token.offset),
                })
            })?;
            let id = format_feature_history_id(
                ctx,
                "identical-instance-output-lane",
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
            ctx.reserve_vec(&mut lanes, 1, "NX identical-instance output lanes")?;
            lanes.push(FeatureIdenticalInstanceOutputLane {
                id,
                operation_label,
                leading_schema_index: lane.leading_schema_index,
                count_schema_index: lane.count_schema_index,
                selectors,
                source_offset: entry_offset + cadmpeg_core::decode::u64_from_index(lane.offset),
            });
        }
    }
    Ok(lanes)
}

use super::deserialize_reference_lane_count;

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_data_block, String, "data_block");
