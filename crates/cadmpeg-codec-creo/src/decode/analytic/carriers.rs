// SPDX-License-Identifier: Apache-2.0
//! Placed carriers, topology-bound plane transfer, and face orientations.

use crate::container::SectionRole;
use crate::feature::schema::SchemaClass;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId, UnknownId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;
use crate::decode::source_carriers::SourceUnitCarriers;
use crate::legacy_geometry::LegacySurfaceNamespace;
use crate::topology::HalfEdgeId;

use super::super::native::annotate;
use super::super::surfaces::cylinders::rowless_round_cylinder_pairs;
use super::super::surfaces::matches_native_surface_id;

use super::super::uniqueness::exactly_one;
use super::equations::{
    CarrierEquation, ConeEquation, CylinderEquation, PlaneEquation, SphereEquation, TorusEquation,
};
use super::planes::{
    agreed_plane, agreed_topology_bound_plane, analytic_boundary_line, analytic_curve_plane,
    placed_planes, topology_bound_plane,
};
use super::vertices::solved_topological_vertices;

const EPS_AGREE: f64 = 1.0e-9;
const EPS_NEAR_ZERO: f64 = 1.0e-12;

fn existing_plane_agrees_with_topology(
    geometry: &SurfaceGeometry,
    topology: PlaneEquation,
) -> Option<bool> {
    match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            Some(
                agreed_plane(&[
                    PlaneEquation {
                        origin: [origin.x, origin.y, origin.z],
                        normal: [normal.x, normal.y, normal.z],
                    },
                    topology,
                ])
                .is_some(),
            )
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => None,
        _ => Some(false),
    }
}

pub(in crate::decode) fn transfer_topology_bound_planes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let carriers = placed_carriers(ctx, scan, ir, source_carriers)?;
    let solved_vertices = solved_topological_vertices(
        ctx,
        scan,
        ir,
        &carriers,
        nurbs_endpoint_witnesses,
        source_carriers,
    )?;
    let vertex_faces =
        crate::topology::vertex_incident_faces(&scan.topology.vertices, &scan.topology.half_edges);
    let unique_rows = crate::identity::uniquely_identified_rows(&scan.surfaces.rows, |row| row.id);
    let mut unique_curve_ids = BTreeSet::new();
    for row in crate::identity::uniquely_identified_rows(&scan.curves.topology_rows, |row| row.id) {
        unique_curve_ids.insert(row.id);
    }
    let mut transferred = 0;
    for row in unique_rows
        .into_iter()
        .filter(|row| row.kind == crate::surface::SurfaceKind::Plane)
    {
        let id = crate::identity::compose_checked::<SurfaceId>(
            &crate::identity::VISIBGEOM_SURFACE,
            row.id,
        )?;
        let points = topology_bound_face_points(&solved_vertices, &vertex_faces, row.id);
        let mut boundary_curves = Vec::new();
        for half_edge in scan
            .topology
            .loops
            .iter()
            .filter(|lp| lp.face_id() == std::num::NonZeroU32::new(row.id))
            .flat_map(|lp| lp.half_edges().iter())
        {
            if unique_curve_ids.contains(&half_edge.curve_id) {
                let id = crate::identity::compose_checked::<CurveId>(
                    &crate::identity::VISIBGEOM_CURVE,
                    half_edge.curve_id,
                )?;
                if let Some(curve) =
                    exactly_one(ir.model.curves.iter().filter(|curve| curve.id == id))
                {
                    boundary_curves.push(source_carriers.curve_geometry(curve));
                }
            }
        }
        let mut curve_planes = Vec::new();
        for geometry in &boundary_curves {
            if let Some(plane) = analytic_curve_plane(geometry) {
                curve_planes.push(plane);
            }
        }
        let lines = boundary_curves
            .iter()
            .filter_map(|geometry| analytic_boundary_line(geometry));
        let Some(plane) = agreed_topology_bound_plane(points, curve_planes, lines) else {
            continue;
        };
        let existing_count = ir
            .model
            .surfaces
            .iter()
            .filter(|surface| surface.id == id)
            .count();
        if existing_count != 0 {
            let conflict = existing_count != 1
                || ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == id)
                    .is_some_and(|surface| {
                        existing_plane_agrees_with_topology(
                            source_carriers.surface_geometry(surface),
                            plane,
                        ) == Some(false)
                    });
            if !conflict {
                continue;
            }
            source_carriers.remove_surface(&id);
            for surface in ir
                .model
                .surfaces
                .iter_mut()
                .filter(|surface| surface.id == id)
            {
                surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                    record: geometry_section_record(scan, row.offset)?,
                });
            }
            annotate(
                annotations,
                &id,
                "VisibGeom",
                cadmpeg_core::decode::u64_from_index(row.offset),
                if existing_count == 1 {
                    "conflicting_topology_plane_carrier"
                } else {
                    "duplicate_topology_plane_carrier"
                },
                Exactness::Unknown,
            )?;
            continue;
        }
        let normal = Vector3::from(plane.normal);
        let Ok(plane_surface) = cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::from(plane.origin),
            normal,
            cadmpeg_ir::geometry::derive_reference_direction(normal),
        ) else {
            continue;
        };
        annotate(
            annotations,
            &id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(row.offset),
            "plane_topology_boundary",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(format_args!(
                        "VisibGeom:{}",
                        row.id
                    ))?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        transferred += 1;
    }
    Ok(transferred)
}

