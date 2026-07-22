// SPDX-License-Identifier: Apache-2.0
//! Native Rhino 3DM archive writing.

use std::io::Write;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::SurfaceGeometry;
use sha2::{Digest, Sha256};

use crate::chunks::{MAGIC, TCODE_ENDOFFILE, TCODE_SHORT};

const TCODE_PROPERTIES_TABLE: u32 = 0x1000_0014;
const TCODE_SETTINGS_TABLE: u32 = 0x1000_0015;
const TCODE_BITMAP_TABLE: u32 = 0x1000_0016;
const TCODE_TEXTURE_MAPPING_TABLE: u32 = 0x1000_0025;
const TCODE_MATERIAL_TABLE: u32 = 0x1000_0010;
const TCODE_LINETYPE_TABLE: u32 = 0x1000_0023;
const TCODE_LAYER_TABLE: u32 = 0x1000_0011;
const TCODE_GROUP_TABLE: u32 = 0x1000_0018;
const TCODE_FONT_TABLE: u32 = 0x1000_0019;
const TCODE_DIMSTYLE_TABLE: u32 = 0x1000_0020;
const TCODE_LIGHT_TABLE: u32 = 0x1000_0012;
const TCODE_HATCH_PATTERN_TABLE: u32 = 0x1000_0022;
const TCODE_INSTANCE_DEFINITION_TABLE: u32 = 0x1000_0021;
const TCODE_OBJECT_TABLE: u32 = 0x1000_0013;
const TCODE_HISTORY_RECORD_TABLE: u32 = 0x1000_0026;
const TCODE_ENDOFTABLE: u32 = 0xffff_ffff;
const TCODE_PROPERTIES_OPENNURBS_VERSION: u32 = 0xa000_0026;
const TCODE_UNITS_AND_TOLERANCES: u32 = 0x2000_8031;
const TCODE_LAYER_RECORD: u32 = 0x2000_8050;
const TCODE_OBJECT_RECORD: u32 = 0x2000_8070;
const TCODE_OBJECT_RECORD_TYPE: u32 = 0x0200_0071;
const TCODE_OBJECT_RECORD_ATTRIBUTES: u32 = 0x0200_8072;
const TCODE_OBJECT_RECORD_END: u32 = 0x0200_007f;
const TCODE_CLASS_WRAPPER: u32 = 0x0002_7ffa;
const TCODE_CLASS_UUID: u32 = 0x0002_fffb;
const TCODE_CLASS_DATA: u32 = 0x0002_fffc;
const TCODE_CLASS_END: u32 = 0x0002_7fff;

