// SPDX-License-Identifier: Apache-2.0
//! Transfer source-owned boundary-plane charts.

use crate::container::ContainerScan;
use crate::decode::{native::annotate, source_carriers::SourceUnitCarriers};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    document::CadIr,
    geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry},
    math::{Point3, Vector3},
    AnnotationBuilder, Exactness, SourceObjectAssociation,
};

pub(in crate::decode) fn transfer_active_boundary_planes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<(), CodecError> {
    for plane in &scan.planes.datum_boundary_planes {
        let refusal = || {
            crate::loss::CreoLossCode::ActiveBoundaryPlaneUnresolved.note(format!(
            "ActDatums plane {} at byte {} requires one complete chart and a unique same-feature visible successor {}; its source remains native",
            plane.row.id, plane.row.offset, plane.row.next_surface,
        ))
        };
        if !plane.admitted(
            &scan.planes.datum_boundary_planes,
            &scan.surfaces.rows,
            &scan.surfaces.nonvisible_rows,
            &scan.planes.datum_cylinders,
        ) {
            losses.push(refusal());
            continue;
        }
        let Some(frame) = plane
            .local_system
            .as_ref()
            .map(crate::surface::PlaneLocalSystem::frame)
        else {
            losses.push(refusal());
            continue;
        };
        let (Some(origin), Some(normal), Some(reference)) =
            (frame.origin, frame.normal(), frame.u_axis())
        else {
            losses.push(refusal());
            continue;
        };
        let id =
            crate::identity::compose_checked(&crate::identity::ACTDATUM_SURFACE, plane.row.id)?;
        let geometry = cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::from(origin),
            Vector3::from(normal),
            Vector3::from(reference),
        )
        .map_err(CodecError::malformed)?;
        annotate(
            annotations,
            &id,
            "ActDatums",
            cadmpeg_core::decode::u64_from_index(plane.row.offset),
            "active_boundary_plane_chart",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(geometry)),
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(format_args!(
                        "ActDatums:{}",
                        plane.row.id
                    ))?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
    }
    Ok(())
}

pub(in crate::decode) fn retain_proved_boundaries(
    scan: &ContainerScan<'_>,
    eligible_faces: &mut std::collections::BTreeMap<u32, Vec<&crate::topology::Loop>>,
    refused_curves: &std::collections::BTreeSet<u32>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    for component in &scan.topology.face_components {
        let Some(curve) = component
            .curve_ids()
            .iter()
            .find(|curve| refused_curves.contains(curve))
        else {
            continue;
        };
        for face in component.face_ids() {
            if eligible_faces.remove(face).is_some() {
                losses.push(crate::loss::CreoLossCode::RevolutionBoundaryIntervalUnresolved.note(format!(
                    "Source face {face} remains native because its connected boundary curve {curve} has no proved revolution interval"
                )));
            }
        }
    }
}

const EPS_INTERVAL_ANGLE: f64 = 1.0e-10;
const EPS_INTERVAL_POSITION: f64 = 1.0e-9;

pub(in crate::decode) enum CircleInterval {
    Unrelated,
    Unresolved,
    Admitted(cadmpeg_ir::topology::IncreasingParameterInterval),
}

fn boundary_frame(
    scan: &ContainerScan<'_>,
    face: u32,
) -> Option<crate::surface::PositionalCylinderFrame> {
    if scan.surfaces.rows.iter().any(|row| row.id == face) {
        return crate::surface::unique_surface_parameter(&scan.surfaces.parameters, face)?
            .positional_cylinder_frame();
    }
    if scan
        .surfaces
        .nonvisible_rows
        .iter()
        .any(|row| row.id == face)
    {
        return None;
    }
    crate::decode::uniqueness::exactly_one(
        scan.planes
            .datum_cylinders
            .iter()
            .filter(|cylinder| cylinder.id == face),
    )
    .map(|cylinder| cylinder.frame)
}

