// SPDX-License-Identifier: Apache-2.0
//! Native Rhino 3DM archive writing.

use crate::mesh::FaceIndexWidth;
use std::collections::BTreeSet;
use std::io::{Seek, SeekFrom, Write};

mod display;
mod layers;
mod model;
mod native;
pub(crate) mod target;
use display::{Displays, ObjectDisplay};
use model::{
    WritableEdge, WritableEdgeCurve, WritableFaceSurface, WritableModel, WritableObjectCurve,
    WritablePcurve, WritableVertex,
};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::LoopBoundaryRole;
use sha2::{Digest, Sha256};

use crate::chunks::{MAGIC, TCODE_ENDOFFILE, TCODE_SHORT};
use crate::loss::RhinoLossCode;
use crate::RhinoArchiveVersion;

const EPS_WRITE_DEGENERATE: f64 = 1.0e-10;

/// The linear tolerance the Rhino Brep write route compares evaluated
/// geometry against: the stated per-entity tolerance, else the document linear
/// tolerance, at least `EPS_WRITE_DEGENERATE`.
fn stated_write_tolerance(stated: Option<cadmpeg_ir::scalar::PositiveReal>, ir: &CadIr) -> f64 {
    stated
        .map_or_else(|| ir.tolerances.linear.get(), |tolerance| tolerance.get())
        .max(EPS_WRITE_DEGENERATE)
}

trait WriteSeek: Write + Seek {}
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

fn is_support(source: Option<&cadmpeg_ir::SourceObjectAssociation>) -> bool {
    source
        .is_some_and(|source| source.geometry_role == Some(cadmpeg_ir::SourceGeometryRole::Support))
}

/// Writes `ir` as a 3DM archive and returns the losses of what the archive
/// does not state.
fn write(
    ir: &CadIr,
    version: RhinoArchiveVersion,
    output: &mut dyn Write,
) -> Result<Vec<LossNote>, CodecError> {
    let mut staged = tempfile::tempfile()?;
    let losses = write_seekable(ir, version, &mut staged)?;
    staged.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut staged, output)?;
    Ok(losses)
}

fn write_seekable(
    ir: &CadIr,
    version: RhinoArchiveVersion,
    output: &mut dyn WriteSeek,
) -> Result<Vec<LossNote>, CodecError> {
    let mut plan = prepare_write(ir, version)?;
    write_archive_prefix(
        ir.tolerances.linear.get(),
        plan.angular_tolerance,
        &plan.layer_records,
        version,
        output,
    )?;
    let table_start = output.stream_position()?;
    output.write_all(&TCODE_OBJECT_TABLE.to_le_bytes())?;
    output.write_all(&0_i64.to_le_bytes())?;
    let body_start = output.stream_position()?;

    plan.brep_records.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut plan.brep_records, output)?;
    for (point, display) in &plan.points {
        let position = point.position().get();
        let mut payload = vec![0x10];
        payload.extend(position.x.to_le_bytes());
        payload.extend(position.y.to_le_bytes());
        payload.extend(position.z.to_le_bytes());
        output.write_all(&attributed_object_record(
            1,
            POINT_CLASS,
            &payload,
            point.id.as_str(),
            *display,
        )?)?;
    }
    for group in &plan.point_groups {
        let display = ObjectDisplay {
            name: group.name.as_deref(),
            color: group.color,
            visible: group.visible,
            layer: group.layer,
        };
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
                display,
            )?)?;
        } else {
            output.write_all(&attributed_object_record(
                2,
                POINT_CLOUD_CLASS,
                &point_cloud_payload(&group.points)?,
                &group.identity,
                display,
            )?)?;
        }
    }
    for (id, (class, payload), display) in &plan.curves {
        output.write_all(&attributed_object_record(
            4, *class, payload, id, *display,
        )?)?;
    }
    for (id, (class, payload), display) in &plan.surfaces {
        output.write_all(&attributed_object_record(
            8, *class, payload, id, *display,
        )?)?;
    }
    plan.mesh_records.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut plan.mesh_records, output)?;

    output.write_all(&short_chunk(TCODE_ENDOFTABLE, 0))?;
    let table_end = output.stream_position()?;
    let body_len = archive_body_len(table_end - body_start)?;
    output.seek(SeekFrom::Start(table_start + 4))?;
    output.write_all(&body_len.to_le_bytes())?;
    output.seek(SeekFrom::Start(table_end))?;
    output.write_all(&table(TCODE_HISTORY_RECORD_TABLE, &[])?)?;
    let final_size = archive_final_size(output.stream_position()?)?;
    output.write_all(&long_chunk(TCODE_ENDOFFILE, &final_size.to_le_bytes())?)?;
    Ok(plan.losses)
}

fn write_archive_prefix(
    linear_tolerance: f64,
    angular_tolerance: f64,
    layer_records: &[Vec<u8>],
    version: RhinoArchiveVersion,
    output: &mut dyn WriteSeek,
) -> Result<(), CodecError> {
    output.write_all(&header(version))?;
    output.write_all(&long_chunk(1, b"cadmpeg")?)?;
    output.write_all(&table(
        TCODE_PROPERTIES_TABLE,
        &[short_chunk(
            TCODE_PROPERTIES_OPENNURBS_VERSION,
            OPENNURBS_WRITER_VERSION,
        )],
    )?)?;
    output.write_all(&table(
        TCODE_SETTINGS_TABLE,
        &[units_record(linear_tolerance, angular_tolerance)?],
    )?)?;
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
        output.write_all(&table(typecode, &[])?)?;
        if typecode == TCODE_LINETYPE_TABLE {
            output.write_all(&table(TCODE_LAYER_TABLE, layer_records)?)?;
        }
    }
    Ok(())
}

/// The ids of the topology and carriers one or more Brep bodies own.
#[derive(Default)]
struct OwnedTopology {
    regions: BTreeSet<String>,
    shells: BTreeSet<String>,
    faces: BTreeSet<String>,
    loops: BTreeSet<String>,
    coedges: BTreeSet<String>,
    edges: BTreeSet<String>,
    vertices: BTreeSet<String>,
    points: BTreeSet<String>,
    surfaces: BTreeSet<String>,
    curves: BTreeSet<String>,
    pcurves: BTreeSet<String>,
}

impl OwnedTopology {
    fn kinds(&self) -> [(&'static str, &BTreeSet<String>); 11] {
        [
            ("region", &self.regions),
            ("shell", &self.shells),
            ("face", &self.faces),
            ("loop", &self.loops),
            ("coedge", &self.coedges),
            ("edge", &self.edges),
            ("vertex", &self.vertices),
            ("point", &self.points),
            ("surface", &self.surfaces),
            ("curve", &self.curves),
            ("pcurve", &self.pcurves),
        ]
    }

    /// The first record `other` claims that `self` already owns.
    fn shared_with(&self, other: &Self) -> Option<String> {
        self.kinds()
            .into_iter()
            .zip(other.kinds())
            .find_map(|((kind, owned), (_, claimed))| {
                claimed
                    .iter()
                    .find(|id| owned.contains(*id))
                    .map(|id| format!("its {kind} carrier {id} is shared with an earlier Brep object"))
            })
    }

    fn extend(&mut self, other: Self) {
        self.regions.extend(other.regions);
        self.shells.extend(other.shells);
        self.faces.extend(other.faces);
        self.loops.extend(other.loops);
        self.coedges.extend(other.coedges);
        self.edges.extend(other.edges);
        self.vertices.extend(other.vertices);
        self.points.extend(other.points);
        self.surfaces.extend(other.surfaces);
        self.curves.extend(other.curves);
        self.pcurves.extend(other.pcurves);
    }
}

/// The Brep bodies of a document, each scoped to its own topology.
struct BrepScopes {
    /// One single-body document per writable Brep body.
    scopes: Vec<CadIr>,
    /// What every Sheet and Solid body owns, including the omitted ones.
    owned: OwnedTopology,
    /// Each body omitted because an earlier body owns one of its records, and
    /// why.
    shared: Vec<(String, String)>,
}

struct BrepPayload {
    body: Vec<u8>,
    direct: Vec<u8>,
}

/// A native class identifier and the payload encoded for it.
type ClassPayload = ([u8; 16], Vec<u8>);

struct WritePlan<'a> {
    /// The angular tolerance the archive states.
    angular_tolerance: f64,
    /// The encoded layer table records.
    layer_records: Vec<Vec<u8>>,
    brep_records: std::fs::File,
    /// Each standalone point and its attributes.
    points: Vec<(&'a cadmpeg_ir::topology::Point, ObjectDisplay<'a>)>,
    point_groups: Vec<PointGroup>,
    /// Each free curve's identity, encoded payload and attributes.
    curves: Vec<(&'a str, ClassPayload, ObjectDisplay<'a>)>,
    /// Each free surface's identity, encoded payload and attributes.
    surfaces: Vec<(&'a str, ClassPayload, ObjectDisplay<'a>)>,
    mesh_records: std::fs::File,
    losses: Vec<LossNote>,
}

