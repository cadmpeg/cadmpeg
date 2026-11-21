// SPDX-License-Identifier: Apache-2.0
//! Validate Emboss scalar ownership and construction-group membership.

use super::{design_stream, Ctx};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::{DesignParameter, DesignParameterKind, DesignParameterOwner};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::{DesignExtrudeSelectionGroup, DesignOperandRole};
use cadmpeg_ir::report::check::{Check, Finding};

pub(super) fn validate(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    for scope in &ctx.native.design_parameter_scopes {
        let operands = scope
            .emboss_operand_construction()
            .zip(scope.emboss_table_slots())
            .is_none_or(|(construction, slots)| {
                construction.sketch_profile.is_some()
                    && ctx.native.design_face_operands.iter().any(|operand| {
                        design_stream(&operand.id) == design_stream(&scope.id)
                            && operand.scope_record_index == scope.record_index
                            && operand.scope_reference_ordinal == slots.face_group
                            && operand.group_record_index() == Some(construction.face_group.value)
                            && operand.group_member_ordinal() == Some(0)
                            && usize::try_from(slots.face_member)
                                .ok()
                                .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                                == Some(&operand.record_index())
                            && operand.recipe_kind
                                == crate::records::recipes::ConstructionRecipeKind::BoundedFace
                    })
            });
        if scope
            .emboss_construction()
            .is_some_and(|construction| !construction.links_match(scope))
            || !operands
            || !auxiliary_links_hold(scope, &ctx.native.design_extrude_selection_groups)
            || !links_hold(
                scope,
                &ctx.native.design_parameters,
                &ctx.native.design_parameter_owners,
                &ctx.native.design_construction_operand_groups,
            )
        {
            super::push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Emboss has an invalid scalar-owner, selection-group, or operand link",
                Some(scope.id.clone()),
            );
        }
    }
}

fn auxiliary_links_hold(
    scope: &DesignParameterScope,
    groups: &[DesignExtrudeSelectionGroup],
) -> bool {
    let (Some(construction), Some(slots)) = (
        scope.emboss_operand_construction(),
        scope.emboss_table_slots(),
    ) else {
        return true;
    };
    let mut matching = groups.iter().filter(|group| {
        design_stream(&group.id) == design_stream(&scope.id)
            && group.scope_record_index == scope.record_index
            && group.record_index == construction.auxiliary_group.value
    });
    let Some(group) = matching.next() else {
        return false;
    };
    matching.next().is_none()
        && group.scope_reference_ordinal == slots.auxiliary_group
        && !group.typed_references
}

fn links_hold(
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    groups: &[DesignConstructionOperandGroup],
) -> bool {
    let (Some(construction), Some(slots)) = (
        scope.emboss_operand_construction(),
        scope.emboss_table_slots(),
    ) else {
        return true;
    };
    let stream = design_stream(&scope.id);
    let scalars = construction
        .scalar_owners()
        .iter()
        .all(|(reference, kind)| {
            let mut matching_owners = owners.iter().filter(|owner| {
                design_stream(owner.id()) == stream
                    && owner.record_index() == reference.value
                    && owner.scope_record_index() == scope.record_index
            });
            let Some(owner) = matching_owners.next() else {
                return false;
            };
            if matching_owners.next().is_some() {
                return false;
            }
            let mut matching_parameters = parameters.iter().filter(|parameter| {
                design_stream(&parameter.id) == stream
                    && parameter.record_index == owner.parameter_record_index()
            });
            let Some(parameter) = matching_parameters.next() else {
                return false;
            };
            matching_parameters.next().is_none()
                && parameter.owner_record_index() == Some(owner.record_index())
                && parameter.kind() == DesignParameterKind::Feature
                && parameter.source_kind_str() == *kind
                && parameter.evaluated_value() == owner.evaluated_value()
                && parameter.unit().is_some_and(|unit| {
                    if *kind == "Rotation Angle" {
                        matches!(unit.value.as_str(), "deg" | "rad")
                    } else {
                        crate::design::feature_project::design_length_unit(unit.value.as_str())
                    }
                })
        });
    let selections = [
        (
            construction.profile_group.value,
            DesignOperandRole::PROFILE,
            slots.profile_group,
            slots.profile_member,
        ),
        (
            construction.face_group.value,
            DesignOperandRole::ROLE_0X10,
            slots.face_group,
            slots.face_member,
        ),
    ]
    .into_iter()
    .all(|(record, role, ordinal, member_ordinal)| {
        let Ok(member_ordinal) = usize::try_from(member_ordinal) else {
            return false;
        };
        let mut matching = groups.iter().filter(|group| {
            design_stream(&group.id) == stream
                && group.scope_record_index == scope.record_index
                && group.record_index == record
        });
        let Some(group) = matching.next() else {
            return false;
        };
        matching.next().is_none()
            && group.role() == role
            && group.scope_reference_ordinal == ordinal
            && group.members().len() == 1
            && group.members().first().map(|member| &member.value)
                == scope.reference_members().values().nth(member_ordinal)
    });
    construction.links_match(scope) && scalars && selections
}

#[cfg(test)]
mod tests;
