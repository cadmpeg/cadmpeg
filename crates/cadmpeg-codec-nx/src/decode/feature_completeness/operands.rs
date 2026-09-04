// SPDX-License-Identifier: Apache-2.0
//! Operand and selection completeness predicates.

use super::{
    finite_feature_point, positive_feature_length, unit_feature_direction, valid_draft_angle,
    valid_feature_direction,
};
use cadmpeg_ir::features::{
    AngularTermination, BodySelection, BooleanOp, ChamferSpec, EdgeSelection, ExtrudeExtent,
    ExtrudeStart, FaceSelection, FeatureId, HoleKind, Length, LinearTermination, LoftPointSection,
    LoftSection, PathRef, PatternKind, ProfileRef, RadiusSpec, RevolutionConstruction,
    RevolveExtent, RibConstruction, RibDraft, SweepMode, SweepOrientation, VertexSelection,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::BTreeSet;

/// Non-zero hole-axis direction acceptance.
const EPS_NONZERO_HOLE_DIRECTION: f64 = 1.0e-12;

pub(crate) fn hole_feature_is_incomplete(
    profile: Option<&ProfileRef>,
    face: Option<&FaceSelection>,
    placements: Option<&[cadmpeg_ir::features::HolePlacement]>,
    treatments: (&HoleKind, Option<&HoleKind>),
    diameter: Option<Length>,
    extent: Option<&LinearTermination>,
) -> bool {
    let (kind, exit_kind) = treatments;
    let profile_incomplete = profile.is_some_and(profile_ref_is_incomplete);
    let face_incomplete = face.is_some_and(face_selection_is_incomplete);
    let finite_point =
        |point: Point3| point.x.is_finite() && point.y.is_finite() && point.z.is_finite();
    let finite_direction = |vector: Vector3| {
        vector.x.is_finite()
            && vector.y.is_finite()
            && vector.z.is_finite()
            && vector.norm() > EPS_NONZERO_HOLE_DIRECTION
    };
    let axis_is_direction_invariant = matches!(extent, Some(LinearTermination::ThroughAll))
        && exit_kind.is_none_or(|exit| exit == kind);
    let placements_complete = placements.is_some_and(|placements| {
        !placements.is_empty()
            && !placements
                .iter()
                .enumerate()
                .any(|(index, placement)| placements[index + 1..].contains(placement))
            && placements.iter().all(|placement| match placement {
                cadmpeg_ir::features::HolePlacement::Directed {
                    position,
                    direction,
                } => finite_point(*position) && finite_direction(*direction),
                cadmpeg_ir::features::HolePlacement::Axis { origin, axis } => {
                    axis_is_direction_invariant && finite_point(*origin) && finite_direction(*axis)
                }
            })
    });
    let placements_incomplete = placements.is_some() && !placements_complete;
    let location_unresolved = !placements_complete && profile.is_none_or(profile_ref_is_incomplete);
    let orientation_unresolved =
        !placements_complete && face.is_none_or(face_selection_is_incomplete);
    profile_incomplete
        || face_incomplete
        || placements_incomplete
        || location_unresolved
        || orientation_unresolved
        || hole_kind_is_incomplete(kind, diameter)
        || exit_kind.is_some_and(|kind| hole_kind_is_incomplete(kind, diameter))
        || diameter.is_none_or(|diameter| !positive_feature_length(diameter))
        || extent.is_none_or(termination_is_incomplete)
}

pub(crate) fn hole_kind_is_incomplete(kind: &HoleKind, bore_diameter: Option<Length>) -> bool {
    let valid_angle = |angle: cadmpeg_ir::features::Angle| {
        angle.0.is_finite() && angle.0 > 0.0 && angle.0 < std::f64::consts::PI
    };
    let treatment_diameter_is_incomplete = |diameter: Length| {
        !positive_feature_length(diameter) || bore_diameter.is_none_or(|bore| diameter.0 <= bore.0)
    };
    match kind {
        HoleKind::Unresolved(_)
        | HoleKind::PartialCounterbore { .. }
        | HoleKind::PartialCountersink { .. } => true,
        HoleKind::Simple => false,
        HoleKind::Chamfer { diameter, angle } | HoleKind::Countersink { diameter, angle } => {
            treatment_diameter_is_incomplete(*diameter) || !valid_angle(*angle)
        }
        HoleKind::SimpleDrilled { drill_point_angle } => !valid_angle(*drill_point_angle),
        HoleKind::Counterbore { diameter, depth } => {
            treatment_diameter_is_incomplete(*diameter) || !positive_feature_length(*depth)
        }
        HoleKind::CounterboreDrilled {
            diameter,
            depth,
            drill_point_angle,
        } => {
            treatment_diameter_is_incomplete(*diameter)
                || !positive_feature_length(*depth)
                || !valid_angle(*drill_point_angle)
        }
        HoleKind::Counterdrill {
            diameter,
            entry_diameter,
            depth,
            angle,
        } => {
            treatment_diameter_is_incomplete(*diameter)
                || entry_diameter
                    .is_some_and(|entry| !positive_feature_length(entry) || entry.0 <= diameter.0)
                || !positive_feature_length(*depth)
                || !valid_angle(*angle)
        }
    }
}

pub(crate) fn hole_auxiliary_semantics_are_incomplete(
    profile_filter: Option<&cadmpeg_ir::features::HoleProfileFilter>,
    bottom: Option<&cadmpeg_ir::features::HoleBottom>,
    taper_angle: Option<cadmpeg_ir::features::Angle>,
    specification: Option<&cadmpeg_ir::features::HoleSpecification>,
) -> bool {
    let valid_angle = |angle: cadmpeg_ir::features::Angle| {
        angle.0.is_finite() && angle.0 > 0.0 && angle.0 < std::f64::consts::PI
    };
    profile_filter.is_some_and(|filter| !filter.points && !filter.circles && !filter.arcs)
        || bottom.is_some_and(|bottom| {
            matches!(
                bottom,
                cadmpeg_ir::features::HoleBottom::Angled { included_angle, .. }
                    if !valid_angle(*included_angle)
            )
        })
        || taper_angle.is_some_and(|angle| !valid_angle(angle))
        || specification.is_some_and(|specification| {
            let (standard, pitch, major_diameter, clearance, depth) = match specification {
                cadmpeg_ir::features::HoleSpecification::Clearance {
                    standard,
                    clearance,
                    depth,
                    ..
                } => (standard, None, None, clearance, depth),
                cadmpeg_ir::features::HoleSpecification::Threaded {
                    standard,
                    pitch,
                    major_diameter,
                    clearance,
                    depth,
                    ..
                } => (standard, *pitch, *major_diameter, clearance, depth),
            };
            standard.trim().is_empty()
                || pitch.is_some_and(|pitch| !positive_feature_length(pitch))
                || major_diameter.is_some_and(|diameter| !positive_feature_length(diameter))
                || clearance.is_some_and(|clearance| !clearance.0.is_finite())
                || matches!(
                    depth,
                    cadmpeg_ir::features::HoleThreadDepth::Blind { depth }
                        if !positive_feature_length(*depth)
                )
        })
}

pub(crate) fn chamfer_spec_is_incomplete(spec: &ChamferSpec) -> bool {
    match spec {
        ChamferSpec::Unresolved { .. } => true,
        ChamferSpec::Distance { distance } => !positive_feature_length(*distance),
        ChamferSpec::TwoDistances { first, second } => {
            !positive_feature_length(*first) || !positive_feature_length(*second)
        }
        ChamferSpec::DistanceAngle { distance, angle } => {
            !positive_feature_length(*distance)
                || !angle.0.is_finite()
                || angle.0 <= 0.0
                || angle.0 >= std::f64::consts::PI
        }
    }
}

pub(crate) fn extrude_extent_is_incomplete(
    extent: &ExtrudeExtent,
    dependencies: &[FeatureId],
) -> bool {
    let side_is_incomplete = |side: &cadmpeg_ir::features::ExtrudeSide| {
        termination_is_incomplete(&side.termination)
            || termination_dependency_is_incomplete(&side.termination, dependencies)
            || side.draft.is_some_and(|angle| {
                !angle.0.is_finite() || angle.0.abs() >= std::f64::consts::FRAC_PI_2
            })
            || side.offset.is_some_and(|offset| !offset.0.is_finite())
    };
    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
            side_is_incomplete(side)
        }
        ExtrudeExtent::TwoSided { first, second } => {
            side_is_incomplete(first) || side_is_incomplete(second)
        }
    }
}