fn topology_bound_face_points(
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    vertex_faces: &BTreeMap<std::num::NonZeroU32, BTreeSet<u32>>,
    face_id: u32,
) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    for (vertex_id, point) in solved_vertices {
        if std::num::NonZeroU32::new(*vertex_id)
            .and_then(|id| vertex_faces.get(&id))
            .is_some_and(|faces| faces.contains(&face_id))
        {
            points.push(*point);
        }
    }
    points
}

pub(in crate::decode) fn retain_unresolved_surface_carriers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<(), cadmpeg_core::CodecError> {
    for (rows, namespace) in [
        (&scan.surfaces.rows, LegacySurfaceNamespace::Visible),
        (
            &scan.surfaces.nonvisible_rows,
            LegacySurfaceNamespace::NonVisible,
        ),
    ] {
        for row in crate::identity::uniquely_identified_rows(rows, |row| row.id) {
            let identity_namespace = match namespace {
                LegacySurfaceNamespace::Visible => &crate::identity::VISIBGEOM_SURFACE,
                LegacySurfaceNamespace::NonVisible => &crate::identity::NOVISGEOM_SURFACE,
            };
            let id = crate::identity::compose_checked::<SurfaceId>(identity_namespace, row.id)?;
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                if namespace.is_visible() {
                    "VisibGeom"
                } else {
                    "NovisGeom"
                },
                cadmpeg_core::decode::u64_from_index(row.offset),
                if namespace.is_visible() {
                    "unresolved_visible_surface_carrier"
                } else {
                    "unresolved_nonvisible_surface_carrier"
                },
                Exactness::Unknown,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: geometry_section_record(scan, row.offset)?,
                    }),
                    source_object: Some(SourceObjectAssociation {
                        geometry_role: None,
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(format_args!(
                            "{}{}",
                            namespace.source_prefix(),
                            row.id
                        ))?,
                        name: None,
                        color: None,
                        visible: if namespace.is_visible() {
                            None
                        } else {
                            Some(false)
                        },
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                },
            )?;
        }
    }
    for row in crate::identity::uniquely_identified_rows(&scan.curves.topology_rows, |row| row.id) {
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
            "unresolved_visible_curve_carrier",
            Exactness::Unknown,
        )?;
        ctx.charge_entities(1, "admit Creo model curves")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                parameter_range: None,
                id,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                    record: geometry_section_record(scan, row.offset)?,
                }),
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(format_args!(
                        "VisibGeom:{}",
                        row.id
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

pub(in crate::decode) fn placed_carriers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &SourceUnitCarriers,
) -> Result<BTreeMap<u32, CarrierEquation>, cadmpeg_core::CodecError> {
    let mut carriers = BTreeMap::new();
    for (id, plane) in placed_planes(scan)? {
        carriers.insert(id, CarrierEquation::Plane(plane));
    }
    let rows = scan
        .surfaces
        .rows
        .iter()
        .chain(&scan.surfaces.nonvisible_rows);
    let mut row_ids = BTreeSet::new();
    let mut row_counts = BTreeMap::<u32, usize>::new();
    for row in rows {
        row_ids.insert(row.id);
        match row_counts.entry(row.id) {
            std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() += 1,
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(1);
            }
        }
    }
    for (namespace_rows, parameters) in [
        (&scan.surfaces.rows, &scan.surfaces.parameters),
        (
            &scan.surfaces.nonvisible_rows,
            &scan.surfaces.nonvisible_parameters,
        ),
    ] {
        for row in namespace_rows
            .iter()
            .filter(|row| row_counts.get(&row.id) == Some(&1))
        {
            if let Some(carrier) =
                positional_cylinder_carrier(ctx, scan, row, parameters, ir, source_carriers)?
            {
                carriers.insert(row.id, carrier);
                continue;
            }
            let mut model_surfaces = ir
                .model
                .surfaces
                .iter()
                .filter(|surface| matches_native_surface_id(scan, row.id, &surface.id));
            let surface = match (model_surfaces.next(), model_surfaces.next()) {
                (None, _) => continue,
                (Some(surface), None) => surface,
                (Some(_), Some(_)) => {
                    carriers.remove(&row.id);
                    continue;
                }
            };
            if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
                source_carriers.surface_geometry(surface)
            {
                let origin = plane_surface.origin().get();
                let normal = plane_surface.frame().axis().as_raw();
                let plane = PlaneEquation {
                    origin: [origin.x, origin.y, origin.z],
                    normal: [normal.x, normal.y, normal.z],
                };
                let agreed = match carriers.get(&row.id) {
                    Some(CarrierEquation::Plane(existing)) => agreed_plane(&[*existing, plane]),
                    Some(_) => None,
                    None => Some(plane),
                };
                if let Some(plane) = agreed {
                    carriers.insert(row.id, CarrierEquation::Plane(plane));
                } else {
                    carriers.remove(&row.id);
                }
            } else if let Some(carrier) = surface_carrier(source_carriers.surface_geometry(surface))
            {
                carriers.insert(row.id, carrier);
            }
        }
    }
    for datum in &scan.planes.datum_cylinders {
        let Some(surface) = exactly_one(
            ir.model
                .surfaces
                .iter()
                .filter(|surface| matches_native_surface_id(scan, datum.id, &surface.id)),
        ) else {
            carriers.remove(&datum.id);
            continue;
        };
        if let Some(carrier) = surface_carrier(source_carriers.surface_geometry(surface)) {
            carriers.insert(datum.id, carrier);
        } else {
            carriers.remove(&datum.id);
        }
    }
    let mut model_surfaces_by_id = BTreeMap::<u32, Vec<&Surface>>::new();
    for surface in &ir.model.surfaces {
        let Some(id) = surface
            .id
            .as_str()
            .strip_prefix("creo:visibgeom:surface#")
            .or_else(|| surface.id.as_str().strip_prefix("creo:novisgeom:surface#"))
            .and_then(|id| id.parse().ok())
        else {
            continue;
        };
        let surfaces = match model_surfaces_by_id.entry(id) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Vec::new()),
        };
        surfaces.push(surface);
    }
    for (id, model_surfaces) in model_surfaces_by_id {
        if row_ids.contains(&id) {
            continue;
        }
        let Some(surface) = exactly_one(model_surfaces.into_iter()) else {
            carriers.remove(&id);
            continue;
        };
        if let Some(carrier) = surface_carrier(source_carriers.surface_geometry(surface)) {
            carriers.insert(id, carrier);
        }
    }
    Ok(carriers)
}