const POINT_CLASS: [u8; 16] = [
    0x1d, 0x1a, 0x10, 0xc3, 0x57, 0xf1, 0xd3, 0x11, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const POINT_CLOUD_CLASS: [u8; 16] = [
    0x47, 0xf3, 0x88, 0x24, 0xfa, 0xf8, 0xd3, 0x11, 0xbf, 0xec, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const LINE_CLASS: [u8; 16] = [
    0xdb, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const BREP_CLASS: [u8; 16] = [
    0xc5, 0xdb, 0xb5, 0x60, 0x60, 0xe6, 0xd3, 0x11, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const ARC_CLASS: [u8; 16] = [
    0x2a, 0xbe, 0x33, 0xcf, 0xb4, 0x09, 0xd4, 0x11, 0xbf, 0xfb, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const NURBS_CURVE_CLASS: [u8; 16] = [
    0xdd, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const NURBS_SURFACE_CLASS: [u8; 16] = [
    0xde, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const PLANE_SURFACE_CLASS: [u8; 16] = [
    0xdf, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const MESH_CLASS: [u8; 16] = [
    0xe4, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const LAYER_CLASS: [u8; 16] = [
    0x13, 0x98, 0x80, 0x95, 0x85, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const CHANNEL_UV: u32 = 0x5248_0001;
const CHANNEL_COLOR: u32 = 0x5248_0002;
const CHANNEL_SURFACE_PARAMETERS: u32 = 0x5248_0003;
const CHANNEL_CURVATURE: u32 = 0x5248_0004;
const DEFAULT_RELATIVE_TOLERANCE: f64 = 0.01;

pub(crate) fn write(ir: &CadIr, version: u64, output: &mut dyn Write) -> Result<(), CodecError> {
    let plan = prepare_write(ir)?;
    let mut objects = plan
        .breps
        .iter()
        .map(|prepared| {
            let scope = &prepared.scope;
            brep_object_record(
                &prepared.payload,
                &scope.body.id.0,
                scope.body.name.as_deref(),
                scope.body.color,
                scope.body.visible,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    objects.extend(
        ir.model
            .points
            .iter()
            .filter(|point| !plan.topology_points.contains(&point.id.0))
            .map(|point| {
                let position = point.position;
                let mut payload = vec![0x10];
                payload.extend(position.x.to_le_bytes());
                payload.extend(position.y.to_le_bytes());
                payload.extend(position.z.to_le_bytes());
                attributed_object_record(1, POINT_CLASS, &payload, &point.id.0, None, None, None)
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    for group in plan.point_groups {
        if group.points.len() == 1 {
            let point = group.points[0];
            let mut payload = vec![0x10];
            payload.extend(point.x.to_le_bytes());
            payload.extend(point.y.to_le_bytes());
            payload.extend(point.z.to_le_bytes());
            objects.push(attributed_object_record(
                1,
                POINT_CLASS,
                &payload,
                &group.identity,
                group.name.as_deref(),
                group.color,
                group.visible,
            )?);
        } else {
            objects.push(attributed_object_record(
                2,
                POINT_CLOUD_CLASS,
                &point_cloud_payload(&group.points),
                &group.identity,
                group.name.as_deref(),
                group.color,
                group.visible,
            )?);
        }
    }
    for curve in &ir.model.curves {
        if plan
            .breps
            .iter()
            .any(|prepared| prepared.scope.curves.contains(&curve.id.0))
        {
            continue;
        }
        let (class, payload) = match &curve.geometry {
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => (
                ARC_CLASS,
                circle_payload(*center, *axis, *ref_direction, *radius),
            ),
            CurveGeometry::Nurbs(nurbs) => (NURBS_CURVE_CLASS, nurbs_curve_payload(nurbs)),
            _ => unreachable!("representability checked before serialization"),
        };
        objects.push(attributed_object_record(
            4,
            class,
            &payload,
            &curve.id.0,
            None,
            None,
            None,
        )?);
    }
    for surface in &ir.model.surfaces {
        if plan
            .breps
            .iter()
            .any(|prepared| prepared.scope.surfaces.contains(&surface.id.0))
        {
            continue;
        }
        let (class, payload) = match &surface.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => (
                PLANE_SURFACE_CLASS,
                plane_surface_payload(*origin, *normal, *u_axis),
            ),
            SurfaceGeometry::Nurbs(nurbs) => (NURBS_SURFACE_CLASS, nurbs_surface_payload(nurbs)),
            _ => unreachable!("representability checked before serialization"),
        };
        objects.push(attributed_object_record(
            8,
            class,
            &payload,
            &surface.id.0,
            None,
            None,
            None,
        )?);
    }
    for mesh in &ir.model.tessellations {
        let payload = mesh_payload(mesh, version);
        objects.push(mesh_object_record(&payload, &mesh.id)?);
    }

    write_archive(ir, version, &objects, output)
}

fn write_archive(
    ir: &CadIr,
    version: u64,
    objects: &[Vec<u8>],
    output: &mut dyn Write,
) -> Result<(), CodecError> {
    let mut bytes = header(version)?;
    bytes.extend(long_chunk(1, b"cadmpeg"));
    bytes.extend(table(
        TCODE_PROPERTIES_TABLE,
        &[short_chunk(TCODE_PROPERTIES_OPENNURBS_VERSION, 200_712_190)],
    ));
    bytes.extend(table(
        TCODE_SETTINGS_TABLE,
        &[units_record(ir.tolerances.linear, ir.tolerances.angular)],
    ));
    for typecode in [
        TCODE_BITMAP_TABLE,
        TCODE_TEXTURE_MAPPING_TABLE,
        TCODE_MATERIAL_TABLE,
        TCODE_LINETYPE_TABLE,
        TCODE_GROUP_TABLE,
        TCODE_FONT_TABLE,
        TCODE_DIMSTYLE_TABLE,
        TCODE_LIGHT_TABLE,
        TCODE_HATCH_PATTERN_TABLE,
        TCODE_INSTANCE_DEFINITION_TABLE,
    ] {
        bytes.extend(table(typecode, &[]));
        if typecode == TCODE_LINETYPE_TABLE {
            bytes.extend(table(
                TCODE_LAYER_TABLE,
                &[zero_crc_chunk(
                    TCODE_LAYER_RECORD,
                    &class_wrapper(LAYER_CLASS, &default_layer_payload()),
                )],
            ));
        }
    }
    bytes.extend(table(TCODE_OBJECT_TABLE, objects));
    bytes.extend(table(TCODE_HISTORY_RECORD_TABLE, &[]));
    let final_size = bytes
        .len()
        .checked_add(20)
        .ok_or_else(|| CodecError::Malformed("3DM output size overflow".into()))?;
    bytes.extend(long_chunk(
        TCODE_ENDOFFILE,
        &(final_size as u64).to_le_bytes(),
    ));
    output.write_all(&bytes)?;
    Ok(())
}

struct BrepScope {
    ir: CadIr,
    body: cadmpeg_ir::topology::Body,
    points: std::collections::BTreeSet<String>,
    surfaces: std::collections::BTreeSet<String>,
    curves: std::collections::BTreeSet<String>,
    pcurves: std::collections::BTreeSet<String>,
}

struct PreparedBrep {
    scope: BrepScope,
    payload: BrepPayload,
}

struct BrepPayload {
    body: Vec<u8>,
    direct: Vec<u8>,
}

struct WritePlan {
    breps: Vec<PreparedBrep>,
    topology_points: std::collections::BTreeSet<String>,
    point_groups: Vec<PointGroup>,
}

fn brep_scopes(ir: &CadIr) -> Result<Vec<BrepScope>, CodecError> {
    use cadmpeg_ir::topology::BodyKind;
    use std::collections::BTreeSet;

    let model = &ir.model;
    let mut scopes = Vec::new();
    let mut all_regions = BTreeSet::new();
    let mut all_shells = BTreeSet::new();
    let mut all_faces = BTreeSet::new();
    let mut all_loops = BTreeSet::new();
    let mut all_coedges = BTreeSet::new();
    let mut all_edges = BTreeSet::new();
    let mut all_vertices = BTreeSet::new();
    let mut all_points = BTreeSet::new();
    let mut all_surfaces = BTreeSet::new();
    let mut all_curves = BTreeSet::new();
    let mut all_pcurves = BTreeSet::new();

    for body in model
        .bodies
        .iter()
        .filter(|body| matches!(body.kind, BodyKind::Sheet | BodyKind::Solid))
    {
        let regions = body
            .regions
            .iter()
            .map(|id| id.0.clone())
            .collect::<BTreeSet<_>>();
        let shells = model
            .regions
            .iter()
            .filter(|region| regions.contains(&region.id.0))
            .flat_map(|region| region.shells.iter().map(|id| id.0.clone()))
            .collect::<BTreeSet<_>>();
        let faces = model
            .shells
            .iter()
            .filter(|shell| shells.contains(&shell.id.0))
            .flat_map(|shell| shell.faces.iter().map(|id| id.0.clone()))
            .collect::<BTreeSet<_>>();
        let surfaces = model
            .faces
            .iter()
            .filter(|face| faces.contains(&face.id.0))
            .map(|face| face.surface.0.clone())
            .collect::<BTreeSet<_>>();
        let loops = model
            .faces
            .iter()
            .filter(|face| faces.contains(&face.id.0))
            .flat_map(|face| face.loops.iter().map(|id| id.0.clone()))
            .collect::<BTreeSet<_>>();
        let coedges = model
            .loops
            .iter()
            .filter(|loop_| loops.contains(&loop_.id.0))
            .flat_map(|loop_| loop_.coedges.iter().map(|id| id.0.clone()))
            .collect::<BTreeSet<_>>();
        let edges = model
            .coedges
            .iter()
            .filter(|coedge| coedges.contains(&coedge.id.0))
            .map(|coedge| coedge.edge.0.clone())
            .collect::<BTreeSet<_>>();
        let pcurves = model
            .coedges
            .iter()
            .filter(|coedge| coedges.contains(&coedge.id.0))
            .filter_map(|coedge| {
                coedge
                    .pcurves
                    .first()
                    .map(|use_| &use_.pcurve)
                    .map(|id| id.0.clone())
            })
            .collect::<BTreeSet<_>>();
        let vertices = model
            .edges
            .iter()
            .filter(|edge| edges.contains(&edge.id.0))
            .flat_map(|edge| [edge.start.0.clone(), edge.end.0.clone()])
            .collect::<BTreeSet<_>>();
        let curves = model
            .edges
            .iter()
            .filter(|edge| edges.contains(&edge.id.0))
            .filter_map(|edge| edge.curve.as_ref().map(|id| id.0.clone()))
            .collect::<BTreeSet<_>>();
        let points = model
            .vertices
            .iter()
            .filter(|vertex| vertices.contains(&vertex.id.0))
            .map(|vertex| vertex.point.0.clone())
            .collect::<BTreeSet<_>>();

        for (owned, global, kind) in [
            (&regions, &mut all_regions, "region"),
            (&shells, &mut all_shells, "shell"),
            (&faces, &mut all_faces, "face"),
            (&loops, &mut all_loops, "loop"),
            (&coedges, &mut all_coedges, "coedge"),
            (&edges, &mut all_edges, "edge"),
            (&vertices, &mut all_vertices, "vertex"),
            (&points, &mut all_points, "point"),
            (&surfaces, &mut all_surfaces, "surface"),
            (&curves, &mut all_curves, "curve"),
            (&pcurves, &mut all_pcurves, "pcurve"),
        ] {
            if owned.iter().any(|id| !global.insert(id.clone())) {
                return Err(CodecError::NotImplemented(format!(
                    "{kind} carrier is shared by multiple Brep objects"
                )));
            }
        }

        let mut scoped = ir.clone();
        scoped
            .model
            .bodies
            .retain(|candidate| candidate.id == body.id);
        scoped
            .model
            .regions
            .retain(|entity| regions.contains(&entity.id.0));
        scoped
            .model
            .shells
            .retain(|entity| shells.contains(&entity.id.0));
        scoped
            .model
            .faces
            .retain(|entity| faces.contains(&entity.id.0));
        scoped
            .model
            .loops
            .retain(|entity| loops.contains(&entity.id.0));
        scoped
            .model
            .coedges
            .retain(|entity| coedges.contains(&entity.id.0));
        scoped
            .model
            .edges
            .retain(|entity| edges.contains(&entity.id.0));
        scoped
            .model
            .vertices
            .retain(|entity| vertices.contains(&entity.id.0));
        scoped
            .model
            .points
            .retain(|entity| points.contains(&entity.id.0));
        scoped
            .model
            .surfaces
            .retain(|entity| surfaces.contains(&entity.id.0));
        scoped
            .model
            .curves
            .retain(|entity| curves.contains(&entity.id.0));
        scoped
            .model
            .pcurves
            .retain(|entity| pcurves.contains(&entity.id.0));
        scoped.model.tessellations.clear();
        scopes.push(BrepScope {
            ir: scoped,
            body: body.clone(),
            points,
            surfaces,
            curves,
            pcurves,
        });
    }
    Ok(scopes)
}

fn general_topology_ir(ir: &CadIr) -> CadIr {
    use cadmpeg_ir::topology::BodyKind;
    use std::collections::BTreeSet;

    let mut scoped = ir.clone();
    scoped
        .model
        .bodies
        .retain(|body| body.kind == BodyKind::General);
    let bodies = scoped
        .model
        .bodies
        .iter()
        .map(|body| body.id.0.clone())
        .collect::<BTreeSet<_>>();
    scoped
        .model
        .regions
        .retain(|region| bodies.contains(&region.body.0));
    let regions = scoped
        .model
        .regions
        .iter()
        .map(|region| region.id.0.clone())
        .collect::<BTreeSet<_>>();
    scoped
        .model
        .shells
        .retain(|shell| regions.contains(&shell.region.0));
    let vertices = scoped
        .model
        .shells
        .iter()
        .flat_map(|shell| shell.free_vertices.iter().map(|id| id.0.clone()))
        .collect::<BTreeSet<_>>();
    scoped
        .model
        .vertices
        .retain(|vertex| vertices.contains(&vertex.id.0));
    scoped.model.faces.clear();
    scoped.model.loops.clear();
    scoped.model.coedges.clear();
    scoped.model.edges.clear();
    scoped
}

fn prepare_write(ir: &CadIr) -> Result<WritePlan, CodecError> {
    if !ir.tolerances.linear.is_finite() || ir.tolerances.linear <= 0.0 {
        return Err(CodecError::Malformed(
            "Rhino absolute tolerance must be positive and finite".into(),
        ));
    }
    if !ir.tolerances.angular.is_finite()
        || ir.tolerances.angular <= 0.0
        || ir.tolerances.angular > std::f64::consts::PI
    {
        return Err(CodecError::Malformed(
            "Rhino angular tolerance must be finite and in (0, pi]".into(),
        ));
    }
    if ir
        .native
        .namespace("rhino")
        .is_some_and(|namespace| !rewritable_generated_namespace(namespace))
    {
        return Err(CodecError::NotImplemented(
            "Rhino native records require explicit survival handling".into(),
        ));
    }
    let model = &ir.model;
    let unsupported = [
        ("subds", model.subds.len()),
        ("procedural_surfaces", model.procedural_surfaces.len()),
        ("procedural_curves", model.procedural_curves.len()),
        ("features", model.features.len()),
        ("configurations", model.configurations.len()),
        ("parameters", model.parameters.len()),
        ("sketches", model.sketches.len()),
        ("sketch_entities", model.sketch_entities.len()),
        ("sketch_constraints", model.sketch_constraints.len()),
        ("appearances", model.appearances.len()),
        ("appearance_bindings", model.appearance_bindings.len()),
        ("attributes", model.attributes.len()),
    ]
    .into_iter()
    .filter(|(_, count)| *count != 0)
    .map(|(name, _)| name)
    .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Err(CodecError::NotImplemented(format!(
            "Rhino writer cannot yet represent arenas: {}",
            unsupported.join(", ")
        )));
    }
    if model.points.len() > i32::MAX as usize
        || model.points.iter().any(|point| {
            !point.position.x.is_finite()
                || !point.position.y.is_finite()
                || !point.position.z.is_finite()
        })
    {
        return Err(CodecError::Malformed(
            "point arena exceeds native counts or contains non-finite coordinates".into(),
        ));
    }
    let breps = brep_scopes(ir)?;
    let prepared_breps = breps
        .into_iter()
        .map(|scope| {
            let payload = planar_sheet_brep_payload(&scope.ir)?.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "body {} topology is not a writable Brep",
                    scope.body.id.0
                ))
            })?;
            Ok(PreparedBrep { scope, payload })
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    let used_pcurves = prepared_breps
        .iter()
        .flat_map(|prepared| prepared.scope.pcurves.iter())
        .collect::<std::collections::BTreeSet<_>>();
    if used_pcurves.len() != model.pcurves.len() {
        return Err(CodecError::NotImplemented(
            "pcurves without writable Brep coedge ownership are not writable".into(),
        ));
    }
    let general = general_topology_ir(ir);
    let scoped_count = |select: fn(&cadmpeg_ir::document::Model) -> usize| {
        prepared_breps
            .iter()
            .map(|prepared| select(&prepared.scope.ir.model))
            .sum::<usize>()
            + select(&general.model)
    };
    if scoped_count(|model| model.bodies.len()) != model.bodies.len()
        || scoped_count(|model| model.regions.len()) != model.regions.len()
        || scoped_count(|model| model.shells.len()) != model.shells.len()
        || scoped_count(|model| model.faces.len()) != model.faces.len()
        || scoped_count(|model| model.loops.len()) != model.loops.len()
        || scoped_count(|model| model.coedges.len()) != model.coedges.len()
        || scoped_count(|model| model.edges.len()) != model.edges.len()
        || scoped_count(|model| model.vertices.len()) != model.vertices.len()
    {
        return Err(CodecError::NotImplemented(
            "orphan or unsupported topology is not writable".into(),
        ));
    }
    let (mut topology_points, point_groups) = free_vertex_groups(&general)?;
    for prepared in &prepared_breps {
        topology_points.extend(prepared.scope.points.iter().cloned());
    }
    for curve in &model.curves {
        if prepared_breps
            .iter()
            .any(|prepared| prepared.scope.curves.contains(&curve.id.0))
        {
            continue;
        }
        if curve.source_object.is_some() {
            return Err(CodecError::NotImplemented(format!(
                "curve {} source-object state is not writable",
                curve.id.0
            )));
        }
        let CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } = &curve.geometry
        else {
            if let CurveGeometry::Nurbs(nurbs) = &curve.geometry {
                check_nurbs_curve(&curve.id.0, nurbs)?;
                continue;
            }
            return Err(CodecError::NotImplemented(format!(
                "Rhino writer cannot represent curve {} as a native object",
                curve.id.0
            )));
        };
        let axis_norm = axis.norm();
        let reference_norm = ref_direction.norm();
        let dot = axis.x * ref_direction.x + axis.y * ref_direction.y + axis.z * ref_direction.z;
        if !center.x.is_finite()
            || !center.y.is_finite()
            || !center.z.is_finite()
            || !radius.is_finite()
            || *radius <= 0.0
            || !axis_norm.is_finite()
            || !reference_norm.is_finite()
            || (axis_norm - 1.0).abs() > 1.0e-10
            || (reference_norm - 1.0).abs() > 1.0e-10
            || dot.abs() > 1.0e-10
        {
            return Err(CodecError::Malformed(format!(
                "curve {} has an invalid circle frame",
                curve.id.0
            )));
        }
    }
    for surface in &model.surfaces {
        if prepared_breps
            .iter()
            .any(|prepared| prepared.scope.surfaces.contains(&surface.id.0))
        {
            continue;
        }
        if surface.source_object.is_some() {
            return Err(CodecError::NotImplemented(format!(
                "surface {} source-object state is not writable",
                surface.id.0
            )));
        }
        match &surface.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => {
                check_frame(&surface.id.0, *origin, *normal, *u_axis, "plane")?;
            }
            SurfaceGeometry::Nurbs(nurbs) => check_nurbs_surface(&surface.id.0, nurbs)?,
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "Rhino writer cannot represent surface {} as a native object",
                    surface.id.0
                )))
            }
        }
    }
    for mesh in &model.tessellations {
        check_mesh(mesh)?;
    }
    Ok(WritePlan {
        breps: prepared_breps,
        topology_points,
        point_groups,
    })
}

fn rewritable_generated_namespace(namespace: &cadmpeg_ir::NativeNamespace) -> bool {
    const REGENERATED: &[&str] = &[
        "byte_spans",
        "document_settings",
        "layers",
        "object_presentation",
        "opaque_records",
        "unknowns",
    ];
    if namespace.version != 2 {
        return false;
    }
    if namespace
        .arenas
        .iter()
        .any(|(name, records)| !records.is_empty() && !REGENERATED.contains(&name.as_str()))
    {
        return false;
    }
    let opaque = namespace.arenas.get("opaque_records");
    let generated_comment = opaque.is_some_and(|records| {
        records.iter().any(|record| {
            record.id == "rhino:source:opaque#comment"
                && record
                    .fields
                    .get("typecode")
                    .and_then(serde_json::Value::as_str)
                    == Some("0x00000001")
                && record
                    .fields
                    .get("data")
                    .and_then(serde_json::Value::as_str)
                    == Some("AQAAAAcAAAAAAAAAY2FkbXBlZw==")
        })
    });
    if !generated_comment
        || opaque.is_some_and(|records| {
            records.iter().any(|record| {
                !matches!(
                    record
                        .fields
                        .get("typecode")
                        .and_then(serde_json::Value::as_str),
                    Some("0x00000001" | "0xa0000026" | "0x20008031" | "0x20008050")
                )
            })
        })
    {
        return false;
    }
    if namespace
        .arenas
        .get("layers")
        .is_some_and(|records| records.len() != 1 || !default_native_layer(&records[0]))
    {
        return false;
    }
    if namespace
        .arenas
        .get("object_presentation")
        .is_some_and(|records| {
            records
                .iter()
                .any(|record| !default_native_presentation(record))
        })
    {
        return false;
    }
    namespace.arenas.get("unknowns").is_none_or(|records| {
        records.iter().all(|record| {
            record
                .fields
                .get("links")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|links| {
                    !links.is_empty()
                        && links.iter().all(|link| {
                            link.as_str().is_some_and(|link| {
                                [
                                    "rhino:object:body#",
                                    "rhino:object:curve#",
                                    "rhino:object:point#",
                                    "rhino:object:surface#",
                                    "rhino:object:tessellation#",
                                ]
                                .iter()
                                .any(|prefix| link.starts_with(prefix))
                            })
                        })
                })
        })
    })
}

fn default_native_layer(record: &cadmpeg_ir::NativeRecord) -> bool {
    json_i64(record, "archive_index") == Some(0)
        && json_i64(record, "linetype_index") == Some(-1)
        && json_i64(record, "material_index") == Some(-1)
        && json_str(record, "name") == Some("Default")
        && json_bool(record, "visible") == Some(true)
        && json_bool(record, "locked") == Some(false)
        && json_array_empty(record, "rendering_materials")
}

fn default_native_presentation(record: &cadmpeg_ir::NativeRecord) -> bool {
    json_i64(record, "layer_index") == Some(0)
        && json_i64(record, "material_index") == Some(-1)
        && json_i64(record, "linetype_index") == Some(-1)
        && json_i64(record, "hatch_pattern_index") == Some(-1)
        && json_i64(record, "object_mode") == Some(0)
        && json_str(record, "name") == Some("")
        && json_str(record, "url") == Some("")
        && json_bool(record, "visible") == Some(true)
        && json_array_empty(record, "group_indexes")
        && json_array_empty(record, "display_materials")
        && json_array_empty(record, "rendering_materials")
        && json_array_empty(record, "clipping_plane_uuids")
}

fn json_i64(record: &cadmpeg_ir::NativeRecord, name: &str) -> Option<i64> {
    record.fields.get(name)?.as_i64()
}

fn json_str<'a>(record: &'a cadmpeg_ir::NativeRecord, name: &str) -> Option<&'a str> {
    record.fields.get(name)?.as_str()
}

fn json_bool(record: &cadmpeg_ir::NativeRecord, name: &str) -> Option<bool> {
    record.fields.get(name)?.as_bool()
}

fn json_array_empty(record: &cadmpeg_ir::NativeRecord, name: &str) -> bool {
    record
        .fields
        .get(name)
        .and_then(serde_json::Value::as_array)
        .is_some_and(Vec::is_empty)
}

fn planar_sheet_brep_payload(ir: &CadIr) -> Result<Option<BrepPayload>, CodecError> {
    use cadmpeg_ir::topology::{BodyKind, Sense};

    let model = &ir.model;
    let Some(body) = model
        .bodies
        .iter()
        .find(|body| matches!(body.kind, BodyKind::Sheet | BodyKind::Solid))
    else {
        return Ok(None);
    };
    if model.faces.len() > 1 || body.kind == BodyKind::Solid {
        return multi_face_brep_payload(ir, body).map(Some);
    }
    let edge_count = model.coedges.len();
    if model.bodies.len() != 1
        || model.regions.len() != 1
        || model.shells.len() != 1
        || model.faces.len() != 1
        || model.loops.is_empty()
        || edge_count < 3
        || model.edges.len() != edge_count
        || model.vertices.len() != edge_count
        || model.points.len() != edge_count
        || model.curves.len() != edge_count
        || model.surfaces.len() != 1
        || !model.tessellations.is_empty()
    {
        return Err(CodecError::NotImplemented(
            "planar sheet writing currently requires one polygonal face with disjoint loops".into(),
        ));
    }
    if body.regions.len() != 1 || body.transform.is_some() {
        return Err(CodecError::NotImplemented(
            "planar sheet body placement is not writable".into(),
        ));
    }
    check_object_attributes(&body.id.0, body.name.as_deref(), body.color)?;
    let region = &model.regions[0];
    let shell = &model.shells[0];
    let face = &model.faces[0];
    if region.id != body.regions[0]
        || region.body != body.id
        || region.shells != [shell.id.clone()]
        || shell.region != region.id
        || shell.faces != [face.id.clone()]
        || !shell.wire_edges.is_empty()
        || !shell.free_vertices.is_empty()
        || face.shell != shell.id
        || face.loops.len() != model.loops.len()
        || face
            .loops
            .iter()
            .zip(&model.loops)
            .any(|(id, loop_)| *id != loop_.id)
        || face.name.is_some()
        || face.color.is_some()
        || model.loops.iter().any(|loop_| loop_.face != face.id)
        || model
            .loops
            .iter()
            .map(|loop_| loop_.coedges.len())
            .sum::<usize>()
            != edge_count
        || model.loops.iter().any(|loop_| loop_.coedges.len() < 3)
    {
        return Err(CodecError::Malformed(
            "planar sheet ownership graph is inconsistent".into(),
        ));
    }
    let surface = model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .ok_or_else(|| CodecError::Malformed("planar triangle surface is missing".into()))?;
    if surface.source_object.is_some() {
        return Err(CodecError::NotImplemented(
            "sheet surface source-object state is not writable".into(),
        ));
    }
    let (plane_frame, nurbs_patch) = match &surface.geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            check_frame(&surface.id.0, *origin, *normal, *u_axis, "plane")?;
            (
                Some((*origin, *normal, *u_axis, cross(*normal, *u_axis))),
                None,
            )
        }
        SurfaceGeometry::Nurbs(nurbs) => {
            check_nurbs_surface(&surface.id.0, nurbs)?;
            if nurbs.u_periodic || nurbs.v_periodic {
                return Err(CodecError::NotImplemented(
                    "rectangular Brep patch surface must be nonperiodic".into(),
                ));
            }
            (None, Some(nurbs))
        }
        _ => {
            return Err(CodecError::NotImplemented(
                "single-face Brep surface is not a plane or NURBS patch".into(),
            ))
        }
    };

    let mut ordered_coedges = Vec::with_capacity(edge_count);
    let mut loop_ranges = Vec::with_capacity(model.loops.len());
    for loop_ in &model.loops {
        let start = ordered_coedges.len();
        for id in &loop_.coedges {
            let coedge = model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .ok_or_else(|| CodecError::Malformed(format!("coedge {} is missing", id.0)))?;
            if coedge.owner_loop != loop_.id {
                return Err(CodecError::Malformed(format!(
                    "coedge {} ownership is inconsistent",
                    coedge.id.0
                )));
            }
            ordered_coedges.push(coedge);
        }
        let end = ordered_coedges.len();
        for index in start..end {
            let current = ordered_coedges[index];
            let offset = index - start;
            let count = end - start;
            if current.next != ordered_coedges[start + (offset + 1) % count].id
                || current.previous != ordered_coedges[start + (offset + count - 1) % count].id
                || current.radial_next != current.id
            {
                return Err(CodecError::Malformed(format!(
                    "coedge {} ring is inconsistent",
                    current.id.0
                )));
            }
        }
        loop_ranges.push(start..end);
    }
    validate_brep_pcurve_ownership(model, &ordered_coedges)?;

    let mut ordered_edges = Vec::with_capacity(edge_count);
    let mut traversal_vertices = Vec::with_capacity(edge_count);
    for coedge in &ordered_coedges {
        let edge = model
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .ok_or_else(|| CodecError::Malformed(format!("edge {} is missing", coedge.edge.0)))?;
        if ordered_edges
            .iter()
            .any(|existing: &&cadmpeg_ir::topology::Edge| existing.id == edge.id)
        {
            return Err(CodecError::NotImplemented(
                "planar sheet cannot reuse an edge in one loop".into(),
            ));
        }
        let from_id = if coedge.sense == Sense::Forward {
            &edge.start
        } else {
            &edge.end
        };
        traversal_vertices.push(from_id.clone());
        ordered_edges.push(edge);
    }
    for range in &loop_ranges {
        for index in range.clone() {
            let edge = ordered_edges[index];
            let traversal_end = if ordered_coedges[index].sense == Sense::Forward {
                &edge.end
            } else {
                &edge.start
            };
            let next = range.start + (index - range.start + 1) % range.len();
            if *traversal_end != traversal_vertices[next] {
                return Err(CodecError::Malformed(
                    "planar coedge traversal does not close".into(),
                ));
            }
        }
    }

    let mut ordered_vertices = Vec::with_capacity(edge_count);
    let mut ordered_points = Vec::with_capacity(edge_count);
    for id in &traversal_vertices {
        let vertex = model
            .vertices
            .iter()
            .find(|vertex| vertex.id == *id)
            .ok_or_else(|| CodecError::Malformed(format!("vertex {} is missing", id.0)))?;
        if ordered_vertices
            .iter()
            .any(|existing: &&cadmpeg_ir::topology::Vertex| existing.id == vertex.id)
        {
            return Err(CodecError::Malformed(
                "planar loop has repeated traversal vertices".into(),
            ));
        }
        let point = model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .ok_or_else(|| CodecError::Malformed(format!("point {} is missing", vertex.point.0)))?;
        ordered_vertices.push(vertex);
        ordered_points.push(point.position);
    }
    for edge in &ordered_edges {
        validate_planar_edge(model, edge, ir.tolerances.linear)?;
    }
    if let Some((origin, normal, _, _)) = plane_frame {
        let plane_tolerance = face.tolerance.unwrap_or(ir.tolerances.linear).max(1.0e-10);
        for point in &ordered_points {
            let distance = (point.x - origin.x) * normal.x
                + (point.y - origin.y) * normal.y
                + (point.z - origin.z) * normal.z;
            if distance.abs() > plane_tolerance {
                return Err(CodecError::Malformed(
                    "planar loop vertex is outside its face plane tolerance".into(),
                ));
            }
        }
        for edge in &ordered_edges {
            let curve = model
                .curves
                .iter()
                .find(|curve| edge.curve.as_ref() == Some(&curve.id))
                .expect("validated edge curve");
            if let CurveGeometry::Nurbs(nurbs) = &curve.geometry {
                for point in &nurbs.control_points {
                    let distance = (point.x - origin.x) * normal.x
                        + (point.y - origin.y) * normal.y
                        + (point.z - origin.z) * normal.z;
                    if distance.abs() > plane_tolerance {
                        return Err(CodecError::Malformed(format!(
                            "edge curve {} is outside its face plane tolerance",
                            curve.id.0
                        )));
                    }
                }
            }
        }
    } else {
        validate_nurbs_trim_loop(
            model,
            nurbs_patch.expect("non-plane patch"),
            face.tolerance.unwrap_or(ir.tolerances.linear),
            &ordered_edges,
            &ordered_coedges,
        )?;
    }

    let mut payload = vec![0x32];
    let mut direct = vec![0x32];
    let c2 = (0..edge_count)
        .map(|index| {
            if let Some((origin, _, u_axis, v_axis)) = plane_frame {
                brep_c2_curve(
                    model,
                    ordered_edges[index],
                    ordered_coedges[index],
                    origin,
                    u_axis,
                    v_axis,
                )
            } else {
                explicit_brep_c2_curve(model, ordered_edges[index], ordered_coedges[index])
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    payload.extend(polymorphic_array(&c2));
    let c3 = ordered_edges
        .iter()
        .map(|edge| brep_c3_curve(model, edge))
        .collect::<Vec<_>>();
    payload.extend(polymorphic_array(&c3));
    let surface_record = if let Some((origin, normal, u_axis, _)) = plane_frame {
        (
            PLANE_SURFACE_CLASS,
            plane_surface_payload(origin, normal, u_axis),
        )
    } else {
        (
            NURBS_SURFACE_CLASS,
            nurbs_surface_payload(nurbs_patch.expect("non-plane patch")),
        )
    };
    payload.extend(polymorphic_array(&[surface_record]));

    let edge_index = ordered_edges
        .iter()
        .enumerate()
        .map(|(index, edge)| (edge.id.0.clone(), index as i32))
        .collect::<std::collections::BTreeMap<_, _>>();
    let vertex_index = ordered_vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| (vertex.id.0.clone(), index as i32))
        .collect::<std::collections::BTreeMap<_, _>>();
    let vertex_records = ordered_vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| {
            let point = ordered_points[index];
            let incident = ordered_edges
                .iter()
                .flat_map(|edge| {
                    [(&edge.start, &vertex.id), (&edge.end, &vertex.id)]
                        .into_iter()
                        .filter(|(endpoint, vertex)| endpoint == vertex)
                        .map(|_| edge_index[&edge.id.0])
                })
                .collect::<Vec<_>>();
            let mut record = (index as i32).to_le_bytes().to_vec();
            for value in [point.x, point.y, point.z] {
                record.extend(value.to_le_bytes());
            }
            record.extend(indexes(&incident));
            record.extend(vertex.tolerance.unwrap_or(0.0).to_le_bytes());
            record
        })
        .collect::<Vec<_>>();
    payload.extend(raw_array(&vertex_records));
    let edge_records = ordered_edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            let mut record = (index as i32).to_le_bytes().to_vec();
            record.extend((index as i32).to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            record.extend(
                edge.param_range
                    .expect("validated edge domain")
                    .into_iter()
                    .flat_map(f64::to_le_bytes),
            );
            record.extend(vertex_index[&edge.start.0].to_le_bytes());
            record.extend(vertex_index[&edge.end.0].to_le_bytes());
            record.extend(indexes(&[index as i32]));
            record.extend(edge.tolerance.unwrap_or(0.0).to_le_bytes());
            record.extend(
                edge.param_range
                    .expect("validated edge domain")
                    .into_iter()
                    .flat_map(f64::to_le_bytes),
            );
            record
        })
        .collect::<Vec<_>>();
    payload.extend(raw_array(&edge_records));
    let trim_records = ordered_coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| {
            let edge = ordered_edges[index];
            let mut record = (index as i32).to_le_bytes().to_vec();
            record.extend((index as i32).to_le_bytes());
            record.extend(
                edge.param_range
                    .expect("validated edge domain")
                    .into_iter()
                    .flat_map(f64::to_le_bytes),
            );
            record.extend((index as i32).to_le_bytes());
            let (from, to) = if coedge.sense == Sense::Forward {
                (&edge.start, &edge.end)
            } else {
                (&edge.end, &edge.start)
            };
            record.extend(vertex_index[&from.0].to_le_bytes());
            record.extend(vertex_index[&to.0].to_le_bytes());
            record.extend(i32::from(coedge.sense == Sense::Reversed).to_le_bytes());
            record.extend(1_i32.to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            let loop_index = loop_ranges
                .iter()
                .position(|range| range.contains(&index))
                .expect("trim belongs to one loop");
            record.extend((loop_index as i32).to_le_bytes());
            record.extend(
                [brep_pcurve_fit_tolerance(model, coedge), 0.0_f64]
                    .into_iter()
                    .flat_map(f64::to_le_bytes),
            );
            record.extend(
                edge.param_range
                    .expect("validated edge domain")
                    .into_iter()
                    .flat_map(f64::to_le_bytes),
            );
            record.push(0);
            record.extend([0_u8; 31]);
            record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
            record
        })
        .collect::<Vec<_>>();
    payload.extend(raw_array(&trim_records));
    let loop_records = loop_ranges
        .iter()
        .enumerate()
        .map(|(index, range)| {
            let mut record = (index as i32).to_le_bytes().to_vec();
            record.extend(indexes(
                &range.clone().map(|trim| trim as i32).collect::<Vec<_>>(),
            ));
            record.extend(if index == 0 { 1_i32 } else { 2_i32 }.to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            record
        })
        .collect::<Vec<_>>();
    payload.extend(raw_array(&loop_records));
    let mut face_record = 0_i32.to_le_bytes().to_vec();
    face_record.extend(indexes(&(0..model.loops.len() as i32).collect::<Vec<_>>()));
    face_record.extend(0_i32.to_le_bytes());
    face_record.extend(i32::from(face.sense == Sense::Reversed).to_le_bytes());
    face_record.extend(0_i32.to_le_bytes());
    payload.extend(raw_array(&[face_record]));
    let min = ordered_points.iter().fold([f64::INFINITY; 3], |a, p| {
        [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
    });
    let max = ordered_points.iter().fold([f64::NEG_INFINITY; 3], |a, p| {
        [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
    });
    for value in min.into_iter().chain(max) {
        let bytes = value.to_le_bytes();
        payload.extend(bytes);
        direct.extend(bytes);
    }
    let mesh_presence = vec![0; model.faces.len() + 1];
    payload.extend(crc_chunk(0x4000_8000, &mesh_presence));
    payload.extend(crc_chunk(0x4000_8000, &mesh_presence));
    let solid = 0_i32.to_le_bytes();
    payload.extend(solid);
    direct.extend(solid);
    Ok(Some(BrepPayload {
        body: payload,
        direct,
    }))
}

#[derive(Clone, Copy)]
enum WritableFaceSurface<'a> {
    Plane {
        origin: cadmpeg_ir::math::Point3,
        normal: cadmpeg_ir::math::Vector3,
        u_axis: cadmpeg_ir::math::Vector3,
        v_axis: cadmpeg_ir::math::Vector3,
    },
    Nurbs(&'a cadmpeg_ir::geometry::NurbsSurface),
}

fn multi_face_brep_payload(
    ir: &CadIr,
    body: &cadmpeg_ir::topology::Body,
) -> Result<BrepPayload, CodecError> {
    use cadmpeg_ir::topology::{BodyKind, Sense};
    use std::collections::{BTreeMap, BTreeSet};

    let model = &ir.model;
    if model.bodies.len() != 1
        || model.regions.len() != 1
        || model.shells.len() != 1
        || model.faces.len() < 2
        || model.loops.len() < model.faces.len()
        || model.coedges.len() < 3 * model.faces.len()
        || model.edges.is_empty()
        || model.vertices.is_empty()
        || model.points.len() != model.vertices.len()
        || model.curves.len() != model.edges.len()
        || model.surfaces.len() != model.faces.len()
        || !model.tessellations.is_empty()
    {
        return Err(CodecError::NotImplemented(
            "multi-face planar sheet writing requires one connected shell with explicit line and plane carriers"
                .into(),
        ));
    }
    if body.regions.len() != 1 || body.transform.is_some() {
        return Err(CodecError::NotImplemented(
            "multi-face planar sheet body placement is not writable".into(),
        ));
    }
    check_object_attributes(&body.id.0, body.name.as_deref(), body.color)?;
    let region = &model.regions[0];
    let shell = &model.shells[0];
    if region.id != body.regions[0]
        || region.body != body.id
        || region.shells != [shell.id.clone()]
        || shell.region != region.id
        || shell.faces
            != model
                .faces
                .iter()
                .map(|face| face.id.clone())
                .collect::<Vec<_>>()
        || !shell.wire_edges.is_empty()
        || !shell.free_vertices.is_empty()
    {
        return Err(CodecError::Malformed(
            "multi-face planar sheet ownership graph is inconsistent".into(),
        ));
    }

    let vertex_index = model
        .vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| (vertex.id.0.clone(), index as i32))
        .collect::<BTreeMap<_, _>>();
    let edge_index = model
        .edges
        .iter()
        .enumerate()
        .map(|(index, edge)| (edge.id.0.clone(), index as i32))
        .collect::<BTreeMap<_, _>>();
    let coedge_index = model
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| (coedge.id.0.clone(), index as i32))
        .collect::<BTreeMap<_, _>>();
    let loop_index = model
        .loops
        .iter()
        .enumerate()
        .map(|(index, loop_)| (loop_.id.0.clone(), index as i32))
        .collect::<BTreeMap<_, _>>();
    let face_index = model
        .faces
        .iter()
        .enumerate()
        .map(|(index, face)| (face.id.0.clone(), index as i32))
        .collect::<BTreeMap<_, _>>();
    let surface_index = model
        .surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| (surface.id.0.clone(), index as i32))
        .collect::<BTreeMap<_, _>>();
    if vertex_index.len() != model.vertices.len()
        || edge_index.len() != model.edges.len()
        || coedge_index.len() != model.coedges.len()
        || loop_index.len() != model.loops.len()
        || face_index.len() != model.faces.len()
        || surface_index.len() != model.surfaces.len()
    {
        return Err(CodecError::Malformed(
            "multi-face planar sheet contains duplicate topology identifiers".into(),
        ));
    }

    let mut points = Vec::with_capacity(model.vertices.len());
    let mut used_points = BTreeSet::new();
    for vertex in &model.vertices {
        let point = model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .ok_or_else(|| CodecError::Malformed(format!("point {} is missing", vertex.point.0)))?;
        if !used_points.insert(point.id.0.clone())
            || !point.position.x.is_finite()
            || !point.position.y.is_finite()
            || !point.position.z.is_finite()
        {
            return Err(CodecError::Malformed(format!(
                "vertex {} has a shared or invalid point",
                vertex.id.0
            )));
        }
        points.push(point.position);
    }
    for edge in &model.edges {
        if !vertex_index.contains_key(&edge.start.0) || !vertex_index.contains_key(&edge.end.0) {
            return Err(CodecError::Malformed(format!(
                "edge {} references a missing vertex",
                edge.id.0
            )));
        }
        validate_planar_edge(model, edge, ir.tolerances.linear)?;
    }
    let used_curves = model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.as_ref().map(|id| id.0.clone()))
        .collect::<BTreeSet<_>>();
    if used_curves.len() != model.curves.len() {
        return Err(CodecError::NotImplemented(
            "multi-face planar sheet requires one distinct line curve per edge".into(),
        ));
    }

    let mut face_surfaces = Vec::with_capacity(model.faces.len());
    let mut used_surfaces = BTreeSet::new();
    let mut owned_loops = BTreeSet::new();
    for face in &model.faces {
        if face.shell != shell.id
            || face.loops.is_empty()
            || face.name.is_some()
            || face.color.is_some()
            || !used_surfaces.insert(face.surface.0.clone())
        {
            return Err(CodecError::NotImplemented(format!(
                "face {} has unsupported ownership, attributes, or shared surface state",
                face.id.0
            )));
        }
        let surface = model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .ok_or_else(|| {
                CodecError::Malformed(format!("surface {} is missing", face.surface.0))
            })?;
        if surface.source_object.is_some() {
            return Err(CodecError::NotImplemented(format!(
                "surface {} source-object state is not writable",
                surface.id.0
            )));
        }
        match &surface.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => {
                check_frame(&surface.id.0, *origin, *normal, *u_axis, "plane")?;
                face_surfaces.push(WritableFaceSurface::Plane {
                    origin: *origin,
                    normal: *normal,
                    u_axis: *u_axis,
                    v_axis: cross(*normal, *u_axis),
                });
            }
            SurfaceGeometry::Nurbs(nurbs) => {
                check_nurbs_surface(&surface.id.0, nurbs)?;
                if nurbs.u_periodic || nurbs.v_periodic {
                    return Err(CodecError::NotImplemented(format!(
                        "face {} does not have a nonperiodic NURBS surface",
                        face.id.0
                    )));
                }
                face_surfaces.push(WritableFaceSurface::Nurbs(nurbs));
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "face {} surface is not a plane or NURBS patch",
                    face.id.0
                )))
            }
        }
        for loop_id in &face.loops {
            if !owned_loops.insert(loop_id.0.clone()) {
                return Err(CodecError::Malformed(format!(
                    "loop {} has multiple face owners",
                    loop_id.0
                )));
            }
        }
    }
    if owned_loops.len() != model.loops.len() {
        return Err(CodecError::NotImplemented(
            "multi-face planar sheet contains orphan loops".into(),
        ));
    }

    let mut owned_coedges = BTreeSet::new();
    for loop_ in &model.loops {
        let face = model
            .faces
            .iter()
            .find(|face| face.id == loop_.face)
            .ok_or_else(|| CodecError::Malformed(format!("face {} is missing", loop_.face.0)))?;
        if !face.loops.contains(&loop_.id) || loop_.coedges.len() < 3 {
            return Err(CodecError::Malformed(format!(
                "loop {} ownership or boundary is invalid",
                loop_.id.0
            )));
        }
        for (offset, id) in loop_.coedges.iter().enumerate() {
            let coedge = model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .ok_or_else(|| CodecError::Malformed(format!("coedge {} is missing", id.0)))?;
            if !owned_coedges.insert(id.0.clone())
                || coedge.owner_loop != loop_.id
                || coedge.next != loop_.coedges[(offset + 1) % loop_.coedges.len()]
                || coedge.previous
                    != loop_.coedges[(offset + loop_.coedges.len() - 1) % loop_.coedges.len()]
            {
                return Err(CodecError::NotImplemented(format!(
                    "coedge {} ownership or ring is not writable",
                    coedge.id.0
                )));
            }
            let edge = model
                .edges
                .iter()
                .find(|edge| edge.id == coedge.edge)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("edge {} is missing", coedge.edge.0))
                })?;
            let end = if coedge.sense == Sense::Forward {
                &edge.end
            } else {
                &edge.start
            };
            let next = model
                .coedges
                .iter()
                .find(|candidate| candidate.id == coedge.next)
                .expect("validated next coedge");
            let next_edge = model
                .edges
                .iter()
                .find(|candidate| candidate.id == next.edge)
                .expect("validated next edge");
            let next_start = if next.sense == Sense::Forward {
                &next_edge.start
            } else {
                &next_edge.end
            };
            if end != next_start {
                return Err(CodecError::Malformed(format!(
                    "loop {} coedge traversal does not close",
                    loop_.id.0
                )));
            }
        }
    }
    if owned_coedges.len() != model.coedges.len() {
        return Err(CodecError::NotImplemented(
            "multi-face planar sheet contains orphan coedges".into(),
        ));
    }
    validate_brep_pcurve_ownership(model, &model.coedges.iter().collect::<Vec<_>>())?;

    let mut edge_uses = Vec::with_capacity(model.edges.len());
    let mut edge_faces = Vec::with_capacity(model.edges.len());
    for edge in &model.edges {
        let uses = model
            .coedges
            .iter()
            .enumerate()
            .filter(|(_, coedge)| coedge.edge == edge.id)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if uses.is_empty() || uses.len() > 2 || body.kind == BodyKind::Solid && uses.len() != 2 {
            return Err(CodecError::NotImplemented(format!(
                "edge {} incidence is incompatible with the body kind",
                edge.id.0
            )));
        }
        let start = uses[0];
        let mut ordered = vec![start];
        while ordered.len() < uses.len() {
            let next_id = &model.coedges[*ordered.last().expect("nonempty")].radial_next;
            let next = *coedge_index.get(&next_id.0).ok_or_else(|| {
                CodecError::Malformed(format!("radial coedge {} is missing", next_id.0))
            })? as usize;
            if !uses.contains(&next) || ordered.contains(&next) {
                return Err(CodecError::Malformed(format!(
                    "edge {} radial ring is inconsistent",
                    edge.id.0
                )));
            }
            ordered.push(next);
        }
        if model.coedges[*ordered.last().expect("nonempty")].radial_next != model.coedges[start].id
        {
            return Err(CodecError::Malformed(format!(
                "edge {} radial ring does not close",
                edge.id.0
            )));
        }
        if ordered.len() == 2 && model.coedges[ordered[0]].sense == model.coedges[ordered[1]].sense
        {
            return Err(CodecError::Malformed(format!(
                "shared edge {} has equal directed uses",
                edge.id.0
            )));
        }
        let faces = ordered
            .iter()
            .map(|coedge| {
                let loop_id = &model.coedges[*coedge].owner_loop;
                let loop_ = &model.loops[*loop_index.get(&loop_id.0).expect("owned loop") as usize];
                *face_index.get(&loop_.face.0).expect("owned face") as usize
            })
            .collect::<Vec<_>>();
        edge_uses.push(ordered);
        edge_faces.push(faces);
    }
    let mut reached = BTreeSet::from([0_usize]);
    loop {
        let prior = reached.len();
        for faces in &edge_faces {
            if faces.iter().any(|face| reached.contains(face)) {
                reached.extend(faces);
            }
        }
        if reached.len() == prior {
            break;
        }
    }
    if reached.len() != model.faces.len() {
        return Err(CodecError::NotImplemented(
            "multi-face planar sheet must be edge-connected in one shell".into(),
        ));
    }

    for (loop_position, loop_) in model.loops.iter().enumerate() {
        let face_position = *face_index.get(&loop_.face.0).expect("owned face") as usize;
        if let WritableFaceSurface::Nurbs(surface) = face_surfaces[face_position] {
            let coedges = loop_
                .coedges
                .iter()
                .map(|id| &model.coedges[*coedge_index.get(&id.0).expect("owned coedge") as usize])
                .collect::<Vec<_>>();
            let edges = coedges
                .iter()
                .map(|coedge| {
                    &model.edges[*edge_index.get(&coedge.edge.0).expect("owned edge") as usize]
                })
                .collect::<Vec<_>>();
            validate_nurbs_trim_loop(
                model,
                surface,
                model.faces[face_position]
                    .tolerance
                    .unwrap_or(ir.tolerances.linear),
                &edges,
                &coedges,
            )?;
            continue;
        }
        let WritableFaceSurface::Plane {
            origin,
            normal,
            u_axis,
            v_axis,
        } = face_surfaces[face_position]
        else {
            unreachable!("NURBS face continued")
        };
        let tolerance = model.faces[face_position]
            .tolerance
            .unwrap_or(ir.tolerances.linear)
            .max(1.0e-10);
        let mut boundary = Vec::with_capacity(loop_.coedges.len());
        for coedge_id in &loop_.coedges {
            let coedge =
                &model.coedges[*coedge_index.get(&coedge_id.0).expect("owned coedge") as usize];
            let edge = &model.edges[*edge_index.get(&coedge.edge.0).expect("owned edge") as usize];
            let curve = model
                .curves
                .iter()
                .find(|curve| edge.curve.as_ref() == Some(&curve.id))
                .expect("validated edge curve");
            if let CurveGeometry::Nurbs(nurbs) = &curve.geometry {
                for point in &nurbs.control_points {
                    let distance = (point.x - origin.x) * normal.x
                        + (point.y - origin.y) * normal.y
                        + (point.z - origin.z) * normal.z;
                    if distance.abs() > tolerance {
                        return Err(CodecError::Malformed(format!(
                            "edge curve {} is outside its face plane tolerance",
                            curve.id.0
                        )));
                    }
                }
            }
            let start = if coedge.sense == Sense::Forward {
                &edge.start
            } else {
                &edge.end
            };
            boundary.push(plane_uv(
                points[*vertex_index.get(&start.0).expect("owned vertex") as usize],
                origin,
                u_axis,
                v_axis,
            ));
            for vertex_id in [&edge.start, &edge.end] {
                let point = points[*vertex_index.get(&vertex_id.0).expect("owned vertex") as usize];
                let distance = (point.x - origin.x) * normal.x
                    + (point.y - origin.y) * normal.y
                    + (point.z - origin.z) * normal.z;
                if distance.abs() > tolerance {
                    return Err(CodecError::Malformed(format!(
                        "loop {} vertex is outside its face plane tolerance",
                        model.loops[loop_position].id.0
                    )));
                }
            }
        }
        let twice_area = boundary
            .iter()
            .zip(boundary.iter().cycle().skip(1))
            .take(boundary.len())
            .map(|(from, to)| from[0] * to[1] - to[0] * from[1])
            .sum::<f64>();
        if !twice_area.is_finite() || twice_area.abs() <= tolerance * tolerance {
            return Err(CodecError::Malformed(format!(
                "loop {} has degenerate planar area",
                loop_.id.0
            )));
        }
    }

    let mut payload = vec![0x32];
    let mut direct = vec![0x32];
    let c2 = model
        .coedges
        .iter()
        .map(|coedge| {
            let loop_ =
                &model.loops[*loop_index.get(&coedge.owner_loop.0).expect("owned loop") as usize];
            let face = *face_index.get(&loop_.face.0).expect("owned face") as usize;
            let edge = &model.edges[*edge_index.get(&coedge.edge.0).expect("owned edge") as usize];
            match face_surfaces[face] {
                WritableFaceSurface::Plane {
                    origin,
                    u_axis,
                    v_axis,
                    ..
                } => brep_c2_curve(model, edge, coedge, origin, u_axis, v_axis),
                WritableFaceSurface::Nurbs(_) => explicit_brep_c2_curve(model, edge, coedge),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    payload.extend(polymorphic_array(&c2));
    let c3 = model
        .edges
        .iter()
        .map(|edge| brep_c3_curve(model, edge))
        .collect::<Vec<_>>();
    payload.extend(polymorphic_array(&c3));
    let surfaces = model
        .surfaces
        .iter()
        .map(|surface| match &surface.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => (
                PLANE_SURFACE_CLASS,
                plane_surface_payload(*origin, *normal, *u_axis),
            ),
            SurfaceGeometry::Nurbs(nurbs) => (NURBS_SURFACE_CLASS, nurbs_surface_payload(nurbs)),
            _ => unreachable!("validated writable face surface"),
        })
        .collect::<Vec<_>>();
    payload.extend(polymorphic_array(&surfaces));

    let vertex_records = model
        .vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| {
            let point = points[index];
            let incident = model
                .edges
                .iter()
                .flat_map(|edge| {
                    [(&edge.start, &vertex.id), (&edge.end, &vertex.id)]
                        .into_iter()
                        .filter(|(endpoint, vertex)| endpoint == vertex)
                        .map(|_| edge_index[&edge.id.0])
                })
                .collect::<Vec<_>>();
            let mut record = (index as i32).to_le_bytes().to_vec();
            for value in [point.x, point.y, point.z] {
                record.extend(value.to_le_bytes());
            }
            record.extend(indexes(&incident));
            record.extend(vertex.tolerance.unwrap_or(0.0).to_le_bytes());
            record
        })
        .collect::<Vec<_>>();
    payload.extend(raw_array(&vertex_records));
    let edge_records = model
        .edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            let domain = edge.param_range.expect("validated edge domain");
            let mut record = (index as i32).to_le_bytes().to_vec();
            record.extend((index as i32).to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            record.extend(domain.into_iter().flat_map(f64::to_le_bytes));
            record.extend(vertex_index[&edge.start.0].to_le_bytes());
            record.extend(vertex_index[&edge.end.0].to_le_bytes());
            record.extend(indexes(
                &edge_uses[index]
                    .iter()
                    .map(|coedge| *coedge as i32)
                    .collect::<Vec<_>>(),
            ));
            record.extend(edge.tolerance.unwrap_or(0.0).to_le_bytes());
            record.extend(domain.into_iter().flat_map(f64::to_le_bytes));
            record
        })
        .collect::<Vec<_>>();
    payload.extend(raw_array(&edge_records));
    let trim_records = model
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| {
            let edge_position = edge_index[&coedge.edge.0] as usize;
            let edge = &model.edges[edge_position];
            let domain = edge.param_range.expect("validated edge domain");
            let (from, to) = if coedge.sense == Sense::Forward {
                (&edge.start, &edge.end)
            } else {
                (&edge.end, &edge.start)
            };
            let mut record = (index as i32).to_le_bytes().to_vec();
            record.extend((index as i32).to_le_bytes());
            record.extend(domain.into_iter().flat_map(f64::to_le_bytes));
            record.extend((edge_position as i32).to_le_bytes());
            record.extend(vertex_index[&from.0].to_le_bytes());
            record.extend(vertex_index[&to.0].to_le_bytes());
            record.extend(i32::from(coedge.sense == Sense::Reversed).to_le_bytes());
            record.extend(
                (if edge_uses[edge_position].len() == 1 {
                    1_i32
                } else {
                    2_i32
                })
                .to_le_bytes(),
            );
            record.extend(0_i32.to_le_bytes());
            record.extend(loop_index[&coedge.owner_loop.0].to_le_bytes());
            record.extend(
                [brep_pcurve_fit_tolerance(model, coedge), 0.0_f64]
                    .into_iter()
                    .flat_map(f64::to_le_bytes),
            );
            record.extend(domain.into_iter().flat_map(f64::to_le_bytes));
            record.push(0);
            record.extend([0_u8; 31]);
            record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
            record
        })
        .collect::<Vec<_>>();
    payload.extend(raw_array(&trim_records));
    let loop_records = model
        .loops
        .iter()
        .enumerate()
        .map(|(index, loop_)| {
            let face = &model.faces[face_index[&loop_.face.0] as usize];
            let mut record = (index as i32).to_le_bytes().to_vec();
            record.extend(indexes(
                &loop_
                    .coedges
                    .iter()
                    .map(|coedge| coedge_index[&coedge.0])
                    .collect::<Vec<_>>(),
            ));
            record.extend(
                (if face.loops.first() == Some(&loop_.id) {
                    1_i32
                } else {
                    2_i32
                })
                .to_le_bytes(),
            );
            record.extend(face_index[&loop_.face.0].to_le_bytes());
            record
        })
        .collect::<Vec<_>>();
    payload.extend(raw_array(&loop_records));
    let face_records = model
        .faces
        .iter()
        .enumerate()
        .map(|(index, face)| {
            let mut record = (index as i32).to_le_bytes().to_vec();
            record.extend(indexes(
                &face
                    .loops
                    .iter()
                    .map(|loop_| loop_index[&loop_.0])
                    .collect::<Vec<_>>(),
            ));
            record.extend(surface_index[&face.surface.0].to_le_bytes());
            record.extend(i32::from(face.sense == Sense::Reversed).to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            record
        })
        .collect::<Vec<_>>();
    payload.extend(raw_array(&face_records));
    let min = points.iter().fold([f64::INFINITY; 3], |a, point| {
        [a[0].min(point.x), a[1].min(point.y), a[2].min(point.z)]
    });
    let max = points.iter().fold([f64::NEG_INFINITY; 3], |a, point| {
        [a[0].max(point.x), a[1].max(point.y), a[2].max(point.z)]
    });
    for value in min.into_iter().chain(max) {
        let bytes = value.to_le_bytes();
        payload.extend(bytes);
        direct.extend(bytes);
    }
    let mesh_presence = vec![0; model.faces.len() + 1];
    payload.extend(crc_chunk(0x4000_8000, &mesh_presence));
    payload.extend(crc_chunk(0x4000_8000, &mesh_presence));
    let solid = i32::from(body.kind == BodyKind::Solid).to_le_bytes();
    payload.extend(solid);
    direct.extend(solid);
    Ok(BrepPayload {
        body: payload,
        direct,
    })
}

