// SPDX-License-Identifier: Apache-2.0
//! Validation of fillet radius groups, fillet sets, and fillet operand groups.

use super::{design_stream, push_constant_finding, Ctx};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::{design, records};
use cadmpeg_ir::report::check::{Check, Finding};
use std::collections::HashSet;

/// Validate Fillet radius-law parameter assignments; returns the assigned groups.
pub(super) fn validate_fillet_radius_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let construction_groups_by_index = &ctx.operand_groups_by_index;
    let mut fillet_radius_group_records = HashSet::new();
    let mut fillet_radius_group_slots = HashSet::new();
    for assignment in &native.design_fillet_radius_groups {
        let native_stream = design_stream(&assignment.id);
        let scope = scopes_by_index.get(&(native_stream, assignment.scope_record_index));
        let group =
            construction_groups_by_index.get(&(native_stream, assignment.group_record_index));
        let assignment_parameter = |record_index: u32| {
            let parameter = *parameters_by_index.get(&(native_stream, record_index))?;
            let owner = *owners_by_index.get(&(native_stream, parameter.owner_record_index()?))?;
            (scope.is_some_and(|scope| scope.names_owner(owner))
                && owner.parameter_record_index() == record_index)
                .then_some(parameter)
        };
        let tangency_weight = assignment
            .tangency_weight_parameter_record_index
            .and_then(&assignment_parameter);
        let is_fillet = |scope: &&records::feature::scope::DesignParameterScope| {
            design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Fillet)
        };
        let valid = scope.is_some_and(is_fillet)
            && group.is_some_and(|group| {
                group.scope_record_index == assignment.scope_record_index
                    && group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq(assignment.edge_operand_record_indices.iter().copied())
            })
            && match &assignment.law {
                records::topology::fillet::DesignFilletRadiusLaw::Constant {
                    radius_parameter_record_index,
                } => {
                    assignment_parameter(*radius_parameter_record_index).is_some_and(|parameter| {
                        parameter.is_fillet_radius()
                            && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    })
                }
                records::topology::fillet::DesignFilletRadiusLaw::Chordal {
                    chord_length_parameter_record_index,
                } => assignment_parameter(*chord_length_parameter_record_index).is_some_and(
                    |parameter| {
                        parameter.source_kind() == "ChordLen"
                            && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    },
                ),
                records::topology::fillet::DesignFilletRadiusLaw::Asymmetric {
                    offset_one_parameter_record_index,
                    offset_two_parameter_record_index,
                } => [
                    (*offset_one_parameter_record_index, "EdgeOffset1"),
                    (*offset_two_parameter_record_index, "EdgeOffset2"),
                ]
                .into_iter()
                .all(|(record_index, kind)| {
                    assignment_parameter(record_index).is_some_and(|parameter| {
                        parameter.source_kind() == kind
                            && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    })
                }),
                records::topology::fillet::DesignFilletRadiusLaw::Variable {
                    start_radius_parameter_record_index,
                    end_radius_parameter_record_index,
                    middle: midpoint_records,
                } => {
                    let radius = |record_index: u32, kind: &str| {
                        assignment_parameter(record_index)
                            .filter(|parameter| {
                                parameter.source_kind() == kind
                                    && parameter
                                        .unit()
                                        .map(|field| field.value.as_str())
                                        .is_some_and(design::feature_project::design_length_unit)
                                    && parameter.evaluated_value().get() >= 0.0
                            })
                            .map(|parameter| parameter.evaluated_value().get())
                    };
                    let start = radius(*start_radius_parameter_record_index, "StartRadius");
                    let end = radius(*end_radius_parameter_record_index, "EndRadius");
                    let explicit_pair_order = scope
                        .and_then(|scope| scope.fillet_sets())
                        .is_some_and(|list| {
                            list.sets.iter().any(|set| {
                                set.group_record_index == assignment.group_record_index
                                    && matches!(
                                        set.law,
                                        records::feature::fillet_set::DesignFilletSetLaw::Variable { .. }
                                    )
                            })
                        });
                    let mut middle_positive = false;
                    let mut previous_position = None;
                    let mut middle_valid = true;
                    for row in midpoint_records {
                        let Some(middle_radius) =
                            radius(row.radius_parameter_record_index, "MidRadius")
                        else {
                            middle_valid = false;
                            break;
                        };
                        let Some(position) = assignment_parameter(row.parameter_record_index)
                            .filter(|parameter| {
                                parameter.source_kind() == "MidParams"
                                    && parameter.unit().is_none()
                                    && (0.0..1.0).contains(&parameter.evaluated_value().get())
                            })
                            .map(|parameter| parameter.evaluated_value().get())
                        else {
                            middle_valid = false;
                            break;
                        };
                        if previous_position.is_some_and(|previous| {
                            previous == position || (!explicit_pair_order && previous > position)
                        }) {
                            middle_valid = false;
                            break;
                        }
                        middle_positive |= middle_radius > 0.0;
                        previous_position = Some(position);
                    }
                    start.zip(end).is_some_and(|(start, end)| {
                        middle_valid && (start > 0.0 || end > 0.0 || middle_positive)
                    })
                }
            }
            && assignment
                .tangency_weight_parameter_record_index
                .is_none_or(|_| {
                    tangency_weight.is_some_and(|parameter| {
                        parameter.source_kind() == "TangencyWeight" && parameter.unit().is_none()
                    })
                })
            && scope
                .and_then(|scope| scope.fillet_sets())
                .is_none_or(|list| {
                    list.sets
                        .iter()
                        .filter(|set| set.group_record_index == assignment.group_record_index)
                        .all(|set| {
                            design::decode::scopes::fillet_set::fillet_set_matches_radius_group(
                                set,
                                assignment,
                                |owner_record_index| {
                                    ctx.owners_by_index
                                        .get(&(native_stream, owner_record_index))
                                        .map(|owner| owner.parameter_record_index())
                                },
                            )
                        })
                })
            && fillet_radius_group_records.insert((native_stream, assignment.group_record_index))
            && fillet_radius_group_slots.insert((
                native_stream,
                assignment.scope_record_index,
                assignment.group_ordinal,
            ));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Fillet radius group has an invalid parameter assignment",
                Some(assignment.id.clone()),
            );
        }
    }
    fillet_radius_group_records
}

