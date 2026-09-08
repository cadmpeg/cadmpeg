// SPDX-License-Identifier: Apache-2.0
//! Resolved extrusion construction from a structured branch.

use super::FeatureConstructionMember;
use crate::om::branch_items::BranchItems;
use serde::{Deserialize, Serialize};

/// Complete alternate extrusion construction using the structured `32` branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ConstructionWire", into = "ConstructionWire")]
pub(crate) struct FeatureExtrude32Construction {
    /// Globally unique construction identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Structured branch supplying the body-anchored construction lanes.
    pub branch: String,
    /// Body object index witnessed at both ends of the structured branch.
    pub body_object_index: u32,
    /// Nonempty ordered profile references paired with resolved targets.
    pub profiles: BranchItems<FeatureConstructionMember>,
    /// Ordered uniquely resolved blocks from the fixed-atom lane.
    pub atom_data_blocks: BranchItems<String>,
    /// Ordered uniquely resolved blocks from the first compact-index lane.
    pub first_data_blocks: BranchItems<String>,
    /// Ordered uniquely resolved blocks from the second compact-index lane.
    pub second_data_blocks: BranchItems<String>,
}

#[derive(Serialize, Deserialize)]
struct ConstructionWire {
    id: String,
    operation_label: String,
    branch: String,
    body_object_index: u32,
    profile_references: Vec<String>,
    profile_data_blocks: Vec<String>,
    atom_data_blocks: Vec<String>,
    first_data_blocks: Vec<String>,
    second_data_blocks: Vec<String>,
}

impl From<FeatureExtrude32Construction> for ConstructionWire {
    fn from(value: FeatureExtrude32Construction) -> Self {
        let (profile_references, profile_data_blocks) = value
            .profiles
            .into_vec()
            .into_iter()
            .map(|member| (member.reference, member.data_block))
            .unzip();
        Self {
            id: value.id,
            operation_label: value.operation_label,
            branch: value.branch,
            body_object_index: value.body_object_index,
            profile_references,
            profile_data_blocks,
            atom_data_blocks: value.atom_data_blocks.into_vec(),
            first_data_blocks: value.first_data_blocks.into_vec(),
            second_data_blocks: value.second_data_blocks.into_vec(),
        }
    }
}

impl TryFrom<ConstructionWire> for FeatureExtrude32Construction {
    type Error = &'static str;
    fn try_from(wire: ConstructionWire) -> Result<Self, Self::Error> {
        if wire.profile_references.len() != wire.profile_data_blocks.len() {
            return Err("profile_references/profile_data_blocks: column lengths differ");
        }
        let profiles = BranchItems::new(
            wire.profile_references
                .into_iter()
                .zip(wire.profile_data_blocks)
                .map(|(reference, data_block)| FeatureConstructionMember {
                    reference,
                    data_block,
                })
                .collect(),
        )
        .map_err(|_| "profile_references/profile_data_blocks: expected 1 through 254 profiles")?;
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            branch: wire.branch,
            body_object_index: wire.body_object_index,
            profiles,
            atom_data_blocks: BranchItems::new(wire.atom_data_blocks)
                .map_err(|_| "atom_data_blocks: expected 1 through 254 blocks")?,
            first_data_blocks: BranchItems::new(wire.first_data_blocks)
                .map_err(|_| "first_data_blocks: expected 1 through 254 blocks")?,
            second_data_blocks: BranchItems::new(wire.second_data_blocks)
                .map_err(|_| "second_data_blocks: expected 1 through 254 blocks")?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureExtrudePayload32BranchWire",
    into = "FeatureExtrudePayload32BranchWire"
)]
pub(crate) struct FeatureExtrudePayload32Branch {
    pub id: String,
    pub operation_label: String,
    pub frame: crate::om::extrude_32::Extrude32Frame<Option<String>>,
}

