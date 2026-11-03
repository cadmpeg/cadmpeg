// SPDX-License-Identifier: Apache-2.0
//! Transfer of carrier intersection curves and NURBS boundary curves.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    nurbs::NurbsSurface, Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::native::annotate;
use super::super::uniqueness::exactly_one;
use crate::decode::analytic::carriers::placed_carriers;
use crate::decode::analytic::equations::{CarrierEquation, CylinderEquation, PlaneEquation};
use crate::decode::analytic::pcurves::{
    canonicalized_pcurve_endpoints, pcurve_edge_endpoint_evidence, ModelSurfaceIndex,
};
use crate::decode::analytic::vertices::{
    finite_model_point, model_points_agree, solved_topological_vertices,
};
use crate::decode::source_carriers::SourceUnitCarriers;

use super::intersection_resolve::curve_contains_points;
use super::intersection_resolve::{
    fc14_held_coordinate, multi_component_intersection_candidates, resolve_curve_candidates,
    select_fc14_axis_coordinate_candidate,
};
use super::intersections::carrier_intersection_curve;
use super::nurbs_boundaries::{
    cubic_extrusion_plane_generator_curve, nurbs_plane_boundary_curve,
    shared_extrusion_generator_curve,
};

pub(in super::super) fn analytic_curve_branches(
    geometry: &CurveGeometry,
    tag: &'static str,
) -> super::intersection_candidates::FixedCandidates<(CurveGeometry, &'static str)> {
    let opposite =
        if let CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(hyperbola_curve)) = geometry {
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
                    hyperbola_curve.opposite_branch(),
                )),
                tag,
            ))
        } else {
            None
        };
    std::iter::once((geometry.clone(), tag))
        .chain(opposite)
        .collect()
}

fn resolve_carrier_intersection_curve(
    first: CarrierEquation,
    second: CarrierEquation,
    points: Option<[[f64; 3]; 2]>,
    allow_unresolved_endpoint_witness: bool,
) -> std::option::Option<(cadmpeg_ir::geometry::CurveGeometry, &'static str)> {
    let (geometry, tag) = carrier_intersection_curve(first, second)?;
    let candidates = analytic_curve_branches(&geometry, tag);
    let selected = match points {
        Some(points) => exactly_one(
            candidates
                .iter()
                .enumerate()
                .filter(|(_, (geometry, _))| curve_contains_points(geometry, points))
                .map(|(index, _)| index),
        ),
        None => (candidates.len() == 1).then_some(0),
    };
    let selected = selected.or_else(|| {
        // A one-sided pcurve on an unresolved adjacent face supplies only a
        // finite-edge witness. It does not veto the exact infinite plane line.
        (tag == "plane_intersection_line"
            && (points.is_none() || allow_unresolved_endpoint_witness))
            .then_some(())
            .filter(|()| candidates.len() == 1)
            .map(|()| 0)
    });
    selected.and_then(|index| candidates.into_iter().nth(index))
}

/// Squared length of the normal cross product below which two plane normals
/// are parallel.
const EPS_COPLANAR_NORMAL_CROSS_SQUARED: f64 = 1.0e-18;
/// Plane offset, relative to the larger model coordinate, below which two
/// parallel planes are one plane.
const EPS_COPLANAR_OFFSET: f64 = 1.0e-9;

/// The line through an edge's two solved endpoints when both incident faces
/// lie on one plane and a face path maps onto the endpoints in exactly one
/// order.
fn coplanar_face_edge_line(
    faces: [PlaneEquation; 2],
    points: [[f64; 3]; 2],
    mapped_paths: &[[[f64; 3]; 2]],
) -> Option<CurveGeometry> {
    one_plane(faces).then_some(())?;
    let [Some(start), Some(end)] = points.map(finite_model_point) else {
        return None;
    };
    (!model_points_agree(start, end)).then_some(())?;
    mapped_paths
        .iter()
        .any(|path| path_maps_onto_once(*path, start, end))
        .then_some(())?;
    let direction = crate::vecmath::normalize(std::array::from_fn(|axis| {
        points[1][axis] - points[0][axis]
    }))?;
    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
        cadmpeg_ir::math::Point3::from(points[0]),
        cadmpeg_ir::math::Vector3::from(direction),
    )
    .ok()
    .map(|line| CurveGeometry::Solved(SolvedCurveGeometry::Line(line)))
}

/// Squared length of the axis cross product below which two cylinder axes
/// are parallel.
const EPS_SHARED_CYLINDER_AXIS_CROSS_SQUARED: f64 = 1.0e-18;
/// Radius, axis-line, and generator deviation relative to the model scale
/// below which two cylinder carriers and an edge agree.
const EPS_SHARED_CYLINDER_AGREEMENT: f64 = 1.0e-9;
/// Chart-parameter difference, relative to the larger parameter, below which a
/// cylinder chart path has one `u` value.
const EPS_GENERATOR_PATH_PARAMETER: f64 = 1.0e-9;