/// The value of one item, or `None` with the loss that names its omission.
///
/// Errors that leave no output (I/O, a resource ceiling, a target refusal)
/// end the write.
fn admit<T>(
    result: Result<T, CodecError>,
    losses: &mut Vec<LossNote>,
    code: RhinoLossCode,
    item: std::fmt::Arguments<'_>,
) -> Result<Option<T>, CodecError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(
            error @ (CodecError::ResourceLimit(_)
            | CodecError::Io(_)
            | CodecError::UnsupportedTarget(_)
            | CodecError::InsufficientDepth { .. }),
        ) => Err(error),
        Err(error) => {
            losses.push(code.note(format!("{item} was omitted: {error}")));
            Ok(None)
        }
    }
}

fn brep_scopes(ir: &CadIr) -> BrepScopes {
    use cadmpeg_ir::topology::BodyKind;

    let geometry = &ir.geometry;
    let mut scopes = Vec::new();
    let mut owned = OwnedTopology::default();
    let mut shared = Vec::new();

    for body in geometry
        .bodies
        .iter()
        .filter(|body| matches!(body.kind, BodyKind::Sheet | BodyKind::Solid))
    {
        let regions = geometry
            .body_regions(&body.id)
            .map(|region| region.id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let shells = geometry
            .body_regions(&body.id)
            .flat_map(|region| region.shells.iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let faces = geometry
            .faces
            .iter()
            .filter(|face| shells.contains(face.shell.as_str()))
            .map(|face| face.id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let surfaces = geometry
            .faces
            .iter()
            .filter(|face| faces.contains(face.id.as_str()))
            .map(|face| face.surface.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let loops = geometry
            .faces
            .iter()
            .filter(|face| faces.contains(face.id.as_str()))
            .flat_map(|face| face.loops.iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let coedges = geometry
            .loops
            .iter()
            .filter(|loop_| loops.contains(loop_.id.as_str()))
            .flat_map(|loop_| loop_.coedges().iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let edges = geometry
            .coedges
            .iter()
            .filter(|coedge| coedges.contains(coedge.id.as_str()))
            .map(|coedge| coedge.edge.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let pcurves = geometry
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
        let vertices = geometry
            .edges
            .iter()
            .filter(|edge| edges.contains(edge.id.as_str()))
            .flat_map(|edge| edge.vertices().map(|vertex| vertex.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let curves = geometry
            .edges
            .iter()
            .filter(|edge| edges.contains(edge.id.as_str()))
            .filter_map(|edge| edge.curve().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let points = geometry
            .vertices
            .iter()
            .filter(|vertex| vertices.contains(vertex.id.as_str()))
            .map(|vertex| vertex.point.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let body_owned = OwnedTopology {
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
            pcurves,
        };
        if let Some(reason) = owned.shared_with(&body_owned) {
            shared.push((body.id.as_str().to_owned(), reason));
            owned.extend(body_owned);
            continue;
        }

        let mut scoped = CadIr::empty();
        scoped.tolerances = ir.tolerances;
        scoped.geometry.bodies.push(body.clone());
        scoped.geometry.regions = geometry
            .regions
            .iter()
            .filter(|entity| body_owned.regions.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.geometry.shells = geometry
            .shells
            .iter()
            .filter(|entity| body_owned.shells.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.geometry.faces = geometry
            .faces
            .iter()
            .filter(|entity| body_owned.faces.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.geometry.loops = geometry
            .loops
            .iter()
            .filter(|entity| body_owned.loops.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.geometry.coedges = geometry
            .coedges
            .iter()
            .filter(|entity| body_owned.coedges.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.geometry.edges = geometry
            .edges
            .iter()
            .filter(|entity| body_owned.edges.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.geometry.vertices = geometry
            .vertices
            .iter()
            .filter(|entity| body_owned.vertices.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.geometry.points = geometry
            .points
            .iter()
            .filter(|entity| body_owned.points.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.geometry.surfaces = geometry
            .surfaces
            .iter()
            .filter(|entity| body_owned.surfaces.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.geometry.curves = geometry
            .curves
            .iter()
            .filter(|entity| body_owned.curves.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.geometry.pcurves = geometry
            .pcurves
            .iter()
            .filter(|entity| body_owned.pcurves.contains(entity.id.as_str()))
            .cloned()
            .collect();
        owned.extend(body_owned);
        scopes.push(scoped);
    }
    BrepScopes {
        scopes,
        owned,
        shared,
    }
}

fn general_topology_ir(ir: &CadIr) -> CadIr {
    use cadmpeg_ir::topology::BodyKind;
    use std::collections::BTreeSet;

    let mut scoped = CadIr::empty();
    scoped.tolerances = ir.tolerances;
    scoped.geometry.bodies = ir
        .geometry
        .bodies
        .iter()
        .filter(|body| body.kind == BodyKind::General)
        .cloned()
        .collect();
    let bodies = scoped
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    scoped.geometry.regions = ir
        .geometry
        .regions
        .iter()
        .filter(|region| bodies.contains(region.body.as_str()))
        .cloned()
        .collect();
    let regions = scoped
        .geometry
        .regions
        .iter()
        .map(|region| region.id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    scoped.geometry.shells = ir
        .geometry
        .shells
        .iter()
        .filter(|shell| regions.contains(shell.region.as_str()))
        .cloned()
        .collect();
    let vertices = scoped
        .geometry
        .shells
        .iter()
        .flat_map(|shell| shell.free_vertices().map(|id| id.as_str().to_owned()))
        .collect::<BTreeSet<_>>();
    scoped.geometry.vertices = ir
        .geometry
        .vertices
        .iter()
        .filter(|vertex| vertices.contains(vertex.id.as_str()))
        .cloned()
        .collect();
    let points = scoped
        .geometry
        .vertices
        .iter()
        .map(|vertex| vertex.point.as_str())
        .collect::<BTreeSet<_>>();
    scoped.geometry.points = ir
        .geometry
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
    use cadmpeg_ir::topology::BodyKind;

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())?;
    let geometry = &ir.geometry;
    let mut displays = Displays::new(ir)?;
    let mut losses = native::omission_losses(ir.native.namespace("rhino"), displays.layers());
    let mut objects = 0_usize;

    let stated_angular = ir.tolerances.angular.get();
    let angular_tolerance = stated_angular.min(std::f64::consts::PI);
    if stated_angular > std::f64::consts::PI {
        losses.push(RhinoLossCode::WriterToleranceAdjusted.note(format!(
            "document angular tolerance {stated_angular} exceeds pi; the archive states pi"
        )));
    }
    let mut constructions = geometry
        .procedural_surfaces
        .iter()
        .map(|record| record.id.as_str())
        .chain(
            geometry
                .procedural_curves
                .iter()
                .map(|record| record.id.as_str()),
        )
        .collect::<Vec<_>>();
    if !constructions.is_empty() {
        constructions.sort_unstable();
        losses.push(RhinoLossCode::WriterProceduralConstructionReduced.note(format!(
            "the archive states no procedural construction: {} construction record(s) omitted; \
             a carrier the write keeps is written from its solved cache: {}",
            constructions.len(),
            constructions.join(", ")
        )));
    }

    let breps = brep_scopes(ir);
    for (body, reason) in &breps.shared {
        losses.push(
            RhinoLossCode::WriterBodyOmitted.note(format!("body {body} was omitted: {reason}")),
        );
    }
    for body in geometry
        .bodies
        .iter()
        .filter(|body| body.kind == BodyKind::Wire)
    {
        losses.push(RhinoLossCode::WriterBodyOmitted.note(format!(
            "body {} was omitted: {WIRE_BODY_REASON}",
            body.id
        )));
    }
    let mut brep_records = tempfile::tempfile()?;
    let mut adjusted_tolerances = Vec::new();
    for scope in &breps.scopes {
        let body = &scope.geometry.bodies[0];
        let (display, admitted) =
            displays.admit(body.id.as_str(), body.name.as_deref(), body.color, body.visible, None);
        let record = admit(
            WritableModel::try_new(scope).and_then(|model| {
                brep_object_record(
                    &brep_payload(&ctx, &model, archive_version)?,
                    body.id.as_str(),
                    display,
                )
            }),
            &mut losses,
            RhinoLossCode::WriterBodyOmitted,
            format_args!("body {}", body.id),
        )?;
        if let Some(record) = record {
            brep_records.write_all(&record)?;
            objects += 1;
            adjusted_tolerances.extend(sub_resolution_tolerances(scope));
            displays.written(body.id.as_str(), admitted);
        }
    }
    if !adjusted_tolerances.is_empty() {
        adjusted_tolerances.sort_unstable();
        losses.push(RhinoLossCode::WriterToleranceAdjusted.note(format!(
            "{} edge or face tolerance(s) are finer than the Rhino Brep write bound \
             {EPS_WRITE_DEGENERATE}; the write checked them at that bound and states them \
             unchanged: {}",
            adjusted_tolerances.len(),
            adjusted_tolerances.join(", ")
        )));
    }

    let general = general_topology_ir(ir);
    let mut claimed = FreeVertexClaims::default();
    let mut point_groups = Vec::new();
    for body in &general.geometry.bodies {
        let points = admit(
            free_vertex_group(&general, body, &mut claimed),
            &mut losses,
            RhinoLossCode::WriterBodyOmitted,
            format_args!("body {}", body.id),
        )?;
        if let Some(points) = points {
            let (display, admitted) =
                displays.admit(body.id.as_str(), body.name.as_deref(), body.color, body.visible, None);
            point_groups.push(PointGroup {
                points,
                identity: body.id.as_str().to_owned(),
                name: display.name.map(str::to_owned),
                color: display.color,
                visible: display.visible,
                layer: display.layer,
            });
            displays.written(body.id.as_str(), admitted);
        }
    }
    objects += point_groups.len();

    let ids = |records: Vec<&str>| records.into_iter().map(str::to_owned).collect::<BTreeSet<_>>();
    let general_regions = ids(general.geometry.regions.iter().map(|record| record.id.as_str()).collect());
    let general_shells = ids(general.geometry.shells.iter().map(|record| record.id.as_str()).collect());
    let general_vertices = ids(general.geometry.vertices.iter().map(|record| record.id.as_str()).collect());
    let general_points = ids(general.geometry.points.iter().map(|record| record.id.as_str()).collect());
    let mut orphans = Vec::new();
    for (kind, ids) in [
        (
            "region",
            unowned(
                geometry.regions.iter().map(|record| record.id.as_str()),
                &[&breps.owned.regions, &general_regions],
            ),
        ),
        (
            "shell",
            unowned(
                geometry.shells.iter().map(|record| record.id.as_str()),
                &[&breps.owned.shells, &general_shells],
            ),
        ),
        (
            "face",
            unowned(
                geometry.faces.iter().map(|record| record.id.as_str()),
                &[&breps.owned.faces],
            ),
        ),
        (
            "loop",
            unowned(
                geometry.loops.iter().map(|record| record.id.as_str()),
                &[&breps.owned.loops],
            ),
        ),
        (
            "coedge",
            unowned(
                geometry.coedges.iter().map(|record| record.id.as_str()),
                &[&breps.owned.coedges],
            ),
        ),
        (
            "edge",
            unowned(
                geometry.edges.iter().map(|record| record.id.as_str()),
                &[&breps.owned.edges],
            ),
        ),
        (
            "vertex",
            unowned(
                geometry.vertices.iter().map(|record| record.id.as_str()),
                &[&breps.owned.vertices, &general_vertices],
            ),
        ),
        (
            "pcurve",
            unowned(
                geometry.pcurves.iter().map(|record| record.id.as_str()),
                &[&breps.owned.pcurves],
            ),
        ),
    ] {
        if !ids.is_empty() {
            orphans.push(format!("{} {kind}(s): {}", ids.len(), ids.join(", ")));
        }
    }
    if !orphans.is_empty() {
        losses.push(RhinoLossCode::WriterTopologyOmitted.note(format!(
            "topology no Sheet, Solid, or free-vertex body owns was omitted: {}",
            orphans.join("; ")
        )));
    }

    let mut points = Vec::new();
    for point in geometry.points.iter().filter(|point| {
        !breps.owned.points.contains(point.id.as_str())
            && !general_points.contains(point.id.as_str())
            && !is_support(point.source_object.as_ref())
    }) {
        let (display, admitted) =
            displays.admit(point.id.as_str(), None, None, None, point.source_object.as_ref());
        displays.written(point.id.as_str(), admitted);
        points.push((point, display));
    }
    objects += points.len();

    let mut curves = Vec::new();
    for curve in geometry.curves.iter().filter(|curve| {
        !breps.owned.curves.contains(curve.id.as_str()) && !is_support(curve.source_object.as_ref())
    }) {
        let payload = admit(
            WritableObjectCurve::try_new(curve).and_then(|curve| curve.payload()),
            &mut losses,
            RhinoLossCode::WriterCurveOmitted,
            format_args!("curve {}", curve.id),
        )?;
        if let Some(payload) = payload {
            let (display, admitted) =
                displays.admit(curve.id.as_str(), None, None, None, curve.source_object.as_ref());
            displays.written(curve.id.as_str(), admitted);
            curves.push((curve.id.as_str(), payload, display));
        }
    }
    objects += curves.len();

    let mut surfaces = Vec::new();
    for surface in geometry
        .surfaces
        .iter()
        .filter(|surface| !breps.owned.surfaces.contains(surface.id.as_str()))
    {
        let payload = admit(
            WritableFaceSurface::try_new(surface).and_then(WritableFaceSurface::payload),
            &mut losses,
            RhinoLossCode::WriterSurfaceOmitted,
            format_args!("surface {}", surface.id),
        )?;
        if let Some(payload) = payload {
            let (display, admitted) = displays.admit(
                surface.id.as_str(),
                None,
                None,
                None,
                surface.source_object.as_ref(),
            );
            displays.written(surface.id.as_str(), admitted);
            surfaces.push((surface.id.as_str(), payload, display));
        }
    }
    objects += surfaces.len();

    let mut mesh_records = tempfile::tempfile()?;
    for mesh in &ir.model.tessellations {
        let (display, admitted) =
            displays.admit(mesh.id.as_str(), None, None, None, mesh.source_object.as_ref());
        let record = admit(
            check_mesh(mesh)
                .and_then(|()| mesh_payload(mesh, archive_version))
                .and_then(|payload| mesh_object_record(&payload, mesh.id.as_str(), display)),
            &mut losses,
            RhinoLossCode::WriterMeshOmitted,
            format_args!("mesh {}", mesh.id),
        )?;
        if let Some(record) = record {
            mesh_records.write_all(&record)?;
            objects += 1;
            displays.written(mesh.id.as_str(), admitted);
        }
    }

    let layer_records = displays.layers().records()?;
    losses.extend(displays.into_losses());
    if objects == 0 {
        losses.push(cadmpeg_ir::codec::write::output_empty(
            "the written archive holds no object",
        ));
    }
    Ok(WritePlan {
        angular_tolerance,
        layer_records,
        brep_records,
        points,
        point_groups,
        curves,
        surfaces,
        mesh_records,
        losses,
    })
}

/// The ids of `records` that none of `owners` holds, in sorted order.
fn unowned<'a>(
    records: impl Iterator<Item = &'a str>,
    owners: &[&BTreeSet<String>],
) -> Vec<&'a str> {
    let mut ids = records
        .filter(|id| !owners.iter().any(|owned| owned.contains(*id)))
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

/// The edges and faces of a Brep scope whose stated tolerance is finer than
/// the write bound.
fn sub_resolution_tolerances(scope: &CadIr) -> Vec<&str> {
    let document = scope.tolerances.linear.get();
    let finer = |stated: Option<cadmpeg_ir::scalar::PositiveReal>| {
        stated.map_or(document, cadmpeg_ir::scalar::PositiveReal::get) < EPS_WRITE_DEGENERATE
    };
    scope
        .geometry
        .edges
        .iter()
        .filter(|edge| finer(edge.tolerance))
        .map(|edge| edge.id.as_str())
        .chain(
            scope
                .geometry
                .faces
                .iter()
                .filter(|face| finer(face.tolerance))
                .map(|face| face.id.as_str()),
        )
        .collect()
}

/// Whether an object presentation record states only what the written object
/// attributes state, its layer aside.
fn default_native_presentation(fields: &NativeFields) -> bool {
    json_i64(fields, "material_index") == Some(-1)
        && json_i64(fields, "linetype_index") == Some(-1)
        && json_i64(fields, "hatch_pattern_index") == Some(-1)
        && json_i64(fields, "object_mode") == Some(0)
        && json_str(fields, "name") == Some("")
        && json_str(fields, "url") == Some("")
        && json_bool(fields, "visible") == Some(true)
        && json_array_empty(fields, "group_indexes")
        && json_array_empty(fields, "display_materials")
        && json_array_empty(fields, "rendering_materials")
        && json_array_empty_or_missing(fields, "rendering_mappings")
        && fields.get("casts_shadows").is_none()
        && fields.get("receives_shadows").is_none()
        && fields.get("advanced_texture_preview").is_none()
        && fields.get("custom_render_mesh").is_none()
        && fields.get("mesh_modifiers").is_none()
        && json_array_empty(fields, "clipping_plane_uuids")
        && json_array_empty_or_missing(fields, "user_strings")
        && json_array_empty_or_missing(fields, "attribute_user_strings")
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
    ctx: &DecodeContext<'_>,
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
    )?);
    let c3 = model
        .edges
        .iter()
        .map(|edge| brep_c3_curve(model, edge))
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(polymorphic_array(c3.iter())?);
    let surfaces = model
        .surfaces
        .iter()
        .map(|surface| surface.payload())
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(polymorphic_array(surfaces.iter())?);
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
                    .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get)
                    .to_le_bytes(),
            );
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(raw_array(&vertices)?);
    let edges = model
        .edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            let index = wire_index(index)?;
            let mut record = index.to_le_bytes().to_vec();
            record.extend(index.to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            record.extend(
                edge.domain
                    .into_iter()
                    .flat_map(|value| value.get().to_le_bytes()),
            );
            record.extend(wire_index(edge.start)?.to_le_bytes());
            record.extend(wire_index(edge.end)?.to_le_bytes());
            record.extend(indexes(&wire_indexes(edge.uses.iter().copied())?)?);
            record.extend(
                edge.source
                    .tolerance
                    .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get)
                    .to_le_bytes(),
            );
            record.extend(
                edge.domain
                    .into_iter()
                    .flat_map(|value| value.get().to_le_bytes()),
            );
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(raw_array(&edges)?);
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
            record.extend(
                edge.domain
                    .into_iter()
                    .flat_map(|value| value.get().to_le_bytes()),
            );
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
            record.extend(
                edge.domain
                    .into_iter()
                    .flat_map(|value| value.get().to_le_bytes()),
            );
            record.push(0);
            record.extend([0_u8; 31]);
            record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(raw_array(&trims)?);
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
    payload.extend(raw_array(&loops)?);
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
    )?);
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
    let mesh_presence = ctx.alloc_filled(model.faces.len(), 0_u8, "Rhino Brep mesh presence")?;
    payload.extend(crc_chunk(0x4000_8000, &mesh_presence)?);
    payload.extend(crc_chunk(0x4000_8000, &mesh_presence)?);
    let solid = if model.body.kind == BodyKind::Solid {
        planar_solid_orientation(model)
    } else {
        0
    }
    .to_le_bytes();
    payload.extend(solid);
    direct.extend(solid);
    if archive_version.uses_extended_brep_layout() {
        payload.extend(empty_region_wrapper()?);
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
    let Some(reference) = model.vertices.first().map(|vertex| vertex.point) else {
        return 0;
    };
    let scale = model
        .vertices
        .iter()
        .map(|vertex| {
            let delta = vertex.point.vector_from(reference);
            delta.x.abs().max(delta.y.abs()).max(delta.z.abs())
        })
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale == 0.0 {
        return 0;
    }
    let local = |point: cadmpeg_ir::math::Point3| {
        let delta = point.vector_from(reference);
        cadmpeg_ir::math::Vector3::new(delta.x / scale, delta.y / scale, delta.z / scale)
    };
    let mut correction = 0.0;
    let mut volume6 = 0.0;
    for loop_ in &model.loops {
        let ring = loop_
            .coedges
            .iter()
            .map(|coedge| model.vertices[model.endpoints(*coedge).0].point)
            .collect::<Vec<_>>();
        if let Some(origin) = ring.first() {
            for triangle in ring[1..].windows(2) {
                let a = local(triangle[0]);
                let b = local(triangle[1]);
                let term = local(*origin).dot(a.cross(b));
                let adjusted = term - correction;
                let total = volume6 + adjusted;
                correction = (total - volume6) - adjusted;
                volume6 = total;
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

fn brep_c3_curve(
    model: &WritableModel<'_>,
    edge: &WritableEdge<'_>,
) -> Result<([u8; 16], Vec<u8>), CodecError> {
    Ok(match edge.curve {
        WritableEdgeCurve::Line(_) => {
            let from = model.vertices[edge.start].point;
            let to = model.vertices[edge.end].point;
            (
                LINE_CLASS,
                bounded_line_payload(
                    [from.x, from.y, from.z],
                    [to.x, to.y, to.z],
                    FiniteReal::raw_array(edge.domain),
                    3,
                )?,
            )
        }
        WritableEdgeCurve::Nurbs(nurbs) => (NURBS_CURVE_CLASS, nurbs_curve_payload(nurbs)?),
    })
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
                    FiniteReal::raw_array(edge.domain),
                    2,
                )?,
            )
        }
        WritableEdgeCurve::Nurbs(nurbs) => {
            let mut projected = nurbs.clone();
            projected
                .try_map_control_points(|_, point| {
                    let mut point = point.get();
                    let uv = plane_uv(point, origin, u_axis, v_axis);
                    point = cadmpeg_ir::math::Point3::new(uv[0], uv[1], 0.0);
                    cadmpeg_ir::features::FinitePoint3::new(point).ok_or_else(|| {
                        cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                            "control_points contains a non-finite point".into(),
                        )
                    })
                })
                .map_err(|error| CodecError::NotImplemented(error.to_string()))?;
            if sense == Sense::Reversed {
                let sum = projected.knots()[usize::try_from(projected.degree()).map_err(|_| {
                    CodecError::Malformed("Rhino count exceeds address space".into())
                })?] + projected.knots()[projected.pole_count()];
                projected.reverse_parameterization();
                projected
                    .edit_knots(|knots| {
                        for knot in knots {
                            *knot += sum;
                        }
                    })
                    .map_err(|error| CodecError::NotImplemented(error.to_string()))?;
                canonicalize_native_curve_knots(&mut projected, edge.curve_id)?;
            }
            (
                NURBS_CURVE_CLASS,
                nurbs_curve_payload_dimension(&projected, 2)?,
            )
        }
    })
}

fn canonicalize_native_curve_knots(
    curve: &mut cadmpeg_ir::geometry::nurbs::NurbsCurve,
    id: &str,
) -> Result<(), CodecError> {
    let order = usize::try_from(curve.degree())
        .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?
        + 1;
    let count = curve.control_points().len();
    let stored = curve.knots()[1..curve.knots().len() - 1].to_vec();
    let reconstructed = crate::surfaces::reconstruct_knots(&stored, order, count)
        .map_err(|error| CodecError::NotImplemented(format!("curve {id}: {error}")))?;
    curve
        .edit_knots(|knots| knots.copy_from_slice(&reconstructed))
        .map_err(|error| CodecError::NotImplemented(format!("curve {id}: {error}")))?;
    Ok(())
}

fn admit_pcurve<'a>(
    edge: &WritableEdge<'_>,
    pcurve: &'a cadmpeg_ir::geometry::pcurve::Pcurve,
) -> Result<WritablePcurve<'a>, CodecError> {
    if pcurve.wrapper_reversed() == Some(true)
        || pcurve.native_tail_flags().is_some()
        || pcurve
            .parameter_range()
            .is_some_and(|range| range.finite_components() != edge.domain)
    {
        return Err(CodecError::NotImplemented(format!(
            "pcurve {} has unsupported wrapper, tail, domain, or tolerance state",
            pcurve.id.as_str()
        )));
    }
    let domain = FiniteReal::raw_array(edge.domain);
    let (payload, domain_extent_points) = match &pcurve.geometry {
        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line) => {
            let origin = line.origin().as_raw();
            let direction = line.direction().as_raw();
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
                (LINE_CLASS, bounded_line_payload(from, to, domain, 2)?),
                vec![
                    cadmpeg_ir::math::Point2::new(from[0], from[1]),
                    cadmpeg_ir::math::Point2::new(to[0], to[1]),
                ],
            )
        }
        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } => {
            let curve = nurbs
                .lift(|point| cadmpeg_ir::math::Point3::new(point.u, point.v, 0.0))
                .map_err(|error| {
                    CodecError::NotImplemented(format!("pcurve {}: {error}", pcurve.id.as_str()))
                })?;
            check_nurbs_curve(pcurve.id.as_str(), &curve)?;
            let count = curve.pole_count();
            if curve.periodic()
                || [
                    curve.knots()[usize::try_from(curve.degree()).map_err(|_| {
                        CodecError::Malformed("Rhino count exceeds address space".into())
                    })?],
                    curve.knots()[count],
                ] != domain
            {
                return Err(CodecError::NotImplemented(format!(
                    "pcurve {} is not a nonperiodic full-domain NURBS curve",
                    pcurve.id.as_str()
                )));
            }
            (
                (NURBS_CURVE_CLASS, nurbs_curve_payload_dimension(&curve, 2)?),
                nurbs.pole_rows().raw_points(),
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

/// Checks that an explicit pcurve tracks its directed edge curve through the
/// NURBS surface.
///
/// `face_tolerance` is an admitted IR-stated tolerance from
/// `stated_write_tolerance`.
fn validate_nurbs_trim(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    face_tolerance: f64,
    edge: &WritableEdge<'_>,
    sense: cadmpeg_ir::topology::Sense,
    explicit: &WritablePcurve<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::topology::Sense;

    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let u_domain = [
        surface.u_knots()[usize::try_from(surface.u_degree())
            .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?],
        surface.u_knots()[u_count],
    ];
    let v_domain = [
        surface.v_knots()[usize::try_from(surface.v_degree())
            .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?],
        surface.v_knots()[v_count],
    ];
    let pcurve = explicit.source;
    let domain = edge.domain;
    let inside_domain = |u: f64, v: f64| {
        cadmpeg_ir::math::parameter_in_domain(u, u_domain, EPS_WRITE_DEGENERATE)
            && cadmpeg_ir::math::parameter_in_domain(v, v_domain, EPS_WRITE_DEGENERATE)
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
    let mut breaks = vec![domain[0].get(), domain[1].get()];
    if let WritableEdgeCurve::Nurbs(nurbs) = edge.curve {
        breaks.extend(
            nurbs
                .knots()
                .finite_knots()
                .filter(|value| *value > domain[0] && *value < domain[1])
                .map(|value| {
                    if sense == Sense::Forward {
                        Ok(value.get())
                    } else {
                        cadmpeg_ir::math::reflect_parameter(value, domain[0], domain[1])
                            .map(FiniteReal::get)
                            .ok_or_else(|| {
                                CodecError::malformed("reversed edge knot is non-finite")
                            })
                    }
                })
                .collect::<Result<Vec<_>, CodecError>>()?,
        );
    }
    if let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry {
        breaks.extend(
            nurbs
                .knots()
                .iter()
                .copied()
                .filter(|value| *value > domain[0].get() && *value < domain[1].get()),
        );
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();

    let tolerance = face_tolerance
        .max(
            edge.source
                .tolerance
                .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
        )
        .max(
            pcurve
                .fit_tolerance()
                .map_or(0.0, cadmpeg_ir::geometry::FitTolerance::get),
        );
    for span in breaks.windows(2) {
        for step in 0..=16 {
            let fraction = f64::from(step) / 16.0;
            let sample = cadmpeg_ir::math::interpolate(span[0], span[1], fraction)
                .ok_or_else(|| CodecError::malformed("non-finite trim sample parameter"))?;
            let parameter = sample.get();
            // A non-finite pcurve point is refused by the domain test.
            let uv = match cadmpeg_ir::eval::decode::pcurve_uv(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &pcurve.geometry,
                parameter,
            ) {
                Ok(uv) => uv.get(),
                Err(failure) => failure.non_finite()?.ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "pcurve {} cannot be evaluated over its edge domain",
                        pcurve.id.as_str()
                    ))
                })?,
            };
            if !inside_domain(uv.u, uv.v) {
                return Err(CodecError::malformed(format_args!(
                    "pcurve {} leaves its NURBS surface parameter domain",
                    pcurve.id.as_str()
                )));
            }
            // A non-finite surface point is measured as a finite one is.
            let mapped = match cadmpeg_ir::eval::decode::nurbs_surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                surface,
                uv.u,
                uv.v,
            ) {
                Ok(point) => point.get(),
                Err(failure) => failure.non_finite()?.ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "pcurve {} cannot be evaluated through its NURBS surface",
                        pcurve.id.as_str()
                    ))
                })?,
            };
            let curve_parameter = if sense == Sense::Forward {
                sample
            } else {
                cadmpeg_ir::math::reflect_parameter(sample, domain[0], domain[1])
                    .ok_or_else(|| CodecError::malformed("reversed edge parameter is non-finite"))?
            };
            // A non-finite edge curve point is measured as a finite one is.
            let edge_point = match edge.curve.point(curve_parameter) {
                Ok(point) => point.get(),
                Err(failure) => failure.non_finite()?.ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "edge curve {} cannot be evaluated over its edge domain",
                        edge.curve_id
                    ))
                })?,
            };
            let distance = cadmpeg_ir::math::Point3::distance(mapped, edge_point);
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

/// A bounded line from its endpoints, which callers compute, and its domain.
fn bounded_line_payload(
    from: [f64; 3],
    to: [f64; 3],
    domain: [f64; 2],
    dimension: i32,
) -> Result<Vec<u8>, CodecError> {
    let mut payload = vec![0x10];
    for value in from.into_iter().chain(to) {
        payload.extend(
            computed(value, "line endpoint coordinate")?
                .get()
                .to_le_bytes(),
        );
    }
    for value in domain {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(dimension.to_le_bytes());
    Ok(payload)
}

/// Admit a number the writer computed for `field`. A 3DM archive states no
/// non-finite number.
fn computed(value: f64, field: &str) -> Result<FiniteReal, CodecError> {
    FiniteReal::new(value).ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "Rhino writer computed the non-finite {field} {value}, which 3DM cannot state"
        ))
    })
}

/// A count the native field cannot store.
fn native_count_limit(limit: u64, count: u64) -> CodecError {
    CodecError::NotImplemented(format!(
        "Rhino native count {count} exceeds the format limit {limit}"
    ))
}

fn polymorphic_array<'a>(
    children: impl ExactSizeIterator<Item = &'a ([u8; 16], Vec<u8>)>,
) -> Result<Vec<u8>, CodecError> {
    let mut body = vec![0x10];
    body.extend(
        (i32::try_from(children.len()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(children.len()),
            )
        })?)
        .to_le_bytes(),
    );
    let mut direct = body.clone();
    for (class, payload) in children {
        body.extend(1_i32.to_le_bytes());
        direct.extend(1_i32.to_le_bytes());
        body.extend(class_wrapper(*class, payload)?);
    }
    crc_chunk_with_direct(0x4000_8000, &body, &direct)
}

