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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &SurfaceGeometry,
    topology: PlaneEquation,
) -> Result<Option<bool>, cadmpeg_core::CodecError> {
    match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            Ok(Some(
                agreed_plane(
                    ctx,
                    &[
                        PlaneEquation {
                            origin: [origin.x, origin.y, origin.z],
                            normal: [normal.x, normal.y, normal.z],
                        },
                        topology,
                    ],
                )?
                .is_some(),
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => Ok(None),
        _ => Ok(Some(false)),
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
    let vertex_faces = crate::topology::vertex_incident_faces(
        ctx,
        &scan.topology.vertices,
        &scan.topology.half_edges,
    )?;
    let unique_rows =
        crate::identity::uniquely_identified_rows_checked(ctx, &scan.surfaces.rows, |row| row.id)?;
    let mut unique_curve_ids = BTreeSet::new();
    let unique_curve_rows = crate::identity::uniquely_identified_rows_checked(
        ctx,
        &scan.curves.topology_rows,
        |row| row.id,
    )?;
    for row in ctx.admit_iter(&unique_curve_rows, "creo topology-bound unique curve rows")? {
        ctx.insert_btree_set(
            &mut unique_curve_ids,
            row.id,
            "creo topology-bound unique curve IDs",
        )?;
    }
    let mut transferred = 0;
    for row in ctx
        .admit_iter(&unique_rows, "creo topology-bound unique surface rows")?
        .filter(|row| row.kind == crate::surface::SurfaceKind::Plane)
    {
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            row.id,
            "creo decoded model identity",
        )?;
        let points = topology_bound_face_points(ctx, &solved_vertices, &vertex_faces, row.id)?;
        let mut boundary_curves = Vec::new();
        let face_id = std::num::NonZeroU32::new(row.id);
        for lp in ctx.admit_iter(&scan.topology.loops, "creo topology-bound face loops")? {
            if !ctx.equal(
                &lp.face_id(),
                &face_id,
                "creo topology-bound face ID comparison",
            )? {
                continue;
            }
            for half_edge in
                ctx.admit_iter(lp.half_edges(), "creo topology-bound face half edges")?
            {
                if unique_curve_ids.contains(&half_edge.curve_id) {
                    let (id, _id_reservation) = crate::identity::compose_scoped::<CurveId>(
                        ctx,
                        &crate::identity::VISIBGEOM_CURVE,
                        half_edge.curve_id,
                        "creo topology-bound curve lookup identity",
                    )?;
                    let mut matching_curve = None;
                    for curve in
                        ctx.admit_iter(&ir.model.curves, "creo topology-bound model curves")?
                    {
                        if ctx.equal(
                            &curve.id,
                            &id,
                            "creo topology-bound model curve ID comparison",
                        )? {
                            if matching_curve.is_some() {
                                matching_curve = None;
                                break;
                            }
                            matching_curve = Some(curve);
                        }
                    }
                    if let Some(curve) = matching_curve {
                        ctx.reserve_vec(
                            &mut boundary_curves,
                            1,
                            "creo topology-bound boundary curves",
                        )?;
                        boundary_curves.push(source_carriers.curve_geometry(curve));
                    }
                }
            }
        }
        let mut curve_planes = Vec::new();
        for geometry in ctx.admit_iter(&boundary_curves, "creo topology-bound boundary curves")? {
            if let Some(plane) = analytic_curve_plane(ctx, geometry)? {
                ctx.reserve_vec(&mut curve_planes, 1, "creo topology-bound curve planes")?;
                curve_planes.push(plane);
            }
        }
        let Some(plane) = agreed_topology_bound_plane(ctx, &points, &curve_planes, |lines| {
            for geometry in
                ctx.admit_iter(&boundary_curves, "creo topology-bound line candidates")?
            {
                if let Some(line) = analytic_boundary_line(ctx, geometry)? {
                    ctx.reserve_vec(lines, 1, "creo plane boundary lines")?;
                    lines.push(line);
                }
            }
            Ok(())
        })?
        else {
            continue;
        };
        let mut existing_count = 0_usize;
        for surface in
            ctx.admit_iter(&ir.model.surfaces, "creo topology-bound existing surfaces")?
        {
            if ctx.equal(
                &surface.id,
                &id,
                "creo topology-bound existing surface ID comparison",
            )? {
                existing_count = existing_count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "creo topology-bound existing surface count",
                        u64::MAX,
                        1,
                    )
                })?;
            }
        }
        if existing_count != 0 {
            let mut conflict = existing_count != 1;
            if !conflict {
                let mut matching_surface = None;
                for surface in ctx.admit_iter(
                    &ir.model.surfaces,
                    "creo topology-bound existing surface lookup",
                )? {
                    if ctx.equal(
                        &surface.id,
                        &id,
                        "creo topology-bound existing surface lookup ID comparison",
                    )? {
                        matching_surface = Some(surface);
                        break;
                    }
                }
                if let Some(surface) = matching_surface {
                    conflict = existing_plane_agrees_with_topology(
                        ctx,
                        source_carriers.surface_geometry(surface),
                        plane,
                    )? == Some(false);
                }
            }
            if !conflict {
                continue;
            }
            source_carriers.remove_surface(&id);
            for surface in &mut ir.model.surfaces {
                if ctx.equal(
                    &surface.id,
                    &id,
                    "creo topology-bound mutable surface ID comparison",
                )? {
                    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: geometry_section_record(ctx, scan, row.offset)?,
                    });
                }
            }
            annotate(
                ctx,
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
            ctx,
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
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(
                        ctx,
                        format_args!("VisibGeom:{}", row.id),
                        "creo source object identity",
                    )?,
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    vertex_faces: &BTreeMap<std::num::NonZeroU32, BTreeSet<u32>>,
    face_id: u32,
) -> Result<Vec<[f64; 3]>, cadmpeg_core::CodecError> {
    let mut points = Vec::new();
    for (vertex_id, point) in
        ctx.admit_iter(solved_vertices, "creo topology-bound solved vertices")?
    {
        let incident_faces = if let Some(id) = std::num::NonZeroU32::new(*vertex_id) {
            ctx.get_btree_map(vertex_faces, &id, "creo topology-bound vertex face lookup")?
        } else {
            None
        };
        if incident_faces.is_some_and(|faces| faces.contains(&face_id)) {
            ctx.reserve_vec(&mut points, 1, "creo topology-bound face points")?;
            points.push(*point);
        }
    }
    Ok(points)
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
        let unique_rows =
            crate::identity::uniquely_identified_rows_checked(ctx, rows, |row| row.id)?;
        for row in ctx.admit_iter(&unique_rows, "creo unresolved surface rows")? {
            let identity_namespace = match namespace {
                LegacySurfaceNamespace::Visible => &crate::identity::VISIBGEOM_SURFACE,
                LegacySurfaceNamespace::NonVisible => &crate::identity::NOVISGEOM_SURFACE,
            };
            let id = crate::identity::compose_checked::<SurfaceId>(
                ctx,
                identity_namespace,
                row.id,
                "creo decoded model identity",
            )?;
            let mut surface_exists = false;
            for surface in ctx.admit_iter(
                &ir.model.surfaces,
                "creo unresolved surface identity lookup",
            )? {
                if ctx.equal(
                    &surface.id,
                    &id,
                    "creo unresolved surface identity comparison",
                )? {
                    surface_exists = true;
                    break;
                }
            }
            if surface_exists {
                continue;
            }
            annotate(
                ctx,
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
                        record: geometry_section_record(ctx, scan, row.offset)?,
                    }),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(
                            ctx,
                            format_args!("{}{}", namespace.source_prefix(), row.id),
                            "creo source object identity",
                        )?,
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
    let unique_curve_rows = crate::identity::uniquely_identified_rows_checked(
        ctx,
        &scan.curves.topology_rows,
        |row| row.id,
    )?;
    for row in ctx.admit_iter(&unique_curve_rows, "creo unresolved curve rows")? {
        let id = crate::identity::compose_checked::<CurveId>(
            ctx,
            &crate::identity::VISIBGEOM_CURVE,
            row.id,
            "creo decoded model identity",
        )?;
        let mut curve_exists = false;
        for curve in ctx.admit_iter(&ir.model.curves, "creo unresolved curve identity lookup")? {
            if ctx.equal(&curve.id, &id, "creo unresolved curve identity comparison")? {
                curve_exists = true;
                break;
            }
        }
        if curve_exists {
            continue;
        }
        annotate(
            ctx,
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
                id,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                    record: geometry_section_record(ctx, scan, row.offset)?,
                }),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(
                        ctx,
                        format_args!("VisibGeom:{}", row.id),
                        "creo source object identity",
                    )?,
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
    let placed_planes = placed_planes(ctx, scan)?;
    for (id, plane) in ctx.admit_iter(&placed_planes, "creo placed planes")? {
        ctx.insert_btree_map(
            &mut carriers,
            *id,
            CarrierEquation::Plane(*plane),
            "creo placed carrier nodes",
        )?;
    }
    let mut row_ids = BTreeSet::new();
    let mut row_counts = BTreeMap::<u32, usize>::new();
    for row in ctx
        .admit_iter(&*scan.surfaces.rows, "creo placed carrier visible rows")?
        .chain(ctx.admit_iter(
            &*scan.surfaces.nonvisible_rows,
            "creo placed carrier nonvisible rows",
        )?)
    {
        ctx.insert_btree_set(&mut row_ids, row.id, "creo placed carrier row IDs")?;
        match ctx.entry_btree_map(&mut row_counts, row.id, "creo placed carrier row counts")? {
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
        for row in ctx
            .admit_iter(&**namespace_rows, "creo placed carrier namespace rows")?
            .filter(|row| row_counts.get(&row.id) == Some(&1))
        {
            if let Some(carrier) =
                positional_cylinder_carrier(ctx, scan, row, parameters, ir, source_carriers)?
            {
                ctx.insert_btree_map(&mut carriers, row.id, carrier, "creo placed carrier nodes")?;
                continue;
            }
            let mut model_surface = None;
            let mut duplicate_model_surface = false;
            for candidate in ctx.admit_iter(
                &ir.model.surfaces,
                "creo placed carrier model surface search",
            )? {
                if matches_native_surface_id(ctx, scan, row.id, &candidate.id)? {
                    if model_surface.is_some() {
                        duplicate_model_surface = true;
                        break;
                    }
                    model_surface = Some(candidate);
                }
            }
            let surface = match (model_surface, duplicate_model_surface) {
                (None, _) => continue,
                (Some(surface), false) => surface,
                (Some(_), true) => {
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
                    Some(CarrierEquation::Plane(existing)) => {
                        agreed_plane(ctx, &[*existing, plane])?
                    }
                    Some(_) => None,
                    None => Some(plane),
                };
                if let Some(plane) = agreed {
                    ctx.insert_btree_map(
                        &mut carriers,
                        row.id,
                        CarrierEquation::Plane(plane),
                        "creo placed carrier nodes",
                    )?;
                } else {
                    carriers.remove(&row.id);
                }
            } else if let Some(carrier) = surface_carrier(source_carriers.surface_geometry(surface))
            {
                ctx.insert_btree_map(&mut carriers, row.id, carrier, "creo placed carrier nodes")?;
            }
        }
    }
    for datum in ctx.admit_iter(
        &scan.planes.datum_cylinders,
        "creo placed carrier datum cylinders",
    )? {
        let mut model_surface = None;
        let mut duplicate_model_surface = false;
        for candidate in ctx.admit_iter(
            &ir.model.surfaces,
            "creo datum carrier model surface search",
        )? {
            if matches_native_surface_id(ctx, scan, datum.id, &candidate.id)? {
                if model_surface.is_some() {
                    duplicate_model_surface = true;
                    break;
                }
                model_surface = Some(candidate);
            }
        }
        let Some(surface) = model_surface.filter(|_| !duplicate_model_surface) else {
            carriers.remove(&datum.id);
            continue;
        };
        if let Some(carrier) = surface_carrier(source_carriers.surface_geometry(surface)) {
            ctx.insert_btree_map(
                &mut carriers,
                datum.id,
                carrier,
                "creo placed carrier nodes",
            )?;
        } else {
            carriers.remove(&datum.id);
        }
    }
    let mut model_surfaces_by_id = BTreeMap::<u32, Vec<&Surface>>::new();
    for surface in ctx.admit_iter(&ir.model.surfaces, "creo rowless model surfaces")? {
        let Some(id) = surface
            .id
            .as_str()
            .strip_prefix("creo:visibgeom:surface#")
            .or_else(|| surface.id.as_str().strip_prefix("creo:novisgeom:surface#"))
            .and_then(|id| id.parse().ok())
        else {
            continue;
        };
        let surfaces = ctx
            .entry_btree_map(&mut model_surfaces_by_id, id, "creo rowless carrier groups")?
            .or_default();
        ctx.reserve_vec(surfaces, 1, "creo rowless carrier members")?;
        surfaces.push(surface);
    }
    for (id, model_surfaces) in
        ctx.admit_iter(&model_surfaces_by_id, "creo rowless carrier groups")?
    {
        if row_ids.contains(id) {
            continue;
        }
        let Some(surface) = exactly_one(ctx.admit_iter(
            model_surfaces,
            "creo rowless carrier model surface selection",
        )?) else {
            carriers.remove(id);
            continue;
        };
        if let Some(carrier) = surface_carrier(source_carriers.surface_geometry(surface)) {
            ctx.insert_btree_map(&mut carriers, *id, carrier, "creo placed carrier nodes")?;
        }
    }
    Ok(carriers)
}

fn positional_cylinder_carrier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    row: &crate::surface::SurfaceRow,
    parameters: &crate::surface::SurfaceParameters,
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
    if crate::decode::sketch_transfer::recipe::feature_schema_class(ctx, scan, row.feature_id)?
        == Some(SchemaClass::Round)
        && !inline
    {
        return Ok(None);
    }
    if crate::decode::sketch_transfer::recipe::feature_schema_class(ctx, scan, row.feature_id)?
        == Some(SchemaClass::Round)
        && inline
    {
        let mut model_surface = None;
        let mut duplicate_model_surface = false;
        for candidate in ctx.admit_iter(
            &ir.model.surfaces,
            "creo placed carrier inline surface search",
        )? {
            if matches_native_surface_id(ctx, scan, row.id, &candidate.id)? {
                if model_surface.is_some() {
                    duplicate_model_surface = true;
                    break;
                }
                model_surface = Some(candidate);
            }
        }
        if let Some(surface) = model_surface.filter(|_| !duplicate_model_surface) {
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    offset: usize,
) -> Result<Option<UnknownId>, cadmpeg_core::CodecError> {
    let Some(section) = ctx
        .admit_iter(&scan.framing.sections, "creo geometry section lookup")?
        .filter(|section| section.role() == SectionRole::PsbGeometry)
        .find(|section| section.contains(offset))
    else {
        return Ok(None);
    };
    let Some(namespace) = crate::identity::section_namespace(section.name()) else {
        return Ok(None);
    };
    crate::identity::compose_checked(
        ctx,
        &namespace,
        section.offset(),
        "creo geometry section record identity",
    )
    .map(Some)
}

#[cfg(test)]
mod tests;

fn projected_loop_polygon(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lp: &crate::topology::Loop,
    plane: PlaneEquation,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Result<Option<Vec<[f64; 2]>>, cadmpeg_core::CodecError> {
    let Some(dropped_axis) = (0..3).max_by(|left, right| {
        plane.normal[*left]
            .abs()
            .total_cmp(&plane.normal[*right].abs())
    }) else {
        return Ok(None);
    };
    let mut polygon = Vec::new();
    for half_edge in ctx.admit_iter(lp.half_edges(), "creo projected loop half edges")? {
        let Some(point) = incidence
            .get(half_edge)
            .and_then(|binding| solved_vertices.get(&binding.start_vertex_id.get()))
        else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut polygon, 1, "creo projected loop polygon points")?;
        polygon.push(match dropped_axis {
            0 => [point[1], point[2]],
            1 => [point[0], point[2]],
            _ => [point[0], point[1]],
        });
    }
    Ok(valid_parameter_polygon(ctx, &polygon)?.then_some(polygon))
}

fn polygon_strictly_contains(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    polygon: &[[f64; 2]],
    point: [f64; 2],
) -> Result<bool, cadmpeg_core::CodecError> {
    use cadmpeg_ir::math::{
        planar::{orientation, point_segment_distance},
        Point2,
    };
    use cadmpeg_ir::units::FinitePoint2;
    use std::cmp::Ordering;
    if polygon.len() < 3 {
        return Ok(false);
    }
    let point = Point2::new(point[0], point[1]);
    let finite_point = FinitePoint2::new(point);
    let mut inside = false;
    for (index, first) in ctx
        .admit_iter(polygon, "creo polygon containment points")?
        .enumerate()
    {
        let first = *first;
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
            return Ok(false);
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
    Ok(inside)
}

fn segments_intersect(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    first: [[f64; 2]; 2],
    second: [[f64; 2]; 2],
) -> Result<bool, cadmpeg_core::CodecError> {
    use cadmpeg_ir::math::Point2;
    let points = [first[0], first[1], second[0], second[1]];
    let mut scale = 0.0_f64;
    for (first_index, first) in ctx
        .admit_iter(
            &points[..points.len() - 1],
            "creo polygon segment distance first points",
        )?
        .enumerate()
    {
        for second in ctx.admit_iter(
            &points[first_index + 1..],
            "creo polygon segment distance second points",
        )? {
            scale = scale.max((first[0] - second[0]).hypot(first[1] - second[1]));
        }
    }
    let [a, b, c, d] = points.map(|point| Point2::new(point[0], point[1]));
    Ok(cadmpeg_ir::math::planar::segments_intersect(
        a,
        b,
        c,
        d,
        EPS_AGREE * scale,
    ))
}

fn polygon_strictly_contains_polygon(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    outer: &[[f64; 2]],
    inner: &[[f64; 2]],
) -> Result<bool, cadmpeg_core::CodecError> {
    for point in ctx.admit_iter(inner, "creo contained polygon vertices")? {
        if !polygon_strictly_contains(ctx, outer, *point)? {
            return Ok(false);
        }
    }
    for (index, first) in ctx
        .admit_iter(inner, "creo contained polygon edges")?
        .enumerate()
    {
        let inner_edge = [*first, inner[(index + 1) % inner.len()]];
        for (outer_index, first) in ctx
            .admit_iter(outer, "creo containing polygon edges")?
            .enumerate()
        {
            let outer_edge = [*first, outer[(outer_index + 1) % outer.len()]];
            if segments_intersect(ctx, inner_edge, outer_edge)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn valid_parameter_polygon(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    polygon: &[[f64; 2]],
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(origin) = polygon.first() else {
        return Ok(false);
    };
    if polygon.len() < 3 {
        return Ok(false);
    }
    let mut has_non_finite_coordinate = false;
    'points: for point in
        ctx.admit_iter(polygon, "creo parameter polygon finite-coordinate scan")?
    {
        for value in ctx.admit_iter(point, "creo parameter polygon point finite-coordinate scan")? {
            if !value.is_finite() {
                has_non_finite_coordinate = true;
                break 'points;
            }
        }
    }
    if has_non_finite_coordinate {
        return Ok(false);
    }
    let mut scale = 0.0_f64;
    for point in ctx.admit_iter(polygon, "creo parameter polygon scale")? {
        for (axis, value) in ctx
            .admit_iter(point, "creo parameter polygon point scale")?
            .enumerate()
        {
            scale = scale.max((value - origin[axis]).abs());
        }
    }
    if scale == 0.0 || !scale.is_finite() {
        return Ok(false);
    }
    let mut local = Vec::new();
    for point in ctx.admit_iter(polygon, "creo parameter polygon normalization")? {
        ctx.reserve_vec(&mut local, 1, "creo normalized polygon points")?;
        local.push(cadmpeg_ir::math::Point2::new(
            (point[0] - origin[0]) / scale,
            (point[1] - origin[1]) / scale,
        ));
    }
    Ok(cadmpeg_ir::math::planar::polygon_area_twice(&local)
        .is_some_and(|area| area.get().abs() > EPS_NEAR_ZERO))
}

fn ordered_contained_face_loops<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    mut loops: Vec<&'a crate::topology::Loop>,
    polygons: &[Vec<[f64; 2]>],
) -> Result<Option<Vec<&'a crate::topology::Loop>>, cadmpeg_core::CodecError> {
    if loops.len() < 2 || loops.len() != polygons.len() {
        return Ok(None);
    }
    for polygon in ctx.admit_iter(polygons, "creo face parameter polygons")? {
        if !valid_parameter_polygon(ctx, polygon)? {
            return Ok(None);
        }
    }
    let mut outer = None;
    for (candidate, polygon) in ctx
        .admit_iter(polygons, "creo outer face parameter polygon candidates")?
        .enumerate()
    {
        let mut contains_all = true;
        for (index, inner) in ctx
            .admit_iter(polygons, "creo nested face parameter polygon candidates")?
            .enumerate()
        {
            if index != candidate && !polygon_strictly_contains_polygon(ctx, polygon, inner)? {
                contains_all = false;
                break;
            }
        }
        if contains_all && outer.replace(candidate).is_some() {
            return Ok(None);
        }
    }
    let Some(outer) = outer else {
        return Ok(None);
    };
    let remove_operation = "creo ordered face loop removal shift";
    let remove_shift_count = loops
        .len()
        .checked_sub(outer)
        .and_then(|length| length.checked_sub(1))
        .ok_or_else(|| ctx.refuse_codec_limit(remove_operation, u64::MAX, u64::MAX))?;
    let remove_shift_bytes = cadmpeg_core::decode::u64_from_index(remove_shift_count)
        .checked_mul(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            &'a crate::topology::Loop,
        >()))
        .ok_or_else(|| ctx.refuse_codec_limit(remove_operation, u64::MAX, u64::MAX))?;
    ctx.charge_work(remove_shift_bytes, remove_operation)?;
    let selected = loops.remove(outer);
    let insert_operation = "creo ordered face loop insertion shift";
    let insert_shift_bytes = cadmpeg_core::decode::u64_from_index(loops.len())
        .checked_mul(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            &'a crate::topology::Loop,
        >()))
        .ok_or_else(|| ctx.refuse_codec_limit(insert_operation, u64::MAX, u64::MAX))?;
    ctx.charge_work(insert_shift_bytes, insert_operation)?;
    loops.insert(0, selected);
    Ok(Some(loops))
}

pub(in crate::decode) fn ordered_planar_face_loops<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    loops: Vec<&'a crate::topology::Loop>,
    plane: PlaneEquation,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Result<Option<Vec<&'a crate::topology::Loop>>, cadmpeg_core::CodecError> {
    if loops.len() == 1 {
        return Ok(Some(loops));
    }
    let mut polygons = Vec::new();
    for lp in ctx.admit_iter(&loops, "creo ordered planar face loops loops traversal")? {
        let Some(polygon) = projected_loop_polygon(ctx, lp, plane, incidence, solved_vertices)?
        else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut polygons, 1, "creo projected loop polygons")?;
        polygons.push(polygon);
    }
    ordered_contained_face_loops(ctx, loops, &polygons)
}