pub(crate) fn extrude_start_is_incomplete(start: &ExtrudeStart) -> bool {
    match start {
        ExtrudeStart::Unresolved => true,
        ExtrudeStart::FromFace { face, offset } => {
            face_selection_is_incomplete(face) || offset.is_some_and(|offset| !offset.0.is_finite())
        }
        ExtrudeStart::OffsetProfilePlane { offset } => !offset.0.is_finite(),
        ExtrudeStart::ProfilePlane => false,
    }
}

pub(crate) fn revolve_feature_is_incomplete(
    construction: &RevolutionConstruction,
    op: BooleanOp,
    dependencies: &[FeatureId],
) -> bool {
    construction
        .profile
        .as_ref()
        .is_none_or(profile_ref_is_incomplete)
        || construction
            .profile
            .as_ref()
            .is_some_and(|profile| profile_dependency_is_incomplete(profile, dependencies))
        || construction.axis.is_none_or(|axis| {
            !finite_feature_point(axis.origin) || !unit_feature_direction(axis.direction)
        })
        || construction.extent.as_ref().is_none_or(|extent| {
            let side_is_incomplete = |termination: &AngularTermination| {
                angular_termination_is_incomplete(termination)
                    || angular_termination_dependency_is_incomplete(termination, dependencies)
            };
            match extent {
                RevolveExtent::OneSided { termination }
                | RevolveExtent::Symmetric { termination } => side_is_incomplete(termination),
                RevolveExtent::TwoSided { first, second } => {
                    side_is_incomplete(first) || side_is_incomplete(second)
                }
            }
        })
        || construction
            .axis_reference
            .as_ref()
            .is_some_and(path_ref_is_incomplete)
        || construction.solid.is_none()
        || construction
            .face_maker_class
            .as_ref()
            .is_some_and(|class| class.trim().is_empty())
        || matches!(op, BooleanOp::Unresolved)
}