fn raw_array(records: &[Vec<u8>]) -> Result<Vec<u8>, CodecError> {
    let mut body = vec![0x10];
    body.extend(
        (i32::try_from(records.len()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(records.len()),
            )
        })?)
        .to_le_bytes(),
    );
    body.extend(records.concat());
    crc_chunk(0x4000_8000, &body)
}

fn face_array(
    records: &[Vec<u8>],
    faces: &[&cadmpeg_ir::topology::Face],
    archive_version: RhinoArchiveVersion,
) -> Result<Vec<u8>, CodecError> {
    let version_two = archive_version.uses_face_array_v2();
    let minor = if version_two { 2 } else { 1 };
    let mut body = vec![0x10 | minor];
    body.extend(
        (i32::try_from(records.len()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(records.len()),
            )
        })?)
        .to_le_bytes(),
    );
    body.extend(records.concat());
    for face in faces {
        body.extend(&Sha256::digest(face.id.as_str().as_bytes())[..16]);
    }
    if version_two {
        body.push(0);
    }
    crc_chunk(0x4000_8000, &body)
}

fn empty_region_wrapper() -> Result<Vec<u8>, CodecError> {
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(1_i32.to_le_bytes());
    body.push(0);
    crc_chunk(0x4000_8000, &body)
}

