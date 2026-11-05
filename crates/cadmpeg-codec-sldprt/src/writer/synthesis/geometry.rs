// SPDX-License-Identifier: Apache-2.0
//! Keep writable sheet faces when one unsupported face does not invalidate a sheet.

use std::collections::HashSet;

use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, EllipseCurve, LineCurve, PlaneSurface,
    SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::{
    CurveGeometry, PlacedCurve, PlacedSurface, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{BodyId, FaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{Body, BodyKind};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::loss::SldprtLossCode;

pub(super) fn convert_polylines(ir: &mut CadIr, losses: &mut Vec<LossNote>) {
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};

    fn convert(geometry: &SolvedCurveGeometry) -> Option<SolvedCurveGeometry> {
        let polyline = match geometry {
            SolvedCurveGeometry::Polyline(polyline) => polyline,
            SolvedCurveGeometry::Transformed(placed) => {
                let basis = convert(placed.basis())?;
                return PlacedCurve::try_new(Box::new(basis), *placed.transform())
                    .ok()
                    .map(SolvedCurveGeometry::Transformed);
            }
            _ => return None,
        };
        let Some(mut samples) = (0..polyline.point_count())
            .map(|index| {
                Some((
                    polyline.parameter_at(index)?.get(),
                    polyline.point_at(index)?,
                ))
            })
            .collect::<Option<Vec<_>>>()
        else {
            return None;
        };
        if samples.len() < 2 {
            return None;
        }
        if samples[0].0 > samples[samples.len() - 1].0 {
            samples.reverse();
        }
        let mut knots = Vec::new();
        knots.push(samples[0].0);
        knots.extend(samples.iter().map(|sample| sample.0));
        knots.push(samples[samples.len() - 1].0);
        NurbsCurve::new(
            1,
            knots,
            NurbsPoles3::Polynomial {
                points: samples.into_iter().map(|sample| sample.1).collect(),
            },
            false,
        )
        .ok()
        .map(SolvedCurveGeometry::Nurbs)
    }

    for curve in &mut ir.model.curves {
        let CurveGeometry::Solved(geometry) = &curve.geometry else {
            continue;
        };
        let Some(converted) = convert(geometry) else {
            continue;
        };
        losses.push(SldprtLossCode::WriterPolylineConverted.note(format!(
            "curve {}: polyline chordal deflection omitted during degree-one NURBS conversion",
            curve.id
        )));
        curve.geometry = CurveGeometry::Solved(converted);
    }
}

/// Fold rigid placements into the carriers the lump writer encodes.
pub(super) fn resolve_placements(ir: &mut CadIr, losses: &mut Vec<LossNote>) {
    for surface in &mut ir.model.surfaces {
        let resolved = match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(placed)) => {
                resolve_surface(placed)
            }
            _ => None,
        };
        if let Some(resolved) = resolved {
            losses.push(SldprtLossCode::WriterPlacementResolved.note(format!(
                "surface {}: placement resolved into the carrier",
                surface.id
            )));
            surface.geometry = SurfaceGeometry::Solved(resolved);
        }
    }
    for curve in &mut ir.model.curves {
        let resolved = match &curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Transformed(placed)) => {
                resolve_curve(placed)
            }
            _ => None,
        };
        if let Some(resolved) = resolved {
            losses.push(SldprtLossCode::WriterPlacementResolved.note(format!(
                "curve {}: placement resolved into the carrier",
                curve.id
            )));
            curve.geometry = CurveGeometry::Solved(resolved);
        }
    }
}

fn resolve_surface(placed: &PlacedSurface) -> Option<SolvedSurfaceGeometry> {
    let transform = *placed.transform();
    if !transform.is_proper_rigid() {
        return None;
    }
    let basis = match placed.basis() {
        SolvedSurfaceGeometry::Transformed(inner) => resolve_surface(inner)?,
        basis => basis.clone(),
    };
    apply_surface_transform(&basis, transform)
}

fn resolve_curve(placed: &PlacedCurve) -> Option<SolvedCurveGeometry> {
    let transform = *placed.transform();
    if !transform.is_proper_rigid() {
        return None;
    }
    let basis = match placed.basis() {
        SolvedCurveGeometry::Transformed(inner) => resolve_curve(inner)?,
        basis => basis.clone(),
    };
    apply_curve_transform(&basis, transform)
}