/// Validate set links and return groups with an explicit radius law.
pub(super) fn validate_fillet_sets<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let mut assigned_groups = HashSet::new();
    for scope in &ctx.native.design_parameter_scopes {
        let Some(list) = scope.fillet_sets() else {
            continue;
        };
        let native_stream = design_stream(&scope.id);
        let owned_kind = |owner_record_index: u32, kind: &str| {
            ctx.owners_by_index
                .get(&(native_stream, owner_record_index))
                .filter(|owner| owner.scope_record_index() == scope.record_index)
                .and_then(|owner| {
                    ctx.parameters_by_index
                        .get(&(native_stream, owner.parameter_record_index()))
                        .filter(|parameter| {
                            parameter.owner_record_index() == Some(owner.record_index())
                        })
                })
                .is_some_and(|parameter| parameter.source_kind() == kind)
        };
        let constant_frame = |set: &records::feature::fillet_set::DesignFilletSet| {
            set.frame_length == 97
                && matches!(
                    set.law,
                    records::feature::fillet_set::DesignFilletSetLaw::Constant { .. }
                )
        };
        let flags_admitted = (list.form
            != records::feature::fillet_set::DesignFilletSetListForm::Compact
            || list.rolling_ball_corner)
            && list
                .sets
                .iter()
                .all(|set| set.curvature_continuity.is_some() == constant_frame(set));
        let valid = flags_admitted
            && list.sets.iter().all(|set| {
                let options_valid = set.options_match_law()
                    && (!set.law.has_implicit_ordinal() || list.sets.len() == 1);
                let mut owners = HashSet::new();
                options_valid
                    && set
                        .law
                        .owner_roles()
                        .chain(
                            set.tangency_weight_owner_record_index
                                .map(|owner| (owner, "TangencyWeight")),
                        )
                        .all(|(owner, kind)| owners.insert(owner) && owned_kind(owner, kind))
                    && scope
                        .reference_members()
                        .values()
                        .any(|member| *member == set.group_record_index)
            })
            && design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Fillet);
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Fillet set names a record outside its scope or an owner of another parameter kind",
                Some(scope.id.clone()),
            );
        } else if list.supports_set_law_projection() {
            assigned_groups.extend(
                list.sets
                    .iter()
                    .map(|set| (native_stream, set.group_record_index)),
            );
        }
    }
    assigned_groups
}