/// Converts one arena position into the native index the Brep records store.
fn wire_index(position: usize) -> Result<i32, CodecError> {
    native_i32_count(
        position,
        "Brep record index exceeds the native index range".into(),
    )
}

fn native_i32_count(value: usize, message: String) -> Result<i32, CodecError> {
    i32::try_from(value).map_err(|_| CodecError::NotImplemented(message))
}

fn archive_body_len(value: u64) -> Result<i64, CodecError> {
    i64::try_from(value)
        .map_err(|_| CodecError::NotImplemented("3DM object table size overflow".into()))
}

fn archive_final_size(position: u64) -> Result<u64, CodecError> {
    position
        .checked_add(20)
        .ok_or_else(|| CodecError::NotImplemented("3DM output size overflow".into()))
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

fn class_wrapper(class_uuid: [u8; 16], payload: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(TCODE_CLASS_UUID, &uuid_body)?;
    let data = crc_chunk(TCODE_CLASS_DATA, payload)?;
    let end = short_chunk(TCODE_CLASS_END, 0);
    long_chunk(TCODE_CLASS_WRAPPER, &[uuid, data, end].concat())
}

fn check_mesh(mesh: &cadmpeg_ir::tessellation::Tessellation) -> Result<(), CodecError> {
    let vertex_count = mesh.vertices().len();
    if vertex_count == 0 || vertex_count > (1 << 24) || mesh.triangles().len() > (1 << 24) {
        return Err(CodecError::NotImplemented(format!(
            "mesh {} counts are outside Rhino's native range",
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
    if mesh.vertices().iter().any(|p| {
        cadmpeg_core::convert::f32_from_f64(p.x).is_none()
            || cadmpeg_core::convert::f32_from_f64(p.y).is_none()
            || cadmpeg_core::convert::f32_from_f64(p.z).is_none()
    }) || mesh.vertex_normals().iter().any(|n| {
        cadmpeg_core::convert::f32_from_f64(n.x).is_none()
            || cadmpeg_core::convert::f32_from_f64(n.y).is_none()
            || cadmpeg_core::convert::f32_from_f64(n.z).is_none()
    }) {
        return Err(CodecError::NotImplemented(format!(
            "mesh {} has a vertex or normal coordinate outside the 32-bit float range of the \
             3DM mesh vertex array",
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
            || usize::try_from(channel.count())
                .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?
                != vertex_count
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
    layer: i32,
}

/// Why a body of wire edges is omitted while its edge curves are written.
const WIRE_BODY_REASON: &str =
    "Rhino states wire geometry as free curves; the body's edge curves are written as curve \
     objects and its edge and vertex topology is not";

/// The records earlier free-vertex bodies own.
#[derive(Default)]
struct FreeVertexClaims {
    regions: BTreeSet<String>,
    shells: BTreeSet<String>,
    vertices: BTreeSet<String>,
    points: BTreeSet<String>,
}

/// The points of one free-vertex body of `ir`, written as a point or
/// point-cloud object.
///
/// The body's records join `claimed` only when the body is admitted.
fn free_vertex_group(
    ir: &CadIr,
    body: &cadmpeg_ir::topology::Body,
    claimed: &mut FreeVertexClaims,
) -> Result<Vec<cadmpeg_ir::math::Point3>, CodecError> {
    let geometry = &ir.geometry;
    if geometry.body_regions(&body.id).count() != 1 || body.transform.is_some() {
        return Err(CodecError::NotImplemented(format!(
            "body {} is not a free-vertex body without placement",
            body.id.as_str()
        )));
    }
    let region = geometry
        .regions
        .iter()
        .find(|region| region.body == body.id)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("body {} region is missing", body.id.as_str()))
        })?;
    if region.shells.len() != 1 || claimed.regions.contains(region.id.as_str()) {
        return Err(CodecError::malformed(format_args!(
            "body {} region graph is invalid",
            body.id.as_str()
        )));
    }
    let shell = geometry
        .shells
        .iter()
        .find(|shell| shell.id == region.shells[0])
        .ok_or_else(|| {
            CodecError::malformed(format_args!("body {} shell is missing", body.id.as_str()))
        })?;
    if geometry.shell_faces(&shell.id).next().is_none() && shell.wire_edges().next().is_some() {
        return Err(CodecError::NotImplemented(WIRE_BODY_REASON.into()));
    }
    if shell.region != region.id
        || geometry.shell_faces(&shell.id).next().is_some()
        || shell.wire_edges().next().is_some()
        || shell.free_vertices().next().is_none()
        || claimed.shells.contains(shell.id.as_str())
    {
        return Err(CodecError::malformed(format_args!(
            "body {} shell graph is invalid",
            body.id.as_str()
        )));
    }
    let mut vertices = BTreeSet::new();
    let mut points = BTreeSet::new();
    let mut group = Vec::new();
    for vertex_id in shell.free_vertices() {
        let vertex = geometry
            .vertices
            .iter()
            .find(|vertex| vertex.id == *vertex_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("vertex {} is missing", vertex_id.as_str()))
            })?;
        if vertex.tolerance.is_some()
            || claimed.vertices.contains(vertex.id.as_str())
            || !vertices.insert(vertex.id.as_str().to_owned())
        {
            return Err(CodecError::NotImplemented(format!(
                "vertex {} has tolerance or multiple ownership",
                vertex.id.as_str()
            )));
        }
        let point = geometry
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("point {} is missing", vertex.point.as_str()))
            })?;
        if claimed.points.contains(point.id.as_str()) || !points.insert(point.id.as_str().to_owned())
        {
            return Err(CodecError::NotImplemented(format!(
                "point {} is shared by multiple free vertices",
                point.id.as_str()
            )));
        }
        if is_support(point.source_object.as_ref()) {
            return Err(CodecError::NotImplemented(format!(
                "Rhino cannot write support point {} as a standalone free-vertex object",
                point.id
            )));
        }
        group.push(point.position().get());
    }
    claimed.regions.insert(region.id.as_str().to_owned());
    claimed.shells.insert(shell.id.as_str().to_owned());
    claimed.vertices.extend(vertices);
    claimed.points.extend(points);
    Ok(group)
}

fn check_frame(
    id: &str,
    frame: cadmpeg_ir::units::OrthonormalFrame3,
    family: &str,
) -> Result<(), CodecError> {
    let normal = frame.axis().as_raw();
    let x = frame.reference().as_raw();
    let dot = normal.x * x.x + normal.y * x.y + normal.z * x.z;
    if (normal.norm() - 1.0).abs() > EPS_WRITE_DEGENERATE
        || (x.norm() - 1.0).abs() > EPS_WRITE_DEGENERATE
        || dot.abs() > EPS_WRITE_DEGENERATE
    {
        return Err(CodecError::NotImplemented(format!(
            "{family} {id} frame is not orthonormal to Rhino's tighter bound {EPS_WRITE_DEGENERATE}"
        )));
    }
    Ok(())
}

/// Admits a NURBS surface the Rhino archive can state, returning its pole
/// count at the archive's own lane width.
///
/// The pole lane is an `i32`, so the rule
/// [`cadmpeg_core::decode::id_from_index`] states applies at that width: a
/// count the lane cannot state is a refusal naming the surface. The proven
/// figure is returned rather than discarded, so the writer states the lane
/// from this admission instead of narrowing the population again.
fn check_nurbs_surface(
    id: &str,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
) -> Result<i32, CodecError> {
    let u_order = usize::try_from(surface.u_degree())
        .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?
        + 1;
    let v_order = usize::try_from(surface.v_degree())
        .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?
        + 1;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let count_error = || format!("surface {id} cannot be represented by Rhino NURBS counts");
    let pole_count = native_i32_count(surface.poles().len(), count_error())?;
    if u_order < 2 || v_order < 2 {
        return Err(CodecError::NotImplemented(count_error()));
    }
    native_i32_count(u_order, count_error())?;
    native_i32_count(v_order, count_error())?;
    native_i32_count(u_count, count_error())?;
    native_i32_count(v_count, count_error())?;
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
    Ok(pole_count)
}

fn check_nurbs_curve(
    id: &str,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<(), CodecError> {
    let order = usize::try_from(curve.degree())
        .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?
        + 1;
    let count = curve.control_points().len();
    let count_error = || format!("curve {id} cannot be represented by Rhino NURBS counts");
    if order < 2 {
        return Err(CodecError::NotImplemented(count_error()));
    }
    native_i32_count(order, count_error())?;
    native_i32_count(count, count_error())?;
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
        return Err(CodecError::NotImplemented(format!(
            "{direction} {id} has a non-increasing native NURBS domain"
        )));
    }
    let reconstructed =
        crate::surfaces::reconstruct_knots(stored, order, count).map_err(|error| match error {
            crate::curves::GeometryError::Codec(CodecError::ResourceLimit(limit)) => {
                CodecError::ResourceLimit(limit)
            }
            error => CodecError::NotImplemented(format!("{direction} {id}: {error}")),
        })?;
    let periodic = crate::surfaces::periodic_knots(stored, order, count);
    if reconstructed != full || periodic != declared_periodic {
        return Err(CodecError::NotImplemented(format!(
            "{direction} {id} knot endpoints or periodic flag are not native-canonical"
        )));
    }
    Ok(())
}

fn header(version: RhinoArchiveVersion) -> Vec<u8> {
    let text = version.value().to_string();
    let mut bytes = MAGIC.to_vec();
    bytes.extend(std::iter::repeat_with(|| b' ').take(8 - text.len()));
    bytes.extend(text.bytes());
    bytes
}

fn long_chunk(typecode: u32, body: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    bytes.extend(
        (i64::try_from(body.len()).map_err(|_| {
            native_count_limit(
                9_223_372_036_854_775_807,
                cadmpeg_core::decode::u64_from_index(body.len()),
            )
        })?)
        .to_le_bytes(),
    );
    bytes.extend(body);
    Ok(bytes)
}

fn crc_chunk(typecode: u32, body: &[u8]) -> Result<Vec<u8>, CodecError> {
    crc_chunk_with_direct(typecode, body, body)
}

fn crc_chunk_with_direct(typecode: u32, body: &[u8], direct: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut payload = body.to_vec();
    payload.extend(crc32fast::hash(direct).to_le_bytes());
    long_chunk(typecode, &payload)
}

fn zero_crc_chunk(typecode: u32, body: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut payload = body.to_vec();
    payload.extend(0_u32.to_le_bytes());
    long_chunk(typecode, &payload)
}

fn short_chunk(typecode: u32, value: i64) -> Vec<u8> {
    let mut bytes = (typecode | TCODE_SHORT).to_le_bytes().to_vec();
    bytes.extend(value.to_le_bytes());
    bytes
}

fn table(typecode: u32, records: &[Vec<u8>]) -> Result<Vec<u8>, CodecError> {
    let mut body = records.concat();
    body.extend(short_chunk(TCODE_ENDOFTABLE, 0));
    long_chunk(typecode, &body)
}

fn units_record(linear: f64, angular: f64) -> Result<Vec<u8>, CodecError> {
    let mut body = 100_i32.to_le_bytes().to_vec();
    body.extend(2_i32.to_le_bytes()); // millimeters
    body.extend(linear.to_le_bytes());
    body.extend(angular.to_le_bytes());
    body.extend(DEFAULT_RELATIVE_TOLERANCE.to_le_bytes());
    crc_chunk(TCODE_UNITS_AND_TOLERANCES, &body)
}

fn point_cloud_payload(points: &[cadmpeg_ir::math::Point3]) -> Result<Vec<u8>, CodecError> {
    if points.is_empty() {
        return Err(CodecError::NotImplemented(
            "Rhino point cloud requires at least one point".into(),
        ));
    }
    let mut payload = vec![0x10];
    payload.extend(
        (i32::try_from(points.len()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(points.len()),
            )
        })?)
        .to_le_bytes(),
    );
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
    Ok(payload)
}