pub(crate) fn termination_is_incomplete(termination: &LinearTermination) -> bool {
    match termination {
        LinearTermination::Unresolved => true,
        LinearTermination::ToFace { face, offset } => {
            face_selection_is_incomplete(face) || offset.is_some_and(|offset| !offset.0.is_finite())
        }
        LinearTermination::ToVertex { vertex } => match vertex {
            VertexSelection::Generated { vertex, native } => {
                native.trim().is_empty() || vertex.local_id.trim().is_empty()
            }
            VertexSelection::Historical {
                state,
                vertex,
                native,
            } => {
                state.0.trim().is_empty() || vertex.0.trim().is_empty() || native.trim().is_empty()
            }
            VertexSelection::Unresolved | VertexSelection::Native(_) => true,
        },
        LinearTermination::OffsetFromFace { face, offset } => {
            face_selection_is_incomplete(face) || !positive_feature_length(*offset)
        }
        LinearTermination::ToShape { target } => face_selection_is_incomplete(target),
        LinearTermination::Blind { length } => !length.0.is_finite() || length.0 == 0.0,
        LinearTermination::ThroughAll
        | LinearTermination::ThroughNext
        | LinearTermination::ToFirst
        | LinearTermination::ToLast => false,
    }
}

pub(crate) fn termination_dependency_is_incomplete(
    termination: &LinearTermination,
    dependencies: &[FeatureId],
) -> bool {
    matches!(
        termination,
        LinearTermination::ToVertex {
            vertex: VertexSelection::Generated { vertex, .. },
        } if !dependencies.contains(&vertex.feature)
    )
}

