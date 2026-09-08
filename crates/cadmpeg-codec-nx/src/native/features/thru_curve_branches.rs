// SPDX-License-Identifier: Apache-2.0
//! Native `THRU_CURVE` groups with one source frame.

use super::{unique_offset_data_block, visit_feature_history_operation_records};
use crate::container::Container;
use crate::om::branch_items::BranchItems;
use crate::om::reference_index::PayloadIndexToken;
use crate::om::thru_curve_branches::{
    thru_curve_payload_branch_group, ThruCurveBranch, ThruCurveGroup,
};
use crate::om::thru_curve_endings::{ThruCurveBranchSuffix, ThruCurveGroupTerminator};
use crate::om::thru_curve_state::ThruCurveBranchItems;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "GroupWire", into = "GroupWire")]
pub(crate) struct FeatureThruCurveConstructionBranchGroup {
    pub(crate) id: String,
    pub(crate) operation_label: String,
    frame: ThruCurveGroup<Option<String>>,
}

#[derive(Serialize, Deserialize)]
struct GroupWire {
    id: String,
    operation_label: String,
    declared_count: u8,
    branches: Vec<BranchWire>,
    terminator: ThruCurveGroupTerminator,
    source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct BranchWire {
    ordinal: u32,
    mode: NonZeroU8,
    declared_count: u8,
    state_lane: Vec<u8>,
    members: Vec<ReferenceWire>,
    terminal: ReferenceWire,
    suffix: ThruCurveBranchSuffix,
    source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct ReferenceWire {
    ordinal: u32,
    #[serde(flatten)]
    token: PayloadIndexToken,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data_block: Option<String>,
    source_offset: u64,
}

impl From<FeatureThruCurveConstructionBranchGroup> for GroupWire {
    fn from(value: FeatureThruCurveConstructionBranchGroup) -> Self {
        let branches = value
            .frame
            .branches()
            .as_slice()
            .iter()
            .zip(value.frame.branch_offsets())
            .enumerate()
            .map(|(ordinal, (branch, source_offset))| {
                let members = branch
                    .members
                    .as_slice()
                    .iter()
                    .zip(branch.member_positions())
                    .enumerate()
                    .map(|(ordinal, ((token, data_block), position))| ReferenceWire {
                        ordinal: ordinal as u32,
                        token: *token,
                        data_block: data_block.clone(),
                        source_offset: source_offset + position,
                    })
                    .collect();
                BranchWire {
                    ordinal: ordinal as u32,
                    mode: branch.mode,
                    declared_count: branch.members.declared_count(),
                    state_lane: branch.members.state_lane(),
                    members,
                    terminal: ReferenceWire {
                        ordinal: branch.members.len() as u32,
                        token: branch.terminal.0,
                        data_block: branch.terminal.1.clone(),
                        source_offset: source_offset + branch.terminal_position(),
                    },
                    suffix: branch.suffix,
                    source_offset,
                }
            })
            .collect();
        Self {
            id: value.id,
            operation_label: value.operation_label,
            declared_count: value.frame.branches().declared_count(),
            branches,
            terminator: value.frame.terminator(),
            source_offset: value.frame.offset(),
        }
    }
}

impl TryFrom<GroupWire> for FeatureThruCurveConstructionBranchGroup {
    type Error = &'static str;

    fn try_from(wire: GroupWire) -> Result<Self, Self::Error> {
        let mut branches = Vec::with_capacity(wire.branches.len());
        let mut locations = Vec::with_capacity(wire.branches.len());
        for (ordinal, branch) in wire.branches.into_iter().enumerate() {
            if branch.ordinal as usize != ordinal {
                return Err("branches.ordinal must equal branch order");
            }
            if usize::from(branch.declared_count) != branch.members.len() + 1 {
                return Err("declared_count must equal members length plus one");
            }
            let mut members = Vec::with_capacity(branch.members.len());
            let mut positions = Vec::with_capacity(branch.members.len());
            for (ordinal, reference) in branch.members.into_iter().enumerate() {
                if reference.ordinal as usize != ordinal {
                    return Err("members.ordinal must equal member order");
                }
                positions.push(reference.source_offset);
                members.push((reference.token, reference.data_block));
            }
            if branch.terminal.ordinal as usize != members.len() {
                return Err("terminal.ordinal must equal members length");
            }
            branches.push(ThruCurveBranch {
                mode: branch.mode,
                members: ThruCurveBranchItems::from_parts(members, &branch.state_lane)?,
                terminal: (branch.terminal.token, branch.terminal.data_block),
                suffix: branch.suffix,
            });
            locations.push((
                branch.source_offset,
                positions,
                branch.terminal.source_offset,
            ));
        }
        let branches = BranchItems::new(branches)?;
        if wire.declared_count != branches.declared_count() {
            return Err("declared_count must equal branches length plus one");
        }
        let frame = ThruCurveGroup::new(wire.source_offset, branches, wire.terminator)?;
        for ((branch, offset), (source_offset, members, terminal)) in frame
            .branches()
            .as_slice()
            .iter()
            .zip(frame.branch_offsets())
            .zip(locations)
        {
            if source_offset != offset {
                return Err("branches.source_offset must follow the group frame");
            }
            if branch
                .member_positions()
                .zip(members)
                .any(|(position, stored)| offset + position != stored)
            {
                return Err("members.source_offset must follow the branch frame");
            }
            if terminal != offset + branch.terminal_position() {
                return Err("terminal.source_offset must follow the branch frame");
            }
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            frame,
        })
    }
}

pub(crate) fn feature_thru_curve_construction_branch_groups(
    container: &Container,
) -> Vec<FeatureThruCurveConstructionBranchGroup> {
    let indexed = container.indexed_om_sections();
    let mut groups = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(group) = thru_curve_payload_branch_group(record.payload_view()) else {
                return;
            };
            let Ok(frame) = group.resolve(entry_offset, |token| {
                unique_offset_data_block(&indexed, token.value())
            }) else {
                return;
            };
            let operation_key = format!("{section_key}-{operation_ordinal:010}");
            groups.push(FeatureThruCurveConstructionBranchGroup {
                id: format!(
                    "nx:feature-history:thru-curve-construction-branch-group#{operation_key}"
                ),
                operation_label: format!("nx:feature-history:operation-label#{operation_key}"),
                frame,
            });
        },
    );
    groups
}