fn circle_payload(
    center: cadmpeg_ir::math::Point3,
    axis: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
    radius: f64,
) -> Result<Vec<u8>, CodecError> {
    let y = cadmpeg_ir::math::Vector3::new(
        axis.y * x.z - axis.z * x.y,
        axis.z * x.x - axis.x * x.z,
        axis.x * x.y - axis.y * x.x,
    );
    let equation_d = computed(
        -(axis.x * center.x + axis.y * center.y + axis.z * center.z),
        "circle plane equation constant",
    )?;
    let arc_point = |value: f64| computed(value, "circle arc point coordinate");
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
        equation_d.get(),
        radius,
        arc_point(center.x + radius * x.x)?.get(),
        arc_point(center.y + radius * x.y)?.get(),
        arc_point(center.z + radius * x.z)?.get(),
        arc_point(center.x + radius * y.x)?.get(),
        arc_point(center.y + radius * y.y)?.get(),
        arc_point(center.z + radius * y.z)?.get(),
        arc_point(center.x - radius * x.x)?.get(),
        arc_point(center.y - radius * x.y)?.get(),
        arc_point(center.z - radius * x.z)?.get(),
        0.0,
        std::f64::consts::TAU,
        0.0,
        std::f64::consts::TAU,
    ] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(3_i32.to_le_bytes());
    Ok(payload)
}