fn angular_termination_is_incomplete(termination: &AngularTermination) -> bool {
    match termination {
        AngularTermination::Unresolved => true,
        AngularTermination::ToFace { face, offset } => {
            face_selection_is_incomplete(face) || offset.is_some_and(|offset| !offset.0.is_finite())
        }
        AngularTermination::ToVertex { vertex } => match vertex {
            VertexSelection::Generated { vertex, native } => {
                native.trim().is_empty() || vertex.local_id.trim().is_empty()
            }
            VertexSelection::Historical {
                state,
                vertex,
                native,
            } => {
                state.0.trim().is_empty() || vertex.0.trim().is_empty() || native.trim().is_empty()
            }
            VertexSelection::Unresolved | VertexSelection::Native(_) => true,
        },
        AngularTermination::OffsetFromFace { face, offset } => {
            face_selection_is_incomplete(face) || !positive_feature_length(*offset)
        }
        AngularTermination::ToShape { target } => face_selection_is_incomplete(target),
        AngularTermination::Angle { angle } => !angle.0.is_finite() || angle.0 <= 0.0,
        AngularTermination::ThroughAll
        | AngularTermination::ThroughNext
        | AngularTermination::ToFirst
        | AngularTermination::ToLast => false,
    }
}

fn angular_termination_dependency_is_incomplete(
    termination: &AngularTermination,
    dependencies: &[FeatureId],
) -> bool {
    matches!(
        termination,
        AngularTermination::ToVertex {
            vertex: VertexSelection::Generated { vertex, .. },
        } if !dependencies.contains(&vertex.feature)
    )
}

pub(crate) fn rib_feature_is_incomplete(construction: &RibConstruction, op: BooleanOp) -> bool {
    construction
        .profile
        .as_ref()
        .is_none_or(profile_ref_is_incomplete)
        || construction
            .direction
            .is_none_or(|direction| !valid_feature_direction(direction))
        || construction
            .thickness
            .is_none_or(|thickness| !positive_feature_length(thickness))
        || construction.side.is_none()
        || matches!(construction.draft, RibDraft::Unresolved)
        || matches!(construction.draft, RibDraft::Angle(angle) if !valid_draft_angle(angle))
        || matches!(op, BooleanOp::Unresolved)
}

pub(crate) fn sweep_mode_is_incomplete(mode: SweepMode) -> bool {
    match mode {
        SweepMode::Unresolved => true,
        SweepMode::NewBody | SweepMode::Solid { .. } | SweepMode::Surface => false,
    }
}

pub(crate) fn sweep_orientation_is_incomplete(orientation: &SweepOrientation) -> bool {
    match orientation {
        SweepOrientation::Auxiliary { path, .. } => path_ref_is_incomplete(path),
        SweepOrientation::GuideSurface { faces } => face_selection_is_incomplete(faces),
        SweepOrientation::Binormal { direction } => !valid_feature_direction(*direction),
        SweepOrientation::CorrectedFrenet | SweepOrientation::Fixed | SweepOrientation::Frenet => {
            false
        }
    }
}

pub(crate) fn pattern_is_incomplete(pattern: &PatternKind) -> bool {
    match pattern {
        PatternKind::Unresolved { .. } => true,
        PatternKind::Linear {
            direction,
            spacing,
            count,
            second,
        } => {
            direction.is_none_or(|direction| !valid_feature_direction(direction))
                || !positive_feature_length(*spacing)
                || *count < 2
                || second.as_ref().is_some_and(|second| {
                    !valid_feature_direction(second.direction)
                        || !positive_feature_length(second.spacing)
                        || second.count == 0
                })
        }
        PatternKind::LinearOffsets { direction, offsets } => {
            direction.is_none_or(|direction| !valid_feature_direction(direction))
                || offsets.len() < 2
                || !valid_increasing_locations(offsets.iter().map(|offset| offset.0))
        }
        PatternKind::Circular {
            axis_origin,
            axis_dir,
            angle,
            count,
        } => {
            !finite_feature_point(*axis_origin)
                || !valid_feature_direction(*axis_dir)
                || !angle.0.is_finite()
                || angle.0 <= 0.0
                || *count < 2
        }
        PatternKind::CircularAngles {
            axis_origin,
            axis_dir,
            angles,
        } => {
            !finite_feature_point(*axis_origin)
                || !valid_feature_direction(*axis_dir)
                || angles.len() < 2
                || !valid_increasing_locations(angles.iter().map(|angle| angle.0))
        }
        PatternKind::Mirror {
            plane_origin,
            plane_normal,
        } => !finite_feature_point(*plane_origin) || !valid_feature_direction(*plane_normal),
        PatternKind::MirrorReference { .. } => true,
        PatternKind::CurveDriven {
            path,
            spacing,
            count,
        } => {
            path.as_ref().is_none_or(path_ref_is_incomplete)
                || !positive_feature_length(*spacing)
                || *count < 2
        }
        PatternKind::Scale {
            center,
            final_factor,
            count,
        } => {
            matches!(center, cadmpeg_ir::features::PatternScaleCenter::Native(_))
                || matches!(
                    center,
                    cadmpeg_ir::features::PatternScaleCenter::Point(point)
                        if !finite_feature_point(*point)
                )
                || !final_factor.is_finite()
                || *final_factor <= 0.0
                || *count < 2
        }
        PatternKind::Composite { stages } => {
            stages.is_empty()
                || stages.iter().enumerate().any(|(index, stage)| {
                    stage.combination
                        != if index == 0 {
                            cadmpeg_ir::features::PatternStageCombination::Initialize
                        } else if matches!(*stage.pattern, PatternKind::Scale { .. }) {
                            cadmpeg_ir::features::PatternStageCombination::AlignedSlices
                        } else {
                            cadmpeg_ir::features::PatternStageCombination::CartesianProduct
                        }
                        || matches!(*stage.pattern, PatternKind::Composite { .. })
                        || pattern_is_incomplete(&stage.pattern)
                })
                || pattern_composition_is_incomplete(stages)
        }
    }
}