fn positional_cylinder_carrier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    row: &crate::surface::SurfaceRow,
    parameters: &[crate::surface::SurfaceParameterRecord],
    ir: &CadIr,
    source_carriers: &SourceUnitCarriers,
) -> Result<Option<CarrierEquation>, cadmpeg_core::CodecError> {
    if row.kind != crate::surface::SurfaceKind::Cylinder {
        return Ok(None);
    }
    let Some(record) = crate::surface::unique_surface_parameter(parameters, row.id) else {
        return Ok(None);
    };
    let inline = record.has_inline_non_plane_envelope()
        || record.has_inline_non_plane_local_system_suffix(ctx)?
        || record.selector_corner_interval_cylinder_frame().is_some();
    if crate::decode::sketch_transfer::recipe::feature_schema_class(scan, row.feature_id)
        == Some(SchemaClass::Round)
        && !inline
    {
        return Ok(None);
    }
    if crate::decode::sketch_transfer::recipe::feature_schema_class(scan, row.feature_id)
        == Some(SchemaClass::Round)
        && inline
    {
        if let Some(surface) = exactly_one(
            ir.model
                .surfaces
                .iter()
                .filter(|surface| matches_native_surface_id(scan, row.id, &surface.id)),
        ) {
            if let Some(carrier) = surface_carrier(source_carriers.surface_geometry(surface)) {
                return Ok(Some(carrier));
            }
        }
    }
    let Some(frame) = record.positional_cylinder_frame() else {
        return Ok(None);
    };
    Ok(Some(CarrierEquation::Cylinder(CylinderEquation {
        origin: frame.frame().origin(),
        axis: frame.frame().axis(),
        ref_direction: frame.frame().ref_direction(),
        radius: frame.radius().get(),
    })))
}

