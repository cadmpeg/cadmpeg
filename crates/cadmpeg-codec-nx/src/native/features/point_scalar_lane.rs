// SPDX-License-Identifier: Apache-2.0
//! Cross-block point scalar lane with derived physical positions.

use crate::om::scalar::ShiftedBinary64;
use serde::{Deserialize, Serialize};

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
    pub scalars: [ShiftedBinary64; 6],
    pub positions: PointScalarPositions,
}

/// Physical positions of the preceding block tail and the target block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointScalarPositions {
    first_source_offset: u64,
    target_source_offset: u64,
}

impl PointScalarPositions {
    pub fn new(first_source_offset: u64, target_source_offset: u64) -> Result<Self, String> {
        first_source_offset
            .checked_add(3)
            .ok_or("source_offsets[0]: span overflow")?;
        target_source_offset
            .checked_add(64)
            .ok_or("source_offsets: target span overflow")?;
        Ok(Self {
            first_source_offset,
            target_source_offset,
        })
    }

    fn source_offsets(self) -> [u64; 6] {
        std::array::from_fn(|slot| {
            if slot == 0 {
                self.first_source_offset
            } else {
                self.target_source_offset + 5 + (slot as u64 - 1) * 8
            }
        })
    }
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
        let source_offsets = lane.positions.source_offsets();
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            construction_header: lane.construction_header,
            data_blocks: lane.data_blocks,
            values: lane.scalars.map(|scalar| scalar.value()),
            raw_values: lane.scalars.map(|scalar| scalar.raw()),
            source_offsets,
        }
    }
}

impl TryFrom<FeaturePointConstructionScalarLaneWire> for FeaturePointConstructionScalarLane {
    type Error = String;

    // Names follow the ordered source slots in this fixed-width lane.
    #[allow(clippy::many_single_char_names)]
    fn try_from(wire: FeaturePointConstructionScalarLaneWire) -> Result<Self, Self::Error> {
        let [a, b, c, d, e, f] = std::array::from_fn::<_, 6, _>(|i| {
            ShiftedBinary64::from_wire(wire.values[i], wire.raw_values[i])
                .map_err(|error| format!("values/raw_values[{i}]: {error}"))
        });
        let scalars = [a?, b?, c?, d?, e?, f?];
        let target_source_offset = wire.source_offsets[1]
            .checked_sub(5)
            .ok_or("source_offsets[1]: target prefix underflow")?;
        let positions = PointScalarPositions::new(wire.source_offsets[0], target_source_offset)?;
        let lane = Self {
            id: wire.id,
            operation_label: wire.operation_label,
            construction_header: wire.construction_header,
            data_blocks: wire.data_blocks,
            scalars,
            positions,
        };
        if lane.positions.source_offsets() != wire.source_offsets {
            return Err("source_offsets: inconsistent point scalar lane positions".into());
        }
        Ok(lane)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_scalar_lane_requires_derived_positions_and_complete_physical_spans() {
        let wire: serde_json::Value = serde_json::from_str(r#"{"id":"lane","operation_label":"operation","construction_header":"header","data_blocks":["first","second"],"values":[1.0,2.0,3.0,4.0,5.0,6.0],"raw_values":[[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0],[48,8,0,0,0,0,0,0],[48,16,0,0,0,0,0,0],[48,20,0,0,0,0,0,0],[48,24,0,0,0,0,0,0]],"source_offsets":[100,110,118,126,134,142]}"#).unwrap();
        for slot in 2..6 {
            let mut invalid = wire.clone();
            invalid["source_offsets"][slot] =
                serde_json::json!(wire["source_offsets"][slot].as_u64().unwrap() + 1);
            let error =
                serde_json::from_value::<FeaturePointConstructionScalarLane>(invalid).unwrap_err();
            assert!(error.to_string().contains("source_offsets"));
        }
        for offsets in [
            [u64::MAX - 2, 110, 118, 126, 134, 142],
            [100, 4, 12, 20, 28, 36],
            [
                100,
                u64::MAX - 58,
                u64::MAX - 50,
                u64::MAX - 42,
                u64::MAX - 34,
                u64::MAX - 26,
            ],
        ] {
            let mut invalid = wire.clone();
            invalid["source_offsets"] = serde_json::json!(offsets);
            let error =
                serde_json::from_value::<FeaturePointConstructionScalarLane>(invalid).unwrap_err();
            assert!(error.to_string().contains("source_offsets"));
        }
        let mut boundary = wire;
        boundary["source_offsets"] = serde_json::json!([
            u64::MAX - 3,
            u64::MAX - 59,
            u64::MAX - 51,
            u64::MAX - 43,
            u64::MAX - 35,
            u64::MAX - 27
        ]);
        let lane: FeaturePointConstructionScalarLane =
            serde_json::from_value(boundary.clone()).unwrap();
        assert_eq!(serde_json::to_value(lane).unwrap(), boundary);
    }
}
