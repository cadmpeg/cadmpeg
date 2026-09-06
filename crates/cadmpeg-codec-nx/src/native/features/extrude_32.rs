// SPDX-License-Identifier: Apache-2.0
//! Resolved extrusion construction from a structured branch.

use super::FeatureConstructionMember;
use crate::om::nonempty::NonEmpty;
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
    pub profiles: NonEmpty<FeatureConstructionMember>,
    /// Ordered uniquely resolved blocks from the fixed-atom lane.
    pub atom_data_blocks: Vec<String>,
    /// Ordered uniquely resolved blocks from the first compact-index lane.
    pub first_data_blocks: Vec<String>,
    /// Ordered uniquely resolved blocks from the second compact-index lane.
    pub second_data_blocks: Vec<String>,
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
            atom_data_blocks: value.atom_data_blocks,
            first_data_blocks: value.first_data_blocks,
            second_data_blocks: value.second_data_blocks,
        }
    }
}

impl TryFrom<ConstructionWire> for FeatureExtrude32Construction {
    type Error = &'static str;
    fn try_from(wire: ConstructionWire) -> Result<Self, Self::Error> {
        if wire.profile_references.len() != wire.profile_data_blocks.len() {
            return Err("profile_references/profile_data_blocks: column lengths differ");
        }
        let profiles = NonEmpty::new(
            wire.profile_references
                .into_iter()
                .zip(wire.profile_data_blocks)
                .map(|(reference, data_block)| FeatureConstructionMember {
                    reference,
                    data_block,
                }),
        )
        .ok_or("profile_references/profile_data_blocks: profile is empty")?;
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            branch: wire.branch,
            body_object_index: wire.body_object_index,
            profiles,
            atom_data_blocks: wire.atom_data_blocks,
            first_data_blocks: wire.first_data_blocks,
            second_data_blocks: wire.second_data_blocks,
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
}
