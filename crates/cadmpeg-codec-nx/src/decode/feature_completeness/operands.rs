// SPDX-License-Identifier: Apache-2.0
//! Operand and selection completeness predicates.

use super::{positive_feature_length, unit_feature_direction};
use cadmpeg_ir::features::{
    AngularTermination, BodySelection, BooleanOp, EdgeSelection, ExtrudeExtent, ExtrudeStart,
    FaceSelection, FeatureId, HoleKind, Length, LinearTermination, LoftPointSection, LoftSection,
    PathRef, PatternKind, PatternTransform, ProfileRef, RevolveConstruction, RevolveExtent,
    RibConstruction, RibDraft, SweepMode, SweepOrientation, VertexSelection,
};
use cadmpeg_ir::ids::BodyId;
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
    let finite_direction = |vector: cadmpeg_ir::features::FeatureDirection3| {
        vector.norm() > EPS_NONZERO_HOLE_DIRECTION
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
                cadmpeg_ir::features::HolePlacement::Directed { direction, .. } => {
                    finite_direction(*direction)
                }
                cadmpeg_ir::features::HolePlacement::Axis { axis, .. } => {
                    axis_is_direction_invariant && finite_direction(*axis)
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
    let treatment_diameter_is_incomplete = |diameter: cadmpeg_ir::features::PositiveLength| {
        bore_diameter.is_none_or(|bore| diameter.get() <= bore.get())
    };
    match kind {
        HoleKind::Unresolved(_)
        | HoleKind::PartialCounterbore { .. }
        | HoleKind::PartialCountersink { .. } => true,
        HoleKind::Simple => false,
        HoleKind::Chamfer { diameter, .. } | HoleKind::Countersink { diameter, .. } => {
            treatment_diameter_is_incomplete(*diameter)
        }
        HoleKind::SimpleDrilled { .. } => false,
        HoleKind::Counterbore { diameter, .. } => treatment_diameter_is_incomplete(*diameter),
        HoleKind::CounterboreDrilled { diameter, .. } => {
            treatment_diameter_is_incomplete(*diameter)
        }
        HoleKind::Counterdrill { diameters, .. } => {
            treatment_diameter_is_incomplete(diameters.diameter())
        }
    }
}

pub(crate) fn hole_specification_is_incomplete(
    specification: Option<&cadmpeg_ir::features::HoleSpecification>,
) -> bool {
    specification.is_some_and(|specification| {
        let (cadmpeg_ir::features::HoleSpecification::Clearance { standard, .. }
        | cadmpeg_ir::features::HoleSpecification::Threaded { standard, .. }) = specification;
        standard.as_str().trim().is_empty()
    })
}

pub(crate) fn extrude_extent_is_incomplete(
    extent: &ExtrudeExtent,
    dependencies: &[FeatureId],
) -> bool {
    let side_is_incomplete = |side: &cadmpeg_ir::features::ExtrudeSide| {
        termination_is_incomplete(&side.termination)
            || termination_dependency_is_incomplete(&side.termination, dependencies)
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
            face_selection_is_incomplete(face)
                || offset.is_some_and(|offset| !offset.get().is_finite())
        }
        ExtrudeStart::OffsetProfilePlane { offset } => !offset.get().is_finite(),
        ExtrudeStart::ProfilePlane => false,
    }
}

pub(crate) fn revolve_feature_is_incomplete(
    construction: &RevolveConstruction,
    op: BooleanOp,
    dependencies: &[FeatureId],
) -> bool {
    let RevolveConstruction::Resolved {
        profile,
        axis,
        extent,
        solid,
        ..
    } = construction
    else {
        return true;
    };
    profile_ref_is_incomplete(profile)
        || profile_dependency_is_incomplete(profile, dependencies)
        || !unit_feature_direction(axis.direction.get())
        || {
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
        }
        || axis.reference.as_ref().is_some_and(path_ref_is_incomplete)
        || solid.is_none()
        || matches!(op, BooleanOp::Unresolved)
}

