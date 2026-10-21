// SPDX-License-Identifier: Apache-2.0
//! Reject a contradictory source cache before Design can bind its face part.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::document::Model;
use cadmpeg_ir::eval::{admission::EvaluationAdmission, EvaluationFailure};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
use cadmpeg_ir::CadIr;

use crate::{loss::F3dLossCode, native::F3dNative};

fn point(
    result: Result<FinitePoint3, EvaluationFailure<Point3>>,
) -> Result<Option<Point3>, CodecError> {
    match result {
        Ok(point) => Ok(Some(point.get())),
        Err(EvaluationFailure::ResourceLimit(limit)) => Err(limit.into()),
        Err(
            EvaluationFailure::NoValue
            | EvaluationFailure::WorkExhausted
            | EvaluationFailure::NonFinite(_),
        ) => Ok(None),
    }
}

/// A finite counterexample rejects a cache. An unevaluated sample proves
/// nothing; the ordinary construction checks still apply to that document.
pub(crate) fn recover(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    native: &mut F3dNative,
    losses: &mut Vec<LossNote>,
) -> Result<bool, CodecError> {
    let mut rejected = BTreeSet::new();
    for surface in &ir.model.surfaces {
        let SurfaceGeometry::Procedural {
            construction,
            cache: Some(cache @ SolvedSurfaceGeometry::Nurbs(nurbs)),
        } = &surface.geometry
        else {
            continue;
        };
        let Some(definition) = ir
            .model
            .procedural_surfaces
            .iter()
            .find(|row| row.id == *construction)
        else {
            continue;
        };
        let domain = [
            [
                nurbs.u_knots()
                    [usize::try_from(nurbs.u_degree()).map_err(CodecError::malformed)?],
                nurbs.u_knots()[nurbs.u_count()],
            ],
            [
                nurbs.v_knots()
                    [usize::try_from(nurbs.v_degree()).map_err(CodecError::malformed)?],
                nurbs.v_knots()[nurbs.v_count()],
            ],
        ];
        // Same existing allowance as neutral procedural-cache validation.
        let baseline = cadmpeg_ir::units::COINCIDENCE_TOLERANCE;
        let allowance = baseline
            + baseline.max(ir.tolerances.linear.get()).max(
                definition
                    .cache_fit_tolerance()
                    .map_or(0.0, cadmpeg_ir::geometry::FitTolerance::get),
            );
        let cacheless = SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        };
        let mut counterexample = None;
        for u in [
            domain[0][0],
            domain[0][0].midpoint(domain[0][1]),
            domain[0][1],
        ] {
            for v in [
                domain[1][0],
                domain[1][0].midpoint(domain[1][1]),
                domain[1][1],
            ] {
                let actual = point(cadmpeg_ir::eval::model_surface_point(
                    EvaluationAdmission::Decode(ctx),
                    ir,
                    &cacheless,
                    u,
                    v,
                ))?;
                let cached = point(cadmpeg_ir::eval::decode::surface_point_solved(
                    EvaluationAdmission::Decode(ctx),
                    cache,
                    u,
                    v,
                ))?;
                if let (Some(actual), Some(cached)) = (actual, cached) {
                    let distance = Point3::distance(actual, cached);
                    if distance > allowance {
                        counterexample = Some((u, v, actual, cached, distance));
                        break;
                    }
                }
            }
            if counterexample.is_some() {
                break;
            }
        }
        if let Some((u, v, actual, cached, distance)) = counterexample {
            rejected.insert(surface.id.clone());
            losses.push(F3dLossCode::SurfaceCacheRejected.note(format!(
                "surface {} source cache rejected: at ({u}, {v}) construction {actual:?} and cache {cached:?} differ by {distance}, beyond allowance {allowance}; retained construction and source bytes",
                surface.id
            )));
        }
    }
    if rejected.is_empty() {
        return Ok(false);
    }
    for surface in &mut ir.model.surfaces {
        if rejected.contains(&surface.id) {
            if let SurfaceGeometry::Procedural { cache, .. } = &mut surface.geometry {
                *cache = None;
            }
        }
    }
    let mut faces = BTreeSet::new();
    for face in &ir.model.faces {
        if rejected.contains(&face.surface) {
            if let Some(proof) = boundary_contradicts_extrusion(ir, face) {
                faces.insert(face.id.as_str().to_owned());
                losses.push(F3dLossCode::SurfaceCacheRejected.note(format!(
                    "face {} omitted independently of its rejected cache: {proof}; retained original source bytes",
                    face.id
                )));
            }
        }
    }
    let removed = omit_face_parts(ctx, &mut ir.model, &faces)?;
    // These are the only native columns present at this pre-Design boundary.
    macro_rules! retain {
        ($($field:ident),* $(,)?) => {$( retain_references(ctx, &mut native.$field, &removed)?; )*};
    }
    retain!(
        body_native_keys,
        edge_continuities,
        edge_ownerships,
        vertex_ownerships,
        face_sidedness,
        face_native_keys,
        tolerant_coedge_parameters,
        tolerant_edge_tails,
        tolerant_vertex_tails,
        mesh_surface_sentinels,
        transform_hints,
        wire_topologies,
        sketch_curve_links,
        persistent_design_links,
        persistent_subentity_tags,
        creation_timestamps
    );
    retain_references(ctx, &mut ir.model.attributes, &removed)?;
    Ok(true)
}