pub(crate) fn pattern_feature_is_incomplete(
    seeds: &[cadmpeg_ir::features::PatternSeed],
    pattern: &PatternKind,
    dependencies: &[cadmpeg_ir::features::FeatureId],
) -> bool {
    seeds.is_empty()
        || seeds.iter().any(|seed| match seed {
            cadmpeg_ir::features::PatternSeed::Feature(feature) => !dependencies.contains(feature),
            cadmpeg_ir::features::PatternSeed::Faces(faces) => face_selection_is_incomplete(faces),
            cadmpeg_ir::features::PatternSeed::Bodies(bodies) => {
                body_selection_is_incomplete(bodies)
            }
            cadmpeg_ir::features::PatternSeed::Occurrences(occurrences) => occurrences.is_empty(),
        })
        || seeds
            .iter()
            .enumerate()
            .any(|(index, seed)| seeds[..index].contains(seed))
        || pattern_is_incomplete(pattern)
}

pub(crate) fn radius_spec_is_incomplete(radius: &RadiusSpec) -> bool {
    match radius {
        RadiusSpec::Unresolved { .. } => true,
        RadiusSpec::Constant { radius } => !positive_feature_length(*radius),
        RadiusSpec::Chordal { chord_length } => !positive_feature_length(*chord_length),
        RadiusSpec::Asymmetric {
            offset_one,
            offset_two,
        } => !positive_feature_length(*offset_one) || !positive_feature_length(*offset_two),
        RadiusSpec::Variable { points } => {
            points.len() < 2
                || points.iter().any(|point| {
                    !point.parameter.is_finite()
                        || !(0.0..=1.0).contains(&point.parameter)
                        || !point.radius.0.is_finite()
                        || point.radius.0 < 0.0
                })
                || !points.iter().any(|point| point.radius.0 > 0.0)
                || points
                    .windows(2)
                    .any(|pair| pair[0].parameter >= pair[1].parameter)
        }
    }
}

pub(crate) fn valid_increasing_locations(locations: impl Iterator<Item = f64>) -> bool {
    let mut locations = locations;
    let Some(first) = locations.next() else {
        return false;
    };
    first == 0.0
        && locations
            .try_fold(first, |previous, location| {
                (location.is_finite() && location > previous).then_some(location)
            })
            .is_some()
}

pub(crate) fn pattern_composition_is_incomplete(
    stages: &[cadmpeg_ir::features::PatternStage],
) -> bool {
    let mut occurrences = None;
    stages.iter().enumerate().any(|(index, stage)| {
        let Some(stage_count) = pattern_occurrence_count(&stage.pattern) else {
            return false;
        };
        if stage_count == 0 {
            return true;
        }
        if index == 0 {
            occurrences = Some(stage_count);
            return false;
        }
        match stage.combination {
            cadmpeg_ir::features::PatternStageCombination::CartesianProduct => {
                if let Some(count) = occurrences {
                    occurrences = count.checked_mul(stage_count);
                    occurrences.is_none()
                } else {
                    false
                }
            }
            cadmpeg_ir::features::PatternStageCombination::AlignedSlices => {
                occurrences.is_some_and(|count| count % stage_count != 0)
            }
            cadmpeg_ir::features::PatternStageCombination::Initialize => true,
        }
    })
}