fn validate_planar_edge(
    model: &cadmpeg_ir::document::Model,
    edge: &cadmpeg_ir::topology::Edge,
    document_tolerance: f64,
) -> Result<(), CodecError> {
    let curve_id = edge.curve.as_ref().ok_or_else(|| {
        CodecError::NotImplemented(format!("edge {} has no writable curve", edge.id.0))
    })?;
    let curve = model
        .curves
        .iter()
        .find(|curve| curve.id == *curve_id)
        .ok_or_else(|| CodecError::Malformed(format!("curve {} is missing", curve_id.0)))?;
    if curve.source_object.is_some() {
        return Err(CodecError::NotImplemented(format!(
            "edge curve {} source-object state is not writable",
            curve.id.0
        )));
    }
    let [start_parameter, end_parameter] = edge.param_range.ok_or_else(|| {
        CodecError::NotImplemented(format!("edge {} has no parameter range", edge.id.0))
    })?;
    if !start_parameter.is_finite()
        || !end_parameter.is_finite()
        || start_parameter >= end_parameter
    {
        return Err(CodecError::Malformed(format!(
            "edge {} has an invalid parameter range",
            edge.id.0
        )));
    }
    let (expected_start, expected_end) = match &curve.geometry {
        CurveGeometry::Line { origin, direction } => {
            if (direction.norm() - 1.0).abs() > 1.0e-10 {
                return Err(CodecError::Malformed(format!(
                    "edge {} has an invalid line parameterization",
                    edge.id.0
                )));
            }
            (
                cadmpeg_ir::math::Point3::new(
                    origin.x + direction.x * start_parameter,
                    origin.y + direction.y * start_parameter,
                    origin.z + direction.z * start_parameter,
                ),
                cadmpeg_ir::math::Point3::new(
                    origin.x + direction.x * end_parameter,
                    origin.y + direction.y * end_parameter,
                    origin.z + direction.z * end_parameter,
                ),
            )
        }
        CurveGeometry::Nurbs(nurbs) => {
            check_nurbs_curve(&curve.id.0, nurbs)?;
            let count = nurbs.control_points.len();
            let domain = [nurbs.knots[nurbs.degree as usize], nurbs.knots[count]];
            if nurbs.periodic || domain != [start_parameter, end_parameter] {
                return Err(CodecError::NotImplemented(format!(
                    "edge {} requires a nonperiodic full-domain NURBS curve",
                    edge.id.0
                )));
            }
            (
                *nurbs.control_points.first().expect("validated NURBS poles"),
                *nurbs.control_points.last().expect("validated NURBS poles"),
            )
        }
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "edge curve {} is not a line or NURBS curve",
                curve.id.0
            )))
        }
    };
    let start = vertex_point(model, &edge.start)
        .ok_or_else(|| CodecError::Malformed(format!("edge {} start is missing", edge.id.0)))?;
    let end = vertex_point(model, &edge.end)
        .ok_or_else(|| CodecError::Malformed(format!("edge {} end is missing", edge.id.0)))?;
    let tolerance = edge.tolerance.unwrap_or(document_tolerance).max(1.0e-10);
    if !close_point(start, expected_start, tolerance) || !close_point(end, expected_end, tolerance)
    {
        return Err(CodecError::Malformed(format!(
            "edge {} endpoints disagree with its line curve",
            edge.id.0
        )));
    }
    Ok(())
}