/// The convex hull of a same-sign rational (or polynomial) directrix bounds
/// every coordinate perpendicular to an extrusion. A boundary vertex beyond
/// that hull cannot lie on the true surface, independently of the bad cache.
/// No inference is made for unsupported constructions or mixed-sign weights.
fn boundary_contradicts_extrusion(ir: &CadIr, face: &cadmpeg_ir::topology::Face) -> Option<String> {
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralSurfaceDefinition, SolvedCurveGeometry};
    let Some(surface) = ir.model.surfaces.iter().find(|row| row.id == face.surface) else {
        return None;
    };
    let SurfaceGeometry::Procedural { construction, .. } = &surface.geometry else {
        return None;
    };
    let Some(row) = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|row| row.id == *construction)
    else {
        return None;
    };
    let ProceduralSurfaceDefinition::Extrusion(extrusion) = row.definition() else {
        return None;
    };
    let Some(curve) = ir
        .model
        .curves
        .iter()
        .find(|row| row.id == *extrusion.directrix())
    else {
        return None;
    };
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &curve.geometry else {
        return None;
    };
    // This proof covers the legacy, unreversed finite directrix interval.
    // Extrapolated or revision-directed parameters need a separate proof.
    if extrusion.revision_form().is_some() {
        return None;
    }
    let [start, end] = extrusion.parameter_interval()?.get();
    let degree = usize::try_from(nurbs.degree()).ok()?;
    if start < nurbs.knots()[degree] || end > nurbs.knots()[nurbs.pole_count()] || start >= end {
        return None;
    }
    if let Some(weights) = nurbs.weights() {
        if !(weights.iter().all(|weight| weight.get() > 0.0)
            || weights.iter().all(|weight| weight.get() < 0.0))
        {
            return None;
        }
    }
    let direction: [f64; 3] = extrusion.direction().get().into();
    let poles: Vec<[f64; 3]> = nurbs
        .control_points()
        .into_iter()
        .map(|point| point.get().into())
        .collect();
    let vertices: BTreeSet<_> = ir
        .model
        .loops
        .iter()
        .filter(|row| row.face == face.id)
        .flat_map(|row| row.coedges().iter())
        .filter_map(|id| ir.model.coedges.iter().find(|row| row.id == *id))
        .filter_map(|coedge| ir.model.edges.iter().find(|edge| edge.id == coedge.edge))
        .flat_map(|edge| [edge.start.clone(), edge.end.clone()])
        .collect();
    for vertex in ir
        .model
        .vertices
        .iter()
        .filter(|row| vertices.contains(&row.id))
    {
        let Some(point) = ir.model.points.iter().find(|row| row.id == vertex.point) else {
            continue;
        };
        let position: [f64; 3] = point.position().get().into();
        let tolerance = ir
            .tolerances
            .linear
            .get()
            .max(
                face.tolerance
                    .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
            )
            .max(
                vertex
                    .tolerance
                    .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
            );
        for axis in 0..3 {
            if direction[axis] != 0.0 {
                continue;
            }
            let low = poles
                .iter()
                .map(|point| point[axis])
                .fold(f64::INFINITY, f64::min);
            let high = poles
                .iter()
                .map(|point| point[axis])
                .fold(f64::NEG_INFINITY, f64::max);
            if position[axis] < low - tolerance || position[axis] > high + tolerance {
                return Some(format!("vertex {} coordinate {axis}={} lies outside the true extrusion directrix hull [{low}, {high}] by more than {tolerance}", vertex.id, position[axis]));
            }
        }
    }
    None
}