fn nurbs_curve_payload(
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<Vec<u8>, CodecError> {
    nurbs_curve_payload_dimension(curve, 3)
}

fn nurbs_curve_payload_dimension(
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    dimension: i32,
) -> Result<Vec<u8>, CodecError> {
    let rational = i32::from(curve.weights().is_some());
    let order = i32::try_from(curve.degree() + 1)
        .map_err(|_| native_count_limit(2_147_483_647, u64::from(curve.degree() + 1)))?;
    let count = i32::try_from(curve.control_points().len()).map_err(|_| {
        native_count_limit(
            2_147_483_647,
            cadmpeg_core::decode::u64_from_index(curve.control_points().len()),
        )
    })?;
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
    payload.extend(
        (i32::try_from(curve.knots().len() - 2).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(curve.knots().len() - 2),
            )
        })?)
        .to_le_bytes(),
    );
    for knot in &curve.knots()[1..curve.knots().len() - 1] {
        payload.extend(knot.to_le_bytes());
    }
    payload.extend(count.to_le_bytes());
    let homogeneous = |value: f64| computed(value, "NURBS curve homogeneous pole coordinate");
    for (index, point) in curve.control_points().iter().enumerate() {
        let weight = curve.weights().map_or(1.0, |weights| weights[index].get());
        payload.extend(homogeneous(point.x * weight)?.get().to_le_bytes());
        payload.extend(homogeneous(point.y * weight)?.get().to_le_bytes());
        if dimension == 3 {
            payload.extend(homogeneous(point.z * weight)?.get().to_le_bytes());
        }
        if rational != 0 {
            payload.extend(weight.to_le_bytes());
        }
    }
    Ok(payload)
}

