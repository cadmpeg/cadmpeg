// SPDX-License-Identifier: Apache-2.0
//! Native terminal discriminator wire adapter.

use crate::iter_wire::IterWire;
use crate::om::compact::{CompactIndexAtom, RawCompactIndex};
use crate::om::terminal_discriminator::OperationTerminalDiscriminator;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureOperationTerminalDiscriminatorWire")]
pub(in crate::native) struct FeatureOperationTerminalDiscriminator {
    pub(in crate::native) id: String,
    pub(in crate::native) operation_label: String,
    pub(super) frame: OperationTerminalDiscriminator,
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

impl Serialize for FeatureOperationTerminalDiscriminator {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry(
            "type_indices",
            &self.frame.type_indices().map(|(token, _)| token.value()),
        )?;
        wire.serialize_entry(
            "raw_type_indices",
            &self
                .frame
                .type_indices()
                .map(|(token, _)| RawCompactIndex(token)),
        )?;
        wire.serialize_entry(
            "type_index_source_offsets",
            &self.frame.type_indices().map(|(_, offset)| offset),
        )?;
        wire.serialize_entry("flags", &self.frame.flags())?;
        wire.serialize_entry(
            "trailing_indices",
            &IterWire(
                self.frame
                    .trailing_indices()
                    .map(|(token, _)| token.value()),
            ),
        )?;
        wire.serialize_entry(
            "raw_trailing_indices",
            &IterWire(
                self.frame
                    .trailing_indices()
                    .map(|(token, _)| RawCompactIndex(token)),
            ),
        )?;
        wire.serialize_entry(
            "trailing_index_source_offsets",
            &IterWire(self.frame.trailing_indices().map(|(_, offset)| offset)),
        )?;
        wire.serialize_entry("source_offset", &self.frame.origin())?;
        wire.end()
    }
}

#[cfg(test)]
impl From<FeatureOperationTerminalDiscriminator> for FeatureOperationTerminalDiscriminatorWire {
    fn from(lane: FeatureOperationTerminalDiscriminator) -> Self {
        Self {
            id: lane.id,
            operation_label: lane.operation_label,
            type_indices: lane.frame.type_indices().map(|(token, _)| token.value()),
            raw_type_indices: lane
                .frame
                .type_indices()
                .map(|(token, _)| token.raw().to_vec()),
            type_index_source_offsets: lane.frame.type_indices().map(|(_, offset)| offset),
            flags: lane.frame.flags(),
            trailing_indices: lane
                .frame
                .trailing_indices()
                .map(|(token, _)| token.value())
                .collect(),
            raw_trailing_indices: lane
                .frame
                .trailing_indices()
                .map(|(token, _)| token.raw().to_vec())
                .collect(),
            trailing_index_source_offsets: lane
                .frame
                .trailing_indices()
                .map(|(_, offset)| offset)
                .collect(),
            source_offset: lane.frame.origin(),
        }
    }
}

impl TryFrom<FeatureOperationTerminalDiscriminatorWire> for FeatureOperationTerminalDiscriminator {
    type Error = String;
    fn try_from(wire: FeatureOperationTerminalDiscriminatorWire) -> Result<Self, Self::Error> {
        if wire.trailing_indices.len() != wire.raw_trailing_indices.len()
            || wire.trailing_indices.len() != wire.trailing_index_source_offsets.len()
        {
            return Err("trailing_indices/raw_trailing_indices/trailing_index_source_offsets: column lengths differ".into());
        }
        let [first, second] = std::array::from_fn::<_, 2, _>(|slot| {
            CompactIndexAtom::from_wire(wire.type_indices[slot], &wire.raw_type_indices[slot])
                .map_err(|error| format!("type_indices[{slot}]: {error}"))
        });
        let trailing = wire
            .trailing_indices
            .into_iter()
            .zip(wire.raw_trailing_indices)
            .enumerate()
            .map(|(slot, (value, raw))| {
                CompactIndexAtom::from_wire(value, &raw)
                    .map_err(|error| format!("trailing_indices[{slot}]: {error}"))
            })
            .collect::<Result<_, _>>()?;
        let frame = OperationTerminalDiscriminator::new(
            wire.source_offset,
            [first?, second?],
            wire.flags,
            trailing,
        )?;
        if frame.type_indices().map(|(_, offset)| offset) != wire.type_index_source_offsets {
            return Err(
                "type_index_source_offsets: disagrees with terminal discriminator frame".into(),
            );
        }
        if !frame
            .trailing_indices()
            .map(|(_, offset)| offset)
            .eq(wire.trailing_index_source_offsets)
        {
            return Err(
                "trailing_index_source_offsets: disagrees with terminal discriminator frame".into(),
            );
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            frame,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FeatureOperationTerminalDiscriminator, FeatureOperationTerminalDiscriminatorWire};

    #[test]
    fn terminal_discriminator_borrowed_wire_matches_owned_bytes_and_retained_limit() {
        let json = r#"{"id":"nx:feature:terminal-discriminator#0","operation_label":"o","type_indices":[0,0],"raw_type_indices":[[0],[128,0]],"type_index_source_offsets":[103,104],"flags":[0,255,128,1],"trailing_indices":[],"raw_trailing_indices":[],"trailing_index_source_offsets":[],"source_offset":100}"#;
        let record: FeatureOperationTerminalDiscriminator = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&FeatureOperationTerminalDiscriminatorWire::from(
                record.clone()
            ))
            .unwrap()
        );
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }

    #[test]
    fn terminal_wire_derives_positions_and_keeps_empty_trailing_arrays() {
        let wire = r#"{"id":"t","operation_label":"o","type_indices":[0,0],"raw_type_indices":[[0],[128,0]],"type_index_source_offsets":[103,104],"flags":[0,255,128,1],"trailing_indices":[],"raw_trailing_indices":[],"trailing_index_source_offsets":[],"source_offset":100}"#;
        let parsed: FeatureOperationTerminalDiscriminator = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&parsed).unwrap(), wire);
        let mut populated: serde_json::Value = serde_json::from_str(wire).unwrap();
        populated["trailing_indices"] = serde_json::json!([0, 4096]);
        populated["raw_trailing_indices"] = serde_json::json!([[0], [144, 0]]);
        populated["trailing_index_source_offsets"] = serde_json::json!([119, 120]);
        let parsed: FeatureOperationTerminalDiscriminator =
            serde_json::from_value(populated.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), populated);
        for field in ["type_index_source_offsets", "trailing_index_source_offsets"] {
            for slot in 0..2 {
                let mut invalid = populated.clone();
                invalid[field][slot] = serde_json::json!(1000);
                assert!(
                    serde_json::from_value::<FeatureOperationTerminalDiscriminator>(invalid)
                        .unwrap_err()
                        .to_string()
                        .contains(field)
                );
            }
        }
        populated["source_offset"] = serde_json::json!(u64::MAX - 22);
        assert!(
            serde_json::from_value::<FeatureOperationTerminalDiscriminator>(populated)
                .unwrap_err()
                .to_string()
                .contains("source_offset")
        );
    }
}