/// Whether two placed cylinder equations describe one cylinder.
fn one_cylinder([first, second]: [CylinderEquation; 2]) -> bool {
    let (Some(first_axis), Some(second_axis)) = (
        crate::vecmath::normalize(first.axis),
        crate::vecmath::normalize(second.axis),
    ) else {
        return false;
    };
    let axis_cross = crate::vecmath::cross(first_axis, second_axis);
    if crate::vecmath::dot(axis_cross, axis_cross) > EPS_SHARED_CYLINDER_AXIS_CROSS_SQUARED {
        return false;
    }
    let scale = first
        .origin
        .into_iter()
        .chain(second.origin)
        .chain([first.radius, second.radius])
        .map(f64::abs)
        .fold(1.0, f64::max);
    (first.radius - second.radius).abs() <= EPS_SHARED_CYLINDER_AGREEMENT * scale
        && axis_line_distance(first.origin, first_axis, second.origin)
            <= EPS_SHARED_CYLINDER_AGREEMENT * scale
}

/// Distance from `point` to the line through `origin` along unit `axis`.
fn axis_line_distance(origin: [f64; 3], axis: [f64; 3], point: [f64; 3]) -> f64 {
    let offset: [f64; 3] = std::array::from_fn(|index| point[index] - origin[index]);
    let along = crate::vecmath::dot(offset, axis);
    let radial: [f64; 3] = std::array::from_fn(|index| offset[index] - along * axis[index]);
    crate::vecmath::dot(radial, radial).sqrt()
}

/// The generator line through an edge's two solved endpoints when both
/// incident faces lie on one cylinder, the endpoints lie on one generator, and
/// a constant-`u` face path maps onto the endpoints in exactly one order.
fn shared_cylinder_generator_line(
    faces: [CylinderEquation; 2],
    points: [[f64; 3]; 2],
    generator_paths: &[[[f64; 3]; 2]],
) -> Option<CurveGeometry> {
    one_cylinder(faces).then_some(())?;
    let [cylinder, _] = faces;
    let axis = crate::vecmath::normalize(cylinder.axis)?;
    let [Some(start), Some(end)] = points.map(finite_model_point) else {
        return None;
    };
    (!model_points_agree(start, end)).then_some(())?;
    let scale = points
        .into_iter()
        .flatten()
        .chain(cylinder.origin)
        .chain([cylinder.radius])
        .map(f64::abs)
        .fold(1.0, f64::max);
    points
        .iter()
        .all(|point| {
            (axis_line_distance(cylinder.origin, axis, *point) - cylinder.radius).abs()
                <= EPS_SHARED_CYLINDER_AGREEMENT * scale
        })
        .then_some(())?;
    let displacement: [f64; 3] = std::array::from_fn(|index| points[1][index] - points[0][index]);
    let off_axis = crate::vecmath::cross(displacement, axis);
    (crate::vecmath::dot(off_axis, off_axis).sqrt() <= EPS_SHARED_CYLINDER_AGREEMENT * scale)
        .then_some(())?;
    generator_paths
        .iter()
        .any(|path| path_maps_onto_once(*path, start, end))
        .then_some(())?;
    let direction = crate::vecmath::normalize(displacement)?;
    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
        cadmpeg_ir::math::Point3::from(points[0]),
        cadmpeg_ir::math::Vector3::from(direction),
    )
    .ok()
    .map(|line| CurveGeometry::Solved(SolvedCurveGeometry::Line(line)))
}

/// Whether a mapped face path yields `start` and `end` in exactly one order.
fn path_maps_onto_once(
    path: [[f64; 3]; 2],
    start: cadmpeg_ir::features::FinitePoint3,
    end: cadmpeg_ir::features::FinitePoint3,
) -> bool {
    let [Some(first_point), Some(second_point)] = path.map(finite_model_point) else {
        return false;
    };
    let forward = model_points_agree(first_point, start) && model_points_agree(second_point, end);
    let reverse = model_points_agree(first_point, end) && model_points_agree(second_point, start);
    forward != reverse
}

