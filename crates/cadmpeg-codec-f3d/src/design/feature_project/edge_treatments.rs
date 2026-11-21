// SPDX-License-Identifier: Apache-2.0
//! Fillet and chamfer projection.

use crate::design::feature_project::chamfer;

use crate::design::edge_resolve::resolved_edge_treatment_group_with_corners;
use crate::design::face_resolve::design_angle;
use crate::design::feature_project::design_positive_length;
use crate::design::feature_project::full_round_fillet_source;

use crate::design::feature_project::project_face_selection;
use crate::design::feature_project::variable_fillet_law;
use crate::design::feature_project::ProjectInputs;
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand;
use crate::records::parameters::DesignParameter;
use crate::records::parameters::DesignParameterOwner;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::DesignEdgeIdentityOperand;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use crate::records::topology::fillet::DesignFilletRadiusGroup;
use crate::records::topology::fillet::DesignFilletRadiusLaw;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) struct ResolvedFilletAssignment<'a> {
    pub(super) assignment: &'a DesignFilletRadiusGroup,
    pub(super) radius: cadmpeg_ir::features::edge_treatments::RadiusSpec,
    pub(super) tangency_weight: Option<cadmpeg_ir::scalar::FiniteReal>,
}

pub(super) fn resolved_fillet_assignments<'a>(
    assignments: &[&'a DesignFilletRadiusGroup],
    parameters: &[(u32, &DesignParameter)],
) -> Option<Vec<ResolvedFilletAssignment<'a>>> {
    use cadmpeg_ir::features::edge_treatments::RadiusSpec;

    let mut by_record = std::collections::BTreeMap::new();
    for (_, parameter) in parameters {
        // discarded-value: duplicate record indices keep the last parameter.
        let _ = by_record.insert(parameter.record_index, *parameter);
    }
    if by_record.len() != parameters.len() {
        return None;
    }
    let mut assigned = Vec::new();
    for assignment in assignments {
        for record in fillet_law_parameter_records(&assignment.law)
            .chain(assignment.tangency_weight_parameter_record_index)
        {
            assigned.push(record);
        }
    }
    let one_shared_set = assignments.windows(2).all(|pair| {
        pair[0].law == pair[1].law
            && pair[0].tangency_weight_parameter_record_index
                == pair[1].tangency_weight_parameter_record_index
    });
    assigned.sort_unstable_by(Ord::cmp);
    if one_shared_set {
        assigned.dedup();
    }
    if !assigned.iter().copied().eq(by_record.keys().copied()) {
        return None;
    }
    let parameter = |record, kind| {
        by_record
            .get(&record)
            .copied()
            .filter(|parameter| parameter.source_kind() == kind)
    };
    let length = |record, kind| design_positive_length(parameter(record, kind)?);
    let mut resolved = Vec::new();
    for &assignment in assignments {
        let tangency_weight = assignment
            .tangency_weight_parameter_record_index
            .map(|record| {
                parameter(record, "TangencyWeight")
                    .filter(|parameter| parameter.unit().is_none())
                    .map(crate::records::parameters::DesignParameter::evaluated_value)
            })
            .map_or(Some(None), |value| value.map(Some))?;
        let radius = match &assignment.law {
            DesignFilletRadiusLaw::Constant {
                radius_parameter_record_index,
            } => RadiusSpec::Constant {
                radius: by_record
                    .get(radius_parameter_record_index)
                    .copied()
                    .filter(|parameter| parameter.is_fillet_radius())
                    .and_then(design_positive_length)?,
            },
            DesignFilletRadiusLaw::Chordal {
                chord_length_parameter_record_index,
            } => RadiusSpec::Chordal {
                chord_length: length(*chord_length_parameter_record_index, "ChordLen")?,
            },
            DesignFilletRadiusLaw::Asymmetric {
                offset_one_parameter_record_index,
                offset_two_parameter_record_index,
            } => RadiusSpec::Asymmetric {
                offset_one: length(*offset_one_parameter_record_index, "EdgeOffset1")?,
                offset_two: length(*offset_two_parameter_record_index, "EdgeOffset2")?,
            },
            DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index,
                end_radius_parameter_record_index,
                middle,
            } => {
                let start = parameter(*start_radius_parameter_record_index, "StartRadius")?;
                let end = parameter(*end_radius_parameter_record_index, "EndRadius")?;
                let mut controls = Vec::new();
                controls.push((0, start));
                controls.push((1, end));
                for (ordinal, row) in middle.iter().enumerate() {
                    let ordinal = u32::try_from(ordinal).ok()?;
                    let radius = parameter(row.radius_parameter_record_index, "MidRadius")?;
                    let position = parameter(row.parameter_record_index, "MidParams")?;
                    controls.push((ordinal, radius));
                    controls.push((ordinal, position));
                }
                let (points, _) = variable_fillet_law(&controls)?;
                RadiusSpec::Variable { points }
            }
        };
        resolved.push(ResolvedFilletAssignment {
            assignment,
            radius,
            tangency_weight,
        });
    }
    Some(resolved)
}