fn surface_carrier(geometry: &SurfaceGeometry) -> Option<CarrierEquation> {
    match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            Some(CarrierEquation::Plane(PlaneEquation {
                origin: [origin.x, origin.y, origin.z],
                normal: [normal.x, normal.y, normal.z],
            }))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let ref_direction = cylinder_surface.frame().reference().as_raw();
            let radius = cylinder_surface.radius().get();
            Some(CarrierEquation::Cylinder(CylinderEquation {
                origin: [origin.x, origin.y, origin.z],
                axis: [axis.x, axis.y, axis.z],
                ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                radius,
            }))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let center = sphere_surface.center().get();
            let ref_direction = sphere_surface.frame().reference().as_raw();
            let radius = sphere_surface.radius().get();
            Some(CarrierEquation::Sphere(SphereEquation {
                center: [center.x, center.y, center.z],
                ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                radius,
            }))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let ref_direction = cone_surface.frame().reference().as_raw();
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            Some(CarrierEquation::Cone(ConeEquation::new(
                [origin.x, origin.y, origin.z],
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
                radius,
                ratio,
                half_angle,
            )?))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let ref_direction = torus_surface.frame().reference().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            Some(CarrierEquation::Torus(TorusEquation {
                center: [center.x, center.y, center.z],
                axis: [axis.x, axis.y, axis.z],
                ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                major_radius,
                minor_radius,
            }))
        }
        _ => None,
    }
}

pub(in crate::decode) fn geometry_section_record(
    scan: &ContainerScan,
    offset: usize,
) -> Result<Option<UnknownId>, cadmpeg_core::CodecError> {
    let Some(section) = scan
        .framing
        .sections
        .iter()
        .filter(|section| section.role() == SectionRole::PsbGeometry)
        .find(|section| section.contains(offset))
    else {
        return Ok(None);
    };
    let Some(namespace) = crate::identity::section_namespace(section.name()) else {
        return Ok(None);
    };
    crate::identity::compose_checked(&namespace, section.offset()).map(Some)
}

#[cfg(test)]
mod tests;

fn projected_loop_polygon(
    lp: &crate::topology::Loop,
    plane: PlaneEquation,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<[f64; 2]>> {
    let dropped_axis = (0..3).max_by(|left, right| {
        plane.normal[*left]
            .abs()
            .total_cmp(&plane.normal[*right].abs())
    })?;
    let mut polygon = Vec::new();
    for half_edge in lp.half_edges() {
        let point = incidence
            .get(half_edge)
            .and_then(|binding| solved_vertices.get(&binding.start_vertex_id.get()))?;
        polygon.push(match dropped_axis {
            0 => [point[1], point[2]],
            1 => [point[0], point[2]],
            _ => [point[0], point[1]],
        });
    }
    valid_parameter_polygon(&polygon).then_some(polygon)
}

fn polygon_strictly_contains(polygon: &[[f64; 2]], point: [f64; 2]) -> bool {
    use cadmpeg_ir::math::{
        planar::{orientation, point_segment_distance},
        Point2,
    };
    use cadmpeg_ir::units::FinitePoint2;
    use std::cmp::Ordering;
    if polygon.len() < 3 {
        return false;
    }
    let point = Point2::new(point[0], point[1]);
    let finite_point = FinitePoint2::new(point);
    let mut inside = false;
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let first = Point2::new(first[0], first[1]);
        let second = Point2::new(second[0], second[1]);
        let length = (second.u - first.u).hypot(second.v - first.v);
        // A point or edge end that is not finite has no distance to measure.
        let on_edge = match [
            finite_point,
            FinitePoint2::new(first),
            FinitePoint2::new(second),
        ] {
            [Some(point), Some(first), Some(second)] => {
                point_segment_distance(point, first, second) <= EPS_AGREE * length
            }
            _ => false,
        };
        if on_edge {
            return false;
        }
        if (first.v > point.v) != (second.v > point.v) {
            let side = if second.v > first.v {
                Ordering::Greater
            } else {
                Ordering::Less
            };
            if orientation(first, second, point) == Some(side) {
                inside = !inside;
            }
        }
    }
    inside
}

