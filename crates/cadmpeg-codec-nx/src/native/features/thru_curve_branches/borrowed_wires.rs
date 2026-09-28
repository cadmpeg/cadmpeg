// SPDX-License-Identifier: Apache-2.0
//! Borrowed `THRU_CURVE` branch group serialization.

use super::FeatureThruCurveConstructionBranchGroup;
use crate::om::thru_curve_branches::ThruCurveBranch;
use crate::om::thru_curve_state::ThruCurveBranchItems;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::Serialize;

#[derive(Serialize)]
struct ReferenceView<'a> {
    ordinal: u32,
    #[serde(flatten)]
    token: &'a crate::om::reference_index::PayloadIndexToken,
    #[serde(skip_serializing_if = "Option::is_none")]
    data_block: Option<&'a str>,
    source_offset: u64,
}

struct StateLaneView<'a>(
    &'a ThruCurveBranchItems<(
        crate::om::reference_index::PayloadIndexToken,
        Option<String>,
    )>,
);

impl Serialize for StateLaneView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            ThruCurveBranchItems::Standard(members) => {
                [0_u8; 258][..members.len() + 4].serialize(serializer)
            }
            ThruCurveBranchItems::Extended {
                values: [first, second],
                ..
            } => [
                0, 0, 0, 0, 1, 5, first[0], first[1], first[2], first[3], 1, 5, second[0],
                second[1], second[2], second[3], 0, 0,
            ]
            .serialize(serializer),
        }
    }
}

struct MembersView<'a> {
    branch: &'a ThruCurveBranch<Option<String>>,
    offset: u64,
}

impl Serialize for MembersView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let members = self.branch.members.as_slice();
        let mut sequence = serializer.serialize_seq(Some(members.len()))?;
        for (ordinal, ((token, data_block), position)) in members
            .iter()
            .zip(self.branch.member_positions())
            .enumerate()
        {
            sequence.serialize_element(&ReferenceView {
                ordinal: ordinal as u32,
                token,
                data_block: data_block.as_deref(),
                source_offset: self.offset + position,
            })?;
        }
        sequence.end()
    }
}

struct BranchView<'a> {
    ordinal: u32,
    branch: &'a ThruCurveBranch<Option<String>>,
    offset: u64,
}

impl Serialize for BranchView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("mode", &self.branch.mode)?;
        wire.serialize_entry("declared_count", &self.branch.members.declared_count())?;
        wire.serialize_entry("state_lane", &StateLaneView(&self.branch.members))?;
        wire.serialize_entry(
            "members",
            &MembersView {
                branch: self.branch,
                offset: self.offset,
            },
        )?;
        wire.serialize_entry(
            "terminal",
            &ReferenceView {
                ordinal: self.branch.members.len() as u32,
                token: &self.branch.terminal.0,
                data_block: self.branch.terminal.1.as_deref(),
                source_offset: self.offset + self.branch.terminal_position(),
            },
        )?;
        wire.serialize_entry("suffix", &self.branch.suffix)?;
        wire.serialize_entry("source_offset", &self.offset)?;
        wire.end()
    }
}

struct BranchesView<'a>(&'a FeatureThruCurveConstructionBranchGroup);

impl Serialize for BranchesView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let branches = self.0.frame.branches().as_slice();
        let mut sequence = serializer.serialize_seq(Some(branches.len()))?;
        for (ordinal, (branch, offset)) in branches
            .iter()
            .zip(self.0.frame.branch_offsets())
            .enumerate()
        {
            sequence.serialize_element(&BranchView {
                ordinal: ordinal as u32,
                branch,
                offset,
            })?;
        }
        sequence.end()
    }
}

impl Serialize for FeatureThruCurveConstructionBranchGroup {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("declared_count", &self.frame.branches().declared_count())?;
        wire.serialize_entry("branches", &BranchesView(self))?;
        wire.serialize_entry("terminator", &self.frame.terminator())?;
        wire.serialize_entry("source_offset", &self.frame.offset())?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_test_support::native_serialization::assert_native_limit;

    #[test]
    fn thru_curve_group_borrowed_bytes_and_limit() {
        let json =
            super::super::tests::WIRE.replace("\"id\":\"g\"", "\"id\":\"nx:feature:thru-curve#0\"");
        let group: FeatureThruCurveConstructionBranchGroup = serde_json::from_str(&json).unwrap();
        let borrowed = serde_json::to_vec(&group).unwrap();
        let owned = serde_json::to_vec(&super::super::GroupWire::from(group.clone())).unwrap();
        assert_eq!(borrowed, owned);
        assert_eq!(borrowed, json.as_bytes());
        assert_native_limit(
            &group,
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        );
    }
}