pub(crate) fn pattern_occurrence_count(pattern: &PatternKind) -> Option<usize> {
    match pattern {
        PatternKind::Linear { count, .. }
        | PatternKind::Circular { count, .. }
        | PatternKind::CurveDriven { count, .. }
        | PatternKind::Scale { count, .. } => usize::try_from(*count).ok(),
        PatternKind::LinearOffsets { offsets, .. } => Some(offsets.len()),
        PatternKind::CircularAngles { angles, .. } => Some(angles.len()),
        PatternKind::Mirror { .. } | PatternKind::MirrorReference { .. } => Some(2),
        PatternKind::Composite { stages } => {
            stages
                .iter()
                .try_fold(None::<usize>, |occurrences, stage| {
                    let stage_count = pattern_occurrence_count(&stage.pattern)?;
                    match stage.combination {
                        cadmpeg_ir::features::PatternStageCombination::Initialize => {
                            occurrences.is_none().then_some(Some(stage_count))
                        }
                        cadmpeg_ir::features::PatternStageCombination::CartesianProduct => {
                            Some(Some(occurrences?.checked_mul(stage_count)?))
                        }
                        cadmpeg_ir::features::PatternStageCombination::AlignedSlices => {
                            let occurrences = occurrences?;
                            (occurrences % stage_count == 0).then_some(Some(occurrences))
                        }
                    }
                })?
        }
        PatternKind::Unresolved { .. } => None,
    }
}

pub(crate) fn body_selection_is_incomplete(selection: &BodySelection) -> bool {
    match selection {
        BodySelection::Bodies(bodies)
        | BodySelection::Resolved { bodies, .. }
        | BodySelection::ResolvedSet { bodies, .. } => selection_ids_are_incomplete(bodies),
        BodySelection::Local { bodies, native } => {
            native.trim().is_empty()
                || selection_ids_are_incomplete(bodies)
                || bodies.iter().any(|body| body.trim().is_empty())
        }
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::HistoricalUnorderedSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Native(_)
        | BodySelection::NativeSet(_) => true,
    }
}

pub(crate) fn body_selections_overlap(first: &BodySelection, second: &BodySelection) -> bool {
    match (first, second) {
        (
            BodySelection::Local { bodies: first, .. },
            BodySelection::Local { bodies: second, .. },
        ) => first.iter().any(|body| second.contains(body)),
        _ => explicit_body_ids(first).is_some_and(|first| {
            explicit_body_ids(second)
                .is_some_and(|second| first.iter().any(|body| second.contains(body)))
        }),
    }
}

pub(crate) fn explicit_body_ids(selection: &BodySelection) -> Option<&[BodyId]> {
    match selection {
        BodySelection::Bodies(bodies)
        | BodySelection::Resolved { bodies, .. }
        | BodySelection::ResolvedSet { bodies, .. } => Some(bodies),
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::HistoricalUnorderedSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Local { .. }
        | BodySelection::Native(_)
        | BodySelection::NativeSet(_) => None,
    }
}

pub(crate) fn resolved_body_selection_len(selection: &BodySelection) -> Option<usize> {
    match selection {
        BodySelection::Bodies(bodies)
        | BodySelection::Resolved { bodies, .. }
        | BodySelection::ResolvedSet { bodies, .. } => Some(bodies.len()),
        BodySelection::Local { bodies, .. } => Some(bodies.len()),
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::HistoricalUnorderedSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Native(_)
        | BodySelection::NativeSet(_) => None,
    }
}

pub(crate) fn face_selection_is_incomplete(selection: &FaceSelection) -> bool {
    match selection {
        FaceSelection::Unresolved
        | FaceSelection::Generated { .. }
        | FaceSelection::Native(_)
        | FaceSelection::Historical { .. }
        | FaceSelection::HistoricalPartial { .. } => true,
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => {
            selection_ids_are_incomplete(faces)
        }
    }
}

