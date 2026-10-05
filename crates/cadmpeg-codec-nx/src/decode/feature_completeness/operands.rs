// SPDX-License-Identifier: Apache-2.0
//! Operand and selection completeness predicates.

use super::{positive_feature_length, CompletenessAdmission};
use cadmpeg_ir::{
    features::{
        holes::HoleKind,
        patterns::{PatternKind, PatternTransform},
        AngularTermination, BodySelection, BooleanOp, EdgeSelection, ExtrudeExtent, ExtrudeStart,
        FaceSelection, FeatureId, LinearTermination, LoftPointSection, LoftSection, PathRef,
        PlanarProfileRef, ProfileRef, RevolveConstruction, RevolveExtent, RibConstruction,
        RibDraft, SweepMode, SweepOrientation, VertexSelection,
    },
    scalar::Length,
};

const SELECTIONS: &str = "nx feature completeness selections";
const DEPENDENCIES: &str = "nx feature completeness dependencies";
const REFERENCES: &str = "nx feature completeness references";

pub(super) fn hole_feature_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    profile: Option<&PlanarProfileRef>,
    face: Option<&FaceSelection>,
    placements: Option<&[cadmpeg_ir::features::holes::HolePlacement]>,
    treatments: (&HoleKind, Option<&HoleKind>),
    diameter: Option<Length>,
    extent: Option<&LinearTermination>,
) -> Result<bool, A::Error> {
    let (kind, exit_kind) = treatments;
    let profile_incomplete = match profile {
        Some(profile) => planar_profile_ref_is_incomplete(admission, profile)?,
        None => false,
    };
    let face_incomplete = match face {
        Some(face) => face_selection_is_incomplete(admission, face)?,
        None => false,
    };
    let axis_is_direction_invariant = matches!(extent, Some(LinearTermination::ThroughAll {}))
        && exit_kind.is_none_or(|exit| exit == kind);
    let placements_complete = match placements {
        Some(placements) => {
            !placements.is_empty()
                && !admission.has_equal_pair(placements, "nx hole placement duplicates")?
                && !admission.any_by(
                    placements,
                    |placement| {
                        Ok(match placement {
                            cadmpeg_ir::features::holes::HolePlacement::Directed { .. } => false,
                            cadmpeg_ir::features::holes::HolePlacement::Axis { .. } => {
                                !axis_is_direction_invariant
                            }
                        })
                    },
                    "nx hole placement directions",
                )?
        }
        None => false,
    };
    let placements_incomplete = placements.is_some() && !placements_complete;
    let location_unresolved = !placements_complete
        && match profile {
            Some(profile) => planar_profile_ref_is_incomplete(admission, profile)?,
            None => true,
        };
    let orientation_unresolved = !placements_complete
        && match face {
            Some(face) => face_selection_is_incomplete(admission, face)?,
            None => true,
        };
    Ok(profile_incomplete
        || face_incomplete
        || placements_incomplete
        || location_unresolved
        || orientation_unresolved
        || hole_kind_is_incomplete(kind, diameter)
        || exit_kind.is_some_and(|kind| hole_kind_is_incomplete(kind, diameter))
        || diameter.is_none_or(|diameter| !positive_feature_length(diameter))
        || match extent {
            Some(extent) => termination_is_incomplete(admission, extent)?,
            None => true,
        })
}

fn hole_kind_is_incomplete(kind: &HoleKind, bore_diameter: Option<Length>) -> bool {
    let treatment_diameter_is_incomplete = |diameter: cadmpeg_ir::scalar::PositiveLength| {
        bore_diameter.is_none_or(|bore| diameter.get() <= bore.get())
    };
    match kind {
        HoleKind::Unresolved(_)
        | HoleKind::PartialCounterbore(..)
        | HoleKind::PartialCountersink(..) => true,
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

pub(super) fn extrude_extent_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    extent: &ExtrudeExtent,
    dependencies: &[FeatureId],
) -> Result<bool, A::Error> {
    let side_is_incomplete = |side: &cadmpeg_ir::features::ExtrudeSide| {
        Ok(termination_is_incomplete(admission, &side.termination)?
            || termination_dependency_is_incomplete(admission, &side.termination, dependencies)?)
    };
    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
            side_is_incomplete(side)
        }
        ExtrudeExtent::TwoSided { first, second } => {
            Ok(side_is_incomplete(first)? || side_is_incomplete(second)?)
        }
    }
}

