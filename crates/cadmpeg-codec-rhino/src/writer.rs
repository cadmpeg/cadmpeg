// SPDX-License-Identifier: Apache-2.0
//! Native Rhino 3DM archive writing.

use crate::mesh::FaceIndexWidth;
use std::io::{Seek, SeekFrom, Write};

mod model;
pub(crate) mod target;
use model::{
    WritableEdge, WritableEdgeCurve, WritableFaceSurface, WritableModel, WritableObjectCurve,
    WritablePcurve, WritableVertex,
};

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::LoopBoundaryRole;
use sha2::{Digest, Sha256};

use crate::chunks::{MAGIC, TCODE_ENDOFFILE, TCODE_SHORT};
use crate::RhinoArchiveVersion;

const EPS_WRITE_DEGENERATE: f64 = 1.0e-10;

pub(crate) trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

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
const OPENNURBS_WRITER_VERSION: i64 = 0xa000_0026;
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

pub(crate) fn write(
    ir: &CadIr,
    version: RhinoArchiveVersion,
    output: &mut dyn Write,
) -> Result<(), CodecError> {
    let mut staged = tempfile::tempfile()?;
    write_seekable(ir, version, &mut staged)?;
    staged.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut staged, output)?;
    Ok(())
}

pub(crate) fn write_seekable(
    ir: &CadIr,
    version: RhinoArchiveVersion,
    output: &mut dyn WriteSeek,
) -> Result<(), CodecError> {
    let mut plan = prepare_write(ir, version)?;
    write_archive_prefix(ir, version, output)?;
    let table_start = output.stream_position()?;
    output.write_all(&TCODE_OBJECT_TABLE.to_le_bytes())?;
    output.write_all(&0_i64.to_le_bytes())?;
    let body_start = output.stream_position()?;

    plan.brep_records.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut plan.brep_records, output)?;
    for point in ir
        .model
        .points
        .iter()
        .filter(|point| !plan.topology_points.contains(point.id.as_str()))
    {
        let position = point.position;
        let mut payload = vec![0x10];
        payload.extend(position.x.to_le_bytes());
        payload.extend(position.y.to_le_bytes());
        payload.extend(position.z.to_le_bytes());
        output.write_all(&attributed_object_record(
            1,
            POINT_CLASS,
            &payload,
            point.id.as_str(),
            None,
            None,
            None,
        )?)?;
    }
    for group in &plan.point_groups {
        if group.points.len() == 1 {
            let point = group.points[0];
            let mut payload = vec![0x10];
            payload.extend(point.x.to_le_bytes());
            payload.extend(point.y.to_le_bytes());
            payload.extend(point.z.to_le_bytes());
            output.write_all(&attributed_object_record(
                1,
                POINT_CLASS,
                &payload,
                &group.identity,
                group.name.as_deref(),
                group.color,
                group.visible,
            )?)?;
        } else {
            output.write_all(&attributed_object_record(
                2,
                POINT_CLOUD_CLASS,
                &point_cloud_payload(&group.points),
                &group.identity,
                group.name.as_deref(),
                group.color,
                group.visible,
            )?)?;
        }
    }
    for (id, curve) in &plan.curves {
        let (class, payload) = curve.payload();
        output.write_all(&attributed_object_record(
            4, class, &payload, id, None, None, None,
        )?)?;
    }
    for (id, surface) in &plan.surfaces {
        let (class, payload) = surface.payload();
        output.write_all(&attributed_object_record(
            8, class, &payload, id, None, None, None,
        )?)?;
    }
    for mesh in &ir.model.tessellations {
        let payload = mesh_payload(mesh, version);
        output.write_all(&mesh_object_record(&payload, mesh.id.as_str())?)?;
    }

    output.write_all(&short_chunk(TCODE_ENDOFTABLE, 0))?;
    let table_end = output.stream_position()?;
    let body_len = i64::try_from(table_end - body_start)
        .map_err(|_| CodecError::Malformed("3DM object table size overflow".into()))?;
    output.seek(SeekFrom::Start(table_start + 4))?;
    output.write_all(&body_len.to_le_bytes())?;
    output.seek(SeekFrom::Start(table_end))?;
    output.write_all(&table(TCODE_HISTORY_RECORD_TABLE, &[]))?;
    let final_size = output
        .stream_position()?
        .checked_add(20)
        .ok_or_else(|| CodecError::Malformed("3DM output size overflow".into()))?;
    output.write_all(&long_chunk(TCODE_ENDOFFILE, &final_size.to_le_bytes()))?;
    Ok(())
}

fn write_archive_prefix(
    ir: &CadIr,
    version: RhinoArchiveVersion,
    output: &mut dyn WriteSeek,
) -> Result<(), CodecError> {
    output.write_all(&header(version))?;
    output.write_all(&long_chunk(1, b"cadmpeg"))?;
    output.write_all(&table(
        TCODE_PROPERTIES_TABLE,
        &[short_chunk(
            TCODE_PROPERTIES_OPENNURBS_VERSION,
            OPENNURBS_WRITER_VERSION,
        )],
    ))?;
    output.write_all(&table(
        TCODE_SETTINGS_TABLE,
        &[units_record(
            ir.tolerances.linear.get(),
            ir.tolerances.angular.get(),
        )],
    ))?;
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
        output.write_all(&table(typecode, &[]))?;
        if typecode == TCODE_LINETYPE_TABLE {
            output.write_all(&table(
                TCODE_LAYER_TABLE,
                &[zero_crc_chunk(
                    TCODE_LAYER_RECORD,
                    &class_wrapper(LAYER_CLASS, &default_layer_payload()),
                )],
            ))?;
        }
    }
    Ok(())
}

struct BrepScope {
    ir: CadIr,
    points: std::collections::BTreeSet<String>,
    surfaces: std::collections::BTreeSet<String>,
    curves: std::collections::BTreeSet<String>,
    pcurves: std::collections::BTreeSet<String>,
}

struct BrepPayload {
    body: Vec<u8>,
    direct: Vec<u8>,
}