#[cfg(test)]
mod tests {
    use super::FeatureThruCurveConstructionBranchGroup;

    // Group count at 100. The standard branch occupies 24 bytes and the
    // extended branch occupies 40 bytes. The adjacent terminator occupies 9.
    const WIRE: &str = concat!(
        r#"{"id":"g","operation_label":"o","declared_count":3,"branches":["#,
        r#"{"ordinal":0,"mode":255,"declared_count":3,"state_lane":[0,0,0,0,0,0],"members":["#,
        r#"{"ordinal":0,"object_index":0,"raw_object_index":[240,0],"data_block":"","source_offset":104},"#,
        r#"{"ordinal":1,"object_index":256,"raw_object_index":[241,1,0],"source_offset":106}],"#,
        r#""terminal":{"ordinal":2,"object_index":1,"raw_object_index":[240,1],"source_offset":120},"#,
        r#""suffix":[129,88],"source_offset":101},"#,
        r#"{"ordinal":1,"mode":1,"declared_count":5,"state_lane":[0,0,0,0,1,5,2,3,4,5,1,5,6,7,8,9,0,0],"members":["#,
        r#"{"ordinal":0,"object_index":1,"raw_object_index":[240,1],"source_offset":128},"#,
        r#"{"ordinal":1,"object_index":2,"raw_object_index":[240,2],"source_offset":130},"#,
        r#"{"ordinal":2,"object_index":3,"raw_object_index":[240,3],"source_offset":132},"#,
        r#"{"ordinal":3,"object_index":4,"raw_object_index":[240,4],"source_offset":134}],"#,
        r#""terminal":{"ordinal":4,"object_index":256,"raw_object_index":[241,1,0],"data_block":"target","source_offset":159},"#,
        r#""suffix":[129,72],"source_offset":125}],"#,
        r#""terminator":[0,0,0,0,0,0,255,255,1],"source_offset":100}"#,
    );

    #[test]
    fn group_wire_derives_both_lane_forms_and_mixed_token_widths() {
        let group: FeatureThruCurveConstructionBranchGroup = serde_json::from_str(WIRE).unwrap();
        assert_eq!(serde_json::to_string(&group).unwrap(), WIRE);
        assert_eq!(group.frame.branch_offsets().collect::<Vec<_>>(), [101, 125]);
    }

    #[test]
    fn group_wire_rejects_independent_counts_ordinals_positions_and_token_grammars() {
        let wire: serde_json::Value = serde_json::from_str(WIRE).unwrap();
        for (path, replacement, field) in [
            (
                "/source_offset",
                serde_json::json!(u64::MAX),
                "source_offset",
            ),
            ("/declared_count", serde_json::json!(2), "declared_count"),
            ("/branches/0/ordinal", serde_json::json!(1), "ordinal"),
            ("/branches/1/ordinal", serde_json::json!(0), "ordinal"),
            (
                "/branches/0/source_offset",
                serde_json::json!(102),
                "source_offset",
            ),
            (
                "/branches/1/source_offset",
                serde_json::json!(126),
                "source_offset",
            ),
            (
                "/branches/0/declared_count",
                serde_json::json!(4),
                "declared_count",
            ),
            (
                "/branches/0/members/0/ordinal",
                serde_json::json!(1),
                "ordinal",
            ),
            (
                "/branches/0/members/0/source_offset",
                serde_json::json!(105),
                "source_offset",
            ),
            (
                "/branches/0/members/0/raw_object_index",
                serde_json::json!([0]),
                "raw_object_index",
            ),
            (
                "/branches/0/terminal/ordinal",
                serde_json::json!(1),
                "ordinal",
            ),
            (
                "/branches/0/terminal/source_offset",
                serde_json::json!(121),
                "source_offset",
            ),
            (
                "/branches/0/terminal/raw_object_index",
                serde_json::json!([1]),
                "raw_object_index",
            ),
            (
                "/branches/1/state_lane/5",
                serde_json::json!(4),
                "state_lane",
            ),
        ] {
            let mut invalid = wire.clone();
            *invalid.pointer_mut(path).unwrap() = replacement;
            let error = serde_json::from_value::<FeatureThruCurveConstructionBranchGroup>(invalid)
                .unwrap_err();
            assert!(error.to_string().contains(field), "{path}: {error}");
        }
    }
}
