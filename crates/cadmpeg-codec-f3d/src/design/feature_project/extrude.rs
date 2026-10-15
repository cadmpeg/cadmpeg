// SPDX-License-Identifier: Apache-2.0
//! Extrude profile, side, and termination projection.

use super::{design_length, feature_stream_matches};
use crate::design::face_resolve::{
    design_angle, extrude_omits_zero_side_one_offset, extrude_profile_group_roots,
    resolved_body_recipe_shape, resolved_extrude_profile_face_group, resolved_face_group,
    resolved_historical_face_group,
};
use crate::ids;
use crate::records::{
    feature::{
        extrude::{
            DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
        },
        fixed_parameters::DesignFixedExtrudeDistance,
        scope::DesignParameterScope,
    },
    parameters::DesignParameter,
    sketch_placement::DesignSketchPlacement,
    topology::{
        body_recipe::DesignBodyRecipeOperand,
        construction::DesignConstructionOperandGroup,
        extrude_selection::{DesignExtrudeFaceRole, DesignExtrudeOperandRole, DesignOperandRole},
        face::DesignFaceOperand,
    },
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

const EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E12: f64 = 1.0e-12;
const EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E9: f64 = 1.0e-9;
pub(super) fn project_extrude(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    placements: &[DesignSketchPlacement],
    body_recipe_operands: &[DesignBodyRecipeOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection,
        FeatureDefinition, FeatureOperation, LinearTermination, PlanarProfileRef, ProfileRef,
    };
    use cadmpeg_ir::scalar::{Angle, Length, NonZeroLength};
    // Per-side terminations without side-local modifiers; drafts and offsets
    // are attached below once they are resolved.
    enum ExtentShape {
        OneSided(LinearTermination),
        Symmetric(LinearTermination),
        TwoSided {
            first: LinearTermination,
            second: LinearTermination,
        },
    }
    #[derive(Clone, Copy)]
    enum AlongDirection {
        SignedDistance,
        PrologueReversal,
    }

    let mut scratch_storage = ctx.reserve_scoped(0, "F3D feature projection scratch")?;

    let supported_parameter = |source_kind: &str| {
        matches!(
            source_kind,
            "AlongDistance"
                | "AgainstDistance"
                | "ProfileOffset"
                | "Side1Offset"
                | "Side2Offset"
                | "TaperAngle"
                | "Side2TaperAngle"
        )
    };
    if ctx.any_by(
        parameters,
        |(_, parameter)| Ok(!supported_parameter(parameter.source_kind())),
        "f3d Extrude parameter kind source",
    )? {
        return Ok(None);
    }
    let scope_stream = ids::native_stream_charged(ctx, &scope.id, "f3d Extrude scope stream")?;
    let mut scope_groups = Vec::new();
    for group in ctx.admit_iter(construction_groups, "f3d project_extrude group source")? {
        if !feature_stream_matches(
            ctx,
            &group.id,
            scope_stream,
            "f3d project_extrude group stream",
        )? || group.scope_record_index != scope.record_index
        {
            continue;
        }
        scratch_storage
            .with_storage(|| ctx.push_vec(&mut scope_groups, group, "f3d Extrude scope group"))?;
    }
    let profile_groups = or_none!(extrude_profile_group_roots(
        ctx,
        scope,
        construction_groups
    )?);
    let prologue = or_none!(scope.extrude_prologue());
    let profile_ref = match scope.extrude_profile() {
        Some(profile) => {
            let placement = or_none!(ctx.find_by(
                placements,
                |placement| {
                    Ok(feature_stream_matches(
                        ctx,
                        &placement.id,
                        scope_stream,
                        "f3d Extrude placement stream",
                    )? && placement.entity_id == profile.entity_id)
                },
                "f3d Extrude placement source"
            )?);
            ProfileRef::Planar(PlanarProfileRef::Sketch(
                crate::design::identity::neutral_sketch_id(ctx, placement)?,
            ))
        }
        None => {
            let [first, rest @ ..] = profile_groups.as_slice() else {
                return Ok(None);
            };
            if rest.is_empty() {
                match resolved_extrude_profile_face_group(
                    ctx,
                    scope,
                    first,
                    construction_groups,
                    face_operands,
                )? {
                    Some(profile) => profile,
                    None => ProfileRef::Planar(PlanarProfileRef::Native(
                        ctx.copy_retained_text(&first.id, "f3d Extrude profile group id")?,
                    )),
                }
            } else {
                let mut state = None;
                let mut faces = Vec::new();
                let mut native = Vec::new();
                let mut complete = true;
                let mut profiles = profile_groups.iter();
                while let Some(group) =
                    ctx.next_charged(&mut profiles, "f3d Extrude selected profile source")?
                {
                    let Some(selection) = resolved_extrude_profile_face_group(
                        ctx,
                        scope,
                        group,
                        construction_groups,
                        face_operands,
                    )?
                    else {
                        complete = false;
                        break;
                    };
                    let ProfileRef::Planar(PlanarProfileRef::HistoricalFaces {
                        state: selected_state,
                        faces: selected_faces,
                        native: selected_native,
                    }) = selection
                    else {
                        complete = false;
                        break;
                    };
                    if match state.as_ref() {
                        Some(state) => !ctx.equal(
                            state,
                            &selected_state,
                            "f3d Extrude profile historical state comparison",
                        )?,
                        None => false,
                    } {
                        complete = false;
                        break;
                    }
                    state = Some(selected_state);
                    let mut selected_faces = selected_faces.into_iter();
                    while let Some(face) = ctx.next_charged(
                        &mut selected_faces,
                        "f3d Extrude selected historical face source",
                    )? {
                        if faces.is_empty()
                            || !ctx.any_by(
                                &faces,
                                |candidate: &cadmpeg_ir::ids::HistoricalFaceId| {
                                    ctx.equal_bytes(
                                        candidate.as_str().as_bytes(),
                                        face.as_str().as_bytes(),
                                        "compare F3D Extrude profile face identity",
                                    )
                                },
                                "visit F3D Extrude profile face membership",
                            )?
                        {
                            ctx.push_vec(&mut faces, face, "f3d Extrude profile historical face")?;
                        }
                    }
                    let mut selected_native = selected_native.into_iter();
                    while let Some(id) = ctx
                        .next_charged(&mut selected_native, "f3d Extrude selected native source")?
                    {
                        ctx.push_vec(&mut native, id, "f3d Extrude profile historical native id")?;
                    }
                }
                ProfileRef::Planar(match (complete, state) {
                    (true, Some(state)) if !faces.is_empty() => {
                        match PlanarProfileRef::historical_faces(state, faces, native, ctx)? {
                            Ok(profile) => profile,
                            Err(_) => PlanarProfileRef::Native(
                                ctx.copy_retained_text(&scope.id, "f3d Extrude fallback scope id")?,
                            ),
                        }
                    }
                    _ => PlanarProfileRef::Native(
                        ctx.copy_retained_text(&scope.id, "f3d Extrude fallback scope id")?,
                    ),
                })
            }
        }
    };
    let mut face_groups = Vec::new();
    for group in ctx
        .admit_iter(&scope_groups, "f3d Extrude group partition source")?
        .filter(|group| {
            group
                .extrude_role()
                .is_some_and(|role| matches!(role, DesignExtrudeOperandRole::Faces(_)))
        })
        .copied()
    {
        scratch_storage
            .with_storage(|| ctx.push_vec(&mut face_groups, group, "f3d Extrude face group"))?;
    }
    let unique = |source_kind: &str| -> Result<Option<Option<&DesignParameter>>, CodecError> {
        let mut source = parameters.iter().map(|(_, parameter)| *parameter);
        let mut matching = || {
            ctx.find_by(
                &mut source,
                |parameter| Ok(parameter.source_kind() == source_kind),
                "f3d Extrude parameter source",
            )
        };
        let first = matching()?;
        Ok(matching()?.is_none().then_some(first))
    };
    let parameter_along = match or_none!(unique("AlongDistance")?) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let fixed_along = scope
        .fixed_extrude_parameters()
        .as_ref()
        .and_then(|fixed| fixed.along_distance.as_ref())
        .map(|fixed| match fixed {
            DesignFixedExtrudeDistance::FixedScalar(scalar) => {
                Length::new(scalar.value.get() * 10.0)
                    .map(|length| (length, AlongDirection::SignedDistance))
                    .ok_or(())
            }
            DesignFixedExtrudeDistance::DistanceConstruction(scalar) => {
                Length::new(scalar.value.get() * 10.0)
                    .map(|length| (length, AlongDirection::PrologueReversal))
                    .ok_or(())
            }
        })
        .transpose()
        .ok();
    let fixed_along = or_none!(fixed_along);
    let along = match (parameter_along, fixed_along) {
        (Some(parameter), Some((fixed, AlongDirection::SignedDistance)))
            if (parameter.get() - fixed.get()).abs() <= EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E9 =>
        {
            Some((parameter, AlongDirection::SignedDistance))
        }
        (Some(distance), None) => Some((distance, AlongDirection::SignedDistance)),
        (None, Some(distance)) => Some(distance),
        (None, None) => None,
        _ => return Ok(None),
    };
    let against = match or_none!(unique("AgainstDistance")?) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let profile_offset = match or_none!(unique("ProfileOffset")?) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let side_one_offset = match or_none!(unique("Side1Offset")?) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let side_one_offset_count = usize::from(side_one_offset.is_some());
    let omitted_zero_side_one_offset = side_one_offset.is_none()
        && extrude_omits_zero_side_one_offset(scope, &prologue, side_one_offset_count);
    let face_side_one_offset =
        side_one_offset.or_else(|| omitted_zero_side_one_offset.then_some(Length::ZERO));
    let effective_side_one_offset = side_one_offset.filter(|offset| offset.get() != 0.0);
    let side_two_offset = match or_none!(unique("Side2Offset")?) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let effective_side_two_offset = side_two_offset.filter(|offset| offset.get() != 0.0);
    if effective_side_two_offset.is_some()
        && !matches!(
            scope
                .extrude_prologue()
                .and_then(DesignExtrudePrologue::extent),
            Some(
                DesignExtrudeExtent::TwoSidedToFaces | DesignExtrudeExtent::TwoSidedDistanceToFace,
            )
        )
    {
        return Ok(None);
    }
    let side_two_draft = match or_none!(unique("Side2TaperAngle")?) {
        Some(parameter) => Some(or_none!(design_angle(parameter))),
        None => None,
    };
    let mut start_groups = Vec::new();
    for group in ctx
        .admit_iter(&face_groups, "f3d Extrude face partition source")?
        .filter(|group| group.extrude_face_role() == Some(DesignExtrudeFaceRole::Start))
        .copied()
    {
        scratch_storage
            .with_storage(|| ctx.push_vec(&mut start_groups, group, "f3d Extrude start group"))?;
    }
    let mut termination_groups = Vec::new();
    for group in ctx
        .admit_iter(&face_groups, "f3d Extrude face partition source")?
        .filter(|group| group.extrude_face_role() == Some(DesignExtrudeFaceRole::Termination))
        .copied()
    {
        scratch_storage.with_storage(|| {
            ctx.push_vec(
                &mut termination_groups,
                group,
                "f3d Extrude termination group",
            )
        })?;
    }
    let first_side_target_ordinal = match prologue {
        DesignExtrudePrologue::ReferenceAware {
            first_side_target_ordinal,
            ..
        } => first_side_target_ordinal.map(|target| target.scope_reference_ordinal),
        DesignExtrudePrologue::LegacyDistance { .. }
        | DesignExtrudePrologue::ShiftedReferenceAware { .. }
        | DesignExtrudePrologue::LegacyShifted { .. } => None,
    };
    let mut target_shape_groups = Vec::new();
    for group in ctx
        .admit_iter(&scope_groups, "f3d Extrude group partition source")?
        .filter(|group| {
            group.role() == DesignOperandRole::ROLE_0X5
                && group.extrude_role().is_none()
                && group.extrude_face_role().is_none()
                && first_side_target_ordinal
                    .is_none_or(|ordinal| group.scope_reference_ordinal == ordinal)
        })
        .copied()
    {
        scratch_storage.with_storage(|| {
            ctx.push_vec(
                &mut target_shape_groups,
                group,
                "f3d Extrude target shape group",
            )
        })?;
    }
    if start_groups.len() + termination_groups.len() != face_groups.len() {
        return Ok(None);
    }
    let selected_face =
        |group: &DesignConstructionOperandGroup| -> Result<FaceSelection, CodecError> {
            if let Some(selection) = resolved_historical_face_group(
                ctx,
                scope,
                scope.previous_history_state_id(),
                group,
                face_operands,
            )? {
                return Ok(selection);
            }
            match resolved_face_group(ctx, group, face_operands)? {
                Some(selection) => Ok(selection),
                None => Ok(FaceSelection::Native(ctx.copy_retained_text(
                    &group.id,
                    "f3d Extrude selected face group id",
                )?)),
            }
        };
    let start = match prologue.start() {
        DesignExtrudeStart::ProfilePlane if start_groups.is_empty() => {
            if profile_offset.is_some() {
                return Ok(None);
            }
            ExtrudeStart::ProfilePlane {}
        }
        DesignExtrudeStart::OffsetProfilePlane if start_groups.is_empty() => {
            ExtrudeStart::OffsetProfilePlane {
                offset: or_none!(profile_offset),
            }
        }
        DesignExtrudeStart::FromFace => {
            let [start] = start_groups.as_slice() else {
                return Ok(None);
            };
            let offset = or_none!(profile_offset);
            ExtrudeStart::FromFace {
                face: selected_face(start)?,
                offset: (offset.get() != 0.0).then_some(offset),
            }
        }
        _ => return Ok(None),
    };
    // A zero distance admits no nonzero length. Its `Some(None)` matches no
    // distance arm and no arm that requires an absent distance, so the final
    // arm refuses it.
    let along = along.map(|(along, direction)| (NonZeroLength::try_from(along).ok(), direction));
    let against = against.map(|against| NonZeroLength::try_from(against).ok());
    let (shape, reverse_direction) = match (or_none!(prologue.extent()), along, against) {
        (DesignExtrudeExtent::OneSidedDistance, Some((Some(along), along_direction)), None)
            if (matches!(along_direction, AlongDirection::PrologueReversal)
                || !prologue.direction_reversed())
                && termination_groups.is_empty()
                && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::OneSided(LinearTermination::Blind {
                    length: NonZeroLength::from(along.abs()),
                }),
                match along_direction {
                    AlongDirection::SignedDistance => along.get() < 0.0,
                    AlongDirection::PrologueReversal => prologue.direction_reversed(),
                },
            )
        }
        (
            DesignExtrudeExtent::TwoSidedDistance,
            Some((Some(along), AlongDirection::SignedDistance)),
            Some(Some(against)),
        ) if !prologue.direction_reversed()
            && termination_groups.is_empty()
            && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::TwoSided {
                    first: LinearTermination::Blind {
                        length: NonZeroLength::from(along.abs()),
                    },
                    second: LinearTermination::Blind {
                        length: NonZeroLength::from(against.abs()),
                    },
                },
                along.get() < 0.0,
            )
        }
        (
            DesignExtrudeExtent::TwoSidedDistanceToFace,
            Some((Some(along), AlongDirection::SignedDistance)),
            None,
        ) if !prologue.direction_reversed()
            && termination_groups.len() == 1
            && target_shape_groups.is_empty()
            && effective_side_one_offset.is_none()
            && side_two_offset.is_some() =>
        {
            let [termination] = termination_groups.as_slice() else {
                return Ok(None);
            };
            (
                ExtentShape::TwoSided {
                    first: LinearTermination::Blind {
                        length: NonZeroLength::from(along.abs()),
                    },
                    second: LinearTermination::ToFace {
                        face: selected_face(termination)?,
                        offset: side_two_offset.filter(|offset| offset.get() != 0.0),
                    },
                },
                along.get() < 0.0,
            )
        }
        (DesignExtrudeExtent::TwoSidedToFaces, None, None)
            if termination_groups.len() == 2
                && target_shape_groups.is_empty()
                && side_one_offset.is_some()
                && side_two_offset.is_some() =>
        {
            let [first, second] = termination_groups.as_slice() else {
                return Ok(None);
            };
            (
                ExtentShape::TwoSided {
                    first: LinearTermination::ToFace {
                        face: selected_face(first)?,
                        offset: side_one_offset.filter(|offset| offset.get() != 0.0),
                    },
                    second: LinearTermination::ToFace {
                        face: selected_face(second)?,
                        offset: side_two_offset.filter(|offset| offset.get() != 0.0),
                    },
                },
                prologue.direction_reversed(),
            )
        }
        (
            DesignExtrudeExtent::SymmetricDistance,
            Some((Some(along), AlongDirection::SignedDistance)),
            None,
        ) if !prologue.direction_reversed()
            && termination_groups.is_empty()
            && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::Symmetric(LinearTermination::Blind {
                    length: NonZeroLength::from(along.abs()),
                }),
                along.get() < 0.0,
            )
        }
        (DesignExtrudeExtent::SymmetricThroughAll, None, None)
            if !prologue.direction_reversed()
                && termination_groups.is_empty()
                && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::Symmetric(LinearTermination::ThroughAll {}),
                false,
            )
        }
        (DesignExtrudeExtent::OneSidedToFace, None, None) => {
            match (
                termination_groups.as_slice(),
                target_shape_groups.as_slice(),
            ) {
                ([termination], []) => {
                    let offset = or_none!(face_side_one_offset);
                    (
                        ExtentShape::OneSided(LinearTermination::ToFace {
                            face: selected_face(termination)?,
                            offset: (offset.get() != 0.0).then_some(offset),
                        }),
                        prologue.direction_reversed(),
                    )
                }
                ([], [target]) if effective_side_one_offset.is_none() => {
                    let target =
                        match resolved_body_recipe_shape(ctx, scope, target, body_recipe_operands)?
                        {
                            Some(selection) => selection,
                            None => FaceSelection::Native(ctx.copy_retained_text(
                                &target.id,
                                "f3d Extrude target shape group id",
                            )?),
                        };
                    (
                        ExtentShape::OneSided(LinearTermination::ToShape { target }),
                        prologue.direction_reversed(),
                    )
                }
                _ => return Ok(None),
            }
        }
        (DesignExtrudeExtent::OneSidedThroughNext, None, None)
            if termination_groups.is_empty() && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::OneSided(LinearTermination::ThroughNext {}),
                prologue.direction_reversed(),
            )
        }
        (DesignExtrudeExtent::OneSidedThroughAll, None, None)
            if termination_groups.is_empty() && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::OneSided(LinearTermination::ThroughAll {}),
                prologue.direction_reversed(),
            )
        }
        _ => return Ok(None),
    };
    let direction = if reverse_direction {
        ExtrudeDirection::ReversedProfileNormal {}
    } else {
        ExtrudeDirection::ProfileNormal {}
    };
    let parameter_draft = match or_none!(unique("TaperAngle")?) {
        Some(parameter) => {
            let angle = or_none!(design_angle(parameter));
            Some(angle)
        }
        None => None,
    };
    let fixed_draft = scope
        .fixed_extrude_parameters()
        .as_ref()
        .and_then(|fixed| fixed.taper_angle.as_ref())
        .map(|fixed| Angle::from_assigned_real(fixed.value));
    let draft = match (parameter_draft, fixed_draft) {
        (Some(parameter), Some(fixed))
            if (parameter.get() - fixed.get()).abs() <= EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E12 =>
        {
            Some(parameter)
        }
        (Some(angle), None) | (None, Some(angle)) => Some(angle),
        (None, None) => None,
        _ => return Ok(None),
    }
    .filter(|angle| angle.get() != 0.0)
    .map(cadmpeg_ir::scalar::SlopeAngle::try_from)
    .transpose()
    .ok();
    let draft = or_none!(draft);
    let second_draft = side_two_draft
        .filter(|angle| angle.get() != 0.0)
        .map(cadmpeg_ir::scalar::SlopeAngle::try_from)
        .transpose()
        .ok();
    let second_draft = or_none!(second_draft);
    // A side-two draft requires a two-sided extent. Other extents have no neutral
    // field for it, so return None.
    if second_draft.is_some() && !matches!(shape, ExtentShape::TwoSided { .. }) {
        return Ok(None);
    }
    let extent = match shape {
        ExtentShape::OneSided(termination) => ExtrudeExtent::OneSided {
            side: ExtrudeSide { termination, draft },
        },
        ExtentShape::Symmetric(termination) => ExtrudeExtent::Symmetric {
            side: ExtrudeSide { termination, draft },
        },
        ExtentShape::TwoSided { first, second } => ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: first,
                draft,
            },
            second: ExtrudeSide {
                termination: second,
                draft: second_draft,
            },
        },
    };
    let has_body_operands = ctx.any_by(
        &scope_groups,
        |group| Ok(group.extrude_role() == Some(DesignExtrudeOperandRole::Bodies)),
        "f3d Extrude body role source",
    )?;
    let op = match (prologue.operation(), has_body_operands) {
        (DesignExtrudeOperation::Join, true) => BooleanOp::Join,
        (DesignExtrudeOperation::Cut, true) => BooleanOp::Cut,
        (DesignExtrudeOperation::Intersect, true) => BooleanOp::Intersect,
        (DesignExtrudeOperation::NewBody, false) => BooleanOp::NewBody,
        _ => return Ok(None),
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Extrude {
            profile: profile_ref,
            direction,
            start,
            extent,
            op,
            solid: Some(prologue.solid_operation()),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
    )))
}

#[cfg(test)]
pub(super) mod test_support;

#[cfg(test)]
mod tests;