/// Whether a Mirror scope owns an operand group of this role: faces or
/// bodies to mirror, the mirror plane, or the `0x10` selection run.
pub(super) fn mirror_group_role_admitted(role: DesignOperandRole) -> bool {
    matches!(
        role,
        DesignOperandRole::BODIES_A
            | DesignOperandRole::ROLE_0X5
            | DesignOperandRole::BODIES_B
            | DesignOperandRole::ROLE_0X10
    )
}

/// Whether a Mirror face operand uses an admitted recipe kind for its group
/// role: a whole-face recipe in a body run, or a face or bounded-face recipe
/// in the mirror-plane run.
pub(super) fn mirror_operand_recipe_admitted(
    role: DesignOperandRole,
    kind: records::recipes::ConstructionRecipeKind,
) -> bool {
    match role {
        DesignOperandRole::BODIES_B => kind == records::recipes::ConstructionRecipeKind::Face,
        DesignOperandRole::ROLE_0X5 => matches!(
            kind,
            records::recipes::ConstructionRecipeKind::Face
                | records::recipes::ConstructionRecipeKind::BoundedFace
        ),
        _ => false,
    }
}

/// Whether a Revolve axis operand uses an admitted face-recipe spelling. The
/// axis face is selected either by its whole-face recipe or by a bounded-face
/// recipe.
/// Whether a `Revolve` group with this role holds face operands: the axis
/// group and a to-object target group.
pub(super) fn revolve_face_group_role_admitted(role: DesignOperandRole) -> bool {
    matches!(
        role,
        DesignOperandRole::ROLE_0X21 | DesignOperandRole::ROLE_0X5
    )
}

pub(super) fn revolve_axis_recipe_kind_admitted(
    kind: records::recipes::ConstructionRecipeKind,
) -> bool {
    matches!(
        kind,
        records::recipes::ConstructionRecipeKind::Face
            | records::recipes::ConstructionRecipeKind::BoundedFace
    )
}

/// Whether a full-round face operand selects a face. The selection is
/// resolved to face slots when the input state names a unique face; otherwise
/// the operand keeps the faces its recipe is bound against, which are the
/// preceding-state or changed faces it was read with.
pub(super) fn full_round_face_is_selected(
    operand: &records::topology::face::DesignFaceOperand,
) -> bool {
    !operand.resolved_face_slots.is_empty()
        || !operand.preceding_candidate_faces.is_empty()
        || !operand.changed_candidate_faces.is_empty()
}