fn close_point(
    left: cadmpeg_ir::math::Point3,
    right: cadmpeg_ir::math::Point3,
    tolerance: f64,
) -> bool {
    (left.x - right.x).abs() <= tolerance
        && (left.y - right.y).abs() <= tolerance
        && (left.z - right.z).abs() <= tolerance
}

fn brep_c3_curve(
    model: &cadmpeg_ir::document::Model,
    edge: &cadmpeg_ir::topology::Edge,
) -> ([u8; 16], Vec<u8>) {
    let curve = model
        .curves
        .iter()
        .find(|curve| edge.curve.as_ref() == Some(&curve.id))
        .expect("validated edge curve");
    match &curve.geometry {
        CurveGeometry::Line { .. } => {
            let from = vertex_point(model, &edge.start).expect("validated edge start");
            let to = vertex_point(model, &edge.end).expect("validated edge end");
            (
                LINE_CLASS,
                bounded_line_payload(
                    [from.x, from.y, from.z],
                    [to.x, to.y, to.z],
                    edge.param_range.expect("validated edge domain"),
                    3,
                ),
            )
        }
        CurveGeometry::Nurbs(nurbs) => (NURBS_CURVE_CLASS, nurbs_curve_payload(nurbs)),
        _ => unreachable!("validated writable Brep curve"),
    }
}

fn generated_projected_brep_c2_curve(
    model: &cadmpeg_ir::document::Model,
    edge: &cadmpeg_ir::topology::Edge,
    sense: cadmpeg_ir::topology::Sense,
    origin: cadmpeg_ir::math::Point3,
    u_axis: cadmpeg_ir::math::Vector3,
    v_axis: cadmpeg_ir::math::Vector3,
) -> Result<([u8; 16], Vec<u8>), CodecError> {
    use cadmpeg_ir::topology::Sense;

    let curve = model
        .curves
        .iter()
        .find(|curve| edge.curve.as_ref() == Some(&curve.id))
        .expect("validated edge curve");
    Ok(match &curve.geometry {
        CurveGeometry::Line { .. } => {
            let (from, to) = if sense == Sense::Forward {
                (&edge.start, &edge.end)
            } else {
                (&edge.end, &edge.start)
            };
            let from = vertex_point(model, from).expect("validated trim start");
            let to = vertex_point(model, to).expect("validated trim end");
            (
                LINE_CLASS,
                bounded_line_payload(
                    plane_uv(from, origin, u_axis, v_axis),
                    plane_uv(to, origin, u_axis, v_axis),
                    edge.param_range.expect("validated edge domain"),
                    2,
                ),
            )
        }
        CurveGeometry::Nurbs(nurbs) => {
            let mut projected = nurbs.clone();
            projected.control_points = nurbs
                .control_points
                .iter()
                .map(|point| {
                    let uv = plane_uv(*point, origin, u_axis, v_axis);
                    cadmpeg_ir::math::Point3::new(uv[0], uv[1], 0.0)
                })
                .collect();
            if sense == Sense::Reversed {
                projected.control_points.reverse();
                if let Some(weights) = &mut projected.weights {
                    weights.reverse();
                }
                let sum = projected.knots[projected.degree as usize]
                    + projected.knots[projected.control_points.len()];
                projected.knots = projected
                    .knots
                    .iter()
                    .rev()
                    .map(|knot| sum - knot)
                    .collect();
                canonicalize_native_curve_knots(&mut projected, &curve.id.0)?;
            }
            (
                NURBS_CURVE_CLASS,
                nurbs_curve_payload_dimension(&projected, 2),
            )
        }
        _ => unreachable!("validated writable Brep curve"),
    })
}

fn canonicalize_native_curve_knots(
    curve: &mut cadmpeg_ir::geometry::NurbsCurve,
    id: &str,
) -> Result<(), CodecError> {
    let order = curve.degree as usize + 1;
    let count = curve.control_points.len();
    let stored = curve.knots[1..curve.knots.len() - 1].to_vec();
    curve.knots = crate::surfaces::reconstruct_knots(&stored, order, count)
        .map_err(|error| CodecError::Malformed(format!("curve {id}: {error}")))?;
    Ok(())
}

fn brep_c2_curve(
    model: &cadmpeg_ir::document::Model,
    edge: &cadmpeg_ir::topology::Edge,
    coedge: &cadmpeg_ir::topology::Coedge,
    origin: cadmpeg_ir::math::Point3,
    u_axis: cadmpeg_ir::math::Vector3,
    v_axis: cadmpeg_ir::math::Vector3,
) -> Result<([u8; 16], Vec<u8>), CodecError> {
    let generated =
        generated_projected_brep_c2_curve(model, edge, coedge.sense, origin, u_axis, v_axis)?;
    if coedge.pcurves.is_empty() {
        return Ok(generated);
    }
    let explicit = explicit_brep_c2_curve(model, edge, coedge)?;
    if explicit != generated {
        let id = coedge
            .pcurves
            .first()
            .map(|use_| &use_.pcurve)
            .expect("explicit pcurve");
        return Err(CodecError::Malformed(format!(
            "pcurve {} does not exactly match its directed planar C3 projection",
            id.0
        )));
    }
    Ok(explicit)
}

fn explicit_brep_c2_curve(
    model: &cadmpeg_ir::document::Model,
    edge: &cadmpeg_ir::topology::Edge,
    coedge: &cadmpeg_ir::topology::Coedge,
) -> Result<([u8; 16], Vec<u8>), CodecError> {
    let pcurve_id = coedge
        .pcurves
        .first()
        .map(|use_| &use_.pcurve)
        .ok_or_else(|| {
            CodecError::NotImplemented(format!("coedge {} has no explicit pcurve", coedge.id.0))
        })?;
    let pcurve = model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id == *pcurve_id)
        .ok_or_else(|| CodecError::Malformed(format!("pcurve {} is missing", pcurve_id.0)))?;
    if pcurve.wrapper_reversed == Some(true)
        || pcurve.native_tail_flags.is_some()
        || pcurve
            .parameter_range
            .is_some_and(|range| Some(range) != edge.param_range)
        || pcurve
            .fit_tolerance
            .is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return Err(CodecError::NotImplemented(format!(
            "pcurve {} has unsupported wrapper, tail, domain, or tolerance state",
            pcurve.id.0
        )));
    }
    let domain = edge.param_range.expect("validated edge domain");
    match &pcurve.geometry {
        cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction } => {
            if !origin.u.is_finite()
                || !origin.v.is_finite()
                || !direction.u.is_finite()
                || !direction.v.is_finite()
                || direction.u == 0.0 && direction.v == 0.0
            {
                return Err(CodecError::Malformed(format!(
                    "pcurve {} has invalid line geometry",
                    pcurve.id.0
                )));
            }
            let from = [
                origin.u + direction.u * domain[0],
                origin.v + direction.v * domain[0],
                0.0,
            ];
            let to = [
                origin.u + direction.u * domain[1],
                origin.v + direction.v * domain[1],
                0.0,
            ];
            Ok((LINE_CLASS, bounded_line_payload(from, to, domain, 2)))
        }
        cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } => {
            let curve = cadmpeg_ir::geometry::NurbsCurve {
                degree: *degree,
                knots: knots.clone(),
                control_points: control_points
                    .iter()
                    .map(|point| cadmpeg_ir::math::Point3::new(point.u, point.v, 0.0))
                    .collect(),
                weights: weights.clone(),
                periodic: *periodic,
            };
            check_nurbs_curve(&pcurve.id.0, &curve)?;
            let count = curve.control_points.len();
            if curve.periodic || [curve.knots[curve.degree as usize], curve.knots[count]] != domain
            {
                return Err(CodecError::NotImplemented(format!(
                    "pcurve {} is not a nonperiodic full-domain NURBS curve",
                    pcurve.id.0
                )));
            }
            Ok((NURBS_CURVE_CLASS, nurbs_curve_payload_dimension(&curve, 2)))
        }
        _ => Err(CodecError::NotImplemented(format!(
            "pcurve {} geometry is not writable as Rhino Brep trim geometry",
            pcurve.id.0
        ))),
    }
}

fn validate_brep_pcurve_ownership(
    model: &cadmpeg_ir::document::Model,
    coedges: &[&cadmpeg_ir::topology::Coedge],
) -> Result<(), CodecError> {
    let mut owned = std::collections::BTreeSet::new();
    for coedge in coedges {
        for pcurve_use in &coedge.pcurves {
            let id = &pcurve_use.pcurve;
            if !owned.insert(id.0.clone()) {
                return Err(CodecError::NotImplemented(format!(
                    "pcurve {} is shared by multiple coedges",
                    id.0
                )));
            }
            if !model.pcurves.iter().any(|pcurve| pcurve.id == *id) {
                return Err(CodecError::Malformed(format!("pcurve {} is missing", id.0)));
            }
        }
    }
    if owned.len() != model.pcurves.len() {
        return Err(CodecError::NotImplemented(
            "orphan Brep pcurves are not writable".into(),
        ));
    }
    Ok(())
}

fn brep_pcurve_fit_tolerance(
    model: &cadmpeg_ir::document::Model,
    coedge: &cadmpeg_ir::topology::Coedge,
) -> f64 {
    coedge
        .pcurves
        .first()
        .map(|pcurve_use| &pcurve_use.pcurve)
        .and_then(|id| model.pcurves.iter().find(|pcurve| pcurve.id == *id))
        .and_then(|pcurve| pcurve.fit_tolerance)
        .unwrap_or(0.0)
}

fn validate_nurbs_trim_loop(
    model: &cadmpeg_ir::document::Model,
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    face_tolerance: f64,
    edges: &[&cadmpeg_ir::topology::Edge],
    coedges: &[&cadmpeg_ir::topology::Coedge],
) -> Result<(), CodecError> {
    use cadmpeg_ir::eval::{curve_point, nurbs_surface_point, pcurve_uv};
    use cadmpeg_ir::topology::Sense;

    let u_count = surface.u_count as usize;
    let v_count = surface.v_count as usize;
    let u_domain = [
        surface.u_knots[surface.u_degree as usize],
        surface.u_knots[u_count],
    ];
    let v_domain = [
        surface.v_knots[surface.v_degree as usize],
        surface.v_knots[v_count],
    ];
    for (edge, coedge) in edges.iter().zip(coedges) {
        explicit_brep_c2_curve(model, edge, coedge)?;
        let pcurve_id = coedge
            .pcurves
            .first()
            .map(|use_| &use_.pcurve)
            .expect("explicit NURBS-face pcurve");
        let pcurve = model
            .pcurves
            .iter()
            .find(|pcurve| pcurve.id == *pcurve_id)
            .expect("validated NURBS-face pcurve");
        let domain = edge.param_range.expect("validated edge domain");
        let uv_epsilon = 1.0e-10
            * u_domain
                .into_iter()
                .chain(v_domain)
                .map(f64::abs)
                .fold(1.0_f64, f64::max);
        let inside_domain = |u: f64, v: f64| {
            u >= u_domain[0] - uv_epsilon
                && u <= u_domain[1] + uv_epsilon
                && v >= v_domain[0] - uv_epsilon
                && v <= v_domain[1] + uv_epsilon
        };
        let control_hull_inside = match &pcurve.geometry {
            cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction } => {
                domain.into_iter().all(|parameter| {
                    inside_domain(
                        origin.u + direction.u * parameter,
                        origin.v + direction.v * parameter,
                    )
                })
            }
            cadmpeg_ir::geometry::PcurveGeometry::Nurbs { control_points, .. } => control_points
                .iter()
                .all(|point| inside_domain(point.u, point.v)),
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "pcurve {} geometry is not writable on a Rhino NURBS face",
                    pcurve.id.0
                )))
            }
        };
        if !control_hull_inside {
            return Err(CodecError::Malformed(format!(
                "pcurve {} leaves its NURBS surface parameter domain",
                pcurve.id.0
            )));
        }
        let curve = model
            .curves
            .iter()
            .find(|curve| edge.curve.as_ref() == Some(&curve.id))
            .expect("validated edge curve");

        let mut breaks = vec![domain[0], domain[1]];
        if let CurveGeometry::Nurbs(nurbs) = &curve.geometry {
            breaks.extend(
                nurbs
                    .knots
                    .iter()
                    .copied()
                    .filter(|value| *value > domain[0] && *value < domain[1])
                    .map(|value| {
                        if coedge.sense == Sense::Forward {
                            value
                        } else {
                            domain[0] + domain[1] - value
                        }
                    }),
            );
        }
        if let cadmpeg_ir::geometry::PcurveGeometry::Nurbs { knots, .. } = &pcurve.geometry {
            breaks.extend(
                knots
                    .iter()
                    .copied()
                    .filter(|value| *value > domain[0] && *value < domain[1]),
            );
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();

        let tolerance = face_tolerance
            .max(edge.tolerance.unwrap_or(0.0))
            .max(pcurve.fit_tolerance.unwrap_or(0.0))
            .max(1.0e-10);
        for span in breaks.windows(2) {
            for step in 0..=16 {
                let fraction = f64::from(step) / 16.0;
                let parameter = span[0] + (span[1] - span[0]) * fraction;
                let uv = pcurve_uv(&pcurve.geometry, parameter).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "pcurve {} cannot be evaluated over its edge domain",
                        pcurve.id.0
                    ))
                })?;
                if uv.u < u_domain[0] - uv_epsilon
                    || uv.u > u_domain[1] + uv_epsilon
                    || uv.v < v_domain[0] - uv_epsilon
                    || uv.v > v_domain[1] + uv_epsilon
                {
                    return Err(CodecError::Malformed(format!(
                        "pcurve {} leaves its NURBS surface parameter domain",
                        pcurve.id.0
                    )));
                }
                let mapped = nurbs_surface_point(surface, uv.u, uv.v).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "pcurve {} cannot be evaluated through its NURBS surface",
                        pcurve.id.0
                    ))
                })?;
                let curve_parameter = if coedge.sense == Sense::Forward {
                    parameter
                } else {
                    domain[0] + domain[1] - parameter
                };
                let edge_point =
                    curve_point(&curve.geometry, curve_parameter).ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "edge curve {} cannot be evaluated over its edge domain",
                            curve.id.0
                        ))
                    })?;
                let distance = ((mapped.x - edge_point.x).powi(2)
                    + (mapped.y - edge_point.y).powi(2)
                    + (mapped.z - edge_point.z).powi(2))
                .sqrt();
                if !distance.is_finite() || distance > tolerance {
                    return Err(CodecError::Malformed(format!(
                        "pcurve {} misses directed edge curve {} by {distance}",
                        pcurve.id.0, curve.id.0
                    )));
                }
            }
        }
    }
    Ok(())
}

fn vertex_point(
    model: &cadmpeg_ir::document::Model,
    vertex_id: &cadmpeg_ir::ids::VertexId,
) -> Option<cadmpeg_ir::math::Point3> {
    let vertex = model
        .vertices
        .iter()
        .find(|vertex| vertex.id == *vertex_id)?;
    model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .map(|point| point.position)
}

fn plane_uv(
    point: cadmpeg_ir::math::Point3,
    origin: cadmpeg_ir::math::Point3,
    u: cadmpeg_ir::math::Vector3,
    v: cadmpeg_ir::math::Vector3,
) -> [f64; 3] {
    let delta = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
    [
        delta[0] * u.x + delta[1] * u.y + delta[2] * u.z,
        delta[0] * v.x + delta[1] * v.y + delta[2] * v.z,
        0.0,
    ]
}

fn bounded_line_payload(from: [f64; 3], to: [f64; 3], domain: [f64; 2], dimension: i32) -> Vec<u8> {
    let mut payload = vec![0x10];
    for value in from.into_iter().chain(to).chain(domain) {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(dimension.to_le_bytes());
    payload
}

fn polymorphic_array(children: &[([u8; 16], Vec<u8>)]) -> Vec<u8> {
    let mut body = vec![0x10];
    body.extend((children.len() as i32).to_le_bytes());
    let mut direct = body.clone();
    for (class, payload) in children {
        body.extend(1_i32.to_le_bytes());
        direct.extend(1_i32.to_le_bytes());
        body.extend(class_wrapper(*class, payload));
    }
    crc_chunk_with_direct(0x4000_8000, &body, &direct)
}

fn raw_array(records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = vec![0x10];
    body.extend((records.len() as i32).to_le_bytes());
    body.extend(records.concat());
    crc_chunk(0x4000_8000, &body)
}

fn indexes(values: &[i32]) -> Vec<u8> {
    let mut bytes = (values.len() as i32).to_le_bytes().to_vec();
    bytes.extend(values.iter().flat_map(|value| value.to_le_bytes()));
    bytes
}

fn class_wrapper(class_uuid: [u8; 16], payload: &[u8]) -> Vec<u8> {
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(TCODE_CLASS_UUID, &uuid_body);
    let data = crc_chunk(TCODE_CLASS_DATA, payload);
    let end = short_chunk(TCODE_CLASS_END, 0);
    long_chunk(TCODE_CLASS_WRAPPER, &[uuid, data, end].concat())
}

fn check_mesh(mesh: &cadmpeg_ir::tessellation::Tessellation) -> Result<(), CodecError> {
    let vertex_count = mesh.vertices.len();
    if vertex_count == 0 || vertex_count > (1 << 24) || mesh.triangles.len() > (1 << 24) {
        return Err(CodecError::Malformed(format!(
            "mesh {} has invalid native counts",
            mesh.id
        )));
    }
    if mesh.body.is_some() || !mesh.strip_lengths.is_empty() {
        return Err(CodecError::NotImplemented(format!(
            "mesh {} uses body binding or strips not yet writable",
            mesh.id
        )));
    }
    if !mesh.normals.is_empty() && mesh.normals.len() != vertex_count {
        return Err(CodecError::Malformed(format!(
            "mesh {} normal count mismatch",
            mesh.id
        )));
    }
    if mesh.vertices.iter().any(|p| {
        !p.x.is_finite()
            || !p.y.is_finite()
            || !p.z.is_finite()
            || !(p.x as f32).is_finite()
            || !(p.y as f32).is_finite()
            || !(p.z as f32).is_finite()
    }) || mesh.normals.iter().any(|n| {
        !n.x.is_finite()
            || !n.y.is_finite()
            || !n.z.is_finite()
            || !(n.x as f32).is_finite()
            || !(n.y as f32).is_finite()
            || !(n.z as f32).is_finite()
    }) {
        return Err(CodecError::Malformed(format!(
            "mesh {} contains non-finite native values",
            mesh.id
        )));
    }
    if mesh
        .triangles
        .iter()
        .flatten()
        .any(|index| *index as usize >= vertex_count)
    {
        return Err(CodecError::Malformed(format!(
            "mesh {} index is out of range",
            mesh.id
        )));
    }
    let mut kinds = std::collections::BTreeSet::new();
    for channel in &mesh.channels {
        let expected = match channel.kind {
            CHANNEL_UV => 8,
            CHANNEL_COLOR => 4,
            CHANNEL_SURFACE_PARAMETERS | CHANNEL_CURVATURE => 16,
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "mesh {} channel kind {:#x} is not writable",
                    mesh.id, channel.kind
                )))
            }
        };
        if !kinds.insert(channel.kind)
            || channel.flags != 0
            || channel.item_size != expected
            || channel.count as usize != vertex_count
            || channel.data.len() != vertex_count * expected as usize
        {
            return Err(CodecError::Malformed(format!(
                "mesh {} channel {:#x} has invalid metadata",
                mesh.id, channel.kind
            )));
        }
    }
    Ok(())
}

struct PointGroup {
    points: Vec<cadmpeg_ir::math::Point3>,
    identity: String,
    name: Option<String>,
    color: Option<cadmpeg_ir::topology::Color>,
    visible: Option<bool>,
}

type PointGroups = (std::collections::BTreeSet<String>, Vec<PointGroup>);