fn segments_intersect(first: [[f64; 2]; 2], second: [[f64; 2]; 2]) -> bool {
    use cadmpeg_ir::math::Point2;
    let points = [first[0], first[1], second[0], second[1]];
    let scale = (0..points.len())
        .flat_map(|first| {
            (first + 1..points.len()).map(move |second| {
                (points[first][0] - points[second][0]).hypot(points[first][1] - points[second][1])
            })
        })
        .fold(0.0_f64, f64::max);
    let [a, b, c, d] = points.map(|point| Point2::new(point[0], point[1]));
    cadmpeg_ir::math::planar::segments_intersect(a, b, c, d, EPS_AGREE * scale)
}

fn polygon_strictly_contains_polygon(outer: &[[f64; 2]], inner: &[[f64; 2]]) -> bool {
    inner
        .iter()
        .copied()
        .all(|point| polygon_strictly_contains(outer, point))
        && (0..inner.len()).all(|index| {
            let inner_edge = [inner[index], inner[(index + 1) % inner.len()]];
            (0..outer.len()).all(|outer_index| {
                let outer_edge = [outer[outer_index], outer[(outer_index + 1) % outer.len()]];
                !segments_intersect(inner_edge, outer_edge)
            })
        })
}

fn valid_parameter_polygon(polygon: &[[f64; 2]]) -> bool {
    let Some(origin) = polygon.first() else {
        return false;
    };
    if polygon.len() < 3 || polygon.iter().flatten().any(|value| !value.is_finite()) {
        return false;
    }
    let scale = polygon
        .iter()
        .flat_map(|point| (0..2).map(|axis| (point[axis] - origin[axis]).abs()))
        .fold(0.0_f64, f64::max);
    if scale == 0.0 || !scale.is_finite() {
        return false;
    }
    let mut local = Vec::new();
    for point in polygon {
        local.push(cadmpeg_ir::math::Point2::new(
            (point[0] - origin[0]) / scale,
            (point[1] - origin[1]) / scale,
        ));
    }
    cadmpeg_ir::math::planar::polygon_area_twice(&local)
        .is_some_and(|area| area.get().abs() > EPS_NEAR_ZERO)
}

fn ordered_contained_face_loops<'a>(
    mut loops: Vec<&'a crate::topology::Loop>,
    polygons: &[Vec<[f64; 2]>],
) -> Option<Vec<&'a crate::topology::Loop>> {
    if loops.len() < 2 || loops.len() != polygons.len() {
        return None;
    }
    for polygon in polygons {
        if !valid_parameter_polygon(polygon) {
            return None;
        }
    }
    let mut outer = None;
    for index in polygons
        .iter()
        .enumerate()
        .filter(|(candidate, polygon)| {
            polygons.iter().enumerate().all(|(index, inner)| {
                index == *candidate || polygon_strictly_contains_polygon(polygon, inner)
            })
        })
        .map(|(index, _)| index)
    {
        if outer.replace(index).is_some() {
            return None;
        }
    }
    let outer = outer?;
    let selected = loops.remove(outer);
    loops.insert(0, selected);
    Some(loops)
}