pub(super) fn extrude_start_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    start: &ExtrudeStart,
) -> Result<bool, A::Error> {
    match start {
        ExtrudeStart::Unresolved {} => Ok(true),
        ExtrudeStart::FromFace { face, .. } => face_selection_is_incomplete(admission, face),
        ExtrudeStart::OffsetProfilePlane { .. } | ExtrudeStart::ProfilePlane {} => Ok(false),
    }
}

pub(super) fn revolve_feature_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    construction: &RevolveConstruction,
    op: BooleanOp,
    dependencies: &[FeatureId],
) -> Result<bool, A::Error> {
    let RevolveConstruction::Resolved {
        profile,
        axis,
        extent,
        solid,
        ..
    } = construction
    else {
        return Ok(true);
    };
    let side_is_incomplete = |termination: &AngularTermination| {
        Ok(angular_termination_is_incomplete(admission, termination)?
            || angular_termination_dependency_is_incomplete(admission, termination, dependencies)?)
    };
    Ok(planar_profile_ref_is_incomplete(admission, profile)?
        || planar_profile_dependency_is_incomplete(admission, profile, dependencies)?
        || match extent {
            RevolveExtent::OneSided { termination } | RevolveExtent::Symmetric { termination } => {
                side_is_incomplete(termination)?
            }
            RevolveExtent::TwoSided { first, second } => {
                side_is_incomplete(first)? || side_is_incomplete(second)?
            }
        }
        || match &axis.reference {
            Some(reference) => path_ref_is_incomplete(admission, reference)?,
            None => false,
        }
        || solid.is_none()
        || matches!(op, BooleanOp::Unresolved))
}

fn historical_vertex_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    vertex: &VertexSelection,
) -> Result<bool, A::Error> {
    match vertex {
        VertexSelection::Generated { .. } => Ok(false),
        VertexSelection::Historical {
            state,
            vertex,
            native,
        } => Ok(admission.is_blank(state.as_str(), REFERENCES)?
            || admission.is_blank(vertex.as_str(), REFERENCES)?
            || admission.is_blank(native.as_str(), REFERENCES)?),
        VertexSelection::Unresolved | VertexSelection::Native(_) => Ok(true),
    }
}

pub(super) fn termination_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    termination: &LinearTermination,
) -> Result<bool, A::Error> {
    match termination {
        LinearTermination::Unresolved {} => Ok(true),
        LinearTermination::ToFace { face, .. } => face_selection_is_incomplete(admission, face),
        LinearTermination::ToVertex { vertex } => {
            historical_vertex_is_incomplete(admission, vertex)
        }
        LinearTermination::OffsetFromFace { face, .. } => {
            face_selection_is_incomplete(admission, face)
        }
        LinearTermination::ToShape { target } => face_selection_is_incomplete(admission, target),
        LinearTermination::Blind { .. } => Ok(false),
        LinearTermination::ThroughAll {}
        | LinearTermination::ThroughNext {}
        | LinearTermination::ToFirst {}
        | LinearTermination::ToLast {} => Ok(false),
    }
}

pub(super) fn termination_dependency_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    termination: &LinearTermination,
    dependencies: &[FeatureId],
) -> Result<bool, A::Error> {
    match termination {
        LinearTermination::ToVertex {
            vertex: VertexSelection::Generated { vertex, .. },
        } => Ok(!admission.contains(dependencies, &vertex.feature, DEPENDENCIES)?),
        _ => Ok(false),
    }
}