/// Report Fillet operand groups that carry no radius assignment.
pub(super) fn validate_fillet_operand_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    fillet_radius_group_records: &HashSet<(&'a str, u32)>,
) {
    let native = ctx.native;
    let scopes_by_index = &ctx.scopes_by_index;
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        let scope = scopes_by_index.get(&(native_stream, group.scope_record_index));
        let is_fillet = scope.is_some_and(|scope| {
            design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Fillet)
        });
        let mut fixed_edge_group_count = 0;
        let mut is_fixed_edge_group = false;
        if let Some(scope) = scope {
            for candidate in &native.design_construction_operand_groups {
                if design_stream(&candidate.id) == native_stream
                    && candidate.scope_record_index == scope.record_index
                    && !candidate.members().is_empty()
                    && candidate
                        .members()
                        .iter()
                        .map(|member| &member.value)
                        .all(|member| {
                            native.design_edge_operands.iter().any(|operand| {
                                design_stream(&operand.id) == native_stream
                                    && operand.scope_record_index == scope.record_index
                                    && operand.record_index() == *member
                            })
                        })
                {
                    fixed_edge_group_count += 1;
                    is_fixed_edge_group |= candidate.record_index == group.record_index;
                }
            }
        }
        let has_radius_assignment =
            fillet_radius_group_records.contains(&(native_stream, group.record_index));
        let has_parameter_owner = native.design_parameter_owners.iter().any(|owner| {
            design_stream(owner.id()) == native_stream
                && owner.scope_record_index() == group.scope_record_index
        });
        let sole_compact_group_shape = scope.is_some_and(|scope| {
            native
                .design_construction_operand_groups
                .iter()
                .filter(|candidate| {
                    design_stream(&candidate.id) == native_stream
                        && candidate.scope_record_index == scope.record_index
                })
                .count()
                == 1
                && group
                    .members()
                    .iter()
                    .map(|member| &member.value)
                    .all(|member| {
                        native.design_edge_identity_operands.iter().any(|operand| {
                            design_stream(&operand.id) == native_stream
                                && operand.scope_record_index == scope.record_index
                                && operand.group_record_index == group.record_index
                                && operand.record_index() == *member
                        })
                    })
        });
        let full_round_group_shape = is_fillet
            && group.role() == DesignOperandRole::BODIES_A
            && !has_radius_assignment
            && !has_parameter_owner
            && scope.is_some_and(|scope| {
                native
                    .design_construction_operand_groups
                    .iter()
                    .filter(|candidate| {
                        design_stream(&candidate.id) == native_stream
                            && candidate.scope_record_index == scope.record_index
                    })
                    .count()
                    == 1
                    && group.members().len() == 1
                    && native.design_edge_operands.iter().all(|operand| {
                        design_stream(&operand.id) != native_stream
                            || operand.scope_record_index != scope.record_index
                            || operand.record_index() != group.members()[0].value
                    })
                    && native.design_face_operands.iter().any(|operand| {
                        design_stream(&operand.id) == native_stream
                            && operand.scope_record_index == scope.record_index
                            && operand.group_record_index() == Some(group.record_index)
                            && operand.group_member_ordinal() == Some(0)
                            && operand.record_index() == group.members()[0].value
                            && operand.recipe_kind
                                == records::recipes::ConstructionRecipeKind::BoundedFace
                    })
            });
        let valid_full_round_group = full_round_group_shape
            && !group.frame.variant
            && group.frame.trailing_records().len() == 1
            && group.frame.trailing_flags().len() == 1
            && group.frame.trailing_records()[0].value
                == group.frame.trailing_flags()[0].record_index
            && group.frame.trailing_flags()[0].value
            && native.design_face_operands.iter().any(|operand| {
                design_stream(&operand.id) == native_stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index() == Some(group.record_index)
                    && operand.group_member_ordinal() == Some(0)
                    && operand.record_index() == group.members()[0].value
                    && full_round_face_is_selected(operand)
            });
        if full_round_group_shape {
            if !valid_full_round_group {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Fillet full-round face group is invalid",
                    Some(group.id.clone()),
                );
            }
            continue;
        }
        let in_selection_pair = scope.is_some_and(|scope| {
            let mut scope_groups = native
                .design_construction_operand_groups
                .iter()
                .filter(|candidate| {
                    design_stream(&candidate.id) == native_stream
                        && candidate.scope_record_index == scope.record_index
                })
                .collect::<Vec<_>>();
            scope_groups.sort_by_key(|candidate| candidate.scope_reference_ordinal);
            design::decode::operands::fillet_edge_and_face_groups(&scope_groups).is_some_and(
                |pair| {
                    let face_backed = pair
                        .iter()
                        .filter(|candidate| candidate.role() == DesignOperandRole::ROLE_0X10)
                        .flat_map(|candidate| {
                            candidate
                                .members()
                                .iter()
                                .map(move |member| (candidate, member))
                        })
                        .all(|(candidate, member)| {
                            native.design_face_operands.iter().any(|operand| {
                                design_stream(&operand.id) == native_stream
                                    && operand.scope_record_index == scope.record_index
                                    && operand.group_record_index() == Some(candidate.record_index)
                                    && operand.record_index() == member.value
                                    && operand.recipe_kind
                                        == records::recipes::ConstructionRecipeKind::BoundedFace
                            })
                        });
                    face_backed
                        && pair
                            .iter()
                            .any(|candidate| candidate.record_index == group.record_index)
                },
            )
        });
        let has_fixed_assignment = scope
            .and_then(|scope| scope.fixed_fillet_parameters().map(|fixed| (scope, fixed)))
            .is_some_and(|(scope, fixed)| {
                native.design_parameter_owners.iter().all(|owner| {
                    design_stream(owner.id()) != native_stream
                        || owner.scope_record_index() != scope.record_index
                }) && fixed
                    .groups
                    .iter()
                    .flat_map(|group| {
                        group
                            .tangency_weight()
                            .into_iter()
                            .map(|tangency| tangency.record_index)
                            .chain(group.law().radii().map(|scalar| scalar.record_index))
                            .chain(
                                group
                                    .law()
                                    .intermediate()
                                    .iter()
                                    .map(|row| row.parameter.record_index),
                            )
                    })
                    .all(|record_index| {
                        scope
                            .reference_members()
                            .values()
                            .filter(|member| **member == record_index)
                            .count()
                            == 1
                    })
                    && ((fixed_edge_group_count == fixed.groups.len() && is_fixed_edge_group)
                        || (fixed.groups.len() == 1
                            && (sole_compact_group_shape || in_selection_pair)))
            });
        let retained_fixed_assignment = !has_parameter_owner
            && scope.is_some_and(|scope| {
                design::decode::operands::fixed_fillet_assignment_is_unresolved(
                    scope,
                    &native.design_construction_operand_groups,
                ) && ctx
                    .ir
                    .model
                    .features
                    .iter()
                    .filter(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
                    .all(|feature| {
                        matches!(
                            feature.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(
                                cadmpeg_ir::features::FeatureOperation::Native { .. }
                            )
                        )
                    })
            });
        let retained_set_list = scope.is_some_and(|scope| {
            design::decode::operands::fillet_set_list_is_unadmitted(
                scope,
                &native.design_construction_operand_groups,
                &native.design_fillet_radius_groups,
                &native.design_parameter_owners,
            ) && ctx
                .ir
                .model
                .features
                .iter()
                .filter(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
                .all(|feature| {
                    matches!(
                        feature.evaluation.definition(),
                        cadmpeg_ir::features::FeatureDefinition::Operation(
                            cadmpeg_ir::features::FeatureOperation::Native { .. }
                        )
                    )
                })
        });
        // DR-321: a sole face-role group with no owner and no fixed lane.
        let no_stored_radius = !has_parameter_owner
            && sole_compact_group_shape
            && group.role() == DesignOperandRole::ROLE_0X10
            && scope.is_some_and(|scope| scope.fixed_fillet_parameters().is_none());
        if is_fillet
            && !retained_fixed_assignment
            && !retained_set_list
            && !no_stored_radius
            && (group.role() == DesignOperandRole::BODIES_B || sole_compact_group_shape)
            && !has_fixed_assignment
            && !has_radius_assignment
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Fillet operand group has no radius assignment",
                Some(group.id.clone()),
            );
        }
    }
}
