// SPDX-License-Identifier: Apache-2.0
//! Pattern construction records and extraction.

use crate::om::branch_items::BranchItems;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

use super::reference::ConstructionReference;
use crate::container::Container;

use super::joined_payload::JoinedPayload;
use super::payload_content::FeaturePayloadContent;
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
use crate::om::reference_index::ReferenceIndexToken;
use crate::om::scalar::ShiftedBinary64;
use crate::om::scalar::ShiftedScalar;
use crate::printable_string::PrintableString;
use serde::Serialize;
use std::num::NonZeroU8;

use super::offset_data_block_bytes;

use super::unique_offset_data_block;
use super::visit_feature_history_operation_records;

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

/// Exact counted reference lane carried by a bounded `Pattern Feature` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeaturePatternCountedReferenceLaneWire",
    into = "FeaturePatternCountedReferenceLaneWire"
)]
pub struct FeaturePatternCountedReferenceLane {
    pub id: String,
    pub operation_label: String,
    pub references: Vec<ConstructionReference<Option<String>>>,
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
                .map(|reference| reference.token.value())
                .collect(),
            raw_object_indices: value
                .references
                .iter()
                .map(|reference| reference.token.raw().to_vec())
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
                .map(|(((value, raw), data_block), source_offset)| {
                    Ok(ConstructionReference {
                        token: ReferenceIndexToken::from_wire(value, &raw).map_err(|error| {
                            format!("object_indices/raw_object_indices: {error}")
                        })?,
                        data_block,
                        source_offset,
                    })
                })
                .collect::<Result<_, String>>()?,
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
    pub rows: PatternRows<crate::om::compact::LocatedCompactIndex<u64>, u64>,
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
                        row.values.scalar.value(),
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
                            value.scalar.value(),
                            value.scalar.as_bytes(),
                            value.offset,
                        );
                    }
                    let terminal = row.values.terminal;
                    wire.push_scalar(
                        terminal.scalar.encoding(),
                        terminal.scalar.value(),
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
            || self.value.to_bits() != scalar.value().to_bits()
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
            || self.value.to_bits() != scalar.value().to_bits()
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
    /// Complete selector groups and their trailing references.
    pub outputs: crate::om::instances::MultiInstanceOutputs<u64>,
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
    pub selectors:
        crate::om::compact::CountedIndexMembers<crate::om::compact::LocatedCompactIndex<u64>>,
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
pub fn feature_pattern_references(container: &Container) -> Vec<FeaturePatternReference> {
    let indexed = container.indexed_om_sections();
    let mut references = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(decoded) = crate::om::pattern_payload_references(record.payload_view()) else {
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
            let Some(lane) =
                crate::om::pattern_payload_counted_reference_lane(record.payload_view())
            else {
                return;
            };
            lanes.push(FeaturePatternCountedReferenceLane {
                id: format!(
                    "nx:feature-history:pattern-counted-reference-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                references: lane.references.into_iter().map(|reference| ConstructionReference {
                    token: reference.token,
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
            let Some(joined) = JoinedPayload::from_source(payload.content.block_ids(), &blocks)
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
            let Some(lane) = crate::om::pattern_payload_transform_lane(record.payload_view())
            else {
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
                    |selector| crate::om::compact::LocatedCompactIndex {
                        atom: selector.atom,
                        offset: entry_offset + selector.offset as u64,
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
            let Some(lane) = crate::om::multi_instance_output_payload_lane(record.payload_view())
            else {
                return;
            };
            lanes.push(FeatureMultiInstanceOutputLane {
                id: format!(
                    "nx:feature-history:multi-instance-output-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                outputs: lane.outputs.map_offsets(|offset| entry_offset + offset as u64),
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
            let Some(lane) =
                crate::om::identical_instance_output_payload_lane(record.payload_view())
            else {
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
                selectors: lane.selectors.map(|token| crate::om::compact::LocatedCompactIndex {
                    atom: token.atom,
                    offset: entry_offset + token.offset as u64,
                }),
                source_offset: entry_offset + lane.offset as u64,
            });
        },
    );
    lanes
}

use super::deserialize_reference_lane_count;

#[cfg(test)]
mod tests;