#[derive(Serialize, Deserialize)]
struct FeatureExtrudePayload32BranchWire {
    /// Globally unique branch identity.
    id: String,
    /// Owning `EXTRUDE` operation label.
    operation_label: String,
    /// Body object index anchoring the branch.
    body_object_index: u32,
    /// Finite shifted-IEEE scalar following the branch marker.
    scalar: f64,
    /// Exact shifted-binary64 scalar encoding.
    raw_scalar: [u8; 8],
    /// Ordered fixed-width big-endian atoms in the first counted lane.
    atoms_be: Vec<u32>,
    /// Absolute source offsets of the fixed-width atoms in lane order.
    atom_source_offsets: Vec<u64>,
    /// Compact indices wrapped by the fixed-width atoms.
    atom_indices: Vec<u32>,
    /// Unique offset-only data blocks addressed by the atom indices.
    atom_data_blocks: Vec<Option<String>>,
    /// Ordered values in the first compact-index lane.
    first_indices: Vec<u32>,
    /// Exact compact-index tokens in the first lane.
    raw_first_indices: Vec<Vec<u8>>,
    /// Absolute source offsets of the first-lane tokens.
    first_index_source_offsets: Vec<u64>,
    /// Unique offset-only data blocks addressed by the first lane.
    first_data_blocks: Vec<Option<String>>,
    /// Ordered values in the second compact-index lane.
    second_indices: Vec<u32>,
    /// Exact compact-index tokens in the second lane.
    raw_second_indices: Vec<Vec<u8>>,
    /// Absolute source offsets of the second-lane tokens.
    second_index_source_offsets: Vec<u64>,
    /// Unique offset-only data blocks addressed by the second lane.
    second_data_blocks: Vec<Option<String>>,
    /// Object index in the terminal field.
    terminal_object_index: u32,
    /// Exact serialized terminal object-index token.
    raw_terminal_object_index: Vec<u8>,
    /// Absolute file offset of the terminal object-index token.
    terminal_source_offset: u64,
    /// Absolute file offset of the `32` branch marker.
    source_offset: u64,
}

impl From<FeatureExtrudePayload32Branch> for FeatureExtrudePayload32BranchWire {
    fn from(branch: FeatureExtrudePayload32Branch) -> Self {
        Self {
            id: branch.id,
            operation_label: branch.operation_label,
            body_object_index: branch.frame.terminal().value(),
            scalar: branch.frame.scalar().value(),
            raw_scalar: branch.frame.scalar().raw(),
            atoms_be: branch
                .frame
                .atoms()
                .map(|(token, _, _)| token.raw())
                .collect(),
            atom_source_offsets: branch.frame.atoms().map(|(_, _, offset)| offset).collect(),
            atom_indices: branch
                .frame
                .atoms()
                .map(|(token, _, _)| token.value())
                .collect(),
            atom_data_blocks: branch
                .frame
                .atoms()
                .map(|(_, binding, _)| binding.clone())
                .collect(),
            first_indices: branch
                .frame
                .first_indices()
                .map(|(token, _, _)| token.value())
                .collect(),
            raw_first_indices: branch
                .frame
                .first_indices()
                .map(|(token, _, _)| token.raw().to_vec())
                .collect(),
            first_index_source_offsets: branch
                .frame
                .first_indices()
                .map(|(_, _, offset)| offset)
                .collect(),
            first_data_blocks: branch
                .frame
                .first_indices()
                .map(|(_, binding, _)| binding.clone())
                .collect(),
            second_indices: branch
                .frame
                .second_indices()
                .map(|(token, _, _)| token.value())
                .collect(),
            raw_second_indices: branch
                .frame
                .second_indices()
                .map(|(token, _, _)| token.raw().to_vec())
                .collect(),
            second_index_source_offsets: branch
                .frame
                .second_indices()
                .map(|(_, _, offset)| offset)
                .collect(),
            second_data_blocks: branch
                .frame
                .second_indices()
                .map(|(_, binding, _)| binding.clone())
                .collect(),
            terminal_object_index: branch.frame.terminal().value(),
            raw_terminal_object_index: branch.frame.terminal().raw().to_vec(),
            terminal_source_offset: branch.frame.terminal_offset(),
            source_offset: branch.frame.origin(),
        }
    }
}