fn moved_point(transform: Transform, point: Point3) -> Option<Point3> {
    transform.apply_point(point).map(cadmpeg_ir::features::FinitePoint3::get)
}

fn moved_direction(transform: Transform, direction: Vector3) -> Option<Vector3> {
    transform.apply_vector(direction).map(cadmpeg_ir::features::FiniteVector3::get)
}

fn apply_surface_transform(
    basis: &SolvedSurfaceGeometry,
    transform: Transform,
) -> Option<SolvedSurfaceGeometry> {
    match basis {
        SolvedSurfaceGeometry::Plane(plane) => {
            let origin = moved_point(transform, plane.origin().get())?;
            let axis = moved_direction(transform, *plane.frame().axis().as_raw())?;
            let reference = moved_direction(transform, *plane.frame().reference().as_raw())?;
            PlaneSurface::try_new(origin, axis, reference)
                .ok()
                .map(SolvedSurfaceGeometry::Plane)
        }
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            let origin = moved_point(transform, cylinder.origin().get())?;
            let axis = moved_direction(transform, *cylinder.frame().axis().as_raw())?;
            let reference = moved_direction(transform, *cylinder.frame().reference().as_raw())?;
            CylinderSurface::try_new(origin, axis, reference, cylinder.radius().get())
                .ok()
                .map(SolvedSurfaceGeometry::Cylinder)
        }
        SolvedSurfaceGeometry::Cone(cone) => {
            let origin = moved_point(transform, cone.origin().get())?;
            let axis = moved_direction(transform, *cone.frame().axis().as_raw())?;
            let reference = moved_direction(transform, *cone.frame().reference().as_raw())?;
            ConeSurface::try_new(
                origin,
                axis,
                reference,
                cone.radius().get(),
                cone.ratio().get(),
                cone.half_angle().get(),
            )
            .ok()
            .map(SolvedSurfaceGeometry::Cone)
        }
        SolvedSurfaceGeometry::Sphere(sphere) => {
            let center = moved_point(transform, sphere.center().get())?;
            let axis = moved_direction(transform, *sphere.frame().axis().as_raw())?;
            let reference = moved_direction(transform, *sphere.frame().reference().as_raw())?;
            SphereSurface::try_new(center, axis, reference, sphere.radius().get())
                .ok()
                .map(SolvedSurfaceGeometry::Sphere)
        }
        SolvedSurfaceGeometry::Torus(torus) => {
            let center = moved_point(transform, torus.center().get())?;
            let axis = moved_direction(transform, *torus.frame().axis().as_raw())?;
            let reference = moved_direction(transform, *torus.frame().reference().as_raw())?;
            TorusSurface::try_new(
                center,
                axis,
                reference,
                torus.major_radius().get(),
                torus.minor_radius().get(),
            )
            .ok()
            .map(SolvedSurfaceGeometry::Torus)
        }
        SolvedSurfaceGeometry::Nurbs(surface) => {
            let mut surface = surface.clone();
            surface
                .try_map_control_points(|_, point| transform.apply_point(point.get()).ok_or(()))
                .ok()?;
            Some(SolvedSurfaceGeometry::Nurbs(surface))
        }
        SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Unknown { .. } => None,
    }
}

