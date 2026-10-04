// SPDX-License-Identifier: Apache-2.0
//! Native `THRU_CURVE` groups with one source frame.

use super::{
    charged_unique_offset_data_block, format_feature_history_id,
    visit_feature_history_operation_records,
};
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

mod borrowed_wires;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "GroupWire")]
pub(in crate::native) struct FeatureThruCurveConstructionBranchGroup {
    id: String,
    operation_label: String,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_data_block"
    )]
    data_block: Option<String>,
    source_offset: u64,
}

#[cfg(test)]
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
                        ordinal: u32::try_from(ordinal).expect("fixture value fits u32"),
                        token: *token,
                        data_block: data_block.clone(),
                        source_offset: source_offset + position,
                    })
                    .collect();
                BranchWire {
                    ordinal: u32::try_from(ordinal).expect("fixture value fits u32"),
                    mode: branch.mode,
                    declared_count: branch.members.declared_count(),
                    state_lane: branch.members.state_lane(),
                    members,
                    terminal: ReferenceWire {
                        ordinal: u32::from(branch.members.declared_count() - 1),
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
        let mut branches = {
            let mut storage = Vec::new();
            storage
                .try_reserve_exact(wire.branches.len())
                .map(|()| storage)
        }
        .map_err(|_| "branches: allocation failed")?;
        let mut locations = {
            let mut storage = Vec::new();
            storage
                .try_reserve_exact(wire.branches.len())
                .map(|()| storage)
        }
        .map_err(|_| "locations: allocation failed")?;
        for (ordinal, branch) in wire.branches.into_iter().enumerate() {
            if cadmpeg_core::decode::index_from_u32(branch.ordinal) != ordinal {
                return Err("branches.ordinal must equal branch order");
            }
            if usize::from(branch.declared_count) != branch.members.len() + 1 {
                return Err("declared_count must equal members length plus one");
            }
            let mut members = {
                let mut storage = Vec::new();
                storage
                    .try_reserve_exact(branch.members.len())
                    .map(|()| storage)
            }
            .map_err(|_| "members: allocation failed")?;
            let mut positions = {
                let mut storage = Vec::new();
                storage
                    .try_reserve_exact(branch.members.len())
                    .map(|()| storage)
            }
            .map_err(|_| "positions: allocation failed")?;
            for (ordinal, reference) in branch.members.into_iter().enumerate() {
                if cadmpeg_core::decode::index_from_u32(reference.ordinal) != ordinal {
                    return Err("members.ordinal must equal member order");
                }
                positions.push(reference.source_offset);
                members.push((reference.token, reference.data_block));
            }
            if cadmpeg_core::decode::index_from_u32(branch.terminal.ordinal) != members.len() {
                return Err("terminal.ordinal must equal members length");
            }
            branches.push(ThruCurveBranch::new(
                branch.mode,
                ThruCurveBranchItems::from_parts(members, &branch.state_lane)?,
                (branch.terminal.token, branch.terminal.data_block),
                branch.suffix,
            )?);
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

pub(in crate::native) fn feature_thru_curve_construction_branch_groups(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureThruCurveConstructionBranchGroup>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut groups = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let group = match thru_curve_payload_branch_group(ctx, record.payload_view()) {
                Ok(Some(group)) => group,
                Ok(None) => return,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let frame = match group.resolve(ctx, entry_offset, |token| {
                charged_unique_offset_data_block(ctx, &indexed, token.value())
            }) {
                Ok(Some(frame)) => frame,
                Ok(None) => return,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let projected = (|| -> Result<_, cadmpeg_core::CodecError> {
                let id = format_feature_history_id(
                    ctx,
                    "thru-curve-construction-branch-group",
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
                ctx.reserve_vec(&mut groups, 1, "NX thru-curve construction branch groups")?;
                Ok(FeatureThruCurveConstructionBranchGroup {
                    id,
                    operation_label,
                    frame,
                })
            })();
            match projected {
                Ok(group) => groups.push(group),
                Err(error) => failure = Some(error),
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::FeatureThruCurveConstructionBranchGroup;

    fn branch_group_container() -> crate::container::Container<'static> {
        let mut branch = b"\x13\x00\x00\x01\x00\xf1\x01\x21\xf1\x01\x22\xf1\x01\x23\x01\x08\x02\x03\x03\x04\x01\x01\x01\x01\x07\xf1\x01\x24\xf1\x01\x25\xf1\x01\x26\xf1\x01\x27\xf1\x01\x28\xf1\x01\x29\x04\x01\xa0\x5e\x38\x13\x01".to_vec();
        branch.extend([2, 0x15, 1, 2, 0xf0, 0x31, 1, 2]);
        branch.extend([0; 5]);
        branch.extend([0xff, 1, 2, 0xf0, 0x32, 0, 0x81, 0x58]);
        branch.extend([0, 0, 0, 0, 0, 0, 0xff, 0, 0xff, 1]);
        let payload = crate::test_support::test_om::composed_feature_history_payload(
            &[(&[0xff; 4], "THRU_CURVE", branch)],
            &[],
        );
        let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            payload,
        )]);
        crate::test_support::with_decode_context(move |ctx| crate::container::scan_bytes(ctx, file))
            .expect("synthetic THRU_CURVE branch group container")
    }

    fn branch_group_route_refusal(
        configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> cadmpeg_core::CodecError {
        let container = branch_group_container();
        let groups = crate::test_support::with_decode_context(|ctx| {
            super::feature_thru_curve_construction_branch_groups(ctx, &container)
        })
        .expect("admitted THRU_CURVE branch group");
        assert_eq!(groups.len(), 1);

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                configure(policy);
            },
            |ctx| {
                super::feature_thru_curve_construction_branch_groups(ctx, &container)
                    .expect_err("THRU_CURVE branch group resource limit")
            },
        )
    }

    #[test]
    fn thru_curve_branch_group_route_refuses_collection_limit() {
        let error = branch_group_route_refusal(|policy| policy.limits.max_collection_items = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn thru_curve_branch_group_route_refuses_retained_limit() {
        let error = branch_group_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn thru_curve_branch_group_route_refuses_work_limit() {
        let error = branch_group_route_refusal(|policy| policy.limits.max_work_units = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
    }

    // Group count at 100. The standard branch occupies 24 bytes and the
    // extended branch occupies 40 bytes. The adjacent terminator occupies 9.
    pub(super) const WIRE: &str = concat!(
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

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_data_block, String, "data_block");
