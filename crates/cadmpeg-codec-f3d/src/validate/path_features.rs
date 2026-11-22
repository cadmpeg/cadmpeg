// SPDX-License-Identifier: Apache-2.0
//! Validation of path feature operand roles.

use super::{design_stream, push_constant_finding, Ctx};
use crate::records;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::report::check::{Check, Finding};

pub(super) fn is_path_seed_group(
    group: &records::topology::construction::DesignConstructionOperandGroup,
) -> bool {
    group.role() == DesignOperandRole::PATH_SEED
        && group.members().is_empty()
        && group.frame.auxiliary_records.len() == 2
        && group
            .frame
            .auxiliary_paths()
            .iter()
            .map(records::topology::construction::DesignConstructionOperandPath::record_index)
            .eq(group
                .frame
                .auxiliary_records
                .iter()
                .map(|record| record.value))
        && group.frame.trailing_records().len() == 1
}

/// Validate the operand-group roles of path-feature scopes. The groups are
/// design-intent records, so a document without them has nothing to check.
pub(super) fn validate_path_feature_operand_roles(ctx: &Ctx, findings: &mut Vec<Finding>) {
    if !ctx.holds_design_intent() {
        return;
    }
    let native = ctx.native;
    // A Pipe construction's complete group relation is admitted with the
    // native records; Intersect has no assigned role sequence.
    for scope in native.design_parameter_scopes.iter().filter(|scope| {
        scope.has_path_construction()
            && scope.kind() != records::feature::scope::DesignFeatureKind::Pipe
    }) {
        let native_stream = design_stream(&scope.id);
        let groups = native
            .design_construction_operand_groups
            .iter()
            .filter(|group| {
                design_stream(&group.id) == native_stream
                    && group.scope_record_index == scope.record_index
            })
            .collect::<Vec<_>>();
        let role_count = |role| groups.iter().filter(|group| group.role() == role).count();
        let valid = match &scope.payload() {
            records::feature::scope::DesignScopePayload::Revolve(Some(
                crate::records::feature::path_features::DesignRevolveConstruction {
                    operation,
                    travel,
                    angle_record_index,
                    opposite_angle,
                    second_side,
                    ..
                },
            )) => {
                let body_count = role_count(DesignOperandRole::BODIES_A)
                    + role_count(DesignOperandRole::BODIES_B);
                let expected_body_count = usize::from(
                    *operation != records::feature::extrude::DesignExtrudeOperation::NewBody,
                );
                let target_groups = match travel {
                    records::feature::path_features::DesignRevolveTravel::Angle(_) => Vec::new(),
                    records::feature::path_features::DesignRevolveTravel::ToTargets(targets) => {
                        std::iter::once(targets.side_one)
                            .chain(targets.side_two)
                            .map(|located| located.value)
                            .collect()
                    }
                };
                target_groups.iter().all(|record_index| {
                    groups.iter().any(|group| {
                        group.record_index == *record_index
                            && group.role() == DesignOperandRole::ROLE_0X5
                            && group.members().len() == 1
                    })
                }) && scope
                    .reference_members()
                    .values()
                    .any(|value| value == angle_record_index)
                    && opposite_angle.is_none_or(|located| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == &located.value)
                    })
                    && second_side.is_none_or(|side| {
                        side.angle_record_index != *angle_record_index
                            && scope
                                .reference_members()
                                .values()
                                .any(|value| value == &side.angle_record_index)
                    })
                    && groups.len() == 2 + expected_body_count + target_groups.len()
                    && role_count(DesignOperandRole::ROLE_0X21) == 1
                    && role_count(DesignOperandRole::PROFILE) == 1
                    && body_count == expected_body_count
            }
            records::feature::scope::DesignScopePayload::Loft(Some(construction)) => {
                construction.section_records.is_empty()
                    || crate::design::feature_project::sweep_loft::loft_groups(
                        construction,
                        &groups,
                    )
                    .is_ok()
            }
            records::feature::scope::DesignScopePayload::Sweep(Some(
                records::feature::scope::DesignSweepScope {
                    construction:
                        Some(records::feature::path_features::DesignSweepConstruction {
                            operation, ..
                        }),
                    ..
                },
            )) => {
                let path_count = role_count(DesignOperandRole::ROLE_0X5);
                let profile_count = role_count(DesignOperandRole::PROFILE);
                let guide_surface_count = role_count(DesignOperandRole::FACES);
                let guide_profile_frame = scope.sweep_profile().is_some_and(|profile| {
                    let profile_groups = || {
                        groups
                            .iter()
                            .filter(|group| group.role() == DesignOperandRole::PROFILE)
                    };
                    profile_groups()
                        .filter(|group| {
                            group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([profile.record_index])
                        })
                        .count()
                        == 1
                        && profile_groups()
                            .filter(|group| {
                                !group
                                    .members()
                                    .iter()
                                    .map(|member| member.value)
                                    .eq([profile.record_index])
                            })
                            .filter(|group| {
                                !group.members().is_empty()
                                    && group.members().iter().map(|member| &member.value).all(
                                        |member| {
                                            native.design_entity_selection_operands.iter().any(
                                                |operand| {
                                                    design_stream(&operand.id) == native_stream
                                                        && operand.scope_record_index
                                                            == scope.record_index
                                                        && operand.group_record_index
                                                            == group.record_index
                                                        && operand.record_index() == *member
                                                },
                                            )
                                        },
                                    )
                            })
                            .count()
                            == 1
                });
                let body_count = role_count(DesignOperandRole::BODIES_A)
                    + role_count(DesignOperandRole::BODIES_B);
                let common_roles =
                    (profile_count == 1 && guide_surface_count == 0 && matches!(path_count, 1 | 2))
                        || (profile_count == 2
                            && guide_surface_count == 1
                            && path_count == 1
                            && guide_profile_frame);
                common_roles
                    && match operation {
                        records::feature::extrude::DesignExtrudeOperation::NewBody => {
                            groups.len() == path_count + profile_count + guide_surface_count
                                && body_count == 0
                        }
                        records::feature::extrude::DesignExtrudeOperation::Join
                        | records::feature::extrude::DesignExtrudeOperation::Cut
                        | records::feature::extrude::DesignExtrudeOperation::Intersect => {
                            guide_surface_count == 0
                                && groups.len() == path_count + 2
                                && body_count == 1
                        }
                    }
            }
            _ => false,
        };
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design path-feature operand roles conflict with its construction",
                Some(scope.id.clone()),
            );
        }
    }
}
