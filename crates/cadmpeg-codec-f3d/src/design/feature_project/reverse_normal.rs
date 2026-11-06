// SPDX-License-Identifier: Apache-2.0

use super::single_operand_group;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;

pub(super) fn project_reverse_normal(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    let operation = scope.reverse_normal_operation()?;
    let group = single_operand_group(groups, scope, DesignOperandRole::BODIES_A)?;
    let mut references = scope.reference_members().values().copied();
    if group.scope_reference_ordinal != 0
        || references.next() != Some(group.record_index)
        || group.record_index != operation.body_group.value
        || !group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(references)
    {
        return None;
    }
    Some(FeatureDefinition::Operation(
        FeatureOperation::ReverseShape {
            source: BodySelection::Native(group.id.clone()),
        },
    ))
}
