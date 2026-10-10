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
use super::model_index::{CurveIndex, ModelPosition, SurfaceIndex};

use super::equations::{
    CarrierEquation, ConeEquation, CylinderEquation, PlaneEquation, SphereEquation, TorusEquation,
};
use super::planes::{
    agreed_plane_pair, agreed_topology_bound_plane, analytic_boundary_line, analytic_curve_plane,
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
                agreed_plane_pair(
                    PlaneEquation {
                        origin: [origin.x, origin.y, origin.z],
                        normal: [normal.x, normal.y, normal.z],
                    },
                    topology,
                )
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
    let mut scratch = ctx.reserve_scoped(0, "creo transfer topology bound planes scratch")?;
    let carriers = scratch.with_storage(|| placed_carriers(ctx, scan, ir, source_carriers))?;
    let solved_vertices = scratch.with_storage(|| {
        solved_topological_vertices(
            ctx,
            scan,
            ir,
            &carriers,
            nurbs_endpoint_witnesses,
            source_carriers,
        )
    })?;
    let vertex_faces = scratch.with_storage(|| {
        crate::topology::vertex_incident_faces(
            ctx,
            &scan.topology.vertices,
            &scan.topology.half_edges,
        )
    })?;
    let mut unique_curve_ids = BTreeSet::new();
    let unique_curve_rows_parts = crate::identity::uniquely_identified_rows_checked(ctx, &scan.curves.topology_rows, |row| {
            row.id
        })?;
    let _unique_curve_rows_storage = unique_curve_rows_parts.1;
    let unique_curve_rows = unique_curve_rows_parts.0;
    for row in ctx.admit_iter(&unique_curve_rows, "creo topology-bound unique curve rows")? {
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut unique_curve_ids,
                row.id,
                "creo topology-bound unique curve IDs",
            )
        })?;
    }
    let curve_index = CurveIndex::new(ctx, &ir.model.curves)?;
    let surface_index = SurfaceIndex::new(ctx, &ir.model.surfaces)?;
    let mut transferred = 0;
    for row in ctx
        .admit_iter(
            &*scan.surfaces.rows,
            "creo topology-bound unique surface rows",
        )?
        .filter(|row| scan.surfaces.rows.unique(row.id).is_some())
    {
        if row.kind != crate::surface::SurfaceKind::Plane {
            continue;
        }
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            row.id,
            "creo decoded model identity",
        )?;
        let mut face_storage = ctx.reserve_scoped(0, "creo topology face scratch")?;
        let points = face_storage.with_storage(|| {
            topology_bound_face_points(ctx, &solved_vertices, &vertex_faces, row.id)
        })?;
        let mut boundary_curves = Vec::new();
        let face_id = std::num::NonZeroU32::new(row.id);
        for lp in ctx.admit_iter(&scan.topology.loops, "creo topology-bound face loops")? {
            if lp.face_id() != face_id {
                continue;
            }
            for half_edge in
                ctx.admit_iter(lp.half_edges(), "creo topology-bound face half edges")?
            {
                if ctx.contains_btree_set(
                    &unique_curve_ids,
                    &half_edge.curve_id,
                    "creo analytic unique curve ids lookup",
                )? {
                    let matching_curve = curve_index
                        .unique(half_edge.curve_id)
                        .map(|position| &ir.model.curves[position]);
                    if let Some(curve) = matching_curve {
                        face_storage.with_storage(|| {
                            ctx.reserve_vec(
                                &mut boundary_curves,
                                1,
                                "creo topology-bound boundary curves",
                            )
                        })?;
                        boundary_curves.push(source_carriers.curve_geometry(curve)?);
                    }
                }
            }
        }
        let mut curve_planes = Vec::new();
        for geometry in ctx.admit_iter(&boundary_curves, "creo topology-bound boundary curves")? {
            if let Some(plane) = analytic_curve_plane(ctx, geometry)? {
                face_storage.with_storage(|| {
                    ctx.reserve_vec(&mut curve_planes, 1, "creo topology-bound curve planes")
                })?;
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
        if let Some(existing) = surface_index.entry(0, row.id) {
            let conflict = match existing {
                ModelPosition::Unique(position) => {
                    existing_plane_agrees_with_topology(
                        source_carriers.surface_geometry(&ir.model.surfaces[position])?,
                        plane,
                    ) == Some(false)
                }
                ModelPosition::Duplicate => true,
            };
            if !conflict {
                continue;
            }
            source_carriers.remove_surface(&id)?;
            for surface in ctx.admit_iter(
                &mut ir.model.surfaces,
                "creo topology-bound mutable surfaces",
            )? {
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
                if matches!(existing, ModelPosition::Unique(_)) {
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
        let incident = match incident_faces {
            Some(faces) => ctx.contains_btree_set(
                faces,
                &face_id,
                "creo topology-bound incident face membership",
            )?,
            None => false,
        };
        if incident {
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
    let surface_index = SurfaceIndex::new(ctx, &ir.model.surfaces)?;
    let curve_index = CurveIndex::new(ctx, &ir.model.curves)?;

    for (rows, namespace) in [
        (&scan.surfaces.rows, LegacySurfaceNamespace::Visible),
        (
            &scan.surfaces.nonvisible_rows,
            LegacySurfaceNamespace::NonVisible,
        ),
    ] {
        let mut traversal = rows.iter();
        while traversal.len() != 0 {
            let Some(row) = ctx.next_charged(&mut traversal, "creo unresolved surface rows")? else {
                break;
            };
            if rows.unique(row.id).is_none() {
                continue;
            }
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
            let namespace_index = usize::from(!namespace.is_visible());
            let surface_exists = surface_index.entry(namespace_index, row.id).is_some();
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
    let unique_curve_rows_parts = crate::identity::uniquely_identified_rows_checked(ctx, &scan.curves.topology_rows, |row| {
            row.id
        })?;
    let _unique_curve_rows_storage = unique_curve_rows_parts.1;
    let unique_curve_rows = unique_curve_rows_parts.0;
    let mut traversal = unique_curve_rows.iter();
    while traversal.len() != 0 {
        let Some(row) = ctx.next_charged(&mut traversal, "creo unresolved curve rows")? else {
            break;
        };
        let id = crate::identity::compose_checked::<CurveId>(
            ctx,
            &crate::identity::VISIBGEOM_CURVE,
            row.id,
            "creo decoded model identity",
        )?;
        let curve_exists = curve_index.contains(row.id);
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
    let mut scratch = ctx.reserve_scoped(0, "creo placed carriers scratch")?;
    let mut carriers = BTreeMap::new();
    let placed_planes = scratch.with_storage(|| placed_planes(ctx, scan))?;
    for (id, plane) in ctx.admit_iter(&placed_planes, "creo placed planes")? {
        ctx.insert_btree_map(
            &mut carriers,
            *id,
            CarrierEquation::Plane(*plane),
            "creo placed carrier nodes",
        )?;
    }
    let surface_index = SurfaceIndex::new(ctx, &ir.model.surfaces)?;
    for (namespace_rows, parameters) in [
        (&scan.surfaces.rows, &scan.surfaces.parameters),
        (
            &scan.surfaces.nonvisible_rows,
            &scan.surfaces.nonvisible_parameters,
        ),
    ] {
        let mut traversal = namespace_rows.iter();
        while traversal.len() != 0 {
            let Some(row) =
                ctx.next_charged(&mut traversal, "creo placed carrier namespace rows")? else {
                break;
            };
            if crate::decode::surfaces::unique_native_surface_row(scan, row.id).is_none() {
                continue;
            }
            let namespace = usize::from(!scan.surfaces.rows.contains_id(row.id));
            let model_surface = surface_index
                .unique(namespace, row.id)
                .map(|index| &ir.model.surfaces[index]);
            if let Some(carrier) = positional_cylinder_carrier(
                ctx,
                scan,
                row,
                parameters,
                model_surface,
                source_carriers,
            )? {
                ctx.insert_btree_map(&mut carriers, row.id, carrier, "creo placed carrier nodes")?;
                continue;
            }
            let surface = match surface_index.entry(namespace, row.id) {
                None => continue,
                Some(ModelPosition::Duplicate) => {
                    ctx.remove_btree_map(&mut carriers, &row.id, "creo analytic carriers lookup")?;
                    continue;
                }
                Some(ModelPosition::Unique(index)) => &ir.model.surfaces[index],
            };
            if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
                source_carriers.surface_geometry(surface)?
            {
                let origin = plane_surface.origin().get();
                let normal = plane_surface.frame().axis().as_raw();
                let plane = PlaneEquation {
                    origin: [origin.x, origin.y, origin.z],
                    normal: [normal.x, normal.y, normal.z],
                };
                let agreed =
                    match ctx.get_btree_map(&carriers, &row.id, "creo analytic carriers lookup")? {
                        Some(CarrierEquation::Plane(existing)) => {
                            agreed_plane_pair(*existing, plane)
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
                    ctx.remove_btree_map(&mut carriers, &row.id, "creo analytic carriers lookup")?;
                }
            } else if let Some(carrier) = surface_carrier(source_carriers.surface_geometry(surface)?)
            {
                ctx.insert_btree_map(&mut carriers, row.id, carrier, "creo placed carrier nodes")?;
            }
        }
    }
    let mut traversal = scan.planes.datum_cylinders.iter();
    while traversal.len() != 0 {
        let Some(datum) =
            ctx.next_charged(&mut traversal, "creo placed carrier datum cylinders")? else {
            break;
        };
        let namespace = if scan.surfaces.rows.contains_id(datum.id) {
            0
        } else if scan.surfaces.nonvisible_rows.contains_id(datum.id) {
            1
        } else {
            2
        };
        let Some(surface) = surface_index
            .unique(namespace, datum.id)
            .map(|index| &ir.model.surfaces[index])
        else {
            ctx.remove_btree_map(&mut carriers, &datum.id, "creo analytic carriers lookup")?;
            continue;
        };
        if let Some(carrier) = surface_carrier(source_carriers.surface_geometry(surface)?) {
            ctx.insert_btree_map(
                &mut carriers,
                datum.id,
                carrier,
                "creo placed carrier nodes",
            )?;
        } else {
            ctx.remove_btree_map(&mut carriers, &datum.id, "creo analytic carriers lookup")?;
        }
    }
    let mut model_surfaces_by_id = BTreeMap::<u32, Vec<&Surface>>::new();
    for surface in ctx.admit_iter(&ir.model.surfaces, "creo rowless model surfaces")? {
        let suffix = surface
            .id
            .as_str()
            .strip_prefix("creo:visibgeom:surface#")
            .or_else(|| surface.id.as_str().strip_prefix("creo:novisgeom:surface#"));
        let Some(suffix) = suffix else {
            continue;
        };
        let Ok(id) = ctx.parse_text::<u32>(suffix, "creo rowless surface identity suffix")? else {
            continue;
        };
        let surfaces = scratch
            .with_storage(|| {
                ctx.entry_btree_map(&mut model_surfaces_by_id, id, "creo rowless carrier groups")
            })?
            .or_default();
        scratch.with_storage(|| ctx.reserve_vec(surfaces, 1, "creo rowless carrier members"))?;
        surfaces.push(surface);
    }
    for (id, model_surfaces) in
        ctx.admit_iter(&model_surfaces_by_id, "creo rowless carrier groups")?
    {
        if scan.surfaces.rows.contains_id(*id) || scan.surfaces.nonvisible_rows.contains_id(*id) {
            continue;
        }
        let [surface] = model_surfaces.as_slice() else {
            ctx.remove_btree_map(&mut carriers, id, "creo analytic carriers lookup")?;
            continue;
        };
        if let Some(carrier) = surface_carrier(source_carriers.surface_geometry(surface)?) {
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
    model_surface: Option<&Surface>,
    source_carriers: &SourceUnitCarriers,
) -> Result<Option<CarrierEquation>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if row.kind != crate::surface::SurfaceKind::Cylinder {
        return Ok(None);
    }
    let Some(record) = crate::surface::unique_surface_parameter(parameters, row.id) else {
        return Ok(None);
    };
    let inline = record.has_inline_non_plane_envelope_checked(ctx)?
        || record.has_inline_non_plane_local_system_suffix(ctx)?
        || record.selector_corner_interval_cylinder_frame().is_some();
    let round_feature =
        crate::decode::sketch_transfer::recipe::feature_schema_class(ctx, scan, row.feature_id)?
            == Some(SchemaClass::Round);
    if round_feature && !inline {
        return Ok(None);
    }
    if round_feature && inline {
        if let Some(surface) = model_surface {
            if let Some(carrier) = surface_carrier(source_carriers.surface_geometry(surface)?) {
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
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut sections = scan.framing.sections.iter();
    while sections.len() != 0 {
        let Some(section) = ctx.next_charged(&mut sections, "creo geometry section lookup")? else {
            break;
        };
        if section.role() != SectionRole::PsbGeometry || !section.contains(offset) {
            continue;
        }
        let Some(namespace) = crate::identity::section_namespace(section.name()) else {
            return Ok(None);
        };
        return crate::identity::compose_checked(
            ctx,
            &namespace,
            section.offset(),
            "creo geometry section record identity",
        ).map(Some);
    }
    Ok(None)
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
    let mut half_edges = lp.half_edges().iter();
    while half_edges.len() != 0 {
        let Some(half_edge) =
            ctx.next_charged(&mut half_edges, "creo projected loop half edges")? else {
            break;
        };
        let Some(binding) =
            ctx.get_btree_map(incidence, half_edge, "creo projected loop incidence lookup")?
        else {
            return Ok(None);
        };
        let Some(point) = ctx.get_btree_map(
            solved_vertices,
            &binding.start_vertex_id.get(),
            "creo projected loop vertex lookup",
        )?
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
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if polygon.len() < 3 {
        return Ok(false);
    }
    let point = Point2::new(point[0], point[1]);
    let finite_point = FinitePoint2::new(point);
    let mut inside = false;
    let mut traversal = (polygon).iter().enumerate();
    while traversal.len() != 0 {
        let Some((index, first)) =
            ctx.next_charged(&mut traversal, "creo polygon containment points")? else {
            break;
        };
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

fn segments_intersect(first: [[f64; 2]; 2], second: [[f64; 2]; 2]) -> bool {
    use cadmpeg_ir::math::Point2;
    let points = [first[0], first[1], second[0], second[1]];
    let mut scale = 0.0_f64;
    for (first_index, first) in points[..points.len() - 1].iter().enumerate() {
        for second in &points[first_index + 1..] {
            scale = scale.max((first[0] - second[0]).hypot(first[1] - second[1]));
        }
    }
    let [a, b, c, d] = points.map(|point| Point2::new(point[0], point[1]));
    cadmpeg_ir::math::planar::segments_intersect(a, b, c, d, EPS_AGREE * scale)
}

fn polygon_strictly_contains_polygon(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    outer: &[[f64; 2]],
    inner: &[[f64; 2]],
) -> Result<bool, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut traversal = (inner).iter();
    while traversal.len() != 0 {
        let Some(point) = ctx.next_charged(&mut traversal, "creo contained polygon vertices")? else {
            break;
        };
        if !polygon_strictly_contains(ctx, outer, *point)? {
            return Ok(false);
        }
    }
    let mut traversal = (inner).iter().enumerate();
    while traversal.len() != 0 {
        let Some((index, first)) =
            ctx.next_charged(&mut traversal, "creo contained polygon edges")? else {
            break;
        };
        let inner_edge = [*first, inner[(index + 1) % inner.len()]];
        let mut traversal = (outer).iter().enumerate();
        while traversal.len() != 0 {
            let Some((outer_index, first)) =
                ctx.next_charged(&mut traversal, "creo containing polygon edges")? else {
                break;
            };
            let outer_edge = [*first, outer[(outer_index + 1) % outer.len()]];
            if segments_intersect(inner_edge, outer_edge) {
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
    let mut scratch = ctx.reserve_scoped(0, "creo valid parameter polygon scratch")?;
    scratch.with_storage(|| {
        let Some(origin) = polygon.first() else {
            return Ok(false);
        };
        if polygon.len() < 3 {
            return Ok(false);
        }
        let mut has_non_finite_coordinate = false;
        let mut traversal = (polygon).iter();
        'points: while traversal.len() != 0 {
            let Some(point) = ctx.next_charged(
                &mut traversal,
                "creo parameter polygon finite-coordinate scan",
            )? else {
                break;
            };
            for value in point {
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
            for (axis, value) in point.iter().enumerate() {
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
        // discarded-value: the math helper performs this pass; the iterator has no reader.
        let _ = ctx.admit_iter(&local, "creo polygon area finite points")?;
        // discarded-value: the math helper performs this pass; the iterator has no reader.
        let _ = ctx.admit_iter(&local, "creo polygon area edge products")?;
        Ok(cadmpeg_ir::math::planar::polygon_area_twice(&local)
            .is_some_and(|area| area.get().abs() > EPS_NEAR_ZERO))
    })
}

fn ordered_contained_face_loops<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    mut loops: Vec<&'a crate::topology::Loop>,
    polygons: &[Vec<[f64; 2]>],
) -> Result<Option<Vec<&'a crate::topology::Loop>>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if loops.len() < 2 || loops.len() != polygons.len() {
        return Ok(None);
    }
    let mut traversal = (polygons).iter();
    while traversal.len() != 0 {
        let Some(polygon) = ctx.next_charged(&mut traversal, "creo face parameter polygons")? else {
            break;
        };
        if !valid_parameter_polygon(ctx, polygon)? {
            return Ok(None);
        }
    }
    let mut outer = None;
    let mut traversal = (polygons).iter().enumerate();
    while traversal.len() != 0 {
        let Some((candidate, polygon)) = ctx.next_charged(
            &mut traversal,
            "creo outer face parameter polygon candidates",
        )? else {
            break;
        };
        let mut contains_all = true;
        let mut traversal = (polygons).iter().enumerate();
        while traversal.len() != 0 {
            let Some((index, inner)) = ctx.next_charged(
                &mut traversal,
                "creo nested face parameter polygon candidates",
            )? else {
                break;
            };
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
    if outer != 0 {
        ctx.rotate_right(&mut loops[..=outer], 1, "creo ordered face loop rotation")?;
    }
    Ok(Some(loops))
}

pub(in crate::decode) fn ordered_planar_face_loops<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    loops: Vec<&'a crate::topology::Loop>,
    plane: PlaneEquation,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Result<Option<Vec<&'a crate::topology::Loop>>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo ordered planar face loops scratch")?;
    if loops.len() == 1 {
        return Ok(Some(loops));
    }
    let mut polygons = Vec::new();
    let mut traversal = loops.iter();
    while traversal.len() != 0 {
        let Some(lp) = ctx.next_charged(
            &mut traversal,
            "creo ordered planar face loops loops traversal",
        )? else {
            break;
        };
        let Some(polygon) = scratch
            .with_storage(|| projected_loop_polygon(ctx, lp, plane, incidence, solved_vertices))?
        else {
            return Ok(None);
        };
        scratch
            .with_storage(|| ctx.reserve_vec(&mut polygons, 1, "creo projected loop polygons"))?;
        polygons.push(polygon);
    }
    ordered_contained_face_loops(ctx, loops, &polygons)
}

pub(in crate::decode) fn ordered_parameter_face_loops<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    loops: Vec<&'a crate::topology::Loop>,
    polygons: &[Vec<[f64; 2]>],
) -> Result<Option<Vec<&'a crate::topology::Loop>>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
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
                let Some(binding) =
                    ctx.get_btree_map(incidence, half_edge, "creo analytic incidence lookup")?
                else {
                    continue;
                };
                let Some(point) = ctx
                    .get_btree_map(
                        solved_vertices,
                        &binding.start_vertex_id.get(),
                        "creo analytic solved vertices lookup",
                    )?
                    .copied()
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
    let mut input_storage = ctx.reserve_scoped(0, "creo face ordering candidate references")?;
    let mut ordered_input = Vec::new();
    input_storage.with_storage(|| {
        ctx.extend_from_slice(
            &mut ordered_input,
            loops,
            "creo native face ordering loop references",
        )
    })?;
    let plane = match plane {
        Some(plane) => Some(plane),
        None => face_boundary_plane(ctx, &ordered_input, incidence, solved_vertices)?,
    };
    let ordered = if let Some(plane) = plane {
        ordered_planar_face_loops(ctx, ordered_input, plane, incidence, solved_vertices)?
    } else {
        (ordered_input.len() == 1).then_some(ordered_input)
    };
    if ordered.is_some() {
        return input_storage.commit_value(ordered);
    }
    Ok(ordered)
}

pub(in crate::decode) fn rowless_round_face_orientations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    round_feature_ids: &BTreeSet<u32>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &crate::surface::SurfaceRows,
    available_surfaces: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, bool>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo rowless round face orientations scratch")?;
    let mut orientations = BTreeMap::new();
    let rowless_pairs = scratch
        .with_storage(|| rowless_round_cylinder_pairs(ctx, round_feature_ids, tables, rows))?;
    for (rowless_id, sibling_id, _) in
        ctx.admit_iter(&rowless_pairs, "creo rowless round cylinder pairs")?
    {
        if !ctx.contains_btree_set(
            available_surfaces,
            rowless_id,
            "creo analytic available surfaces lookup",
        )? {
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
    let mut scratch = ctx.reserve_scoped(0, "creo native face orientations scratch")?;
    let mut orientations = BTreeMap::new();
    for source in ctx
        .admit_iter(&*scan.surfaces.rows, "creo native face visible rows")?
        .chain(ctx.admit_iter(
            &*scan.surfaces.nonvisible_rows,
            "creo native face nonvisible rows",
        )?)
    {
        if let Some(row) = crate::decode::surfaces::unique_native_surface_row(scan, source.id) {
            ctx.insert_btree_map(
                &mut orientations,
                source.id,
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
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut round_feature_ids,
                row.feature_id,
                "creo native round feature ID nodes",
            )
        })?;
    }
    let mut available_surfaces = BTreeSet::new();
    for surface in ctx.admit_iter(&ir.model.surfaces, "creo native face model surfaces")? {
        let Some(suffix) = surface.id.as_str().strip_prefix("creo:visibgeom:surface#") else {
            continue;
        };
        if let Ok(id) = ctx.parse_text::<u32>(suffix, "creo native face identity suffix")? {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut available_surfaces,
                    id,
                    "creo available surface ID nodes",
                )
            })?;
        }
    }
    let rowless_orientations = scratch.with_storage(|| {
        rowless_round_face_orientations(
            ctx,
            &round_feature_ids,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
            &available_surfaces,
        )
    })?;
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