fn angular_termination_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    termination: &AngularTermination,
) -> Result<bool, A::Error> {
    match termination {
        AngularTermination::Unresolved {} => Ok(true),
        AngularTermination::ToFace { face, .. } => face_selection_is_incomplete(admission, face),
        AngularTermination::ToVertex { vertex } => {
            historical_vertex_is_incomplete(admission, vertex)
        }
        AngularTermination::OffsetFromFace { face, .. } => {
            face_selection_is_incomplete(admission, face)
        }
        AngularTermination::ToShape { target } => face_selection_is_incomplete(admission, target),
        AngularTermination::Angle { .. } => Ok(false),
        AngularTermination::ThroughAll {}
        | AngularTermination::ThroughNext {}
        | AngularTermination::ToFirst {}
        | AngularTermination::ToLast {} => Ok(false),
    }
}

fn angular_termination_dependency_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    termination: &AngularTermination,
    dependencies: &[FeatureId],
) -> Result<bool, A::Error> {
    match termination {
        AngularTermination::ToVertex {
            vertex: VertexSelection::Generated { vertex, .. },
        } => Ok(!admission.contains(dependencies, &vertex.feature, DEPENDENCIES)?),
        _ => Ok(false),
    }
}

pub(super) fn rib_feature_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    construction: &RibConstruction,
    op: BooleanOp,
) -> Result<bool, A::Error> {
    Ok(match &construction.profile {
        Some(profile) => planar_profile_ref_is_incomplete(admission, profile)?,
        None => true,
    } || construction.direction.is_none()
        || construction.thickness.is_none()
        || construction.side.is_none()
        || matches!(construction.draft, RibDraft::Unresolved)
        || matches!(op, BooleanOp::Unresolved))
}

pub(super) fn sweep_mode_is_incomplete(mode: SweepMode) -> bool {
    match mode {
        SweepMode::Unresolved {} => true,
        SweepMode::Solid {
            op: cadmpeg_ir::features::SolidSweepOperation::NewBody,
        }
        | SweepMode::Solid { .. }
        | SweepMode::Surface {} => false,
    }
}

pub(super) fn sweep_orientation_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    orientation: &SweepOrientation,
) -> Result<bool, A::Error> {
    match orientation {
        SweepOrientation::Auxiliary { path, .. } => path_ref_is_incomplete(admission, path),
        SweepOrientation::GuideSurface { faces } => face_selection_is_incomplete(admission, faces),
        SweepOrientation::Binormal { .. } => Ok(false),
        SweepOrientation::CorrectedFrenet {}
        | SweepOrientation::Fixed {}
        | SweepOrientation::Frenet {} => Ok(false),
    }
}

pub(super) fn pattern_is_incomplete<
    A: CompletenessAdmission,
    C: cadmpeg_ir::features::patterns::CompositeStages,
>(
    admission: &A,
    pattern: &PatternKind<C>,
) -> Result<bool, A::Error> {
    match pattern.definition() {
        PatternTransform::Unresolved { .. } => Ok(true),
        PatternTransform::Linear {
            direction, count, ..
        } => Ok(direction.is_none() || *count < 2),
        PatternTransform::LinearOffsets { direction, offsets } => {
            Ok(direction.is_none() || offsets.len() < 2)
        }
        PatternTransform::Circular { count, .. } => Ok(*count < 2),
        PatternTransform::CircularAngles { angles, .. } => Ok(angles.len() < 2),
        PatternTransform::Mirror { .. } => Ok(false),
        PatternTransform::MirrorReference { .. } => Ok(true),
        PatternTransform::CurveDriven { path, count, .. } => Ok(match path {
            Some(path) => path_ref_is_incomplete(admission, path)?,
            None => true,
        } || *count < 2),
        PatternTransform::Scale { center, .. } => Ok(matches!(
            center,
            cadmpeg_ir::features::patterns::PatternScaleCenter::Native(_)
        )),
        PatternTransform::Composite { stages } => admission.any_by(
            stages.stages(),
            |stage| pattern_is_incomplete(admission, &stage.pattern),
            "nx pattern stages",
        ),
    }
}