pub(in crate::decode) fn circle_parameter_range(
    scan: &ContainerScan<'_>,
    geometry: &mut cadmpeg_ir::geometry::CurveGeometry,
    faces: [u32; 2],
    points: [[f64; 3]; 2],
    curve_id: u32,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> CircleInterval {
    let frames = faces
        .map(|face| boundary_frame(scan, face).filter(|frame| frame.parameter_bounds().is_some()));
    if !matches!(
        geometry.solved(),
        Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Circle(_))
    ) || frames.iter().all(Option::is_none)
    {
        return CircleInterval::Unrelated;
    }
    if frames.iter().flatten().all(|frame| {
        frame.parameter_bounds().is_some_and(|bounds| {
            bounds[0][1] - bounds[0][0] >= std::f64::consts::TAU - EPS_INTERVAL_ANGLE
        })
    }) {
        return CircleInterval::Unrelated;
    }
    let Some(range) = circle_interval(geometry, points, frames.into_iter().flatten())
        .and_then(cadmpeg_ir::topology::IncreasingParameterInterval::new)
    else {
        losses.push(crate::loss::CreoLossCode::RevolutionBoundaryIntervalUnresolved.note(format!("Curve {curve_id} on source faces {} and {} has no unique circle interval inside their stored revolution domains", faces[0],faces[1])));
        return CircleInterval::Unresolved;
    };
    CircleInterval::Admitted(range)
}

fn lifted_angle(theta: f64, bounds: [f64; 2]) -> Option<f64> {
    let base = bounds[0] + (theta - bounds[0]).rem_euclid(std::f64::consts::TAU);
    let mut selected: Option<f64> = None;
    for angle in [
        base - std::f64::consts::TAU,
        base,
        base + std::f64::consts::TAU,
    ] {
        if angle < bounds[0] - EPS_INTERVAL_ANGLE || angle > bounds[1] + EPS_INTERVAL_ANGLE {
            continue;
        }
        let angle = angle.clamp(bounds[0], bounds[1]);
        if selected.is_some_and(|previous| (previous - angle).abs() > EPS_INTERVAL_ANGLE) {
            return None;
        }
        selected = Some(angle);
    }
    selected
}

fn circle_interval(
    geometry: &mut cadmpeg_ir::geometry::CurveGeometry,
    points: [[f64; 3]; 2],
    frames: impl Iterator<Item = crate::surface::PositionalCylinderFrame>,
) -> Option<[f64; 2]> {
    use crate::decode::surfaces::intersection_resolve::curve_contains_points;
    use crate::vecmath::{cross, dot, normalize};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let SolvedCurveGeometry::Circle(circle) = geometry.solved()? else {
        return None;
    };
    curve_contains_points(geometry, points).then_some(())?;
    let center = <[f64; 3]>::from(circle.center().get());
    let axis = <[f64; 3]>::from(*circle.frame().axis().as_raw());
    let reference = <[f64; 3]>::from(*circle.frame().reference().as_raw());
    let second = cross(axis, reference);
    let parameter = |point: [f64; 3]| {
        let relative = std::array::from_fn(|i| point[i] - center[i]);
        dot(relative, second).atan2(dot(relative, reference))
    };
    let first_angle = parameter(points[0]);
    let positive_span = (parameter(points[1]) - first_angle).rem_euclid(std::f64::consts::TAU);
    (positive_span > EPS_INTERVAL_ANGLE
        && positive_span < std::f64::consts::TAU - EPS_INTERVAL_ANGLE)
        .then_some(())?;
    let mut selected: Option<f64> = None;
    for frame in frames {
        let [angular, axial] = frame.parameter_bounds()?;
        let origin = frame.frame().origin();
        let n = frame.frame().axis();
        let u = frame.frame().ref_direction();
        let v = cross(n, u);
        ((dot(n, axis).abs() - 1.0).abs() <= EPS_INTERVAL_ANGLE).then_some(())?;
        let radius = frame.radius().get();
        let scale = points
            .into_iter()
            .flatten()
            .chain(origin)
            .map(f64::abs)
            .fold(radius.max(1.0), f64::max);
        let tolerance = EPS_INTERVAL_POSITION * scale;
        ((radius - circle.radius().get()).abs() <= tolerance).then_some(())?;
        let center_delta = std::array::from_fn(|i| center[i] - origin[i]);
        let center_height = dot(center_delta, n);
        (0..3)
            .all(|i| (center_delta[i] - n[i] * center_height).abs() <= tolerance)
            .then_some(())?;
        let full_turn = angular[1] - angular[0] >= std::f64::consts::TAU - EPS_INTERVAL_ANGLE;
        let coordinate = |point: [f64; 3]| -> Option<[f64; 2]> {
            let relative = std::array::from_fn(|i| point[i] - origin[i]);
            let theta = dot(relative, v).atan2(dot(relative, u));
            let theta = if full_turn {
                theta
            } else {
                lifted_angle(theta, angular)?
            };
            let height = dot(relative, n);
            (height >= axial[0].min(axial[1]) - tolerance
                && height <= axial[0].max(axial[1]) + tolerance)
                .then_some([theta, height])
        };
        let [Some(first), Some(last)] = points.map(coordinate) else {
            return None;
        };
        ((first[1] - last[1]).abs() <= tolerance).then_some(())?;
        if full_turn {
            continue;
        }
        let theta = first[0].midpoint(last[0]);
        let height = first[1].midpoint(last[1]);
        let interior = std::array::from_fn(|i| {
            origin[i] + radius * (u[i] * theta.cos() + v[i] * theta.sin()) + n[i] * height
        });
        curve_contains_points(geometry, [interior; 2]).then_some(())?;
        let interior_angle = (parameter(interior) - first_angle).rem_euclid(std::f64::consts::TAU);
        let span = if interior_angle > EPS_INTERVAL_ANGLE
            && interior_angle < positive_span - EPS_INTERVAL_ANGLE
        {
            positive_span
        } else if interior_angle > positive_span + EPS_INTERVAL_ANGLE
            && interior_angle < std::f64::consts::TAU - EPS_INTERVAL_ANGLE
        {
            positive_span - std::f64::consts::TAU
        } else {
            return None;
        };
        if selected.is_some_and(|previous| (previous - span).abs() > EPS_INTERVAL_ANGLE) {
            return None;
        }
        selected = Some(span);
    }
    let span = selected?;
    let reference = normalize(std::array::from_fn(|i| points[0][i] - center[i]))?;
    let oriented = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
        circle.center().get(),
        Vector3::from(axis.map(|component| component * span.signum())),
        Vector3::from(reference),
        circle.radius().get(),
    )
    .ok()?;
    *geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(oriented));
    Some([0.0, span.abs()])
}

