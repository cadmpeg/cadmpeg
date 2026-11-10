// SPDX-License-Identifier: Apache-2.0
//! `Extrude` scope projection.

use super::{
    design_length, selected_work_planes, unique_feature_match, work_plane_support_frame,
    EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E12, EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E9,
};
use crate::design::face_resolve::design_angle;
use crate::design::face_resolve::extrude_omits_zero_side_one_offset;
use crate::design::face_resolve::extrude_profile_group_roots;
use crate::design::face_resolve::resolved_body_recipe_shape;
use crate::design::face_resolve::resolved_extrude_profile_face_group;
use crate::design::face_resolve::resolved_face_group;
use crate::design::face_resolve::resolved_historical_face_group;
use crate::ids::native_stream;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::extrude::DesignExtrudeStart;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeDistance;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::topology::body_recipe::DesignBodyRecipeOperand;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignExtrudeFaceRole;
use crate::records::topology::extrude_selection::DesignExtrudeOperandRole;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Replace each zero-offset native start or termination face group of a
/// projected `Extrude` that selects one `WorkPlane` with that plane.
pub(super) fn bind_extrude_work_plane_targets(
    definition: &mut cadmpeg_ir::features::FeatureDefinition,
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    use cadmpeg_ir::features::{
        ExtrudeExtent, ExtrudeStart, FaceSelection, FeatureDefinition, FeatureOperation,
        LinearTermination, PlaneTargetSource,
    };

    let FeatureDefinition::Operation(FeatureOperation::Extrude { start, extent, .. }) = definition
    else {
        return;
    };
    let stream = native_stream(&scope.id);
    let work_plane_target = |native: &str| {
        let group = unique_feature_match(construction_groups.iter().filter(|group| {
            group.id == native
                && group.scope_record_index == scope.record_index
                && native_stream(&group.id) == stream
        }))?;
        let planes = selected_work_planes(scope, group, entity_selection_operands, scopes)?;
        let [plane] = planes.as_slice() else {
            return None;
        };
        let frame = work_plane_support_frame(plane.work_plane_transform()?.into())?;
        Some((
            frame,
            cadmpeg_core::text::NonBlankString::new(native.to_owned())?,
        ))
    };
    if let ExtrudeStart::FromFace {
        face: FaceSelection::Native(native),
        offset: None,
    } = start
    {
        if let Some((plane, native)) = work_plane_target(native) {
            *start = ExtrudeStart::FromPlane {
                plane,
                native,
                source: Some(PlaneTargetSource::WorkPlane),
            };
        }
    }
    let sides = match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => vec![side],
        ExtrudeExtent::TwoSided { first, second } => vec![first, second],
    };
    for side in sides {
        if let LinearTermination::ToFace {
            face: FaceSelection::Native(native),
            offset: None,
        } = &side.termination
        {
            if let Some((plane, native)) = work_plane_target(native) {
                side.termination = LinearTermination::ToPlane {
                    plane,
                    native,
                    source: Some(PlaneTargetSource::WorkPlane),
                };
            }
        }
    }
}

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
                | "Side1WallThickness"
                | "Side2WallThickness"
        )
    };
    if parameters
        .iter()
        .any(|(_, parameter)| !supported_parameter(parameter.source_kind_str()))
    {
        return Ok(None);
    }
    let mut scope_groups = Vec::new();
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == native_stream(&scope.id)
            && group.scope_record_index == scope.record_index
    }) {
        scope_groups.push(group);
    }
    let profile_groups = or_none!(extrude_profile_group_roots(
        ctx,
        scope,
        construction_groups
    )?);
    let prologue = or_none!(scope.extrude_prologue());
    let profile_ref = if scope.extrude_native_profiles().is_some() {
        ProfileRef::Planar(PlanarProfileRef::Native(scope.id.clone()))
    } else if !prologue.solid_operation()
        && scope.extrude_profile().is_none()
        && profile_groups.is_empty()
    {
        let group = or_none!(unique_feature_match(scope_groups.iter().copied().filter(
            |group| {
                group.role() == DesignOperandRole::ROLE_0X43
                    && !group.members().is_empty()
                    && usize::try_from(group.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                        == Some(&group.record_index)
            }
        )));
        ProfileRef::Planar(PlanarProfileRef::Native(group.id.clone()))
    } else {
        match scope.extrude_profile() {
            Some(profile) => {
                match placements.iter().find(|placement| {
                    native_stream(&placement.id) == native_stream(&scope.id)
                        && placement.entity_id == profile.entity_id
                }) {
                    Some(placement) => ProfileRef::Planar(PlanarProfileRef::Sketch(
                        crate::ids::neutral_sketch_id(placement),
                    )),
                    None => ProfileRef::Planar(PlanarProfileRef::Native(scope.id.clone())),
                }
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
                        None => ProfileRef::Planar(PlanarProfileRef::Native(first.id.clone())),
                    }
                } else {
                    let mut state = None;
                    let mut faces = Vec::new();
                    let mut native = Vec::new();
                    let mut complete = true;
                    for group in &profile_groups {
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
                        if state.as_ref().is_some_and(|state| state != &selected_state) {
                            complete = false;
                            break;
                        }
                        state = Some(selected_state);
                        for face in selected_faces {
                            if !faces.contains(&face) {
                                faces.push(face);
                            }
                        }
                        for id in selected_native {
                            native.push(id);
                        }
                    }
                    ProfileRef::Planar(match (complete, state) {
                        (true, Some(state)) if !faces.is_empty() => {
                            match PlanarProfileRef::historical_faces(state, faces, native) {
                                Ok(profile) => profile,
                                Err(_) => PlanarProfileRef::Native(scope.id.clone()),
                            }
                        }
                        _ => PlanarProfileRef::Native(scope.id.clone()),
                    })
                }
            }
        }
    };
    let mut face_groups = Vec::new();
    for group in scope_groups
        .iter()
        .filter(|group| {
            group
                .extrude_role()
                .is_some_and(|role| matches!(role, DesignExtrudeOperandRole::Faces(_)))
        })
        .copied()
    {
        face_groups.push(group);
    }
    let unique = |source_kind: &str| {
        let mut matches = parameters
            .iter()
            .map(|(_, parameter)| *parameter)
            .filter(|parameter| parameter.source_kind() == source_kind);
        let first = matches.next();
        matches.next().is_none().then_some(first)
    };
    let parameter_along = match or_none!(unique("AlongDistance")) {
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
    let assumed_fixed_symmetric = parameter_along.is_none()
        && fixed_along.is_some()
        && prologue.extent() == Some(DesignExtrudeExtent::SymmetricDistance)
        && prologue.distance_tail().is_none();
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
    let parameter_against = match or_none!(unique("AgainstDistance")) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let fixed_against = scope
        .fixed_extrude_parameters()
        .as_ref()
        .and_then(|fixed| fixed.against_distance.as_ref())
        .map(|fixed| Length::new(fixed.value.get() * 10.0).ok_or(()))
        .transpose()
        .ok();
    let fixed_against = or_none!(fixed_against);
    let against = match (parameter_against, fixed_against) {
        (Some(parameter), Some(fixed))
            if (parameter.get() - fixed.get()).abs() <= EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E9 =>
        {
            Some(parameter)
        }
        (Some(distance), None) | (None, Some(distance)) => Some(distance),
        (None, None) => None,
        _ => return Ok(None),
    };
    let fixed_length =
        |lane: Option<&crate::records::feature::fixed_parameters::DesignFixedExtrudeScalar>| {
            lane.map(|lane| Length::new(lane.value.get() * 10.0).ok_or(()))
                .transpose()
                .ok()
        };
    let fixed_lanes = scope.fixed_extrude_parameters();
    // When a parameter and a fixed lane both carry an offset, they agree.
    let offset = |parameter: Option<Length>, lane: Option<Length>| match (parameter, lane) {
        (Some(parameter), Some(lane))
            if (parameter.get() - lane.get()).abs() > EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E9 =>
        {
            Err(())
        }
        (Some(value), _) | (None, Some(value)) => Ok(Some(value)),
        (None, None) => Ok(None),
    };
    let parameter_profile_offset = match or_none!(unique("ProfileOffset")) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let profile_offset = or_none!(offset(
        parameter_profile_offset,
        or_none!(fixed_length(
            fixed_lanes.and_then(|fixed| fixed.profile_offset.as_ref())
        )),
    )
    .ok());
    let parameter_side_one_offset = match or_none!(unique("Side1Offset")) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let side_one_offset = or_none!(offset(
        parameter_side_one_offset,
        or_none!(fixed_length(
            fixed_lanes.and_then(|fixed| fixed.side_one_offset.as_ref())
        )),
    )
    .ok());
    let side_one_offset_count = parameters
        .iter()
        .filter(|(_, parameter)| parameter.source_kind() == "Side1Offset")
        .count();
    let omitted_zero_side_one_offset = side_one_offset.is_none()
        && extrude_omits_zero_side_one_offset(scope, &prologue, side_one_offset_count);
    let face_side_one_offset =
        side_one_offset.or_else(|| omitted_zero_side_one_offset.then_some(Length::ZERO));
    let effective_side_one_offset = side_one_offset.filter(|offset| offset.get() != 0.0);
    let parameter_side_two_offset = match or_none!(unique("Side2Offset")) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let side_two_offset = or_none!(offset(
        parameter_side_two_offset,
        or_none!(fixed_length(
            fixed_lanes.and_then(|fixed| fixed.side_two_offset.as_ref())
        )),
    )
    .ok());
    let effective_side_two_offset = side_two_offset.filter(|offset| offset.get() != 0.0);
    if effective_side_two_offset.is_some()
        && !matches!(
            scope
                .extrude_prologue()
                .and_then(DesignExtrudePrologue::extent),
            Some(
                DesignExtrudeExtent::TwoSidedToFaces
                    | DesignExtrudeExtent::TwoSidedDistanceToFace
                    | DesignExtrudeExtent::TwoSidedDistanceToFaceControlOne
                    | DesignExtrudeExtent::TwoSidedToTargetsOptionZero
                    | DesignExtrudeExtent::TwoSidedToTargetsOptionTwo
                    | DesignExtrudeExtent::TwoSidedThroughAllToFace,
            )
        )
    {
        return Ok(None);
    }
    let parameter_side_two_draft = match or_none!(unique("Side2TaperAngle")) {
        Some(parameter) => Some(or_none!(design_angle(parameter))),
        None => None,
    };
    let fixed_side_two_draft = scope
        .fixed_extrude_parameters()
        .as_ref()
        .and_then(|fixed| fixed.side_two_taper_angle.as_ref())
        .map(|fixed| Angle::from_assigned_real(fixed.value));
    let side_two_draft = match (parameter_side_two_draft, fixed_side_two_draft) {
        (Some(parameter), Some(fixed))
            if (parameter.get() - fixed.get()).abs() <= EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E12 =>
        {
            Some(parameter)
        }
        (Some(angle), None) | (None, Some(angle)) => Some(angle),
        (None, None) => None,
        _ => return Ok(None),
    };
    let mut start_groups = Vec::new();
    for group in face_groups
        .iter()
        .filter(|group| group.extrude_face_role() == Some(DesignExtrudeFaceRole::Start))
        .copied()
    {
        start_groups.push(group);
    }
    let mut termination_groups = Vec::new();
    for group in face_groups
        .iter()
        .filter(|group| group.extrude_face_role() == Some(DesignExtrudeFaceRole::Termination))
        .copied()
    {
        termination_groups.push(group);
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
    for group in scope_groups
        .iter()
        .filter(|group| {
            group.role() == DesignOperandRole::ROLE_0X5
                && group.extrude_role().is_none()
                && group.extrude_face_role().is_none()
                && first_side_target_ordinal
                    .is_none_or(|ordinal| group.scope_reference_ordinal == ordinal)
        })
        .copied()
    {
        target_shape_groups.push(group);
    }
    if start_groups.len() + termination_groups.len() != face_groups.len() {
        return Ok(None);
    }
    let selected_face =
        |group: &DesignConstructionOperandGroup| -> Result<FaceSelection, CodecError> {
            if let Some(selection) = resolved_historical_face_group(
                scope,
                scope.previous_history_state_id(),
                group,
                face_operands,
            ) {
                return Ok(selection);
            }
            match resolved_face_group(group, face_operands)? {
                Some(selection) => Ok(selection),
                None => Ok(FaceSelection::Native(group.id.clone())),
            }
        };
    let selected_target = |group: &DesignConstructionOperandGroup,
                           offset: Length|
     -> Result<Option<LinearTermination>, CodecError> {
        if group.extrude_face_role() == Some(DesignExtrudeFaceRole::Termination) {
            return Ok(Some(LinearTermination::ToFace {
                face: selected_face(group)?,
                offset: (offset.get() != 0.0).then_some(offset),
            }));
        }
        if offset.get() != 0.0 {
            return Ok(None);
        }
        let target = resolved_body_recipe_shape(scope, group, body_recipe_operands)
            .unwrap_or_else(|| FaceSelection::Native(group.id.clone()));
        Ok(Some(LinearTermination::ToShape { target }))
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
        DesignExtrudeStart::FromFace
            if start_groups.is_empty()
                && termination_groups.len() == 1
                && prologue.extent() == Some(DesignExtrudeExtent::OneSidedToFace) =>
        {
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
    let first_side_zero = matches!(
        (prologue.extent(), along, against),
        (
            Some(DesignExtrudeExtent::TwoSidedDistance),
            Some((None, AlongDirection::SignedDistance)),
            Some(Some(_)),
        )
    );
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
            Some((along, AlongDirection::SignedDistance)),
            Some(against),
        ) if !prologue.direction_reversed()
            && termination_groups.is_empty()
            && effective_side_one_offset.is_none() =>
        {
            let blind = |length: NonZeroLength| LinearTermination::Blind {
                length: NonZeroLength::from(length.abs()),
            };
            match (along, against) {
                (Some(along), Some(against)) => (
                    ExtentShape::TwoSided {
                        first: blind(along),
                        second: blind(against),
                    },
                    along.get() < 0.0,
                ),
                (Some(along), None) => (ExtentShape::OneSided(blind(along)), along.get() < 0.0),
                (None, Some(against)) => (ExtentShape::OneSided(blind(against)), true),
                (None, None) => return Ok(None),
            }
        }
        (
            DesignExtrudeExtent::TwoSidedDistanceThroughAll,
            Some((Some(along), AlongDirection::SignedDistance)),
            None,
        ) if !prologue.direction_reversed()
            && termination_groups.is_empty()
            && effective_side_one_offset.is_none()
            && effective_side_two_offset.is_none() =>
        {
            (
                ExtentShape::TwoSided {
                    first: LinearTermination::Blind {
                        length: NonZeroLength::from(along.abs()),
                    },
                    second: LinearTermination::ThroughAll {},
                },
                along.get() < 0.0,
            )
        }
        (
            DesignExtrudeExtent::TwoSidedDistanceToFace
            | DesignExtrudeExtent::TwoSidedDistanceToFaceControlOne,
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
        (DesignExtrudeExtent::TwoSidedThroughAllToFace, None, None)
            if termination_groups.len() == 1
                && effective_side_one_offset.is_none()
                && side_two_offset.is_some() =>
        {
            let [termination] = termination_groups.as_slice() else {
                return Ok(None);
            };
            (
                ExtentShape::TwoSided {
                    first: LinearTermination::ThroughAll {},
                    second: LinearTermination::ToFace {
                        face: selected_face(termination)?,
                        offset: side_two_offset.filter(|offset| offset.get() != 0.0),
                    },
                },
                prologue.direction_reversed(),
            )
        }
        (
            DesignExtrudeExtent::TwoSidedTargetToDistanceOptionZero,
            Some((distance, AlongDirection::SignedDistance)),
            None,
        )
        | (DesignExtrudeExtent::TwoSidedTargetToDistanceOptionZero, None, Some(distance))
            if termination_groups.len() + target_shape_groups.len() == 1
                && side_one_offset.is_some()
                && effective_side_two_offset.is_none() =>
        {
            let target = termination_groups
                .first()
                .or_else(|| target_shape_groups.first())
                .copied();
            let first = or_none!(selected_target(
                or_none!(target),
                or_none!(side_one_offset)
            )?);
            let shape = match distance {
                Some(length) => ExtentShape::TwoSided {
                    first,
                    second: LinearTermination::Blind {
                        length: NonZeroLength::from(length.abs()),
                    },
                },
                None => ExtentShape::OneSided(first),
            };
            (shape, prologue.direction_reversed())
        }
        (
            DesignExtrudeExtent::TwoSidedToTargetsOptionZero
            | DesignExtrudeExtent::TwoSidedToTargetsOptionTwo,
            None,
            None,
        ) if termination_groups.len() + target_shape_groups.len() == 2
            && side_one_offset.is_some()
            && side_two_offset.is_some() =>
        {
            let mut targets = termination_groups.clone();
            targets.extend(target_shape_groups.iter().copied());
            targets.sort_by_key(|group| group.scope_reference_ordinal);
            let [first, second] = targets.as_slice() else {
                return Ok(None);
            };
            let first = or_none!(selected_target(first, or_none!(side_one_offset))?);
            let second = or_none!(selected_target(second, or_none!(side_two_offset))?);
            (
                ExtentShape::TwoSided { first, second },
                prologue.direction_reversed(),
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
            let each_side = assumed_fixed_symmetric
                || match prologue.distance_tail().map(|tail| tail.measurement) {
                    Some(crate::records::feature::extrude::DesignExtrudeMeasurement::EachSide) => {
                        true
                    }
                    Some(_) => false,
                    None => prologue.side_extent_discriminators() == Some([1, 1]),
                };
            let length = if each_side {
                or_none!(NonZeroLength::new(2.0 * along.get()))
            } else {
                along
            };
            (
                ExtentShape::Symmetric(LinearTermination::Blind {
                    length: NonZeroLength::from(length.abs()),
                }),
                along.get() < 0.0,
            )
        }
        (DesignExtrudeExtent::TwoSidedThroughAll, None, None)
            if crate::design::decode::scopes::extrude::through_all::taper_parameters_match(
                scope,
                parameters.iter().map(|(_, parameter)| *parameter),
            ) && !prologue.direction_reversed()
                && termination_groups.is_empty()
                && effective_side_one_offset.is_none()
                && side_two_offset.is_none_or(|offset| offset.get() == 0.0) =>
        {
            (
                ExtentShape::TwoSided {
                    first: LinearTermination::ThroughAll {},
                    second: LinearTermination::ThroughAll {},
                },
                false,
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
                        match resolved_body_recipe_shape(scope, target, body_recipe_operands) {
                            Some(selection) => selection,
                            None => FaceSelection::Native(target.id.clone()),
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
    let parameter_draft = match or_none!(unique("TaperAngle")) {
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
    let (draft, second_draft) = if first_side_zero {
        (second_draft, None)
    } else {
        (draft, second_draft)
    };
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
    let has_body_operands = scope_groups
        .iter()
        .any(|group| group.extrude_role() == Some(DesignExtrudeOperandRole::Bodies));
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