/// Project Fusion's role-`0x4` full-round face construction.
///
/// This form has no radius parameter. Its one compact member identifies the
/// center face; the trailing true flag requests automatic inference of both
/// side-face sets. A role-`0x4` group with any other shape remains available to
/// the regular feature projectors.
pub(super) fn project_full_round_fillet(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    owners: &[DesignParameterOwner],
    fillet_radius_groups: &[DesignFilletRadiusGroup],
    histories: &[crate::history_records::AsmHistory],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    let (group, operand) = or_none!(full_round_fillet_source(
        scope,
        construction_groups,
        face_operands,
        owners,
        fillet_radius_groups,
    ));
    if operand.resolved_face_slots.is_empty() {
        return Ok(None);
    }
    let center_faces = project_face_selection(scope, group, face_operands, histories)?;
    if matches!(center_faces, cadmpeg_ir::features::FaceSelection::Native(_)) {
        return Ok(None);
    }
    Ok(Some(cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::FullRoundFillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(or_none!(
                cadmpeg_ir::features::edge_treatments::FullRoundFilletGroup::new(
                    center_faces,
                    cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Automatic,
                    cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Automatic,
                )
                .ok()
            )),
        },
    )))
}

pub(super) fn project_variable_fillet(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        edge_treatments::{FilletGroup, RadiusSpec},
        FeatureDefinition, FeatureOperation,
    };

    let construction_groups = inputs.construction_groups;
    let edge_operands = inputs.edge_operands;
    let edge_identity_operands = inputs.edge_identity_operands;
    let edge_treatment_vertex_operands = inputs.edge_treatment_vertex_operands;
    let histories = inputs.histories;
    let stream = or_none!(native_stream(&scope.id));
    let mut groups = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let Some(group) = groups.next() else {
        return Ok(None);
    };
    if groups.next().is_some() {
        return Ok(None);
    }
    let (points, tangency_weight) = or_none!(variable_fillet_law(parameters));
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(FilletGroup {
                edges: resolved_edge_treatment_group_with_corners(
                    group,
                    crate::design::edge_resolve::EdgeTreatmentInputs {
                        groups: construction_groups,
                        operands: edge_operands,
                        identity_operands: edge_identity_operands,
                        vertex_operands: edge_treatment_vertex_operands,
                        histories,
                        previous_state_id: scope.previous_history_state_id(),
                        feature_id: &crate::ids::neutral_feature_id(scope),
                        treatment_radius: None,
                    },
                    ctx,
                    losses,
                )?,
                radius: RadiusSpec::Variable { points },
                tangency_weight,
            }),
        },
    )))
}