pub(crate) fn pattern_feature_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    seeds: &[cadmpeg_ir::features::patterns::PatternSeed],
    pattern: &PatternKind,
    dependencies: &[cadmpeg_ir::features::FeatureId],
) -> Result<bool, A::Error> {
    Ok(seeds.is_empty()
        || admission.any_by(
            seeds,
            |seed| match seed {
                cadmpeg_ir::features::patterns::PatternSeed::Feature(feature) => {
                    Ok(!admission.contains(dependencies, feature, DEPENDENCIES)?)
                }
                cadmpeg_ir::features::patterns::PatternSeed::Faces(faces) => {
                    face_selection_is_incomplete(admission, faces)
                }
                cadmpeg_ir::features::patterns::PatternSeed::Bodies(bodies) => {
                    body_selection_is_incomplete(admission, bodies)
                }
                cadmpeg_ir::features::patterns::PatternSeed::Occurrences(occurrences) => {
                    Ok(occurrences.is_empty())
                }
            },
            "nx pattern seeds",
        )?
        || admission.has_equal_pair(seeds, "nx pattern seed duplicates")?
        || pattern_is_incomplete(admission, pattern)?)
}

/// The occurrence count of a complete pattern transform. Composite stages
/// are visited in order and never nest.
pub(crate) fn pattern_occurrence_count<
    A: CompletenessAdmission,
    C: cadmpeg_ir::features::patterns::CompositeStages,
>(
    admission: &A,
    pattern: &PatternKind<C>,
) -> Result<Option<usize>, A::Error> {
    Ok(match pattern.definition() {
        PatternTransform::Linear { count, .. }
        | PatternTransform::Circular { count, .. }
        | PatternTransform::CurveDriven { count, .. }
        | PatternTransform::Scale { count, .. } => usize::try_from(*count).ok(),
        PatternTransform::LinearOffsets { offsets, .. } => Some(offsets.len()),
        PatternTransform::CircularAngles { angles, .. } => Some(angles.len()),
        PatternTransform::Mirror { .. } | PatternTransform::MirrorReference { .. } => Some(2),
        PatternTransform::Composite { stages } => {
            let mut occurrences = None::<usize>;
            let mut complete = true;
            let mut first = true;
            admission.any_by(
                stages.stages(),
                |stage| {
                    let Some(stage_count) = pattern_occurrence_count(admission, &stage.pattern)?
                    else {
                        complete = false;
                        return Ok(true);
                    };
                    let combined = if first {
                        first = false;
                        occurrences.is_none().then_some(stage_count)
                    } else if matches!(stage.pattern.definition(), PatternTransform::Scale { .. }) {
                        occurrences
                            .filter(|occurrences| occurrences.checked_rem(stage_count) == Some(0))
                    } else {
                        occurrences.and_then(|occurrences| occurrences.checked_mul(stage_count))
                    };
                    let Some(combined) = combined else {
                        complete = false;
                        return Ok(true);
                    };
                    occurrences = Some(combined);
                    Ok(false)
                },
                "nx pattern stage occurrences",
            )?;
            if complete {
                occurrences
            } else {
                None
            }
        }
        PatternTransform::Unresolved { .. } => None,
    })
}

pub(crate) fn body_selection_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    selection: &BodySelection,
) -> Result<bool, A::Error> {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            selection_ids_are_incomplete(admission, bodies)
        }
        BodySelection::ResolvedSet { .. } => Ok(false),
        BodySelection::Local { bodies, native } => Ok(admission.is_blank(native, REFERENCES)?
            || selection_ids_are_incomplete(admission, bodies)?
            || admission.any_by(
                bodies,
                |body| admission.is_blank(body, REFERENCES),
                SELECTIONS,
            )?),
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Native(_)
        | BodySelection::NativeSet(_) => Ok(true),
    }
}

pub(in crate::decode) fn face_selection_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    selection: &FaceSelection,
) -> Result<bool, A::Error> {
    match selection {
        FaceSelection::Unresolved
        | FaceSelection::Generated { .. }
        | FaceSelection::Native(_)
        | FaceSelection::Historical { .. }
        | FaceSelection::HistoricalPartial { .. } => Ok(true),
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => {
            selection_ids_are_incomplete(admission, faces)
        }
    }
}

pub(super) fn edge_selection_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    selection: &EdgeSelection,
) -> Result<bool, A::Error> {
    match selection {
        EdgeSelection::Unresolved
        | EdgeSelection::Generated { .. }
        | EdgeSelection::Native(_)
        | EdgeSelection::Historical { .. }
        | EdgeSelection::HistoricalPartial { .. } => Ok(true),
        EdgeSelection::All => Ok(false),
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => {
            selection_ids_are_incomplete(admission, edges)
        }
    }
}