pub(in crate::decode) fn ordered_parameter_face_loops<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    loops: Vec<&'a crate::topology::Loop>,
    polygons: &[Vec<[f64; 2]>],
) -> Result<Option<Vec<&'a crate::topology::Loop>>, cadmpeg_core::CodecError> {
    if loops.len() == 1 {
        return Ok(Some(loops));
    }
    ordered_contained_face_loops(ctx, loops, polygons)
}

fn face_boundary_plane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    loops: &[&crate::topology::Loop],
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Result<Option<PlaneEquation>, cadmpeg_core::CodecError> {
    topology_bound_plane(ctx, |points| {
        for lp in ctx.admit_iter(loops, "creo face boundary loops")? {
            for half_edge in ctx.admit_iter(lp.half_edges(), "creo face boundary half edges")? {
                let Some(binding) = incidence.get(half_edge) else {
                    continue;
                };
                let Some(point) = solved_vertices.get(&binding.start_vertex_id.get()).copied()
                else {
                    continue;
                };
                ctx.reserve_vec(points, 1, "creo topology plane candidate points")?;
                points.push(point);
            }
        }
        Ok(())
    })
}

pub(in crate::decode) fn ordered_face_loops<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    loops: &[&'a crate::topology::Loop],
    plane: Option<PlaneEquation>,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Result<Option<Vec<&'a crate::topology::Loop>>, cadmpeg_core::CodecError> {
    let mut ordered_input = Vec::new();
    ctx.reserve_vec(
        &mut ordered_input,
        loops.len(),
        "creo native face ordering loop references",
    )?;
    ordered_input.extend_from_slice(loops);
    let plane = match plane {
        Some(plane) => Some(plane),
        None => face_boundary_plane(ctx, &ordered_input, incidence, solved_vertices)?,
    };
    if let Some(plane) = plane {
        ordered_planar_face_loops(ctx, ordered_input, plane, incidence, solved_vertices)
    } else {
        Ok((ordered_input.len() == 1).then_some(ordered_input))
    }
}