fn retain_references<T: RewriteIdentities>(
    ctx: &DecodeContext<'_>,
    rows: &mut Vec<T>,
    removed: &BTreeSet<String>,
) -> Result<(), CodecError> {
    let mut kept = Vec::new();
    for row in std::mem::take(rows) {
        let mut missing = false;
        row.visit_identity_references(ctx, &mut |id| {
            missing |= removed.contains(id);
            Ok(())
        })?;
        if !missing {
            kept.push(row);
        }
    }
    *rows = kept;
    Ok(())
}

/// Remove only the closure formerly owned by rejected faces. Standalone
/// geometry and dependencies still used by a sibling remain document content.
fn omit_face_parts(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
    faces: &BTreeSet<String>,
) -> Result<BTreeSet<String>, CodecError> {
    let mut removed = faces.clone();
    let possible_surfaces: BTreeSet<_> = model
        .faces
        .iter()
        .filter(|face| faces.contains(face.id.as_str()))
        .map(|face| face.surface.as_str().to_owned())
        .collect();
    let loops: BTreeSet<_> = model
        .loops
        .iter()
        .filter(|row| faces.contains(row.face.as_str()))
        .map(|row| row.id.as_str().to_owned())
        .collect();
    let original_radials: BTreeMap<_, _> = model
        .coedges
        .iter()
        .map(|row| (row.id.clone(), row.radial_next.clone()))
        .collect();
    let mut possible_edges = BTreeSet::new();
    for coedge in &model.coedges {
        if loops.contains(coedge.owner_loop.as_str()) {
            removed.insert(coedge.id.as_str().to_owned());
            possible_edges.insert(coedge.edge.clone());
        }
    }
    removed.extend(loops.iter().cloned());
    model.faces.retain(|row| !faces.contains(row.id.as_str()));
    model.loops.retain(|row| !loops.contains(row.id.as_str()));
    model
        .coedges
        .retain(|row| !removed.contains(row.id.as_str()));
    for coedge in &mut model.coedges {
        let mut next = &original_radials[&coedge.id];
        let mut visited = BTreeSet::new();
        while removed.contains(next.as_str()) {
            if !visited.insert(next.clone()) {
                return Err(CodecError::malformed(
                    "rejected source face has an unclosed radial ring",
                ));
            }
            next = original_radials.get(next).ok_or_else(|| {
                CodecError::malformed("rejected source face has a dangling radial reference")
            })?;
        }
        coedge.radial_next = next.clone();
    }
    let kept_edges: BTreeSet<_> = model
        .coedges
        .iter()
        .map(|row| row.edge.clone())
        .chain(
            model
                .shells
                .iter()
                .flat_map(|row| row.wire_edges().iter().cloned()),
        )
        .collect();
    let removed_edges: BTreeSet<_> = possible_edges.difference(&kept_edges).cloned().collect();
    let possible_vertices: BTreeSet<_> = model
        .edges
        .iter()
        .filter(|row| removed_edges.contains(&row.id))
        .flat_map(|row| [row.start.clone(), row.end.clone()])
        .collect();
    model.edges.retain(|row| !removed_edges.contains(&row.id));
    removed.extend(removed_edges.iter().map(|id| id.as_str().to_owned()));
    let kept_vertices: BTreeSet<_> = model
        .edges
        .iter()
        .flat_map(|row| [row.start.clone(), row.end.clone()])
        .chain(
            model
                .shells
                .iter()
                .flat_map(|row| row.free_vertices().iter().cloned()),
        )
        .chain(model.loops.iter().flat_map(|row| row.vertices().cloned()))
        .collect();
    let removed_vertices: BTreeSet<_> = possible_vertices
        .difference(&kept_vertices)
        .cloned()
        .collect();
    let possible_points: BTreeSet<_> = model
        .vertices
        .iter()
        .filter(|row| removed_vertices.contains(&row.id))
        .map(|row| row.point.clone())
        .collect();
    model
        .vertices
        .retain(|row| !removed_vertices.contains(&row.id));
    removed.extend(removed_vertices.iter().map(|id| id.as_str().to_owned()));
    let kept_points: BTreeSet<_> = model.vertices.iter().map(|row| row.point.clone()).collect();
    let removed_points: BTreeSet<_> = possible_points.difference(&kept_points).cloned().collect();
    model.points.retain(|row| !removed_points.contains(&row.id));
    removed.extend(removed_points.iter().map(|id| id.as_str().to_owned()));
    let affected_shells: BTreeSet<_> = model
        .shells
        .iter()
        .filter(|shell| shell.faces().iter().any(|id| faces.contains(id.as_str())))
        .map(|shell| shell.id.clone())
        .collect();
    let changed_regions: BTreeSet<_> = model
        .shells
        .iter()
        .filter(|shell| affected_shells.contains(&shell.id))
        .map(|shell| shell.region.clone())
        .collect();
    let changed_bodies: BTreeSet<_> = model
        .regions
        .iter()
        .filter(|region| changed_regions.contains(&region.id))
        .map(|region| region.body.clone())
        .collect();
    let mut shells = Vec::new();
    for mut shell in std::mem::take(&mut model.shells) {
        let kept = shell
            .faces()
            .iter()
            .filter(|id| !faces.contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !affected_shells.contains(&shell.id) {
            shells.push(shell);
        } else if kept.is_empty()
            && shell.wire_edges().is_empty()
            && shell.free_vertices().is_empty()
        {
            removed.insert(shell.id.as_str().to_owned());
        } else {
            shell.replace_faces(kept).map_err(CodecError::malformed)?;
            shells.push(shell);
        }
    }
    model.shells = shells;
    let affected_regions: BTreeSet<_> = model
        .regions
        .iter()
        .filter(|region| region.shells.iter().any(|id| removed.contains(id.as_str())))
        .map(|region| region.id.clone())
        .collect();
    for region in &mut model.regions {
        region.shells.retain(|id| !removed.contains(id.as_str()));
    }
    model.regions.retain(|row| {
        if affected_regions.contains(&row.id) && row.shells.is_empty() {
            removed.insert(row.id.as_str().to_owned());
            false
        } else {
            true
        }
    });
    let affected_bodies: BTreeSet<_> = model
        .bodies
        .iter()
        .filter(|body| body.regions.iter().any(|id| removed.contains(id.as_str())))
        .map(|body| body.id.clone())
        .collect();
    for body in &mut model.bodies {
        body.regions.retain(|id| !removed.contains(id.as_str()));
    }
    model.bodies.retain(|row| {
        if affected_bodies.contains(&row.id) && row.regions.is_empty() {
            removed.insert(row.id.as_str().to_owned());
            false
        } else {
            true
        }
    });
    // A source Solid may lose a boundary face. Reuse ASM's semantic
    // classification on the surviving uses, only for touched bodies.
    let collapsed: BTreeSet<_> = model
        .edges
        .iter()
        .filter(|edge| {
            edge.start == edge.end
                && edge.curve().is_some_and(|id| {
                    model.curves.iter().any(|curve| {
                        curve.id == *id
                            && matches!(
                                curve.geometry,
                                cadmpeg_ir::geometry::CurveGeometry::Solved(
                                    cadmpeg_ir::geometry::SolvedCurveGeometry::Degenerate(_)
                                )
                            )
                    })
                })
        })
        .map(|edge| edge.id.clone())
        .collect();
    for body in &mut model.bodies {
        if !changed_bodies.contains(&body.id) {
            continue;
        }
        let regions: BTreeSet<_> = body.regions.iter().collect();
        let shells: Vec<_> = model
            .shells
            .iter()
            .filter(|shell| regions.contains(&shell.region))
            .collect();
        let faces: BTreeSet<_> = shells
            .iter()
            .flat_map(|shell| shell.faces().iter())
            .collect();
        let loops: BTreeSet<_> = model
            .loops
            .iter()
            .filter(|row| faces.contains(&row.face))
            .map(|row| &row.id)
            .collect();
        body.kind = if faces.is_empty() {
            cadmpeg_ir::topology::BodyKind::Wire
        } else if shells
            .iter()
            .any(|shell| !shell.wire_edges().is_empty() || !shell.free_vertices().is_empty())
        {
            cadmpeg_ir::topology::BodyKind::General
        } else {
            cadmpeg_asm::brep::geometry::faced_body_kind(
                model
                    .coedges
                    .iter()
                    .filter(|row| loops.contains(&row.owner_loop))
                    .map(|row| (&row.edge, row.sense)),
                |edge| collapsed.contains(edge),
            )
        };
    }
    // A solved face-owned carrier has no standalone semantic owner after
    // its final face is omitted. Preserve it if any surviving typed record
    // still refers to it; procedural carriers keep their true construction.
    struct References<'a, 'ctx> {
        ctx: &'a DecodeContext<'ctx>,
        used: BTreeSet<String>,
        candidates: &'a BTreeSet<String>,
    }
    impl cadmpeg_ir::document::EntityRewrite for References<'_, '_> {
        type Error = CodecError;
        fn rewrite<T: RewriteIdentities>(&mut self, entity: T) -> Result<T, CodecError> {
            entity.visit_identity_references(self.ctx, &mut |id| {
                if self.candidates.contains(id) {
                    self.used.insert(id.to_owned());
                }
                Ok(())
            })?;
            Ok(entity)
        }
    }
    let mut references = References {
        ctx,
        used: BTreeSet::new(),
        candidates: &possible_surfaces,
    };
    // The structural visitor includes entity declarations too. Exclude the
    // Surface declaration slot itself, while still walking its actual data.
    let surfaces = std::mem::take(&mut model.surfaces);
    for surface in &surfaces {
        if surface.source_object.is_some() {
            references.used.insert(surface.id.as_str().to_owned());
        }
        surface.geometry.visit_identity_references(ctx, &mut |id| {
            if possible_surfaces.contains(id) {
                references.used.insert(id.to_owned());
            }
            Ok(())
        })?;
        surface
            .source_object
            .visit_identity_references(ctx, &mut |id| {
                if possible_surfaces.contains(id) {
                    references.used.insert(id.to_owned());
                }
                Ok(())
            })?;
    }
    let original = std::mem::take(model);
    model.extend_rewritten(original, &mut references)?;
    model.surfaces = surfaces;
    model.surfaces.retain(|surface| {
        let id = surface.id.as_str();
        if possible_surfaces.contains(id)
            && !references.used.contains(id)
            && matches!(surface.geometry, SurfaceGeometry::Solved(_))
        {
            removed.insert(id.to_owned());
            false
        } else {
            true
        }
    });
    Ok(removed)
}

#[cfg(test)]
mod tests;