fn apply_curve_transform(
    basis: &SolvedCurveGeometry,
    transform: Transform,
) -> Option<SolvedCurveGeometry> {
    match basis {
        SolvedCurveGeometry::Line(line) => {
            let origin = moved_point(transform, line.origin().get())?;
            let direction = moved_direction(transform, *line.direction().as_raw())?;
            LineCurve::try_new(origin, direction)
                .ok()
                .map(SolvedCurveGeometry::Line)
        }
        SolvedCurveGeometry::Circle(circle) => {
            let center = moved_point(transform, circle.center().get())?;
            let axis = moved_direction(transform, *circle.frame().axis().as_raw())?;
            let reference = moved_direction(transform, *circle.frame().reference().as_raw())?;
            CircleCurve::try_new(center, axis, reference, circle.radius().get())
                .ok()
                .map(SolvedCurveGeometry::Circle)
        }
        SolvedCurveGeometry::Ellipse(ellipse) => {
            let center = moved_point(transform, ellipse.center().get())?;
            let axis = moved_direction(transform, *ellipse.frame().axis().as_raw())?;
            let reference = moved_direction(transform, *ellipse.frame().reference().as_raw())?;
            EllipseCurve::try_new(
                center,
                axis,
                reference,
                ellipse.major_radius().get(),
                ellipse.minor_radius().get(),
            )
            .ok()
            .map(SolvedCurveGeometry::Ellipse)
        }
        SolvedCurveGeometry::Nurbs(curve) => {
            let mut curve = curve.clone();
            curve
                .try_map_control_points(|_, point| transform.apply_point(point.get()).ok_or(()))
                .ok()?;
            Some(SolvedCurveGeometry::Nurbs(curve))
        }
        SolvedCurveGeometry::Parabola(_)
        | SolvedCurveGeometry::Hyperbola(_)
        | SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Polyline(_)
        | SolvedCurveGeometry::Transformed(_)
        | SolvedCurveGeometry::Unknown { .. } => None,
    }
}

pub(super) fn preflight(
    ir: &CadIr,
    body: &Body,
) -> Result<(), super::super::write_error::WriteError> {
    let mut candidate = super::super::body_subset(ir, std::slice::from_ref(&body.id))?;
    // Presentation is admitted separately; unrelated mesh ownership cannot
    // invalidate one body's geometry projection.
    candidate.model.tessellations.clear();
    crate::writer_transform::bake(&mut candidate)?;
    super::super::check_semantic_support(&candidate)?;
    super::super::brep_body(&candidate, 0.001, body.kind == BodyKind::Sheet)?;
    Ok(())
}

pub(super) fn reduce_sheet(
    ir: &mut CadIr,
    body: &Body,
    losses: &mut Vec<LossNote>,
) -> Result<bool, super::super::write_error::WriteError> {
    let original = super::super::body_subset(ir, std::slice::from_ref(&body.id))?;
    let mut kept = HashSet::new();
    for face in &original.model.faces {
        let mut candidate = original.clone();
        retain_faces(&mut candidate, &body.id, &HashSet::from([face.id.clone()]));
        match preflight(&candidate, body) {
            Ok(()) => {
                kept.insert(face.id.clone());
            }
            Err(super::super::write_error::WriteError::Decline(reason)) => losses.push(
                SldprtLossCode::WriterEntityOmitted
                    .note(format!("sheet face {} omitted: {reason}", face.id)),
            ),
            Err(error) => return Err(error),
        }
    }
    if kept.is_empty() {
        return Ok(false);
    }
    retain_faces(ir, &body.id, &kept);
    match preflight(ir, body) {
        Ok(()) => Ok(true),
        Err(super::super::write_error::WriteError::Decline(_)) => Ok(false),
        Err(error) => Err(error),
    }
}

fn retain_faces(ir: &mut CadIr, body: &BodyId, kept: &HashSet<FaceId>) {
    let regions = ir
        .model
        .regions
        .iter()
        .filter(|region| &region.body == body)
        .map(|region| region.id.clone())
        .collect::<HashSet<_>>();
    let originally_empty = ir
        .model
        .regions
        .iter()
        .filter(|region| &region.body == body && region.shells.is_empty())
        .map(|region| region.id.clone())
        .collect::<HashSet<_>>();
    let mut shells = Vec::new();
    for mut shell in std::mem::take(&mut ir.model.shells) {
        if regions.contains(&shell.region) {
            let faces = shell
                .faces()
                .iter()
                .filter(|face| kept.contains(*face))
                .cloned()
                .collect();
            if shell.replace_faces(faces).is_err() {
                continue;
            }
        }
        shells.push(shell);
    }
    ir.model.shells = shells;
    let shells = ir
        .model
        .shells
        .iter()
        .map(|shell| shell.id.clone())
        .collect::<HashSet<_>>();
    ir.model.regions.retain_mut(|region| {
        if &region.body != body {
            return true;
        }
        region.shells.retain(|shell| shells.contains(shell));
        !region.shells.is_empty() || originally_empty.contains(&region.id)
    });
    let regions = ir
        .model
        .regions
        .iter()
        .map(|region| region.id.clone())
        .collect::<HashSet<_>>();
    for candidate in &mut ir.model.bodies {
        if &candidate.id == body {
            candidate.regions.retain(|region| regions.contains(region));
        }
    }
}