struct WritePlan<'a> {
    curves: Vec<(&'a str, WritableObjectCurve<'a>)>,
    surfaces: Vec<(&'a str, WritableFaceSurface<'a>)>,
    brep_records: std::fs::File,
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
            .map(|id| id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let shells = model
            .regions
            .iter()
            .filter(|region| regions.contains(region.id.as_str()))
            .flat_map(|region| region.shells.iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let faces = model
            .shells
            .iter()
            .filter(|shell| shells.contains(shell.id.as_str()))
            .flat_map(|shell| shell.faces().iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let surfaces = model
            .faces
            .iter()
            .filter(|face| faces.contains(face.id.as_str()))
            .map(|face| face.surface.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let loops = model
            .faces
            .iter()
            .filter(|face| faces.contains(face.id.as_str()))
            .flat_map(|face| face.loops.iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let coedges = model
            .loops
            .iter()
            .filter(|loop_| loops.contains(loop_.id.as_str()))
            .flat_map(|loop_| loop_.coedges().iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let edges = model
            .coedges
            .iter()
            .filter(|coedge| coedges.contains(coedge.id.as_str()))
            .map(|coedge| coedge.edge.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let pcurves = model
            .coedges
            .iter()
            .filter(|coedge| coedges.contains(coedge.id.as_str()))
            .filter_map(|coedge| {
                coedge
                    .pcurves
                    .first()
                    .map(|use_| &use_.pcurve)
                    .map(|id| id.as_str().to_owned())
            })
            .collect::<BTreeSet<_>>();
        let vertices = model
            .edges
            .iter()
            .filter(|edge| edges.contains(edge.id.as_str()))
            .flat_map(|edge| [edge.start.as_str().to_owned(), edge.end.as_str().to_owned()])
            .collect::<BTreeSet<_>>();
        let curves = model
            .edges
            .iter()
            .filter(|edge| edges.contains(edge.id.as_str()))
            .filter_map(|edge| edge.curve().as_ref().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let points = model
            .vertices
            .iter()
            .filter(|vertex| vertices.contains(vertex.id.as_str()))
            .map(|vertex| vertex.point.as_str().to_owned())
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

        let mut scoped = CadIr::empty();
        scoped.tolerances = ir.tolerances;
        scoped.model.bodies.push(body.clone());
        scoped.model.regions = model
            .regions
            .iter()
            .filter(|entity| regions.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.shells = model
            .shells
            .iter()
            .filter(|entity| shells.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.faces = model
            .faces
            .iter()
            .filter(|entity| faces.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.loops = model
            .loops
            .iter()
            .filter(|entity| loops.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.coedges = model
            .coedges
            .iter()
            .filter(|entity| coedges.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.edges = model
            .edges
            .iter()
            .filter(|entity| edges.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.vertices = model
            .vertices
            .iter()
            .filter(|entity| vertices.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.points = model
            .points
            .iter()
            .filter(|entity| points.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.surfaces = model
            .surfaces
            .iter()
            .filter(|entity| surfaces.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.curves = model
            .curves
            .iter()
            .filter(|entity| curves.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.pcurves = model
            .pcurves
            .iter()
            .filter(|entity| pcurves.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scopes.push(BrepScope {
            ir: scoped,
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

    let mut scoped = CadIr::empty();
    scoped.tolerances = ir.tolerances;
    scoped.model.bodies = ir
        .model
        .bodies
        .iter()
        .filter(|body| body.kind == BodyKind::General)
        .cloned()
        .collect();
    let bodies = scoped
        .model
        .bodies
        .iter()
        .map(|body| body.id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    scoped.model.regions = ir
        .model
        .regions
        .iter()
        .filter(|region| bodies.contains(region.body.as_str()))
        .cloned()
        .collect();
    let regions = scoped
        .model
        .regions
        .iter()
        .map(|region| region.id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    scoped.model.shells = ir
        .model
        .shells
        .iter()
        .filter(|shell| regions.contains(shell.region.as_str()))
        .cloned()
        .collect();
    let vertices = scoped
        .model
        .shells
        .iter()
        .flat_map(|shell| {
            shell
                .free_vertices()
                .iter()
                .map(|id| id.as_str().to_owned())
        })
        .collect::<BTreeSet<_>>();
    scoped.model.vertices = ir
        .model
        .vertices
        .iter()
        .filter(|vertex| vertices.contains(vertex.id.as_str()))
        .cloned()
        .collect();
    let points = scoped
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.as_str())
        .collect::<BTreeSet<_>>();
    scoped.model.points = ir
        .model
        .points
        .iter()
        .filter(|point| points.contains(point.id.as_str()))
        .cloned()
        .collect();
    scoped
}

fn prepare_write(
    ir: &CadIr,
    archive_version: RhinoArchiveVersion,
) -> Result<WritePlan<'_>, CodecError> {
    if ir.tolerances.angular.get() > std::f64::consts::PI {
        return Err(CodecError::Malformed(
            "Rhino angular tolerance must not exceed pi".into(),
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
    if i32::try_from(model.points.len()).is_err()
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
    let used_pcurves = breps
        .iter()
        .flat_map(|scope| scope.pcurves.iter())
        .collect::<std::collections::BTreeSet<_>>();
    if used_pcurves.len() != model.pcurves.len() {
        return Err(CodecError::NotImplemented(
            "pcurves without writable Brep coedge ownership are not writable".into(),
        ));
    }
    let topology_curves = breps
        .iter()
        .flat_map(|scope| scope.curves.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    let topology_surfaces = breps
        .iter()
        .flat_map(|scope| scope.surfaces.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    let general = general_topology_ir(ir);
    let scoped_count = |select: fn(&cadmpeg_ir::document::Model) -> usize| {
        breps
            .iter()
            .map(|scope| select(&scope.ir.model))
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
    for scope in &breps {
        topology_points.extend(scope.points.iter().cloned());
    }
    let curves = model
        .curves
        .iter()
        .filter(|curve| !topology_curves.contains(curve.id.as_str()))
        .map(|curve| {
            WritableObjectCurve::try_new(curve).map(|geometry| (curve.id.as_str(), geometry))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let surfaces = model
        .surfaces
        .iter()
        .filter(|surface| !topology_surfaces.contains(surface.id.as_str()))
        .map(|surface| {
            WritableFaceSurface::try_new(surface).map(|geometry| (surface.id.as_str(), geometry))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for mesh in &model.tessellations {
        check_mesh(mesh)?;
    }
    let mut brep_records = tempfile::tempfile()?;
    for scope in &breps {
        let body = &scope.ir.model.bodies[0];
        let model = WritableModel::try_new(&scope.ir)?;
        let payload = brep_payload(&model, archive_version)?;
        brep_records.write_all(&brep_object_record(
            &payload,
            body.id.as_str(),
            body.name.as_deref(),
            body.color,
            body.visible,
        )?)?;
    }
    Ok(WritePlan {
        curves,
        surfaces,
        brep_records,
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
    if namespace
        .arenas()
        .iter()
        .any(|(name, records)| !records.is_empty() && !REGENERATED.contains(&name.as_str()))
    {
        return false;
    }
    let opaque = namespace.arenas().get("opaque_records");
    let generated_comment = opaque.is_some_and(|records| {
        records.iter().any(|record| {
            let fields = record.fields();
            record.id() == "rhino:source:opaque#comment"
                && fields.get("typecode").and_then(serde_json::Value::as_str) == Some("0x00000001")
                && fields.get("data").and_then(serde_json::Value::as_str)
                    == Some("AQAAAAcAAAAAAAAAY2FkbXBlZw==")
        })
    });
    if (opaque.is_some() && !generated_comment)
        || opaque.is_some_and(|records| {
            records.iter().any(|record| {
                !matches!(
                    record
                        .field("typecode")
                        .as_ref()
                        .and_then(serde_json::Value::as_str),
                    Some("0x00000001" | "0xa0000026" | "0x20008031" | "0x20008050")
                )
            })
        })
    {
        return false;
    }
    if namespace
        .arenas()
        .get("layers")
        .is_some_and(|records| records.len() != 1 || !default_native_layer(&records[0]))
    {
        return false;
    }
    if namespace
        .arenas()
        .get("object_presentation")
        .is_some_and(|records| {
            records
                .iter()
                .any(|record| !default_native_presentation(record))
        })
    {
        return false;
    }
    namespace.arenas().get("unknowns").is_none_or(|records| {
        records.iter().all(|record| {
            record
                .field("links")
                .as_ref()
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
    let fields = record.fields();
    json_i64(&fields, "archive_index") == Some(0)
        && json_i64(&fields, "linetype_index") == Some(-1)
        && json_i64(&fields, "material_index") == Some(-1)
        && json_str(&fields, "name") == Some("Default")
        && json_bool(&fields, "visible") == Some(true)
        && json_bool(&fields, "locked") == Some(false)
        && json_array_empty(&fields, "rendering_materials")
        && json_array_empty_or_missing(&fields, "per_viewport_settings")
}

fn default_native_presentation(record: &cadmpeg_ir::NativeRecord) -> bool {
    let fields = record.fields();
    json_i64(&fields, "layer_index") == Some(0)
        && json_i64(&fields, "material_index") == Some(-1)
        && json_i64(&fields, "linetype_index") == Some(-1)
        && json_i64(&fields, "hatch_pattern_index") == Some(-1)
        && json_i64(&fields, "object_mode") == Some(0)
        && json_str(&fields, "name") == Some("")
        && json_str(&fields, "url") == Some("")
        && json_bool(&fields, "visible") == Some(true)
        && json_array_empty(&fields, "group_indexes")
        && json_array_empty(&fields, "display_materials")
        && json_array_empty(&fields, "rendering_materials")
        && json_array_empty_or_missing(&fields, "rendering_mappings")
        && fields.get("casts_shadows").is_none()
        && fields.get("receives_shadows").is_none()
        && fields.get("advanced_texture_preview").is_none()
        && fields.get("custom_render_mesh").is_none()
        && fields.get("mesh_modifiers").is_none()
        && json_array_empty(&fields, "clipping_plane_uuids")
        && json_array_empty_or_missing(&fields, "user_strings")
        && json_array_empty_or_missing(&fields, "attribute_user_strings")
}

type NativeFields = serde_json::Map<String, serde_json::Value>;

fn json_i64(fields: &NativeFields, name: &str) -> Option<i64> {
    fields.get(name)?.as_i64()
}

fn json_str<'a>(fields: &'a NativeFields, name: &str) -> Option<&'a str> {
    fields.get(name)?.as_str()
}

fn json_bool(fields: &NativeFields, name: &str) -> Option<bool> {
    fields.get(name)?.as_bool()
}

fn json_array_empty(fields: &NativeFields, name: &str) -> bool {
    fields
        .get(name)
        .and_then(serde_json::Value::as_array)
        .is_some_and(Vec::is_empty)
}

fn json_array_empty_or_missing(fields: &NativeFields, name: &str) -> bool {
    fields
        .get(name)
        .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty))
}

fn brep_payload(
    model: &WritableModel<'_>,
    archive_version: RhinoArchiveVersion,
) -> Result<BrepPayload, CodecError> {
    use cadmpeg_ir::topology::{BodyKind, Sense};

    let version = if archive_version.uses_extended_brep_layout() {
        0x33
    } else {
        0x32
    };
    let mut payload = vec![version];
    let mut direct = vec![version];
    payload.extend(polymorphic_array(
        model.coedges.iter().map(|coedge| &coedge.c2),
    ));
    let c3 = model
        .edges
        .iter()
        .map(|edge| brep_c3_curve(model, edge))
        .collect::<Vec<_>>();
    payload.extend(polymorphic_array(c3.iter()));
    let surfaces = model
        .surfaces
        .iter()
        .map(|surface| surface.payload())
        .collect::<Vec<_>>();
    payload.extend(polymorphic_array(surfaces.iter()));
    let vertices = model
        .vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| {
            let incident =
                wire_indexes(model.edges.iter().enumerate().flat_map(|(position, edge)| {
                    [edge.start, edge.end]
                        .into_iter()
                        .filter(move |endpoint| *endpoint == index)
                        .map(move |_| position)
                }))?;
            let mut record = wire_index(index)?.to_le_bytes().to_vec();
            for value in [vertex.point.x, vertex.point.y, vertex.point.z] {
                record.extend(value.to_le_bytes());
            }
            record.extend(indexes(&incident)?);
            record.extend(
                vertex
                    .source
                    .tolerance
                    .map_or(0.0, cadmpeg_ir::units::PositiveScalar::get)
                    .to_le_bytes(),
            );
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(raw_array(&vertices));
    let edges = model
        .edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            let index = wire_index(index)?;
            let mut record = index.to_le_bytes().to_vec();
            record.extend(index.to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            record.extend(edge.domain.into_iter().flat_map(f64::to_le_bytes));
            record.extend(wire_index(edge.start)?.to_le_bytes());
            record.extend(wire_index(edge.end)?.to_le_bytes());
            record.extend(indexes(&wire_indexes(edge.uses.iter().copied())?)?);
            record.extend(
                edge.source
                    .tolerance
                    .map_or(0.0, cadmpeg_ir::units::PositiveScalar::get)
                    .to_le_bytes(),
            );
            record.extend(edge.domain.into_iter().flat_map(f64::to_le_bytes));
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(raw_array(&edges));
    let trims = model
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| {
            let edge = &model.edges[coedge.edge];
            let (from, to) = model.endpoints(index);
            let index = wire_index(index)?;
            let mut record = index.to_le_bytes().to_vec();
            record.extend(index.to_le_bytes());
            record.extend(edge.domain.into_iter().flat_map(f64::to_le_bytes));
            record.extend(wire_index(coedge.edge)?.to_le_bytes());
            record.extend(wire_index(from)?.to_le_bytes());
            record.extend(wire_index(to)?.to_le_bytes());
            record.extend(i32::from(coedge.source.sense == Sense::Reversed).to_le_bytes());
            let same_loop = edge.uses.len() == 2
                && model.coedges[edge.uses[0]].owner_loop == model.coedges[edge.uses[1]].owner_loop;
            record.extend(brep_trim_type(edge.uses.len(), same_loop).to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            record.extend(wire_index(coedge.owner_loop)?.to_le_bytes());
            record.extend(
                [coedge.fit_tolerance, 0.0_f64]
                    .into_iter()
                    .flat_map(f64::to_le_bytes),
            );
            record.extend(edge.domain.into_iter().flat_map(f64::to_le_bytes));
            record.push(0);
            record.extend([0_u8; 31]);
            record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(raw_array(&trims));
    let loops = model
        .loops
        .iter()
        .enumerate()
        .map(|(index, loop_)| {
            let face = &model.faces[loop_.face];
            let mut record = wire_index(index)?.to_le_bytes().to_vec();
            record.extend(indexes(&wire_indexes(loop_.coedges.iter().copied())?)?);
            record.extend(
                brep_loop_type(
                    face.source.loop_role(&loop_.source.id),
                    face.loops.first() == Some(&index),
                )
                .to_le_bytes(),
            );
            record.extend(wire_index(loop_.face)?.to_le_bytes());
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(raw_array(&loops));
    let faces = model
        .faces
        .iter()
        .enumerate()
        .map(|(index, face)| {
            let mut record = wire_index(index)?.to_le_bytes().to_vec();
            record.extend(indexes(&wire_indexes(face.loops.iter().copied())?)?);
            record.extend(wire_index(face.surface)?.to_le_bytes());
            record.extend(i32::from(face.source.sense == Sense::Reversed).to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(face_array(
        &faces,
        &model
            .faces
            .iter()
            .map(|face| face.source)
            .collect::<Vec<_>>(),
        archive_version,
    ));
    let min = model.vertices.iter().fold([f64::INFINITY; 3], |a, vertex| {
        let p = vertex.point;
        [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
    });
    let max = model
        .vertices
        .iter()
        .fold([f64::NEG_INFINITY; 3], |a, vertex| {
            let p = vertex.point;
            [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
        });
    for value in min.into_iter().chain(max) {
        let bytes = value.to_le_bytes();
        payload.extend(bytes);
        direct.extend(bytes);
    }
    let mesh_presence = alloc_filled(model.faces.len(), 0_u8, "Rhino Brep mesh presence")?;
    payload.extend(crc_chunk(0x4000_8000, &mesh_presence));
    payload.extend(crc_chunk(0x4000_8000, &mesh_presence));
    let solid = if model.body.kind == BodyKind::Solid {
        planar_solid_orientation(model)
    } else {
        0
    }
    .to_le_bytes();
    payload.extend(solid);
    direct.extend(solid);
    if archive_version.uses_extended_brep_layout() {
        payload.extend(empty_region_wrapper());
    }
    Ok(BrepPayload {
        body: payload,
        direct,
    })
}

fn brep_trim_type(edge_use_count: usize, two_uses_in_one_loop: bool) -> i32 {
    if edge_use_count == 1 {
        1
    } else if two_uses_in_one_loop {
        3
    } else {
        2
    }
}

fn brep_loop_type(role: LoopBoundaryRole, first_on_face: bool) -> i32 {
    match role {
        LoopBoundaryRole::Outer => 1,
        LoopBoundaryRole::Inner => 2,
        LoopBoundaryRole::Unspecified if first_on_face => 1,
        LoopBoundaryRole::Unspecified => 2,
    }
}

fn planar_solid_orientation(model: &WritableModel<'_>) -> i32 {
    let mut volume6 = 0.0;
    for loop_ in &model.loops {
        let ring = loop_
            .coedges
            .iter()
            .map(|coedge| model.vertices[model.endpoints(*coedge).0].point)
            .collect::<Vec<_>>();
        if let Some(origin) = ring.first() {
            for triangle in ring[1..].windows(2) {
                let a = triangle[0];
                let b = triangle[1];
                volume6 += origin.x * (a.y * b.z - a.z * b.y)
                    + origin.y * (a.z * b.x - a.x * b.z)
                    + origin.z * (a.x * b.y - a.y * b.x);
            }
        }
    }
    if volume6 > 0.0 {
        1
    } else if volume6 < 0.0 {
        2
    } else {
        0
    }
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

fn brep_c3_curve(model: &WritableModel<'_>, edge: &WritableEdge<'_>) -> ([u8; 16], Vec<u8>) {
    match edge.curve {
        WritableEdgeCurve::Line(_) => {
            let from = model.vertices[edge.start].point;
            let to = model.vertices[edge.end].point;
            (
                LINE_CLASS,
                bounded_line_payload([from.x, from.y, from.z], [to.x, to.y, to.z], edge.domain, 3),
            )
        }
        WritableEdgeCurve::Nurbs(nurbs) => (NURBS_CURVE_CLASS, nurbs_curve_payload(nurbs)),
    }
}

fn generated_projected_brep_c2_curve(
    vertices: &[WritableVertex<'_>],
    edge: &WritableEdge<'_>,
    sense: cadmpeg_ir::topology::Sense,
    origin: cadmpeg_ir::math::Point3,
    u_axis: cadmpeg_ir::math::Vector3,
    v_axis: cadmpeg_ir::math::Vector3,
) -> Result<([u8; 16], Vec<u8>), CodecError> {
    use cadmpeg_ir::topology::Sense;

    Ok(match edge.curve {
        WritableEdgeCurve::Line(_) => {
            let (from, to) = if sense == Sense::Forward {
                (edge.start, edge.end)
            } else {
                (edge.end, edge.start)
            };
            let from = vertices[from].point;
            let to = vertices[to].point;
            (
                LINE_CLASS,
                bounded_line_payload(
                    plane_uv(from, origin, u_axis, v_axis),
                    plane_uv(to, origin, u_axis, v_axis),
                    edge.domain,
                    2,
                ),
            )
        }
        WritableEdgeCurve::Nurbs(nurbs) => {
            let mut projected = nurbs.clone();
            projected
                .edit_control_points(|points| {
                    for point in points {
                        let uv = plane_uv(*point, origin, u_axis, v_axis);
                        *point = cadmpeg_ir::math::Point3::new(uv[0], uv[1], 0.0);
                    }
                })
                .map_err(|error| CodecError::malformed(error.to_string()))?;
            if sense == Sense::Reversed {
                let sum = projected.knots()[projected.degree() as usize]
                    + projected.knots()[projected.control_points().len()];
                projected.reverse_parameterization();
                projected
                    .edit_knots(|knots| {
                        for knot in knots {
                            *knot += sum;
                        }
                    })
                    .map_err(|error| CodecError::malformed(error.to_string()))?;
                canonicalize_native_curve_knots(&mut projected, edge.curve_id)?;
            }
            (
                NURBS_CURVE_CLASS,
                nurbs_curve_payload_dimension(&projected, 2),
            )
        }
    })
}

fn canonicalize_native_curve_knots(
    curve: &mut cadmpeg_ir::geometry::NurbsCurve,
    id: &str,
) -> Result<(), CodecError> {
    let order = curve.degree() as usize + 1;
    let count = curve.control_points().len();
    let stored = curve.knots()[1..curve.knots().len() - 1].to_vec();
    let reconstructed = crate::surfaces::reconstruct_knots(&stored, order, count)
        .map_err(|error| CodecError::malformed(format_args!("curve {id}: {error}")))?;
    curve
        .edit_knots(|knots| knots.copy_from_slice(&reconstructed))
        .map_err(|error| CodecError::malformed(format_args!("curve {id}: {error}")))?;
    Ok(())
}

fn admit_pcurve<'a>(
    edge: &WritableEdge<'_>,
    pcurve: &'a cadmpeg_ir::geometry::Pcurve,
) -> Result<WritablePcurve<'a>, CodecError> {
    if pcurve.wrapper_reversed() == Some(true)
        || pcurve.native_tail_flags().is_some()
        || pcurve
            .parameter_range()
            .is_some_and(|range| range != edge.domain)
        || pcurve
            .fit_tolerance()
            .is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return Err(CodecError::NotImplemented(format!(
            "pcurve {} has unsupported wrapper, tail, domain, or tolerance state",
            pcurve.id.as_str()
        )));
    }
    let domain = edge.domain;
    let (payload, domain_extent_points) = match &pcurve.geometry {
        cadmpeg_ir::geometry::PcurveGeometry::Line(line) => {
            let origin = line.origin();
            let direction = line.direction();
            if !origin.u.is_finite()
                || !origin.v.is_finite()
                || !direction.u.is_finite()
                || !direction.v.is_finite()
                || direction.u == 0.0 && direction.v == 0.0
            {
                return Err(CodecError::malformed(format_args!(
                    "pcurve {} has invalid line geometry",
                    pcurve.id.as_str()
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
            (
                (LINE_CLASS, bounded_line_payload(from, to, domain, 2)),
                vec![
                    cadmpeg_ir::math::Point2::new(from[0], from[1]),
                    cadmpeg_ir::math::Point2::new(to[0], to[1]),
                ],
            )
        }
        cadmpeg_ir::geometry::PcurveGeometry::Nurbs { nurbs } => {
            let curve = cadmpeg_ir::geometry::NurbsCurve::new(
                nurbs.degree(),
                nurbs.knots().to_vec(),
                nurbs
                    .control_points()
                    .iter()
                    .map(|point| cadmpeg_ir::math::Point3::new(point.u, point.v, 0.0))
                    .collect(),
                nurbs.weights().map(<[f64]>::to_vec),
                nurbs.periodic(),
            )
            .map_err(|error| {
                CodecError::malformed(format_args!("pcurve {}: {error}", pcurve.id.as_str()))
            })?;
            check_nurbs_curve(pcurve.id.as_str(), &curve)?;
            let count = curve.control_points().len();
            if curve.periodic()
                || [curve.knots()[curve.degree() as usize], curve.knots()[count]] != domain
            {
                return Err(CodecError::NotImplemented(format!(
                    "pcurve {} is not a nonperiodic full-domain NURBS curve",
                    pcurve.id.as_str()
                )));
            }
            (
                (NURBS_CURVE_CLASS, nurbs_curve_payload_dimension(&curve, 2)),
                nurbs.control_points().to_vec(),
            )
        }
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "pcurve {} geometry is not writable as Rhino Brep trim geometry",
                pcurve.id.as_str()
            )))
        }
    };
    Ok(WritablePcurve {
        source: pcurve,
        payload,
        domain_extent_points,
    })
}

fn validate_nurbs_trim(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    face_tolerance: f64,
    edge: &WritableEdge<'_>,
    sense: cadmpeg_ir::topology::Sense,
    explicit: &WritablePcurve<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::eval::{nurbs_surface_point, pcurve_uv};
    use cadmpeg_ir::topology::Sense;

    let u_count = surface.u_count() as usize;
    let v_count = surface.v_count() as usize;
    let u_domain = [
        surface.u_knots()[surface.u_degree() as usize],
        surface.u_knots()[u_count],
    ];
    let v_domain = [
        surface.v_knots()[surface.v_degree() as usize],
        surface.v_knots()[v_count],
    ];
    let pcurve = explicit.source;
    let domain = edge.domain;
    let uv_epsilon = EPS_WRITE_DEGENERATE
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
    let domain_extent_inside = explicit
        .domain_extent_points
        .iter()
        .all(|point| inside_domain(point.u, point.v));
    if !domain_extent_inside {
        return Err(CodecError::malformed(format_args!(
            "pcurve {} leaves its NURBS surface parameter domain",
            pcurve.id.as_str()
        )));
    }
    let mut breaks = vec![domain[0], domain[1]];
    if let WritableEdgeCurve::Nurbs(nurbs) = edge.curve {
        breaks.extend(
            nurbs
                .knots()
                .iter()
                .copied()
                .filter(|value| *value > domain[0] && *value < domain[1])
                .map(|value| {
                    if sense == Sense::Forward {
                        value
                    } else {
                        domain[0] + domain[1] - value
                    }
                }),
        );
    }
    if let cadmpeg_ir::geometry::PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry {
        breaks.extend(
            nurbs
                .knots()
                .iter()
                .copied()
                .filter(|value| *value > domain[0] && *value < domain[1]),
        );
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();

    let tolerance = face_tolerance
        .max(
            edge.source
                .tolerance
                .map_or(0.0, cadmpeg_ir::units::PositiveScalar::get),
        )
        .max(pcurve.fit_tolerance().unwrap_or(0.0))
        .max(EPS_WRITE_DEGENERATE);
    for span in breaks.windows(2) {
        for step in 0..=16 {
            let fraction = f64::from(step) / 16.0;
            let parameter = span[0] + (span[1] - span[0]) * fraction;
            let uv = pcurve_uv(&pcurve.geometry, parameter).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "pcurve {} cannot be evaluated over its edge domain",
                    pcurve.id.as_str()
                ))
            })?;
            if uv.u < u_domain[0] - uv_epsilon
                || uv.u > u_domain[1] + uv_epsilon
                || uv.v < v_domain[0] - uv_epsilon
                || uv.v > v_domain[1] + uv_epsilon
            {
                return Err(CodecError::malformed(format_args!(
                    "pcurve {} leaves its NURBS surface parameter domain",
                    pcurve.id.as_str()
                )));
            }
            let mapped = nurbs_surface_point(surface, uv.u, uv.v).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "pcurve {} cannot be evaluated through its NURBS surface",
                    pcurve.id.as_str()
                ))
            })?;
            let curve_parameter = if sense == Sense::Forward {
                parameter
            } else {
                domain[0] + domain[1] - parameter
            };
            let edge_point = edge.curve.point(curve_parameter).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "edge curve {} cannot be evaluated over its edge domain",
                    edge.curve_id
                ))
            })?;
            let distance = ((mapped.x - edge_point.x).powi(2)
                + (mapped.y - edge_point.y).powi(2)
                + (mapped.z - edge_point.z).powi(2))
            .sqrt();
            if !distance.is_finite() || distance > tolerance {
                return Err(CodecError::malformed(format_args!(
                    "pcurve {} misses directed edge curve {} by {distance}",
                    pcurve.id.as_str(),
                    edge.curve_id
                )));
            }
        }
    }
    Ok(())
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

fn polymorphic_array<'a>(
    children: impl ExactSizeIterator<Item = &'a ([u8; 16], Vec<u8>)>,
) -> Vec<u8> {
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

fn face_array(
    records: &[Vec<u8>],
    faces: &[&cadmpeg_ir::topology::Face],
    archive_version: RhinoArchiveVersion,
) -> Vec<u8> {
    let version_two = archive_version.uses_face_array_v2();
    let minor = if version_two { 2 } else { 1 };
    let mut body = vec![0x10 | minor];
    body.extend((records.len() as i32).to_le_bytes());
    body.extend(records.concat());
    for face in faces {
        body.extend(&Sha256::digest(face.id.as_str().as_bytes())[..16]);
    }
    if version_two {
        body.push(0);
    }
    crc_chunk(0x4000_8000, &body)
}

fn empty_region_wrapper() -> Vec<u8> {
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(1_i32.to_le_bytes());
    body.push(0);
    crc_chunk(0x4000_8000, &body)
}

/// Converts one arena position into the native index the Brep records store.
fn wire_index(position: usize) -> Result<i32, CodecError> {
    i32::try_from(position).map_err(|_| {
        CodecError::Malformed("Brep record index exceeds the native index range".into())
    })
}

/// Converts a list of arena positions into the native indexes they store as.
fn wire_indexes(positions: impl IntoIterator<Item = usize>) -> Result<Vec<i32>, CodecError> {
    positions.into_iter().map(wire_index).collect()
}

fn indexes(values: &[i32]) -> Result<Vec<u8>, CodecError> {
    let mut bytes = wire_index(values.len())?.to_le_bytes().to_vec();
    bytes.extend(values.iter().flat_map(|value| value.to_le_bytes()));
    Ok(bytes)
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
    let vertex_count = mesh.vertices().len();
    if vertex_count == 0 || vertex_count > (1 << 24) || mesh.triangles().len() > (1 << 24) {
        return Err(CodecError::malformed(format_args!(
            "mesh {} has invalid native counts",
            mesh.id
        )));
    }
    if mesh.body.is_some() || !mesh.strip_lengths().is_empty() {
        return Err(CodecError::NotImplemented(format!(
            "mesh {} uses body binding or strips not yet writable",
            mesh.id
        )));
    }
    if !mesh.feature_edges().is_empty() || !mesh.per_corner_normals().is_empty() {
        return Err(CodecError::NotImplemented(format!(
            "mesh {} uses feature edges or corner normals not yet writable",
            mesh.id
        )));
    }
    if !mesh.triangle_groups().is_empty() || !mesh.texture_assignments().is_empty() {
        return Err(CodecError::NotImplemented(format!(
            "mesh {} uses triangle groups or texture assignments not yet writable",
            mesh.id
        )));
    }
    if !mesh.vertex_normals().is_empty() && mesh.vertex_normals().len() != vertex_count {
        return Err(CodecError::malformed(format_args!(
            "mesh {} normal count mismatch",
            mesh.id
        )));
    }
    if mesh.vertices().iter().any(|p| {
        !p.x.is_finite()
            || !p.y.is_finite()
            || !p.z.is_finite()
            || !(p.x as f32).is_finite()
            || !(p.y as f32).is_finite()
            || !(p.z as f32).is_finite()
    }) || mesh.vertex_normals().iter().any(|n| {
        !n.x.is_finite()
            || !n.y.is_finite()
            || !n.z.is_finite()
            || !(n.x as f32).is_finite()
            || !(n.y as f32).is_finite()
            || !(n.z as f32).is_finite()
    }) {
        return Err(CodecError::malformed(format_args!(
            "mesh {} contains non-finite native values",
            mesh.id
        )));
    }
    if mesh
        .triangles()
        .iter()
        .flatten()
        .any(|index| *index as usize >= vertex_count)
    {
        return Err(CodecError::malformed(format_args!(
            "mesh {} index is out of range",
            mesh.id
        )));
    }
    let mut kinds = std::collections::BTreeSet::new();
    for channel in mesh.channels() {
        let expected = match channel.kind() {
            CHANNEL_UV => 8,
            CHANNEL_COLOR => 4,
            CHANNEL_SURFACE_PARAMETERS | CHANNEL_CURVATURE => 16,
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "mesh {} channel kind {:#x} is not writable",
                    mesh.id,
                    channel.kind()
                )));
            }
        };
        if !kinds.insert(channel.kind())
            || channel.flags() != 0
            || channel.item_size() != expected
            || channel.count() as usize != vertex_count
            || channel.data().len() != vertex_count * expected as usize
        {
            return Err(CodecError::malformed(format_args!(
                "mesh {} channel {:#x} has invalid metadata",
                mesh.id,
                channel.kind()
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
                body.id.as_str()
            )));
        }
        check_object_attributes(body.id.as_str(), body.name.as_deref())?;
        let region = model
            .regions
            .iter()
            .find(|region| region.id == body.regions[0])
            .ok_or_else(|| {
                CodecError::malformed(format_args!("body {} region is missing", body.id.as_str()))
            })?;
        if region.body != body.id
            || region.shells.len() != 1
            || !regions.insert(region.id.as_str().to_owned())
        {
            return Err(CodecError::malformed(format_args!(
                "body {} region graph is invalid",
                body.id.as_str()
            )));
        }
        let shell = model
            .shells
            .iter()
            .find(|shell| shell.id == region.shells[0])
            .ok_or_else(|| {
                CodecError::malformed(format_args!("body {} shell is missing", body.id.as_str()))
            })?;
        if shell.region != region.id
            || !shell.faces().is_empty()
            || !shell.wire_edges().is_empty()
            || shell.free_vertices().is_empty()
            || !shells.insert(shell.id.as_str().to_owned())
        {
            return Err(CodecError::malformed(format_args!(
                "body {} shell graph is invalid",
                body.id.as_str()
            )));
        }
        let mut group = Vec::with_capacity(shell.free_vertices().len());
        for vertex_id in shell.free_vertices() {
            let vertex = model
                .vertices
                .iter()
                .find(|vertex| vertex.id == *vertex_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("vertex {} is missing", vertex_id.as_str()))
                })?;
            if vertex.tolerance.is_some() || !vertices.insert(vertex.id.as_str().to_owned()) {
                return Err(CodecError::NotImplemented(format!(
                    "vertex {} has tolerance or multiple ownership",
                    vertex.id.as_str()
                )));
            }
            let point = model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "point {} is missing",
                        vertex.point.as_str()
                    ))
                })?;
            if !points.insert(point.id.as_str().to_owned()) {
                return Err(CodecError::NotImplemented(format!(
                    "point {} is shared by multiple free vertices",
                    point.id.as_str()
                )));
            }
            group.push(point.position);
        }
        groups.push(PointGroup {
            points: group,
            identity: body.id.as_str().to_owned(),
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
    normal: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
    family: &str,
) -> Result<(), CodecError> {
    let dot = normal.x * x.x + normal.y * x.y + normal.z * x.z;
    if (normal.norm() - 1.0).abs() > EPS_WRITE_DEGENERATE
        || (x.norm() - 1.0).abs() > EPS_WRITE_DEGENERATE
        || dot.abs() > EPS_WRITE_DEGENERATE
    {
        return Err(CodecError::malformed(format_args!(
            "{family} {id} frame is not orthonormal to Rhino's tighter bound {EPS_WRITE_DEGENERATE}"
        )));
    }
    Ok(())
}

fn check_nurbs_surface(
    id: &str,
    surface: &cadmpeg_ir::geometry::NurbsSurface,
) -> Result<(), CodecError> {
    let u_order = surface.u_degree() as usize + 1;
    let v_order = surface.v_degree() as usize + 1;
    let u_count = surface.u_count() as usize;
    let v_count = surface.v_count() as usize;
    if u_order < 2
        || v_order < 2
        || i32::try_from(u_order).is_err()
        || i32::try_from(v_order).is_err()
        || i32::try_from(u_count).is_err()
        || i32::try_from(v_count).is_err()
        || i32::try_from(surface.control_points().len()).is_err()
    {
        return Err(CodecError::malformed(format_args!(
            "surface {id} cannot be represented by Rhino NURBS counts"
        )));
    }
    check_knot_roundtrip(
        id,
        "surface U",
        surface.u_knots(),
        u_order,
        u_count,
        surface.u_periodic(),
    )?;
    check_knot_roundtrip(
        id,
        "surface V",
        surface.v_knots(),
        v_order,
        v_count,
        surface.v_periodic(),
    )?;
    Ok(())
}

fn check_nurbs_curve(id: &str, curve: &cadmpeg_ir::geometry::NurbsCurve) -> Result<(), CodecError> {
    let order = curve.degree() as usize + 1;
    let count = curve.control_points().len();
    if i32::try_from(order).is_err() || i32::try_from(count).is_err() || order < 2 {
        return Err(CodecError::malformed(format_args!(
            "curve {id} cannot be represented by Rhino NURBS counts"
        )));
    }
    check_knot_roundtrip(id, "curve", curve.knots(), order, count, curve.periodic())?;
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
        return Err(CodecError::malformed(format_args!(
            "{direction} {id} has a non-increasing native NURBS domain"
        )));
    }
    let reconstructed = crate::surfaces::reconstruct_knots(stored, order, count)
        .map_err(|error| CodecError::malformed(format_args!("{direction} {id}: {error}")))?;
    let periodic = crate::surfaces::periodic_knots(stored, order, count);
    if reconstructed != full || periodic != declared_periodic {
        return Err(CodecError::malformed(format_args!(
            "{direction} {id} knot endpoints or periodic flag are not native-canonical"
        )));
    }
    Ok(())
}

fn header(version: RhinoArchiveVersion) -> Vec<u8> {
    let text = version.value().to_string();
    let mut bytes = MAGIC.to_vec();
    bytes.extend(std::iter::repeat_n(b' ', 8 - text.len()));
    bytes.extend(text.bytes());
    bytes
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
    let rational = i32::from(curve.weights().is_some());
    let order = (curve.degree() + 1) as i32;
    let count = curve.control_points().len() as i32;
    let mut payload = vec![0x10];
    for value in [dimension, rational, order, count, 0, 0] {
        payload.extend(value.to_le_bytes());
    }
    let min = curve
        .control_points()
        .iter()
        .fold([f64::INFINITY; 3], |a, p| {
            [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
        });
    let max = curve
        .control_points()
        .iter()
        .fold([f64::NEG_INFINITY; 3], |a, p| {
            [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
        });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(((curve.knots().len() - 2) as i32).to_le_bytes());
    for knot in &curve.knots()[1..curve.knots().len() - 1] {
        payload.extend(knot.to_le_bytes());
    }
    payload.extend(count.to_le_bytes());
    for (index, point) in curve.control_points().iter().enumerate() {
        let weight = curve.weights().map_or(1.0, |weights| weights[index]);
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
    let y = normal.cross(x);
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
    let rational = i32::from(surface.weights().is_some());
    let mut payload = vec![0x10];
    for value in [
        3,
        rational,
        (surface.u_degree() + 1) as i32,
        (surface.v_degree() + 1) as i32,
        surface.u_count() as i32,
        surface.v_count() as i32,
        0,
        0,
    ] {
        payload.extend(value.to_le_bytes());
    }
    let min = surface
        .control_points()
        .iter()
        .fold([f64::INFINITY; 3], |a, p| {
            [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
        });
    let max = surface
        .control_points()
        .iter()
        .fold([f64::NEG_INFINITY; 3], |a, p| {
            [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
        });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    for knots in [surface.u_knots(), surface.v_knots()] {
        payload.extend(((knots.len() - 2) as i32).to_le_bytes());
        for knot in &knots[1..knots.len() - 1] {
            payload.extend(knot.to_le_bytes());
        }
    }
    payload.extend((surface.control_points().len() as i32).to_le_bytes());
    for (index, point) in surface.control_points().iter().enumerate() {
        let weight = surface.weights().map_or(1.0, |weights| weights[index]);
        payload.extend((point.x * weight).to_le_bytes());
        payload.extend((point.y * weight).to_le_bytes());
        payload.extend((point.z * weight).to_le_bytes());
        if rational != 0 {
            payload.extend(weight.to_le_bytes());
        }
    }
    payload
}

struct MeshPayload {
    body: Vec<u8>,
    direct: Vec<u8>,
}

fn mesh_payload(
    mesh: &cadmpeg_ir::tessellation::Tessellation,
    archive_version: RhinoArchiveVersion,
) -> MeshPayload {
    let writes_double_vertices = archive_version.stores_mesh_vertices_as_f64();
    let payload_version = if writes_double_vertices { 0x38 } else { 0x35 };
    let mut payload = vec![payload_version];
    payload.extend((mesh.vertices().len() as i32).to_le_bytes());
    payload.extend((mesh.triangles().len() as i32).to_le_bytes());
    for _ in 0..4 {
        payload.extend(0.0_f64.to_le_bytes());
        payload.extend(1.0_f64.to_le_bytes());
    }
    payload.extend([0_u8; 16]);
    payload.extend([0_u8; 16 * 4]);
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0_u8; 5]);

    let width = if mesh.vertices().len() < 256 {
        FaceIndexWidth::One
    } else if mesh.vertices().len() < 65_536 {
        FaceIndexWidth::Two
    } else {
        FaceIndexWidth::Four
    };
    payload.extend((width.bytes() as i32).to_le_bytes());
    for triangle in mesh.triangles() {
        for index in [triangle[0], triangle[1], triangle[2], triangle[2]] {
            match width {
                FaceIndexWidth::One => payload.push(index as u8),
                FaceIndexWidth::Two => payload.extend((index as u16).to_le_bytes()),
                FaceIndexWidth::Four => payload.extend(index.to_le_bytes()),
            }
        }
    }

    let float_vertices = mesh
        .vertices()
        .iter()
        .flat_map(|point| {
            [point.x as f32, point.y as f32, point.z as f32]
                .into_iter()
                .flat_map(f32::to_le_bytes)
        })
        .collect::<Vec<_>>();
    let normals = mesh
        .vertex_normals()
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
    if writes_double_vertices {
        payload.push(0);
        direct.push(0);
        payload.push(1);
        direct.push(1);
        let doubles = mesh
            .vertices()
            .iter()
            .flat_map(|point| {
                [point.x, point.y, point.z]
                    .into_iter()
                    .flat_map(f64::to_le_bytes)
            })
            .collect::<Vec<_>>();
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        body.extend((mesh.vertices().len() as u32).to_le_bytes());
        body.extend(mesh_buffer(&doubles));
        payload.extend(crc_chunk(0x4000_8000, &body));
        let min = mesh.vertices().iter().fold([f64::INFINITY; 3], |a, point| {
            [a[0].min(point.x), a[1].min(point.y), a[2].min(point.z)]
        });
        let max = mesh
            .vertices()
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
    mesh.channels()
        .iter()
        .find(|channel| channel.kind() == kind)
        .map_or(&[], |channel| channel.data())
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
    check_object_attributes(identity, name)?;
    Ok(framed_object_record(
        object_type,
        class_uuid,
        payload,
        None,
        Some(object_attributes_payload(identity, name, color, visible)),
    ))
}

fn mesh_object_record(payload: &MeshPayload, identity: &str) -> Result<Vec<u8>, CodecError> {
    check_object_attributes(identity, None)?;
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
    check_object_attributes(identity, name)?;
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

fn check_object_attributes(identity: &str, name: Option<&str>) -> Result<(), CodecError> {
    if identity.is_empty() || name.is_some_and(|value| value.contains('\0')) {
        return Err(CodecError::malformed(format_args!(
            "object {identity} has an invalid identity or name"
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
            unit_color_channel(color.r()),
            unit_color_channel(color.g()),
            unit_color_channel(color.b()),
            unit_color_channel(1.0 - color.a()),
        ]);
    }
    if let Some(visible) = visible {
        payload.extend([11, u8::from(visible)]);
    }
    if color.is_some() {
        payload.extend([13, 1]);
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
    if !units.is_empty() {
        units.push(0);
    }
    let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
    for unit in units {
        bytes.extend(unit.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests;