fn plane_surface_payload(
    origin: cadmpeg_ir::math::Point3,
    normal: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
) -> Result<Vec<u8>, CodecError> {
    let y = normal.cross(x);
    let d = computed(
        -(normal.x * origin.x + normal.y * origin.y + normal.z * origin.z),
        "plane equation constant",
    )?;
    let mut payload = vec![0x10];
    for value in [
        origin.x,
        origin.y,
        origin.z,
        x.x,
        x.y,
        x.z,
        y.x,
        y.y,
        y.z,
        normal.x,
        normal.y,
        normal.z,
        normal.x,
        normal.y,
        normal.z,
        d.get(),
        -1.0,
        1.0,
        -1.0,
        1.0,
    ] {
        payload.extend(value.to_le_bytes());
    }
    Ok(payload)
}

/// Serializes a NURBS surface.
///
/// `pole_count` is the figure `check_nurbs_surface` proved when the surface
/// was admitted; the population is never narrowed again here.
fn nurbs_surface_payload(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    pole_count: i32,
) -> Result<Vec<u8>, CodecError> {
    let rational = i32::from(surface.weights().is_some());
    let mut payload = vec![0x10];
    for value in [
        3,
        rational,
        i32::try_from(surface.u_degree() + 1)
            .map_err(|_| native_count_limit(2_147_483_647, u64::from(surface.u_degree() + 1)))?,
        i32::try_from(surface.v_degree() + 1)
            .map_err(|_| native_count_limit(2_147_483_647, u64::from(surface.v_degree() + 1)))?,
        i32::try_from(surface.u_count()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(surface.u_count()),
            )
        })?,
        i32::try_from(surface.v_count()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(surface.v_count()),
            )
        })?,
        0,
        0,
    ] {
        payload.extend(value.to_le_bytes());
    }
    let poles = surface.poles();
    let min = poles.iter().fold([f64::INFINITY; 3], |a, p| {
        [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
    });
    let max = poles.iter().fold([f64::NEG_INFINITY; 3], |a, p| {
        [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
    });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    for knots in [surface.u_knots(), surface.v_knots()] {
        payload.extend(
            (i32::try_from(knots.len() - 2).map_err(|_| {
                native_count_limit(
                    2_147_483_647,
                    cadmpeg_core::decode::u64_from_index(knots.len() - 2),
                )
            })?)
            .to_le_bytes(),
        );
        for knot in &knots[1..knots.len() - 1] {
            payload.extend(knot.to_le_bytes());
        }
    }
    payload.extend(pole_count.to_le_bytes());
    let pole_weights = surface.pole_weights();
    let homogeneous = |value: f64| computed(value, "NURBS surface homogeneous pole coordinate");
    for (index, point) in poles.iter().enumerate() {
        let weight = pole_weights
            .as_ref()
            .map_or(1.0, |weights| weights[index].get());
        payload.extend(homogeneous(point.x * weight)?.get().to_le_bytes());
        payload.extend(homogeneous(point.y * weight)?.get().to_le_bytes());
        payload.extend(homogeneous(point.z * weight)?.get().to_le_bytes());
        if rational != 0 {
            payload.extend(weight.to_le_bytes());
        }
    }
    Ok(payload)
}

struct MeshPayload {
    body: Vec<u8>,
    direct: Vec<u8>,
}

fn mesh_payload(
    mesh: &cadmpeg_ir::tessellation::Tessellation,
    archive_version: RhinoArchiveVersion,
) -> Result<MeshPayload, CodecError> {
    let writes_double_vertices = archive_version.stores_mesh_vertices_as_f64();
    let payload_version = if writes_double_vertices { 0x38 } else { 0x35 };
    let mut payload = vec![payload_version];
    payload.extend(
        (i32::try_from(mesh.vertices().len()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(mesh.vertices().len()),
            )
        })?)
        .to_le_bytes(),
    );
    payload.extend(
        (i32::try_from(mesh.triangles().len()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(mesh.triangles().len()),
            )
        })?)
        .to_le_bytes(),
    );
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
    payload.extend(
        (i32::try_from(width.bytes()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(width.bytes()),
            )
        })?)
        .to_le_bytes(),
    );
    for triangle in mesh.triangles() {
        for index in [triangle[0], triangle[1], triangle[2], triangle[2]] {
            match width {
                FaceIndexWidth::One => payload.push(u8::try_from(index).map_err(|_| {
                    CodecError::Malformed("mesh face index exceeds chosen width".into())
                })?),
                FaceIndexWidth::Two => payload.extend(
                    (u16::try_from(index).map_err(|_| {
                        CodecError::Malformed("mesh face index exceeds chosen width".into())
                    })?)
                    .to_le_bytes(),
                ),
                FaceIndexWidth::Four => payload.extend(index.to_le_bytes()),
            }
        }
    }

    let mut float_vertices = Vec::new();
    for point in mesh.vertices() {
        for value in [point.x, point.y, point.z] {
            let value = cadmpeg_core::convert::f32_from_f64(value)
                .ok_or_else(|| CodecError::Malformed("mesh vertex exceeds f32 range".into()))?;
            float_vertices.extend(value.to_le_bytes());
        }
    }
    let mut normals = Vec::new();
    for normal in mesh.vertex_normals() {
        for value in [normal.x, normal.y, normal.z] {
            let value = cadmpeg_core::convert::f32_from_f64(value)
                .ok_or_else(|| CodecError::Malformed("mesh normal exceeds f32 range".into()))?;
            normals.extend(value.to_le_bytes());
        }
    }
    for data in [
        &float_vertices[..],
        &normals[..],
        mesh_channel(mesh, CHANNEL_UV),
        mesh_channel(mesh, CHANNEL_CURVATURE),
        mesh_channel(mesh, CHANNEL_COLOR),
    ] {
        payload.extend(mesh_buffer(data)?);
    }
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0_u8; 16]);
    payload.extend(mesh_buffer(mesh_channel(mesh, CHANNEL_SURFACE_PARAMETERS))?);
    let mut direct = payload.clone();
    payload.extend(mesh_mapping_tag()?);
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
        body.extend(
            (u32::try_from(mesh.vertices().len()).map_err(|_| {
                native_count_limit(
                    4_294_967_295,
                    cadmpeg_core::decode::u64_from_index(mesh.vertices().len()),
                )
            })?)
            .to_le_bytes(),
        );
        body.extend(mesh_buffer(&doubles)?);
        payload.extend(crc_chunk(0x4000_8000, &body)?);
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
    Ok(MeshPayload {
        body: payload,
        direct,
    })
}