/// Reduce composites to their segments when their carriers permit it.
pub(super) fn flatten_composite_curves(
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::topology::EdgeCarrier;

    fn leaf_of(
        geometries: &std::collections::BTreeMap<&str, &CurveGeometry>,
        id: &str,
    ) -> Option<String> {
        let mut current = id;
        let mut seen = std::collections::BTreeSet::new();
        loop {
            if !seen.insert(current.to_owned()) {
                return None;
            }
            match geometries.get(current)? {
                CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. }) => {
                    if segments.len() != 1 {
                        return None;
                    }
                    let segment = &segments[0];
                    if !segment.same_sense {
                        return None;
                    }
                    current = segment.curve.as_str();
                }
                _ => return Some(current.to_owned()),
            }
        }
    }

    fn has_segments<'a>(
        geometries: &std::collections::BTreeMap<&'a str, &'a CurveGeometry>,
        id: &'a str,
    ) -> bool {
        let mut active = std::collections::BTreeSet::new();
        let mut completed = std::collections::BTreeSet::new();
        let mut pending = vec![(id, false)];
        while let Some((current, done)) = pending.pop() {
            if done {
                active.remove(current);
                completed.insert(current);
                continue;
            }
            if completed.contains(current) {
                continue;
            }
            if !active.insert(current) {
                return false;
            }
            pending.push((current, true));
            match geometries.get(current) {
                Some(CurveGeometry::Solved(SolvedCurveGeometry::Composite {
                    segments, ..
                })) => {
                    pending.extend(
                        segments
                            .iter()
                            .map(|segment| (segment.curve.as_str(), false)),
                    );
                }
                Some(_) => {}
                None => return false,
            }
        }
        true
    }

    let geometries: std::collections::BTreeMap<&str, &CurveGeometry> = ir
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect();
    let mut flattened = std::collections::BTreeMap::new();
    for edge in &ir.model.edges {
        if let EdgeCarrier::Curve(id) = &edge.carrier {
            if let Some(leaf) = leaf_of(&geometries, id.as_str()) {
                if leaf != id.as_str() {
                    flattened.insert(id.as_str().to_owned(), leaf);
                }
            }
        }
    }
    let composites = ir
        .model
        .curves
        .iter()
        .filter(|curve| {
            matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
            )
        })
        .map(|curve| curve.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if composites.is_empty() {
        return Ok(());
    }
    let rewritten_edges = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| match &edge.carrier {
            EdgeCarrier::Curve(id) if flattened.contains_key(id.as_str()) => {
                Some((edge.id.as_str(), id.as_str()))
            }
            _ => None,
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::desktop(),
    )?;
    let mut retained = std::collections::BTreeSet::<String>::new();
    ir.visit_neutral_references(&ctx, &mut |owner, target| {
        if composites.contains(target)
            && !composites.contains(owner)
            && rewritten_edges.get(owner).copied() != Some(target)
        {
            retained.insert(target.to_owned());
        }
        Ok(())
    })?;
    let mut pending = retained.iter().cloned().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        if let Some(CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. })) =
            geometries.get(id.as_str())
        {
            for segment in segments {
                let id = segment.curve.as_str();
                if retained.insert(id.to_owned()) {
                    pending.push(id.to_owned());
                }
            }
        }
    }
    let mut dropped = Vec::new();
    for curve in &ir.model.curves {
        let composite = matches!(
            curve.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
        );
        if !composite {
            continue;
        }
        if !retained.contains(curve.id.as_str()) && has_segments(&geometries, curve.id.as_str()) {
            let message = match leaf_of(&geometries, curve.id.as_str()) {
                Some(leaf) => format!(
                    "composite curve {} written as its segment {}",
                    curve.id.as_str(),
                    leaf
                ),
                None => format!(
                    "composite curve {} reduced to its segments",
                    curve.id.as_str()
                ),
            };
            dropped.push((curve.id.clone(), message));
        }
    }
    let mut rewritten = Vec::new();
    for edge in &mut ir.model.edges {
        if let EdgeCarrier::Curve(id) = &edge.carrier {
            if let Some(leaf) = flattened.get(id.as_str()).cloned() {
                let Ok(leaf) = CurveId::mint(leaf) else {
                    continue;
                };
                rewritten.push((edge.id.clone(), id.clone(), leaf.clone()));
                edge.map_curve(|_| leaf);
            }
        }
    }
    let dropped_ids: std::collections::BTreeSet<&str> =
        dropped.iter().map(|(id, _)| id.as_str()).collect();
    ir.model.curves.retain(|curve| {
        !matches!(
            curve.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
        ) || !dropped_ids.contains(curve.id.as_str())
    });
    for (id, message) in dropped {
        let mut message = message;
        let edges: Vec<String> = rewritten
            .iter()
            .filter(|(_, from, _)| from.as_str() == id.as_str())
            .map(|(edge, _, _)| edge.as_str().to_owned())
            .collect();
        let coedges: Vec<String> = ir
            .model
            .coedges
            .iter()
            .filter(|coedge| edges.iter().any(|edge| edge == coedge.edge.as_str()))
            .map(|coedge| coedge.id.as_str().to_owned())
            .collect();
        let mut touched = edges;
        touched.extend(coedges);
        if !touched.is_empty() {
            message.push_str(&format!(
                "; edges and coedges now carried by the leaf: {}",
                touched.join(", ")
            ));
        }
        losses.push(SldprtLossCode::WriterEntityOmitted.note(message));
    }
    Ok(())
}

