// SPDX-License-Identifier: Apache-2.0
//! Native surface construction branches with derived reference positions.

use super::{unique_offset_data_block, visit_feature_history_operation_records};
use crate::container::Container;
use crate::om::branch_items::BranchItems;
use crate::om::discriminators::SurfaceBranchMode;
use crate::om::reference_index::PayloadIndexToken;
use crate::om::surface_branches::{
    surface_feature_payload_branches, SurfaceBranch, SurfaceFamily, SurfaceSuffix,
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU8;

/// One exact counted branch in a bounded surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SurfaceBranchWire", into = "SurfaceBranchWire")]
pub(crate) struct FeatureSurfaceConstructionBranch {
    pub(crate) id: String,
    pub(crate) operation_label: String,
    order: NonZeroU8,
    pub(crate) family: SurfaceFamily,
    pub(crate) header_code: u8,
    pub(crate) references: SurfaceBranch<Option<String>>,
}

impl FeatureSurfaceConstructionBranch {
    pub(crate) fn ordinal(&self) -> u32 {
        u32::from(self.order.get()) - 1
    }
}

#[derive(Serialize, Deserialize)]
struct SurfaceReferenceWire {
    ordinal: u32,
    #[serde(flatten)]
    token: PayloadIndexToken,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data_block: Option<String>,
    source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct SurfaceBranchWire {
    id: String,
    operation_label: String,
    ordinal: u32,
    family: SurfaceFamily,
    header_code: u8,
    mode: SurfaceBranchMode,
    declared_count: u8,
    witnessed: bool,
    members: Vec<SurfaceReferenceWire>,
    terminal: SurfaceReferenceWire,
    suffix: Vec<u8>,
    source_offset: u64,
}

impl From<FeatureSurfaceConstructionBranch> for SurfaceBranchWire {
    fn from(value: FeatureSurfaceConstructionBranch) -> Self {
        let branch = &value.references;
        let members = branch
            .members()
            .as_slice()
            .iter()
            .zip(branch.member_offsets())
            .enumerate()
            .map(
                |(ordinal, ((token, data_block), source_offset))| SurfaceReferenceWire {
                    ordinal: ordinal as u32,
                    token: *token,
                    data_block: data_block.clone(),
                    source_offset,
                },
            )
            .collect();
        Self {
            ordinal: value.ordinal(),
            id: value.id,
            operation_label: value.operation_label,
            family: value.family,
            header_code: value.header_code,
            mode: branch.mode(),
            declared_count: branch.members().declared_count(),
            witnessed: branch.witnessed(),
            members,
            terminal: SurfaceReferenceWire {
                ordinal: branch.members().len() as u32,
                token: branch.terminal().0,
                data_block: branch.terminal().1.clone(),
                source_offset: branch.terminal_offset(),
            },
            suffix: branch.suffix().clone().into_vec(),
            source_offset: branch.offset(),
        }
    }
}

impl TryFrom<SurfaceBranchWire> for FeatureSurfaceConstructionBranch {
    type Error = String;

    fn try_from(wire: SurfaceBranchWire) -> Result<Self, Self::Error> {
        let order = wire
            .ordinal
            .checked_add(1)
            .and_then(|order| u8::try_from(order).ok())
            .and_then(NonZeroU8::new)
            .ok_or("ordinal must be 0 through 254")?;
        let members =
            BranchItems::new(wire.members).map_err(|error| format!("members: {error}"))?;
        if wire.declared_count != members.declared_count() {
            return Err("declared_count must equal members length plus one".to_owned());
        }
        let positions = members
            .as_slice()
            .iter()
            .map(|member| (member.ordinal, member.source_offset))
            .collect::<Vec<_>>();
        let terminal_position = (wire.terminal.ordinal, wire.terminal.source_offset);
        let members = members.map_indexed(|_, member| (member.token, member.data_block));
        let references = SurfaceBranch::new(
            wire.source_offset,
            wire.mode,
            wire.witnessed,
            members,
            (wire.terminal.token, wire.terminal.data_block),
            SurfaceSuffix::new(wire.suffix)?,
        )?;
        for ((ordinal, source_offset), (expected_ordinal, expected_offset)) in positions
            .into_iter()
            .zip(references.member_offsets().enumerate())
        {
            if ordinal != expected_ordinal as u32 {
                return Err("members.ordinal must follow serialized order".to_owned());
            }
            if source_offset != expected_offset {
                return Err("members.source_offset must follow the branch frame".to_owned());
            }
        }
        if terminal_position.0 != references.members().len() as u32 {
            return Err("terminal.ordinal must equal members length".to_owned());
        }
        if terminal_position.1 != references.terminal_offset() {
            return Err("terminal.source_offset must follow the branch frame".to_owned());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            order,
            family: wire.family,
            header_code: wire.header_code,
            references,
        })
    }
}

/// Resolve branch references without assigning section or guide semantics.
pub(crate) fn feature_surface_construction_branches(
    container: &Container,
) -> Vec<FeatureSurfaceConstructionBranch> {
    let indexed = container.indexed_om_sections();
    let mut branches = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(group) = surface_feature_payload_branches(record.payload_view()) else {
                return;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let family = group.family;
            let header_code = group.header_code;
            branches.extend(group.into_branches().into_iter().enumerate().filter_map(|(ordinal, branch)| {
                let order = u8::try_from(ordinal + 1).ok().and_then(NonZeroU8::new)?;
                let references = branch.resolve(entry_offset, |token| unique_offset_data_block(&indexed, token.value())).ok()?;
                Some(FeatureSurfaceConstructionBranch {
                    id: format!("nx:feature-history:surface-construction-branch#{section_key}-{operation_ordinal:010}-{ordinal:010}"),
                    operation_label: operation_label.clone(), order, family, header_code, references,
                })
            }));
        },
    );
    branches
}