pub(super) fn profile_ref_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    profile: &ProfileRef,
) -> Result<bool, A::Error> {
    match profile {
        ProfileRef::SpatialSketchSelection { .. } => Ok(true),
        ProfileRef::SpatialSketchProfiles { .. } => Ok(false),
        ProfileRef::Planar(planar) => planar_profile_ref_is_incomplete(admission, planar),
    }
}

pub(super) fn planar_profile_ref_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    profile: &PlanarProfileRef,
) -> Result<bool, A::Error> {
    match profile {
        PlanarProfileRef::Unresolved(_)
        | PlanarProfileRef::Native(_)
        | PlanarProfileRef::SketchSelection { .. } => Ok(true),
        PlanarProfileRef::Sketch(_)
        | PlanarProfileRef::SketchEntities { .. }
        | PlanarProfileRef::SketchProfiles { .. }
        | PlanarProfileRef::SketchRegions { .. }
        | PlanarProfileRef::HistoricalFaces { .. }
        | PlanarProfileRef::Feature(_) => Ok(false),
        PlanarProfileRef::Generated { curves, .. } => {
            admission.has_equal_pair(curves, "nx generated profile curve duplicates")
        }
        PlanarProfileRef::Faces(faces) => selection_ids_are_incomplete(admission, faces),
    }
}

pub(super) fn profile_dependency_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    profile: &ProfileRef,
    dependencies: &[FeatureId],
) -> Result<bool, A::Error> {
    match profile {
        ProfileRef::Planar(planar) => {
            planar_profile_dependency_is_incomplete(admission, planar, dependencies)
        }
        ProfileRef::SpatialSketchProfiles { .. } | ProfileRef::SpatialSketchSelection { .. } => {
            Ok(false)
        }
    }
}

pub(super) fn planar_profile_dependency_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    profile: &PlanarProfileRef,
    dependencies: &[FeatureId],
) -> Result<bool, A::Error> {
    match profile {
        PlanarProfileRef::Feature(feature) => {
            Ok(!admission.contains(dependencies, feature, DEPENDENCIES)?)
        }
        PlanarProfileRef::Generated { curves, .. } => admission.any_by(
            curves,
            |curve| Ok(!admission.contains(dependencies, &curve.feature, DEPENDENCIES)?),
            "nx generated profile curves",
        ),
        _ => Ok(false),
    }
}

pub(super) fn loft_section_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    section: &LoftSection,
) -> Result<bool, A::Error> {
    match section {
        LoftSection::Profile(profile) => profile_ref_is_incomplete(admission, profile),
        LoftSection::Point(LoftPointSection::Native(_)) => Ok(true),
        LoftSection::Point(LoftPointSection::Point(_)) => Ok(false),
        LoftSection::Point(LoftPointSection::Vertex(_)) => Ok(false),
    }
}

fn selection_ids_are_incomplete<A: CompletenessAdmission, T>(
    admission: &A,
    ids: &[T],
) -> Result<bool, A::Error>
where
    T: Ord + cadmpeg_core::decode::cost::DecodeCost,
{
    Ok(ids.is_empty() || admission.has_duplicate(ids, "nx selection duplicates")?)
}

pub(in crate::decode) fn path_ref_is_incomplete<A: CompletenessAdmission>(
    admission: &A,
    path: &PathRef,
) -> Result<bool, A::Error> {
    match path {
        PathRef::Unresolved(_) | PathRef::Native(_) | PathRef::SpatialSketchSelection { .. } => {
            Ok(true)
        }
        PathRef::HistoricalEdges { .. } => Ok(false),
        PathRef::Sketch(_) => Ok(false),
        PathRef::SketchCurves { .. } => Ok(false),
        PathRef::SpatialSketchCurves { .. } => Ok(false),
        PathRef::Edges(edges) => selection_ids_are_incomplete(admission, edges),
        PathRef::Curves(curves) => selection_ids_are_incomplete(admission, curves),
    }
}

#[cfg(test)]
mod tests;