pub(in crate::decode) fn rowless_round_face_orientations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    round_feature_ids: &BTreeSet<u32>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &crate::surface::SurfaceRows,
    available_surfaces: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, bool>, cadmpeg_core::CodecError> {
    let mut orientations = BTreeMap::new();
    let rowless_pairs = rowless_round_cylinder_pairs(ctx, round_feature_ids, tables, rows)?;
    for (rowless_id, sibling_id, _) in
        ctx.admit_iter(&rowless_pairs, "creo rowless round cylinder pairs")?
    {
        if !available_surfaces.contains(rowless_id) {
            continue;
        }
        let Some(reversed) =
            crate::surface::unique_surface_row(rows, *sibling_id).map(|row| row.reversed)
        else {
            continue;
        };
        ctx.insert_btree_map(
            &mut orientations,
            *rowless_id,
            reversed,
            "creo rowless face orientation nodes",
        )?;
    }
    Ok(orientations)
}

pub(in crate::decode) fn native_face_orientations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
) -> Result<BTreeMap<u32, bool>, cadmpeg_core::CodecError> {
    let mut source_ids = BTreeSet::new();
    for row in ctx
        .admit_iter(&*scan.surfaces.rows, "creo native face visible rows")?
        .chain(ctx.admit_iter(
            &*scan.surfaces.nonvisible_rows,
            "creo native face nonvisible rows",
        )?)
    {
        ctx.insert_btree_set(&mut source_ids, row.id, "creo native face source ID nodes")?;
    }
    let mut orientations = BTreeMap::new();
    for id in ctx.admit_iter(&source_ids, "creo native face source IDs")? {
        if let Some(row) = crate::decode::surfaces::unique_native_surface_row(scan, *id) {
            ctx.insert_btree_map(
                &mut orientations,
                *id,
                row.reversed,
                "creo native face orientation nodes",
            )?;
        }
    }
    for datum in ctx.admit_iter(
        &scan.planes.datum_cylinders,
        "creo native face datum cylinders",
    )? {
        ctx.insert_btree_map(
            &mut orientations,
            datum.id,
            datum.reversed,
            "creo native face orientation nodes",
        )?;
    }
    let mut round_feature_ids = BTreeSet::new();
    for row in ctx
        .admit_iter(&scan.features.rows, "creo native face feature rows")?
        .filter(|row| row.root_schema_class == Some(SchemaClass::Round))
    {
        ctx.insert_btree_set(
            &mut round_feature_ids,
            row.feature_id,
            "creo native round feature ID nodes",
        )?;
    }
    let mut available_surfaces = BTreeSet::new();
    for surface in ctx.admit_iter(&ir.model.surfaces, "creo native face model surfaces")? {
        if let Some(id) = surface
            .id
            .as_str()
            .strip_prefix("creo:visibgeom:surface#")
            .and_then(|suffix| suffix.parse::<u32>().ok())
        {
            ctx.insert_btree_set(
                &mut available_surfaces,
                id,
                "creo available surface ID nodes",
            )?;
        }
    }
    let rowless_orientations = rowless_round_face_orientations(
        ctx,
        &round_feature_ids,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
        &available_surfaces,
    )?;
    for (id, reversed) in ctx.admit_iter(
        &rowless_orientations,
        "creo rowless round face orientations",
    )? {
        ctx.insert_btree_map(
            &mut orientations,
            *id,
            *reversed,
            "creo native face orientation nodes",
        )?;
    }
    Ok(orientations)
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

        let orientations = crate::decode::with_test_decode_ctx(|ctx| {
            native_face_orientations(ctx, &scan, &CadIr::empty())
        })
        .expect("service native orientations admitted");

        assert_eq!(orientations.get(&17), Some(&true));
    }
}