pub(in crate::decode) fn generator_carrier(
    scan: &ContainerScan<'_>,
    row: &crate::curve::CurveTopologyRow,
    points: [[f64; 3]; 2],
) -> Option<Result<cadmpeg_ir::geometry::CurveGeometry, &'static str>> {
    let record = crate::decode::uniqueness::exactly_one(
        scan.curves
            .parameters
            .iter()
            .filter(|record| record.curve_id == row.id),
    )?;
    matches!(record.type_byte, 0x00 | 0x01 | 0x06 | 0x08).then_some(())?;
    (record.type_byte == row.type_byte).then_some(())?;
    let [Some(first_face), Some(second_face)] = row.faces else {
        return None;
    };
    let faces = [first_face.get(), second_face.get()];
    let [Some(first), Some(second)] = faces.map(|face| boundary_frame(scan, face)) else {
        return None;
    };
    let frames = [first, second];
    frames
        .iter()
        .all(|frame| {
            frame.parameter_bounds().is_some_and(|bounds| {
                let span = bounds[0][1] - bounds[0][0];
                span > EPS_INTERVAL_ANGLE && span < std::f64::consts::TAU - EPS_INTERVAL_ANGLE
            })
        })
        .then_some(())?;
    let mut body = record.body.as_slice();
    if body.first() == Some(&0xfc) {
        let (count, start) = crate::psb::compact_int(body, 1);
        (count == 2 && start > 1).then_some(())?;
        body = body.get(start..)?;
    }
    let values =
        crate::scalar::decode_pcurve_scalar_frame(body, &crate::scalar::ScalarCache::default())?
            .values;
    let paths = [
        [[values[0], values[1]], [values[4], values[5]]],
        [[values[2], values[3]], [values[6], values[7]]],
    ];
    paths
        .iter()
        .any(|path| path[0][0] == path[1][0] && path[0][1] != path[1][1])
        .then_some(())?;
    Some(bounded_generator(&frames, points, paths).ok_or(
        "the complete endpoint charts require one shared stored cylinder generator and agreeing full paths",
    ))
}