pub(in crate::decode) fn ordered_planar_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    plane: PlaneEquation,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    if loops.len() == 1 {
        return Some(loops);
    }
    let mut polygons = Vec::new();
    for lp in &loops {
        let polygon = projected_loop_polygon(lp, plane, incidence, solved_vertices)?;
        polygons.push(polygon);
    }
    ordered_contained_face_loops(loops, &polygons)
}

pub(in crate::decode) fn ordered_parameter_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    polygons: &[Vec<[f64; 2]>],
) -> Option<Vec<&'a crate::topology::Loop>> {
    if loops.len() == 1 {
        return Some(loops);
    }
    ordered_contained_face_loops(loops, polygons)
}

fn face_boundary_plane(
    loops: &[&crate::topology::Loop],
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<PlaneEquation> {
    topology_bound_plane(loops.iter().flat_map(|lp| {
        lp.half_edges()
            .iter()
            .filter_map(|half_edge| incidence.get(half_edge))
            .filter_map(|binding| solved_vertices.get(&binding.start_vertex_id.get()).copied())
    }))
}

pub(in crate::decode) fn ordered_face_loops<'a>(
    loops: &[&'a crate::topology::Loop],
    plane: Option<PlaneEquation>,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    let mut ordered_input = Vec::new();
    ordered_input.extend_from_slice(loops);
    let plane = match plane {
        Some(plane) => Some(plane),
        None => face_boundary_plane(&ordered_input, incidence, solved_vertices),
    };
    if let Some(plane) = plane {
        ordered_planar_face_loops(ordered_input, plane, incidence, solved_vertices)
    } else {
        (ordered_input.len() == 1).then_some(ordered_input)
    }
}

pub(in crate::decode) fn rowless_round_face_orientations(
    round_feature_ids: &BTreeSet<u32>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
    available_surfaces: &BTreeSet<u32>,
) -> BTreeMap<u32, bool> {
    let mut orientations = BTreeMap::new();
    for (rowless_id, sibling_id, _) in rowless_round_cylinder_pairs(round_feature_ids, tables, rows)
    {
        if !available_surfaces.contains(&rowless_id) {
            continue;
        }
        let Some(reversed) =
            crate::surface::unique_surface_row(rows, sibling_id).map(|row| row.reversed)
        else {
            continue;
        };
        orientations.insert(rowless_id, reversed);
    }
    orientations
}

pub(in crate::decode) fn native_face_orientations(
    scan: &ContainerScan,
    ir: &CadIr,
) -> BTreeMap<u32, bool> {
    let mut source_ids = BTreeSet::new();
    for row in scan
        .surfaces
        .rows
        .iter()
        .chain(&scan.surfaces.nonvisible_rows)
    {
        source_ids.insert(row.id);
    }
    let mut orientations = BTreeMap::new();
    for id in source_ids {
        if let Some(row) = crate::decode::surfaces::unique_native_surface_row(scan, id) {
            orientations.insert(id, row.reversed);
        }
    }
    for datum in &scan.planes.datum_cylinders {
        orientations.insert(datum.id, datum.reversed);
    }
    let mut round_feature_ids = BTreeSet::new();
    for row in scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(SchemaClass::Round))
    {
        round_feature_ids.insert(row.feature_id);
    }
    let mut available_surfaces = BTreeSet::new();
    for surface in &ir.model.surfaces {
        if let Some(id) = surface
            .id
            .as_str()
            .strip_prefix("creo:visibgeom:surface#")
            .and_then(|suffix| suffix.parse::<u32>().ok())
        {
            available_surfaces.insert(id);
        }
    }
    for (id, reversed) in rowless_round_face_orientations(
        &round_feature_ids,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
        &available_surfaces,
    ) {
        orientations.insert(id, reversed);
    }
    orientations
}

#[cfg(test)]
mod namespace_tests {
    use super::native_face_orientations;
    use cadmpeg_ir::document::CadIr;

    #[test]
    fn native_face_orientations_reads_nonvisible_rows() {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces
            .nonvisible_rows
            .push(crate::surface::SurfaceRow {
                id: 17,
                kind: crate::surface::SurfaceKind::Plane,
                feature_id: 1,
                reversed: true,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: 0,
            });

        let orientations = { native_face_orientations(&scan, &CadIr::empty()) };

        assert_eq!(orientations.get(&17), Some(&true));
    }
}