pub(super) fn fillet_law_parameter_records(
    law: &DesignFilletRadiusLaw,
) -> impl Iterator<Item = u32> + '_ {
    let (first, second, middle) = match law {
        DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index,
        } => (Some(*radius_parameter_record_index), None, None),
        DesignFilletRadiusLaw::Chordal {
            chord_length_parameter_record_index,
        } => (Some(*chord_length_parameter_record_index), None, None),
        DesignFilletRadiusLaw::Asymmetric {
            offset_one_parameter_record_index,
            offset_two_parameter_record_index,
        } => (
            Some(*offset_one_parameter_record_index),
            Some(*offset_two_parameter_record_index),
            None,
        ),
        DesignFilletRadiusLaw::Variable {
            start_radius_parameter_record_index,
            end_radius_parameter_record_index,
            middle,
        } => (
            Some(*start_radius_parameter_record_index),
            Some(*end_radius_parameter_record_index),
            Some(middle.as_slice()),
        ),
    };
    [first, second]
        .into_iter()
        .flatten()
        .chain(
            middle
                .into_iter()
                .flatten()
                .map(|row| row.radius_parameter_record_index),
        )
        .chain(
            middle
                .into_iter()
                .flatten()
                .map(|row| row.parameter_record_index),
        )
}

pub(super) fn project_chamfer(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    enum Lanes<'a> {
        Distance(&'a [&'a DesignParameter]),
        TwoDistances(&'a [&'a DesignParameter], &'a [&'a DesignParameter]),
        DistanceAngle(&'a [&'a DesignParameter], &'a [&'a DesignParameter]),
    }
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, ChamferSpec},
        FeatureDefinition, FeatureOperation,
    };

    let construction_groups = inputs.construction_groups;
    let edge_operands = inputs.edge_operands;
    let edge_identity_operands = inputs.edge_identity_operands;
    let edge_treatment_vertex_operands = inputs.edge_treatment_vertex_operands;
    let histories = inputs.histories;
    let native_scope = native_stream(&scope.id);
    let mut edge_groups = Vec::new();
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == native_scope
            && group.scope_record_index == scope.record_index
            && group.extrude_role().is_none()
    }) {
        edge_groups.push(group);
    }
    edge_groups.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });
    let ordered_parameters =
        |matches_kind: &dyn Fn(&str) -> bool| -> Result<Vec<&DesignParameter>, CodecError> {
            {
                let mut matches = Vec::new();
                for parameter in parameters
                    .iter()
                    .filter(|(_, parameter)| matches_kind(parameter.source_kind_str()))
                    .copied()
                {
                    matches.push(parameter);
                }
                matches.sort_by_key(|left| left.0);
                let mut out = Vec::new();
                for (_, parameter) in matches {
                    out.push(parameter);
                }
                Ok(out)
            }
        };
    let distances = ordered_parameters(&|kind| kind == "Distance")?;
    let first_distances = ordered_parameters(&|kind| kind == "Distance 1")?;
    let second_distances = ordered_parameters(&|kind| kind == "Distance 2")?;
    let left_distances = ordered_parameters(&|kind| kind == "leftDistance")?;
    let right_distances = ordered_parameters(&|kind| kind == "rightDistance")?;
    let angles =
        ordered_parameters(&|kind| matches!(kind, "Angle" | "Rotate Angle" | "rotateAngle"))?;
    // Face-role groups next to the one edge-role group add faces to that
    // selection. When the scope stores one specification, they all take it.
    let face_groups = edge_groups
        .iter()
        .filter(|group| group.role() == DesignOperandRole::ROLE_0X10)
        .count();
    let one_specification = [&distances, &first_distances, &left_distances]
        .into_iter()
        .map(Vec::len)
        .sum::<usize>()
        == 1;
    let shared_specification =
        one_specification && face_groups != 0 && edge_groups.len() - face_groups == 1;
    let group_count = if shared_specification {
        1
    } else {
        edge_groups.len().max(1)
    };
    // The specification is shared with one edge-role group. Any other role
    // keeps that reading and says so.
    let edge_role_assumed = shared_specification
        && edge_groups.iter().any(|group| {
            !matches!(
                group.role(),
                DesignOperandRole::ROLE_0X10 | DesignOperandRole::BODIES_B
            )
        });
    if !parameters.iter().all(|(_, parameter)| {
        matches!(
            parameter.source_kind_str(),
            "Distance"
                | "Distance 1"
                | "Distance 2"
                | "leftDistance"
                | "rightDistance"
                | "Angle"
                | "Rotate Angle"
                | "rotateAngle"
        )
    }) {
        return Ok(None);
    }

    let lanes = if !left_distances.is_empty() || !right_distances.is_empty() {
        if !distances.is_empty() || !first_distances.is_empty() || !second_distances.is_empty() {
            return Ok(None);
        }
        if right_distances.is_empty() && !angles.is_empty() {
            if left_distances.len() != group_count || angles.len() != group_count {
                return Ok(None);
            }
            Lanes::DistanceAngle(&left_distances, &angles)
        } else if right_distances.is_empty() {
            if left_distances.len() != group_count {
                return Ok(None);
            }
            Lanes::Distance(&left_distances)
        } else {
            if !angles.is_empty()
                || left_distances.len() != group_count
                || right_distances.len() != group_count
            {
                return Ok(None);
            }
            Lanes::TwoDistances(&left_distances, &right_distances)
        }
    } else if !first_distances.is_empty() || !second_distances.is_empty() {
        if !distances.is_empty()
            || !angles.is_empty()
            || first_distances.len() != group_count
            || second_distances.len() != group_count
        {
            return Ok(None);
        }
        Lanes::TwoDistances(&first_distances, &second_distances)
    } else if !angles.is_empty() {
        if distances.len() != group_count || angles.len() != group_count {
            return Ok(None);
        }
        Lanes::DistanceAngle(&distances, &angles)
    } else if !distances.is_empty() {
        if distances.len() != group_count {
            return Ok(None);
        }
        Lanes::Distance(&distances)
    } else {
        return Ok(None);
    };
    let mut candidates = Vec::new();
    for ordinal in 0..group_count {
        let spec = match lanes {
            Lanes::Distance(distance) => design_positive_length(distance[ordinal])
                .map(|distance| ChamferSpec::Distance { distance }),
            Lanes::TwoDistances(first, second) => design_positive_length(first[ordinal])
                .zip(design_positive_length(second[ordinal]))
                .map(|(first, second)| ChamferSpec::TwoDistances { first, second }),
            Lanes::DistanceAngle(distance, angle) => design_positive_length(distance[ordinal])
                .zip(
                    design_angle(angle[ordinal])
                        .and_then(|value| cadmpeg_ir::scalar::InteriorAngle::try_from(value).ok()),
                )
                .map(|(distance, angle)| ChamferSpec::DistanceAngle { distance, angle }),
        };
        let spec = or_none!(spec);
        candidates.push(spec);
    }
    for spec in &candidates {
        chamfer::note_orientation_assumptions(
            &format!("Chamfer scope {}", scope.id),
            *spec,
            losses,
        );
    }
    let mut groups = Vec::new();
    if edge_groups.is_empty() {
        groups.push(ChamferGroup {
            edges: chamfer::unresolved_selection(scope, None, losses),
            spec: candidates[0],
        });
    }
    for (ordinal, group) in edge_groups.into_iter().enumerate() {
        let spec = *or_none!(candidates.get(if group_count == 1 { 0 } else { ordinal }));
        let group = ChamferGroup {
            edges: resolved_edge_treatment_group_with_corners(
                group,
                crate::design::edge_resolve::EdgeTreatmentInputs {
                    groups: construction_groups,
                    operands: edge_operands,
                    identity_operands: edge_identity_operands,
                    vertex_operands: edge_treatment_vertex_operands,
                    histories,
                    previous_state_id: scope.previous_history_state_id(),
                    feature_id: &crate::ids::neutral_feature_id(scope),
                    treatment_radius: None,
                },
                ctx,
                losses,
            )?,
            spec,
        };
        groups.push(group);
    }
    let groups = or_none!(groups.try_into().ok());
    if edge_role_assumed {
        losses.push(crate::loss::F3dLossCode::ChamferEdgeRoleAssumed.note(format!(
            "Chamfer scope {} shares one specification between face groups and a group whose role is not the edge role; the group is read as the edge group.",
            scope.id
        )));
    }
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Chamfer {
            groups,
            flip_direction: false,
        },
    )))
}