/// References removed by export reduction cannot invalidate unrelated design.
pub(super) fn reduce_feature_references(
    original: &CadIr,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut removed = HashSet::new();
    macro_rules! removed_arena { ($($arena:ident),* $(,)?) => { $(
        let retained = ir.model.$arena.iter().map(|record| record.id.as_str()).collect::<HashSet<_>>();
        removed.extend(original.model.$arena.iter().filter(|record| !retained.contains(record.id.as_str())).map(|record| record.id.as_str()));
    )* }; }
    removed_arena!(
        bodies,
        regions,
        shells,
        faces,
        loops,
        coedges,
        edges,
        vertices,
        points,
        surfaces,
        curves,
        feature_input_topologies,
        feature_result_topologies
    );
    if removed.is_empty() {
        return Ok(());
    }
    let validation = cadmpeg_ir::validate::validate_neutral(ir, Vec::new())?;
    let affected = validation
        .findings
        .iter()
        .filter(|finding| finding.severity >= cadmpeg_ir::report::Severity::Error)
        .filter(|finding| {
            finding
                .related
                .iter()
                .any(|related| removed.contains(related.as_str()))
        })
        .filter_map(|finding| finding.entity.as_deref())
        .collect::<HashSet<_>>();
    for feature in &mut ir.model.features {
        if affected.contains(feature.id.as_str()) {
            losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                "feature {}: construction references omitted geometry; solved outputs remain",
                feature.id
            )));
            super::reduce_feature_to_solved(feature)?;
        }
    }
    for configuration in &mut ir.model.configurations {
        if affected.contains(configuration.id.as_str()) && !configuration.feature_states.is_empty()
        {
            losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                "configuration {}: evaluated feature state references omitted geometry",
                configuration.id
            )));
            configuration.feature_states.clear();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::geometry::analytic::{CircleCurve, PlaneSurface};
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, PlacedCurve, PlacedSurface, SolvedCurveGeometry,
        SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::report::Severity;
    use cadmpeg_ir::transform::Transform;
    use cadmpeg_ir::CadIr;

    use super::{convert_polylines, resolve_placements};

    fn translated_plane() -> SurfaceGeometry {
        let basis = SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(12.0, 8.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("admitted plane basis"),
        );
        let placed = PlacedSurface::try_new(
            Box::new(basis),
            Transform::affine([
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 10.0],
            ])
            .expect("finite translation"),
        )
        .expect("admitted placement");
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(placed))
    }

    fn placed_circle(transform: Transform) -> CurveGeometry {
        let basis = SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(12.0, 8.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                3.0,
            )
            .expect("admitted circle basis"),
        );
        let placed = PlacedCurve::try_new(Box::new(basis), transform).expect("admitted placement");
        CurveGeometry::Solved(SolvedCurveGeometry::Transformed(placed))
    }

    #[test]
    fn placed_polyline_becomes_a_translated_nurbs() {
        use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples};

        let points = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 1.0, 0.0)]
            .try_into()
            .unwrap();
        let polyline =
            PolylineCurve::new(PolylineSamples::Unparameterized { points }, 0.0).unwrap();
        let transform = Transform::affine([
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, 7.0],
            [0.0, 0.0, 1.0, 9.0],
        ])
        .unwrap();
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve {
            native_ref: None,
            id: CurveId::mint("test:model:curve#placed-polyline").unwrap(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
                PlacedCurve::try_new(Box::new(SolvedCurveGeometry::Polyline(polyline)), transform)
                    .unwrap(),
            )),
            parameter_range: None,
            source_object: None,
        });
        let mut losses = Vec::new();
        convert_polylines(&mut ir, &mut losses);
        resolve_placements(&mut ir, &mut losses);
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = &ir.model.curves[0].geometry
        else {
            panic!("converted curve");
        };
        assert_eq!(curve.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            curve
                .control_points()
                .iter()
                .map(|point| point.get())
                .collect::<Vec<_>>(),
            [Point3::new(5.0, 7.0, 9.0), Point3::new(7.0, 8.0, 9.0)]
        );
        assert!(losses
            .iter()
            .any(|loss| loss.code.local_code() == "writer.polyline-converted"));
    }

    #[test]
    fn rigid_placement_preserves_nurbs_knots_and_weights() {
        use cadmpeg_ir::codec::{Codec, DecodeOptions};
        use std::io::Cursor;

        let source = crate::test_support::container::sldprt_with_body(
            &crate::test_support::parasolid::nurbs_triangle_body(),
        );
        let decoded = crate::SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let mut ir = decoded.ir().clone();
        let transform = Transform::affine([
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, 7.0],
            [0.0, 0.0, 1.0, 9.0],
        ])
        .unwrap();
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = &ir.model.curves[0].geometry
        else {
            panic!("fixture curve");
        };
        let original_curve = curve.clone();
        ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
            PlacedCurve::try_new(
                Box::new(SolvedCurveGeometry::Nurbs(curve.clone())),
                transform,
            )
            .unwrap(),
        ));
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) =
            &ir.model.surfaces[0].geometry
        else {
            panic!("fixture surface");
        };
        let original_surface = surface.clone();
        ir.model.surfaces[0].geometry =
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
                PlacedSurface::try_new(
                    Box::new(SolvedSurfaceGeometry::Nurbs(surface.clone())),
                    transform,
                )
                .unwrap(),
            ));
        let mut losses = Vec::new();
        resolve_placements(&mut ir, &mut losses);
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = &ir.model.curves[0].geometry
        else {
            panic!("resolved curve");
        };
        assert_eq!(curve.knots(), original_curve.knots());
        assert_eq!(curve.weights(), original_curve.weights());
        for (actual, original) in curve
            .control_points()
            .iter()
            .zip(original_curve.control_points())
        {
            let original = original.get();
            assert_eq!(
                actual.get(),
                Point3::new(original.x + 5.0, original.y + 7.0, original.z + 9.0)
            );
        }
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) =
            &ir.model.surfaces[0].geometry
        else {
            panic!("resolved surface");
        };
        assert_eq!(surface.u_knots(), original_surface.u_knots());
        assert_eq!(surface.v_knots(), original_surface.v_knots());
        assert_eq!(surface.weights(), original_surface.weights());
        for (actual, original) in surface.poles().iter().zip(original_surface.poles()) {
            let original = original.get();
            assert_eq!(
                actual.get(),
                Point3::new(original.x + 5.0, original.y + 7.0, original.z + 9.0)
            );
        }
    }

    #[test]
    fn placed_nurbs_carriers_survive_synthesis() {
        use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
        use cadmpeg_ir::codec::{Codec, DecodeOptions};
        use std::io::Cursor;

        let source = crate::test_support::container::sldprt_with_body(
            &crate::test_support::parasolid::nurbs_triangle_body(),
        );
        let decoded = crate::SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let mut ir = decoded.ir().clone();
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = &ir.model.curves[0].geometry
        else {
            panic!("fixture curve");
        };
        let curve = curve.clone();
        ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
            PlacedCurve::try_new(
                Box::new(SolvedCurveGeometry::Nurbs(curve.clone())),
                Transform::identity(),
            )
            .unwrap(),
        ));
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) =
            &ir.model.surfaces[0].geometry
        else {
            panic!("fixture surface");
        };
        let surface = surface.clone();
        ir.model.surfaces[0].geometry =
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
                PlacedSurface::try_new(
                    Box::new(SolvedSurfaceGeometry::Nurbs(surface.clone())),
                    Transform::identity(),
                )
                .unwrap(),
            ));
        let plan = crate::SldprtCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = crate::SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(output.ir().model.curves.iter().any(|value| value.geometry
            == CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve.clone()))));
        assert!(output.ir().model.surfaces.iter().any(|value| value.geometry
            == SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface.clone()))));
    }

    #[test]
    fn rigid_translation_resolves_into_the_carrier_and_names_the_record() {
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            native_ref: None,
            id: SurfaceId::mint("test:model:surface#1").expect("surface id"),
            geometry: translated_plane(),
            source_object: None,
        });
        let mut losses = Vec::new();
        resolve_placements(&mut ir, &mut losses);
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) =
            &ir.model.surfaces[0].geometry
        else {
            panic!("a rigid placement resolves into its plane");
        };
        assert_eq!(plane.origin().get(), Point3::new(12.0, 8.0, 10.0));
        assert_eq!(losses.len(), 1);
        let note = &losses[0];
        assert!(note.message.contains("test:model:surface#1"), "{note:?}");
        assert_eq!(note.code.local_code(), "writer.placement-resolved");
        assert_eq!(note.severity, Severity::Warning);
    }

    #[test]
    fn scaled_placement_stays_for_the_preflight() {
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve {
            native_ref: None,
            id: CurveId::mint("test:model:curve#1").expect("curve id"),
            geometry: placed_circle(
                Transform::affine([
                    [2.0, 0.0, 0.0, 0.0],
                    [0.0, 2.0, 0.0, 0.0],
                    [0.0, 0.0, 2.0, 0.0],
                ])
                .expect("finite scale"),
            ),
            parameter_range: None,
            source_object: None,
        });
        let mut losses = Vec::new();
        resolve_placements(&mut ir, &mut losses);
        assert!(
            matches!(
                ir.model.curves[0].geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Transformed(_))
            ),
            "a scaled placement is not a rigid carrier"
        );
        assert!(losses.is_empty());
    }

    #[test]
    fn mirrored_placement_stays_for_the_preflight() {
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve {
            native_ref: None,
            id: CurveId::mint("test:model:curve#2").expect("curve id"),
            geometry: placed_circle(
                Transform::affine([
                    [-1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ])
                .expect("finite mirror"),
            ),
            parameter_range: None,
            source_object: None,
        });
        let mut losses = Vec::new();
        resolve_placements(&mut ir, &mut losses);
        assert!(
            matches!(
                ir.model.curves[0].geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Transformed(_))
            ),
            "a mirror is not a proper rigid placement"
        );
        assert!(losses.is_empty());
    }

    #[test]
    fn nested_rigid_placements_resolve_inside_out() {
        let inner = PlacedSurface::try_new(
            Box::new(SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("admitted plane basis"),
            )),
            Transform::affine([
                [1.0, 0.0, 0.0, 5.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("finite translation"),
        )
        .expect("admitted placement");
        let outer = PlacedSurface::try_new(
            Box::new(SolvedSurfaceGeometry::Transformed(inner)),
            Transform::affine([
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 10.0],
            ])
            .expect("finite translation"),
        )
        .expect("admitted placement");
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            native_ref: None,
            id: SurfaceId::mint("test:model:surface#2").expect("surface id"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(outer)),
            source_object: None,
        });
        let mut losses = Vec::new();
        resolve_placements(&mut ir, &mut losses);
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) =
            &ir.model.surfaces[0].geometry
        else {
            panic!("nested rigid placements resolve into one plane");
        };
        assert_eq!(plane.origin().get(), Point3::new(5.0, 0.0, 10.0));
        assert_eq!(losses.len(), 1);
    }
}