pub(crate) fn face_selections_overlap(first: &FaceSelection, second: &FaceSelection) -> bool {
    let first = match first {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => faces,
        FaceSelection::Unresolved
        | FaceSelection::Generated { .. }
        | FaceSelection::Native(_)
        | FaceSelection::Historical { .. }
        | FaceSelection::HistoricalPartial { .. } => return false,
    };
    let second = match second {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => faces,
        FaceSelection::Unresolved
        | FaceSelection::Generated { .. }
        | FaceSelection::Native(_)
        | FaceSelection::Historical { .. }
        | FaceSelection::HistoricalPartial { .. } => return false,
    };
    first.iter().any(|face| second.contains(face))
}

pub(crate) fn edge_selection_is_incomplete(selection: &EdgeSelection) -> bool {
    match selection {
        EdgeSelection::Unresolved
        | EdgeSelection::Generated { .. }
        | EdgeSelection::Native(_)
        | EdgeSelection::Historical { .. }
        | EdgeSelection::HistoricalPartial { .. } => true,
        EdgeSelection::All => false,
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => {
            selection_ids_are_incomplete(edges)
        }
    }
}

pub(crate) fn profile_ref_is_incomplete(profile: &ProfileRef) -> bool {
    match profile {
        ProfileRef::Unresolved(_)
        | ProfileRef::Native(_)
        | ProfileRef::SketchSelection { .. }
        | ProfileRef::SpatialSketchSelection { .. } => true,
        ProfileRef::Sketch(_) => false,
        ProfileRef::SketchEntities { entities, .. } => selection_ids_are_incomplete(entities),
        ProfileRef::SketchProfiles { profiles, .. }
        | ProfileRef::SpatialSketchProfiles { profiles, .. } => {
            selection_ids_are_incomplete(profiles)
        }
        ProfileRef::SketchRegions { regions, .. } => {
            regions.is_empty()
                || regions
                    .iter()
                    .enumerate()
                    .any(|(index, region)| regions[..index].contains(region))
        }
        ProfileRef::HistoricalFaces { faces, .. } => selection_ids_are_incomplete(faces),
        ProfileRef::Generated { curves, native } => {
            native.trim().is_empty()
                || curves.is_empty()
                || curves.iter().enumerate().any(|(index, curve)| {
                    curve.local_id.trim().is_empty() || curves[..index].contains(curve)
                })
        }
        ProfileRef::Feature(_) => false,
        ProfileRef::Faces(faces) => selection_ids_are_incomplete(faces),
    }
}

pub(crate) fn profile_dependency_is_incomplete(
    profile: &ProfileRef,
    dependencies: &[FeatureId],
) -> bool {
    match profile {
        ProfileRef::Feature(feature) => !dependencies.contains(feature),
        ProfileRef::Generated { curves, .. } => curves
            .iter()
            .any(|curve| !dependencies.contains(&curve.feature)),
        _ => false,
    }
}

pub(crate) fn loft_section_is_incomplete(section: &LoftSection) -> bool {
    match section {
        LoftSection::Profile(profile) => profile_ref_is_incomplete(profile),
        LoftSection::Point(LoftPointSection::Native(_)) => true,
        LoftSection::Point(LoftPointSection::Point(point)) => !finite_feature_point(*point),
        LoftSection::Point(LoftPointSection::Vertex(vertex)) => vertex.0.trim().is_empty(),
    }
}

pub(crate) fn selection_ids_are_incomplete<T: Ord>(ids: &[T]) -> bool {
    ids.is_empty() || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
}

pub(crate) fn path_ref_is_incomplete(path: &PathRef) -> bool {
    match path {
        PathRef::Unresolved(_) | PathRef::Native(_) | PathRef::SpatialSketchSelection { .. } => {
            true
        }
        PathRef::HistoricalEdges { edges, .. } => selection_ids_are_incomplete(edges),
        PathRef::Sketch(_) => false,
        PathRef::SketchCurves { curves, .. } => selection_ids_are_incomplete(curves),
        PathRef::SpatialSketchCurves { curves, .. } => selection_ids_are_incomplete(curves),
        PathRef::Edges(edges) => selection_ids_are_incomplete(edges),
        PathRef::Curves(curves) => selection_ids_are_incomplete(curves),
    }
}