pub(super) fn project_fixed_chamfer(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    edge_treatment_vertex_operands: &[DesignEdgeTreatmentVertexOperand],
    histories: &[crate::history_records::AsmHistory],
    (ctx, losses): (
        &DecodeContext<'_>,
        &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    ),
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, ChamferSpec},
        FeatureDefinition, FeatureOperation,
    };

    let fixed = or_none!(scope.fixed_chamfer_parameters());
    let stream = or_none!(native_stream(&scope.id));
    let mut groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.extrude_role().is_none()
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group.scope_reference_ordinal);
    // Face-role groups next to the one edge-role group take its fixed
    // specification, as in the parameterised projection.
    let face_groups = groups
        .iter()
        .filter(|group| group.role() == DesignOperandRole::ROLE_0X10)
        .count();
    let shared_specification = face_groups != 0 && groups.len() - face_groups == 1;
    if groups.len() > 1 && (scope.chamfer_sets().is_some() || !shared_specification) {
        groups.clear();
    }
    let edge_role_assumed = groups.len() > 1
        && groups.iter().any(|group| {
            !matches!(
                group.role(),
                DesignOperandRole::ROLE_0X10 | DesignOperandRole::BODIES_B
            )
        });
    let spec = match fixed {
        crate::records::feature::fixed_parameters::DesignFixedChamferParameters::EqualDistance { distance } => {
            ChamferSpec::Distance {
                distance: or_none!(cadmpeg_ir::scalar::PositiveLength::new(distance.value.get() * 10.0)),
            }
        }
        crate::records::feature::fixed_parameters::DesignFixedChamferParameters::TwoDistances { first, second } => {
            ChamferSpec::TwoDistances {
                first: or_none!(cadmpeg_ir::scalar::PositiveLength::new(first.value.get() * 10.0)),
                second: or_none!(cadmpeg_ir::scalar::PositiveLength::new(second.value.get() * 10.0)),
            }
        }
    };
    chamfer::note_orientation_assumptions(&format!("Chamfer scope {}", scope.id), spec, losses);
    let shared = groups.len() > 1;
    let mut projected = Vec::with_capacity(groups.len());
    for group in groups {
        let edges = resolved_edge_treatment_group_with_corners(
            group,
            crate::design::edge_resolve::EdgeTreatmentInputs {
                groups: construction_groups,
                operands: edge_operands,
                identity_operands: edge_identity_operands,
                vertex_operands: edge_treatment_vertex_operands,
                histories,
                previous_state_id: scope.previous_history_state_id(),
                feature_id: &crate::ids::neutral_feature_id(scope),
                treatment_radius: None,
            },
            ctx,
            losses,
        )?;
        if shared && matches!(edges, cadmpeg_ir::features::EdgeSelection::Native(_)) {
            losses.push(crate::loss::F3dLossCode::ChamferSelectionUnresolved.note(format!(
                "Chamfer {} retains native selection {} because the group of its shared fixed specification resolves to no input edges.",
                scope.id, group.id
            )));
        }
        projected.push(ChamferGroup { edges, spec });
    }
    if shared {
        losses.push(crate::loss::F3dLossCode::ChamferFixedFaceGroupAssumed.note(format!(
            "Chamfer scope {} applies its fixed scalar to each face-role group beside its edge-role group (DR-891).",
            scope.id
        )));
    }
    if edge_role_assumed {
        losses.push(crate::loss::F3dLossCode::ChamferEdgeRoleAssumed.note(format!(
            "Chamfer scope {} shares one specification between face groups and a group whose role is not the edge role; the group is read as the edge group.",
            scope.id
        )));
    }
    let groups = projected.try_into().unwrap_or_else(|_| {
        cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
            edges: chamfer::unresolved_selection(scope, None, losses),
            spec,
        })
    });
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Chamfer {
            groups,
            flip_direction: false,
        },
    )))
}