pub(crate) fn termination_is_incomplete(termination: &LinearTermination) -> bool {
    match termination {
        LinearTermination::Unresolved => true,
        LinearTermination::ToFace { face, offset } => {
            face_selection_is_incomplete(face)
                || offset.is_some_and(|offset| !offset.get().is_finite())
        }
        LinearTermination::ToVertex { vertex } => match vertex {
            VertexSelection::Generated { .. } => false,
            VertexSelection::Historical {
                state,
                vertex,
                native,
            } => {
                state.as_str().trim().is_empty()
                    || vertex.as_str().trim().is_empty()
                    || native.as_str().trim().is_empty()
            }
            VertexSelection::Unresolved | VertexSelection::Native(_) => true,
        },
        LinearTermination::OffsetFromFace { face, .. } => face_selection_is_incomplete(face),
        LinearTermination::ToShape { target } => face_selection_is_incomplete(target),
        LinearTermination::Blind { .. } => false,
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
            face_selection_is_incomplete(face)
                || offset.is_some_and(|offset| !offset.get().is_finite())
        }
        AngularTermination::ToVertex { vertex } => match vertex {
            VertexSelection::Generated { .. } => false,
            VertexSelection::Historical {
                state,
                vertex,
                native,
            } => {
                state.as_str().trim().is_empty()
                    || vertex.as_str().trim().is_empty()
                    || native.as_str().trim().is_empty()
            }
            VertexSelection::Unresolved | VertexSelection::Native(_) => true,
        },
        AngularTermination::OffsetFromFace { face, .. } => face_selection_is_incomplete(face),
        AngularTermination::ToShape { target } => face_selection_is_incomplete(target),
        AngularTermination::Angle { .. } => false,
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
        .as_deref()
        .is_none_or(profile_ref_is_incomplete)
        || construction.direction.is_none()
        || construction.thickness.is_none()
        || construction.side.is_none()
        || matches!(construction.draft, RibDraft::Unresolved)
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
        SweepOrientation::Binormal { .. } => false,
        SweepOrientation::CorrectedFrenet | SweepOrientation::Fixed | SweepOrientation::Frenet => {
            false
        }
    }
}

pub(crate) fn pattern_is_incomplete(pattern: &PatternKind) -> bool {
    match pattern.definition() {
        PatternTransform::Unresolved
        | PatternTransform::UnresolvedLinear
        | PatternTransform::UnresolvedCircular
        | PatternTransform::UnresolvedCurveDriven
        | PatternTransform::UnresolvedMirror
        | PatternTransform::UnresolvedScale
        | PatternTransform::UnresolvedComposite => true,
        PatternTransform::Linear {
            direction, count, ..
        } => direction.is_none() || *count < 2,
        PatternTransform::LinearOffsets { direction, offsets } => {
            direction.is_none() || offsets.len() < 2
        }
        PatternTransform::Circular { count, .. } => *count < 2,
        PatternTransform::CircularAngles { angles, .. } => angles.len() < 2,
        PatternTransform::Mirror { .. } => false,
        PatternTransform::MirrorReference { .. } => true,
        PatternTransform::CurveDriven { path, count, .. } => {
            path.as_ref().is_none_or(path_ref_is_incomplete) || *count < 2
        }
        PatternTransform::Scale { center, .. } => {
            matches!(center, cadmpeg_ir::features::PatternScaleCenter::Native(_))
        }
        PatternTransform::Composite { stages } => stages
            .iter()
            .any(|stage| pattern_is_incomplete(&stage.pattern)),
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

pub(crate) fn pattern_occurrence_count(pattern: &PatternKind) -> Option<usize> {
    match pattern.definition() {
        PatternTransform::Linear { count, .. }
        | PatternTransform::Circular { count, .. }
        | PatternTransform::CurveDriven { count, .. }
        | PatternTransform::Scale { count, .. } => usize::try_from(*count).ok(),
        PatternTransform::LinearOffsets { offsets, .. } => Some(offsets.len()),
        PatternTransform::CircularAngles { angles, .. } => Some(angles.len()),
        PatternTransform::Mirror { .. } | PatternTransform::MirrorReference { .. } => Some(2),
        PatternTransform::Composite { stages } => {
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
        PatternTransform::Unresolved
        | PatternTransform::UnresolvedLinear
        | PatternTransform::UnresolvedCircular
        | PatternTransform::UnresolvedCurveDriven
        | PatternTransform::UnresolvedMirror
        | PatternTransform::UnresolvedScale
        | PatternTransform::UnresolvedComposite => None,
    }
}

pub(crate) fn body_selection_is_incomplete(selection: &BodySelection) -> bool {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            selection_ids_are_incomplete(bodies)
        }
        BodySelection::ResolvedSet { .. } => false,
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

pub(crate) fn explicit_body_ids(selection: &BodySelection) -> Option<Vec<BodyId>> {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            Some(bodies.clone())
        }
        BodySelection::ResolvedSet { members } => Some(members.bodies().cloned().collect()),
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
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            Some(bodies.len())
        }
        BodySelection::ResolvedSet { members } => Some(members.len()),
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
        ProfileRef::SketchEntities { .. } => false,
        ProfileRef::SketchProfiles { .. } | ProfileRef::SpatialSketchProfiles { .. } => false,
        ProfileRef::SketchRegions { .. } => false,
        ProfileRef::HistoricalFaces { .. } => false,
        ProfileRef::Generated { curves, .. } => curves
            .iter()
            .enumerate()
            .any(|(index, curve)| curves[..index].contains(curve)),
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
        LoftSection::Point(LoftPointSection::Point(_)) => false,
        LoftSection::Point(LoftPointSection::Vertex(_)) => false,
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
        PathRef::HistoricalEdges { .. } => false,
        PathRef::Sketch(_) => false,
        PathRef::SketchCurves { .. } => false,
        PathRef::SpatialSketchCurves { .. } => false,
        PathRef::Edges(edges) => selection_ids_are_incomplete(edges),
        PathRef::Curves(curves) => selection_ids_are_incomplete(curves),
    }
}