fn free_vertex_groups(ir: &CadIr) -> Result<PointGroups, CodecError> {
    use cadmpeg_ir::topology::BodyKind;

    let model = &ir.model;
    let mut regions = std::collections::BTreeSet::new();
    let mut shells = std::collections::BTreeSet::new();
    let mut vertices = std::collections::BTreeSet::new();
    let mut points = std::collections::BTreeSet::new();
    let mut groups = Vec::with_capacity(model.bodies.len());
    for body in &model.bodies {
        if body.kind != BodyKind::General || body.regions.len() != 1 || body.transform.is_some() {
            return Err(CodecError::NotImplemented(format!(
                "body {} is not a free-vertex body without placement",
                body.id.0
            )));
        }
        check_object_attributes(&body.id.0, body.name.as_deref(), body.color)?;
        let region = model
            .regions
            .iter()
            .find(|region| region.id == body.regions[0])
            .ok_or_else(|| {
                CodecError::Malformed(format!("body {} region is missing", body.id.0))
            })?;
        if region.body != body.id
            || region.shells.len() != 1
            || !regions.insert(region.id.0.clone())
        {
            return Err(CodecError::Malformed(format!(
                "body {} region graph is invalid",
                body.id.0
            )));
        }
        let shell = model
            .shells
            .iter()
            .find(|shell| shell.id == region.shells[0])
            .ok_or_else(|| CodecError::Malformed(format!("body {} shell is missing", body.id.0)))?;
        if shell.region != region.id
            || !shell.faces.is_empty()
            || !shell.wire_edges.is_empty()
            || shell.free_vertices.is_empty()
            || !shells.insert(shell.id.0.clone())
        {
            return Err(CodecError::Malformed(format!(
                "body {} shell graph is invalid",
                body.id.0
            )));
        }
        let mut group = Vec::with_capacity(shell.free_vertices.len());
        for vertex_id in &shell.free_vertices {
            let vertex = model
                .vertices
                .iter()
                .find(|vertex| vertex.id == *vertex_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("vertex {} is missing", vertex_id.0))
                })?;
            if vertex.tolerance.is_some() || !vertices.insert(vertex.id.0.clone()) {
                return Err(CodecError::NotImplemented(format!(
                    "vertex {} has tolerance or multiple ownership",
                    vertex.id.0
                )));
            }
            let point = model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("point {} is missing", vertex.point.0))
                })?;
            if !points.insert(point.id.0.clone()) {
                return Err(CodecError::NotImplemented(format!(
                    "point {} is shared by multiple free vertices",
                    point.id.0
                )));
            }
            group.push(point.position);
        }
        groups.push(PointGroup {
            points: group,
            identity: body.id.0.clone(),
            name: body.name.clone(),
            color: body.color,
            visible: body.visible,
        });
    }
    if regions.len() != model.regions.len()
        || shells.len() != model.shells.len()
        || vertices.len() != model.vertices.len()
    {
        return Err(CodecError::NotImplemented(
            "orphan region, shell, or vertex topology is not writable".into(),
        ));
    }
    Ok((points, groups))
}

fn check_frame(
    id: &str,
    origin: cadmpeg_ir::math::Point3,
    normal: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
    family: &str,
) -> Result<(), CodecError> {
    let dot = normal.x * x.x + normal.y * x.y + normal.z * x.z;
    if !origin.x.is_finite()
        || !origin.y.is_finite()
        || !origin.z.is_finite()
        || (normal.norm() - 1.0).abs() > 1.0e-10
        || (x.norm() - 1.0).abs() > 1.0e-10
        || dot.abs() > 1.0e-10
    {
        return Err(CodecError::Malformed(format!(
            "{family} {id} has an invalid frame"
        )));
    }
    Ok(())
}

fn check_nurbs_surface(
    id: &str,
    surface: &cadmpeg_ir::geometry::NurbsSurface,
) -> Result<(), CodecError> {
    let u_order = surface.u_degree as usize + 1;
    let v_order = surface.v_degree as usize + 1;
    let u_count = surface.u_count as usize;
    let v_count = surface.v_count as usize;
    let pole_count = u_count.checked_mul(v_count);
    if u_order < 2
        || v_order < 2
        || u_count < u_order
        || v_count < v_order
        || i32::try_from(u_order).is_err()
        || i32::try_from(v_order).is_err()
        || i32::try_from(u_count).is_err()
        || i32::try_from(v_count).is_err()
        || pole_count.is_none_or(|count| i32::try_from(count).is_err())
        || surface.u_knots.len() != u_count + u_order
        || surface.v_knots.len() != v_count + v_order
        || pole_count != Some(surface.control_points.len())
    {
        return Err(CodecError::Malformed(format!(
            "surface {id} has inconsistent NURBS counts"
        )));
    }
    if surface
        .u_knots
        .iter()
        .chain(&surface.v_knots)
        .any(|v| !v.is_finite())
        || surface
            .u_knots
            .windows(2)
            .chain(surface.v_knots.windows(2))
            .any(|v| v[0] > v[1])
        || surface
            .control_points
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
        || surface.weights.as_ref().is_some_and(|w| {
            w.len() != surface.control_points.len() || w.iter().any(|v| !v.is_finite() || *v == 0.0)
        })
    {
        return Err(CodecError::Malformed(format!(
            "surface {id} has invalid NURBS data"
        )));
    }
    check_knot_roundtrip(
        id,
        "surface U",
        &surface.u_knots,
        u_order,
        u_count,
        surface.u_periodic,
    )?;
    check_knot_roundtrip(
        id,
        "surface V",
        &surface.v_knots,
        v_order,
        v_count,
        surface.v_periodic,
    )?;
    Ok(())
}

fn check_nurbs_curve(id: &str, curve: &cadmpeg_ir::geometry::NurbsCurve) -> Result<(), CodecError> {
    let order = curve.degree as usize + 1;
    let count = curve.control_points.len();
    if i32::try_from(order).is_err()
        || i32::try_from(count).is_err()
        || order < 2
        || count < order
        || curve.knots.len() != count + order
    {
        return Err(CodecError::Malformed(format!(
            "curve {id} has inconsistent NURBS counts"
        )));
    }
    if curve.knots.iter().any(|v| !v.is_finite())
        || curve.knots.windows(2).any(|v| v[0] > v[1])
        || curve
            .control_points
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
        || curve
            .weights
            .as_ref()
            .is_some_and(|w| w.len() != count || w.iter().any(|v| !v.is_finite() || *v == 0.0))
    {
        return Err(CodecError::Malformed(format!(
            "curve {id} has invalid NURBS data"
        )));
    }
    check_knot_roundtrip(id, "curve", &curve.knots, order, count, curve.periodic)?;
    Ok(())
}

fn check_knot_roundtrip(
    id: &str,
    direction: &str,
    full: &[f64],
    order: usize,
    count: usize,
    declared_periodic: bool,
) -> Result<(), CodecError> {
    let stored = &full[1..full.len() - 1];
    if stored[order - 2] >= stored[count - 1] {
        return Err(CodecError::Malformed(format!(
            "{direction} {id} has a non-increasing native NURBS domain"
        )));
    }
    let reconstructed = crate::surfaces::reconstruct_knots(stored, order, count)
        .map_err(|error| CodecError::Malformed(format!("{direction} {id}: {error}")))?;
    let periodic = crate::surfaces::periodic_knots(stored, order, count);
    if reconstructed != full || periodic != declared_periodic {
        return Err(CodecError::Malformed(format!(
            "{direction} {id} knot endpoints or periodic flag are not native-canonical"
        )));
    }
    Ok(())
}

fn header(version: u64) -> Result<Vec<u8>, CodecError> {
    let text = version.to_string();
    if text.len() > 8 {
        return Err(CodecError::Malformed(
            "3DM archive version exceeds header field".into(),
        ));
    }
    let mut bytes = MAGIC.to_vec();
    bytes.extend(std::iter::repeat_n(b' ', 8 - text.len()));
    bytes.extend(text.bytes());
    Ok(bytes)
}

fn long_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    bytes.extend((body.len() as i64).to_le_bytes());
    bytes.extend(body);
    bytes
}

fn crc_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
    crc_chunk_with_direct(typecode, body, body)
}

fn crc_chunk_with_direct(typecode: u32, body: &[u8], direct: &[u8]) -> Vec<u8> {
    let mut payload = body.to_vec();
    payload.extend(crc32fast::hash(direct).to_le_bytes());
    long_chunk(typecode, &payload)
}

fn zero_crc_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = body.to_vec();
    payload.extend(0_u32.to_le_bytes());
    long_chunk(typecode, &payload)
}

fn short_chunk(typecode: u32, value: i64) -> Vec<u8> {
    let mut bytes = (typecode | TCODE_SHORT).to_le_bytes().to_vec();
    bytes.extend(value.to_le_bytes());
    bytes
}

fn table(typecode: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = records.concat();
    body.extend(short_chunk(TCODE_ENDOFTABLE, 0));
    long_chunk(typecode, &body)
}

fn units_record(linear: f64, angular: f64) -> Vec<u8> {
    let mut body = 100_i32.to_le_bytes().to_vec();
    body.extend(2_i32.to_le_bytes()); // millimeters
    body.extend(linear.to_le_bytes());
    body.extend(angular.to_le_bytes());
    body.extend(DEFAULT_RELATIVE_TOLERANCE.to_le_bytes());
    crc_chunk(TCODE_UNITS_AND_TOLERANCES, &body)
}

