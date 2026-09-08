// SPDX-License-Identifier: Apache-2.0
//! Fixed block-construction reference positions and native records.

use serde::{Deserialize, Serialize};

/// Ordered construction reference carried by a bounded `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureBlockConstructionReference {
    /// Globally unique construction-reference identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Payload control byte preceding the construction field.
    pub control: u8,
    /// Checked position in the fixed field; terminal status is derived.
    #[serde(flatten)]
    pub position: BlockReferencePosition,
    /// Checked index retaining the exact serialized token.
    #[serde(flatten)]
    pub token: crate::om::reference_index::PayloadIndexToken,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Position in the eighteen-member field followed by its terminal reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PositionWire", into = "PositionWire")]
pub(crate) struct BlockReferencePosition(u8);

impl BlockReferencePosition {
    pub(crate) fn new(ordinal: u32) -> Result<Self, &'static str> {
        if ordinal > 18 {
            return Err("ordinal: block reference position must be within 0..=18");
        }
        Ok(Self(ordinal as u8))
    }
    pub(crate) fn ordinal(self) -> u32 {
        u32::from(self.0)
    }
    fn terminal(self) -> bool {
        self.0 == 18
    }
    pub(crate) fn enumerate<T>(references: [T; 19]) -> impl Iterator<Item = (Self, T)> {
        references
            .into_iter()
            .enumerate()
            .map(|(ordinal, reference)| (Self(ordinal as u8), reference))
    }
}

#[derive(Serialize, Deserialize)]
struct PositionWire {
    ordinal: u32,
    terminal: bool,
}

impl From<BlockReferencePosition> for PositionWire {
    fn from(position: BlockReferencePosition) -> Self {
        Self {
            ordinal: position.ordinal(),
            terminal: position.terminal(),
        }
    }
}

impl TryFrom<PositionWire> for BlockReferencePosition {
    type Error = &'static str;
    fn try_from(wire: PositionWire) -> Result<Self, Self::Error> {
        let position = Self::new(wire.ordinal)?;
        if wire.terminal != position.terminal() {
            return Err("terminal: only ordinal 18 is the terminal block reference");
        }
        Ok(position)
    }
}

#[cfg(test)]
mod tests {
    // Wire fixtures use checked positions; failure assertions exercise deserialization.
    #![allow(clippy::unwrap_used)]
    use super::{BlockReferencePosition, FeatureBlockConstructionReference};

    #[test]
    fn block_positions_follow_the_fixed_field_and_preserve_native_wire() {
        for (ordinal, (position, value)) in BlockReferencePosition::enumerate([42; 19]).enumerate()
        {
            assert_eq!(value, 42);
            assert_eq!(position.ordinal(), ordinal as u32);
            assert_eq!(position.terminal(), ordinal == 18);
            assert_eq!(
                BlockReferencePosition::new(ordinal as u32).unwrap(),
                position
            );
            let terminal = ordinal == 18;
            for target in ["", r#","data_block":"block""#] {
                let wire = format!(
                    r#"{{"id":"reference","operation_label":"block-op","control":38,"ordinal":{ordinal},"terminal":{terminal},"object_index":66,"raw_object_index":[240,66]{target},"source_offset":100}}"#
                );
                let reference: FeatureBlockConstructionReference =
                    serde_json::from_str(&wire).unwrap();
                assert_eq!(serde_json::to_string(&reference).unwrap(), wire);
                let mut invalid = serde_json::to_value(reference).unwrap();
                invalid["terminal"] = serde_json::json!(!terminal);
                assert!(
                    serde_json::from_value::<FeatureBlockConstructionReference>(invalid)
                        .unwrap_err()
                        .to_string()
                        .contains("terminal")
                );
            }
        }
        let invalid = r#"{"id":"reference","operation_label":"block-op","control":38,"ordinal":0,"terminal":false,"object_index":66,"raw_object_index":[66],"source_offset":100}"#;
        assert!(serde_json::from_str::<FeatureBlockConstructionReference>(invalid).is_err());
        for ordinal in [19, u32::MAX] {
            let wire = serde_json::json!({"ordinal":ordinal,"terminal":false});
            assert!(serde_json::from_value::<BlockReferencePosition>(wire)
                .unwrap_err()
                .to_string()
                .contains("ordinal"));
        }
    }
}