#[cfg(test)]
mod tests {
    use super::FeatureSurfaceConstructionBranch;

    const WIRE: &str = r#"{"id":"branch","operation_label":"operation","ordinal":254,"family":80,"header_code":255,"mode":22,"declared_count":3,"witnessed":false,"members":[{"ordinal":0,"object_index":0,"raw_object_index":[240,0],"data_block":"zero","source_offset":103},{"ordinal":1,"object_index":256,"raw_object_index":[241,1,0],"source_offset":105}],"terminal":{"ordinal":2,"object_index":1,"raw_object_index":[240,1],"source_offset":116},"suffix":[0,255],"source_offset":100}"#;

    #[test]
    fn surface_branch_positions_follow_token_widths_and_count_witness() {
        for wire in [
            WIRE.to_owned(),
            WIRE.replace("\"witnessed\":false", "\"witnessed\":true")
                .replace("\"source_offset\":116", "\"source_offset\":119"),
        ] {
            let branch: FeatureSurfaceConstructionBranch = serde_json::from_str(&wire).unwrap();
            assert_eq!(serde_json::to_string(&branch).unwrap(), wire);
        }
    }

    #[test]
    fn surface_branch_rejects_independent_positions_and_wrong_reference_grammar() {
        for (old, new, field) in [
            ("\"ordinal\":254", "\"ordinal\":255", "ordinal"),
            ("\"ordinal\":0", "\"ordinal\":1", "members.ordinal"),
            ("\"ordinal\":2,", "\"ordinal\":0,", "terminal.ordinal"),
            (
                "\"source_offset\":103",
                "\"source_offset\":104",
                "members.source_offset",
            ),
            (
                "\"source_offset\":105",
                "\"source_offset\":106",
                "members.source_offset",
            ),
            (
                "\"source_offset\":116",
                "\"source_offset\":117",
                "terminal.source_offset",
            ),
            (
                "\"witnessed\":false",
                "\"witnessed\":true",
                "terminal.source_offset",
            ),
            (
                "\"source_offset\":100",
                "\"source_offset\":18446744073709551615",
                "source_offset",
            ),
            ("[240,0]", "[0]", "raw_object_index"),
            ("\"family\":80", "\"family\":81", "family"),
            ("\"suffix\":[0,255]", "\"suffix\":[]", "suffix"),
            ("\"suffix\":[0,255]", "\"suffix\":[0,1,2,3,4,5]", "suffix"),
        ] {
            let invalid = WIRE.replace(old, new);
            let error =
                serde_json::from_str::<FeatureSurfaceConstructionBranch>(&invalid).unwrap_err();
            assert!(error.to_string().contains(field), "{field}: {error}");
        }
    }
}
