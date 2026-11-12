// SPDX-License-Identifier: Apache-2.0
//! Extrude parameter and face-operand consistency.

use super::{design_stream, push_constant_finding, Ctx};
use crate::design;
use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_layout;
use crate::design::decode::scopes::extrude::through_all;
use crate::records;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::id_from_index;
use cadmpeg_ir::report::check::Check;
use cadmpeg_ir::report::check::Finding;

/// True for a one-sided face target whose stored face-group roles equal
/// `expected` with its termination role removed (DR-360).
pub(super) fn one_sided_face_target_lacks_only_its_termination(
    extent: records::feature::extrude::DesignExtrudeExtent,
    expected: &[records::topology::extrude_selection::DesignExtrudeFaceRole],
    stored: impl Iterator<Item = Option<records::topology::extrude_selection::DesignExtrudeFaceRole>>,
) -> bool {
    use records::topology::extrude_selection::DesignExtrudeFaceRole::Termination;
    extent == records::feature::extrude::DesignExtrudeExtent::OneSidedToFace
        && expected.contains(&Termination)
        && stored.eq(expected
            .iter()
            .copied()
            .filter(|role| *role != Termination)
            .map(Some))
}

/// Validate Extrude profile, operation, start, and extent operand agreement.
/// The body operands are design-intent records, so the operation is checked
/// against them only in a document that holds them.
pub(super) fn validate_extrude_parameter_operands(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    for scope in native.design_parameter_scopes.iter().filter(|scope| {
        matches!(
            design::design_feature_family(&scope.kind()),
            Some(
                design::DesignFeatureFamily::Extrude
                    | design::DesignFeatureFamily::Fillet
                    | design::DesignFeatureFamily::Chamfer
            )
        )
    }) {
        let native_stream = design_stream(&scope.id);
        if design::design_feature_family(&scope.kind())
            == Some(design::DesignFeatureFamily::Extrude)
        {
            let mut profile_groups = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role()
                            == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Profile)
                });
            let first_profile_group = profile_groups.next();
            let second_profile_group = profile_groups.next();
            let profile_matches_operand = scope.extrude_profile().is_none_or(|profile| {
                match (first_profile_group, second_profile_group) {
                    (None, _) => {
                        usize::try_from(profile.scope_reference_ordinal)
                            .ok()
                            .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                            == Some(&profile.record_index)
                    }
                    (Some(group), None) => {
                        group.members().first().map(|member| &member.value)
                            == Some(&profile.record_index)
                    }
                    (Some(_), Some(_)) => false,
                }
            });
            if !profile_matches_operand {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Extrude profile conflicts with its profile operand group",
                    Some(scope.id.clone()),
                );
            }
            let has_body_operands = native
                .design_construction_operand_groups
                .iter()
                .any(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role()
                            == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Bodies)
                });
            let face_operand_group_count = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role().is_some_and(|role| {
                            matches!(role, records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_))
                        })
                })
                .count();
            let target_shape_group_count = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.role() == DesignOperandRole::ROLE_0X5
                        && group.extrude_role().is_none()
                        && !group.members().is_empty()
                        && group
                            .members()
                            .iter()
                            .map(|member| &member.value)
                            .enumerate()
                            .all(|(ordinal, record_index)| {
                                id_from_index(ordinal).is_some_and(|ordinal| {
                                    native.design_body_recipe_operands.iter().any(|operand| {
                                        design_stream(&operand.id) == native_stream
                                            && operand.scope_record_index == scope.record_index
                                            && operand.owner.group()
                                                == Some((group.record_index, ordinal))
                                            && operand.record_index() == *record_index
                                    })
                                })
                            })
                })
                .count();
            let operation_matches_operands = match scope
                .extrude_prologue()
                .map(records::feature::extrude::DesignExtrudePrologue::operation)
            {
                Some(records::feature::extrude::DesignExtrudeOperation::NewBody) => {
                    !has_body_operands
                }
                Some(
                    records::feature::extrude::DesignExtrudeOperation::Join
                    | records::feature::extrude::DesignExtrudeOperation::Cut
                    | records::feature::extrude::DesignExtrudeOperation::Intersect,
                ) => has_body_operands || scope.history_state_id().is_none(),
                None => true,
            };
            if ctx.holds_design_intent() && !operation_matches_operands {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Extrude operation conflicts with its body operands",
                    Some(scope.id.clone()),
                );
            }
            let Some(prologue) = scope.extrude_prologue() else {
                continue;
            };
            let Some(extrude_extent) = prologue.extent() else {
                continue;
            };
            let parameter_kind_count = |source_kind: &str| {
                native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == scope.record_index
                    })
                    .filter_map(|owner| {
                        parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                    })
                    .filter(|parameter| parameter.source_kind() == source_kind)
                    .count()
            };
            let parameter_kind_values = |source_kind: &'static str| {
                native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == scope.record_index
                    })
                    .filter_map(|owner| {
                        parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                    })
                    .filter(move |parameter| parameter.source_kind() == source_kind)
                    .map(|parameter| parameter.evaluated_value().get())
            };
            let along_count = parameter_kind_count("AlongDistance");
            let against_count = parameter_kind_count("AgainstDistance");
            let fixed_lanes = scope.fixed_extrude_parameters();
            // A fixed lane is usually also the parameter owner of its source
            // kind; it adds a carrier only when it is not.
            let lane_count = |source_kind: &str,
                              lane: Option<
                &records::feature::fixed_parameters::DesignFixedExtrudeScalar,
            >| {
                let lane_is_owner = lane.is_some_and(|lane| {
                    native.design_parameter_owners.iter().any(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == scope.record_index
                            && owner.record_index() == lane.record_index
                            && parameters_by_index
                                .get(&(native_stream, owner.parameter_record_index()))
                                .is_some_and(|parameter| parameter.source_kind() == source_kind)
                    })
                });
                parameter_kind_count(source_kind) + usize::from(lane.is_some() && !lane_is_owner)
            };
            let profile_offset_count = lane_count(
                "ProfileOffset",
                fixed_lanes.and_then(|fixed| fixed.profile_offset.as_ref()),
            );
            let side_one_offset_count = lane_count(
                "Side1Offset",
                fixed_lanes.and_then(|fixed| fixed.side_one_offset.as_ref()),
            );
            // A to-face Extrude's fixed lanes carry only its termination
            // offset and taper.
            let fixed_lanes_are_to_face_lanes = fixed_lanes.is_none_or(|fixed| {
                fixed.along_distance.is_none()
                    && fixed.against_distance.is_none()
                    && fixed.side_two_taper_angle.is_none()
            });
            let omitted_zero_side_one_offset =
                crate::design::face_resolve::extrude_omits_zero_side_one_offset(
                    scope,
                    &prologue,
                    side_one_offset_count,
                );
            let mut side_one_offsets = parameter_kind_values("Side1Offset");
            let side_one_offset_is_absent = match side_one_offsets.next() {
                None => true,
                Some(0.0) => side_one_offsets.next().is_none(),
                Some(_) => false,
            };
            let side_two_offset_count = lane_count(
                "Side2Offset",
                fixed_lanes.and_then(|fixed| fixed.side_two_offset.as_ref()),
            );
            let has_fixed_extrude_parameters = scope.fixed_extrude_parameters().is_some();
            let has_fixed_along = scope
                .fixed_extrude_parameters()
                .as_ref()
                .is_some_and(|fixed| fixed.along_distance.is_some());
            let fixed_along_uses_reversal = scope
                .fixed_extrude_parameters()
                .as_ref()
                .and_then(|fixed| fixed.along_distance.as_ref())
                .is_some_and(|distance| {
                    matches!(
                        distance,
                        records::feature::fixed_parameters::DesignFixedExtrudeDistance::DistanceConstruction(_)
                    )
                });
            let has_one_along_carrier = along_count <= 1 && (along_count == 1 || has_fixed_along);
            let has_fixed_against = scope
                .fixed_extrude_parameters()
                .as_ref()
                .is_some_and(|fixed| fixed.against_distance.is_some());
            let has_one_against_carrier =
                against_count <= 1 && (against_count == 1 || has_fixed_against);
            let class_296_two_faces_layout = scope
                .reference_count_offset()
                .checked_sub(scope.byte_offset())
                .is_some_and(|reference_count_offset| {
                    is_class_296_two_sided_to_faces_layout(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                        scope.frame_length(),
                        reference_count_offset,
                        scope.reference_members().len(),
                    )
                });
            let extent_matches_operands = match extrude_extent {
                records::feature::extrude::DesignExtrudeExtent::OneSidedDistance => {
                    has_one_along_carrier
                        && against_count == 0
                        && side_one_offset_is_absent
                        && (!prologue.direction_reversed() || fixed_along_uses_reversal)
                }
                records::feature::extrude::DesignExtrudeExtent::OneSidedToFace => {
                    along_count == 0
                        && fixed_lanes_are_to_face_lanes
                        && against_count == 0
                        && if target_shape_group_count == 1 {
                            side_one_offset_is_absent
                        } else {
                            side_one_offset_count == 1 || omitted_zero_side_one_offset
                        }
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces => {
                    along_count == 0
                        && !has_fixed_along
                        && against_count == 0
                        && !has_fixed_against
                        && side_one_offset_count == 1
                        && side_two_offset_count == 1
                        && (!prologue.direction_reversed()
                            || class_296_two_faces_layout
                            || design::decode::operands::extrude_has_selected_termination(scope))
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance => {
                    has_one_along_carrier
                        && has_one_against_carrier
                        && side_one_offset_count == 0
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceThroughAll => {
                    has_one_along_carrier
                        && against_count == 0
                        && !has_fixed_against
                        && side_one_offset_count == 0
                        && side_two_offset_count == 0
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace
                | records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFaceControlOne => {
                    along_count == 1
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_count == 0
                        && side_two_offset_count == 1
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedThroughAllToFace => {
                    along_count == 0
                        && !has_fixed_along
                        && against_count == 0
                        && !has_fixed_against
                        && side_one_offset_count == 0
                        && side_two_offset_count == 1
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedTargetToDistanceOptionZero => {
                    ((has_one_along_carrier && against_count == 0 && !has_fixed_against)
                        || (along_count == 0 && !has_fixed_along && has_one_against_carrier))
                        && side_one_offset_count == 1
                        && side_two_offset_count == 0
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedToTargetsOptionZero
                | records::feature::extrude::DesignExtrudeExtent::TwoSidedToTargetsOptionTwo => {
                    along_count == 0
                        && !has_fixed_along
                        && against_count == 0
                        && !has_fixed_against
                        && side_one_offset_count == 1
                        && side_two_offset_count == 1
                }
                records::feature::extrude::DesignExtrudeExtent::SymmetricDistance => {
                    has_one_along_carrier
                        && against_count == 0
                        && side_one_offset_is_absent
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedThroughAll => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_count == 0
                        && side_two_offset_count == 0
                        && !prologue.direction_reversed()
                        && through_all::taper_parameters_match(
                            scope,
                            native
                                .design_parameter_owners
                                .iter()
                                .filter(|owner| {
                                    design_stream(owner.id()) == native_stream
                                        && owner.scope_record_index() == scope.record_index
                                })
                                .filter_map(|owner| {
                                    parameters_by_index
                                        .get(&(native_stream, owner.parameter_record_index()))
                                        .copied()
                                }),
                        )
                }
                records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_is_absent
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::OneSidedThroughNext
                | records::feature::extrude::DesignExtrudeExtent::OneSidedThroughAll => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_is_absent
                }
            };
            let extrude_start = prologue.start();
            let start_matches_operands = match extrude_start {
                records::feature::extrude::DesignExtrudeStart::ProfilePlane => {
                    profile_offset_count == 0
                }
                records::feature::extrude::DesignExtrudeStart::OffsetProfilePlane
                | records::feature::extrude::DesignExtrudeStart::FromFace => {
                    profile_offset_count == 1
                }
            };
            let target_face_group_count = match extrude_extent {
                records::feature::extrude::DesignExtrudeExtent::TwoSidedTargetToDistanceOptionZero => {
                    1usize.checked_sub(target_shape_group_count)
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedToTargetsOptionZero
                | records::feature::extrude::DesignExtrudeExtent::TwoSidedToTargetsOptionTwo => {
                    2usize.checked_sub(target_shape_group_count)
                }
                _ => Some(0),
            };
            let expected_face_group_count = target_face_group_count.map(|target_face_group_count| usize::from(
                matches!(
                    extrude_extent,
                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace
                ) && target_shape_group_count == 0,
            ) + 2 * usize::from(matches!(
                extrude_extent,
                records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces
            )) + usize::from(matches!(
                extrude_extent,
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace
                    | records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFaceControlOne
                    | records::feature::extrude::DesignExtrudeExtent::TwoSidedThroughAllToFace
            )) + target_face_group_count + usize::from(matches!(
                extrude_start,
                records::feature::extrude::DesignExtrudeStart::FromFace
            )));
            let mut face_groups = native.design_construction_operand_groups.iter().filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role().is_some_and(|role| {
                            matches!(role, records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_))
                        })
                }).collect::<Vec<_>>();
            face_groups.sort_by(|left, right| {
                left.scope_reference_ordinal
                    .cmp(&right.scope_reference_ordinal)
            });
            let expected_face_roles = match (extrude_start, extrude_extent) {
                (start, records::feature::extrude::DesignExtrudeExtent::TwoSidedTargetToDistanceOptionZero
                    | records::feature::extrude::DesignExtrudeExtent::TwoSidedToTargetsOptionZero
                    | records::feature::extrude::DesignExtrudeExtent::TwoSidedToTargetsOptionTwo
                    | records::feature::extrude::DesignExtrudeExtent::TwoSidedThroughAllToFace) => {
                    let mut roles = Vec::new();
                    if start == records::feature::extrude::DesignExtrudeStart::FromFace {
                        roles.push(records::topology::extrude_selection::DesignExtrudeFaceRole::Start);
                    }
                    let terminations = expected_face_group_count
                        .and_then(|count| count.checked_sub(roles.len()))
                        .unwrap_or(0);
                    roles.extend(std::iter::repeat_n(
                        records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                        terminations,
                    ));
                    roles
                }
                (
                    records::feature::extrude::DesignExtrudeStart::FromFace,
                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace,
                ) if target_shape_group_count == 0 => vec![
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Start,
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                ],
                (records::feature::extrude::DesignExtrudeStart::FromFace, _) => {
                    vec![records::topology::extrude_selection::DesignExtrudeFaceRole::Start]
                }
                (_, records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                    if target_shape_group_count == 0 =>
                {
                    vec![records::topology::extrude_selection::DesignExtrudeFaceRole::Termination]
                }
                (_, records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces) => vec![
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                ],
                (_, records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace
                    | records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFaceControlOne) => {
                    vec![records::topology::extrude_selection::DesignExtrudeFaceRole::Termination]
                }
                _ => Vec::new(),
            };
            let invalid_face_groups_hide_extent_conflict =
                class_296_two_faces_layout && face_operand_group_count == 0;
            // DR-322: a new-body one-sided distance scope with start-support
            // byte 2, a ProfileOffset, and no face group of any role.
            let selected_start_without_group = matches!(
                extrude_start,
                records::feature::extrude::DesignExtrudeStart::FromFace
            ) && ((matches!(
                extrude_extent,
                records::feature::extrude::DesignExtrudeExtent::OneSidedDistance
            ) && prologue.operation()
                == records::feature::extrude::DesignExtrudeOperation::NewBody
                && face_operand_group_count == 0
                && face_groups.is_empty())
                || (matches!(
                    extrude_extent,
                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace
                ) && face_groups.len() == 1
                    && face_groups.iter().all(|group| {
                        group.extrude_face_role()
                            == Some(records::topology::extrude_selection::DesignExtrudeFaceRole::Termination)
                    })));
            // DR-360: a one-sided face target whose roster stores every
            // expected group except its termination group.
            let termination_missing = one_sided_face_target_lacks_only_its_termination(
                extrude_extent,
                &expected_face_roles,
                face_groups.iter().map(|group| group.extrude_face_role()),
            );
            if ctx.holds_design_intent()
                && !invalid_face_groups_hide_extent_conflict
                && (!extent_matches_operands
                    || !start_matches_operands
                    || (!selected_start_without_group
                        && !termination_missing
                        && (Some(face_operand_group_count) != expected_face_group_count
                            || face_groups
                                .iter()
                                .map(|group| group.extrude_face_role())
                                .ne(expected_face_roles.iter().copied().map(Some)))))
            {
                push_constant_finding(findings, Check::NativeLinks, "Fusion Design Extrude start or extent conflicts with its parameters and face operands", Some(scope.id.clone()));
            }
        }
        if design::design_feature_family(&scope.kind()) == Some(design::DesignFeatureFamily::Sweep)
        {
            let mut profile_groups =
                native
                    .design_construction_operand_groups
                    .iter()
                    .filter(|group| {
                        design_stream(&group.id) == native_stream
                            && group.scope_record_index == scope.record_index
                            && group.role() == DesignOperandRole::PROFILE
                    });
            let profile_group = profile_groups.next();
            let profile_matches_operand = profile_groups.next().is_none()
                && scope.sweep_profile().is_none_or(|profile| {
                    profile_group.is_some_and(|group| {
                        group
                            .members()
                            .iter()
                            .map(|member| member.value)
                            .eq([profile.record_index])
                    })
                });
            if !profile_matches_operand {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Sweep profile conflicts with its profile operand group",
                    Some(scope.id.clone()),
                );
            }
        }
    }
}
