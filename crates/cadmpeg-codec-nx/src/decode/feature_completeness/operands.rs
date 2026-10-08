// SPDX-License-Identifier: Apache-2.0
//! Operand and selection completeness predicates.

use super::positive_feature_length;
use cadmpeg_core::decode::{cost::DecodeCost, DecodeContext};
use cadmpeg_core::CodecError;
use std::collections::BTreeSet;
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

const DEPENDENCIES: &str = "nx feature completeness dependencies";

pub(super) fn hole_feature_is_incomplete(
    ctx: &DecodeContext<'_>,
    profile: Option<&PlanarProfileRef>,
    face: Option<&FaceSelection>,
    placements: Option<&[cadmpeg_ir::features::holes::HolePlacement]>,
    treatments: (&HoleKind, Option<&HoleKind>),
    diameter: Option<Length>,
    extent: Option<&LinearTermination>,
) -> Result<bool, CodecError> {
    let (kind, exit_kind) = treatments;
    let profile_incomplete = match profile {
        Some(profile) => planar_profile_ref_is_incomplete(ctx, profile)?,
        None => false,
    };
    let face_incomplete = match face {
        Some(face) => face_selection_is_incomplete(ctx, face)?,
        None => false,
    };
    let axis_is_direction_invariant = matches!(extent, Some(LinearTermination::ThroughAll {}))
        && exit_kind.is_none_or(|exit| exit == kind);
    let placements_complete = match placements {
        Some(placements) => {
            let operation = "nx hole placement duplicates";
            let mut visits = placements.iter();
            let mut index = 0;
            let has_duplicate = loop {
                if visits.as_slice().is_empty() {
                    break false;
                }
                let Some(placement) = ctx.next_charged(&mut visits, operation)? else {
                    break false;
                };
                let duplicate = ctx.contains(&placements[..index], placement, operation)?;
                index += 1;
                if duplicate {
                    break true;
                }
            };
            !placements.is_empty()
                && !has_duplicate
                && !ctx.any_by(
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
            Some(_) => profile_incomplete,
            None => true,
        };
    let orientation_unresolved = !placements_complete
        && match face {
            Some(_) => face_incomplete,
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
            Some(extent) => termination_is_incomplete(ctx, extent)?,
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

pub(super) fn extrude_extent_is_incomplete(
    ctx: &DecodeContext<'_>,
    extent: &ExtrudeExtent,
    dependencies: &[FeatureId],
) -> Result<bool, CodecError> {
    let side_is_incomplete =
        |side: &cadmpeg_ir::features::ExtrudeSide| -> Result<bool, CodecError> {
            Ok(termination_is_incomplete(ctx, &side.termination)?
                || termination_dependency_is_incomplete(ctx, &side.termination, dependencies)?)
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

pub(super) fn extrude_start_is_incomplete(
    ctx: &DecodeContext<'_>,
    start: &ExtrudeStart,
) -> Result<bool, CodecError> {
    match start {
        ExtrudeStart::Unresolved {} => Ok(true),
        ExtrudeStart::FromFace { face, .. } => face_selection_is_incomplete(ctx, face),
        ExtrudeStart::OffsetProfilePlane { .. } | ExtrudeStart::ProfilePlane {} => Ok(false),
    }
}

pub(super) fn revolve_feature_is_incomplete(
    ctx: &DecodeContext<'_>,
    construction: &RevolveConstruction,
    op: BooleanOp,
    dependencies: &[FeatureId],
) -> Result<bool, CodecError> {
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
    let side_is_incomplete = |termination: &AngularTermination| -> Result<bool, CodecError> {
        Ok(angular_termination_is_incomplete(ctx, termination)?
            || angular_termination_dependency_is_incomplete(ctx, termination, dependencies)?)
    };
    Ok(planar_profile_ref_is_incomplete(ctx, profile)?
        || planar_profile_dependency_is_incomplete(ctx, profile, dependencies)?
        || match extent {
            RevolveExtent::OneSided { termination } | RevolveExtent::Symmetric { termination } => {
                side_is_incomplete(termination)?
            }
            RevolveExtent::TwoSided { first, second } => {
                side_is_incomplete(first)? || side_is_incomplete(second)?
            }
        }
        || match &axis.reference {
            Some(reference) => path_ref_is_incomplete(ctx, reference)?,
            None => false,
        }
        || solid.is_none()
        || matches!(op, BooleanOp::Unresolved))
}

/// Whether a vertex operand is unresolved. Generated and historical vertices
/// are complete: their identities and native references are nonblank by
/// construction.
fn vertex_selection_is_incomplete(vertex: &VertexSelection) -> bool {
    match vertex {
        VertexSelection::Generated { .. } | VertexSelection::Historical { .. } => false,
        VertexSelection::Unresolved | VertexSelection::Native(_) => true,
    }
}

pub(super) fn termination_is_incomplete(
    ctx: &DecodeContext<'_>,
    termination: &LinearTermination,
) -> Result<bool, CodecError> {
    match termination {
        LinearTermination::Unresolved {} => Ok(true),
        LinearTermination::ToFace { face, .. } => face_selection_is_incomplete(ctx, face),
        LinearTermination::ToVertex { vertex } => Ok(vertex_selection_is_incomplete(vertex)),
        LinearTermination::OffsetFromFace { face, .. } => {
            face_selection_is_incomplete(ctx, face)
        }
        LinearTermination::ToShape { target } => face_selection_is_incomplete(ctx, target),
        LinearTermination::Blind { .. } => Ok(false),
        LinearTermination::ThroughAll {}
        | LinearTermination::ThroughNext {}
        | LinearTermination::ToFirst {}
        | LinearTermination::ToLast {} => Ok(false),
    }
}

pub(super) fn termination_dependency_is_incomplete(
    ctx: &DecodeContext<'_>,
    termination: &LinearTermination,
    dependencies: &[FeatureId],
) -> Result<bool, CodecError> {
    match termination {
        LinearTermination::ToVertex {
            vertex: VertexSelection::Generated { vertex, .. },
        } => Ok(!ctx.contains(dependencies, &vertex.feature, DEPENDENCIES)?),
        _ => Ok(false),
    }
}

fn angular_termination_is_incomplete(
    ctx: &DecodeContext<'_>,
    termination: &AngularTermination,
) -> Result<bool, CodecError> {
    match termination {
        AngularTermination::Unresolved {} => Ok(true),
        AngularTermination::ToFace { face, .. } => face_selection_is_incomplete(ctx, face),
        AngularTermination::ToVertex { vertex } => Ok(vertex_selection_is_incomplete(vertex)),
        AngularTermination::OffsetFromFace { face, .. } => {
            face_selection_is_incomplete(ctx, face)
        }
        AngularTermination::ToShape { target } => face_selection_is_incomplete(ctx, target),
        AngularTermination::Angle { .. } => Ok(false),
        AngularTermination::ThroughAll {}
        | AngularTermination::ThroughNext {}
        | AngularTermination::ToFirst {}
        | AngularTermination::ToLast {} => Ok(false),
    }
}

fn angular_termination_dependency_is_incomplete(
    ctx: &DecodeContext<'_>,
    termination: &AngularTermination,
    dependencies: &[FeatureId],
) -> Result<bool, CodecError> {
    match termination {
        AngularTermination::ToVertex {
            vertex: VertexSelection::Generated { vertex, .. },
        } => Ok(!ctx.contains(dependencies, &vertex.feature, DEPENDENCIES)?),
        _ => Ok(false),
    }
}

pub(super) fn rib_feature_is_incomplete(
    ctx: &DecodeContext<'_>,
    construction: &RibConstruction,
    op: BooleanOp,
) -> Result<bool, CodecError> {
    Ok(match &construction.profile {
        Some(profile) => planar_profile_ref_is_incomplete(ctx, profile)?,
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

pub(super) fn sweep_orientation_is_incomplete(
    ctx: &DecodeContext<'_>,
    orientation: &SweepOrientation,
) -> Result<bool, CodecError> {
    match orientation {
        SweepOrientation::Auxiliary { path, .. } => path_ref_is_incomplete(ctx, path),
        SweepOrientation::GuideSurface { faces } => face_selection_is_incomplete(ctx, faces),
        SweepOrientation::Binormal { .. } => Ok(false),
        SweepOrientation::CorrectedFrenet {}
        | SweepOrientation::Fixed {}
        | SweepOrientation::Frenet {} => Ok(false),
    }
}

pub(super) fn pattern_is_incomplete<C: cadmpeg_ir::features::patterns::CompositeStages>(
    ctx: &DecodeContext<'_>,
    pattern: &PatternKind<C>,
) -> Result<bool, CodecError> {
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
            Some(path) => path_ref_is_incomplete(ctx, path)?,
            None => true,
        } || *count < 2),
        PatternTransform::Scale { center, .. } => Ok(matches!(
            center,
            cadmpeg_ir::features::patterns::PatternScaleCenter::Native(_)
        )),
        PatternTransform::Composite { stages } => ctx.any_by(
            stages.stages(),
            |stage| pattern_is_incomplete(ctx, &stage.pattern),
            "nx pattern stages",
        ),
    }
}

pub(crate) fn pattern_feature_is_incomplete(
    ctx: &DecodeContext<'_>,
    seeds: &[cadmpeg_ir::features::patterns::PatternSeed],
    pattern: &PatternKind,
    dependencies: &[cadmpeg_ir::features::FeatureId],
) -> Result<bool, CodecError> {
    Ok(seeds.is_empty()
        || ctx.any_by(
            seeds,
            |seed| match seed {
                cadmpeg_ir::features::patterns::PatternSeed::Feature(feature) => {
                    Ok(!ctx.contains(dependencies, feature, DEPENDENCIES)?)
                }
                cadmpeg_ir::features::patterns::PatternSeed::Faces(faces) => {
                    face_selection_is_incomplete(ctx, faces)
                }
                cadmpeg_ir::features::patterns::PatternSeed::Bodies(bodies) => {
                    Ok(body_selection_is_incomplete(bodies))
                }
                cadmpeg_ir::features::patterns::PatternSeed::Occurrences(occurrences) => {
                    Ok(occurrences.is_empty())
                }
            },
            "nx pattern seeds",
        )?
        || {
            let operation = "nx pattern seed duplicates";
            let mut visits = seeds.iter();
            let mut index = 0;
            loop {
                if visits.as_slice().is_empty() {
                    break false;
                }
                let Some(seed) = ctx.next_charged(&mut visits, operation)? else {
                    break false;
                };
                let duplicate = ctx.contains(&seeds[..index], seed, operation)?;
                index += 1;
                if duplicate {
                    break true;
                }
            }
        }
        || pattern_is_incomplete(ctx, pattern)?)
}

/// The occurrence count of a complete pattern transform. Composite stages
/// are visited in order and never nest.
pub(crate) fn pattern_occurrence_count<C: cadmpeg_ir::features::patterns::CompositeStages>(
    ctx: &DecodeContext<'_>,
    pattern: &PatternKind<C>,
) -> Result<Option<usize>, CodecError> {
    Ok(match pattern.definition() {
        PatternTransform::Linear { count, .. }
        | PatternTransform::Circular { count, .. }
        | PatternTransform::CurveDriven { count, .. }
        | PatternTransform::Scale { count, .. } => usize::try_from(*count).ok(),
        PatternTransform::LinearOffsets { offsets, .. } => Some(offsets.len()),
        PatternTransform::CircularAngles { angles, .. } => Some(angles.len()),
        PatternTransform::Mirror { .. } | PatternTransform::MirrorReference { .. } => Some(2),
        PatternTransform::Composite { stages } => {
            // The occurrence product so far; `None` before the first stage.
            let mut occurrences = None::<usize>;
            let incomplete = ctx.any_by(
                stages.stages(),
                |stage| {
                    let Some(stage_count) = pattern_occurrence_count(ctx, &stage.pattern)?
                    else {
                        return Ok(true);
                    };
                    occurrences = match occurrences {
                        None => Some(stage_count),
                        Some(current)
                            if matches!(
                                stage.pattern.definition(),
                                PatternTransform::Scale { .. }
                            ) =>
                        {
                            (current.checked_rem(stage_count) == Some(0)).then_some(current)
                        }
                        Some(current) => current.checked_mul(stage_count),
                    };
                    Ok(occurrences.is_none())
                },
                "nx pattern stage occurrences",
            )?;
            if incomplete {
                None
            } else {
                occurrences
            }
        }
        PatternTransform::Unresolved { .. } => None,
    })
}

pub(crate) fn body_selection_is_incomplete(selection: &BodySelection) -> bool {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            bodies.is_empty()
        }
        // Local members are nonempty, distinct and nonblank, and the native
        // reference is nonblank, by construction.
        BodySelection::ResolvedSet { .. } | BodySelection::Local { .. } => false,
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Native(_)
        | BodySelection::NativeSet(_) => true,
    }
}

pub(in crate::decode) fn face_selection_is_incomplete(
    ctx: &DecodeContext<'_>,
    selection: &FaceSelection,
) -> Result<bool, CodecError> {
    match selection {
        FaceSelection::Unresolved
        | FaceSelection::Generated { .. }
        | FaceSelection::Native(_)
        | FaceSelection::Historical { .. }
        | FaceSelection::HistoricalPartial { .. } => Ok(true),
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => {
            selection_ids_are_incomplete(ctx, faces)
        }
    }
}

pub(super) fn edge_selection_is_incomplete(
    ctx: &DecodeContext<'_>,
    selection: &EdgeSelection,
) -> Result<bool, CodecError> {
    match selection {
        EdgeSelection::Unresolved
        | EdgeSelection::Generated { .. }
        | EdgeSelection::Native(_)
        | EdgeSelection::Historical { .. }
        | EdgeSelection::HistoricalPartial { .. } => Ok(true),
        EdgeSelection::All => Ok(false),
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => {
            selection_ids_are_incomplete(ctx, edges)
        }
    }
}

pub(super) fn profile_ref_is_incomplete(
    ctx: &DecodeContext<'_>,
    profile: &ProfileRef,
) -> Result<bool, CodecError> {
    match profile {
        ProfileRef::SpatialSketchSelection { .. } => Ok(true),
        ProfileRef::SpatialSketchProfiles { .. } => Ok(false),
        ProfileRef::Planar(planar) => planar_profile_ref_is_incomplete(ctx, planar),
    }
}

pub(super) fn planar_profile_ref_is_incomplete(
    ctx: &DecodeContext<'_>,
    profile: &PlanarProfileRef,
) -> Result<bool, CodecError> {
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
            let operation = "nx generated profile curve duplicates";
            let mut visits = curves.iter();
            let mut index = 0;
            loop {
                if visits.as_slice().is_empty() {
                    break Ok(false);
                }
                let Some(curve) = ctx.next_charged(&mut visits, operation)? else {
                    break Ok(false);
                };
                let duplicate = ctx.contains(&curves[..index], curve, operation)?;
                index += 1;
                if duplicate {
                    break Ok(true);
                }
            }
        }
        PlanarProfileRef::Faces(faces) => selection_ids_are_incomplete(ctx, faces),
    }
}

pub(super) fn profile_dependency_is_incomplete(
    ctx: &DecodeContext<'_>,
    profile: &ProfileRef,
    dependencies: &[FeatureId],
) -> Result<bool, CodecError> {
    match profile {
        ProfileRef::Planar(planar) => {
            planar_profile_dependency_is_incomplete(ctx, planar, dependencies)
        }
        ProfileRef::SpatialSketchProfiles { .. } | ProfileRef::SpatialSketchSelection { .. } => {
            Ok(false)
        }
    }
}

pub(super) fn planar_profile_dependency_is_incomplete(
    ctx: &DecodeContext<'_>,
    profile: &PlanarProfileRef,
    dependencies: &[FeatureId],
) -> Result<bool, CodecError> {
    match profile {
        PlanarProfileRef::Feature(feature) => {
            Ok(!ctx.contains(dependencies, feature, DEPENDENCIES)?)
        }
        PlanarProfileRef::Generated { curves, .. } => ctx.any_by(
            curves,
            |curve| Ok(!ctx.contains(dependencies, &curve.feature, DEPENDENCIES)?),
            "nx generated profile curves",
        ),
        _ => Ok(false),
    }
}

pub(super) fn loft_section_is_incomplete(
    ctx: &DecodeContext<'_>,
    section: &LoftSection,
) -> Result<bool, CodecError> {
    match section {
        LoftSection::Profile(profile) => profile_ref_is_incomplete(ctx, profile),
        LoftSection::Point(LoftPointSection::Native(_)) => Ok(true),
        LoftSection::Point(LoftPointSection::Point(_)) => Ok(false),
        LoftSection::Point(LoftPointSection::Vertex(_)) => Ok(false),
    }
}

fn selection_ids_are_incomplete<T>(
    ctx: &DecodeContext<'_>,
    ids: &[T],
) -> Result<bool, CodecError>
where
    T: Ord + DecodeCost,
{
    const OPERATION: &str = "nx selection duplicates";
    if ids.is_empty() {
        return Ok(true);
    }
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut seen = BTreeSet::new();
    let mut visits = ids.iter();
    while !visits.as_slice().is_empty() {
        let Some(id) = ctx.next_charged(&mut visits, OPERATION)? else {
            break;
        };
        if !storage.with_storage(|| ctx.insert_btree_set(&mut seen, id, OPERATION))? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(in crate::decode) fn path_ref_is_incomplete(
    ctx: &DecodeContext<'_>,
    path: &PathRef,
) -> Result<bool, CodecError> {
    match path {
        PathRef::Unresolved(_) | PathRef::Native(_) | PathRef::SpatialSketchSelection { .. } => {
            Ok(true)
        }
        PathRef::HistoricalEdges { .. } => Ok(false),
        PathRef::Sketch(_) => Ok(false),
        PathRef::SketchCurves { .. } => Ok(false),
        PathRef::SpatialSketchCurves { .. } => Ok(false),
        PathRef::Edges(edges) => selection_ids_are_incomplete(ctx, edges),
        PathRef::Curves(curves) => selection_ids_are_incomplete(ctx, curves),
    }
}

#[cfg(test)]
mod tests;