fn mesh_mapping_tag() -> Result<Vec<u8>, CodecError> {
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

fn mesh_buffer(data: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut result = (u32::try_from(data.len()).map_err(|_| {
        native_count_limit(
            4_294_967_295,
            cadmpeg_core::decode::u64_from_index(data.len()),
        )
    })?)
    .to_le_bytes()
    .to_vec();
    if !data.is_empty() {
        result.extend(crc32fast::hash(data).to_le_bytes());
        result.push(0);
        result.extend(data);
    }
    Ok(result)
}

fn attributed_object_record(
    object_type: i64,
    class_uuid: [u8; 16],
    payload: &[u8],
    identity: &str,
    display: ObjectDisplay<'_>,
) -> Result<Vec<u8>, CodecError> {
    framed_object_record(
        object_type,
        class_uuid,
        payload,
        None,
        Some(object_attributes_payload(identity, display)?),
    )
}

fn mesh_object_record(
    payload: &MeshPayload,
    identity: &str,
    display: ObjectDisplay<'_>,
) -> Result<Vec<u8>, CodecError> {
    framed_object_record(
        0x20,
        MESH_CLASS,
        &payload.body,
        Some(&payload.direct),
        Some(object_attributes_payload(identity, display)?),
    )
}

fn brep_object_record(
    payload: &BrepPayload,
    identity: &str,
    display: ObjectDisplay<'_>,
) -> Result<Vec<u8>, CodecError> {
    framed_object_record(
        0x10,
        BREP_CLASS,
        &payload.body,
        Some(&payload.direct),
        Some(object_attributes_payload(identity, display)?),
    )
}

fn framed_object_record(
    object_type: i64,
    class_uuid: [u8; 16],
    payload: &[u8],
    direct_class_data: Option<&[u8]>,
    attributes: Option<Vec<u8>>,
) -> Result<Vec<u8>, CodecError> {
    let object_type = short_chunk(TCODE_OBJECT_RECORD_TYPE, object_type);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(TCODE_CLASS_UUID, &uuid_body)?;
    let class_data = if let Some(direct) = direct_class_data {
        crc_chunk_with_direct(TCODE_CLASS_DATA, payload, direct)?
    } else {
        crc_chunk(TCODE_CLASS_DATA, payload)?
    };
    let class_end = short_chunk(TCODE_CLASS_END, 0);
    let class = long_chunk(TCODE_CLASS_WRAPPER, &[uuid, class_data, class_end].concat())?;
    let object_end = short_chunk(TCODE_OBJECT_RECORD_END, 0);
    let mut body = [object_type, class].concat();
    if let Some(attributes) = attributes {
        body.extend(crc_chunk(TCODE_OBJECT_RECORD_ATTRIBUTES, &attributes)?);
    }
    body.extend(object_end);
    zero_crc_chunk(TCODE_OBJECT_RECORD, &body)
}

fn object_attributes_payload(
    identity: &str,
    display: ObjectDisplay<'_>,
) -> Result<Vec<u8>, CodecError> {
    let ObjectDisplay {
        name,
        color,
        visible,
        layer,
    } = display;
    let digest = Sha256::digest(identity.as_bytes());
    let mut payload = vec![0x20];
    payload.extend(&digest[..16]);
    payload.extend(layer.to_le_bytes());
    if let Some(name) = name {
        payload.push(1);
        payload.extend(utf16(name)?);
    }
    if let Some(color) = color {
        payload.push(6);
        payload.extend([
            unit_color_channel(color.r())?,
            unit_color_channel(color.g())?,
            unit_color_channel(color.b())?,
            unit_color_channel(1.0 - color.a())?,
        ]);
    }
    if let Some(visible) = visible {
        payload.extend([11, u8::from(visible)]);
    }
    if color.is_some() {
        payload.extend([13, 1]);
    }
    payload.push(0);
    Ok(payload)
}

/// The two vertices that bound an edge. A Rhino Brep edge names both.
fn edge_ends(
    edge: &cadmpeg_ir::topology::Edge,
) -> Result<&cadmpeg_ir::topology::EdgeEnds, CodecError> {
    edge.ends.as_ref().ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "Rhino writer does not encode edge {} with no vertex",
            edge.id
        ))
    })
}

fn unit_color_channel(value: f64) -> Result<u8, CodecError> {
    cadmpeg_core::convert::truncate_f64_to_u8((value * 255.0).round())
        .ok_or_else(|| CodecError::Malformed("color channel is outside byte range".into()))
}

fn utf16(value: &str) -> Result<Vec<u8>, CodecError> {
    let mut units = value.encode_utf16().collect::<Vec<_>>();
    if !units.is_empty() {
        units.push(0);
    }
    let mut bytes = (u32::try_from(units.len()).map_err(|_| {
        native_count_limit(
            4_294_967_295,
            cadmpeg_core::decode::u64_from_index(units.len()),
        )
    })?)
    .to_le_bytes()
    .to_vec();
    for unit in units {
        bytes.extend(unit.to_le_bytes());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