/// The first and last chart point of each constant-`u` path of `row` in a
/// placed cylindrical face chart, mapped to model space.
fn mapped_cylinder_generator_paths(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    model_surfaces: &ModelSurfaceIndex<'_>,
    source_carriers: &SourceUnitCarriers,
    row: &crate::curve::CurveTopologyRow,
) -> Result<Vec<[[f64; 3]; 2]>, CodecError> {
    let mut chart_paths = Vec::<(u32, Vec<[f64; 2]>)>::new();
    for (curve_id, faces, face_0_endpoints, face_1_endpoints) in scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            )
        })
        .chain(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            )
        }))
    {
        if curve_id != row.id {
            continue;
        }
        let endpoint_sets =
            canonicalized_pcurve_endpoints(scan, faces, face_0_endpoints, face_1_endpoints);
        for (face, endpoints) in faces.into_iter().zip(endpoint_sets) {
            if let Some(face) = face {
                chart_paths.push((face.get(), endpoints.to_vec()));
            }
        }
    }
    for pcurve in scan
        .curves
        .two_chart_pcurves
        .iter()
        .filter(|pcurve| pcurve.curve_id == row.id)
    {
        for (side, face) in pcurve.faces.into_iter().enumerate() {
            chart_paths.push((
                face,
                pcurve.samples.iter().map(|sample| sample[side]).collect(),
            ));
        }
    }
    let mut paths = Vec::new();
    for (face, samples) in chart_paths {
        if !row.faces.contains(&std::num::NonZeroU32::new(face)) {
            continue;
        }
        let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
            continue;
        };
        if samples.len() < 2
            || !samples.iter().all(|sample| {
                (sample[0] - first[0]).abs()
                    <= EPS_GENERATOR_PATH_PARAMETER * sample[0].abs().max(first[0].abs()).max(1.0)
            })
        {
            continue;
        }
        let Some(surface) = model_surfaces.unique(face) else {
            continue;
        };
        let geometry = source_carriers.surface_geometry(surface);
        if !matches!(
            geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_))
        ) {
            continue;
        }
        let mut mapped = [[0.0; 3]; 2];
        let mut complete = true;
        for (slot, uv) in mapped.iter_mut().zip([*first, *last]) {
            match cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::surface_point(ctx, geometry, uv[0], uv[1]),
            )?)? {
                Some(point) => *slot = [point.x, point.y, point.z],
                None => complete = false,
            }
        }
        if complete {
            paths.push(mapped);
        }
    }
    Ok(paths)
}

/// The single carrier that both faces of an edge lie on.
enum SharedCarrier {
    Plane([PlaneEquation; 2]),
    Cylinder([CylinderEquation; 2]),
}

/// Whether two placed plane equations describe one plane.
fn one_plane([first, second]: [PlaneEquation; 2]) -> bool {
    let normal_cross = crate::vecmath::cross(first.normal, second.normal);
    if crate::vecmath::dot(normal_cross, normal_cross) > EPS_COPLANAR_NORMAL_CROSS_SQUARED {
        return false;
    }
    let Some(normal) = crate::vecmath::normalize(first.normal) else {
        return false;
    };
    let offset = crate::vecmath::dot(
        normal,
        std::array::from_fn(|axis| second.origin[axis] - first.origin[axis]),
    );
    let scale = first
        .origin
        .into_iter()
        .chain(second.origin)
        .map(f64::abs)
        .fold(1.0, f64::max);
    offset.abs() <= EPS_COPLANAR_OFFSET * scale
}

/// Each native pcurve endpoint pair of `row` mapped through its placed planar
/// face chart.
fn mapped_planar_face_paths(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    model_surfaces: &ModelSurfaceIndex<'_>,
    source_carriers: &SourceUnitCarriers,
    row: &crate::curve::CurveTopologyRow,
) -> Result<Vec<[[f64; 3]; 2]>, CodecError> {
    let records = scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            )
        })
        .chain(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            )
        }));
    let mut paths = Vec::new();
    for (curve_id, faces, face_0_endpoints, face_1_endpoints) in records {
        if curve_id != row.id {
            continue;
        }
        let endpoint_sets =
            canonicalized_pcurve_endpoints(scan, faces, face_0_endpoints, face_1_endpoints);
        for (face, endpoints) in faces.into_iter().zip(endpoint_sets) {
            let Some(face) = face.filter(|face| row.faces.contains(&Some(*face))) else {
                continue;
            };
            let Some(surface) = model_surfaces.unique(face.get()) else {
                continue;
            };
            let geometry = source_carriers.surface_geometry(surface);
            if !matches!(
                geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
            ) {
                continue;
            }
            let mut mapped = [[0.0; 3]; 2];
            let mut complete = true;
            for (slot, uv) in mapped.iter_mut().zip(endpoints) {
                match cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::surface_point(ctx, geometry, uv[0], uv[1]),
                )?)? {
                    Some(point) => *slot = [point.x, point.y, point.z],
                    None => complete = false,
                }
            }
            if complete {
                paths.push(mapped);
            }
        }
    }
    Ok(paths)
}