impl TryFrom<FeatureExtrudePayload32BranchWire> for FeatureExtrudePayload32Branch {
    type Error = String;
    fn try_from(wire: FeatureExtrudePayload32BranchWire) -> Result<Self, Self::Error> {
        use crate::om::compact::{CompactIndexAtom, WrappedCompactIndex};
        use crate::om::extrude_32::Extrude32Frame;
        use crate::om::reference_index::FeatureReferenceToken;
        use crate::om::scalar::ShiftedBinary64;
        if wire.atom_indices.len() != wire.atoms_be.len()
            || wire.atom_indices.len() != wire.atom_source_offsets.len()
            || wire.atom_indices.len() != wire.atom_data_blocks.len()
        {
            return Err("extrusion atom_indices/atoms_be/atom_source_offsets/atom_data_blocks column lengths differ".into());
        }
        if wire.first_indices.len() != wire.raw_first_indices.len()
            || wire.first_indices.len() != wire.first_index_source_offsets.len()
            || wire.first_indices.len() != wire.first_data_blocks.len()
        {
            return Err("extrusion first_indices/raw_first_indices/first_index_source_offsets/first_data_blocks column lengths differ".into());
        }
        if wire.second_indices.len() != wire.raw_second_indices.len()
            || wire.second_indices.len() != wire.second_index_source_offsets.len()
            || wire.second_indices.len() != wire.second_data_blocks.len()
        {
            return Err("extrusion second_indices/raw_second_indices/second_index_source_offsets/second_data_blocks column lengths differ".into());
        }
        if wire.body_object_index != wire.terminal_object_index {
            return Err("body_object_index must match terminal_object_index".into());
        }
        let atoms = wire
            .atom_indices
            .into_iter()
            .zip(wire.atoms_be)
            .zip(wire.atom_data_blocks)
            .map(|((value, raw), binding)| {
                WrappedCompactIndex::from_wire(value, raw)
                    .map(|token| (token, binding))
                    .map_err(|error| format!("atom_indices/atoms_be: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let first = wire
            .first_indices
            .into_iter()
            .zip(wire.raw_first_indices)
            .zip(wire.first_data_blocks)
            .map(|((value, raw), binding)| {
                CompactIndexAtom::from_wire(value, &raw)
                    .map(|token| (token, binding))
                    .map_err(|error| format!("first_indices/raw_first_indices: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let second = wire
            .second_indices
            .into_iter()
            .zip(wire.raw_second_indices)
            .zip(wire.second_data_blocks)
            .map(|((value, raw), binding)| {
                CompactIndexAtom::from_wire(value, &raw)
                    .map(|token| (token, binding))
                    .map_err(|error| format!("second_indices/raw_second_indices: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let frame = Extrude32Frame::new(
            wire.source_offset,
            ShiftedBinary64::from_wire(wire.scalar, wire.raw_scalar)
                .map_err(|error| format!("scalar/raw_scalar: {error}"))?,
            BranchItems::new(atoms).map_err(|error| format!("atom_indices: {error}"))?,
            BranchItems::new(first).map_err(|error| format!("first_indices: {error}"))?,
            BranchItems::new(second).map_err(|error| format!("second_indices: {error}"))?,
            FeatureReferenceToken::from_wire(
                wire.terminal_object_index,
                &wire.raw_terminal_object_index,
            )
            .map_err(|error| format!("terminal_object_index/raw_terminal_object_index: {error}"))?,
        )?;
        if !frame
            .atoms()
            .map(|(_, _, offset)| offset)
            .eq(wire.atom_source_offsets)
        {
            return Err("atom_source_offsets: disagrees with extrusion frame".into());
        }
        if !frame
            .first_indices()
            .map(|(_, _, offset)| offset)
            .eq(wire.first_index_source_offsets)
        {
            return Err("first_index_source_offsets: disagrees with extrusion frame".into());
        }
        if !frame
            .second_indices()
            .map(|(_, _, offset)| offset)
            .eq(wire.second_index_source_offsets)
        {
            return Err("second_index_source_offsets: disagrees with extrusion frame".into());
        }
        if frame.terminal_offset() != wire.terminal_source_offset {
            return Err("terminal_source_offset: disagrees with extrusion frame".into());
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
    use super::*;

    #[test]
    fn construction_keeps_paired_nonempty_profiles_and_the_exact_wire() {
        let wire = r#"{"id":"c","operation_label":"o","branch":"b","body_object_index":0,"profile_references":["","r"],"profile_data_blocks":["a",""],"atom_data_blocks":["atom"],"first_data_blocks":["first"],"second_data_blocks":["second"]}"#;
        let value: FeatureExtrude32Construction = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), wire);
        for fields in [
            vec!["profile_references"],
            vec!["profile_data_blocks"],
            vec!["profile_references", "profile_data_blocks"],
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
            for field in fields {
                invalid[field] = serde_json::json!([]);
            }
            let error =
                serde_json::from_value::<FeatureExtrude32Construction>(invalid).unwrap_err();
            assert!(error
                .to_string()
                .contains("profile_references/profile_data_blocks"));
        }
    }
    #[test]
    fn construction_rejects_out_of_range_lane_counts() {
        let original = serde_json::json!({"id":"c", "operation_label":"o", "branch":"b", "body_object_index":0, "profile_references":[""], "profile_data_blocks":[""], "atom_data_blocks":[""], "first_data_blocks":[""], "second_data_blocks":[""]});
        for field in [
            "atom_data_blocks",
            "first_data_blocks",
            "second_data_blocks",
        ] {
            for length in [0, 255] {
                let mut invalid = original.clone();
                invalid[field] = serde_json::json!(vec![""; length]);
                assert!(
                    serde_json::from_value::<FeatureExtrude32Construction>(invalid)
                        .unwrap_err()
                        .to_string()
                        .contains(field)
                );
            }
        }
        let mut invalid = original;
        invalid["profile_references"] = serde_json::json!(vec![""; 255]);
        invalid["profile_data_blocks"] = serde_json::json!(vec![""; 255]);
        assert!(serde_json::from_value::<FeatureExtrude32Construction>(invalid).is_err());
    }
    #[test]
    fn branch_rejects_impossible_positions_and_lane_counts() {
        let wire = r#"{"id":"b","operation_label":"o","body_object_index":0,"scalar":1.0,"raw_scalar":[47,240,0,0,0,0,0,0],"atoms_be":[1031798784],"atom_source_offsets":[113],"atom_indices":[0],"atom_data_blocks":[""],"first_indices":[0,4096],"raw_first_indices":[[0],[144,0]],"first_index_source_offsets":[119,120],"first_data_blocks":[null,""],"second_indices":[0],"raw_second_indices":[[128,0]],"second_index_source_offsets":[124],"second_data_blocks":[null],"terminal_object_index":0,"raw_terminal_object_index":[144,0,0],"terminal_source_offset":128,"source_offset":100}"#;
        let parsed: FeatureExtrudePayload32Branch = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&parsed).unwrap(), wire);
        for field in [
            "atom_source_offsets",
            "first_index_source_offsets",
            "second_index_source_offsets",
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
            invalid[field][0] = serde_json::json!(1000);
            assert!(
                serde_json::from_value::<FeatureExtrudePayload32Branch>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains(field)
            );
        }
        for field in ["source_offset", "terminal_source_offset"] {
            let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
            invalid[field] = serde_json::json!(u64::MAX);
            assert!(
                serde_json::from_value::<FeatureExtrudePayload32Branch>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains(field)
            );
        }
        for fields in [
            [
                "atoms_be",
                "atom_source_offsets",
                "atom_indices",
                "atom_data_blocks",
            ],
            [
                "first_indices",
                "raw_first_indices",
                "first_index_source_offsets",
                "first_data_blocks",
            ],
            [
                "second_indices",
                "raw_second_indices",
                "second_index_source_offsets",
                "second_data_blocks",
            ],
        ] {
            for length in [0, 255] {
                let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
                for field in fields {
                    let item = invalid[field][0].clone();
                    invalid[field] = serde_json::json!(vec![item; length]);
                }
                assert!(serde_json::from_value::<FeatureExtrudePayload32Branch>(invalid).is_err());
            }
        }
        let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
        invalid["raw_terminal_object_index"] = serde_json::json!([240, 0]);
        assert!(serde_json::from_value::<FeatureExtrudePayload32Branch>(invalid).is_err());
    }
}