fn point_cloud_payload(points: &[cadmpeg_ir::math::Point3]) -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend((points.len() as i32).to_le_bytes());
    for point in points {
        payload.extend(point.x.to_le_bytes());
        payload.extend(point.y.to_le_bytes());
        payload.extend(point.z.to_le_bytes());
    }
    for value in [
        0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0,
    ] {
        payload.extend(value.to_le_bytes());
    }
    let min = points.iter().fold([f64::INFINITY; 3], |a, p| {
        [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
    });
    let max = points.iter().fold([f64::NEG_INFINITY; 3], |a, p| {
        [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
    });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(0_i32.to_le_bytes());
    payload
}

fn circle_payload(
    center: cadmpeg_ir::math::Point3,
    axis: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
    radius: f64,
) -> Vec<u8> {
    let y = cadmpeg_ir::math::Vector3::new(
        axis.y * x.z - axis.z * x.y,
        axis.z * x.x - axis.x * x.z,
        axis.x * x.y - axis.y * x.x,
    );
    let equation_d = -(axis.x * center.x + axis.y * center.y + axis.z * center.z);
    let mut payload = vec![0x10];
    for value in [
        center.x,
        center.y,
        center.z,
        x.x,
        x.y,
        x.z,
        y.x,
        y.y,
        y.z,
        axis.x,
        axis.y,
        axis.z,
        axis.x,
        axis.y,
        axis.z,
        equation_d,
        radius,
        center.x + radius * x.x,
        center.y + radius * x.y,
        center.z + radius * x.z,
        center.x + radius * y.x,
        center.y + radius * y.y,
        center.z + radius * y.z,
        center.x - radius * x.x,
        center.y - radius * x.y,
        center.z - radius * x.z,
        0.0,
        std::f64::consts::TAU,
        0.0,
        std::f64::consts::TAU,
    ] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(3_i32.to_le_bytes());
    payload
}

fn nurbs_curve_payload(curve: &cadmpeg_ir::geometry::NurbsCurve) -> Vec<u8> {
    nurbs_curve_payload_dimension(curve, 3)
}

fn nurbs_curve_payload_dimension(
    curve: &cadmpeg_ir::geometry::NurbsCurve,
    dimension: i32,
) -> Vec<u8> {
    let rational = i32::from(curve.weights.is_some());
    let order = (curve.degree + 1) as i32;
    let count = curve.control_points.len() as i32;
    let mut payload = vec![0x10];
    for value in [dimension, rational, order, count, 0, 0] {
        payload.extend(value.to_le_bytes());
    }
    let min = curve
        .control_points
        .iter()
        .fold([f64::INFINITY; 3], |a, p| {
            [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
        });
    let max = curve
        .control_points
        .iter()
        .fold([f64::NEG_INFINITY; 3], |a, p| {
            [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
        });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(((curve.knots.len() - 2) as i32).to_le_bytes());
    for knot in &curve.knots[1..curve.knots.len() - 1] {
        payload.extend(knot.to_le_bytes());
    }
    payload.extend(count.to_le_bytes());
    for (index, point) in curve.control_points.iter().enumerate() {
        let weight = curve.weights.as_ref().map_or(1.0, |weights| weights[index]);
        payload.extend((point.x * weight).to_le_bytes());
        payload.extend((point.y * weight).to_le_bytes());
        if dimension == 3 {
            payload.extend((point.z * weight).to_le_bytes());
        }
        if rational != 0 {
            payload.extend(weight.to_le_bytes());
        }
    }
    payload
}

fn plane_surface_payload(
    origin: cadmpeg_ir::math::Point3,
    normal: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
) -> Vec<u8> {
    let y = cross(normal, x);
    let d = -(normal.x * origin.x + normal.y * origin.y + normal.z * origin.z);
    let mut payload = vec![0x10];
    for value in [
        origin.x, origin.y, origin.z, x.x, x.y, x.z, y.x, y.y, y.z, normal.x, normal.y, normal.z,
        normal.x, normal.y, normal.z, d, -1.0, 1.0, -1.0, 1.0,
    ] {
        payload.extend(value.to_le_bytes());
    }
    payload
}

fn nurbs_surface_payload(surface: &cadmpeg_ir::geometry::NurbsSurface) -> Vec<u8> {
    let rational = i32::from(surface.weights.is_some());
    let mut payload = vec![0x10];
    for value in [
        3,
        rational,
        (surface.u_degree + 1) as i32,
        (surface.v_degree + 1) as i32,
        surface.u_count as i32,
        surface.v_count as i32,
        0,
        0,
    ] {
        payload.extend(value.to_le_bytes());
    }
    let min = surface
        .control_points
        .iter()
        .fold([f64::INFINITY; 3], |a, p| {
            [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
        });
    let max = surface
        .control_points
        .iter()
        .fold([f64::NEG_INFINITY; 3], |a, p| {
            [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
        });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    for knots in [&surface.u_knots, &surface.v_knots] {
        payload.extend(((knots.len() - 2) as i32).to_le_bytes());
        for knot in &knots[1..knots.len() - 1] {
            payload.extend(knot.to_le_bytes());
        }
    }
    payload.extend((surface.control_points.len() as i32).to_le_bytes());
    for (index, point) in surface.control_points.iter().enumerate() {
        let weight = surface
            .weights
            .as_ref()
            .map_or(1.0, |weights| weights[index]);
        payload.extend((point.x * weight).to_le_bytes());
        payload.extend((point.y * weight).to_le_bytes());
        payload.extend((point.z * weight).to_le_bytes());
        if rational != 0 {
            payload.extend(weight.to_le_bytes());
        }
    }
    payload
}

fn cross(a: cadmpeg_ir::math::Vector3, b: cadmpeg_ir::math::Vector3) -> cadmpeg_ir::math::Vector3 {
    cadmpeg_ir::math::Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

struct MeshPayload {
    body: Vec<u8>,
    direct: Vec<u8>,
}

fn mesh_payload(
    mesh: &cadmpeg_ir::tessellation::Tessellation,
    archive_version: u64,
) -> MeshPayload {
    let minor = if archive_version == 50 { 5_u8 } else { 8_u8 };
    let mut payload = vec![0x30 | minor];
    payload.extend((mesh.vertices.len() as i32).to_le_bytes());
    payload.extend((mesh.triangles.len() as i32).to_le_bytes());
    for _ in 0..4 {
        payload.extend(0.0_f64.to_le_bytes());
        payload.extend(1.0_f64.to_le_bytes());
    }
    payload.extend([0_u8; 16]);
    payload.extend([0_u8; 16 * 4]);
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0_u8; 5]);

    let width = if mesh.vertices.len() < 256 {
        1_i32
    } else if mesh.vertices.len() < 65_536 {
        2_i32
    } else {
        4_i32
    };
    payload.extend(width.to_le_bytes());
    for triangle in &mesh.triangles {
        for index in [triangle[0], triangle[1], triangle[2], triangle[2]] {
            match width {
                1 => payload.push(index as u8),
                2 => payload.extend((index as u16).to_le_bytes()),
                4 => payload.extend(index.to_le_bytes()),
                _ => unreachable!(),
            }
        }
    }

    let float_vertices = mesh
        .vertices
        .iter()
        .flat_map(|point| {
            [point.x as f32, point.y as f32, point.z as f32]
                .into_iter()
                .flat_map(f32::to_le_bytes)
        })
        .collect::<Vec<_>>();
    let normals = mesh
        .normals
        .iter()
        .flat_map(|normal| {
            [normal.x as f32, normal.y as f32, normal.z as f32]
                .into_iter()
                .flat_map(f32::to_le_bytes)
        })
        .collect::<Vec<_>>();
    for data in [
        &float_vertices[..],
        &normals[..],
        mesh_channel(mesh, CHANNEL_UV),
        mesh_channel(mesh, CHANNEL_CURVATURE),
        mesh_channel(mesh, CHANNEL_COLOR),
    ] {
        payload.extend(mesh_buffer(data));
    }
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0_u8; 16]);
    payload.extend(mesh_buffer(mesh_channel(mesh, CHANNEL_SURFACE_PARAMETERS)));
    let mut direct = payload.clone();
    payload.extend(mesh_mapping_tag());
    payload.extend([0_u8; 3]);
    direct.extend([0_u8; 3]);
    if minor >= 6 {
        payload.push(0);
        direct.push(0);
    }
    if minor >= 7 {
        payload.push(1);
        direct.push(1);
        let doubles = mesh
            .vertices
            .iter()
            .flat_map(|point| {
                [point.x, point.y, point.z]
                    .into_iter()
                    .flat_map(f64::to_le_bytes)
            })
            .collect::<Vec<_>>();
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        body.extend((mesh.vertices.len() as u32).to_le_bytes());
        body.extend(mesh_buffer(&doubles));
        payload.extend(crc_chunk(0x4000_8000, &body));
    }
    if minor >= 8 {
        let min = mesh.vertices.iter().fold([f64::INFINITY; 3], |a, point| {
            [a[0].min(point.x), a[1].min(point.y), a[2].min(point.z)]
        });
        let max = mesh
            .vertices
            .iter()
            .fold([f64::NEG_INFINITY; 3], |a, point| {
                [a[0].max(point.x), a[1].max(point.y), a[2].max(point.z)]
            });
        let bounding_box = min
            .into_iter()
            .chain(max)
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        payload.extend(&bounding_box);
        direct.extend(bounding_box);
    }
    MeshPayload {
        body: payload,
        direct,
    }
}

fn mesh_mapping_tag() -> Vec<u8> {
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.extend([0_u8; 16]);
    body.extend(0_i32.to_le_bytes());
    for value in [
        1.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ] {
        body.extend(value.to_le_bytes());
    }
    crc_chunk(0x4000_8000, &body)
}

fn mesh_channel(mesh: &cadmpeg_ir::tessellation::Tessellation, kind: u32) -> &[u8] {
    mesh.channels
        .iter()
        .find(|channel| channel.kind == kind)
        .map_or(&[], |channel| channel.data.as_slice())
}

fn mesh_buffer(data: &[u8]) -> Vec<u8> {
    let mut result = (data.len() as u32).to_le_bytes().to_vec();
    if !data.is_empty() {
        result.extend(crc32fast::hash(data).to_le_bytes());
        result.push(0);
        result.extend(data);
    }
    result
}

fn attributed_object_record(
    object_type: i64,
    class_uuid: [u8; 16],
    payload: &[u8],
    identity: &str,
    name: Option<&str>,
    color: Option<cadmpeg_ir::topology::Color>,
    visible: Option<bool>,
) -> Result<Vec<u8>, CodecError> {
    check_object_attributes(identity, name, color)?;
    Ok(framed_object_record(
        object_type,
        class_uuid,
        payload,
        None,
        Some(object_attributes_payload(identity, name, color, visible)),
    ))
}

fn mesh_object_record(payload: &MeshPayload, identity: &str) -> Result<Vec<u8>, CodecError> {
    check_object_attributes(identity, None, None)?;
    Ok(framed_object_record(
        0x20,
        MESH_CLASS,
        &payload.body,
        Some(&payload.direct),
        Some(object_attributes_payload(identity, None, None, None)),
    ))
}

fn brep_object_record(
    payload: &BrepPayload,
    identity: &str,
    name: Option<&str>,
    color: Option<cadmpeg_ir::topology::Color>,
    visible: Option<bool>,
) -> Result<Vec<u8>, CodecError> {
    check_object_attributes(identity, name, color)?;
    Ok(framed_object_record(
        0x10,
        BREP_CLASS,
        &payload.body,
        Some(&payload.direct),
        Some(object_attributes_payload(identity, name, color, visible)),
    ))
}

fn framed_object_record(
    object_type: i64,
    class_uuid: [u8; 16],
    payload: &[u8],
    direct_class_data: Option<&[u8]>,
    attributes: Option<Vec<u8>>,
) -> Vec<u8> {
    let object_type = short_chunk(TCODE_OBJECT_RECORD_TYPE, object_type);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(TCODE_CLASS_UUID, &uuid_body);
    let class_data = if let Some(direct) = direct_class_data {
        crc_chunk_with_direct(TCODE_CLASS_DATA, payload, direct)
    } else {
        crc_chunk(TCODE_CLASS_DATA, payload)
    };
    let class_end = short_chunk(TCODE_CLASS_END, 0);
    let class = long_chunk(TCODE_CLASS_WRAPPER, &[uuid, class_data, class_end].concat());
    let object_end = short_chunk(TCODE_OBJECT_RECORD_END, 0);
    let mut body = [object_type, class].concat();
    if let Some(attributes) = attributes {
        body.extend(crc_chunk(TCODE_OBJECT_RECORD_ATTRIBUTES, &attributes));
    }
    body.extend(object_end);
    zero_crc_chunk(TCODE_OBJECT_RECORD, &body)
}

fn check_object_attributes(
    identity: &str,
    name: Option<&str>,
    color: Option<cadmpeg_ir::topology::Color>,
) -> Result<(), CodecError> {
    if identity.is_empty() || name.is_some_and(|value| value.contains('\0')) {
        return Err(CodecError::Malformed(format!(
            "object {identity} has an invalid identity or name"
        )));
    }
    if color.is_some_and(|value| {
        [value.r, value.g, value.b, value.a]
            .into_iter()
            .any(|channel| !channel.is_finite() || !(0.0..=1.0).contains(&channel))
    }) {
        return Err(CodecError::Malformed(format!(
            "object {identity} has an invalid color"
        )));
    }
    Ok(())
}

fn object_attributes_payload(
    identity: &str,
    name: Option<&str>,
    color: Option<cadmpeg_ir::topology::Color>,
    visible: Option<bool>,
) -> Vec<u8> {
    let digest = Sha256::digest(identity.as_bytes());
    let mut payload = vec![0x20];
    payload.extend(&digest[..16]);
    payload.extend(0_i32.to_le_bytes());
    if let Some(name) = name {
        payload.push(1);
        payload.extend(utf16(name));
    }
    if let Some(color) = color {
        payload.push(6);
        payload.extend([
            unit_color_channel(color.r),
            unit_color_channel(color.g),
            unit_color_channel(color.b),
            unit_color_channel(1.0 - color.a),
        ]);
        payload.extend([13, 1]);
    }
    if let Some(visible) = visible {
        payload.extend([11, u8::from(visible)]);
    }
    payload.push(0);
    payload
}

fn default_layer_payload() -> Vec<u8> {
    let mut payload = vec![0x15];
    payload.extend(0_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend([0_u8, 0, 0, 0]);
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(utf16("Default"));
    payload.push(1);
    payload.extend((-1_i32).to_le_bytes());
    payload.extend([0_u8, 0, 0, 0]);
    payload.extend(0.0_f64.to_le_bytes());
    payload.push(0);
    payload.extend(&Sha256::digest(b"cadmpeg:default-layer")[..16]);
    payload
}

fn unit_color_channel(value: f32) -> u8 {
    (value * 255.0).round() as u8
}

fn utf16(value: &str) -> Vec<u8> {
    let mut units = value.encode_utf16().collect::<Vec<_>>();
    units.push(0);
    let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
    for unit in units {
        bytes.extend(unit.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::ids::PointId;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Point;
    use cadmpeg_ir::units::Units;
    use sha2::{Digest, Sha256};

    use super::{
        vertex_point, CHANNEL_COLOR, CHANNEL_CURVATURE, CHANNEL_SURFACE_PARAMETERS, CHANNEL_UV,
    };
    use crate::{RhinoArchiveVersion, RhinoCodec, RhinoEncoder};

    #[test]
    fn source_less_points_round_trip_across_target_versions() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.points.push(Point {
            id: PointId("point:a".into()),
            position: Point3::new(1.25, -2.5, 3.75),
            source_object: None,
        });

        for (version, value) in [
            (RhinoArchiveVersion::V5, "50"),
            (RhinoArchiveVersion::V6, "60"),
            (RhinoArchiveVersion::V7, "70"),
            (RhinoArchiveVersion::V8, "80"),
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            assert_eq!(std::str::from_utf8(&bytes[24..32]).unwrap().trim(), value);
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert_eq!(decoded.ir.model.points.len(), 1);
            assert_eq!(
                decoded.ir.model.points[0].position,
                Point3::new(1.25, -2.5, 3.75)
            );
        }
    }

    #[test]
    fn coarse_absolute_tolerance_writes_valid_independent_relative_tolerance() {
        let mut ir = CadIr::empty(Units::default());
        ir.tolerances.linear = 2.0;
        ir.model.points.push(Point {
            id: PointId("point:coarse-tolerance".into()),
            position: Point3::new(1.0, 2.0, 3.0),
            source_object: None,
        });

        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut bytes)
            .expect("coarse absolute tolerance is writable");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("generated settings record remains valid");

        assert_eq!(decoded.ir.tolerances.linear, 2.0);
        assert!(decoded
            .report
            .losses
            .iter()
            .all(|loss| !loss.message.contains("relative tolerance")));
    }

    #[test]
    fn invalid_archive_tolerances_are_rejected_before_output() {
        for (linear, angular) in [
            (0.0, 1.0e-10),
            (f64::INFINITY, 1.0e-10),
            (1.0e-6, 0.0),
            (1.0e-6, std::f64::consts::PI.next_up()),
        ] {
            let mut ir = CadIr::empty(Units::default());
            ir.tolerances.linear = linear;
            ir.tolerances.angular = angular;
            let mut output = vec![0xaa];
            let error = RhinoEncoder::new(RhinoArchiveVersion::V8)
                .encode(&ir, &mut output)
                .expect_err("invalid tolerance must not be serialized");
            assert!(matches!(error, cadmpeg_ir::codec::CodecError::Malformed(_)));
            assert_eq!(output, [0xaa]);
        }
    }

    #[test]
    fn rejection_occurs_before_output() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: cadmpeg_ir::ids::CurveId("curve:a".into()),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Degenerate {
                point: Point3::new(0.0, 0.0, 0.0),
            },
            source_object: None,
        });
        let mut output = vec![0xaa];
        assert!(RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut output)
            .is_err());
        assert_eq!(output, [0xaa]);
    }

    #[test]
    fn source_less_circle_round_trips_with_its_frame() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: cadmpeg_ir::ids::CurveId("curve:circle".into()),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Circle {
                center: Point3::new(1.0, 2.0, 3.0),
                axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
                ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                radius: 4.0,
            },
            source_object: None,
        });
        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut bytes)
            .unwrap();
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir.model.curves.len(), 1);
        assert_eq!(
            decoded.ir.model.curves[0].geometry,
            ir.model.curves[0].geometry
        );
        let digest = Sha256::digest(b"curve:circle");
        let expected = crate::wire::Uuid::from_wire(digest[..16].try_into().unwrap()).to_string();
        assert_eq!(
            decoded.ir.model.curves[0]
                .source_object
                .as_ref()
                .expect("generated object identity")
                .object_id,
            expected
        );
    }

    #[test]
    fn rational_nurbs_curve_round_trips_homogeneous_poles() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: cadmpeg_ir::ids::CurveId("curve:nurbs".into()),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(
                cadmpeg_ir::geometry::NurbsCurve {
                    degree: 2,
                    knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    control_points: vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(1.0, 2.0, 0.0),
                        Point3::new(3.0, 0.0, 0.0),
                    ],
                    weights: Some(vec![1.0, 0.5, 1.0]),
                    periodic: false,
                },
            ),
            source_object: None,
        });
        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut bytes)
            .unwrap();
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            decoded.ir.model.curves[0].geometry,
            ir.model.curves[0].geometry
        );
    }

    #[test]
    fn reversed_unclamped_nurbs_knots_are_native_canonical() {
        let mut curve = cadmpeg_ir::geometry::NurbsCurve {
            degree: 2,
            knots: vec![-3.0, 0.0, 1.0, 5.0, 8.0, 9.0, 10.0, 11.0, 14.0],
            control_points: (0..6)
                .map(|index| Point3::new(f64::from(index), 0.0, 0.0))
                .collect(),
            weights: None,
            periodic: false,
        };
        super::canonicalize_native_curve_knots(&mut curve, "reversed")
            .expect("reflected stored knots reconstruct");

        assert_eq!(
            curve.knots,
            [-1.0, 0.0, 1.0, 5.0, 8.0, 9.0, 10.0, 11.0, 12.0]
        );
        super::check_knot_roundtrip("reversed", "curve", &curve.knots, 3, 6, curve.periodic)
            .expect("canonicalized knots serialize without another change");
    }

    #[test]
    fn free_plane_and_rational_nurbs_surface_round_trip() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId("surface:plane".into()),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Plane {
                origin: Point3::new(1.0, 2.0, 3.0),
                normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                u_axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            },
            source_object: None,
        });
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId("surface:nurbs".into()),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(
                cadmpeg_ir::geometry::NurbsSurface {
                    u_degree: 1,
                    v_degree: 1,
                    u_knots: vec![0.0, 0.0, 1.0, 1.0],
                    v_knots: vec![2.0, 2.0, 5.0, 5.0],
                    u_count: 2,
                    v_count: 2,
                    control_points: vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(0.0, 2.0, 0.0),
                        Point3::new(3.0, 0.0, 1.0),
                        Point3::new(3.0, 2.0, 1.0),
                    ],
                    weights: Some(vec![1.0, 0.75, 0.5, 1.0]),
                    u_periodic: false,
                    v_periodic: false,
                },
            ),
            source_object: None,
        });
        ir.finalize();
        let expected = ir
            .model
            .surfaces
            .iter()
            .map(|s| s.geometry.clone())
            .collect::<Vec<_>>();
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let actual = decoded
                .ir
                .model
                .surfaces
                .iter()
                .map(|s| s.geometry.clone())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn standalone_mesh_round_trips_across_archive_versions() {
        let mut ir = CadIr::empty(Units::default());
        ir.model
            .tessellations
            .push(cadmpeg_ir::tessellation::Tessellation {
                id: "cadir:model:tessellation#mesh".into(),
                body: None,
                faces: Vec::new(),
                chordal_deflection: None,
                source_object: None,
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(2.0, 0.0, 0.0),
                    Point3::new(0.0, 3.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
                strip_lengths: Vec::new(),
                normals: vec![cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0); 3],
                channels: Vec::new(),
            });
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert!(
                decoded
                    .report
                    .losses
                    .iter()
                    .all(|loss| !loss.message.contains("CRC mismatch")),
                "{version:?}: {:?}",
                decoded.report.losses
            );
            assert_eq!(decoded.ir.model.tessellations.len(), 1);
            let actual = &decoded.ir.model.tessellations[0];
            assert_eq!(actual.vertices, ir.model.tessellations[0].vertices);
            assert_eq!(actual.triangles, ir.model.tessellations[0].triangles);
            assert_eq!(actual.normals, ir.model.tessellations[0].normals);
        }
    }

    #[test]
    fn mesh_precision_is_target_specific_and_reported() {
        let mut ir = CadIr::empty(Units::default());
        ir.model
            .tessellations
            .push(cadmpeg_ir::tessellation::Tessellation {
                id: "cadir:model:tessellation#precision".into(),
                body: None,
                faces: Vec::new(),
                chordal_deflection: None,
                source_object: None,
                vertices: vec![
                    Point3::new(0.1, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
                strip_lengths: Vec::new(),
                normals: Vec::new(),
                channels: Vec::new(),
            });
        let mut v5 = Vec::new();
        let v5_report = RhinoEncoder::new(RhinoArchiveVersion::V5)
            .encode(&ir, &mut v5)
            .unwrap();
        assert_eq!(v5_report.losses.len(), 1);
        let decoded_v5 = RhinoCodec
            .decode(&mut Cursor::new(v5), &DecodeOptions::default())
            .unwrap();
        assert_ne!(decoded_v5.ir.model.tessellations[0].vertices[0].x, 0.1);
        let mut v8 = Vec::new();
        let v8_report = RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut v8)
            .unwrap();
        assert!(v8_report.losses.is_empty());
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(v8), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir.model.tessellations[0].vertices[0].x, 0.1);
    }

    #[test]
    fn mesh_auxiliary_channels_round_trip_by_kind() {
        let mut ir = CadIr::empty(Units::default());
        let vertices = vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        let channels = [
            (CHANNEL_UV, 8_u32, vec![0_u8; 24]),
            (CHANNEL_COLOR, 4, vec![0x7f; 12]),
            (CHANNEL_SURFACE_PARAMETERS, 16, vec![0x11; 48]),
            (CHANNEL_CURVATURE, 16, vec![0x22; 48]),
        ]
        .into_iter()
        .map(
            |(kind, item_size, data)| cadmpeg_ir::tessellation::TessellationChannel {
                item_size,
                kind,
                flags: 0,
                count: 3,
                data,
            },
        )
        .collect::<Vec<_>>();
        ir.model
            .tessellations
            .push(cadmpeg_ir::tessellation::Tessellation {
                id: "cadir:model:tessellation#channels".into(),
                body: None,
                faces: Vec::new(),
                chordal_deflection: None,
                source_object: None,
                vertices,
                triangles: vec![[0, 1, 2]],
                strip_lengths: Vec::new(),
                normals: Vec::new(),
                channels: channels.clone(),
            });
        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut bytes)
            .unwrap();
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let actual = &decoded.ir.model.tessellations[0].channels;
        for expected in channels {
            assert_eq!(
                actual.iter().find(|channel| channel.kind == expected.kind),
                Some(&expected)
            );
        }
    }

    #[test]
    fn mesh_channel_bytes_cannot_impersonate_nested_chunk_framing() {
        let mut ir = CadIr::empty(Units::default());
        let mut uv_data = vec![0_u8; 24];
        uv_data[..4].copy_from_slice(&0x4000_8000_u32.to_le_bytes());
        uv_data[4..12].copy_from_slice(&160_i64.to_le_bytes());
        ir.model
            .tessellations
            .push(cadmpeg_ir::tessellation::Tessellation {
                id: "cadir:model:tessellation#chunk-like-channel".into(),
                body: None,
                faces: Vec::new(),
                chordal_deflection: None,
                source_object: None,
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
                strip_lengths: Vec::new(),
                normals: Vec::new(),
                channels: vec![cadmpeg_ir::tessellation::TessellationChannel {
                    kind: CHANNEL_UV,
                    item_size: 8,
                    flags: 0,
                    count: 3,
                    data: uv_data.clone(),
                }],
            });

        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut bytes)
            .expect("channel bytes are opaque to chunk framing");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("generated mesh remains decodable");
        assert_eq!(decoded.ir.model.tessellations[0].channels[0].data, uv_data);
    }

    #[test]
    fn free_vertex_body_preserves_point_cloud_grouping() {
        let mut ir = CadIr::empty(Units::default());
        let body_id: cadmpeg_ir::ids::BodyId = "cadir:model:body#cloud".into();
        let region_id: cadmpeg_ir::ids::RegionId = "cadir:model:region#cloud".into();
        let shell_id: cadmpeg_ir::ids::ShellId = "cadir:model:shell#cloud".into();
        let vertex_ids = [
            cadmpeg_ir::ids::VertexId("cadir:model:vertex#cloud.0".into()),
            cadmpeg_ir::ids::VertexId("cadir:model:vertex#cloud.1".into()),
        ];
        let point_ids = [
            cadmpeg_ir::ids::PointId("cadir:model:point#cloud.0".into()),
            cadmpeg_ir::ids::PointId("cadir:model:point#cloud.1".into()),
        ];
        ir.model.bodies.push(cadmpeg_ir::topology::Body {
            id: body_id.clone(),
            kind: cadmpeg_ir::topology::BodyKind::General,
            regions: vec![region_id.clone()],
            transform: None,
            name: Some("survey points".into()),
            color: Some(cadmpeg_ir::topology::Color {
                r: 1.0,
                g: 0.0,
                b: 128.0 / 255.0,
                a: 1.0,
            }),
            visible: Some(false),
        });
        ir.model.regions.push(cadmpeg_ir::topology::Region {
            id: region_id.clone(),
            body: body_id,
            shells: vec![shell_id.clone()],
        });
        ir.model.shells.push(cadmpeg_ir::topology::Shell {
            id: shell_id,
            region: region_id,
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: vertex_ids.to_vec(),
        });
        for (index, (vertex, point)) in vertex_ids.into_iter().zip(point_ids).enumerate() {
            ir.model.vertices.push(cadmpeg_ir::topology::Vertex {
                id: vertex,
                point: point.clone(),
                tolerance: None,
            });
            ir.model.points.push(cadmpeg_ir::topology::Point {
                id: point,
                position: Point3::new(index as f64, index as f64 + 2.0, 3.0),
                source_object: None,
            });
        }
        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut bytes)
            .unwrap();
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir.model.bodies.len(), 1);
        assert_eq!(decoded.ir.model.vertices.len(), 2);
        assert_eq!(decoded.ir.model.points.len(), 2);
        assert_eq!(
            decoded.ir.model.bodies[0].name.as_deref(),
            Some("survey points")
        );
        assert_eq!(decoded.ir.model.bodies[0].color, ir.model.bodies[0].color);
        assert_eq!(decoded.ir.model.bodies[0].visible, Some(false));
    }

    #[test]
    fn supported_decoded_geometry_can_be_edited_and_rewritten() {
        let mut source = CadIr::empty(Units::default());
        source.model.points.push(Point {
            id: PointId("cadir:model:point#retained".into()),
            position: Point3::new(1.0, 2.0, 3.0),
            source_object: None,
        });
        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&source, &mut bytes)
            .unwrap();
        let mut decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(decoded.ir.native.namespace("rhino").is_some());
        decoded.ir.model.points[0].position = Point3::new(4.0, 5.0, 6.0);

        let mut output = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&decoded.ir, &mut output)
            .unwrap();
        let rewritten = RhinoCodec
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            rewritten.ir.model.points[0].position,
            Point3::new(4.0, 5.0, 6.0)
        );
    }

    #[test]
    fn unsupported_retained_native_records_are_refused_before_output() {
        let mut source = CadIr::empty(Units::default());
        source.model.points.push(Point {
            id: PointId("cadir:model:point#retained".into()),
            position: Point3::new(1.0, 2.0, 3.0),
            source_object: None,
        });
        let mut bytes = Vec::new();
        RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&source, &mut bytes)
            .unwrap();
        let mut decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        decoded
            .ir
            .native
            .namespace_mut("rhino")
            .arenas
            .entry("materials".into())
            .or_default()
            .push(cadmpeg_ir::NativeRecord {
                id: "rhino:presentation:material#unsupported".into(),
                fields: serde_json::Map::new(),
            });

        let mut output = vec![0xaa];
        let error = RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&decoded.ir, &mut output)
            .unwrap_err();
        assert!(error.to_string().contains("survival handling"));
        assert_eq!(output, [0xaa]);
    }

    #[test]
    fn noncanonical_nurbs_periodicity_is_rejected_atomically() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: cadmpeg_ir::ids::CurveId("cadir:model:curve#periodic".into()),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(
                cadmpeg_ir::geometry::NurbsCurve {
                    degree: 2,
                    knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    control_points: vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(1.0, 1.0, 0.0),
                        Point3::new(2.0, 0.0, 0.0),
                    ],
                    weights: None,
                    periodic: true,
                },
            ),
            source_object: None,
        });
        let mut output = vec![0xaa];
        assert!(RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut output)
            .is_err());
        assert_eq!(output, [0xaa]);
    }

    #[test]
    fn planar_triangle_sheet_round_trips_connected_topology() {
        let ir = polygon_sheet(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ]);
        assert_planar_sheet_round_trip(&ir, 1, 3);
    }

    #[test]
    fn planar_quad_sheet_round_trips_connected_topology() {
        let ir = polygon_sheet(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(3.0, 2.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ]);
        assert_planar_sheet_round_trip(&ir, 1, 4);
    }

    #[test]
    fn planar_sheet_round_trips_object_attributes() {
        let mut ir = polygon_sheet(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ]);
        ir.model.bodies[0].name = Some("named sheet".into());
        ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
            r: 64.0 / 255.0,
            g: 128.0 / 255.0,
            b: 1.0,
            a: 192.0 / 255.0,
        });
        ir.model.bodies[0].visible = Some(false);
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let body = &decoded.ir.model.bodies[0];
            assert_eq!(body.name.as_deref(), Some("named sheet"), "{version:?}");
            assert_eq!(body.color, ir.model.bodies[0].color, "{version:?}");
            assert_eq!(body.visible, Some(false), "{version:?}");
        }
    }

    #[test]
    fn planar_sheet_with_hole_round_trips_connected_topology() {
        let mut ir = polygon_sheet(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(4.0, 3.0, 0.0),
            Point3::new(0.0, 3.0, 0.0),
        ]);
        add_polygon_hole(
            &mut ir,
            &[
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(1.0, 2.0, 0.0),
                Point3::new(3.0, 2.0, 0.0),
                Point3::new(3.0, 1.0, 0.0),
            ],
        );
        assert_planar_sheet_round_trip(&ir, 2, 8);
    }

    #[test]
    fn adjacent_planar_faces_round_trip_shared_edge_and_domains() {
        let ir = adjacent_quad_sheet();
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert_eq!(decoded.ir.model.bodies.len(), 1, "{version:?}");
            assert_eq!(
                decoded.ir.model.bodies[0].kind,
                cadmpeg_ir::topology::BodyKind::Sheet,
                "{version:?}"
            );
            assert_eq!(decoded.ir.model.shells.len(), 1, "{version:?}");
            assert_eq!(decoded.ir.model.faces.len(), 2, "{version:?}");
            assert_eq!(decoded.ir.model.loops.len(), 2, "{version:?}");
            assert_eq!(decoded.ir.model.coedges.len(), 8, "{version:?}");
            assert_eq!(decoded.ir.model.edges.len(), 7, "{version:?}");
            assert_eq!(decoded.ir.model.vertices.len(), 6, "{version:?}");
            assert!(decoded
                .ir
                .model
                .edges
                .iter()
                .all(|edge| edge.param_range == Some([2.0, 3.0])));
            let shared = decoded
                .ir
                .model
                .edges
                .iter()
                .find(|edge| {
                    decoded
                        .ir
                        .model
                        .coedges
                        .iter()
                        .filter(|coedge| coedge.edge == edge.id)
                        .count()
                        == 2
                })
                .expect("one shared edge");
            let uses = decoded
                .ir
                .model
                .coedges
                .iter()
                .filter(|coedge| coedge.edge == shared.id)
                .collect::<Vec<_>>();
            assert_ne!(uses[0].sense, uses[1].sense);
            assert_eq!(uses[0].radial_next, uses[1].id);
            assert_eq!(uses[1].radial_next, uses[0].id);
            assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
        }
    }

    #[test]
    fn shared_rational_nurbs_edge_round_trips_c3_and_reversed_c2() {
        let mut ir = adjacent_quad_sheet();
        let edge = &mut ir.model.edges[1];
        edge.param_range = Some([2.0, 5.0]);
        ir.model.curves[1].geometry =
            cadmpeg_ir::geometry::CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
                degree: 2,
                knots: vec![2.0, 2.0, 2.0, 5.0, 5.0, 5.0],
                control_points: vec![
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.25, 0.5, 0.0),
                    Point3::new(1.0, 1.0, 0.0),
                ],
                weights: Some(vec![1.0, 0.75, 1.0]),
                periodic: false,
            });
        let expected = ir.model.curves[1].geometry.clone();
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let shared = decoded
                .ir
                .model
                .edges
                .iter()
                .find(|edge| edge.param_range == Some([2.0, 5.0]))
                .expect("NURBS edge domain");
            let curve = decoded
                .ir
                .model
                .curves
                .iter()
                .find(|curve| shared.curve.as_ref() == Some(&curve.id))
                .expect("NURBS C3");
            assert_eq!(curve.geometry, expected, "{version:?}");
            let uses = decoded
                .ir
                .model
                .coedges
                .iter()
                .filter(|coedge| coedge.edge == shared.id)
                .collect::<Vec<_>>();
            assert_eq!(uses.len(), 2, "{version:?}");
            assert_ne!(uses[0].sense, uses[1].sense, "{version:?}");
            for use_ in uses {
                let pcurve = decoded
                    .ir
                    .model
                    .pcurves
                    .iter()
                    .find(|pcurve| {
                        use_.pcurves.first().map(|use_| &use_.pcurve) == Some(&pcurve.id)
                    })
                    .expect("projected NURBS C2");
                assert!(matches!(
                    pcurve.geometry,
                    cadmpeg_ir::geometry::PcurveGeometry::Nurbs { .. }
                ));
            }
            assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
        }
    }

    #[test]
    fn explicit_nurbs_pcurves_round_trip_owned_geometry_and_tolerance() {
        let mut ir = adjacent_quad_sheet();
        ir.model.edges[1].param_range = Some([2.0, 5.0]);
        ir.model.curves[1].geometry =
            cadmpeg_ir::geometry::CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
                degree: 2,
                knots: vec![2.0, 2.0, 2.0, 5.0, 5.0, 5.0],
                control_points: vec![
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.25, 0.5, 0.0),
                    Point3::new(1.0, 1.0, 0.0),
                ],
                weights: Some(vec![1.0, 0.75, 1.0]),
                periodic: false,
            });
        for (coedge, reversed) in [(1_usize, false), (7, true)] {
            let id: cadmpeg_ir::ids::PcurveId =
                format!("cadir:model:pcurve#explicit.{coedge}").into();
            let mut control_points = vec![
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
                cadmpeg_ir::math::Point2::new(1.25, 0.5),
                cadmpeg_ir::math::Point2::new(1.0, 1.0),
            ];
            if reversed {
                control_points.reverse();
            }
            ir.model.pcurves.push(cadmpeg_ir::geometry::Pcurve {
                id: id.clone(),
                geometry: cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
                    degree: 2,
                    knots: vec![2.0, 2.0, 2.0, 5.0, 5.0, 5.0],
                    control_points,
                    weights: Some(vec![1.0, 0.75, 1.0]),
                    periodic: false,
                },
                wrapper_reversed: Some(false),
                native_tail_flags: None,
                parameter_range: Some([2.0, 5.0]),
                fit_tolerance: Some(0.001),
            });
            ir.model.coedges[coedge].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
                pcurve: id,
                isoparametric: None,
                parameter_range: None,
            }];
        }
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let explicit = decoded
                .ir
                .model
                .pcurves
                .iter()
                .filter(|pcurve| pcurve.fit_tolerance == Some(0.001))
                .collect::<Vec<_>>();
            assert_eq!(explicit.len(), 2, "{version:?}");
            assert!(explicit.iter().all(|pcurve| {
                pcurve.wrapper_reversed == Some(false)
                    && pcurve.parameter_range == Some([2.0, 5.0])
                    && matches!(
                        pcurve.geometry,
                        cadmpeg_ir::geometry::PcurveGeometry::Nurbs { .. }
                    )
            }));
            assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
        }
    }

    #[test]
    fn inconsistent_explicit_pcurve_is_rejected_before_output() {
        let mut ir = polygon_sheet(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ]);
        let id: cadmpeg_ir::ids::PcurveId = "cadir:model:pcurve#mismatch".into();
        ir.model.pcurves.push(cadmpeg_ir::geometry::Pcurve {
            id: id.clone(),
            geometry: cadmpeg_ir::geometry::PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(0.0, 1.0),
                direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            },
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: ir.model.edges[0].param_range,
            fit_tolerance: None,
        });
        ir.model.coedges[0].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }];
        let mut output = vec![0xaa];
        let error = RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut output)
            .unwrap_err();
        assert!(error.to_string().contains("does not exactly match"));
        assert_eq!(output, [0xaa]);
    }

    #[test]
    fn explicit_line_pcurve_round_trips_as_native_c2() {
        let mut ir = polygon_sheet(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ]);
        let id: cadmpeg_ir::ids::PcurveId = "cadir:model:pcurve#line".into();
        ir.model.pcurves.push(cadmpeg_ir::geometry::Pcurve {
            id: id.clone(),
            geometry: cadmpeg_ir::geometry::PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            },
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: Some([0.0, 2.0]),
            fit_tolerance: Some(0.002),
        });
        ir.model.coedges[0].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }];
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let pcurve = decoded
                .ir
                .model
                .pcurves
                .iter()
                .find(|pcurve| pcurve.fit_tolerance == Some(0.002))
                .expect("explicit line C2");
            assert_eq!(pcurve.parameter_range, Some([0.0, 2.0]));
            assert!(matches!(
                pcurve.geometry,
                cadmpeg_ir::geometry::PcurveGeometry::Nurbs { degree: 1, .. }
            ));
        }
    }

    #[test]
    fn rational_nurbs_surface_patch_round_trips_exact_boundaries() {
        let ir = rectangular_nurbs_patch();
        let expected_surface = ir.model.surfaces[0].geometry.clone();
        let expected_curves = ir
            .model
            .curves
            .iter()
            .map(|curve| curve.geometry.clone())
            .collect::<Vec<_>>();
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert_eq!(decoded.ir.model.bodies.len(), 1, "{version:?}");
            assert_eq!(
                decoded.ir.model.bodies[0].kind,
                cadmpeg_ir::topology::BodyKind::Sheet,
                "{version:?}"
            );
            assert_eq!(decoded.ir.model.surfaces[0].geometry, expected_surface);
            assert_eq!(
                decoded
                    .ir
                    .model
                    .curves
                    .iter()
                    .map(|curve| curve.geometry.clone())
                    .collect::<Vec<_>>(),
                expected_curves,
                "{version:?}"
            );
            assert_eq!(decoded.ir.model.pcurves.len(), 4, "{version:?}");
            assert!(decoded
                .ir
                .model
                .pcurves
                .iter()
                .all(|pcurve| pcurve.fit_tolerance == Some(0.001)));
            assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
        }
    }

    #[test]
    fn mixed_plane_and_nurbs_faces_round_trip_shared_edge() {
        let ir = mixed_plane_nurbs_sheet();
        let expected_surface = ir.model.surfaces[0].geometry.clone();
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert_eq!(decoded.ir.model.bodies.len(), 1, "{version:?}");
            assert_eq!(
                decoded.ir.model.bodies[0].kind,
                cadmpeg_ir::topology::BodyKind::Sheet,
                "{version:?}"
            );
            assert_eq!(decoded.ir.model.faces.len(), 2, "{version:?}");
            assert_eq!(decoded.ir.model.surfaces[0].geometry, expected_surface);
            assert_eq!(decoded.ir.model.edges[1].param_range, Some([30.0, 32.0]));
            let shared_uses = decoded
                .ir
                .model
                .coedges
                .iter()
                .enumerate()
                .filter(|(_, coedge)| coedge.edge == decoded.ir.model.edges[1].id)
                .collect::<Vec<_>>();
            assert_eq!(shared_uses.len(), 2, "{version:?}");
            assert_ne!(shared_uses[0].1.sense, shared_uses[1].1.sense);
            assert_eq!(
                shared_uses[0].1.radial_next, shared_uses[1].1.id,
                "{version:?}"
            );
            assert_eq!(
                shared_uses[1].1.radial_next, shared_uses[0].1.id,
                "{version:?}"
            );
            assert_eq!(
                decoded
                    .ir
                    .model
                    .pcurves
                    .iter()
                    .filter(|pcurve| pcurve.fit_tolerance == Some(0.001))
                    .count(),
                4,
                "{version:?}"
            );
            let planar_shared_pcurve = decoded
                .ir
                .model
                .pcurves
                .iter()
                .find(|pcurve| {
                    pcurve.parameter_range == Some([30.0, 32.0])
                        && pcurve.fit_tolerance != Some(0.001)
                })
                .expect("generated planar shared-edge pcurve");
            assert!(matches!(
                planar_shared_pcurve.geometry,
                cadmpeg_ir::geometry::PcurveGeometry::Nurbs { .. }
            ));
            assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
        }
    }

    #[test]
    fn generally_trimmed_nurbs_face_round_trips_outer_loop_and_hole() {
        let mut ir = polygon_sheet(&[
            Point3::new(0.25, 0.25, 0.0),
            Point3::new(3.5, 0.75, 0.0),
            Point3::new(2.75, 3.5, 0.0),
            Point3::new(0.5, 2.75, 0.0),
        ]);
        add_polygon_hole(
            &mut ir,
            &[
                Point3::new(1.25, 1.25, 0.0),
                Point3::new(1.5, 2.25, 0.0),
                Point3::new(2.25, 1.5, 0.0),
            ],
        );
        make_planar_nurbs_trimmed_face(&mut ir);
        let domain = ir.model.edges[0].param_range.expect("fixture domain");
        let poles = [
            Point3::new(0.25, 0.25, 0.0),
            Point3::new(2.0, 0.25, 0.0),
            Point3::new(3.5, 0.75, 0.0),
        ];
        ir.model.curves[0].geometry =
            cadmpeg_ir::geometry::CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
                degree: 2,
                knots: vec![
                    domain[0], domain[0], domain[0], domain[1], domain[1], domain[1],
                ],
                control_points: poles.to_vec(),
                weights: Some(vec![1.0, 0.8, 1.0]),
                periodic: false,
            });
        ir.model.pcurves[0].geometry = cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            degree: 2,
            knots: vec![
                domain[0], domain[0], domain[0], domain[1], domain[1], domain[1],
            ],
            control_points: poles
                .iter()
                .map(|point| cadmpeg_ir::math::Point2::new(point.x, point.y))
                .collect(),
            weights: Some(vec![1.0, 0.8, 1.0]),
            periodic: false,
        };
        let expected_surface = ir.model.surfaces[0].geometry.clone();
        let expected_curve = ir.model.curves[0].geometry.clone();
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert_eq!(decoded.ir.model.surfaces[0].geometry, expected_surface);
            assert_eq!(decoded.ir.model.curves[0].geometry, expected_curve);
            assert_eq!(decoded.ir.model.loops.len(), 2, "{version:?}");
            assert_eq!(decoded.ir.model.pcurves.len(), 7, "{version:?}");
            assert!(decoded
                .ir
                .model
                .pcurves
                .iter()
                .all(|pcurve| pcurve.fit_tolerance == Some(0.0001)));
            assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
        }
    }

    #[test]
    fn nurbs_trim_that_misses_its_edge_is_rejected_atomically() {
        let mut ir = polygon_sheet(&[
            Point3::new(0.5, 0.5, 0.0),
            Point3::new(3.5, 0.5, 0.0),
            Point3::new(2.0, 3.0, 0.0),
        ]);
        make_planar_nurbs_trimmed_face(&mut ir);
        let cadmpeg_ir::geometry::PcurveGeometry::Line { direction, .. } =
            &mut ir.model.pcurves[0].geometry
        else {
            unreachable!()
        };
        direction.v += 0.25;
        let mut output = vec![0xaa];
        let error = RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut output)
            .unwrap_err();
        assert!(error.to_string().contains("misses directed edge curve"));
        assert_eq!(output, [0xaa]);
    }

    #[test]
    fn nurbs_surface_patch_without_boundary_pcurves_is_rejected_atomically() {
        let mut ir = rectangular_nurbs_patch();
        ir.model.pcurves.clear();
        for coedge in &mut ir.model.coedges {
            coedge.pcurves.clear();
        }
        let mut output = vec![0xaa];
        let error = RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut output)
            .unwrap_err();
        assert!(error.to_string().contains("explicit pcurve"));
        assert_eq!(output, [0xaa]);
    }

    #[test]
    fn planar_tetrahedron_round_trips_as_closed_solid() {
        let ir = planar_tetrahedron();
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert_eq!(decoded.ir.model.bodies.len(), 1, "{version:?}");
            assert_eq!(
                decoded.ir.model.bodies[0].kind,
                cadmpeg_ir::topology::BodyKind::Solid,
                "{version:?}"
            );
            assert_eq!(decoded.ir.model.shells.len(), 1, "{version:?}");
            assert_eq!(decoded.ir.model.faces.len(), 4, "{version:?}");
            assert_eq!(decoded.ir.model.loops.len(), 4, "{version:?}");
            assert_eq!(decoded.ir.model.coedges.len(), 12, "{version:?}");
            assert_eq!(decoded.ir.model.edges.len(), 6, "{version:?}");
            assert_eq!(decoded.ir.model.vertices.len(), 4, "{version:?}");
            for (actual, expected) in decoded.ir.model.edges.iter().zip(&ir.model.edges) {
                assert_eq!(actual.param_range, expected.param_range, "{version:?}");
                assert_eq!(
                    decoded
                        .ir
                        .model
                        .coedges
                        .iter()
                        .filter(|coedge| coedge.edge == actual.id)
                        .count(),
                    2,
                    "{version:?}"
                );
            }
            assert!(decoded
                .ir
                .model
                .coedges
                .iter()
                .all(|coedge| coedge.radial_next != coedge.id));
            assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
        }
    }

    #[test]
    fn multiple_brep_objects_round_trip_in_one_archive() {
        let mut ir = polygon_sheet(&[
            Point3::new(-2.0, 0.0, 0.0),
            Point3::new(-1.0, 0.0, 0.0),
            Point3::new(-1.5, 1.0, 0.0),
        ]);
        let mut adjacent = adjacent_quad_sheet();
        ir.model.bodies.append(&mut adjacent.model.bodies);
        ir.model.regions.append(&mut adjacent.model.regions);
        ir.model.shells.append(&mut adjacent.model.shells);
        ir.model.faces.append(&mut adjacent.model.faces);
        ir.model.loops.append(&mut adjacent.model.loops);
        ir.model.coedges.append(&mut adjacent.model.coedges);
        ir.model.edges.append(&mut adjacent.model.edges);
        ir.model.vertices.append(&mut adjacent.model.vertices);
        ir.model.points.append(&mut adjacent.model.points);
        ir.model.surfaces.append(&mut adjacent.model.surfaces);
        ir.model.curves.append(&mut adjacent.model.curves);
        ir.model.pcurves.append(&mut adjacent.model.pcurves);
        ir.finalize();
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert!(
                decoded
                    .report
                    .losses
                    .iter()
                    .all(|loss| !loss.message.contains("CRC mismatch")),
                "{version:?}: {:?}",
                decoded.report.losses
            );
            assert_eq!(decoded.ir.model.bodies.len(), 2, "{version:?}");
            assert_eq!(decoded.ir.model.faces.len(), 3, "{version:?}");
            assert_eq!(decoded.ir.model.edges.len(), 10, "{version:?}");
            assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
        }
    }

    #[test]
    fn brep_and_free_geometry_round_trip_in_one_archive() {
        use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
        use cadmpeg_ir::ids::{CurveId, SurfaceId};
        use cadmpeg_ir::math::Vector3;

        let mut ir = polygon_sheet(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ]);
        ir.model.points.push(Point {
            id: PointId("cadir:model:point#free".into()),
            position: Point3::new(5.0, 6.0, 7.0),
            source_object: None,
        });
        ir.model.curves.push(Curve {
            id: CurveId("cadir:model:curve#free".into()),
            geometry: CurveGeometry::Circle {
                center: Point3::new(5.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        });
        ir.model.surfaces.push(Surface {
            id: SurfaceId("cadir:model:surface#free".into()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 3.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        ir.finalize();
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert_eq!(decoded.ir.model.bodies.len(), 2, "{version:?}");
            assert!(decoded
                .ir
                .model
                .points
                .iter()
                .any(|point| point.position == Point3::new(5.0, 6.0, 7.0)));
            assert!(decoded
                .ir
                .model
                .curves
                .iter()
                .any(|curve| matches!(curve.geometry, CurveGeometry::Circle { radius: 2.0, .. })));
            assert!(decoded.ir.model.surfaces.iter().any(|surface| matches!(
                surface.geometry,
                SurfaceGeometry::Plane { origin, .. } if origin.z == 3.0
            )));
            assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
        }
    }

    #[test]
    fn open_planar_solid_is_rejected_before_output() {
        let mut ir = adjacent_quad_sheet();
        ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Solid;
        let mut output = vec![0xaa];
        let error = RhinoEncoder::new(RhinoArchiveVersion::V8)
            .encode(&ir, &mut output)
            .unwrap_err();
        assert!(error.to_string().contains("incidence"));
        assert_eq!(output, [0xaa]);
    }

    fn assert_planar_sheet_round_trip(ir: &CadIr, loop_count: usize, edge_count: usize) {
        for version in [
            RhinoArchiveVersion::V5,
            RhinoArchiveVersion::V6,
            RhinoArchiveVersion::V7,
            RhinoArchiveVersion::V8,
        ] {
            let mut bytes = Vec::new();
            RhinoEncoder::new(version).encode(&ir, &mut bytes).unwrap();
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert!(
                decoded
                    .report
                    .losses
                    .iter()
                    .all(|loss| !loss.message.contains("Brep mesh cache degraded")),
                "{version:?}: {:?}",
                decoded.report.losses
            );
            assert_eq!(decoded.ir.model.bodies.len(), 1, "{version:?}");
            assert_eq!(
                decoded.ir.model.bodies[0].kind,
                cadmpeg_ir::topology::BodyKind::Sheet,
                "{version:?}"
            );
            assert_eq!(decoded.ir.model.faces.len(), 1, "{version:?}");
            assert_eq!(decoded.ir.model.loops.len(), loop_count, "{version:?}");
            assert_eq!(decoded.ir.model.coedges.len(), edge_count, "{version:?}");
            assert_eq!(decoded.ir.model.edges.len(), edge_count, "{version:?}");
            assert_eq!(decoded.ir.model.vertices.len(), edge_count, "{version:?}");
            for (actual, expected) in decoded.ir.model.edges.iter().zip(&ir.model.edges) {
                assert_eq!(actual.param_range, expected.param_range, "{version:?}");
            }
            assert!(
                cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok(),
                "{version:?}"
            );
        }
    }

    fn polygon_sheet(points: &[Point3]) -> CadIr {
        use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
        use cadmpeg_ir::ids::*;
        use cadmpeg_ir::math::Vector3;
        use cadmpeg_ir::topology::*;

        let mut ir = CadIr::empty(Units::default());
        let body: BodyId = "cadir:model:body#polygon".into();
        let region: RegionId = "cadir:model:region#polygon".into();
        let shell: ShellId = "cadir:model:shell#polygon".into();
        let face: FaceId = "cadir:model:face#polygon".into();
        let loop_id: LoopId = "cadir:model:loop#polygon".into();
        let surface: SurfaceId = "cadir:model:surface#polygon".into();
        let point_ids = (0..points.len())
            .map(|index| PointId(format!("cadir:model:point#polygon.{index}")))
            .collect::<Vec<_>>();
        let vertex_ids = (0..points.len())
            .map(|index| VertexId(format!("cadir:model:vertex#polygon.{index}")))
            .collect::<Vec<_>>();
        let edge_ids = (0..points.len())
            .map(|index| EdgeId(format!("cadir:model:edge#polygon.{index}")))
            .collect::<Vec<_>>();
        let curve_ids = (0..points.len())
            .map(|index| CurveId(format!("cadir:model:curve#polygon.{index}")))
            .collect::<Vec<_>>();
        let coedge_ids = (0..points.len())
            .map(|index| CoedgeId(format!("cadir:model:coedge#polygon.{index}")))
            .collect::<Vec<_>>();
        ir.model.bodies.push(Body {
            id: body.clone(),
            kind: BodyKind::Sheet,
            regions: vec![region.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region.clone(),
            body,
            shells: vec![shell.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell.clone(),
            region,
            faces: vec![face.clone()],
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        ir.model.faces.push(Face {
            id: face.clone(),
            shell,
            surface: surface.clone(),
            sense: Sense::Forward,
            loops: vec![loop_id.clone()],
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.loops.push(Loop {
            id: loop_id.clone(),
            face,
            boundary_role: Default::default(),
            coedges: coedge_ids.to_vec(),
            vertex_uses: Vec::new(),
        });
        ir.model.surfaces.push(Surface {
            id: surface,
            geometry: SurfaceGeometry::Plane {
                origin: points[0],
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        for index in 0..points.len() {
            let next = points[(index + 1) % points.len()];
            let delta = Vector3::new(
                next.x - points[index].x,
                next.y - points[index].y,
                next.z - points[index].z,
            );
            let length = delta.norm();
            let direction = Vector3::new(delta.x / length, delta.y / length, delta.z / length);
            ir.model.points.push(Point {
                id: point_ids[index].clone(),
                position: points[index],
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vertex_ids[index].clone(),
                point: point_ids[index].clone(),
                tolerance: None,
            });
            ir.model.curves.push(Curve {
                id: curve_ids[index].clone(),
                geometry: CurveGeometry::Line {
                    origin: points[index],
                    direction,
                },
                source_object: None,
            });
            ir.model.edges.push(Edge {
                id: edge_ids[index].clone(),
                curve: Some(curve_ids[index].clone()),
                start: vertex_ids[index].clone(),
                end: vertex_ids[(index + 1) % points.len()].clone(),
                param_range: Some([0.0, length]),
                tolerance: None,
            });
            ir.model.coedges.push(Coedge {
                id: coedge_ids[index].clone(),
                owner_loop: loop_id.clone(),
                edge: edge_ids[index].clone(),
                next: coedge_ids[(index + 1) % points.len()].clone(),
                previous: coedge_ids[(index + points.len() - 1) % points.len()].clone(),
                radial_next: coedge_ids[index].clone(),
                sense: Sense::Forward,
                pcurves: Vec::new(),
                use_curve: None,
                use_curve_parameter_range: None,
            });
        }
        ir.finalize();
        ir
    }

    fn add_polygon_hole(ir: &mut CadIr, points: &[Point3]) {
        use cadmpeg_ir::geometry::{Curve, CurveGeometry};
        use cadmpeg_ir::ids::*;
        use cadmpeg_ir::math::Vector3;
        use cadmpeg_ir::topology::*;

        let base = ir.model.edges.len();
        let face = ir.model.faces[0].id.clone();
        let loop_id = LoopId(format!("cadir:model:loop#polygon.{}", ir.model.loops.len()));
        let point_ids = (0..points.len())
            .map(|index| PointId(format!("cadir:model:point#polygon.{}", base + index)))
            .collect::<Vec<_>>();
        let vertex_ids = (0..points.len())
            .map(|index| VertexId(format!("cadir:model:vertex#polygon.{}", base + index)))
            .collect::<Vec<_>>();
        let edge_ids = (0..points.len())
            .map(|index| EdgeId(format!("cadir:model:edge#polygon.{}", base + index)))
            .collect::<Vec<_>>();
        let curve_ids = (0..points.len())
            .map(|index| CurveId(format!("cadir:model:curve#polygon.{}", base + index)))
            .collect::<Vec<_>>();
        let coedge_ids = (0..points.len())
            .map(|index| CoedgeId(format!("cadir:model:coedge#polygon.{}", base + index)))
            .collect::<Vec<_>>();
        ir.model.faces[0].loops.push(loop_id.clone());
        ir.model.loops.push(Loop {
            id: loop_id.clone(),
            face,
            boundary_role: Default::default(),
            coedges: coedge_ids.clone(),
            vertex_uses: Vec::new(),
        });
        for index in 0..points.len() {
            let next_index = (index + 1) % points.len();
            let next = points[next_index];
            let delta = Vector3::new(
                next.x - points[index].x,
                next.y - points[index].y,
                next.z - points[index].z,
            );
            let length = delta.norm();
            ir.model.points.push(Point {
                id: point_ids[index].clone(),
                position: points[index],
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vertex_ids[index].clone(),
                point: point_ids[index].clone(),
                tolerance: None,
            });
            ir.model.curves.push(Curve {
                id: curve_ids[index].clone(),
                geometry: CurveGeometry::Line {
                    origin: points[index],
                    direction: Vector3::new(delta.x / length, delta.y / length, delta.z / length),
                },
                source_object: None,
            });
            ir.model.edges.push(Edge {
                id: edge_ids[index].clone(),
                curve: Some(curve_ids[index].clone()),
                start: vertex_ids[index].clone(),
                end: vertex_ids[next_index].clone(),
                param_range: Some([0.0, length]),
                tolerance: None,
            });
            ir.model.coedges.push(Coedge {
                id: coedge_ids[index].clone(),
                owner_loop: loop_id.clone(),
                edge: edge_ids[index].clone(),
                next: coedge_ids[next_index].clone(),
                previous: coedge_ids[(index + points.len() - 1) % points.len()].clone(),
                radial_next: coedge_ids[index].clone(),
                sense: Sense::Forward,
                pcurves: Vec::new(),
                use_curve: None,
                use_curve_parameter_range: None,
            });
        }
        ir.finalize();
    }

    fn adjacent_quad_sheet() -> CadIr {
        use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
        use cadmpeg_ir::ids::*;
        use cadmpeg_ir::math::Vector3;
        use cadmpeg_ir::topology::*;

        let mut ir = CadIr::empty(Units::default());
        let body: BodyId = "cadir:model:body#adjacent".into();
        let region: RegionId = "cadir:model:region#adjacent".into();
        let shell: ShellId = "cadir:model:shell#adjacent".into();
        let face_ids = [
            FaceId("cadir:model:face#adjacent.0".into()),
            FaceId("cadir:model:face#adjacent.1".into()),
        ];
        let loop_ids = [
            LoopId("cadir:model:loop#adjacent.0".into()),
            LoopId("cadir:model:loop#adjacent.1".into()),
        ];
        let surface_ids = [
            SurfaceId("cadir:model:surface#adjacent.0".into()),
            SurfaceId("cadir:model:surface#adjacent.1".into()),
        ];
        let positions = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.0, 1.0, 0.0),
        ];
        let point_ids = (0..positions.len())
            .map(|index| PointId(format!("cadir:model:point#adjacent.{index}")))
            .collect::<Vec<_>>();
        let vertex_ids = (0..positions.len())
            .map(|index| VertexId(format!("cadir:model:vertex#adjacent.{index}")))
            .collect::<Vec<_>>();
        let edge_ids = (0..7)
            .map(|index| EdgeId(format!("cadir:model:edge#adjacent.{index}")))
            .collect::<Vec<_>>();
        let curve_ids = (0..7)
            .map(|index| CurveId(format!("cadir:model:curve#adjacent.{index}")))
            .collect::<Vec<_>>();
        let coedge_ids = (0..8)
            .map(|index| CoedgeId(format!("cadir:model:coedge#adjacent.{index}")))
            .collect::<Vec<_>>();
        ir.model.bodies.push(Body {
            id: body.clone(),
            kind: BodyKind::Sheet,
            regions: vec![region.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region.clone(),
            body,
            shells: vec![shell.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell.clone(),
            region,
            faces: face_ids.to_vec(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        for index in 0..2 {
            ir.model.faces.push(Face {
                id: face_ids[index].clone(),
                shell: shell.clone(),
                surface: surface_ids[index].clone(),
                sense: Sense::Forward,
                loops: vec![loop_ids[index].clone()],
                name: None,
                color: None,
                tolerance: None,
            });
            ir.model.surfaces.push(Surface {
                id: surface_ids[index].clone(),
                geometry: SurfaceGeometry::Plane {
                    origin: positions[0],
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            });
        }
        ir.model.loops.push(Loop {
            id: loop_ids[0].clone(),
            face: face_ids[0].clone(),
            boundary_role: Default::default(),
            coedges: coedge_ids[0..4].to_vec(),
            vertex_uses: Vec::new(),
        });
        ir.model.loops.push(Loop {
            id: loop_ids[1].clone(),
            face: face_ids[1].clone(),
            boundary_role: Default::default(),
            coedges: coedge_ids[4..8].to_vec(),
            vertex_uses: Vec::new(),
        });
        for index in 0..positions.len() {
            ir.model.points.push(Point {
                id: point_ids[index].clone(),
                position: positions[index],
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vertex_ids[index].clone(),
                point: point_ids[index].clone(),
                tolerance: None,
            });
        }
        let endpoints = [(0, 1), (1, 2), (2, 3), (3, 0), (1, 4), (4, 5), (5, 2)];
        for (index, (start, end)) in endpoints.into_iter().enumerate() {
            let delta = Vector3::new(
                positions[end].x - positions[start].x,
                positions[end].y - positions[start].y,
                positions[end].z - positions[start].z,
            );
            ir.model.curves.push(Curve {
                id: curve_ids[index].clone(),
                geometry: CurveGeometry::Line {
                    origin: Point3::new(
                        positions[start].x - 2.0 * delta.x,
                        positions[start].y - 2.0 * delta.y,
                        positions[start].z - 2.0 * delta.z,
                    ),
                    direction: delta,
                },
                source_object: None,
            });
            ir.model.edges.push(Edge {
                id: edge_ids[index].clone(),
                curve: Some(curve_ids[index].clone()),
                start: vertex_ids[start].clone(),
                end: vertex_ids[end].clone(),
                param_range: Some([2.0, 3.0]),
                tolerance: None,
            });
        }
        let uses = [
            (0, Sense::Forward),
            (1, Sense::Forward),
            (2, Sense::Forward),
            (3, Sense::Forward),
            (4, Sense::Forward),
            (5, Sense::Forward),
            (6, Sense::Forward),
            (1, Sense::Reversed),
        ];
        for (index, (edge, sense)) in uses.into_iter().enumerate() {
            let loop_start = if index < 4 { 0 } else { 4 };
            let offset = index - loop_start;
            let radial_next = if index == 1 {
                7
            } else if index == 7 {
                1
            } else {
                index
            };
            ir.model.coedges.push(Coedge {
                id: coedge_ids[index].clone(),
                owner_loop: loop_ids[usize::from(index >= 4)].clone(),
                edge: edge_ids[edge].clone(),
                next: coedge_ids[loop_start + (offset + 1) % 4].clone(),
                previous: coedge_ids[loop_start + (offset + 3) % 4].clone(),
                radial_next: coedge_ids[radial_next].clone(),
                sense,
                pcurves: Vec::new(),
                use_curve: None,
                use_curve_parameter_range: None,
            });
        }
        ir.finalize();
        ir
    }

    fn planar_tetrahedron() -> CadIr {
        use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
        use cadmpeg_ir::ids::*;
        use cadmpeg_ir::math::Vector3;
        use cadmpeg_ir::topology::*;

        let mut ir = CadIr::empty(Units::default());
        let body: BodyId = "cadir:model:body#tetrahedron".into();
        let region: RegionId = "cadir:model:region#tetrahedron".into();
        let shell: ShellId = "cadir:model:shell#tetrahedron".into();
        let positions = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
        ];
        let point_ids = (0..4)
            .map(|index| PointId(format!("cadir:model:point#tetrahedron.{index}")))
            .collect::<Vec<_>>();
        let vertex_ids = (0..4)
            .map(|index| VertexId(format!("cadir:model:vertex#tetrahedron.{index}")))
            .collect::<Vec<_>>();
        let edge_ids = (0..6)
            .map(|index| EdgeId(format!("cadir:model:edge#tetrahedron.{index}")))
            .collect::<Vec<_>>();
        let curve_ids = (0..6)
            .map(|index| CurveId(format!("cadir:model:curve#tetrahedron.{index}")))
            .collect::<Vec<_>>();
        let face_ids = (0..4)
            .map(|index| FaceId(format!("cadir:model:face#tetrahedron.{index}")))
            .collect::<Vec<_>>();
        let loop_ids = (0..4)
            .map(|index| LoopId(format!("cadir:model:loop#tetrahedron.{index}")))
            .collect::<Vec<_>>();
        let surface_ids = (0..4)
            .map(|index| SurfaceId(format!("cadir:model:surface#tetrahedron.{index}")))
            .collect::<Vec<_>>();
        let coedge_ids = (0..12)
            .map(|index| CoedgeId(format!("cadir:model:coedge#tetrahedron.{index}")))
            .collect::<Vec<_>>();
        ir.model.bodies.push(Body {
            id: body.clone(),
            kind: BodyKind::Solid,
            regions: vec![region.clone()],
            transform: None,
            name: Some("tetrahedron".into()),
            color: None,
            visible: Some(true),
        });
        ir.model.regions.push(Region {
            id: region.clone(),
            body,
            shells: vec![shell.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell.clone(),
            region,
            faces: face_ids.clone(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        for index in 0..4 {
            ir.model.points.push(Point {
                id: point_ids[index].clone(),
                position: positions[index],
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vertex_ids[index].clone(),
                point: point_ids[index].clone(),
                tolerance: None,
            });
        }
        let endpoints = [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)];
        for (index, (start, end)) in endpoints.into_iter().enumerate() {
            let delta = Vector3::new(
                positions[end].x - positions[start].x,
                positions[end].y - positions[start].y,
                positions[end].z - positions[start].z,
            );
            let length = delta.norm();
            let direction = Vector3::new(delta.x / length, delta.y / length, delta.z / length);
            ir.model.curves.push(Curve {
                id: curve_ids[index].clone(),
                geometry: CurveGeometry::Line {
                    origin: Point3::new(
                        positions[start].x - 2.0 * direction.x,
                        positions[start].y - 2.0 * direction.y,
                        positions[start].z - 2.0 * direction.z,
                    ),
                    direction,
                },
                source_object: None,
            });
            ir.model.edges.push(Edge {
                id: edge_ids[index].clone(),
                curve: Some(curve_ids[index].clone()),
                start: vertex_ids[start].clone(),
                end: vertex_ids[end].clone(),
                param_range: Some([2.0, 2.0 + length]),
                tolerance: None,
            });
        }
        let inverse_sqrt_2 = 1.0 / 2.0_f64.sqrt();
        let inverse_sqrt_3 = 1.0 / 3.0_f64.sqrt();
        let planes = [
            (Vector3::new(0.0, 0.0, -1.0), Vector3::new(1.0, 0.0, 0.0)),
            (Vector3::new(0.0, -1.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
            (
                Vector3::new(inverse_sqrt_3, inverse_sqrt_3, inverse_sqrt_3),
                Vector3::new(-inverse_sqrt_2, inverse_sqrt_2, 0.0),
            ),
            (Vector3::new(-1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
        ];
        let face_uses = [
            [
                (2, Sense::Reversed),
                (1, Sense::Reversed),
                (0, Sense::Reversed),
            ],
            [
                (0, Sense::Forward),
                (4, Sense::Forward),
                (3, Sense::Reversed),
            ],
            [
                (1, Sense::Forward),
                (5, Sense::Forward),
                (4, Sense::Reversed),
            ],
            [
                (3, Sense::Forward),
                (5, Sense::Reversed),
                (2, Sense::Forward),
            ],
        ];
        for face in 0..4 {
            let start = face * 3;
            ir.model.faces.push(Face {
                id: face_ids[face].clone(),
                shell: shell.clone(),
                surface: surface_ids[face].clone(),
                sense: Sense::Forward,
                loops: vec![loop_ids[face].clone()],
                name: None,
                color: None,
                tolerance: None,
            });
            ir.model.loops.push(Loop {
                id: loop_ids[face].clone(),
                face: face_ids[face].clone(),
                boundary_role: Default::default(),
                coedges: coedge_ids[start..start + 3].to_vec(),
                vertex_uses: Vec::new(),
            });
            ir.model.surfaces.push(Surface {
                id: surface_ids[face].clone(),
                geometry: SurfaceGeometry::Plane {
                    origin: positions[face_uses[face][0].0],
                    normal: planes[face].0,
                    u_axis: planes[face].1,
                },
                source_object: None,
            });
            for offset in 0..3 {
                let index = start + offset;
                ir.model.coedges.push(Coedge {
                    id: coedge_ids[index].clone(),
                    owner_loop: loop_ids[face].clone(),
                    edge: edge_ids[face_uses[face][offset].0].clone(),
                    next: coedge_ids[start + (offset + 1) % 3].clone(),
                    previous: coedge_ids[start + (offset + 2) % 3].clone(),
                    radial_next: coedge_ids[index].clone(),
                    sense: face_uses[face][offset].1,
                    pcurves: Vec::new(),
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
            }
        }
        for edge in 0..6 {
            let uses = ir
                .model
                .coedges
                .iter()
                .enumerate()
                .filter(|(_, coedge)| coedge.edge == edge_ids[edge])
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            ir.model.coedges[uses[0]].radial_next = coedge_ids[uses[1]].clone();
            ir.model.coedges[uses[1]].radial_next = coedge_ids[uses[0]].clone();
        }
        ir.finalize();
        ir
    }

    fn rectangular_nurbs_patch() -> CadIr {
        use cadmpeg_ir::geometry::{
            CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, SurfaceGeometry,
        };

        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(3.0, 2.0, 1.0),
            Point3::new(0.0, 2.0, 0.0),
        ];
        let mut ir = polygon_sheet(&points);
        ir.model.surfaces[0].geometry = SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![2.0, 2.0, 5.0, 5.0],
            v_knots: vec![7.0, 7.0, 11.0, 11.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![points[0], points[3], points[1], points[2]],
            weights: Some(vec![1.0, 0.8, 1.2, 1.0]),
            u_periodic: false,
            v_periodic: false,
        });
        let edge_data = [
            (
                [20.0, 23.0],
                vec![points[0], points[1]],
                vec![1.0, 1.2],
                cadmpeg_ir::math::Point2::new(-18.0, 7.0),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            ),
            (
                [30.0, 32.0],
                vec![points[1], points[2]],
                vec![1.2, 1.0],
                cadmpeg_ir::math::Point2::new(5.0, -53.0),
                cadmpeg_ir::math::Point2::new(0.0, 2.0),
            ),
            (
                [40.0, 43.0],
                vec![points[2], points[3]],
                vec![1.0, 0.8],
                cadmpeg_ir::math::Point2::new(45.0, 11.0),
                cadmpeg_ir::math::Point2::new(-1.0, 0.0),
            ),
            (
                [50.0, 52.0],
                vec![points[3], points[0]],
                vec![0.8, 1.0],
                cadmpeg_ir::math::Point2::new(2.0, 111.0),
                cadmpeg_ir::math::Point2::new(0.0, -2.0),
            ),
        ];
        for (index, (domain, control_points, weights, origin, direction)) in
            edge_data.into_iter().enumerate()
        {
            ir.model.edges[index].param_range = Some(domain);
            ir.model.curves[index].geometry = CurveGeometry::Nurbs(NurbsCurve {
                degree: 1,
                knots: vec![domain[0], domain[0], domain[1], domain[1]],
                control_points,
                weights: Some(weights),
                periodic: false,
            });
            let id: cadmpeg_ir::ids::PcurveId = format!("cadir:model:pcurve#patch.{index}").into();
            ir.model.pcurves.push(Pcurve {
                id: id.clone(),
                geometry: PcurveGeometry::Line { origin, direction },
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: Some(domain),
                fit_tolerance: Some(0.001),
            });
            ir.model.coedges[index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
                pcurve: id,
                isoparametric: None,
                parameter_range: None,
            }];
        }
        ir.finalize();
        ir
    }

    fn mixed_plane_nurbs_sheet() -> CadIr {
        use cadmpeg_ir::geometry::{
            CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, SurfaceGeometry,
        };

        let mut ir = adjacent_quad_sheet();
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        ir.model.surfaces[0].geometry = SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![2.0, 2.0, 5.0, 5.0],
            v_knots: vec![7.0, 7.0, 11.0, 11.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![points[0], points[3], points[1], points[2]],
            weights: Some(vec![1.0, 0.8, 1.2, 1.0]),
            u_periodic: false,
            v_periodic: false,
        });
        let edge_data = [
            (
                [20.0, 23.0],
                vec![points[0], points[1]],
                vec![1.0, 1.2],
                cadmpeg_ir::math::Point2::new(-18.0, 7.0),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            ),
            (
                [30.0, 32.0],
                vec![points[1], points[2]],
                vec![1.2, 1.0],
                cadmpeg_ir::math::Point2::new(5.0, -53.0),
                cadmpeg_ir::math::Point2::new(0.0, 2.0),
            ),
            (
                [40.0, 43.0],
                vec![points[2], points[3]],
                vec![1.0, 0.8],
                cadmpeg_ir::math::Point2::new(45.0, 11.0),
                cadmpeg_ir::math::Point2::new(-1.0, 0.0),
            ),
            (
                [50.0, 52.0],
                vec![points[3], points[0]],
                vec![0.8, 1.0],
                cadmpeg_ir::math::Point2::new(2.0, 111.0),
                cadmpeg_ir::math::Point2::new(0.0, -2.0),
            ),
        ];
        for (index, (domain, control_points, weights, origin, direction)) in
            edge_data.into_iter().enumerate()
        {
            ir.model.edges[index].param_range = Some(domain);
            ir.model.curves[index].geometry = CurveGeometry::Nurbs(NurbsCurve {
                degree: 1,
                knots: vec![domain[0], domain[0], domain[1], domain[1]],
                control_points,
                weights: Some(weights),
                periodic: false,
            });
            let id: cadmpeg_ir::ids::PcurveId = format!("cadir:model:pcurve#mixed.{index}").into();
            ir.model.pcurves.push(Pcurve {
                id: id.clone(),
                geometry: PcurveGeometry::Line { origin, direction },
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: Some(domain),
                fit_tolerance: Some(0.001),
            });
            ir.model.coedges[index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
                pcurve: id,
                isoparametric: None,
                parameter_range: None,
            }];
        }
        ir.finalize();
        ir
    }

    fn make_planar_nurbs_trimmed_face(ir: &mut CadIr) {
        use cadmpeg_ir::geometry::{NurbsSurface, Pcurve, PcurveGeometry, SurfaceGeometry};

        ir.model.surfaces[0].geometry = SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 4.0, 4.0],
            v_knots: vec![0.0, 0.0, 4.0, 4.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 4.0, 0.0),
                Point3::new(4.0, 0.0, 0.0),
                Point3::new(4.0, 4.0, 0.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        });
        for index in 0..ir.model.coedges.len() {
            let coedge = &ir.model.coedges[index];
            let edge = ir
                .model
                .edges
                .iter()
                .find(|edge| edge.id == coedge.edge)
                .expect("fixture edge");
            let domain = edge.param_range.expect("fixture edge domain");
            let (start, end) = if coedge.sense == cadmpeg_ir::topology::Sense::Forward {
                (&edge.start, &edge.end)
            } else {
                (&edge.end, &edge.start)
            };
            let start = vertex_point(&ir.model, start).expect("fixture start");
            let end = vertex_point(&ir.model, end).expect("fixture end");
            let scale = domain[1] - domain[0];
            let direction =
                cadmpeg_ir::math::Point2::new((end.x - start.x) / scale, (end.y - start.y) / scale);
            let origin = cadmpeg_ir::math::Point2::new(
                start.x - direction.u * domain[0],
                start.y - direction.v * domain[0],
            );
            let id: cadmpeg_ir::ids::PcurveId =
                format!("cadir:model:pcurve#general.{index}").into();
            ir.model.pcurves.push(Pcurve {
                id: id.clone(),
                geometry: PcurveGeometry::Line { origin, direction },
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: Some(domain),
                fit_tolerance: Some(0.0001),
            });
            ir.model.coedges[index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
                pcurve: id,
                isoparametric: None,
                parameter_range: None,
            }];
        }
        ir.finalize();
    }
}