pub(in crate::decode) fn bounded_generator(
    frames: &[crate::surface::PositionalCylinderFrame; 2],
    points: [[f64; 3]; 2],
    paths: [[[f64; 2]; 2]; 2],
) -> Option<cadmpeg_ir::geometry::CurveGeometry> {
    use crate::decode::surfaces::intersection_resolve::curve_contains_points;
    use crate::vecmath::{cross, dot};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let witness = paths
        .iter()
        .position(|path| path[0][0] == path[1][0] && path[0][1] != path[1][1])?;
    let radii = frames.each_ref().map(|frame| frame.radius().get());
    let bounds = [frames[0].parameter_bounds()?, frames[1].parameter_bounds()?];
    let scale = radii
        .into_iter()
        .chain(bounds.map(|bound| (bound[1][1] - bound[1][0]).abs()))
        .fold(1.0, f64::max);
    let tolerance = EPS_INTERVAL_POSITION * scale;
    ((radii[0] - radii[1]).abs() <= tolerance).then_some(())?;
    let axes = frames.each_ref().map(|frame| frame.frame().axis());
    ((dot(axes[0], axes[1]).abs() - 1.0).abs() <= EPS_INTERVAL_ANGLE).then_some(())?;
    let origins = frames.each_ref().map(|frame| frame.frame().origin());
    let displacement = std::array::from_fn(|i| origins[1][i] - origins[0][i]);
    let along = dot(displacement, axes[0]);
    (0..3)
        .all(|i| (displacement[i] - axes[0][i] * along).abs() <= tolerance)
        .then_some(())?;
    let radial = |index: usize, angle: f64| {
        let u = frames[index].frame().ref_direction();
        let v = cross(axes[index], u);
        std::array::from_fn::<_, 3, _>(|i| radii[index] * (u[i] * angle.cos() + v[i] * angle.sin()))
    };
    let at = |index: usize, uv: [f64; 2]| {
        let radial = radial(index, uv[0]);
        std::array::from_fn::<_, 3, _>(|i| origins[index][i] + radial[i] + axes[index][i] * uv[1])
    };
    let agrees = |first: [f64; 3], second: [f64; 3]| {
        first
            .into_iter()
            .zip(second)
            .all(|(first, second)| (first - second).abs() <= tolerance)
    };
    let mut alignments = [true; 2];
    for index in 0..2 {
        let [angular, axial] = bounds[index];
        let delta = (paths[index][1][0] - paths[index][0][0]).abs();
        (delta <= EPS_INTERVAL_ANGLE && radii[index] * delta <= tolerance).then_some(())?;
        for uv in paths[index] {
            lifted_angle(uv[0], angular)?;
            (uv[1] >= axial[0].min(axial[1]) - tolerance
                && uv[1] <= axial[0].max(axial[1]) + tolerance)
                .then_some(())?;
        }
        let mapped = paths[index].map(|uv| at(index, uv));
        alignments[0] &= agrees(mapped[0], points[0]) && agrees(mapped[1], points[1]);
        alignments[1] &= agrees(mapped[0], points[1]) && agrees(mapped[1], points[0]);
        let matching = angular.into_iter().filter(|angle| {
            let radial = radial(index, *angle);
            points.iter().all(|point| {
                let relative = std::array::from_fn(|i| point[i] - origins[index][i]);
                let height = dot(relative, axes[index]);
                (0..3)
                    .all(|i| (relative[i] - height * axes[index][i] - radial[i]).abs() <= tolerance)
            })
        });
        crate::decode::uniqueness::exactly_one(matching)?;
    }
    matches!(alignments, [true, false] | [false, true]).then_some(())?;
    let line = cadmpeg_ir::geometry::analytic::LineCurve::try_new(
        Point3::from(at(witness, paths[witness][0])),
        Vector3::from(axes[witness]),
    )
    .ok()?;
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(line));
    curve_contains_points(&geometry, points).then_some(geometry)
}

pub(in crate::decode) fn native_solid_contains_feature(
    scan: &ContainerScan<'_>,
    ir: &CadIr,
    feature_id: u32,
) -> bool {
    if feature_id == 0 {
        return false;
    }
    crate::identity::uniquely_identified_rows(&scan.surfaces.rows, |row| row.id)
        .into_iter()
        .filter(|row| row.feature_id == feature_id)
        .any(|row| {
            let Some(face) =
                crate::decode::uniqueness::exactly_one(ir.model.faces.iter().filter(|face| {
                    crate::identity::matches_numbered_identity(
                        face.id.as_str(),
                        "creo:visibgeom:face#",
                        row.id,
                    )
                }))
            else {
                return false;
            };
            let Some(shell) = crate::decode::uniqueness::exactly_one(
                ir.model
                    .shells
                    .iter()
                    .filter(|shell| shell.id == face.shell),
            ) else {
                return false;
            };
            let Some(region) = crate::decode::uniqueness::exactly_one(
                ir.model
                    .regions
                    .iter()
                    .filter(|region| region.id == shell.region),
            ) else {
                return false;
            };
            let Some(body) = crate::decode::uniqueness::exactly_one(
                ir.model.bodies.iter().filter(|body| body.id == region.body),
            ) else {
                return false;
            };
            body.kind == cadmpeg_ir::topology::BodyKind::Solid
                && body.id.as_str().starts_with("creo:visibgeom:body#")
                && body.regions.contains(&region.id)
                && region.shells.contains(&shell.id)
                && shell.faces().contains(&face.id)
        })
}