pub(in super::super) fn transfer_carrier_intersection_curves(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<BTreeSet<CurveId>, cadmpeg_core::CodecError> {
    let mut transferred = BTreeSet::new();
    let carriers = placed_carriers(ctx, scan, ir, source_carriers)?;
    let solved_vertices = solved_topological_vertices(
        ctx,
        scan,
        ir,
        &carriers,
        nurbs_endpoint_witnesses,
        source_carriers,
    )?;
    let endpoint_evidence = pcurve_edge_endpoint_evidence(ctx, scan, ir, source_carriers)?;
    let edge_vertices =
        crate::topology::edge_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    let mut shared_carrier_lines = std::collections::BTreeMap::new();
    {
        let model_surfaces = ModelSurfaceIndex::new(&ir.model.surfaces);
        for row in
            crate::identity::uniquely_identified_rows(&scan.curves.topology_rows, |row| row.id)
        {
            let [Some(first_face), Some(second_face)] = row.faces else {
                continue;
            };
            let pair = (
                carriers.get(&first_face.get()).copied(),
                carriers.get(&second_face.get()).copied(),
            );
            let shared = match pair {
                (Some(CarrierEquation::Plane(first)), Some(CarrierEquation::Plane(second)))
                    if one_plane([first, second]) =>
                {
                    SharedCarrier::Plane([first, second])
                }
                (
                    Some(CarrierEquation::Cylinder(first)),
                    Some(CarrierEquation::Cylinder(second)),
                ) if one_cylinder([first, second]) => SharedCarrier::Cylinder([first, second]),
                _ => continue,
            };
            let Some(points) = edge_vertices.get(&row.id).and_then(|vertices| {
                Some([
                    *solved_vertices.get(&vertices[0].get())?,
                    *solved_vertices.get(&vertices[1].get())?,
                ])
            }) else {
                continue;
            };
            let line = match shared {
                SharedCarrier::Plane(faces) => {
                    let paths =
                        mapped_planar_face_paths(ctx, scan, &model_surfaces, source_carriers, row)?;
                    coplanar_face_edge_line(faces, points, &paths)
                        .map(|line| (line, "coplanar_face_pcurve_line"))
                }
                SharedCarrier::Cylinder(faces) => {
                    let paths = mapped_cylinder_generator_paths(
                        ctx,
                        scan,
                        &model_surfaces,
                        source_carriers,
                        row,
                    )?;
                    shared_cylinder_generator_line(faces, points, &paths)
                        .map(|line| (line, "shared_cylinder_generator_line"))
                }
            };
            if let Some(line) = line {
                shared_carrier_lines.insert(row.id, line);
            }
        }
    }
    for row in crate::identity::uniquely_identified_rows(&scan.curves.topology_rows, |row| row.id) {
        let [Some(first_face), Some(second_face)] = row.faces else {
            continue;
        };
        let (Some(first), Some(second)) = (
            carriers.get(&first_face.get()).copied(),
            carriers.get(&second_face.get()).copied(),
        ) else {
            continue;
        };
        let points = (|| {
            let vertices = edge_vertices.get(&row.id)?;
            let points = [
                *solved_vertices.get(&vertices[0].get())?,
                *solved_vertices.get(&vertices[1].get())?,
            ];
            Some(points)
        })();
        let curve_id =
            crate::identity::compose_checked::<CurveId>(&crate::identity::VISIBGEOM_CURVE, row.id)?;
        let allow_unresolved_endpoint_witness = endpoint_evidence
            .get(&row.id)
            .is_some_and(|evidence| !evidence.complete)
            && !nurbs_endpoint_witnesses.contains(&curve_id);
        let resolved = resolve_carrier_intersection_curve(
            first,
            second,
            points,
            allow_unresolved_endpoint_witness,
        )
        .or_else(|| {
            let candidates = multi_component_intersection_candidates(first, second);
            if points.is_some() {
                resolve_curve_candidates(candidates, points)
            } else {
                let held = fc14_held_coordinate(&scan.curves.fc_coordinates, row.id)?;
                select_fc14_axis_coordinate_candidate(candidates, held)
            }
        })
        .or_else(|| shared_carrier_lines.remove(&row.id));
        let Some((geometry, tag)) = resolved else {
            continue;
        };
        let id = curve_id;
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(row.offset),
            tag,
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model curves")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                parameter_range: None,
                id: crate::identity::copy_checked_id(id.as_str())?,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::new(format!(
                        "VisibGeom:{}",
                        row.id
                    ))
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        transferred.insert(id);
    }
    Ok(transferred)
}

pub(in super::super) struct TransferredNurbsBoundaryCurves {
    pub(in super::super) ids: BTreeSet<CurveId>,
    pub(in super::super) endpoint_witnesses: BTreeSet<CurveId>,
    pub(in super::super) extrusion_plane_count: usize,
    pub(in super::super) extrusion_plane_section_generator_count: usize,
    pub(in super::super) shared_extrusion_generator_count: usize,
}

#[derive(Clone, Copy)]
enum NurbsBoundaryKind {
    ExtrusionPlane,
    ExtrusionPlaneSectionGenerator,
    SharedExtrusionGenerator,
}

/// Boundary curve of an extrusion surface against a plane, with the
/// section-generator fallback.
///
/// The sink is drained before the fallback runs, so the fallback's own
/// refusals never mix with the records of the first attempt. A refused lane
/// that the fallback covers costs the model nothing and states no loss. A
/// refused lane that leaves the curve-topology row without a carrier is a loss
/// note naming the row and the surface that stated the lane.
fn extrusion_plane_boundary_curve(
    nurbs: &NurbsSurface,
    surface_id: u32,
    curve_row_id: u32,
    plane: PlaneEquation,
    refusal: &mut crate::lane_refusal::LaneRefusals,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<(CurveGeometry, NurbsBoundaryKind)> {
    if let Some(geometry) = nurbs_plane_boundary_curve(nurbs, surface_id, plane, refusal) {
        return Some((geometry, NurbsBoundaryKind::ExtrusionPlane));
    }
    let refused = refusal.take_records();
    let mut fallback_losses = Vec::new();
    let fallback =
        cubic_extrusion_plane_generator_curve(nurbs, surface_id, plane, &mut fallback_losses)
            .map(|geometry| (geometry, NurbsBoundaryKind::ExtrusionPlaneSectionGenerator));
    if fallback.is_none() {
        losses.extend(fallback_losses);
        note_boundary_lane_records(curve_row_id, &refused, losses);
    }
    fallback
}

/// Drain every refused boundary lane into one loss note per record.
fn note_refused_boundary_lanes(
    curve_row_id: u32,
    refusal: &mut crate::lane_refusal::LaneRefusals,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let records = refusal.take_records();
    note_boundary_lane_records(curve_row_id, &records, losses);
}

/// One loss note per refused boundary-lane record, each naming the row.
fn note_boundary_lane_records(
    curve_row_id: u32,
    records: &[String],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    for record in records {
        let message = format!(
            "VisibGeom curve-topology row {curve_row_id} states no NURBS boundary carrier: \
         {record}"
        );
        losses.push(crate::loss::CreoLossCode::NurbsBoundaryCarrierUnresolved.note(message));
    }
}

pub(in super::super) fn transfer_nurbs_boundary_curves(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<TransferredNurbsBoundaryCurves, CodecError> {
    let mut result = TransferredNurbsBoundaryCurves {
        ids: BTreeSet::new(),
        endpoint_witnesses: BTreeSet::new(),
        extrusion_plane_count: 0,
        extrusion_plane_section_generator_count: 0,
        shared_extrusion_generator_count: 0,
    };
    for row in crate::identity::uniquely_identified_rows(&scan.curves.topology_rows, |row| row.id) {
        let [Some(first_face), Some(second_face)] = row.faces else {
            continue;
        };
        let Some(first) = crate::surface::unique_surface_row(&scan.surfaces.rows, first_face.get())
        else {
            continue;
        };
        let Some(second) =
            crate::surface::unique_surface_row(&scan.surfaces.rows, second_face.get())
        else {
            continue;
        };
        let geometry = |surface_id| {
            exactly_one(ir.model.surfaces.iter().filter(|surface| {
                crate::identity::matches_numbered_identity(
                    surface.id.as_str(),
                    "creo:visibgeom:surface#",
                    surface_id,
                )
            }))
            .map(|surface| source_carriers.surface_geometry(surface))
        };
        let Some(first_geometry) = geometry(first.id) else {
            continue;
        };
        let Some(second_geometry) = geometry(second.id) else {
            continue;
        };
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let refusal = &mut refusal;
        let resolved = match (first.kind, second.kind, first_geometry, second_geometry) {
            (
                crate::surface::SurfaceKind::Extrusion(_),
                crate::surface::SurfaceKind::Plane,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
            ) => {
                let origin = plane_surface.origin().get();
                let normal = plane_surface.frame().axis().as_raw();
                let plane = PlaneEquation {
                    origin: [origin.x, origin.y, origin.z],
                    normal: [normal.x, normal.y, normal.z],
                };
                extrusion_plane_boundary_curve(nurbs, first.id, row.id, plane, refusal, losses)
            }
            (
                crate::surface::SurfaceKind::Plane,
                crate::surface::SurfaceKind::Extrusion(_),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface_2)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
            ) => {
                let origin = plane_surface_2.origin();
                let normal = plane_surface_2.frame().axis().as_raw();
                let plane = PlaneEquation {
                    origin: [origin.x, origin.y, origin.z],
                    normal: [normal.x, normal.y, normal.z],
                };
                extrusion_plane_boundary_curve(nurbs, second.id, row.id, plane, refusal, losses)
            }
            (
                crate::surface::SurfaceKind::Extrusion(_),
                crate::surface::SurfaceKind::Extrusion(_),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(first_nurbs)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(second_nurbs)),
            ) => shared_extrusion_generator_curve(
                first_nurbs,
                first.id,
                second_nurbs,
                second.id,
                refusal,
            )
            .map(|geometry| (geometry, NurbsBoundaryKind::SharedExtrusionGenerator)),
            _ => None,
        };
        let Some((geometry, kind)) = resolved else {
            note_refused_boundary_lanes(row.id, refusal, losses);
            continue;
        };
        let id =
            crate::identity::compose_checked::<CurveId>(&crate::identity::VISIBGEOM_CURVE, row.id)?;
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(row.offset),
            match kind {
                NurbsBoundaryKind::ExtrusionPlane => "extrusion_plane_nurbs_boundary",
                NurbsBoundaryKind::ExtrusionPlaneSectionGenerator => {
                    "extrusion_plane_nurbs_section_generator"
                }
                NurbsBoundaryKind::SharedExtrusionGenerator => "shared_extrusion_nurbs_generator",
            },
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model curves")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                parameter_range: None,
                id: crate::identity::copy_checked_id(id.as_str())?,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::new(format!(
                        "VisibGeom:{}",
                        row.id
                    ))
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        result
            .ids
            .insert(crate::identity::copy_checked_id(id.as_str())?);
        result.endpoint_witnesses.insert(id);
        match kind {
            NurbsBoundaryKind::ExtrusionPlane => result.extrusion_plane_count += 1,
            NurbsBoundaryKind::ExtrusionPlaneSectionGenerator => {
                result.extrusion_plane_section_generator_count += 1;
            }
            NurbsBoundaryKind::SharedExtrusionGenerator => {
                result.shared_extrusion_generator_count += 1;
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::resolve_carrier_intersection_curve;
    use std::collections::BTreeSet;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{
        nurbs::NurbsSurface, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
        SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::AnnotationBuilder;

    use super::transfer_carrier_intersection_curves;
    use super::transfer_nurbs_boundary_curves;
    use crate::decode::analytic::equations::{CarrierEquation, PlaneEquation};
    use crate::topology::{HalfEdge, HalfEdgeId, HalfEdgeVertexIncidence};
    use crate::{container, curve, surface};

    #[test]
    fn analytic_curve_branches_keep_a_procedural_curve_as_one_branch() {
        use cadmpeg_ir::ids::ProceduralCurveId;

        let identity = "test:model:procedural#curve";
        let geometry = CurveGeometry::Procedural {
            construction: ProceduralCurveId::mint(identity).expect("valid construction ID"),
            cache: None,
        };
        let branches = { super::analytic_curve_branches(&geometry, "procedural") };
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].0, geometry);
    }

    #[test]
    fn coplanar_face_edge_line_needs_a_face_path_onto_both_solved_endpoints() {
        let plane = |origin, normal| PlaneEquation { origin, normal };
        let faces = [
            plane([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]),
            plane([5.0, 1.0, 2.0], [0.0, 0.0, -1.0]),
        ];
        let points = [[1.0, 0.0, 2.0], [1.0, 3.0, 2.0]];
        let missing = [[1.0, 0.0, 2.0], [1.25, 3.0, 2.0]];
        for path in [points, [points[1], points[0]]] {
            let Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(line))) =
                super::coplanar_face_edge_line(faces, points, &[missing, path])
            else {
                panic!("line through the solved endpoints");
            };
            let origin = line.origin().get();
            let direction = *line.direction().as_raw();
            assert_eq!([origin.x, origin.y, origin.z], points[0]);
            assert_eq!([direction.x, direction.y, direction.z], [0.0, 1.0, 0.0]);
        }
        assert!(super::coplanar_face_edge_line(faces, points, &[missing]).is_none());
        assert!(super::coplanar_face_edge_line(
            faces,
            [points[0], points[0]],
            &[[points[0], points[0]]]
        )
        .is_none());
        let offset = [faces[0], plane([0.0, 0.0, 3.0], [0.0, 0.0, 1.0])];
        assert!(super::coplanar_face_edge_line(offset, points, &[points]).is_none());
        let crossing = [faces[0], plane([1.0, 0.0, 0.0], [1.0, 0.0, 0.0])];
        assert!(super::coplanar_face_edge_line(crossing, points, &[points]).is_none());
    }

    #[test]
    fn shared_cylinder_generator_line_needs_one_generator_and_a_path_onto_it() {
        use crate::decode::analytic::equations::CylinderEquation;

        let cylinder = |origin, radius| CylinderEquation {
            origin,
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius,
        };
        let faces = [
            cylinder([0.0, 0.0, 0.0], 2.0),
            cylinder([0.0, 0.0, 5.0], 2.0),
        ];
        let points = [[2.0, 0.0, 0.0], [2.0, 0.0, 6.0]];
        for path in [points, [points[1], points[0]]] {
            let Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(line))) =
                super::shared_cylinder_generator_line(faces, points, &[path])
            else {
                panic!("generator through the solved endpoints");
            };
            let origin = line.origin().get();
            let direction = *line.direction().as_raw();
            assert_eq!([origin.x, origin.y, origin.z], points[0]);
            assert_eq!([direction.x, direction.y, direction.z], [0.0, 0.0, 1.0]);
        }
        assert!(super::shared_cylinder_generator_line(faces, points, &[]).is_none());
        let skew = [[2.0, 0.0, 0.0], [0.0, 2.0, 6.0]];
        assert!(super::shared_cylinder_generator_line(faces, skew, &[skew]).is_none());
        let outside = [[2.5, 0.0, 0.0], [2.5, 0.0, 6.0]];
        assert!(super::shared_cylinder_generator_line(faces, outside, &[outside]).is_none());
        for other in [
            cylinder([0.0, 0.0, 0.0], 3.0),
            cylinder([1.0, 0.0, 0.0], 2.0),
        ] {
            assert!(
                super::shared_cylinder_generator_line([faces[0], other], points, &[points])
                    .is_none()
            );
        }
    }

    #[test]
    fn cylinder_generator_paths_keep_constant_u_chart_paths() {
        const EPS_GENERATOR_PATH_TEST: f64 = 1e-12;

        let tau = std::f64::consts::TAU;
        let mut scan = crate::test_support::empty_container_scan();
        scan.curves.two_chart_pcurves = vec![
            curve::TwoChartPcurveSamples {
                curve_id: 10,
                faces: [1, 2],
                samples: vec![[[0.0, 0.0], [tau, 0.0]], [[0.0, 6.0], [tau, 6.0]]],
                offset: 0,
            },
            curve::TwoChartPcurveSamples {
                curve_id: 10,
                faces: [1, 2],
                samples: vec![[[0.0, 0.0], [tau, 0.0]], [[1.0, 6.0], [tau, 6.0]]],
                offset: 1,
            },
        ];
        let row = curve::CurveTopologyRow {
            id: 10,
            type_byte: 1,
            feature_id: 0,
            directions: [0x01, 0xf6],
            faces: [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)],
            next_edges: [10, 10],
            offset: 0,
        };
        let mut ir = CadIr::empty();
        ir.model.surfaces.extend([1, 2].map(|id| {
            Surface {
                id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}"))
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        2.0,
                    )
                    .expect("valid CylinderSurface fixture"),
                )),
                source_object: None,
            }
        }));
        let paths = crate::decode::with_service_context(|ctx| {
            super::mapped_cylinder_generator_paths(
                ctx,
                &scan,
                &crate::decode::analytic::pcurves::ModelSurfaceIndex::new(&ir.model.surfaces),
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                &row,
            )
        })
        .expect("bounded chart evaluation");
        assert_eq!(paths.len(), 3);
        for path in paths {
            for (point, height) in path.into_iter().zip([0.0, 6.0]) {
                for (actual, expected) in point.into_iter().zip([2.0, 0.0, height]) {
                    assert!((actual - expected).abs() <= EPS_GENERATOR_PATH_TEST);
                }
            }
        }
    }

    #[test]
    fn carrier_intersection_rejects_solved_endpoints_off_candidate() {
        let first = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        });
        let second = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        });
        let off_candidate = [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0]];

        assert!(
            resolve_carrier_intersection_curve(first, second, Some(off_candidate), false).is_none()
        );
        assert!(
            resolve_carrier_intersection_curve(first, second, Some(off_candidate), true).is_some()
        );
        assert!(resolve_carrier_intersection_curve(first, second, None, false).is_some());
    }

    #[test]
    fn plane_intersection_survives_inconsistent_endpoint_witness() {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.rows = [1_u32, 2, 3]
            .into_iter()
            .map(|id| surface::SurfaceRow {
                id,
                kind: surface::SurfaceKind::Plane,
                feature_id: 0,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: 0,
            })
            .collect();
        scan.curves.topology_rows = vec![curve::CurveTopologyRow {
            id: 10,
            type_byte: 0,
            feature_id: 0,
            directions: [0x01, 0xf6],
            faces: [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)],
            next_edges: [10, 10],
            offset: 0,
        }];
        scan.curves.pcurves = vec![curve::PcurveEndpoints {
            curve_id: 10,
            faces: [1, 3].map(std::num::NonZeroU32::new),
            face_0_endpoints: [[0.0, 1.0], [1.0, 1.0]],
            face_1_endpoints: [[0.0, 0.0], [1.0, 0.0]],
            offset: 0,
        }];
        scan.topology.half_edges = vec![
            HalfEdge {
                id: HalfEdgeId {
                    curve_id: 10,
                    side: crate::topology::Side::Zero,
                },
                face_id: std::num::NonZeroU32::new(1),
                next: None,
            },
            HalfEdge {
                id: HalfEdgeId {
                    curve_id: 10,
                    side: crate::topology::Side::One,
                },
                face_id: std::num::NonZeroU32::new(2),
                next: None,
            },
        ];
        scan.topology.vertices = vec![
            crate::topology::TopologicalVertex::new_for_test(
                1,
                vec![HalfEdgeId {
                    curve_id: 10,
                    side: crate::topology::Side::Zero,
                }],
            )
            .expect("valid vertex fixture"),
            crate::topology::TopologicalVertex::new_for_test(
                2,
                vec![HalfEdgeId {
                    curve_id: 10,
                    side: crate::topology::Side::One,
                }],
            )
            .expect("valid vertex fixture"),
        ];
        scan.topology.half_edge_vertex_incidence = vec![
            HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: 10,
                    side: crate::topology::Side::Zero,
                },
                start_vertex_id: std::num::NonZeroU32::new(1).expect("one-based vertex fixture"),
                end_vertex_id: std::num::NonZeroU32::new(2),
            },
            HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: 10,
                    side: crate::topology::Side::One,
                },
                start_vertex_id: std::num::NonZeroU32::new(2).expect("one-based vertex fixture"),
                end_vertex_id: std::num::NonZeroU32::new(1),
            },
        ];

        let mut ir = CadIr::empty();
        ir.model.surfaces.extend([
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#1".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 2.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
                source_object: None,
            },
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#2".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
                source_object: None,
            },
            Surface {
                id: SurfaceId::mint("creo:visibgeom:surface#3".to_string())
                    .expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                source_object: None,
            },
        ]);

        let transferred = crate::decode::with_service_context(|ctx| {
            transfer_carrier_intersection_curves(
                ctx,
                &scan,
                &mut ir,
                &mut AnnotationBuilder::new(),
                &BTreeSet::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("valid source object identity");
        assert_eq!(
            transferred,
            BTreeSet::from([
                CurveId::mint("creo:visibgeom:curve#10".to_string()).expect("identity grammar")
            ])
        );
        assert!(matches!(ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id
            == CurveId::mint("creo:visibgeom:curve#10".to_string()).expect("identity grammar"))
        .map(|curve| &curve.geometry), Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)))
            if {
                let origin = line_curve.origin().get();
                let direction = *line_curve.direction().as_raw();
                origin.x == 0.0
                    && origin.y == 2.0
                    && origin.z == 0.0
                    && direction.x == 1.0
                    && direction.y == 0.0
                    && direction.z == 0.0
            }));
    }

    fn nurbs_boundary_fixture(
        duplicate_extrusion: bool,
    ) -> (container::ContainerScan<'static>, CadIr) {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.rows = vec![
            surface::SurfaceRow {
                id: 1,
                kind: surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
                feature_id: 0,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: 0,
            },
            surface::SurfaceRow {
                id: 2,
                kind: surface::SurfaceKind::Plane,
                feature_id: 0,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: 0,
            },
        ];
        scan.curves.topology_rows = vec![curve::CurveTopologyRow {
            id: 10,
            type_byte: 0,
            feature_id: 0,
            directions: [0; 2],
            faces: [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)],
            next_edges: [10, 10],
            offset: 0,
        }];

        let extrusion = Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#1".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                NurbsSurface::from_lanes(
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                        1,
                        vec![0.0, 0.0, 1.0, 1.0],
                        false,
                    ),
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                        1,
                        vec![0.0, 0.0, 1.0, 1.0],
                        false,
                    ),
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                        vec![
                            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                            vec![Point3::new(1.0, 0.0, 1.0), Point3::new(1.0, 1.0, 1.0)],
                        ],
                        None,
                    ),
                    false,
                )
                .expect("valid test surface"),
            )),
            source_object: None,
        };
        let plane = Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#2".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        };
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(extrusion.clone());
        if duplicate_extrusion {
            ir.model.surfaces.push(extrusion);
        }
        ir.model.surfaces.push(plane);
        (scan, ir)
    }

    #[test]
    fn nurbs_boundary_rejects_duplicate_model_surface_ids() {
        let (scan, mut ir) = nurbs_boundary_fixture(true);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default())
            .expect("test decode context");

        let result = transfer_nurbs_boundary_curves(
            &ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
        .expect("transfer should not fail");

        assert!(result.ids.is_empty());
        assert!(ir.model.curves.is_empty());
    }

    #[test]
    fn nurbs_boundary_transfer_records_one_curve_and_endpoint_witness() {
        let (scan, mut ir) = nurbs_boundary_fixture(false);
        let service = crate::decode::with_service_context(|ctx| {
            transfer_nurbs_boundary_curves(
                ctx,
                &scan,
                &mut ir,
                &mut AnnotationBuilder::new(),
                &mut Vec::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("transfer the NURBS boundary curves");
        assert_eq!(service.ids.len(), 1);
        assert_eq!(service.endpoint_witnesses.len(), 1);
    }

    #[test]
    fn refused_boundary_lanes_name_the_curve_topology_row_and_continue() {
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        refusal.note(
            "creo VisibGeom surface row 7 boundary curve record",
            &cadmpeg_ir::geometry::nurbs::NurbsError::Structure("knot vector".to_owned()),
        );
        let mut losses = Vec::new();

        super::note_refused_boundary_lanes(41, &mut refusal, &mut losses);

        let [note] = losses.as_slice() else {
            panic!("one loss note per refused record");
        };
        assert_eq!(
            note.code,
            crate::loss::CreoLossCode::NurbsBoundaryCarrierUnresolved.kind()
        );
        assert!(note.message.contains("row 41"), "{}", note.message);
        assert!(note.message.contains("surface row 7"), "{}", note.message);
        assert!(refusal.take_records().is_empty());
    }

    #[test]
    fn boundary_loss_message_names_the_curve_row() {
        let mut service = Vec::new();
        super::note_boundary_lane_records(41, &["surface row 7".to_owned()], &mut service);
        assert_eq!(service.len(), 1);
        assert!(service[0].message.contains("row 41"));
    }
}
