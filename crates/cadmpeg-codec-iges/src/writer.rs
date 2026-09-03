// SPDX-License-Identifier: Apache-2.0
//! Bounded IGES Fixed ASCII writing.
//!
//! An unchanged decode with a verified document baseline replays its retained
//! source image byte for byte. Otherwise the semantic writer emits the current
//! supported neutral profile and refuses unsupported models or native records.

use crate::entities::curve_conversion::ANGULAR_TOLERANCE;
use crate::loss::IgesLossCode;
use cadmpeg_core::decode::alloc_filled;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{ExportBody, WritePath};
use cadmpeg_ir::eval::{curve_point, model_surface_point, pcurve_uv};
use cadmpeg_ir::geometry::{
    knots_nondecreasing, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, PointId, ShellId, SurfaceId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::{CensusBasis, EntityCensus, LossNote};
use cadmpeg_ir::topology::{BodyKind, Edge, Loop, LoopBoundaryRole, PcurveUse, Region, Sense};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;
use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

const EPS_WRITE_COARSE_GEOMETRY: f64 = 1.0e-6;
const EPS_WRITE_POSITION: f64 = 1.0e-8;
const EPS_WRITE_DEGENERATE: f64 = 1.0e-10;

const ALLOWED_NATIVE_ARENAS: &[&str] = &[
    "boundary_vertex_sewing",
    "cards",
    "copious_data",
    "directions",
    "display_attributes",
    "entities",
    // Type 186 sits in this writer's encodable entity-type list and
    // `brep_entities` emits complete 186 records, void pairs included, so
    // the typed arena never blocks a semantic write.
    "manifold_solids",
    "product_occurrence_expansion",
    "quarantined_directory_records",
    "quarantined_parameter_records",
    "transformations",
];
const FRAME_REPAIR_DOT_LIMIT: f64 = EPS_WRITE_COARSE_GEOMETRY;
const NURBS_CLOSEDNESS_TOLERANCE: f64 = EPS_WRITE_DEGENERATE;
// Roundoff guard for geometric plane classification. This is not serialized
// as an IGES tolerance and never supplies a normal for a non-unique plane.
const NURBS_PLANE_COMPUTATION_TOLERANCE: f64 = 64.0 * f64::EPSILON;
const WRITER_ENDPOINT_RELATIVE_TOLERANCE: f64 = EPS_WRITE_POSITION;
const PHYSICALLY_DEPENDENT_STATUS: &str = "00010000";
const PHYSICALLY_DEPENDENT_EDGE_LIST_STATUS: &str = "00010001";
const PARAMETER_CURVE_STATUS: &str = "00010500";
const BOUNDARY_PREFERENCE_MODEL_CURVES: i32 = 1;
const CURVE_ON_SURFACE_CREATION_UNSPECIFIED: i32 = 0;
const CURVE_ON_SURFACE_PREFERENCE_MODEL_CURVE: i32 = 2;
const WRITER_SENDER_PRODUCT: &str = "cadmpeg";
const WRITER_NATIVE_FILE_NAME: &str = "generated.igs";
const WRITER_NATIVE_SYSTEM_ID: &str = "cadmpeg";
const WRITER_PREPROCESSOR_VERSION: &str = "0.1";
const WRITER_INTEGER_REPRESENTATION_BITS: i64 = 32;
const WRITER_SINGLE_PRECISION_MAGNITUDE: i64 = 38;
const WRITER_SINGLE_PRECISION_SIGNIFICANCE: i64 = 6;
const WRITER_DOUBLE_PRECISION_MAGNITUDE: i64 = 308;
const WRITER_DOUBLE_PRECISION_SIGNIFICANCE: i64 = 17;
const WRITER_MODEL_SPACE_SCALE: &str = "1.0";
const WRITER_UNITS_FLAG: i64 = 2;
const WRITER_UNITS_NAME: &str = "MM";
const WRITER_MAXIMUM_LINE_WEIGHT_GRADATIONS: i64 = 1;
const WRITER_MAXIMUM_LINE_WIDTH: &str = "1.0";
const WRITER_AUTHOR_NAME: &str = "author";
const WRITER_AUTHOR_ORGANIZATION: &str = "cadmpeg";
const WRITER_DRAFTING_STANDARD_FLAG: i64 = 0;
const WRITER_ENTITY_TYPES: &[u32] = &[
    100, 102, 104, 108, 110, 116, 120, 122, 123, 124, 126, 128, 141, 142, 143, 144, 186, 190, 192,
    194, 196, 198, 502, 504, 508, 510, 514,
];

pub(crate) mod target;

fn body(
    bytes: Vec<u8>,
    write_path: WritePath,
    losses: Vec<LossNote>,
    note: &str,
    counts: BTreeMap<String, usize>,
) -> ExportBody {
    ExportBody {
        bytes,
        census: EntityCensus {
            basis: CensusBasis::TargetRecords,
            counts,
        },
        write_path,
        losses,
        notes: vec![note.into()],
    }
}

fn counts_for_ir(ir: &CadIr) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    if let Some(namespace) = ir.native.namespace("iges") {
        if let Some(records) = namespace.arenas.get("entities") {
            for record in records {
                if let Some(entity_type) =
                    record.field("entity_type").and_then(|value| value.as_i64())
                {
                    *counts.entry(format!("{entity_type}_entity")).or_insert(0) += 1;
                }
            }
        }
    }
    if counts.is_empty() {
        counts.insert("116_point".into(), ir.model.points.len());
    }
    counts
}

struct Synthesis {
    bytes: Vec<u8>,
    counts: BTreeMap<String, usize>,
    losses: Vec<LossNote>,
}

fn synthesize(ir: &CadIr, version: crate::IgesVersion) -> Result<Synthesis, CodecError> {
    reject_unsupported_model(ir)?;
    validate_analytic_surface_context(ir)?;
    let mut losses = procedural_reduction_losses(ir)?;
    losses.extend(reject_unsupported_native(ir)?);

    let mut entities = if has_brep_topology(ir) {
        brep_entities(ir, version)?
    } else if has_trimmed_sheet_topology(ir) {
        topology_entities(ir, version)?
    } else {
        let mut entities = Vec::new();
        let mut consumed_points = std::collections::BTreeSet::new();
        let mut consumed_curves = BTreeSet::<String>::new();
        let mut surfaces = ir.model.surfaces.iter().collect::<Vec<_>>();
        surfaces.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        for surface in surfaces {
            append_surface_entities(&mut entities, ir, &surface.geometry, version)?;
        }
        for directrix in ir.model.surfaces.iter().filter_map(|surface| {
            let SurfaceGeometry::Procedural { construction } = &surface.geometry else {
                return None;
            };
            ir.model
                .procedural_surfaces
                .iter()
                .find(|procedural| procedural.id == *construction)
                .and_then(|procedural| match procedural.definition() {
                    ProceduralSurfaceDefinition::Revolution { directrix, .. }
                    | ProceduralSurfaceDefinition::Extrusion { directrix, .. } => Some(directrix),
                    _ => None,
                })
        }) {
            mark_curve_descendants(ir, directrix, &mut consumed_curves, &mut BTreeSet::new())?;
        }
        let mut edges = ir.model.edges.iter().collect::<Vec<_>>();
        edges.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        for edge in edges {
            let curve_id = edge.curve.as_ref().ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "IGES semantic writer does not encode carrier-less edge {}",
                    edge.id
                ))
            })?;
            let curve = ir
                .model
                .curves
                .iter()
                .find(|candidate| candidate.id == *curve_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES edge {} references missing curve {}",
                        edge.id, curve_id
                    ))
                })?;
            let geometry = flatten_curve(&curve.geometry)?;
            let span = edge_span(ir, edge, &geometry)?;
            append_curve_entity(
                &mut entities,
                ir,
                CurveEntityRequest {
                    version,
                    curve_id,
                    geometry: &geometry,
                    span: Some(&span),
                    sense: Sense::Forward,
                    status: "00000000",
                    reference_offset: 0,
                },
            )?;
            mark_curve_descendants(ir, curve_id, &mut consumed_curves, &mut BTreeSet::new())?;
            consumed_points.insert(vertex_point_id(ir, &edge.start)?);
            consumed_points.insert(vertex_point_id(ir, &edge.end)?);
        }

        let mut curves = ir.model.curves.iter().collect::<Vec<_>>();
        curves.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        for curve in curves {
            if consumed_curves.contains(curve.id.as_str()) {
                continue;
            }
            let geometry = flatten_curve(&curve.geometry)?;
            append_curve_entity(
                &mut entities,
                ir,
                CurveEntityRequest {
                    version,
                    curve_id: &curve.id,
                    geometry: &geometry,
                    span: None,
                    sense: Sense::Forward,
                    status: "00000000",
                    reference_offset: 0,
                },
            )?;
            mark_curve_descendants(ir, &curve.id, &mut consumed_curves, &mut BTreeSet::new())?;
        }

        let mut points = ir.model.points.iter().collect::<Vec<_>>();
        points.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        for point in points {
            if consumed_points.contains(&point.id) {
                continue;
            }
            ensure_finite_point(point.position, &point.id.0)?;
            entities.push(point_entity(point.position));
        }
        entities
    };
    ensure_version_support(&entities, version)?;
    resolve_entity_references(&mut entities)?;
    if entities.is_empty() {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer refuses an empty model".into(),
        ));
    }

    let minimum_resolution = minimum_resolution_for_output(ir);
    if let Some(loss) = minimum_resolution_loss(ir, minimum_resolution) {
        losses.push(loss);
    }
    let counts = entity_counts(&entities);
    Ok(Synthesis {
        bytes: encode_file(&entities, version, minimum_resolution)?,
        counts,
        losses,
    })
}

fn validate_analytic_surface_context(ir: &CadIr) -> Result<(), CodecError> {
    let writes_brep = has_brep_topology(ir);
    if let Some(surface) = ir.model.surfaces.iter().find(|surface| {
        matches!(
            surface.geometry,
            SurfaceGeometry::Cylinder { .. }
                | SurfaceGeometry::Cone { .. }
                | SurfaceGeometry::Sphere { .. }
                | SurfaceGeometry::Torus { .. }
        ) && (!writes_brep || !ir.model.faces.iter().any(|face| face.surface == surface.id))
    }) {
        return Err(CodecError::NotImplemented(format!(
            "IGES analytic surface {} requires B-rep topology for Type 192 through 198 output; no bounded Type 128 domain is available",
            surface.id
        )));
    }
    Ok(())
}

impl crate::IgesVersion {
    fn admits(self, entity: &Entity) -> bool {
        if !WRITER_ENTITY_TYPES.contains(&entity.type_code) {
            return false;
        }
        match self {
            crate::IgesVersion::V4_0 => matches!(
                (entity.type_code, entity.form),
                (
                    100 | 102 | 108 | 110 | 116 | 120 | 122 | 124 | 126 | 128 | 142 | 144,
                    0
                ) | (104, 0 | 2 | 3)
            ),
            crate::IgesVersion::V5_0 => matches!(
                (entity.type_code, entity.form),
                (
                    100 | 102
                        | 108
                        | 110
                        | 116
                        | 120
                        | 122
                        | 124
                        | 126
                        | 128
                        | 141
                        | 142
                        | 143
                        | 144,
                    0
                ) | (104, 1..=3)
            ),
            crate::IgesVersion::V5_1 | crate::IgesVersion::V5_2 | crate::IgesVersion::V5_3 => {
                match entity.type_code {
                    100 | 102 | 110 | 116 | 120 | 122 | 123 | 124 | 126 | 128 | 141 | 142 | 143
                    | 144 | 186 => entity.form == 0,
                    104 => matches!(entity.form, 0 | 2 | 3),
                    190 | 192 | 194 | 196 | 198 => entity.form == 1,
                    502 | 504 | 508 | 510 => entity.form == 1,
                    514 => {
                        entity.form == 1 || (entity.form == 2 && self == crate::IgesVersion::V5_3)
                    }
                    _ => false,
                }
            }
        }
    }
}

fn ensure_version_support(
    entities: &[Entity],
    version: crate::IgesVersion,
) -> Result<(), CodecError> {
    if let Some(entity) = entities.iter().find(|entity| !version.admits(entity)) {
        return Err(CodecError::NotImplemented(format!(
            "IGES {} does not define emitted entity Type {} Form {}",
            version.name(),
            entity.type_code,
            entity.form
        )));
    }
    if version == crate::IgesVersion::V4_0 {
        for entity in entities.iter().filter(|entity| entity.type_code == 102) {
            if composite_entity_references(entity)?.len() < 2 {
                return Err(CodecError::NotImplemented(
                    "IGES 4.0 Type 102 requires at least two constituent entities".into(),
                ));
            }
        }
    }
    Ok(())
}

fn solid_shell_roles(region: &Region) -> Result<(&ShellId, &[ShellId]), CodecError> {
    let (exterior, voids) = region.shells.split_first().ok_or_else(|| {
        CodecError::malformed(format_args!(
            "IGES solid region {} has no exterior shell",
            region.id
        ))
    })?;
    let mut distinct = std::collections::BTreeSet::new();
    for shell_id in &region.shells {
        if !distinct.insert(shell_id.as_str()) {
            return Err(CodecError::malformed(format_args!(
                "IGES solid region {} repeats shell {}",
                region.id, shell_id
            )));
        }
    }
    Ok((exterior, voids))
}

fn has_trimmed_sheet_topology(ir: &CadIr) -> bool {
    !ir.model.faces.is_empty()
        || !ir.model.loops.is_empty()
        || !ir.model.coedges.is_empty()
        || !ir.model.pcurves.is_empty()
}

fn has_brep_topology(ir: &CadIr) -> bool {
    if ir
        .model
        .bodies
        .iter()
        .any(|body| !is_decoder_free_geometry_body(body) && body.kind == BodyKind::Solid)
    {
        return true;
    }
    if ir.model.faces.iter().any(|face| {
        face.loops.iter().any(|loop_id| {
            ir.model
                .loops
                .iter()
                .find(|loop_| loop_.id == *loop_id)
                .is_some_and(|loop_| loop_.vertices().next().is_some())
        })
    }) {
        return true;
    }
    let mut edge_use_counts = BTreeMap::new();
    for coedge in &ir.model.coedges {
        *edge_use_counts
            .entry(coedge.edge.as_str())
            .or_insert(0_usize) += 1;
    }
    if edge_use_counts.values().any(|count| *count > 1) {
        return true;
    }
    if ir.model.bodies.iter().any(|body| {
        if is_decoder_free_geometry_body(body) || body.kind != BodyKind::Sheet {
            return false;
        }
        body.regions.iter().any(|region_id| {
            ir.model
                .regions
                .iter()
                .find(|region| region.id == *region_id)
                .is_some_and(|region| region.shells.len() != 1)
        })
    }) {
        return true;
    }
    ir.model.faces.iter().any(|face| {
        ir.model
            .shells
            .iter()
            .find(|shell| shell.faces.iter().any(|face_id| face_id == &face.id))
            .is_some_and(|shell| shell.faces.len() > 1)
    })
}

fn procedural_reduction_losses(ir: &CadIr) -> Result<Vec<LossNote>, CodecError> {
    for procedural in &ir.model.procedural_surfaces {
        if matches!(
            procedural.definition(),
            ProceduralSurfaceDefinition::CurveBounded { .. }
        ) {
            continue;
        }
        let surface = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == procedural.surface)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES procedural surface {} references missing solved surface {}",
                    procedural.id, procedural.surface
                ))
            })?;
        if is_native_surface_construction(
            &surface.geometry,
            &procedural.id,
            procedural.definition(),
        ) {
            continue;
        }
        if !matches!(
            &surface.geometry,
            SurfaceGeometry::Plane { .. }
                | SurfaceGeometry::Nurbs(_)
                | SurfaceGeometry::Cylinder { .. }
                | SurfaceGeometry::Cone { .. }
                | SurfaceGeometry::Sphere { .. }
                | SurfaceGeometry::Torus { .. }
        ) {
            return Err(CodecError::NotImplemented(format!(
                "IGES procedural surface {} has no writable solved carrier",
                procedural.id
            )));
        }
    }
    for procedural in &ir.model.procedural_curves {
        let curve = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == procedural.curve)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES procedural curve {} references missing solved curve {}",
                    procedural.id, procedural.curve
                ))
            })?;
        let geometry = flatten_curve(&curve.geometry)?;
        if !matches!(
            geometry,
            CurveGeometry::Line { .. }
                | CurveGeometry::Circle { .. }
                | CurveGeometry::Ellipse { .. }
                | CurveGeometry::Parabola { .. }
                | CurveGeometry::Hyperbola { .. }
                | CurveGeometry::Nurbs(_)
                | CurveGeometry::Polyline { .. }
        ) {
            return Err(CodecError::NotImplemented(format!(
                "IGES procedural curve {} has no writable solved carrier",
                procedural.id
            )));
        }
    }
    let surface_count = ir
        .model
        .procedural_surfaces
        .iter()
        .filter(|procedural| {
            if matches!(
                procedural.definition(),
                ProceduralSurfaceDefinition::CurveBounded { .. }
            ) {
                return false;
            }
            let Some(surface) = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == procedural.surface)
            else {
                return true;
            };
            !is_native_surface_construction(
                &surface.geometry,
                &procedural.id,
                procedural.definition(),
            )
        })
        .count();
    let curve_count = ir.model.procedural_curves.len();
    if surface_count == 0 && curve_count == 0 {
        return Ok(Vec::new());
    }
    Ok(vec![IgesLossCode::ProceduralReduced.note(format!(
        "{surface_count} procedural surface definition(s) and {curve_count} procedural curve definition(s) were reduced to writable solved carriers"
    ))])
}

fn is_native_surface_construction(
    geometry: &SurfaceGeometry,
    construction: &cadmpeg_ir::ids::ProceduralSurfaceId,
    definition: &ProceduralSurfaceDefinition,
) -> bool {
    if !matches!(
        geometry,
        SurfaceGeometry::Procedural {
            construction: owner
        } if owner == construction
    ) {
        return false;
    }
    matches!(
        definition,
        ProceduralSurfaceDefinition::Revolution {
            angular_parameter_interval: None,
            parameter_interval: Some(_),
            transposed: false,
            revision_form: None,
            ..
        } | ProceduralSurfaceDefinition::Extrusion {
            parameter_interval: Some(_),
            revision_form: None,
            ..
        }
    )
}

fn validate_brep_topology(ir: &CadIr, version: crate::IgesVersion) -> Result<(), CodecError> {
    let bodies = ir
        .model
        .bodies
        .iter()
        .filter(|body| !is_decoder_free_geometry_body(body))
        .collect::<Vec<_>>();
    if bodies.is_empty() {
        return Err(CodecError::NotImplemented(
            "IGES B-rep writer requires at least one supported body".into(),
        ));
    }
    let supported_body_ids = bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut owned_regions = std::collections::BTreeSet::new();
    let mut owned_shells = std::collections::BTreeSet::new();
    let mut owned_faces = std::collections::BTreeSet::new();
    let mut used_loops = std::collections::BTreeSet::new();
    let mut used_coedges = std::collections::BTreeSet::new();
    let mut used_edges = std::collections::BTreeSet::new();
    let mut used_vertices = std::collections::BTreeSet::new();
    let mut edge_bodies = BTreeMap::<String, (String, BodyKind)>::new();
    let mut edge_coedges = BTreeMap::<String, Vec<String>>::new();

    for body in &bodies {
        if !matches!(body.kind, BodyKind::Solid | BodyKind::Sheet) {
            return Err(CodecError::NotImplemented(format!(
                "IGES B-rep writer does not encode body kind {:?} ({})",
                body.kind, body.id
            )));
        }
        if body.regions.len() != 1 {
            return Err(CodecError::NotImplemented(format!(
                "IGES B-rep writer requires one region per body ({})",
                body.id
            )));
        }
        let region_id = &body.regions[0];
        let region = ir
            .model
            .regions
            .iter()
            .find(|region| region.id == *region_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES body {} references missing region {}",
                    body.id, region_id
                ))
            })?;
        if region.body != body.id || region.shells.is_empty() {
            return Err(CodecError::malformed(format_args!(
                "IGES region {} is not a nonempty region of body {}",
                region.id, body.id
            )));
        }
        if body.kind == BodyKind::Solid {
            let _ = solid_shell_roles(region)?;
        }
        if body.kind == BodyKind::Sheet && region.shells.len() != 1 {
            return Err(CodecError::NotImplemented(format!(
                "IGES B-rep writer requires one shell for a sheet body ({})",
                body.id
            )));
        }
        if !owned_regions.insert(region.id.as_str().to_owned()) {
            return Err(CodecError::malformed(format_args!(
                "IGES region {} is owned more than once",
                region.id
            )));
        }
        for shell_id in &region.shells {
            let shell = ir
                .model
                .shells
                .iter()
                .find(|shell| shell.id == *shell_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES region {} references missing shell {}",
                        region.id, shell_id
                    ))
                })?;
            if shell.region != region.id || shell.faces.is_empty() {
                return Err(CodecError::malformed(format_args!(
                    "IGES shell {} is not a nonempty shell of region {}",
                    shell.id, region.id
                )));
            }
            if !shell.wire_edges.is_empty() || !shell.free_vertices.is_empty() {
                return Err(CodecError::NotImplemented(format!(
                    "IGES B-rep writer does not encode wire edges or free vertices in shell {}",
                    shell.id
                )));
            }
            if !owned_shells.insert(shell.id.as_str().to_owned()) {
                return Err(CodecError::malformed(format_args!(
                    "IGES shell {} is owned more than once",
                    shell.id
                )));
            }
            for face_id in &shell.faces {
                let face = ir
                    .model
                    .faces
                    .iter()
                    .find(|face| face.id == *face_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES shell {} references missing face {}",
                            shell.id, face_id
                        ))
                    })?;
                if face.shell != shell.id || face.loops.is_empty() {
                    return Err(CodecError::malformed(format_args!(
                        "IGES face {} is not a nonempty face of shell {}",
                        face.id, shell.id
                    )));
                }
                let face_loops = face_loop_order(ir, face)?;
                let has_unspecified_loop = face_loops
                    .iter()
                    .any(|loop_| loop_.boundary_role == LoopBoundaryRole::Unspecified);
                let has_outer_loop = face_loops
                    .iter()
                    .any(|loop_| loop_.boundary_role == LoopBoundaryRole::Outer);
                let has_inner_loop = face_loops
                    .iter()
                    .any(|loop_| loop_.boundary_role == LoopBoundaryRole::Inner);
                if has_unspecified_loop && (has_outer_loop || has_inner_loop) {
                    return Err(CodecError::NotImplemented(format!(
                        "IGES B-rep writer cannot mix classified and unspecified boundary loops ({})",
                        face.id
                    )));
                }
                if has_inner_loop && !has_outer_loop {
                    return Err(CodecError::NotImplemented(format!(
                        "IGES B-rep writer requires an explicit outer loop for inner boundary loops ({})",
                        face.id
                    )));
                }
                if !owned_faces.insert(face.id.as_str().to_owned()) {
                    return Err(CodecError::malformed(format_args!(
                        "IGES face {} is owned more than once",
                        face.id
                    )));
                }
                let surface = ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == face.surface)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES face {} references missing surface {}",
                            face.id, face.surface
                        ))
                    })?;
                surface_entities_for_ir(ir, &surface.geometry, 0, version)?;
                if matches!(surface.geometry, SurfaceGeometry::Cylinder { .. })
                    && face.loops.iter().any(|loop_id| {
                        let Some(loop_) = ir.model.loops.iter().find(|loop_| loop_.id == *loop_id)
                        else {
                            return false;
                        };
                        loop_.coedges().len() == 2
                            && loop_
                                .coedges()
                                .iter()
                                .filter_map(|coedge_id| {
                                    ir.model
                                        .coedges
                                        .iter()
                                        .find(|coedge| coedge.id == *coedge_id)
                                        .map(|coedge| coedge.edge.as_str())
                                })
                                .collect::<std::collections::BTreeSet<_>>()
                                .len()
                                == 1
                    })
                {
                    return Err(CodecError::NotImplemented(format!(
                        "IGES B-rep writer refuses cylindrical face {} with a boundary loop that repeats one seam edge without axial bounds",
                        face.id
                    )));
                }
                for loop_id in &face.loops {
                    let loop_ = ir
                        .model
                        .loops
                        .iter()
                        .find(|loop_| loop_.id == *loop_id)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "IGES face {} references missing loop {}",
                                face.id, loop_id
                            ))
                        })?;
                    if loop_.face != face.id {
                        return Err(CodecError::malformed(format_args!(
                            "IGES loop {} is not a valid loop of face {}",
                            loop_.id, face.id
                        )));
                    }
                    if !used_loops.insert(loop_.id.as_str().to_owned()) {
                        return Err(CodecError::malformed(format_args!(
                            "IGES loop {} is used more than once",
                            loop_.id
                        )));
                    }
                    for (index, coedge_id) in loop_.coedges().iter().enumerate() {
                        let coedge = ir
                            .model
                            .coedges
                            .iter()
                            .find(|coedge| coedge.id == *coedge_id)
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "IGES loop {} references missing coedge {}",
                                    loop_.id, coedge_id
                                ))
                            })?;
                        let next = &loop_.coedges()[(index + 1) % loop_.coedges().len()];
                        let previous = &loop_.coedges()
                            [(index + loop_.coedges().len() - 1) % loop_.coedges().len()];
                        if coedge.owner_loop != loop_.id
                            || coedge.next != *next
                            || coedge.previous != *previous
                            || coedge.use_curve.is_some()
                        {
                            return Err(CodecError::malformed(format_args!(
                                "IGES coedge {} is not a valid loop use",
                                coedge.id
                            )));
                        }
                        if !used_coedges.insert(coedge.id.as_str().to_owned()) {
                            return Err(CodecError::malformed(format_args!(
                                "IGES coedge {} is used more than once",
                                coedge.id
                            )));
                        }
                        let edge = ir
                            .model
                            .edges
                            .iter()
                            .find(|edge| edge.id == coedge.edge)
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "IGES coedge {} references missing edge {}",
                                    coedge.id, coedge.edge
                                ))
                            })?;
                        if let Some((owner, _)) = edge_bodies.get(edge.id.as_str()) {
                            if owner != body.id.as_str() {
                                return Err(CodecError::malformed(format_args!(
                                    "IGES edge {} is used by multiple bodies",
                                    edge.id
                                )));
                            }
                        } else {
                            edge_bodies.insert(
                                edge.id.as_str().to_owned(),
                                (body.id.as_str().to_owned(), body.kind),
                            );
                        }
                        used_edges.insert(edge.id.as_str().to_owned());
                        edge_coedges
                            .entry(edge.id.as_str().to_owned())
                            .or_default()
                            .push(coedge.id.as_str().to_owned());
                        let curve_id = edge.curve.as_ref().ok_or_else(|| {
                            CodecError::NotImplemented(format!(
                                "IGES B-rep writer does not encode carrier-less edge {}",
                                edge.id
                            ))
                        })?;
                        let curve = ir
                            .model
                            .curves
                            .iter()
                            .find(|curve| curve.id == *curve_id)
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "IGES edge {} references missing curve {}",
                                    edge.id, curve_id
                                ))
                            })?;
                        let geometry = flatten_curve(&curve.geometry)?;
                        let span = edge_span(ir, edge, &geometry)?;
                        for vertex_id in [&edge.start, &edge.end] {
                            let vertex = ir
                                .model
                                .vertices
                                .iter()
                                .find(|vertex| vertex.id == *vertex_id)
                                .ok_or_else(|| {
                                    CodecError::malformed(format_args!(
                                        "IGES edge {} references missing vertex {}",
                                        edge.id, vertex_id
                                    ))
                                })?;
                            used_vertices.insert(vertex.id.as_str().to_owned());
                            point_position(ir, &vertex.point)?;
                        }
                        let orientation = pcurve_orientation_context(
                            ir,
                            &surface.geometry,
                            span.start,
                            span.end,
                            coedge.sense,
                            topology_edge_explicit_tolerance(ir, edge),
                            coedge.id.as_str(),
                        );
                        validate_brep_pcurve_uses(&orientation, &coedge.pcurves)?;
                    }
                    for (vertex_id, after, pcurves) in loop_.vertex_occurrences() {
                        if after.is_some_and(|coedge_id| !loop_.coedges().contains(coedge_id)) {
                            return Err(CodecError::malformed(format_args!(
                                "IGES loop {} vertex use references a coedge outside the loop",
                                loop_.id
                            )));
                        }
                        let vertex = ir
                            .model
                            .vertices
                            .iter()
                            .find(|vertex| vertex.id == *vertex_id)
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "IGES loop {} references missing vertex {}",
                                    loop_.id, vertex_id
                                ))
                            })?;
                        used_vertices.insert(vertex.id.as_str().to_owned());
                        let position = point_position(ir, &vertex.point)?;
                        let orientation = pcurve_orientation_context(
                            ir,
                            &surface.geometry,
                            position,
                            position,
                            Sense::Forward,
                            cadmpeg_ir::units::COINCIDENCE_TOLERANCE,
                            loop_.id.as_str(),
                        );
                        validate_brep_pcurve_uses(&orientation, pcurves)?;
                    }
                }
            }
        }
    }

    let ignored_carriers = ignored_carrier_geometry(ir);
    let mut admitted_edges = used_edges.clone();
    admitted_edges.extend(ignored_carriers.edges.iter().cloned());
    let mut admitted_vertices = used_vertices.clone();
    admitted_vertices.extend(ignored_carriers.vertices.iter().cloned());
    let supported_region_count = ir
        .model
        .regions
        .iter()
        .filter(|region| supported_body_ids.contains(&region.body))
        .count();
    let supported_shell_count = ir
        .model
        .shells
        .iter()
        .filter(|shell| {
            ir.model.regions.iter().any(|region| {
                supported_body_ids.contains(&region.body) && region.shells.contains(&shell.id)
            })
        })
        .count();
    if owned_regions.len() != supported_region_count
        || owned_shells.len() != supported_shell_count
        || owned_faces.len() != ir.model.faces.len()
        || used_loops.len() != ir.model.loops.len()
        || used_coedges.len() != ir.model.coedges.len()
        || admitted_edges.len() != ir.model.edges.len()
        || admitted_vertices.len() != ir.model.vertices.len()
        || used_brep_pcurve_ids(ir).len() != ir.model.pcurves.len()
    {
        return Err(CodecError::NotImplemented(format!(
            "IGES B-rep topology ownership is incomplete: regions {}/{} shells {}/{} faces {}/{} loops {}/{} coedges {}/{} edges {}/{} vertices {}/{} pcurves {}/{}",
            owned_regions.len(),
            supported_region_count,
            owned_shells.len(),
            supported_shell_count,
            owned_faces.len(),
            ir.model.faces.len(),
            used_loops.len(),
            ir.model.loops.len(),
            used_coedges.len(),
            ir.model.coedges.len(),
            admitted_edges.len(),
            ir.model.edges.len(),
            admitted_vertices.len(),
            ir.model.vertices.len(),
            used_brep_pcurve_ids(ir).len(),
            ir.model.pcurves.len()
        )));
    }

    for (edge_id, uses) in &edge_coedges {
        let first = uses.first().ok_or_else(|| {
            CodecError::malformed(format_args!("IGES edge {edge_id} has no coedge uses"))
        })?;
        let mut ring = Vec::new();
        let mut current = first.clone();
        loop {
            if ring.contains(&current) {
                if current == *first {
                    break;
                }
                return Err(CodecError::malformed(format_args!(
                    "IGES edge {edge_id} has an invalid radial ring"
                )));
            }
            if ring.len() >= uses.len() {
                return Err(CodecError::malformed(format_args!(
                    "IGES edge {edge_id} has an invalid radial ring"
                )));
            }
            ring.push(current.clone());
            let coedge = ir
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id.as_str() == current)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES radial ring references missing coedge {current}"
                    ))
                })?;
            if coedge.edge.as_str() != edge_id {
                return Err(CodecError::malformed(format_args!(
                    "IGES radial ring for edge {edge_id} names another edge"
                )));
            }
            current = coedge.radial_next.as_str().to_owned();
        }
        if ring.len() != uses.len() {
            return Err(CodecError::malformed(format_args!(
                "IGES edge {edge_id} radial ring does not cover every use"
            )));
        }
        let senses = ring
            .iter()
            .map(|coedge_id| {
                ir.model
                    .coedges
                    .iter()
                    .find(|coedge| coedge.id.as_str() == coedge_id)
                    .map(|coedge| coedge.sense)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES radial ring references missing coedge {coedge_id}"
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if senses.len() == 2 && senses[0] == senses[1] {
            return Err(CodecError::malformed(format_args!(
                "IGES edge {edge_id} has two coedges with the same sense"
            )));
        }
        if edge_bodies[edge_id].1 == BodyKind::Solid && ring.len() != 2 {
            return Err(CodecError::malformed(format_args!(
                "IGES solid edge {edge_id} is not used by exactly two coedges"
            )));
        }
    }
    Ok(())
}

fn brep_entities(ir: &CadIr, version: crate::IgesVersion) -> Result<Vec<Entity>, CodecError> {
    validate_brep_topology(ir, version)?;
    let ignored_carriers = ignored_carrier_geometry(ir);
    let mut topology_point_ids = std::collections::BTreeSet::new();
    for coedge in &ir.model.coedges {
        let Some(edge) = ir.model.edges.iter().find(|edge| edge.id == coedge.edge) else {
            continue;
        };
        for vertex_id in [&edge.start, &edge.end] {
            if let Some(vertex) = ir
                .model
                .vertices
                .iter()
                .find(|vertex| vertex.id == *vertex_id)
            {
                topology_point_ids.insert(vertex.point.as_str().to_owned());
            }
        }
    }
    for loop_ in &ir.model.loops {
        for vertex_id in loop_.vertices() {
            if let Some(vertex) = ir
                .model
                .vertices
                .iter()
                .find(|vertex| vertex.id == *vertex_id)
            {
                topology_point_ids.insert(vertex.point.as_str().to_owned());
            }
        }
    }
    let bodies = ir
        .model
        .bodies
        .iter()
        .filter(|body| !is_decoder_free_geometry_body(body))
        .collect::<Vec<_>>();
    let mut entities = Vec::new();
    let mut surface_indices = BTreeMap::new();
    let mut surfaces = ir.model.surfaces.iter().collect::<Vec<_>>();
    surfaces.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for surface in surfaces {
        let index = append_surface_entities(&mut entities, ir, &surface.geometry, version)?;
        surface_indices.insert(surface.id.as_str().to_owned(), index);
    }

    let mut consumed_curve_ids = ir
        .model
        .edges
        .iter()
        .filter(|edge| ir.model.coedges.iter().any(|coedge| coedge.edge == edge.id))
        .filter_map(|edge| edge.curve.as_ref().map(|curve| curve.as_str().to_owned()))
        .collect::<BTreeSet<_>>();
    let mut topology_edge_ids = std::collections::BTreeSet::new();
    for coedge in &ir.model.coedges {
        topology_edge_ids.insert(coedge.edge.as_str().to_owned());
    }
    let mut edge_curve_indices = BTreeMap::new();
    let mut edges = topology_edge_ids.iter().collect::<Vec<_>>();
    edges.sort();
    for edge_id in edges {
        let edge = ir
            .model
            .edges
            .iter()
            .find(|candidate| candidate.id.as_str() == edge_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES topology references missing edge {edge_id}"
                ))
            })?;
        let curve_id = edge.curve.as_ref().ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "IGES B-rep writer does not encode carrier-less edge {}",
                edge.id
            ))
        })?;
        let curve = ir
            .model
            .curves
            .iter()
            .find(|candidate| candidate.id == *curve_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES edge {} references missing curve {}",
                    edge.id, curve_id
                ))
            })?;
        let geometry = flatten_curve(&curve.geometry)?;
        let span = edge_span(ir, edge, &geometry)?;
        let index = append_curve_entity(
            &mut entities,
            ir,
            CurveEntityRequest {
                version,
                curve_id,
                geometry: &geometry,
                span: Some(&span),
                sense: Sense::Forward,
                status: "00000000",
                reference_offset: 0,
            },
        )?;
        edge_curve_indices.insert(edge.id.as_str().to_owned(), index);
        mark_curve_descendants(ir, curve_id, &mut consumed_curve_ids, &mut BTreeSet::new())?;
    }

    let mut curves = ir.model.curves.iter().collect::<Vec<_>>();
    curves.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for curve in curves {
        if consumed_curve_ids.contains(curve.id.as_str())
            || ignored_carriers.curves.contains(curve.id.as_str())
        {
            continue;
        }
        let geometry = flatten_curve(&curve.geometry)?;
        append_curve_entity(
            &mut entities,
            ir,
            CurveEntityRequest {
                version,
                curve_id: &curve.id,
                geometry: &geometry,
                span: None,
                sense: Sense::Forward,
                status: "00000000",
                reference_offset: 0,
            },
        )?;
        mark_curve_descendants(ir, &curve.id, &mut consumed_curve_ids, &mut BTreeSet::new())?;
    }

    let used_pcurve_ids = used_brep_pcurve_ids(ir);
    let mut pcurve_indices = BTreeMap::new();
    for pcurve_id in used_pcurve_ids {
        let pcurve = ir
            .model
            .pcurves
            .iter()
            .find(|candidate| candidate.id.as_str() == pcurve_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES topology references missing pcurve {pcurve_id}"
                ))
            })?;
        let index = entities.len();
        entities.push(pcurve_entity(ir, pcurve)?);
        pcurve_indices.insert(pcurve_id, index);
    }

    let mut body_ids = bodies
        .iter()
        .map(|body| body.id.as_str().to_owned())
        .collect::<Vec<_>>();
    body_ids.sort();
    for body_id in body_ids {
        let body = bodies
            .iter()
            .find(|body| body.id.as_str() == body_id)
            .ok_or_else(|| CodecError::malformed(format_args!("IGES body {body_id} is missing")))?;
        let region = ir
            .model
            .regions
            .iter()
            .find(|region| region.body.as_str() == body_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("IGES body {body_id} has no region"))
            })?;
        let shells = region
            .shells
            .iter()
            .map(|shell_id| {
                ir.model
                    .shells
                    .iter()
                    .find(|shell| shell.id == *shell_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES region {} references missing shell {}",
                            region.id, shell_id
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut body_edge_ids = std::collections::BTreeSet::new();
        let mut body_vertex_ids = std::collections::BTreeSet::new();
        let mut body_loop_ids = Vec::new();
        let mut body_face_ids = Vec::new();
        for shell in &shells {
            for face_id in &shell.faces {
                let face = ir
                    .model
                    .faces
                    .iter()
                    .find(|face| face.id == *face_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES shell {} references missing face {}",
                            shell.id, face_id
                        ))
                    })?;
                body_face_ids.push(face.id.clone());
                for loop_id in &face.loops {
                    let loop_ = ir
                        .model
                        .loops
                        .iter()
                        .find(|loop_| loop_.id == *loop_id)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "IGES face {} references missing loop {}",
                                face.id, loop_id
                            ))
                        })?;
                    body_loop_ids.push(loop_.id.clone());
                    for coedge_id in loop_.coedges() {
                        let coedge = ir
                            .model
                            .coedges
                            .iter()
                            .find(|coedge| coedge.id == *coedge_id)
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "IGES loop {} references missing coedge {}",
                                    loop_.id, coedge_id
                                ))
                            })?;
                        body_edge_ids.insert(coedge.edge.as_str().to_owned());
                        let edge = ir
                            .model
                            .edges
                            .iter()
                            .find(|edge| edge.id == coedge.edge)
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "IGES coedge {} references missing edge {}",
                                    coedge.id, coedge.edge
                                ))
                            })?;
                        body_vertex_ids.insert(edge.start.as_str().to_owned());
                        body_vertex_ids.insert(edge.end.as_str().to_owned());
                    }
                    for vertex in loop_.vertices() {
                        body_vertex_ids.insert(vertex.as_str().to_owned());
                    }
                }
            }
        }

        let mut vertex_ids = body_vertex_ids.into_iter().collect::<Vec<_>>();
        vertex_ids.sort();
        let mut vertex_indices = BTreeMap::new();
        for (index, vertex_id) in vertex_ids.iter().enumerate() {
            vertex_indices.insert(vertex_id.clone(), index);
        }
        let mut parameters = format!("502,{}", vertex_ids.len());
        for vertex_id in &vertex_ids {
            let vertex = ir
                .model
                .vertices
                .iter()
                .find(|vertex| vertex.id.as_str() == vertex_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES B-rep references missing vertex {vertex_id}"
                    ))
                })?;
            let point = point_position(ir, &vertex.point)?;
            for value in [point.x, point.y, point.z] {
                parameters.push(',');
                parameters.push_str(&number(value));
            }
        }
        parameters.push(';');
        let vertex_list_index = entities.len();
        entities.push(Entity {
            type_code: 502,
            form: 1,
            label: "VERTICES",
            status: PHYSICALLY_DEPENDENT_STATUS,
            parameters: parameters.into_bytes(),
            transform: None,
        });

        let mut edge_ids = body_edge_ids.into_iter().collect::<Vec<_>>();
        edge_ids.sort();
        let mut edge_indices = BTreeMap::new();
        for (index, edge_id) in edge_ids.iter().enumerate() {
            edge_indices.insert(edge_id.clone(), index);
        }
        let edge_list_index = if edge_ids.is_empty() {
            None
        } else {
            let mut parameters = format!("504,{}", edge_ids.len());
            for edge_id in &edge_ids {
                let edge = ir
                    .model
                    .edges
                    .iter()
                    .find(|edge| edge.id.as_str() == edge_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!("IGES B-rep edge {edge_id} is missing"))
                    })?;
                let curve_index = edge_curve_indices[edge_id];
                let start_index = vertex_indices[edge.start.as_str()];
                let end_index = vertex_indices[edge.end.as_str()];
                let _ = write!(
                    parameters,
                    ",{},{},{},{},{}",
                    reference_marker(curve_index),
                    reference_marker(vertex_list_index),
                    start_index + 1,
                    reference_marker(vertex_list_index),
                    end_index + 1
                );
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 504,
                form: 1,
                label: "EDGES",
                status: PHYSICALLY_DEPENDENT_EDGE_LIST_STATUS,
                parameters: parameters.into_bytes(),
                transform: None,
            });
            Some(index)
        };

        let mut loop_indices = BTreeMap::new();
        for loop_id in &body_loop_ids {
            let loop_ = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == *loop_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("IGES B-rep loop {loop_id} is missing"))
                })?;
            let face = ir
                .model
                .faces
                .iter()
                .find(|face| face.id == loop_.face)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES B-rep loop {} references missing face {}",
                        loop_.id, loop_.face
                    ))
                })?;
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES B-rep face {} references missing surface {}",
                        face.id, face.surface
                    ))
                })?;
            let use_count = loop_
                .coedges()
                .len()
                .checked_add(loop_.vertices().count())
                .ok_or_else(|| CodecError::Malformed("IGES loop use count overflows".into()))?;
            let mut parameters = format!("508,{use_count}");
            for coedge_id in loop_.coedges() {
                let coedge = ir
                    .model
                    .coedges
                    .iter()
                    .find(|coedge| coedge.id == *coedge_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES loop {} references missing coedge {}",
                            loop_.id, coedge_id
                        ))
                    })?;
                let edge_index = edge_indices[coedge.edge.as_str()];
                let edge_list_index = edge_list_index.ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES loop {} has a coedge but no edge list",
                        loop_.id
                    ))
                })?;
                let sense = brep_sense(coedge.sense);
                let edge = ir
                    .model
                    .edges
                    .iter()
                    .find(|edge| edge.id == coedge.edge)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES coedge {} references missing edge {}",
                            coedge.id, coedge.edge
                        ))
                    })?;
                let curve_id = edge.curve.as_ref().ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "IGES B-rep coedge {} cannot orient pcurves for carrier-less edge {}",
                        coedge.id, edge.id
                    ))
                })?;
                let curve = ir
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == *curve_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES edge {} references missing curve {}",
                            edge.id, curve_id
                        ))
                    })?;
                let geometry = flatten_curve(&curve.geometry)?;
                let span = edge_span(ir, edge, &geometry)?;
                let pcurve_entities = pcurve_orientation_context(
                    ir,
                    &surface.geometry,
                    span.start,
                    span.end,
                    coedge.sense,
                    topology_edge_explicit_tolerance(ir, edge),
                    coedge.id.as_str(),
                )
                .oriented_entities(
                    &coedge.pcurves,
                    &pcurve_indices,
                    &mut entities,
                )?;
                let _ = write!(
                    parameters,
                    ",0,{},{},{},{}",
                    reference_marker(edge_list_index),
                    edge_index + 1,
                    sense,
                    coedge.pcurves.len()
                );
                for (pcurve_use, pcurve_index) in pcurve_entities {
                    let _ = write!(
                        parameters,
                        ",{},{}",
                        isoparametric_flag(pcurve_use, loop_.id.as_str())?,
                        reference_marker(pcurve_index)
                    );
                }
                for vertex_use in loop_
                    .anchored_vertex_uses()
                    .iter()
                    .filter(|vertex_use| vertex_use.after == coedge.id)
                {
                    let vertex_index = vertex_indices[vertex_use.vertex.as_str()];
                    let _ = write!(
                        parameters,
                        ",1,{},{},{},{}",
                        reference_marker(vertex_list_index),
                        vertex_index + 1,
                        0,
                        vertex_use.pcurves.len()
                    );
                    for pcurve_use in &vertex_use.pcurves {
                        let _ = write!(
                            parameters,
                            ",{},{}",
                            isoparametric_flag(pcurve_use, loop_.id.as_str())?,
                            reference_marker(pcurve_indices[pcurve_use.pcurve.as_str()])
                        );
                    }
                }
            }
            if let Some((vertex, pcurves)) = loop_.singular_vertex() {
                let vertex_index = vertex_indices[vertex.as_str()];
                let _ = write!(
                    parameters,
                    ",1,{},{},{},{}",
                    reference_marker(vertex_list_index),
                    vertex_index + 1,
                    0,
                    pcurves.len()
                );
                for pcurve_use in pcurves {
                    let _ = write!(
                        parameters,
                        ",{},{}",
                        isoparametric_flag(pcurve_use, loop_.id.as_str())?,
                        reference_marker(pcurve_indices[pcurve_use.pcurve.as_str()])
                    );
                }
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 508,
                form: 1,
                label: "LOOP",
                status: PHYSICALLY_DEPENDENT_STATUS,
                parameters: parameters.into_bytes(),
                transform: None,
            });
            loop_indices.insert(loop_id.as_str().to_owned(), index);
        }

        let mut face_indices = BTreeMap::new();
        for face_id in &body_face_ids {
            let face = ir
                .model
                .faces
                .iter()
                .find(|face| face.id == *face_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("IGES B-rep face {face_id} is missing"))
                })?;
            let loops = face_loop_order(ir, face)?;
            let has_outer = face_outer_loop(&loops).is_some();
            let mut parameters = format!(
                "510,{},{},{}",
                reference_marker(surface_indices[face.surface.as_str()]),
                loops.len(),
                i32::from(has_outer)
            );
            for loop_ in loops {
                let _ = write!(
                    parameters,
                    ",{}",
                    reference_marker(loop_indices[loop_.id.as_str()])
                );
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 510,
                form: 1,
                label: "FACE",
                status: PHYSICALLY_DEPENDENT_STATUS,
                parameters: parameters.into_bytes(),
                transform: None,
            });
            face_indices.insert(face_id.as_str().to_owned(), index);
        }

        let mut shell_indices = BTreeMap::new();
        for shell in &shells {
            let mut parameters = format!("514,{}", shell.faces.len());
            for face_id in &shell.faces {
                let face = ir
                    .model
                    .faces
                    .iter()
                    .find(|face| face.id == *face_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES shell {} references missing face {}",
                            shell.id, face_id
                        ))
                    })?;
                let _ = write!(
                    parameters,
                    ",{},{}",
                    reference_marker(face_indices[face.id.as_str()]),
                    brep_sense(face.sense)
                );
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 514,
                form: if body.kind == BodyKind::Solid { 1 } else { 2 },
                label: "SHELL",
                status: if body.kind == BodyKind::Solid {
                    PHYSICALLY_DEPENDENT_STATUS
                } else {
                    "00000000"
                },
                parameters: parameters.into_bytes(),
                transform: None,
            });
            shell_indices.insert(shell.id.as_str(), index);
        }
        if body.kind == BodyKind::Solid {
            let (exterior_shell, void_shells) = solid_shell_roles(region)?;
            let mut parameters = format!(
                "186,{},1,{}",
                reference_marker(shell_indices[exterior_shell.as_str()]),
                void_shells.len()
            );
            for void_shell in void_shells {
                let _ = write!(
                    parameters,
                    ",{},1",
                    reference_marker(shell_indices[void_shell.as_str()])
                );
            }
            parameters.push(';');
            entities.push(Entity {
                type_code: 186,
                form: 0,
                label: "SOLID",
                status: "00000000",
                parameters: parameters.into_bytes(),
                transform: None,
            });
        }
    }
    let mut points = ir.model.points.iter().collect::<Vec<_>>();
    points.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for point in points {
        if topology_point_ids.contains(point.id.as_str())
            || ignored_carriers.points.contains(point.id.as_str())
        {
            continue;
        }
        ensure_finite_point(point.position, &point.id.0)?;
        entities.push(point_entity(point.position));
    }
    Ok(entities)
}

fn brep_sense(sense: Sense) -> i32 {
    match sense {
        Sense::Forward => 1,
        Sense::Reversed => 0,
    }
}

fn used_brep_pcurve_ids(ir: &CadIr) -> std::collections::BTreeSet<String> {
    let mut ids = ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter())
        .map(|use_| use_.pcurve.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    ids.extend(
        ir.model
            .loops
            .iter()
            .flat_map(Loop::vertex_pcurves)
            .map(|use_| use_.pcurve.as_str().to_owned()),
    );
    ids
}

struct IgnoredCarrierGeometry {
    edges: std::collections::BTreeSet<String>,
    curves: std::collections::BTreeSet<String>,
    vertices: std::collections::BTreeSet<String>,
    points: std::collections::BTreeSet<String>,
}

fn ignored_carrier_geometry(ir: &CadIr) -> IgnoredCarrierGeometry {
    let topology_edge_ids = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    let topology_edges = ir
        .model
        .edges
        .iter()
        .filter(|edge| topology_edge_ids.contains(edge.id.as_str()))
        .collect::<Vec<_>>();
    let mut ignored = IgnoredCarrierGeometry {
        edges: std::collections::BTreeSet::new(),
        curves: std::collections::BTreeSet::new(),
        vertices: std::collections::BTreeSet::new(),
        points: std::collections::BTreeSet::new(),
    };
    for body in &ir.model.bodies {
        if !is_decoder_free_geometry_body(body) {
            continue;
        }
        for region_id in &body.regions {
            let Some(region) = ir
                .model
                .regions
                .iter()
                .find(|region| region.id == *region_id)
            else {
                continue;
            };
            for shell_id in &region.shells {
                let Some(shell) = ir.model.shells.iter().find(|shell| shell.id == *shell_id) else {
                    continue;
                };
                ignored.vertices.extend(
                    shell
                        .free_vertices
                        .iter()
                        .map(|vertex| vertex.as_str().to_owned()),
                );
            }
        }
    }
    for edge in &ir.model.edges {
        if topology_edge_ids.contains(edge.id.as_str()) {
            continue;
        }
        let Some(curve_id) = &edge.curve else {
            continue;
        };
        let Some(curve) = ir.model.curves.iter().find(|curve| curve.id == *curve_id) else {
            continue;
        };
        let matching_topology_tolerance = topology_edges
            .iter()
            .filter(|topology_edge| {
                topology_edge.curve.as_ref() == Some(curve_id)
                    && topology_edge.param_range.zip(edge.param_range).is_some_and(
                        |(topology_range, edge_range)| same_range(topology_range, edge_range),
                    )
            })
            .map(|topology_edge| topology_edge_explicit_tolerance(ir, topology_edge))
            .fold(0.0, f64::max);
        let is_model_carrier = topology_edges.iter().any(|topology_edge| {
            topology_edge.curve.as_ref() == Some(curve_id)
                && topology_edge.param_range.zip(edge.param_range).is_some_and(
                    |(topology_range, edge_range)| same_range(topology_range, edge_range),
                )
                && vertex_position(ir, &topology_edge.start)
                    .zip(vertex_position(ir, &edge.start))
                    .is_some_and(|(topology_start, edge_start)| {
                        same_point_with_tolerance(
                            topology_start,
                            edge_start,
                            topology_edge_explicit_tolerance(ir, topology_edge),
                        )
                    })
                && vertex_position(ir, &topology_edge.end)
                    .zip(vertex_position(ir, &edge.end))
                    .is_some_and(|(topology_end, edge_end)| {
                        same_point_with_tolerance(
                            topology_end,
                            edge_end,
                            topology_edge_explicit_tolerance(ir, topology_edge),
                        )
                    })
        });
        let is_pcurve_carrier = ir.model.pcurves.iter().any(|pcurve| {
            pcurve.parameter_range.is_some_and(|range| {
                curve_matches_pcurve(&curve.geometry, range, pcurve)
                    && edge
                        .param_range
                        .is_some_and(|edge_range| same_range(edge_range, range))
                    && vertex_position(ir, &edge.start)
                        .zip(curve_point(&curve.geometry, range[0]))
                        .is_some_and(|(start, evaluated)| {
                            same_point_with_tolerance(start, evaluated, matching_topology_tolerance)
                        })
                    && vertex_position(ir, &edge.end)
                        .zip(curve_point(&curve.geometry, range[1]))
                        .is_some_and(|(end, evaluated)| {
                            same_point_with_tolerance(end, evaluated, matching_topology_tolerance)
                        })
            })
        });
        if is_model_carrier || is_pcurve_carrier {
            ignored.edges.insert(edge.id.as_str().to_owned());
            ignored.curves.insert(curve_id.as_str().to_owned());
            ignored.vertices.insert(edge.start.as_str().to_owned());
            ignored.vertices.insert(edge.end.as_str().to_owned());
        }
    }
    for vertex in &ir.model.vertices {
        if ignored.vertices.contains(vertex.id.as_str()) {
            ignored.points.insert(vertex.point.as_str().to_owned());
        }
    }
    ignored
}

fn curve_matches_pcurve(curve: &CurveGeometry, range: [f64; 2], pcurve: &Pcurve) -> bool {
    let CurveGeometry::Nurbs(curve) = curve else {
        return false;
    };
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = &pcurve.geometry
    else {
        return false;
    };
    curve.degree == *degree
        && curve.periodic == *periodic
        && curve.knots.len() == knots.len()
        && curve
            .knots
            .iter()
            .zip(knots)
            .all(|(left, right)| same_float(*left, *right))
        && curve.control_points.len() == control_points.len()
        && curve
            .control_points
            .iter()
            .zip(control_points)
            .all(|(left, right)| {
                same_float(left.x, right.u)
                    && same_float(left.y, right.v)
                    && same_float(left.z, 0.0)
            })
        && match (&curve.weights, weights) {
            (None, None) => true,
            (Some(left), Some(right)) if left.len() == right.len() => left
                .iter()
                .zip(right)
                .all(|(left, right)| same_float(*left, *right)),
            _ => false,
        }
        && pcurve
            .parameter_range
            .is_some_and(|candidate| same_range(candidate, range))
}

fn same_float(left: f64, right: f64) -> bool {
    (left - right).abs() <= left.abs().max(right.abs()).max(1.0) * EPS_WRITE_DEGENERATE
}

fn topology_entities(ir: &CadIr, version: crate::IgesVersion) -> Result<Vec<Entity>, CodecError> {
    validate_trimmed_sheet_topology(ir, version)?;
    let ignored_carriers = ignored_carrier_geometry(ir);
    let topology_edge_ids = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    let mut entities = Vec::new();
    let mut surface_indices = BTreeMap::new();
    let mut surfaces = ir.model.surfaces.iter().collect::<Vec<_>>();
    surfaces.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for surface in surfaces {
        let index = append_surface_entities(&mut entities, ir, &surface.geometry, version)?;
        surface_indices.insert(surface.id.as_str().to_owned(), index);
    }

    let mut edge_indices = BTreeMap::new();
    let mut consumed_curves = std::collections::BTreeSet::new();
    let mut consumed_points = std::collections::BTreeSet::new();
    let mut edges = ir.model.edges.iter().collect::<Vec<_>>();
    edges.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for edge in edges {
        if !topology_edge_ids.contains(edge.id.as_str()) {
            continue;
        }
        let curve_id = edge.curve.as_ref().ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "IGES semantic writer does not encode carrier-less edge {}",
                edge.id
            ))
        })?;
        let curve = ir
            .model
            .curves
            .iter()
            .find(|candidate| candidate.id == *curve_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES edge {} references missing curve {}",
                    edge.id, curve_id
                ))
            })?;
        let geometry = flatten_curve(&curve.geometry)?;
        let span = edge_span(ir, edge, &geometry)?;
        let index = append_curve_entity(
            &mut entities,
            ir,
            CurveEntityRequest {
                version,
                curve_id,
                geometry: &geometry,
                span: Some(&span),
                sense: Sense::Forward,
                status: PHYSICALLY_DEPENDENT_STATUS,
                reference_offset: 0,
            },
        )?;
        edge_indices.insert(edge.id.as_str().to_owned(), index);
        mark_curve_descendants(ir, curve_id, &mut consumed_curves, &mut BTreeSet::new())?;
        consumed_points.insert(vertex_point_id(ir, &edge.start)?.as_str().to_owned());
        consumed_points.insert(vertex_point_id(ir, &edge.end)?.as_str().to_owned());
    }

    let mut curves = ir.model.curves.iter().collect::<Vec<_>>();
    curves.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for curve in curves {
        if consumed_curves.contains(curve.id.as_str())
            || ignored_carriers.curves.contains(curve.id.as_str())
        {
            continue;
        }
        let geometry = flatten_curve(&curve.geometry)?;
        append_curve_entity(
            &mut entities,
            ir,
            CurveEntityRequest {
                version,
                curve_id: &curve.id,
                geometry: &geometry,
                span: None,
                sense: Sense::Forward,
                status: "00000000",
                reference_offset: 0,
            },
        )?;
        mark_curve_descendants(ir, &curve.id, &mut consumed_curves, &mut BTreeSet::new())?;
    }

    let mut pcurve_ids = std::collections::BTreeSet::new();
    for face in &ir.model.faces {
        for loop_id in &face.loops {
            let loop_ = ir
                .model
                .loops
                .iter()
                .find(|candidate| candidate.id == *loop_id)
                .expect("validated loop reference");
            for coedge_id in loop_.coedges() {
                let coedge = ir
                    .model
                    .coedges
                    .iter()
                    .find(|candidate| candidate.id == *coedge_id)
                    .expect("validated coedge reference");
                pcurve_ids.extend(
                    coedge
                        .pcurves
                        .iter()
                        .map(|use_| use_.pcurve.as_str().to_owned()),
                );
            }
        }
    }
    let mut pcurve_indices = BTreeMap::new();
    for pcurve_id in pcurve_ids {
        let pcurve = ir
            .model
            .pcurves
            .iter()
            .find(|candidate| candidate.id.as_str() == pcurve_id)
            .expect("validated pcurve reference");
        let index = entities.len();
        entities.push(pcurve_entity(ir, pcurve)?);
        pcurve_indices.insert(pcurve_id, index);
    }

    let mut boundary_indices = BTreeMap::new();
    let mut curve_on_surface_indices = BTreeMap::new();
    let mut faces = ir.model.faces.iter().collect::<Vec<_>>();
    faces.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for face in &faces {
        let surface_index = *surface_indices
            .get(face.surface.as_str())
            .expect("validated face surface reference");
        let surface = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .expect("validated face surface reference");
        let loops = face_loop_order(ir, face)?;
        let bounded = loops
            .iter()
            .all(|loop_| loop_.boundary_role == LoopBoundaryRole::Unspecified);
        for loop_ in loops {
            if bounded {
                let boundary = boundary_entity(
                    ir,
                    loop_,
                    surface_index,
                    &surface.geometry,
                    &edge_indices,
                    &pcurve_indices,
                    &mut entities,
                )?;
                let index = entities.len();
                entities.push(boundary);
                boundary_indices.insert(loop_.id.as_str().to_owned(), index);
            } else {
                let curve_on_surface = curve_on_surface_entity(
                    ir,
                    &mut entities,
                    CurveOnSurfaceEntityRequest {
                        version,
                        loop_,
                        surface_index,
                        surface: &surface.geometry,
                        edge_indices: &edge_indices,
                        pcurve_indices: &pcurve_indices,
                    },
                )?;
                let index = entities.len();
                entities.push(curve_on_surface);
                curve_on_surface_indices.insert(loop_.id.as_str().to_owned(), index);
            }
        }
    }

    for face in &faces {
        let surface_index = *surface_indices
            .get(face.surface.as_str())
            .expect("validated face surface reference");
        let loops = face_loop_order(ir, face)?;
        let bounded = loops
            .iter()
            .all(|loop_| loop_.boundary_role == LoopBoundaryRole::Unspecified);
        let mut parameters = if bounded {
            let representation = loops
                .first()
                .and_then(|loop_| loop_.coedges().first())
                .and_then(|coedge_id| {
                    ir.model
                        .coedges
                        .iter()
                        .find(|coedge| coedge.id == *coedge_id)
                })
                .map_or(0, |coedge| i32::from(!coedge.pcurves.is_empty()));
            format!(
                "143,{representation},{},{}",
                reference_marker(surface_index),
                loops.len()
            )
        } else {
            let outer = face_outer_loop(&loops);
            let inner = if outer.is_some() {
                &loops[1..]
            } else {
                &loops[..]
            };
            let mut parameters = format!(
                "144,{},{},{},{}",
                reference_marker(surface_index),
                i32::from(outer.is_some()),
                inner.len(),
                outer.map_or_else(
                    || "0".into(),
                    |loop_| reference_marker(curve_on_surface_indices[loop_.id.as_str()]),
                )
            );
            for loop_ in inner {
                parameters.push(',');
                parameters.push_str(&reference_marker(
                    curve_on_surface_indices[loop_.id.as_str()],
                ));
            }
            parameters
        };
        if bounded {
            for loop_ in &loops {
                parameters.push(',');
                parameters.push_str(&reference_marker(boundary_indices[loop_.id.as_str()]));
            }
        }
        parameters.push(';');
        entities.push(Entity {
            type_code: if bounded { 143 } else { 144 },
            form: 0,
            label: if bounded { "BOUNDED" } else { "TRIMMED" },
            status: "00000000",
            parameters: parameters.into_bytes(),
            transform: None,
        });
    }

    let mut points = ir.model.points.iter().collect::<Vec<_>>();
    points.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for point in points {
        if consumed_points.contains(point.id.as_str())
            || ignored_carriers.points.contains(point.id.as_str())
        {
            continue;
        }
        ensure_finite_point(point.position, &point.id.0)?;
        entities.push(point_entity(point.position));
    }
    Ok(entities)
}

fn validate_trimmed_sheet_topology(
    ir: &CadIr,
    version: crate::IgesVersion,
) -> Result<(), CodecError> {
    if ir.model.faces.is_empty() {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer requires at least one face for topology output".into(),
        ));
    }
    let supported_bodies = ir
        .model
        .bodies
        .iter()
        .filter(|body| !is_decoder_free_geometry_body(body))
        .collect::<Vec<_>>();
    let supported_body_ids = supported_bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let supported_region_ids = ir
        .model
        .regions
        .iter()
        .filter(|region| supported_body_ids.contains(&region.body))
        .map(|region| region.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let supported_shell_count = ir
        .model
        .shells
        .iter()
        .filter(|shell| supported_region_ids.contains(&shell.region))
        .count();
    if supported_bodies.len() != ir.model.faces.len()
        || supported_region_ids.len() != ir.model.faces.len()
        || supported_shell_count != ir.model.faces.len()
    {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer currently encodes one trimmed sheet face per body".into(),
        ));
    }
    let mut owned_faces = std::collections::BTreeSet::new();
    for body in &ir.model.bodies {
        if is_decoder_free_geometry_body(body) {
            continue;
        }
        if body.kind != BodyKind::Sheet
            || body.regions.len() != 1
            || body
                .transform
                .is_some_and(|transform| transform != cadmpeg_ir::transform::Transform::identity())
        {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer only encodes identity single-face sheet body {}",
                body.id
            )));
        }
        let region_id = &body.regions[0];
        let region = ir
            .model
            .regions
            .iter()
            .find(|candidate| candidate.id == *region_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES body {} references missing region {}",
                    body.id, region_id
                ))
            })?;
        if region.body != body.id || region.shells.len() != 1 {
            return Err(CodecError::malformed(format_args!(
                "IGES region {} is not owned by body {} with one shell",
                region.id, body.id
            )));
        }
        let shell_id = &region.shells[0];
        let shell = ir
            .model
            .shells
            .iter()
            .find(|candidate| candidate.id == *shell_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES region {} references missing shell {}",
                    region.id, shell_id
                ))
            })?;
        if shell.region != region.id
            || shell.faces.len() != 1
            || !shell.wire_edges.is_empty()
            || !shell.free_vertices.is_empty()
        {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer only encodes a single face in shell {}",
                shell.id
            )));
        }
        let face = ir
            .model
            .faces
            .iter()
            .find(|candidate| candidate.id == shell.faces[0])
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES shell {} references missing face {}",
                    shell.id, shell.faces[0]
                ))
            })?;
        if face.shell != shell.id {
            return Err(CodecError::malformed(format_args!(
                "IGES face {} is not owned by shell {}",
                face.id, shell.id
            )));
        }
        if !owned_faces.insert(face.id.as_str().to_owned()) {
            return Err(CodecError::malformed(format_args!(
                "IGES face {} is owned by more than one sheet shell",
                face.id
            )));
        }
    }
    if owned_faces.len() != ir.model.faces.len() {
        return Err(CodecError::Malformed(
            "IGES sheet body hierarchy does not own every face exactly once".into(),
        ));
    }

    let mut used_loops = std::collections::BTreeSet::new();
    let mut used_coedges = std::collections::BTreeSet::new();
    let mut used_edges = std::collections::BTreeSet::new();
    let mut used_vertices = std::collections::BTreeSet::new();
    let mut used_pcurves = std::collections::BTreeSet::new();
    let ignored_carriers = ignored_carrier_geometry(ir);
    for face in &ir.model.faces {
        if face.sense != Sense::Forward {
            return Err(CodecError::NotImplemented(format!(
                "IGES Type 144 output cannot encode reversed face sense {}",
                face.id
            )));
        }
        let surface = ir
            .model
            .surfaces
            .iter()
            .find(|candidate| candidate.id == face.surface)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES face {} references missing surface {}",
                    face.id, face.surface
                ))
            })?;
        surface_entities_for_ir(ir, &surface.geometry, 0, version)?;
        let loops = face_loop_order(ir, face)?;
        if loops.is_empty() {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer requires at least one boundary loop per face ({})",
                face.id
            )));
        }
        let has_unspecified_loop = loops
            .iter()
            .any(|loop_| loop_.boundary_role == LoopBoundaryRole::Unspecified);
        let has_explicit_loop = loops
            .iter()
            .any(|loop_| loop_.boundary_role != LoopBoundaryRole::Unspecified);
        if has_unspecified_loop && has_explicit_loop {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer cannot mix classified and unspecified boundary loops ({})",
                face.id
            )));
        }
        let trimmed = has_explicit_loop;
        let mut bounded_representation = None;
        for loop_ in loops {
            if !used_loops.insert(loop_.id.as_str().to_owned()) {
                return Err(CodecError::malformed(format_args!(
                    "IGES face {} uses loop {} more than once",
                    face.id, loop_.id
                )));
            }
            if loop_.coedges().is_empty() || loop_.vertices().next().is_some() {
                return Err(CodecError::NotImplemented(format!(
                    "IGES semantic writer only encodes edge loops without pole vertices ({})",
                    loop_.id
                )));
            }
            let first_pcurve_count = loop_.coedges().first().and_then(|coedge_id| {
                ir.model
                    .coedges
                    .iter()
                    .find(|coedge| coedge.id == *coedge_id)
                    .map(|coedge| coedge.pcurves.len())
            });
            let Some(first_pcurve_count) = first_pcurve_count else {
                return Err(CodecError::malformed(format_args!(
                    "IGES loop {} references a missing first coedge",
                    loop_.id
                )));
            };
            if !trimmed {
                let loop_has_pcurves = first_pcurve_count != 0;
                if bounded_representation.is_some_and(|expected| expected != loop_has_pcurves) {
                    return Err(CodecError::NotImplemented(format!(
                        "IGES Type 143 requires one representation type for every boundary loop ({})",
                        face.id
                    )));
                }
                bounded_representation = Some(loop_has_pcurves);
            }
            for (index, coedge_id) in loop_.coedges().iter().enumerate() {
                let coedge = ir
                    .model
                    .coedges
                    .iter()
                    .find(|candidate| candidate.id == *coedge_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES loop {} references missing coedge {}",
                            loop_.id, coedge_id
                        ))
                    })?;
                if !used_coedges.insert(coedge.id.as_str().to_owned()) {
                    return Err(CodecError::malformed(format_args!(
                        "IGES coedge {} is used more than once",
                        coedge.id
                    )));
                }
                let next = &loop_.coedges()[(index + 1) % loop_.coedges().len()];
                let previous =
                    &loop_.coedges()[(index + loop_.coedges().len() - 1) % loop_.coedges().len()];
                if coedge.owner_loop != loop_.id
                    || coedge.next != *next
                    || coedge.previous != *previous
                    || coedge.radial_next != coedge.id
                    || coedge.use_curve.is_some()
                {
                    return Err(CodecError::malformed(format_args!(
                        "IGES coedge {} is not a simple laminar loop use",
                        coedge.id
                    )));
                }
                if !trimmed && coedge.pcurves.is_empty() != (first_pcurve_count == 0) {
                    return Err(CodecError::NotImplemented(format!(
                        "IGES Type 141 requires consistent parameter-curve presence per loop ({})",
                        loop_.id
                    )));
                }
                if trimmed && coedge.pcurves.is_empty() {
                    return Err(CodecError::NotImplemented(format!(
                        "IGES Type 144 requires parameter curves for every coedge ({})",
                        loop_.id
                    )));
                }
                let edge = ir
                    .model
                    .edges
                    .iter()
                    .find(|candidate| candidate.id == coedge.edge)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES coedge {} references missing edge {}",
                            coedge.id, coedge.edge
                        ))
                    })?;
                if !used_edges.insert(edge.id.as_str().to_owned()) {
                    return Err(CodecError::NotImplemented(format!(
                        "IGES semantic writer does not yet encode a shared or seam edge {}",
                        edge.id
                    )));
                }
                let curve_id = edge.curve.as_ref().ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "IGES semantic writer does not encode carrier-less edge {}",
                        edge.id
                    ))
                })?;
                if !ir.model.curves.iter().any(|curve| curve.id == *curve_id) {
                    return Err(CodecError::malformed(format_args!(
                        "IGES edge {} references missing curve {}",
                        edge.id, curve_id
                    )));
                }
                let span = edge_span(
                    ir,
                    edge,
                    &flatten_curve(
                        &ir.model
                            .curves
                            .iter()
                            .find(|curve| curve.id == *curve_id)
                            .expect("curve existence checked")
                            .geometry,
                    )?,
                )?;
                let start_vertex = ir
                    .model
                    .vertices
                    .iter()
                    .find(|vertex| vertex.id == edge.start)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES edge {} references missing start vertex {}",
                            edge.id, edge.start
                        ))
                    })?;
                let end_vertex = ir
                    .model
                    .vertices
                    .iter()
                    .find(|vertex| vertex.id == edge.end)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES edge {} references missing end vertex {}",
                            edge.id, edge.end
                        ))
                    })?;
                if !ir
                    .model
                    .points
                    .iter()
                    .any(|point| point.id == start_vertex.point)
                    || !ir
                        .model
                        .points
                        .iter()
                        .any(|point| point.id == end_vertex.point)
                {
                    return Err(CodecError::malformed(format_args!(
                        "IGES edge {} references a vertex with a missing point",
                        edge.id
                    )));
                }
                used_vertices.insert(edge.start.as_str().to_owned());
                used_vertices.insert(edge.end.as_str().to_owned());
                for pcurve_use in &coedge.pcurves {
                    if pcurve_use.isoparametric == Some(true) {
                        return Err(CodecError::NotImplemented(format!(
                            "IGES semantic writer does not encode isoparametric pcurve use {}",
                            pcurve_use.pcurve
                        )));
                    }
                    let pcurve = ir
                        .model
                        .pcurves
                        .iter()
                        .find(|candidate| candidate.id == pcurve_use.pcurve)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "IGES coedge {} references missing pcurve {}",
                                coedge.id, pcurve_use.pcurve
                            ))
                        })?;
                    if pcurve.wrapper_reversed.is_some() || pcurve.native_tail_flags.is_some() {
                        return Err(CodecError::NotImplemented(format!(
                            "IGES semantic writer does not encode pcurve wrapper metadata {}",
                            pcurve.id
                        )));
                    }
                    let Some(parameter_range) = pcurve.parameter_range else {
                        return Err(CodecError::NotImplemented(format!(
                            "IGES semantic writer requires a parameter range for pcurve {}",
                            pcurve.id
                        )));
                    };
                    if pcurve_use
                        .parameter_range
                        .is_some_and(|range| !same_range(range, parameter_range))
                    {
                        return Err(CodecError::NotImplemented(format!(
                            "IGES semantic writer cannot restrict pcurve use {}",
                            pcurve_use.pcurve
                        )));
                    }
                    pcurve_entity(ir, pcurve)?;
                    used_pcurves.insert(pcurve.id.as_str().to_owned());
                }
                let orientation = pcurve_orientation_context(
                    ir,
                    &surface.geometry,
                    span.start,
                    span.end,
                    coedge.sense,
                    topology_edge_explicit_tolerance(ir, edge),
                    coedge.id.as_str(),
                );
                orientation.validate(&coedge.pcurves)?;
            }
        }
    }
    let mut admitted_edges = used_edges.clone();
    admitted_edges.extend(ignored_carriers.edges.iter().cloned());
    let mut admitted_vertices = used_vertices.clone();
    admitted_vertices.extend(ignored_carriers.vertices.iter().cloned());
    if used_loops.len() != ir.model.loops.len()
        || used_coedges.len() != ir.model.coedges.len()
        || admitted_edges.len() != ir.model.edges.len()
        || admitted_vertices.len() != ir.model.vertices.len()
        || used_pcurves.len() != ir.model.pcurves.len()
    {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer requires every topology arena entry to belong to a supported sheet face".into(),
        ));
    }
    Ok(())
}

fn is_decoder_free_geometry_body(body: &cadmpeg_ir::topology::Body) -> bool {
    body.id.as_str() == "iges:model:body#free-geometry"
        && body.kind == BodyKind::Wire
        && body.name.as_deref() == Some("IGES free geometry")
}

fn face_loop_order<'a>(
    ir: &'a CadIr,
    face: &cadmpeg_ir::topology::Face,
) -> Result<Vec<&'a Loop>, CodecError> {
    let mut loops = Vec::with_capacity(face.loops.len());
    for loop_id in &face.loops {
        let loop_ = ir
            .model
            .loops
            .iter()
            .find(|candidate| candidate.id == *loop_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES face {} references missing loop {}",
                    face.id, loop_id
                ))
            })?;
        loops.push(loop_);
    }
    if loops
        .iter()
        .filter(|loop_| loop_.boundary_role == LoopBoundaryRole::Outer)
        .count()
        > 1
    {
        return Err(CodecError::NotImplemented(format!(
            "IGES Type 144 supports one explicit outer loop per face ({})",
            face.id
        )));
    }
    loops.sort_by_key(|loop_| match loop_.boundary_role {
        LoopBoundaryRole::Outer => 0,
        LoopBoundaryRole::Unspecified => 1,
        LoopBoundaryRole::Inner => 2,
    });
    Ok(loops)
}

fn face_outer_loop<'a>(loops: &'a [&Loop]) -> Option<&'a Loop> {
    loops
        .first()
        .copied()
        .filter(|loop_| loop_.boundary_role == LoopBoundaryRole::Outer)
}

fn boundary_entity(
    ir: &CadIr,
    loop_: &Loop,
    surface_index: usize,
    surface: &SurfaceGeometry,
    edge_indices: &BTreeMap<String, usize>,
    pcurve_indices: &BTreeMap<String, usize>,
    entities: &mut Vec<Entity>,
) -> Result<Entity, CodecError> {
    let coedges = loop_
        .coedges()
        .iter()
        .map(|coedge_id| {
            ir.model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *coedge_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES loop {} references missing coedge {}",
                        loop_.id, coedge_id
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let has_pcurves = coedges
        .first()
        .is_some_and(|coedge| !coedge.pcurves.is_empty());
    let representation = i32::from(has_pcurves);
    let mut parameters = format!(
        "141,{representation},{BOUNDARY_PREFERENCE_MODEL_CURVES},{},{}",
        reference_marker(surface_index),
        loop_.coedges().len()
    );
    for coedge in coedges {
        let edge_index = edge_indices
            .get(coedge.edge.as_str())
            .copied()
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES boundary loop {} references missing edge entity {}",
                    loop_.id, coedge.edge
                ))
            })?;
        let sense = match coedge.sense {
            Sense::Forward => 1,
            Sense::Reversed => 2,
        };
        parameters.push(',');
        parameters.push_str(&reference_marker(edge_index));
        let _ = write!(parameters, ",{sense},{}", coedge.pcurves.len());
        let pcurve_entities = if coedge.pcurves.is_empty() {
            Vec::new()
        } else {
            let edge = ir
                .model
                .edges
                .iter()
                .find(|edge| edge.id == coedge.edge)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES coedge {} references missing edge {}",
                        coedge.id, coedge.edge
                    ))
                })?;
            let curve_id = edge.curve.as_ref().ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "IGES boundary loop {} cannot orient pcurves for carrier-less edge {}",
                    loop_.id, edge.id
                ))
            })?;
            let curve = ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *curve_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES edge {} references missing curve {}",
                        edge.id, curve_id
                    ))
                })?;
            let geometry = flatten_curve(&curve.geometry)?;
            let span = edge_span(ir, edge, &geometry)?;
            pcurve_orientation_context(
                ir,
                surface,
                span.start,
                span.end,
                coedge.sense,
                topology_edge_explicit_tolerance(ir, edge),
                coedge.id.as_str(),
            )
            .oriented_entities(&coedge.pcurves, pcurve_indices, entities)?
        };
        for (_, pcurve_index) in pcurve_entities {
            parameters.push(',');
            parameters.push_str(&reference_marker(pcurve_index));
        }
    }
    parameters.push(';');
    Ok(Entity {
        type_code: 141,
        form: 0,
        label: "BOUNDARY",
        status: PHYSICALLY_DEPENDENT_STATUS,
        parameters: parameters.into_bytes(),
        transform: None,
    })
}

#[derive(Clone, Copy)]
struct CurveOnSurfaceEntityRequest<'a> {
    version: crate::IgesVersion,
    loop_: &'a Loop,
    surface_index: usize,
    surface: &'a SurfaceGeometry,
    edge_indices: &'a BTreeMap<String, usize>,
    pcurve_indices: &'a BTreeMap<String, usize>,
}

fn curve_on_surface_entity(
    ir: &CadIr,
    entities: &mut Vec<Entity>,
    request: CurveOnSurfaceEntityRequest<'_>,
) -> Result<Entity, CodecError> {
    let CurveOnSurfaceEntityRequest {
        version,
        loop_,
        surface_index,
        surface,
        edge_indices,
        pcurve_indices,
    } = request;
    let mut model_children = Vec::with_capacity(loop_.coedges().len());
    let mut pcurve_children = Vec::new();
    for coedge_id in loop_.coedges() {
        let coedge = ir
            .model
            .coedges
            .iter()
            .find(|coedge| coedge.id == *coedge_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES loop {} references missing coedge {}",
                    loop_.id, coedge_id
                ))
            })?;
        let edge = ir
            .model
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES coedge {} references missing edge {}",
                    coedge.id, coedge.edge
                ))
            })?;
        let curve_id = edge.curve.as_ref().ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "IGES Type 142 output does not encode carrier-less edge {}",
                edge.id
            ))
        })?;
        let curve = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *curve_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES edge {} references missing curve {}",
                    edge.id, curve_id
                ))
            })?;
        let geometry = flatten_curve(&curve.geometry)?;
        let span = edge_span(ir, edge, &geometry)?;
        let model_index = if coedge.sense == Sense::Forward {
            *edge_indices.get(edge.id.as_str()).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES Type 142 loop {} references missing edge entity {}",
                    loop_.id, edge.id
                ))
            })?
        } else {
            append_curve_entity(
                entities,
                ir,
                CurveEntityRequest {
                    version,
                    curve_id,
                    geometry: &geometry,
                    span: Some(&span),
                    sense: coedge.sense,
                    status: PHYSICALLY_DEPENDENT_STATUS,
                    reference_offset: 0,
                },
            )?
        };
        model_children.push(model_index);
        pcurve_children.extend(
            pcurve_orientation_context(
                ir,
                surface,
                span.start,
                span.end,
                coedge.sense,
                topology_edge_explicit_tolerance(ir, edge),
                coedge.id.as_str(),
            )
            .oriented_entities(&coedge.pcurves, pcurve_indices, entities)?
            .into_iter()
            .map(|(_, index)| index),
        );
    }
    if model_children.is_empty() || pcurve_children.is_empty() {
        return Err(CodecError::NotImplemented(format!(
            "IGES Type 144 loop {} requires model and parameter curve carriers",
            loop_.id
        )));
    }
    let model_curve = if model_children.len() == 1 {
        model_children[0]
    } else {
        push_composite_entity(
            entities,
            &model_children,
            "MODEL",
            PHYSICALLY_DEPENDENT_STATUS,
        )?
    };
    let parameter_curve = if pcurve_children.len() == 1 {
        pcurve_children[0]
    } else {
        push_composite_entity(entities, &pcurve_children, "PCURVE", PARAMETER_CURVE_STATUS)?
    };
    Ok(Entity {
        type_code: 142,
        form: 0,
        label: "CURVSURF",
        status: PHYSICALLY_DEPENDENT_STATUS,
        parameters: format!(
            "142,{CURVE_ON_SURFACE_CREATION_UNSPECIFIED},{},{},{},{CURVE_ON_SURFACE_PREFERENCE_MODEL_CURVE};",
            reference_marker(surface_index),
            reference_marker(parameter_curve),
            reference_marker(model_curve)
        )
        .into_bytes(),
        transform: None,
    })
}

fn push_composite_entity(
    entities: &mut Vec<Entity>,
    children: &[usize],
    label: &'static str,
    status: &'static str,
) -> Result<usize, CodecError> {
    push_composite_entity_with_reference_offset(entities, children, label, status, 0)
}

fn push_composite_entity_with_reference_offset(
    entities: &mut Vec<Entity>,
    children: &[usize],
    label: &'static str,
    status: &'static str,
    reference_offset: usize,
) -> Result<usize, CodecError> {
    let children = flatten_composite_children(entities, children)?;
    if children.is_empty() {
        return Err(CodecError::Malformed(
            "IGES composite curve has no children".into(),
        ));
    }
    let mut parameters = format!("102,{}", children.len());
    for child in children {
        let child = child
            .checked_add(reference_offset)
            .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
        parameters.push(',');
        parameters.push_str(&reference_marker(child));
    }
    parameters.push(';');
    let index = entities.len();
    entities.push(Entity {
        type_code: 102,
        form: 0,
        label,
        status,
        parameters: parameters.into_bytes(),
        transform: None,
    });
    Ok(index)
}

fn flatten_composite_children(
    entities: &[Entity],
    children: &[usize],
) -> Result<Vec<usize>, CodecError> {
    let mut flattened = Vec::new();
    let mut active = BTreeSet::new();
    for child in children {
        flatten_composite_child(entities, *child, &mut active, &mut flattened)?;
    }
    Ok(flattened)
}

fn flatten_composite_child(
    entities: &[Entity],
    index: usize,
    active: &mut BTreeSet<usize>,
    flattened: &mut Vec<usize>,
) -> Result<(), CodecError> {
    let entity = entities.get(index).ok_or_else(|| {
        CodecError::Malformed("IGES composite curve child index is out of range".into())
    })?;
    if entity.type_code != 102 {
        flattened.push(index);
        return Ok(());
    }
    if !active.insert(index) {
        return Err(CodecError::Malformed(
            "IGES emitted composite curve graph contains a cycle".into(),
        ));
    }
    let references = composite_entity_references(entity)?;
    for child in references {
        flatten_composite_child(entities, child, active, flattened)?;
    }
    active.remove(&index);
    Ok(())
}

fn composite_entity_references(entity: &Entity) -> Result<Vec<usize>, CodecError> {
    if entity.type_code != 102 {
        return Err(CodecError::Malformed(
            "IGES composite reference extraction received a non-composite entity".into(),
        ));
    }
    let text = std::str::from_utf8(&entity.parameters).map_err(|_| {
        CodecError::Malformed("IGES emitted composite curve parameters are not UTF-8".into())
    })?;
    let mut fields = text.trim_end_matches(';').split(',');
    if fields.next() != Some("102") {
        return Err(CodecError::Malformed(
            "IGES emitted composite curve has an invalid type field".into(),
        ));
    }
    let count = fields
        .next()
        .ok_or_else(|| {
            CodecError::Malformed("IGES emitted composite curve has no constituent count".into())
        })?
        .parse::<usize>()
        .map_err(|_| {
            CodecError::Malformed(
                "IGES emitted composite curve has an invalid constituent count".into(),
            )
        })?;
    let references = fields
        .map(|field| {
            field
                .strip_prefix("@R")
                .and_then(|field| field.strip_suffix('@'))
                .ok_or_else(|| {
                    CodecError::Malformed(
                        "IGES emitted composite curve has a non-pointer constituent".into(),
                    )
                })?
                .parse::<usize>()
                .map_err(|_| {
                    CodecError::Malformed(
                        "IGES emitted composite curve has an invalid constituent pointer".into(),
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if references.len() != count {
        return Err(CodecError::Malformed(
            "IGES emitted composite curve constituent count disagrees with its list".into(),
        ));
    }
    Ok(references)
}

fn oriented_curve_entity(
    geometry: &CurveGeometry,
    span: &CurveSpan,
    sense: Sense,
    version: crate::IgesVersion,
) -> Result<Entity, CodecError> {
    if sense == Sense::Forward {
        let mut entity = curve_entity(geometry, Some(span), version)?;
        entity.status = PHYSICALLY_DEPENDENT_STATUS;
        return Ok(entity);
    }
    let reversed_span = CurveSpan {
        range: span.range,
        start: span.end,
        end: span.start,
    };
    let mut entity = match geometry {
        CurveGeometry::Line { .. } => curve_entity(geometry, Some(&reversed_span), version)?,
        CurveGeometry::Nurbs(nurbs) => {
            let (reversed, range) = reverse_nurbs(nurbs, span.range)?;
            let reversed_span = CurveSpan {
                range,
                ..reversed_span
            };
            curve_entity(
                &CurveGeometry::Nurbs(reversed),
                Some(&reversed_span),
                version,
            )?
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let reversed = crate::entities::curve_conversion::circular_arc_nurbs(
                *center,
                *axis,
                *ref_direction,
                *radius,
                span.range,
            )
            .ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "IGES reversed circular edge span is not convertible ({span:?})"
                ))
            })?;
            let (reversed, range) = reverse_nurbs(&reversed, span.range)?;
            let reversed_span = CurveSpan {
                range,
                ..reversed_span
            };
            curve_entity(
                &CurveGeometry::Nurbs(reversed),
                Some(&reversed_span),
                version,
            )?
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let reversed = crate::entities::curve_conversion::elliptical_arc_nurbs(
                *center,
                *axis,
                *major_direction,
                *major_radius,
                *minor_radius,
                span.range,
            )
            .ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "IGES reversed elliptical edge span is not convertible ({span:?})"
                ))
            })?;
            let (reversed, range) = reverse_nurbs(&reversed, span.range)?;
            let reversed_span = CurveSpan {
                range,
                ..reversed_span
            };
            curve_entity(
                &CurveGeometry::Nurbs(reversed),
                Some(&reversed_span),
                version,
            )?
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => {
            let reversed = crate::entities::curve_conversion::parabolic_arc_nurbs(
                *vertex,
                *axis,
                *major_direction,
                *focal_distance,
                span.range,
            )
            .ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "IGES reversed parabolic edge span is not convertible ({span:?})"
                ))
            })?;
            let (reversed, range) = reverse_nurbs(&reversed, span.range)?;
            let reversed_span = CurveSpan {
                range,
                ..reversed_span
            };
            curve_entity(
                &CurveGeometry::Nurbs(reversed),
                Some(&reversed_span),
                version,
            )?
        }
        CurveGeometry::Polyline {
            points, parameters, ..
        } => {
            let values = polyline_parameters(points.len(), parameters.as_deref())?;
            let original = NurbsCurve {
                degree: 1,
                knots: polyline_knots(&values),
                control_points: points.clone(),
                weights: None,
                periodic: false,
            };
            let (reversed, range) = reverse_nurbs(&original, span.range)?;
            let reversed_span = CurveSpan {
                range,
                ..reversed_span
            };
            curve_entity(
                &CurveGeometry::Nurbs(reversed),
                Some(&reversed_span),
                version,
            )?
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            // The hyperbola parameterization satisfies p(-u) = p(u) with its
            // transverse axis reversed. Emit that equivalent frame with the
            // reflected interval so the Type 104 endpoints follow the
            // reversed coedge without introducing an approximation.
            let reversed_geometry = CurveGeometry::Hyperbola {
                center: *center,
                axis: axis.scale(-1.0),
                major_direction: *major_direction,
                major_radius: *major_radius,
                minor_radius: *minor_radius,
            };
            let reversed_range = [-span.range[1], -span.range[0]];
            if reversed_range.iter().any(|value| !value.is_finite()) {
                return Err(CodecError::Malformed(
                    "IGES reversed hyperbola parameter range is non-finite".into(),
                ));
            }
            let reversed_span = CurveSpan {
                range: reversed_range,
                start: span.end,
                end: span.start,
            };
            curve_entity(&reversed_geometry, Some(&reversed_span), version)?
        }
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "IGES reversed Type 142 edge carrier is unsupported ({geometry:?})"
            )))
        }
    };
    entity.status = PHYSICALLY_DEPENDENT_STATUS;
    Ok(entity)
}

fn line_directrix(ir: &CadIr, curve_id: &CurveId) -> bool {
    fn is_line(geometry: &CurveGeometry, depth: usize) -> bool {
        if depth > 256 {
            return false;
        }
        match geometry {
            CurveGeometry::Line { .. } => true,
            CurveGeometry::Transformed { basis, .. } => is_line(basis, depth + 1),
            _ => false,
        }
    }

    ir.model
        .curves
        .iter()
        .find(|curve| curve.id == *curve_id)
        .is_some_and(|curve| is_line(&curve.geometry, 0))
}

fn affine_parameter_map(source: [f64; 2], target: [f64; 2]) -> Option<(f64, f64)> {
    let source_width = source[1] - source[0];
    let target_width = target[1] - target[0];
    if !source
        .iter()
        .chain(target.iter())
        .all(|value| value.is_finite())
        || source_width <= 0.0
        || target_width <= 0.0
    {
        return None;
    }
    let scale = target_width / source_width;
    let offset = target[0] - source[0] * scale;
    (scale.is_finite() && offset.is_finite()).then_some((scale, offset))
}

fn procedural_pcurve_source_map(
    ir: &CadIr,
    surface_id: &SurfaceId,
) -> Result<Option<(f64, f64, f64, f64)>, CodecError> {
    let Some(procedural) = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.surface == *surface_id)
    else {
        return Ok(None);
    };
    let (directrix, fallback_interval) = match procedural.definition() {
        ProceduralSurfaceDefinition::Extrusion {
            directrix,
            parameter_interval,
            ..
        }
        | ProceduralSurfaceDefinition::Revolution {
            directrix,
            parameter_interval,
            ..
        } => (directrix, parameter_interval.unwrap_or([0.0, 1.0])),
        _ => return Ok(None),
    };
    let source_curve = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES procedural surface directrix {directrix} is missing"
            ))
        })?;
    let geometry = flatten_curve(&source_curve.geometry)?;
    let carrier_interval =
        construction_carrier_interval(ir, directrix, &geometry, procedural, fallback_interval)?;
    let mut u_map;
    let mut v_map = (1.0, 0.0);
    match procedural.definition() {
        ProceduralSurfaceDefinition::Extrusion {
            directrix,
            parameter_interval,
            ..
        } => {
            let source_interval = if line_directrix(ir, directrix) {
                parameter_interval.unwrap_or([0.0, 1.0])
            } else {
                parameter_interval.unwrap_or(carrier_interval)
            };
            u_map = affine_parameter_map(carrier_interval, source_interval).ok_or_else(|| {
                CodecError::Malformed(
                    "IGES procedural surface parameter domains are invalid".into(),
                )
            })?;
        }
        ProceduralSurfaceDefinition::Revolution {
            directrix,
            angular_interval,
            angular_parameter_interval,
            parameter_interval,
            transposed,
            ..
        } => {
            let source_interval = if line_directrix(ir, directrix) {
                parameter_interval.unwrap_or([0.0, 1.0])
            } else {
                parameter_interval.unwrap_or(carrier_interval)
            };
            u_map = affine_parameter_map(carrier_interval, source_interval).ok_or_else(|| {
                CodecError::Malformed(
                    "IGES procedural surface parameter domains are invalid".into(),
                )
            })?;
            if let Some(parameter_interval) = angular_parameter_interval {
                v_map = affine_parameter_map(*angular_interval, *parameter_interval).ok_or_else(
                    || {
                        CodecError::Malformed(
                            "IGES procedural surface angular domains are invalid".into(),
                        )
                    },
                )?;
            }
            if *transposed {
                std::mem::swap(&mut u_map, &mut v_map);
            }
        }
        _ => return Ok(None),
    }
    Ok(Some((u_map.0, u_map.1, v_map.0, v_map.1)))
}

fn pcurve_support_surfaces(ir: &CadIr, pcurve_id: &cadmpeg_ir::ids::PcurveId) -> Vec<SurfaceId> {
    let mut surfaces = BTreeSet::new();
    for face in &ir.model.faces {
        for loop_id in &face.loops {
            let Some(loop_) = ir.model.loops.iter().find(|loop_| loop_.id == *loop_id) else {
                continue;
            };
            if loop_.coedges().iter().any(|coedge_id| {
                ir.model
                    .coedges
                    .iter()
                    .find(|coedge| coedge.id == *coedge_id)
                    .is_some_and(|coedge| {
                        coedge.pcurves.iter().any(|use_| use_.pcurve == *pcurve_id)
                    })
            }) {
                surfaces.insert(face.surface.clone());
            }
        }
    }
    surfaces.into_iter().collect()
}

fn source_pcurve(ir: &CadIr, pcurve: &Pcurve) -> Result<Pcurve, CodecError> {
    let mut parameter_map = None;
    for surface_id in pcurve_support_surfaces(ir, &pcurve.id) {
        let candidate =
            procedural_pcurve_source_map(ir, &surface_id)?.unwrap_or((1.0, 0.0, 1.0, 0.0));
        if parameter_map.is_some_and(|existing| existing != candidate) {
            return Err(CodecError::NotImplemented(format!(
                "IGES pcurve {} is shared by incompatible procedural surface parameterizations",
                pcurve.id
            )));
        }
        parameter_map = Some(candidate);
    }
    let Some((u_factor, u_offset, v_factor, v_offset)) = parameter_map else {
        return Ok(pcurve.clone());
    };
    let mut pcurve = pcurve.clone();
    let PcurveGeometry::Nurbs { control_points, .. } = &mut pcurve.geometry else {
        return Ok(pcurve);
    };
    for point in control_points {
        point.u = point.u.mul_add(u_factor, u_offset);
        point.v = point.v.mul_add(v_factor, v_offset);
    }
    Ok(pcurve)
}

fn oriented_pcurve_entity(ir: &CadIr, pcurve: &Pcurve) -> Result<Entity, CodecError> {
    let pcurve = source_pcurve(ir, pcurve)?;
    let range = pcurve.parameter_range.ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "IGES semantic writer requires a parameter range for pcurve {}",
            pcurve.id
        ))
    })?;
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = &pcurve.geometry
    else {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer only encodes NURBS pcurves ({})",
            pcurve.id
        )));
    };
    let nurbs = NurbsCurve {
        degree: *degree,
        knots: knots.clone(),
        control_points: control_points
            .iter()
            .map(|point| Point3::new(point.u, point.v, 0.0))
            .collect(),
        weights: weights.clone(),
        periodic: *periodic,
    };
    let (reversed, range) = reverse_nurbs(&nurbs, range)?;
    encode_nurbs(&reversed, range, "PCURVE")
}

fn reverse_nurbs(
    nurbs: &NurbsCurve,
    range: [f64; 2],
) -> Result<(NurbsCurve, [f64; 2]), CodecError> {
    let domain = nurbs_domain(nurbs)?;
    if domain.iter().any(|value| !value.is_finite())
        || domain[0] > domain[1]
        || range.iter().any(|value| !value.is_finite())
        || range[0] > range[1]
        || range[0] < domain[0]
        || range[1] > domain[1]
        || nurbs.knots.iter().any(|value| !value.is_finite())
        || !knots_nondecreasing(&nurbs.knots)
    {
        return Err(CodecError::Malformed(
            "IGES reversed NURBS domain or parameter range is invalid".into(),
        ));
    }
    let sum = domain[0] + domain[1];
    if !sum.is_finite() {
        return Err(CodecError::Malformed(
            "IGES reversed NURBS parameter range is non-finite".into(),
        ));
    }
    let knots = nurbs
        .knots
        .iter()
        .rev()
        .map(|knot| sum - knot)
        .collect::<Vec<_>>();
    let reversed_range = [sum - range[1], sum - range[0]];
    if reversed_range.iter().any(|value| !value.is_finite())
        || knots.iter().any(|knot| !knot.is_finite())
    {
        return Err(CodecError::Malformed(
            "IGES reversed NURBS knot vector or parameter range is non-finite".into(),
        ));
    }
    Ok((
        NurbsCurve {
            degree: nurbs.degree,
            knots,
            control_points: nurbs.control_points.iter().rev().copied().collect(),
            weights: nurbs
                .weights
                .as_ref()
                .map(|weights| weights.iter().rev().copied().collect()),
            periodic: nurbs.periodic,
        },
        reversed_range,
    ))
}

fn pcurve_entity(ir: &CadIr, pcurve: &Pcurve) -> Result<Entity, CodecError> {
    let pcurve = source_pcurve(ir, pcurve)?;
    let range = pcurve.parameter_range.ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "IGES semantic writer requires a parameter range for pcurve {}",
            pcurve.id
        ))
    })?;
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = &pcurve.geometry
    else {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer only encodes NURBS pcurves ({})",
            pcurve.id
        )));
    };
    let control_points = control_points
        .iter()
        .map(|point| Point3::new(point.u, point.v, 0.0))
        .collect();
    encode_nurbs(
        &NurbsCurve {
            degree: *degree,
            knots: knots.clone(),
            control_points,
            weights: weights.clone(),
            periodic: *periodic,
        },
        range,
        "PCURVE",
    )
}

fn reference_marker(index: usize) -> String {
    format!("@R{index}@")
}

fn resolve_entity_references(entities: &mut [Entity]) -> Result<(), CodecError> {
    let mut directory_sequences = Vec::with_capacity(entities.len());
    let mut expanded_index = 0_u32;
    for entity in entities.iter() {
        if entity.transform.is_some() {
            expanded_index = expanded_index
                .checked_add(1)
                .ok_or_else(|| CodecError::Malformed("IGES entity sequence overflows".into()))?;
        }
        let sequence = expanded_index
            .checked_mul(2)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| CodecError::Malformed("IGES entity sequence overflows".into()))?;
        directory_sequences.push(sequence);
        expanded_index = expanded_index
            .checked_add(1)
            .ok_or_else(|| CodecError::Malformed("IGES entity sequence overflows".into()))?;
    }
    for entity in entities {
        let mut resolved = Vec::with_capacity(entity.parameters.len());
        let mut index = 0;
        while index < entity.parameters.len() {
            if entity.parameters[index] == b'@' && entity.parameters.get(index + 1) == Some(&b'R') {
                let Some(end_offset) = entity.parameters[index + 2..]
                    .iter()
                    .position(|byte| *byte == b'@')
                else {
                    return Err(CodecError::Malformed(
                        "IGES entity contains an unterminated pointer reference".into(),
                    ));
                };
                let end = index + 2 + end_offset;
                let target = std::str::from_utf8(&entity.parameters[index + 2..end])
                    .ok()
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or_else(|| {
                        CodecError::Malformed(
                            "IGES entity contains an invalid pointer reference".into(),
                        )
                    })?;
                let sequence = directory_sequences.get(target).copied().ok_or_else(|| {
                    CodecError::Malformed("IGES pointer reference targets no entity".into())
                })?;
                resolved.extend_from_slice(sequence.to_string().as_bytes());
                index = end + 1;
            } else {
                resolved.push(entity.parameters[index]);
                index += 1;
            }
        }
        entity.parameters = resolved;
    }
    Ok(())
}

fn same_range(left: [f64; 2], right: [f64; 2]) -> bool {
    left.into_iter().zip(right).all(|(left, right)| {
        (left - right).abs() <= left.abs().max(right.abs()).max(1.0) * EPS_WRITE_DEGENERATE
    })
}

fn isoparametric_flag(pcurve_use: &PcurveUse, owner: &str) -> Result<i32, CodecError> {
    match pcurve_use.isoparametric {
        Some(isoparametric) => Ok(i32::from(isoparametric)),
        None => Err(CodecError::NotImplemented(format!(
            "IGES {owner} requires an explicit isoparametric flag for pcurve {}",
            pcurve_use.pcurve
        ))),
    }
}

fn validate_brep_pcurve_uses(
    orientation: &PcurveOrientationContext<'_>,
    uses: &[PcurveUse],
) -> Result<(), CodecError> {
    for pcurve_use in uses {
        isoparametric_flag(pcurve_use, orientation.owner)?;
        let pcurve = orientation
            .ir
            .model
            .pcurves
            .iter()
            .find(|pcurve| pcurve.id == pcurve_use.pcurve)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES B-rep {} references missing pcurve {}",
                    orientation.owner, pcurve_use.pcurve
                ))
            })?;
        let range = pcurve.parameter_range.ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "IGES B-rep {} requires a parameter range for pcurve {}",
                orientation.owner, pcurve.id
            ))
        })?;
        if pcurve_use
            .parameter_range
            .is_some_and(|use_range| !same_range(use_range, range))
        {
            return Err(CodecError::NotImplemented(format!(
                "IGES B-rep {} cannot restrict pcurve use {}",
                orientation.owner, pcurve_use.pcurve
            )));
        }
        if pcurve.wrapper_reversed.is_some() || pcurve.native_tail_flags.is_some() {
            return Err(CodecError::NotImplemented(format!(
                "IGES B-rep {} does not encode pcurve wrapper metadata {}",
                orientation.owner, pcurve.id
            )));
        }
        pcurve_entity(orientation.ir, pcurve)?;
    }
    orientation.validate(uses)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PcurveOrientation {
    Natural,
    Directed,
}

struct PcurveOrientationContext<'a> {
    ir: &'a CadIr,
    surface: &'a SurfaceGeometry,
    natural_start: Point3,
    natural_end: Point3,
    sense: Sense,
    tolerance: f64,
    owner: &'a str,
}

impl PcurveOrientationContext<'_> {
    fn validate(&self, uses: &[PcurveUse]) -> Result<(), CodecError> {
        self.orientation(uses).map(|_| ())
    }

    fn orientation(&self, uses: &[PcurveUse]) -> Result<PcurveOrientation, CodecError> {
        if uses.is_empty() {
            return Ok(PcurveOrientation::Directed);
        }
        let tolerance = effective_topology_tolerance(self.tolerance);
        let mapped = self.map(uses)?;
        let (directed_start, directed_end) = if self.sense == Sense::Forward {
            (self.natural_start, self.natural_end)
        } else {
            (self.natural_end, self.natural_start)
        };
        if pcurve_chain_matches(&mapped, directed_start, directed_end, tolerance) {
            return Ok(PcurveOrientation::Directed);
        }
        if self.sense == Sense::Reversed
            && pcurve_chain_matches(&mapped, self.natural_start, self.natural_end, tolerance)
        {
            return Ok(PcurveOrientation::Natural);
        }
        Err(CodecError::malformed(format_args!(
            "IGES {} pcurve chain endpoints disagree with its directed support edge",
            self.owner
        )))
    }

    fn map(&self, uses: &[PcurveUse]) -> Result<Vec<(Point3, Point3)>, CodecError> {
        let mut mapped = Vec::with_capacity(uses.len());
        for pcurve_use in uses {
            let pcurve = self
                .ir
                .model
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == pcurve_use.pcurve)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES {} references missing pcurve {}",
                        self.owner, pcurve_use.pcurve
                    ))
                })?;
            let range = pcurve.parameter_range.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "IGES {} requires a parameter range for pcurve {}",
                    self.owner, pcurve.id
                ))
            })?;
            if range.iter().any(|value| !value.is_finite()) || range[0] > range[1] {
                return Err(CodecError::malformed(format_args!(
                    "IGES {} pcurve {} has an invalid parameter range",
                    self.owner, pcurve.id
                )));
            }
            let start_uv = pcurve_uv(&pcurve.geometry, range[0]).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES {} pcurve {} start cannot be evaluated",
                    self.owner, pcurve.id
                ))
            })?;
            let end_uv = pcurve_uv(&pcurve.geometry, range[1]).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES {} pcurve {} end cannot be evaluated",
                    self.owner, pcurve.id
                ))
            })?;
            let start = model_surface_point(self.ir, self.surface, start_uv.u, start_uv.v)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES {} pcurve {} start is outside its support",
                        self.owner, pcurve.id
                    ))
                })?;
            let end = model_surface_point(self.ir, self.surface, end_uv.u, end_uv.v).ok_or_else(
                || {
                    CodecError::malformed(format_args!(
                        "IGES {} pcurve {} end is outside its support",
                        self.owner, pcurve.id
                    ))
                },
            )?;
            ensure_finite_point(start, &format!("{} pcurve {} start", self.owner, pcurve.id))?;
            ensure_finite_point(end, &format!("{} pcurve {} end", self.owner, pcurve.id))?;
            mapped.push((start, end));
        }
        Ok(mapped)
    }

    fn oriented_entities<'uses>(
        &self,
        uses: &'uses [PcurveUse],
        pcurve_indices: &BTreeMap<String, usize>,
        entities: &mut Vec<Entity>,
    ) -> Result<Vec<(&'uses PcurveUse, usize)>, CodecError> {
        let orientation = self.orientation(uses)?;
        let reverse = self.sense == Sense::Reversed && orientation == PcurveOrientation::Natural;
        let ordered_uses = if reverse {
            uses.iter().rev().collect::<Vec<_>>()
        } else {
            uses.iter().collect::<Vec<_>>()
        };
        ordered_uses
            .into_iter()
            .map(|pcurve_use| {
                let pcurve = self
                    .ir
                    .model
                    .pcurves
                    .iter()
                    .find(|pcurve| pcurve.id == pcurve_use.pcurve)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES {} references missing pcurve {}",
                            self.owner, pcurve_use.pcurve
                        ))
                    })?;
                let index = if reverse {
                    let index = entities.len();
                    entities.push(oriented_pcurve_entity(self.ir, pcurve)?);
                    index
                } else {
                    *pcurve_indices.get(pcurve.id.as_str()).ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES {} references missing pcurve entity {}",
                            self.owner, pcurve.id
                        ))
                    })?
                };
                Ok((pcurve_use, index))
            })
            .collect()
    }
}

fn pcurve_orientation_context<'a>(
    ir: &'a CadIr,
    surface: &'a SurfaceGeometry,
    natural_start: Point3,
    natural_end: Point3,
    sense: Sense,
    tolerance: f64,
    owner: &'a str,
) -> PcurveOrientationContext<'a> {
    PcurveOrientationContext {
        ir,
        surface,
        natural_start,
        natural_end,
        sense,
        tolerance,
        owner,
    }
}

fn pcurve_chain_matches(
    mapped: &[(Point3, Point3)],
    expected_start: Point3,
    expected_end: Point3,
    tolerance: f64,
) -> bool {
    mapped
        .first()
        .is_some_and(|(start, _)| same_point_with_tolerance(*start, expected_start, tolerance))
        && mapped
            .last()
            .is_some_and(|(_, end)| same_point_with_tolerance(*end, expected_end, tolerance))
        && mapped
            .windows(2)
            .all(|pair| same_point_with_tolerance(pair[0].1, pair[1].0, tolerance))
}

fn same_point(left: Point3, right: Point3) -> bool {
    same_float(left.x, right.x) && same_float(left.y, right.y) && same_float(left.z, right.z)
}

fn same_point_with_tolerance(left: Point3, right: Point3, explicit_tolerance: f64) -> bool {
    same_point(left, right)
        || (explicit_tolerance.is_finite()
            && explicit_tolerance > 0.0
            && left.distance(right) <= explicit_tolerance)
}

fn topology_edge_explicit_tolerance(ir: &CadIr, edge: &Edge) -> f64 {
    let mut tolerance = edge.tolerance.unwrap_or(0.0);
    for vertex_id in [&edge.start, &edge.end] {
        if let Some(vertex) = ir
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == *vertex_id)
        {
            tolerance = tolerance.max(vertex.tolerance.unwrap_or(0.0));
        }
    }
    tolerance
}

fn generated_minimum_resolution(ir: &CadIr) -> f64 {
    let topology_tolerance = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.tolerance)
        .chain(
            ir.model
                .vertices
                .iter()
                .filter_map(|vertex| vertex.tolerance),
        )
        .filter(|tolerance| tolerance.is_finite() && *tolerance > 0.0)
        .map(effective_topology_tolerance)
        .fold(cadmpeg_ir::units::COINCIDENCE_TOLERANCE, f64::max);
    let endpoint_scale = generated_endpoint_coordinate_scale(ir);
    topology_tolerance.max(endpoint_scale * WRITER_ENDPOINT_RELATIVE_TOLERANCE)
}

fn minimum_resolution_for_output(ir: &CadIr) -> f64 {
    let generated = generated_minimum_resolution(ir);
    if ir.tolerances.linear.is_finite() && ir.tolerances.linear > 0.0 {
        generated.max(ir.tolerances.linear)
    } else {
        generated
    }
}

fn minimum_resolution_loss(ir: &CadIr, emitted: f64) -> Option<LossNote> {
    ir.source.as_ref()?;
    let declared = ir.tolerances.linear;
    if declared.is_finite() && declared > 0.0 && emitted <= declared {
        return None;
    }
    Some(IgesLossCode::WriterMinimumResolutionAdjusted.note(format!(
        "IGES Global minimum resolution changed from {declared:.17e} mm to {emitted:.17e} mm to cover the emitted geometry"
    )))
}

fn effective_topology_tolerance(explicit_tolerance: f64) -> f64 {
    if explicit_tolerance.is_finite() {
        explicit_tolerance.max(cadmpeg_ir::units::COINCIDENCE_TOLERANCE)
    } else {
        cadmpeg_ir::units::COINCIDENCE_TOLERANCE
    }
}

fn generated_endpoint_coordinate_scale(ir: &CadIr) -> f64 {
    let mut scale = 1.0_f64;
    for edge in &ir.model.edges {
        for vertex_id in [&edge.start, &edge.end] {
            if let Some(point) = vertex_position(ir, vertex_id) {
                scale = scale.max(point_coordinate_scale(point));
            }
        }
        let Some(curve_id) = edge.curve.as_ref() else {
            continue;
        };
        let Some(range) = edge.param_range else {
            continue;
        };
        let Some(curve) = ir.model.curves.iter().find(|curve| curve.id == *curve_id) else {
            continue;
        };
        for parameter in range {
            if let Some(point) = curve_point(&curve.geometry, parameter) {
                scale = scale.max(point_coordinate_scale(point));
            }
        }
    }
    scale
}

fn point_coordinate_scale(point: Point3) -> f64 {
    [point.x, point.y, point.z]
        .into_iter()
        .filter(|value| value.is_finite())
        .map(f64::abs)
        .fold(1.0, f64::max)
}

fn entity_counts(entities: &[Entity]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for entity in entities {
        let name = match entity.type_code {
            100 => "100_circular_arc",
            104 => "104_conic_arc",
            108 => "108_plane",
            190 => "190_pointer_plane",
            192 => "192_cylinder",
            194 => "194_cone",
            196 => "196_sphere",
            198 => "198_torus",
            110 => "110_line",
            116 => "116_point",
            123 => "123_direction",
            126 => "126_nurbs_curve",
            128 => "128_nurbs_surface",
            124 => "124_transformation",
            106 => "106_copious_data",
            102 => "102_composite_curve",
            141 => "141_boundary",
            142 => "142_curve_on_parametric_surface",
            143 => "143_bounded_surface",
            144 => "144_trimmed_surface",
            186 => "186_manifold_solid_brep",
            502 => "502_vertex_list",
            504 => "504_edge_list",
            508 => "508_loop",
            510 => "510_face",
            514 => "514_shell",
            _ => "unknown_entity",
        };
        *counts.entry(name.into()).or_insert(0) += 1;
    }
    counts
}

fn reject_unsupported_model(ir: &CadIr) -> Result<(), CodecError> {
    let unsupported = [
        ("subds", !ir.model.subds.is_empty()),
        ("assets", !ir.model.assets.is_empty()),
        ("features", !ir.model.features.is_empty()),
        (
            "feature_input_topologies",
            !ir.model.feature_input_topologies.is_empty(),
        ),
        (
            "feature_result_topologies",
            !ir.model.feature_result_topologies.is_empty(),
        ),
        ("configurations", !ir.model.configurations.is_empty()),
        ("parameters", !ir.model.parameters.is_empty()),
        ("sketches", !ir.model.sketches.is_empty()),
        ("sketch_entities", !ir.model.sketch_entities.is_empty()),
        (
            "sketch_constraints",
            !ir.model.sketch_constraints.is_empty(),
        ),
        ("spatial_sketches", !ir.model.spatial_sketches.is_empty()),
        (
            "spatial_sketch_entities",
            !ir.model.spatial_sketch_entities.is_empty(),
        ),
        (
            "spatial_sketch_constraints",
            !ir.model.spatial_sketch_constraints.is_empty(),
        ),
        ("spreadsheets", !ir.model.spreadsheets.is_empty()),
        (
            "product_definitions",
            !ir.model.product_definitions.is_empty(),
        ),
        ("occurrences", !ir.model.occurrences.is_empty()),
        ("assembly_joints", !ir.model.assembly_joints.is_empty()),
        ("drawings", !ir.model.drawings.is_empty()),
        (
            "semantic_annotations",
            !ir.model.semantic_annotations.is_empty(),
        ),
        (
            "presentation_documents",
            !ir.model.presentation_documents.is_empty(),
        ),
        (
            "view_presentations",
            !ir.model.view_presentations.is_empty(),
        ),
        ("tessellations", !ir.model.tessellations.is_empty()),
        ("appearances", !ir.model.appearances.is_empty()),
        (
            "appearance_bindings",
            !ir.model.appearance_bindings.is_empty(),
        ),
        ("attributes", !ir.model.attributes.is_empty()),
        ("pmi", !ir.model.pmi.is_empty()),
        (
            "presentation_layers",
            !ir.model.presentation_layers.is_empty(),
        ),
    ];
    if let Some((arena, _)) = unsupported.into_iter().find(|(_, present)| *present) {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer does not encode model arena {arena}"
        )));
    }
    for body in &ir.model.bodies {
        if body
            .transform
            .is_some_and(|transform| transform != cadmpeg_ir::transform::Transform::identity())
        {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer does not apply body transform {}",
                body.id
            )));
        }
    }
    for vertex in &ir.model.vertices {
        if !ir.model.points.iter().any(|point| point.id == vertex.point) {
            return Err(CodecError::malformed(format_args!(
                "IGES vertex {} references missing point {}",
                vertex.id, vertex.point
            )));
        }
    }
    Ok(())
}

fn reject_unsupported_native(ir: &CadIr) -> Result<Vec<LossNote>, CodecError> {
    let Some(namespace) = ir.native.namespace("iges") else {
        return Ok(Vec::new());
    };
    if let Some((arena, _)) = namespace.arenas.iter().find(|(arena, records)| {
        !records.is_empty() && !ALLOWED_NATIVE_ARENAS.contains(&arena.as_str())
    }) {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer cannot preserve native arena {arena}"
        )));
    }
    if let Some(record) = namespace
        .arenas
        .get("entities")
        .into_iter()
        .flatten()
        .find(|record| {
            !matches!(
                record.field("entity_type").and_then(|value| value.as_i64()),
                Some(
                    100 | 102
                        | 104
                        | 106
                        | 108
                        | 110
                        | 112
                        | 114
                        | 116
                        | 118
                        | 120
                        | 122
                        | 123
                        | 124
                        | 126
                        | 128
                        | 130
                        | 141
                        | 142
                        | 143
                        | 144
                        | 140
                        | 190
                        | 192
                        | 194
                        | 196
                        | 198
                        | 186
                        | 502
                        | 504
                        | 508
                        | 510
                        | 514,
                )
            )
        })
    {
        let entity_type = record
            .field("entity_type")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer does not encode native entity type {entity_type}"
        )));
    }
    let mut native_entities = namespace.arenas.get("entities").into_iter().flatten();
    for record in native_entities.clone().filter(|record| {
        let entity_type = record.field("entity_type").and_then(|value| value.as_i64());
        let form = record.field("form").and_then(|value| value.as_i64());
        matches!(entity_type, Some(100 | 102 | 104 | 110 | 112 | 126 | 130))
            || (entity_type == Some(106) && matches!(form, Some(1..=3 | 11..=13 | 63)))
    }) {
        let Some(sequence) = record
            .field("directory_sequence")
            .and_then(|value| value.as_i64())
            .and_then(|value| u32::try_from(value).ok())
        else {
            return Err(CodecError::Malformed(
                "IGES native curve entity has no directory sequence".into(),
            ));
        };
        let object_id = format!("D{sequence}");
        if !ir.model.curves.iter().any(|curve| {
            curve
                .source_object
                .as_ref()
                .is_some_and(|source| source.format == "iges" && source.object_id == object_id)
        }) {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer cannot preserve native curve entity {object_id} without neutral geometry"
            )));
        }
    }
    for record in native_entities.clone().filter(|record| {
        record.field("entity_type").and_then(|value| value.as_i64()) == Some(116)
            && record
                .field("subordinate_status")
                .and_then(|value| value.as_i64())
                != Some(1)
    }) {
        let Some(sequence) = record
            .field("directory_sequence")
            .and_then(|value| value.as_i64())
            .and_then(|value| u32::try_from(value).ok())
        else {
            return Err(CodecError::Malformed(
                "IGES native point entity has no directory sequence".into(),
            ));
        };
        let point_id = format!("iges:model:point#D{sequence}");
        if !ir
            .model
            .points
            .iter()
            .any(|point| point.id.as_str() == point_id)
        {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer cannot preserve native point entity D{sequence} without neutral geometry"
            )));
        }
    }
    let has_native_surface = native_entities.clone().any(|record| {
        matches!(
            record.field("entity_type").and_then(|value| value.as_i64()),
            Some(108 | 114 | 118 | 120 | 122 | 128 | 140 | 190 | 192 | 194 | 196 | 198)
        )
    });
    for record in native_entities.clone().filter(|record| {
        matches!(
            record.field("entity_type").and_then(|value| value.as_i64()),
            Some(108 | 114 | 118 | 120 | 122 | 128 | 140 | 190 | 192 | 194 | 196 | 198)
        )
    }) {
        let Some(sequence) = record
            .field("directory_sequence")
            .and_then(|value| value.as_i64())
            .and_then(|value| u32::try_from(value).ok())
        else {
            return Err(CodecError::Malformed(
                "IGES native surface entity has no directory sequence".into(),
            ));
        };
        let object_id = format!("D{sequence}");
        if !ir.model.surfaces.iter().any(|surface| {
            surface
                .source_object
                .as_ref()
                .is_some_and(|source| source.format == "iges" && source.object_id == object_id)
        }) {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer cannot preserve native surface entity D{sequence} without neutral geometry"
            )));
        }
    }
    let has_native_topology = native_entities.any(|record| {
        matches!(
            record.field("entity_type").and_then(|value| value.as_i64()),
            Some(141..=144 | 186 | 502 | 504 | 508 | 510 | 514)
        )
    });
    if has_native_surface && ir.model.surfaces.is_empty() {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer cannot preserve a native support surface without neutral geometry".into(),
        ));
    }
    if has_native_topology && !has_trimmed_sheet_topology(ir) {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer cannot preserve native trimming without neutral topology".into(),
        ));
    }
    let mut losses = Vec::new();
    for (arena, records) in &namespace.arenas {
        if records.is_empty() {
            continue;
        }
        let message = match arena.as_str() {
            "directions" => {
                "IGES direction records are regenerated only for required analytic-surface support; native direction identities and unrelated records are omitted".to_owned()
            }
            "display_attributes" => {
                "IGES display attributes are not regenerated by the bounded semantic writer".to_owned()
            }
            "product_occurrence_expansion" => {
                "IGES product occurrence expansion is not regenerated by the bounded semantic writer".to_owned()
            }
            "quarantined_directory_records" | "quarantined_parameter_records" => format!(
                "IGES native arena {arena} holds {} quarantined record(s) that the bounded semantic writer does not regenerate, because a record that failed typing has no fields to write",
                records.len()
            ),
            _ => continue,
        };
        losses.push(IgesLossCode::PassthroughRecordOmitted.note(message));
    }
    Ok(losses)
}

#[derive(Clone, Copy)]
struct Placement {
    rows: [[f64; 4]; 3],
}

#[derive(Debug)]
struct CurveSpan {
    range: [f64; 2],
    start: Point3,
    end: Point3,
}

fn construction_carrier_interval(
    ir: &CadIr,
    directrix: &CurveId,
    geometry: &CurveGeometry,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    fallback: [f64; 2],
) -> Result<[f64; 2], CodecError> {
    match procedural.record_bounds {
        None => {
            if matches!(geometry, CurveGeometry::Line { .. })
                && ir
                    .model
                    .edges
                    .iter()
                    .any(|edge| edge.curve.as_ref() == Some(directrix))
            {
                curve_reference_span(ir, directrix, geometry).map(|span| span.range)
            } else {
                Ok(fallback)
            }
        }
        Some([Some(start), Some(end), _, _])
            if start.is_finite() && end.is_finite() && start < end =>
        {
            Ok([start, end])
        }
        Some(_) => Err(CodecError::Malformed(
            "IGES procedural surface directrix bounds are invalid".into(),
        )),
    }
}

fn point_entity(position: Point3) -> Entity {
    point_entity_with_status(position, "00000000")
}

fn point_entity_with_status(position: Point3, status: &'static str) -> Entity {
    Entity {
        type_code: 116,
        form: 0,
        label: "POINT",
        status,
        parameters: format!(
            "116,{},{},{};",
            number(position.x),
            number(position.y),
            number(position.z)
        )
        .into_bytes(),
        transform: None,
    }
}

fn direction_entity(direction: Vector3) -> Result<Entity, CodecError> {
    let direction = unit(direction, "analytic surface direction")?;
    Ok(Entity {
        type_code: 123,
        form: 0,
        label: "DIRECTN",
        status: PHYSICALLY_DEPENDENT_STATUS,
        parameters: format!(
            "123,{},{},{};",
            number(direction.x),
            number(direction.y),
            number(direction.z)
        )
        .into_bytes(),
        transform: None,
    })
}

fn pointer_surface_support(
    base_index: usize,
    location: Point3,
    axis: Vector3,
    reference: Vector3,
) -> Result<(Vec<Entity>, usize, usize, usize), CodecError> {
    ensure_finite_point(location, "analytic surface location")?;
    let (axis, reference) = orthonormal_pair(
        axis,
        reference,
        "analytic surface axis and reference direction",
    )?;
    let location_index = base_index;
    let axis_index = base_index
        .checked_add(1)
        .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
    let reference_index = base_index
        .checked_add(2)
        .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
    Ok((
        vec![
            point_entity_with_status(location, PHYSICALLY_DEPENDENT_STATUS),
            direction_entity(axis)?,
            direction_entity(reference)?,
        ],
        location_index,
        axis_index,
        reference_index,
    ))
}

#[derive(Clone, Copy)]
enum AnalyticSurfaceFamily {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
}

impl AnalyticSurfaceFamily {
    const fn type_code(self) -> u32 {
        match self {
            Self::Plane => 190,
            Self::Cylinder => 192,
            Self::Cone => 194,
            Self::Sphere => 196,
            Self::Torus => 198,
        }
    }
}

fn analytic_surface_family(geometry: &SurfaceGeometry) -> Option<AnalyticSurfaceFamily> {
    match geometry {
        SurfaceGeometry::Plane { .. } => Some(AnalyticSurfaceFamily::Plane),
        SurfaceGeometry::Cylinder { .. } => Some(AnalyticSurfaceFamily::Cylinder),
        SurfaceGeometry::Cone { .. } => Some(AnalyticSurfaceFamily::Cone),
        SurfaceGeometry::Sphere { .. } => Some(AnalyticSurfaceFamily::Sphere),
        SurfaceGeometry::Torus { .. } => Some(AnalyticSurfaceFamily::Torus),
        SurfaceGeometry::Nurbs(_) => None,
        _ => None,
    }
}

fn append_surface_entities(
    entities: &mut Vec<Entity>,
    ir: &CadIr,
    geometry: &SurfaceGeometry,
    version: crate::IgesVersion,
) -> Result<usize, CodecError> {
    let base_index = entities.len();
    let additions = surface_entities_for_ir(ir, geometry, base_index, version)?;
    let surface_offset = additions
        .len()
        .checked_sub(1)
        .ok_or_else(|| CodecError::Malformed("IGES surface encoder produced no entity".into()))?;
    let surface_index = base_index
        .checked_add(surface_offset)
        .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
    entities.extend(additions);
    Ok(surface_index)
}

fn surface_entities_for_ir(
    ir: &CadIr,
    geometry: &SurfaceGeometry,
    base_index: usize,
    version: crate::IgesVersion,
) -> Result<Vec<Entity>, CodecError> {
    match geometry {
        SurfaceGeometry::Procedural { construction } => {
            let procedural = ir
                .model
                .procedural_surfaces
                .iter()
                .find(|candidate| candidate.id == *construction)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES procedural surface construction {construction} is missing"
                    ))
                })?;
            match procedural.definition() {
                ProceduralSurfaceDefinition::Revolution { .. } => {
                    revolution_surface_entities(ir, construction, base_index, version)
                }
                ProceduralSurfaceDefinition::Extrusion { .. } => {
                    extrusion_surface_entities(ir, construction, base_index, version)
                }
                _ => Err(CodecError::NotImplemented(
                    "IGES semantic writer only encodes procedural Revolution and Extrusion surfaces as native entities".into(),
                )),
            }
        }
        _ => surface_entities(geometry, base_index, version),
    }
}

fn extrusion_surface_entities(
    ir: &CadIr,
    construction: &cadmpeg_ir::ids::ProceduralSurfaceId,
    base_index: usize,
    version: crate::IgesVersion,
) -> Result<Vec<Entity>, CodecError> {
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|candidate| candidate.id == *construction)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES procedural surface construction {construction} is missing"
            ))
        })?;
    let ProceduralSurfaceDefinition::Extrusion {
        directrix,
        parameter_interval,
        direction,
        native_position,
        revision_form,
    } = procedural.definition()
    else {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer only encodes Extrusion surfaces as Type 122".into(),
        ));
    };
    if revision_form.is_some() {
        return Err(CodecError::NotImplemented(
            "IGES Type 122 output does not encode revision-gated extrusion fields".into(),
        ));
    }
    let [start_parameter, terminate_parameter] = parameter_interval.ok_or_else(|| {
        CodecError::NotImplemented(
            "IGES Type 122 output requires a bounded directrix parameter interval".into(),
        )
    })?;
    if !start_parameter.is_finite()
        || !terminate_parameter.is_finite()
        || start_parameter >= terminate_parameter
    {
        return Err(CodecError::Malformed(
            "IGES Type 122 directrix parameter interval is invalid".into(),
        ));
    }
    if !direction.x.is_finite()
        || !direction.y.is_finite()
        || !direction.z.is_finite()
        || direction.norm() <= 0.0
    {
        return Err(CodecError::Malformed(
            "IGES Type 122 sweep direction must be finite and non-zero".into(),
        ));
    }
    let source_curve = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES Type 122 directrix {directrix} is missing"
            ))
        })?;
    let geometry = flatten_curve(&source_curve.geometry)?;
    let carrier_interval = construction_carrier_interval(
        ir,
        directrix,
        &geometry,
        procedural,
        [start_parameter, terminate_parameter],
    )?;
    let (start, end) = if matches!(&geometry, CurveGeometry::Composite { .. }) {
        let span = curve_reference_span(ir, directrix, &geometry)?;
        if !same_range(span.range, [start_parameter, terminate_parameter]) {
            return Err(CodecError::NotImplemented(
                "IGES Type 122 composite directrix range is not its canonical Type 102 range"
                    .into(),
            ));
        }
        (span.start, span.end)
    } else {
        let evaluation_interval = if matches!(&geometry, CurveGeometry::Line { .. }) {
            carrier_interval
        } else {
            [start_parameter, terminate_parameter]
        };
        let start = curve_point(&geometry, evaluation_interval[0]).ok_or_else(|| {
            CodecError::Malformed("IGES Type 122 directrix start cannot be evaluated".into())
        })?;
        let end = curve_point(&geometry, evaluation_interval[1]).ok_or_else(|| {
            CodecError::Malformed("IGES Type 122 directrix terminate cannot be evaluated".into())
        })?;
        (start, end)
    };
    ensure_finite_point(start, "Type 122 directrix start")?;
    ensure_finite_point(end, "Type 122 directrix terminate")?;
    let inferred_target = start.translated(*direction, 1.0);
    ensure_finite_point(inferred_target, "Type 122 inferred terminate point")?;
    let target = native_position.as_ref().copied().unwrap_or(inferred_target);
    ensure_finite_point(target, "Type 122 terminate point")?;
    if !same_point(target, inferred_target) {
        return Err(CodecError::Malformed(
            "IGES Type 122 native terminate point disagrees with its sweep direction".into(),
        ));
    }

    let directrix_span = CurveSpan {
        range: [start_parameter, terminate_parameter],
        start,
        end,
    };
    let mut entities = Vec::new();
    let directrix_local_index = append_curve_entity_with_reference_offset(
        &mut entities,
        ir,
        CurveEntityRequest {
            version,
            curve_id: directrix,
            geometry: &geometry,
            span: Some(&directrix_span),
            sense: Sense::Forward,
            status: PHYSICALLY_DEPENDENT_STATUS,
            reference_offset: base_index,
        },
    )?;
    let directrix_index = base_index
        .checked_add(directrix_local_index)
        .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
    entities.push(Entity {
        type_code: 122,
        form: 0,
        label: "TABULATE",
        status: "00000000",
        parameters: format!(
            "122,{},{},{},{};",
            reference_marker(directrix_index),
            number(target.x),
            number(target.y),
            number(target.z)
        )
        .into_bytes(),
        transform: None,
    });
    Ok(entities)
}

fn revolution_surface_entities(
    ir: &CadIr,
    construction: &cadmpeg_ir::ids::ProceduralSurfaceId,
    base_index: usize,
    version: crate::IgesVersion,
) -> Result<Vec<Entity>, CodecError> {
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|candidate| candidate.id == *construction)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES procedural surface construction {construction} is missing"
            ))
        })?;
    let ProceduralSurfaceDefinition::Revolution {
        directrix,
        axis_origin,
        axis_direction,
        angular_interval,
        angular_parameter_interval,
        parameter_interval,
        transposed,
        revision_form,
    } = procedural.definition()
    else {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer only encodes procedural Revolution surfaces as Type 120".into(),
        ));
    };
    if angular_parameter_interval.is_some() || *transposed || revision_form.is_some() {
        return Err(CodecError::NotImplemented(
            "IGES Type 120 output requires the default revolution parameterization".into(),
        ));
    }
    let [start_angle, terminate_angle] = *angular_interval;
    let sweep = terminate_angle - start_angle;
    if !start_angle.is_finite()
        || !terminate_angle.is_finite()
        || !sweep.is_finite()
        || sweep <= 0.0
        || sweep > TAU + ANGULAR_TOLERANCE
    {
        return Err(CodecError::Malformed(
            "IGES Type 120 angular interval is outside (0, 2*pi]".into(),
        ));
    }
    let terminate_angle = start_angle + sweep.min(TAU);
    let [start_parameter, terminate_parameter] = parameter_interval.ok_or_else(|| {
        CodecError::NotImplemented(
            "IGES Type 120 output requires a bounded generatrix parameter interval".into(),
        )
    })?;
    if !start_parameter.is_finite()
        || !terminate_parameter.is_finite()
        || start_parameter >= terminate_parameter
    {
        return Err(CodecError::Malformed(
            "IGES Type 120 generatrix parameter interval is invalid".into(),
        ));
    }
    let source_curve = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES Type 120 generatrix {directrix} is missing"
            ))
        })?;
    let geometry = flatten_curve(&source_curve.geometry)?;
    let carrier_interval = construction_carrier_interval(
        ir,
        directrix,
        &geometry,
        procedural,
        [start_parameter, terminate_parameter],
    )?;
    let evaluation_interval = if matches!(&geometry, CurveGeometry::Line { .. }) {
        carrier_interval
    } else {
        [start_parameter, terminate_parameter]
    };
    let start = curve_point(&geometry, evaluation_interval[0]).ok_or_else(|| {
        CodecError::Malformed("IGES Type 120 generatrix start cannot be evaluated".into())
    })?;
    let end = curve_point(&geometry, evaluation_interval[1]).ok_or_else(|| {
        CodecError::Malformed("IGES Type 120 generatrix terminate cannot be evaluated".into())
    })?;
    ensure_finite_point(start, "Type 120 generatrix start")?;
    ensure_finite_point(end, "Type 120 generatrix terminate")?;
    let axis_direction = unit(*axis_direction, "Type 120 axis direction")?;
    let axis_end = axis_origin.translated(axis_direction, 1.0);
    let axis_geometry = CurveGeometry::Line {
        origin: *axis_origin,
        direction: axis_direction,
    };
    let axis_span = CurveSpan {
        range: [0.0, 1.0],
        start: *axis_origin,
        end: axis_end,
    };
    let generatrix_span = CurveSpan {
        range: [start_parameter, terminate_parameter],
        start,
        end,
    };
    let directrix_index = base_index
        .checked_add(1)
        .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
    Ok(vec![
        curve_entity(&axis_geometry, Some(&axis_span), version)?,
        curve_entity(&geometry, Some(&generatrix_span), version)?,
        Entity {
            type_code: 120,
            form: 0,
            label: "REVOLVE",
            status: "00000000",
            parameters: format!(
                "120,{},{},{},{};",
                reference_marker(base_index),
                reference_marker(directrix_index),
                number(start_angle),
                number(terminate_angle)
            )
            .into_bytes(),
            transform: None,
        },
    ])
}

fn surface_entities(
    geometry: &SurfaceGeometry,
    base_index: usize,
    version: crate::IgesVersion,
) -> Result<Vec<Entity>, CodecError> {
    let analytic_type_code =
        analytic_surface_family(geometry).map(AnalyticSurfaceFamily::type_code);
    match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            if matches!(version, crate::IgesVersion::V4_0 | crate::IgesVersion::V5_0) {
                let (normal, u_axis) = orthonormal_pair(*normal, *u_axis, "legacy plane basis")?;
                let v_axis = normal.cross(u_axis);
                return Ok(vec![Entity {
                    // Type 108 Form 0 is the unbounded plane carrier in the
                    // V4.0 and V5.0 profiles.  Keep the neutral plane frame
                    // in its exact local Z=0 form and apply the frame through
                    // the standard rigid Directory transformation.
                    type_code: 108,
                    form: 0,
                    label: "PLANE",
                    status: "00000000",
                    parameters: b"108,0,0,1,0,0,0,0,0,0;".to_vec(),
                    transform: Some(placement(*origin, u_axis, v_axis, normal)?),
                }]);
            }
            let (mut entities, location, axis, reference) =
                pointer_surface_support(base_index, *origin, *normal, *u_axis)?;
            entities.push(Entity {
                type_code: analytic_type_code.ok_or_else(|| {
                    CodecError::Malformed("IGES plane has no analytic surface family".into())
                })?,
                form: 1,
                label: "PLANE",
                status: PHYSICALLY_DEPENDENT_STATUS,
                parameters: format!(
                    "190,{},{},{};",
                    reference_marker(location),
                    reference_marker(axis),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            });
            Ok(entities)
        }
        SurfaceGeometry::Nurbs(nurbs) => Ok(vec![encode_nurbs_surface(nurbs)?]),
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            if !radius.is_finite() || *radius <= 0.0 {
                return Err(CodecError::Malformed(
                    "IGES cylinder radius must be positive and finite".into(),
                ));
            }
            let (mut entities, location, axis, reference) =
                pointer_surface_support(base_index, *origin, *axis, *ref_direction)?;
            let surface = Entity {
                type_code: analytic_type_code.ok_or_else(|| {
                    CodecError::Malformed("IGES cylinder has no analytic surface family".into())
                })?,
                form: 1,
                label: "CYLINDER",
                status: "00000000",
                parameters: format!(
                    "192,{},{},{},{};",
                    reference_marker(location),
                    reference_marker(axis),
                    number(*radius),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            };
            entities.push(surface);
            Ok(entities)
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            if !same_float(*ratio, 1.0) {
                return Err(CodecError::NotImplemented(
                    "IGES analytic cone writer only encodes circular cones".into(),
                ));
            }
            if !radius.is_finite() || *radius < 0.0 {
                return Err(CodecError::Malformed(
                    "IGES cone radius must be finite and non-negative".into(),
                ));
            }
            if !half_angle.is_finite()
                || *half_angle <= 0.0
                || *half_angle >= std::f64::consts::FRAC_PI_2
            {
                return Err(CodecError::Malformed(
                    "IGES cone semi-angle must be in (0, 90) degrees".into(),
                ));
            }
            let (mut entities, location, axis, reference) =
                pointer_surface_support(base_index, *origin, *axis, *ref_direction)?;
            let surface = Entity {
                type_code: analytic_type_code.ok_or_else(|| {
                    CodecError::Malformed("IGES cone has no analytic surface family".into())
                })?,
                form: 1,
                label: "CONE",
                status: "00000000",
                parameters: format!(
                    "194,{},{},{},{},{};",
                    reference_marker(location),
                    reference_marker(axis),
                    number(*radius),
                    number(half_angle.to_degrees()),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            };
            entities.push(surface);
            Ok(entities)
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            if !radius.is_finite() || *radius <= 0.0 {
                return Err(CodecError::Malformed(
                    "IGES sphere radius must be positive and finite".into(),
                ));
            }
            let (mut entities, location, axis, reference) =
                pointer_surface_support(base_index, *center, *axis, *ref_direction)?;
            let surface = Entity {
                type_code: analytic_type_code.ok_or_else(|| {
                    CodecError::Malformed("IGES sphere has no analytic surface family".into())
                })?,
                form: 1,
                label: "SPHERE",
                status: "00000000",
                parameters: format!(
                    "196,{},{},{},{};",
                    reference_marker(location),
                    number(*radius),
                    reference_marker(axis),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            };
            entities.push(surface);
            Ok(entities)
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            if !major_radius.is_finite()
                || !minor_radius.is_finite()
                || *minor_radius <= 0.0
                || *minor_radius >= *major_radius
            {
                return Err(CodecError::Malformed(
                    "IGES torus radii must satisfy 0 < minor < major".into(),
                ));
            }
            let (mut entities, location, axis, reference) =
                pointer_surface_support(base_index, *center, *axis, *ref_direction)?;
            let surface = Entity {
                type_code: analytic_type_code.ok_or_else(|| {
                    CodecError::Malformed("IGES torus has no analytic surface family".into())
                })?,
                form: 1,
                label: "TORUS",
                status: "00000000",
                parameters: format!(
                    "198,{},{},{},{},{};",
                    reference_marker(location),
                    reference_marker(axis),
                    number(*major_radius),
                    number(*minor_radius),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            };
            entities.push(surface);
            Ok(entities)
        }
        other => Err(CodecError::NotImplemented(format!(
            "IGES semantic writer does not encode surface geometry {other:?}"
        ))),
    }
}

fn encode_nurbs_surface(nurbs: &NurbsSurface) -> Result<Entity, CodecError> {
    let u_count = usize::try_from(nurbs.u_count)
        .map_err(|_| CodecError::Malformed("IGES surface u count overflows usize".into()))?;
    let v_count = usize::try_from(nurbs.v_count)
        .map_err(|_| CodecError::Malformed("IGES surface v count overflows usize".into()))?;
    let u_degree = usize::try_from(nurbs.u_degree)
        .map_err(|_| CodecError::Malformed("IGES surface u degree overflows usize".into()))?;
    let v_degree = usize::try_from(nurbs.v_degree)
        .map_err(|_| CodecError::Malformed("IGES surface v degree overflows usize".into()))?;
    let pole_count = u_count.checked_mul(v_count).ok_or_else(|| {
        CodecError::Malformed("IGES surface control-point count overflows".into())
    })?;
    let u_knot_count = u_count
        .checked_add(u_degree)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| CodecError::Malformed("IGES surface u knot count overflows".into()))?;
    let v_knot_count = v_count
        .checked_add(v_degree)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| CodecError::Malformed("IGES surface v knot count overflows".into()))?;
    if u_count == 0
        || v_count == 0
        || u_degree >= u_count
        || v_degree >= v_count
        || nurbs.control_points.len() != pole_count
        || nurbs.u_knots.len() != u_knot_count
        || nurbs.v_knots.len() != v_knot_count
        || nurbs.u_knots.iter().any(|value| !value.is_finite())
        || nurbs.v_knots.iter().any(|value| !value.is_finite())
        || !knots_nondecreasing(&nurbs.u_knots)
        || !knots_nondecreasing(&nurbs.v_knots)
        || nurbs.control_points.iter().any(|point| {
            [point.x, point.y, point.z]
                .iter()
                .any(|value| !value.is_finite())
        })
    {
        return Err(CodecError::Malformed(
            "IGES NURBS surface dimensions, knots, or poles are invalid".into(),
        ));
    }
    let weights = match &nurbs.weights {
        Some(weights) if weights.len() == pole_count => {
            if weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
            {
                return Err(CodecError::Malformed(
                    "IGES NURBS surface weights must be finite and positive".into(),
                ));
            }
            weights.clone()
        }
        Some(_) => {
            return Err(CodecError::Malformed(
                "IGES NURBS surface weight count does not match poles".into(),
            ));
        }
        None => alloc_filled(pole_count, 1.0, "iges NURBS surface weights")?,
    };
    let u_range = [nurbs.u_knots[u_degree], nurbs.u_knots[u_count]];
    let v_range = [nurbs.v_knots[v_degree], nurbs.v_knots[v_count]];
    if u_range[0] >= u_range[1] || v_range[0] >= v_range[1] {
        return Err(CodecError::Malformed(
            "IGES NURBS surface has an empty parameter domain".into(),
        ));
    }
    let closed_u = nurbs.u_periodic || nurbs_surface_closed_u(nurbs, u_range, v_range);
    let closed_v = nurbs.v_periodic || nurbs_surface_closed_v(nurbs, u_range, v_range);
    let mut parameters = format!(
        "128,{},{},{},{},{},{},{},{},{}",
        u_count - 1,
        v_count - 1,
        nurbs.u_degree,
        nurbs.v_degree,
        i32::from(closed_u),
        i32::from(closed_v),
        i32::from(nurbs.weights.is_none()),
        i32::from(nurbs.u_periodic),
        i32::from(nurbs.v_periodic)
    );
    for value in &nurbs.u_knots {
        parameters.push(',');
        parameters.push_str(&number(*value));
    }
    for value in &nurbs.v_knots {
        parameters.push(',');
        parameters.push_str(&number(*value));
    }
    for v in 0..v_count {
        for u in 0..u_count {
            parameters.push(',');
            parameters.push_str(&number(weights[u * v_count + v]));
        }
    }
    for v in 0..v_count {
        for u in 0..u_count {
            let point = nurbs.control_points[u * v_count + v];
            for value in [point.x, point.y, point.z] {
                parameters.push(',');
                parameters.push_str(&number(value));
            }
        }
    }
    for value in [u_range[0], u_range[1], v_range[0], v_range[1]] {
        parameters.push(',');
        parameters.push_str(&number(value));
    }
    parameters.push(';');
    Ok(Entity {
        type_code: 128,
        form: 0,
        label: "NURBS",
        status: "00000000",
        parameters: parameters.into_bytes(),
        transform: None,
    })
}

fn nurbs_surface_closed_u(nurbs: &NurbsSurface, u_range: [f64; 2], v_range: [f64; 2]) -> bool {
    [v_range[0], v_range[0].midpoint(v_range[1]), v_range[1]]
        .into_iter()
        .all(|v| {
            let Some(start) = cadmpeg_ir::eval::nurbs_surface_point(nurbs, u_range[0], v) else {
                return false;
            };
            let Some(end) = cadmpeg_ir::eval::nurbs_surface_point(nurbs, u_range[1], v) else {
                return false;
            };
            close_point(start, end)
        })
}

fn nurbs_surface_closed_v(nurbs: &NurbsSurface, u_range: [f64; 2], v_range: [f64; 2]) -> bool {
    [u_range[0], u_range[0].midpoint(u_range[1]), u_range[1]]
        .into_iter()
        .all(|u| {
            let Some(start) = cadmpeg_ir::eval::nurbs_surface_point(nurbs, u, v_range[0]) else {
                return false;
            };
            let Some(end) = cadmpeg_ir::eval::nurbs_surface_point(nurbs, u, v_range[1]) else {
                return false;
            };
            close_point(start, end)
        })
}

fn vertex_point_id(ir: &CadIr, vertex_id: &VertexId) -> Result<PointId, CodecError> {
    let vertex = ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == *vertex_id)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES edge references missing vertex {vertex_id}"
            ))
        })?;
    Ok(vertex.point.clone())
}

fn point_position(ir: &CadIr, point_id: &PointId) -> Result<Point3, CodecError> {
    ir.model
        .points
        .iter()
        .find(|point| point.id == *point_id)
        .map(|point| point.position)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES topology references missing point {point_id}"
            ))
        })
}

fn vertex_position(ir: &CadIr, vertex_id: &VertexId) -> Option<Point3> {
    let point_id = ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == *vertex_id)?
        .point
        .clone();
    ir.model
        .points
        .iter()
        .find(|point| point.id == point_id)
        .map(|point| point.position)
}

#[derive(Clone, Copy)]
struct CurveEntityRequest<'a> {
    version: crate::IgesVersion,
    curve_id: &'a CurveId,
    geometry: &'a CurveGeometry,
    span: Option<&'a CurveSpan>,
    sense: Sense,
    status: &'static str,
    reference_offset: usize,
}

fn append_curve_entity(
    entities: &mut Vec<Entity>,
    ir: &CadIr,
    request: CurveEntityRequest<'_>,
) -> Result<usize, CodecError> {
    append_curve_entity_with_reference_offset(entities, ir, request)
}

fn append_curve_entity_with_reference_offset(
    entities: &mut Vec<Entity>,
    ir: &CadIr,
    request: CurveEntityRequest<'_>,
) -> Result<usize, CodecError> {
    let mut emitter = CurveEntityEmitter {
        entities,
        ir,
        active: BTreeSet::new(),
        version: request.version,
        reference_offset: request.reference_offset,
    };
    emitter.append(
        request.curve_id,
        request.geometry,
        request.span,
        request.sense,
        request.status,
    )
}

struct CurveEntityEmitter<'a> {
    entities: &'a mut Vec<Entity>,
    ir: &'a CadIr,
    active: BTreeSet<CurveId>,
    version: crate::IgesVersion,
    reference_offset: usize,
}

impl CurveEntityEmitter<'_> {
    fn append(
        &mut self,
        curve_id: &CurveId,
        geometry: &CurveGeometry,
        span: Option<&CurveSpan>,
        sense: Sense,
        status: &'static str,
    ) -> Result<usize, CodecError> {
        if !self.active.insert(curve_id.clone()) {
            return Err(CodecError::malformed(format_args!(
                "IGES composite curve graph contains a cycle at {curve_id}"
            )));
        }
        let result = match geometry {
            CurveGeometry::Composite { segments, .. } => {
                if segments.is_empty() {
                    Err(CodecError::malformed(format_args!(
                        "IGES composite curve {curve_id} has no segments"
                    )))
                } else {
                    let children = self.append_composite_constituents(segments, sense)?;
                    push_composite_entity_with_reference_offset(
                        self.entities,
                        &children,
                        "COMPOSIT",
                        status,
                        self.reference_offset,
                    )
                }
            }
            _ => {
                let mut entity = match sense {
                    Sense::Forward => curve_entity(geometry, span, self.version)?,
                    Sense::Reversed => {
                        let span = span.ok_or_else(|| {
                            CodecError::NotImplemented(format!(
                                "IGES reversed curve {curve_id} requires a parameter range"
                            ))
                        })?;
                        oriented_curve_entity(geometry, span, Sense::Reversed, self.version)?
                    }
                };
                entity.status = status;
                let index = self.entities.len();
                self.entities.push(entity);
                Ok(index)
            }
        };
        self.active.remove(curve_id);
        result
    }

    fn append_composite_constituents(
        &mut self,
        segments: &[cadmpeg_ir::geometry::CompositeCurveSegment],
        sense: Sense,
    ) -> Result<Vec<usize>, CodecError> {
        let ordered = if sense == Sense::Forward {
            segments.iter().collect::<Vec<_>>()
        } else {
            segments.iter().rev().collect::<Vec<_>>()
        };
        let mut children = Vec::new();
        for segment in ordered {
            let child = self
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == segment.curve)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES composite curve references missing child {}",
                        segment.curve
                    ))
                })?;
            let child_geometry = flatten_curve(&child.geometry)?;
            let child_sense = match (sense, segment.same_sense) {
                (Sense::Forward, true) | (Sense::Reversed, false) => Sense::Forward,
                (Sense::Forward, false) | (Sense::Reversed, true) => Sense::Reversed,
            };
            if let CurveGeometry::Composite {
                segments: nested, ..
            } = &child_geometry
            {
                if !self.active.insert(segment.curve.clone()) {
                    return Err(CodecError::malformed(format_args!(
                        "IGES composite curve graph contains a cycle at {}",
                        segment.curve
                    )));
                }
                let nested_children = self.append_composite_constituents(nested, child_sense);
                self.active.remove(&segment.curve);
                children.extend(nested_children?);
            } else {
                let child_span = curve_reference_span_inner(
                    self.ir,
                    &segment.curve,
                    &child_geometry,
                    &mut self.active,
                )?;
                children.push(self.append(
                    &segment.curve,
                    &child_geometry,
                    Some(&child_span),
                    child_sense,
                    PHYSICALLY_DEPENDENT_STATUS,
                )?);
            }
        }
        if children.is_empty() {
            return Err(CodecError::NotImplemented(
                "IGES Type 102 composite has no writable curve constituents".into(),
            ));
        }
        Ok(children)
    }
}

fn curve_reference_span(
    ir: &CadIr,
    curve_id: &CurveId,
    geometry: &CurveGeometry,
) -> Result<CurveSpan, CodecError> {
    curve_reference_span_inner(ir, curve_id, geometry, &mut BTreeSet::new())
}

fn curve_reference_span_inner(
    ir: &CadIr,
    curve_id: &CurveId,
    geometry: &CurveGeometry,
    active: &mut BTreeSet<CurveId>,
) -> Result<CurveSpan, CodecError> {
    if !active.insert(curve_id.clone()) {
        return Err(CodecError::malformed(format_args!(
            "IGES composite curve graph contains a cycle at {curve_id}"
        )));
    }
    let result = match geometry {
        CurveGeometry::Composite { segments, .. } => {
            if segments.is_empty() {
                Err(CodecError::malformed(format_args!(
                    "IGES composite curve {curve_id} has no segments"
                )))
            } else {
                let mut child_spans = Vec::with_capacity(segments.len());
                let mut total = 0.0;
                for segment in segments {
                    let child = ir
                        .model
                        .curves
                        .iter()
                        .find(|curve| curve.id == segment.curve)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "IGES composite curve {curve_id} references missing child {}",
                                segment.curve
                            ))
                        })?;
                    let child_geometry = flatten_curve(&child.geometry)?;
                    let child_span =
                        curve_reference_span_inner(ir, &segment.curve, &child_geometry, active)?;
                    let width = child_span.range[1] - child_span.range[0];
                    if !width.is_finite() || width <= 0.0 {
                        return Err(CodecError::malformed(format_args!(
                            "IGES composite curve {curve_id} has a child with an invalid parameter span"
                        )));
                    }
                    total += width;
                    if !total.is_finite() {
                        return Err(CodecError::malformed(format_args!(
                            "IGES composite curve {curve_id} parameter span overflows"
                        )));
                    }
                    child_spans.push(child_span);
                }
                let first = &child_spans[0];
                let last = &child_spans[child_spans.len() - 1];
                let derived_start = if segments[0].same_sense {
                    first.start
                } else {
                    first.end
                };
                let derived_end = if segments[segments.len() - 1].same_sense {
                    last.end
                } else {
                    last.start
                };
                let derived_range = [0.0, total];
                let matching_edges = ir
                    .model
                    .edges
                    .iter()
                    .filter(|edge| edge.curve.as_ref() == Some(curve_id))
                    .collect::<Vec<_>>();
                if matching_edges.is_empty() {
                    Ok(CurveSpan {
                        range: derived_range,
                        start: derived_start,
                        end: derived_end,
                    })
                } else {
                    let mut selected = None;
                    let mut tolerance: f64 = 0.0;
                    for edge in matching_edges {
                        if let Some(range) = edge.param_range {
                            if !same_range(range, derived_range) {
                                return Err(CodecError::NotImplemented(format!(
                                    "IGES composite curve {curve_id} has an edge parameter range that cannot be represented by Type 102"
                                )));
                            }
                        }
                        let start = point_position(ir, &vertex_point_id(ir, &edge.start)?)?;
                        let end = point_position(ir, &vertex_point_id(ir, &edge.end)?)?;
                        let edge_tolerance = edge_topology_tolerance(ir, edge)?;
                        tolerance = tolerance.max(edge_tolerance);
                        if !close_point_with_tolerance(start, derived_start, edge_tolerance)
                            || !close_point_with_tolerance(end, derived_end, edge_tolerance)
                        {
                            return Err(CodecError::malformed(format_args!(
                                "IGES composite curve {curve_id} endpoints disagree with its child sequence"
                            )));
                        }
                        if let Some((selected_start, selected_end)) = selected {
                            if !close_point_with_tolerance(start, selected_start, tolerance)
                                || !close_point_with_tolerance(end, selected_end, tolerance)
                            {
                                return Err(CodecError::NotImplemented(format!(
                                    "IGES composite curve {curve_id} has ambiguous edge endpoints"
                                )));
                            }
                        } else {
                            selected = Some((start, end));
                        }
                    }
                    let (start, end) = selected.expect("matching composite edge is nonempty");
                    Ok(CurveSpan {
                        range: derived_range,
                        start,
                        end,
                    })
                }
            }
        }
        _ => {
            let matching_edges = ir
                .model
                .edges
                .iter()
                .filter(|edge| edge.curve.as_ref() == Some(curve_id))
                .collect::<Vec<_>>();
            if matching_edges.is_empty() {
                let range = default_range(geometry)?;
                let start = curve_point(geometry, range[0]).ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "IGES composite child curve {curve_id} has no evaluable start"
                    ))
                })?;
                let end = curve_point(geometry, range[1]).ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "IGES composite child curve {curve_id} has no evaluable end"
                    ))
                })?;
                Ok(CurveSpan { range, start, end })
            } else if matching_edges.iter().any(|edge| edge.param_range.is_none()) {
                Err(CodecError::NotImplemented(format!(
                    "IGES composite child curve {curve_id} requires a parameter range"
                )))
            } else {
                let spans = matching_edges
                    .iter()
                    .map(|edge| edge_span(ir, edge, geometry))
                    .collect::<Result<Vec<_>, _>>()?;
                let first = spans.first().expect("matching edge is nonempty");
                let tolerance = matching_edges
                    .iter()
                    .map(|edge| edge_topology_tolerance(ir, edge))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .fold(0.0, f64::max);
                if spans.iter().skip(1).any(|span| {
                    !same_range(span.range, first.range)
                        || !close_point_with_tolerance(span.start, first.start, tolerance)
                        || !close_point_with_tolerance(span.end, first.end, tolerance)
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "IGES composite child curve {curve_id} has ambiguous edge parameter ranges"
                    )));
                }
                Ok(CurveSpan {
                    range: first.range,
                    start: first.start,
                    end: first.end,
                })
            }
        }
    };
    active.remove(curve_id);
    result
}

fn mark_curve_descendants(
    ir: &CadIr,
    curve_id: &CurveId,
    consumed: &mut BTreeSet<String>,
    active: &mut BTreeSet<CurveId>,
) -> Result<(), CodecError> {
    if !active.insert(curve_id.clone()) {
        return Err(CodecError::malformed(format_args!(
            "IGES composite curve graph contains a cycle at {curve_id}"
        )));
    }
    if !consumed.insert(curve_id.as_str().to_owned()) {
        active.remove(curve_id);
        return Ok(());
    }
    let curve = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *curve_id)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES curve reference points to missing curve {curve_id}"
            ))
        })?;
    if let CurveGeometry::Composite { segments, .. } = &curve.geometry {
        for segment in segments {
            mark_curve_descendants(ir, &segment.curve, consumed, active)?;
        }
    }
    active.remove(curve_id);
    Ok(())
}

fn edge_span(ir: &CadIr, edge: &Edge, geometry: &CurveGeometry) -> Result<CurveSpan, CodecError> {
    if matches!(geometry, CurveGeometry::Composite { .. }) {
        let curve_id = edge.curve.as_ref().ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES composite edge {} has no curve reference",
                edge.id
            ))
        })?;
        return curve_reference_span(ir, curve_id, geometry);
    }
    let range = edge.param_range.ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "IGES semantic writer requires a parameter range for edge {}",
            edge.id
        ))
    })?;
    if range.iter().any(|value| !value.is_finite()) || range[0] >= range[1] {
        return Err(CodecError::malformed(format_args!(
            "IGES edge {} requires a finite non-zero parameter span",
            edge.id
        )));
    }
    let start = point_position(ir, &vertex_point_id(ir, &edge.start)?)?;
    let end = point_position(ir, &vertex_point_id(ir, &edge.end)?)?;
    ensure_finite_point(start, &format!("edge {} start", edge.id))?;
    ensure_finite_point(end, &format!("edge {} end", edge.id))?;
    if matches!(
        geometry,
        CurveGeometry::Circle { .. }
            | CurveGeometry::Ellipse { .. }
            | CurveGeometry::Parabola { .. }
            | CurveGeometry::Hyperbola { .. }
            | CurveGeometry::Nurbs(_)
            | CurveGeometry::Polyline { .. }
    ) {
        let evaluated_start = curve_point(geometry, range[0]).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES edge {} start cannot be evaluated on its curve",
                edge.id
            ))
        })?;
        let evaluated_end = curve_point(geometry, range[1]).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES edge {} end cannot be evaluated on its curve",
                edge.id
            ))
        })?;
        let tolerance = edge_topology_tolerance(ir, edge)?;
        if !close_point_with_tolerance(start, evaluated_start, tolerance)
            || !close_point_with_tolerance(end, evaluated_end, tolerance)
        {
            return Err(CodecError::malformed(format_args!(
                "IGES edge {} endpoints disagree with its curve parameter range",
                edge.id
            )));
        }
    }
    Ok(CurveSpan { range, start, end })
}

fn edge_topology_tolerance(ir: &CadIr, edge: &Edge) -> Result<f64, CodecError> {
    let mut tolerance = edge.tolerance.unwrap_or(0.0);
    for vertex_id in [&edge.start, &edge.end] {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == *vertex_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES edge {} references missing vertex {}",
                    edge.id, vertex_id
                ))
            })?;
        tolerance = tolerance.max(vertex.tolerance.unwrap_or(0.0));
    }
    Ok(tolerance)
}

fn default_range(geometry: &CurveGeometry) -> Result<[f64; 2], CodecError> {
    match geometry {
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => Ok([0.0, TAU]),
        CurveGeometry::Nurbs(nurbs) => nurbs_domain(nurbs),
        CurveGeometry::Polyline {
            points, parameters, ..
        } => {
            if points.len() < 2 {
                return Err(CodecError::NotImplemented(
                    "IGES semantic writer requires at least two polyline points".into(),
                ));
            }
            let values = polyline_parameters(points.len(), parameters.as_deref())?;
            Ok([values[0], *values.last().expect("polyline has points")])
        }
        CurveGeometry::Line { .. }
        | CurveGeometry::Parabola { .. }
        | CurveGeometry::Hyperbola { .. }
        | CurveGeometry::Degenerate { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Unknown { .. } => Err(CodecError::NotImplemented(
            "IGES semantic writer requires a finite curve parameter range".into(),
        )),
        CurveGeometry::Transformed { .. } => Err(CodecError::Malformed(
            "IGES transformed curve was not flattened before encoding".into(),
        )),
    }
}

fn curve_entity(
    geometry: &CurveGeometry,
    span: Option<&CurveSpan>,
    version: crate::IgesVersion,
) -> Result<Entity, CodecError> {
    let range = span.map_or_else(|| default_range(geometry), |span| Ok(span.range))?;
    if range.iter().any(|value| !value.is_finite()) || range[0] > range[1] {
        return Err(CodecError::Malformed(
            "IGES curve parameter range is invalid".into(),
        ));
    }
    match geometry {
        CurveGeometry::Line { .. } => {
            let span = span.ok_or_else(|| {
                CodecError::NotImplemented(
                    "IGES semantic writer cannot bound an unreferenced line".into(),
                )
            })?;
            Ok(Entity {
                type_code: 110,
                form: 0,
                label: "LINE",
                status: "00000000",
                parameters: format!(
                    "110,{},{},{},{},{},{};",
                    number(span.start.x),
                    number(span.start.y),
                    number(span.start.z),
                    number(span.end.x),
                    number(span.end.y),
                    number(span.end.z)
                )
                .into_bytes(),
                transform: None,
            })
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let (axis, reference) = orthonormal_pair(*axis, *ref_direction, "circle basis")?;
            if !radius.is_finite() || *radius <= 0.0 {
                return Err(CodecError::Malformed(
                    "IGES circle radius must be positive and finite".into(),
                ));
            }
            let y_axis = axis.cross(reference);
            validate_arc_sweep(range)?;
            let start_xy = [radius * range[0].cos(), radius * range[0].sin()];
            let end_xy = if is_full_arc(span) {
                start_xy
            } else {
                [radius * range[1].cos(), radius * range[1].sin()]
            };
            Ok(Entity {
                type_code: 100,
                form: 0,
                label: "ARC",
                status: "00000000",
                parameters: format!(
                    "100,0,0,0,{},{},{},{};",
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(*center, reference, y_axis, axis)?),
            })
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let (axis, major) = orthonormal_pair(*axis, *major_direction, "ellipse basis")?;
            if !major_radius.is_finite()
                || !minor_radius.is_finite()
                || *major_radius <= 0.0
                || *minor_radius <= 0.0
            {
                return Err(CodecError::Malformed(
                    "IGES ellipse basis or radii are invalid".into(),
                ));
            }
            let y_axis = axis.cross(major);
            validate_arc_sweep(range)?;
            let start_xy = [major_radius * range[0].cos(), minor_radius * range[0].sin()];
            let end_xy = if is_full_arc(span) {
                start_xy
            } else {
                [major_radius * range[1].cos(), minor_radius * range[1].sin()]
            };
            // V5.0 identifies the coefficient-defined ellipse as Form 1.  The
            // Parameter Data is identical to the compatibility Form 0 used by
            // V4.0 and V5.1 through V5.3.
            let form = i64::from(version == crate::IgesVersion::V5_0);
            Ok(Entity {
                type_code: 104,
                form,
                label: "CONIC",
                status: "00000000",
                parameters: format!(
                    "104,{},0,{},0,0,-1,0,{},{},{},{};",
                    number(1.0 / (major_radius * major_radius)),
                    number(1.0 / (minor_radius * minor_radius)),
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(*center, major, y_axis, axis)?),
            })
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => {
            if range[0] == range[1] || !focal_distance.is_finite() || *focal_distance <= 0.0 {
                return Err(CodecError::Malformed(
                    "IGES parabola requires a finite non-zero parameter span".into(),
                ));
            }
            let (axis, major) = orthonormal_pair(*axis, *major_direction, "parabola basis")?;
            let x_axis = major.cross(axis);
            let start_xy = parabola_point(*focal_distance, range[0])?;
            let end_xy = parabola_point(*focal_distance, range[1])?;
            Ok(Entity {
                type_code: 104,
                form: 3,
                label: "CONIC",
                status: "00000000",
                parameters: format!(
                    "104,1,0,0,0,{},0,0,{},{},{},{};",
                    number(-4.0 * focal_distance),
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(*vertex, x_axis, major, axis)?),
            })
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            if range[0] == range[1]
                || !major_radius.is_finite()
                || !minor_radius.is_finite()
                || *major_radius <= 0.0
                || *minor_radius <= 0.0
            {
                return Err(CodecError::Malformed(
                    "IGES hyperbola requires positive radii and a finite span".into(),
                ));
            }
            let (axis, major) = orthonormal_pair(*axis, *major_direction, "hyperbola basis")?;
            let y_axis = axis.cross(major);
            let start_xy = hyperbola_point(*major_radius, *minor_radius, range[0])?;
            let end_xy = hyperbola_point(*major_radius, *minor_radius, range[1])?;
            Ok(Entity {
                type_code: 104,
                form: 2,
                label: "CONIC",
                status: "00000000",
                parameters: format!(
                    "104,{},0,{},0,0,-1,0,{},{},{},{};",
                    number(1.0 / (major_radius * major_radius)),
                    number(-1.0 / (minor_radius * minor_radius)),
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(*center, major, y_axis, axis)?),
            })
        }
        CurveGeometry::Nurbs(nurbs) => encode_nurbs(nurbs, range, "NURBS"),
        CurveGeometry::Polyline {
            points, parameters, ..
        } => {
            let values = polyline_parameters(points.len(), parameters.as_deref())?;
            let nurbs = NurbsCurve {
                degree: 1,
                knots: polyline_knots(&values),
                control_points: points.clone(),
                weights: None,
                periodic: false,
            };
            encode_nurbs(&nurbs, range, "POLYLINE")
        }
        CurveGeometry::Degenerate { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Unknown { .. }
        | CurveGeometry::Transformed { .. } => Err(CodecError::NotImplemented(
            "IGES semantic writer does not encode this curve geometry".into(),
        )),
    }
}

fn encode_nurbs(
    nurbs: &NurbsCurve,
    range: [f64; 2],
    label: &'static str,
) -> Result<Entity, CodecError> {
    let control_count = nurbs.control_points.len();
    let degree = usize::try_from(nurbs.degree)
        .map_err(|_| CodecError::Malformed("IGES NURBS degree overflows usize".into()))?;
    if control_count == 0
        || degree >= control_count
        || nurbs.knots.len() != control_count + degree + 1
        || range[0] > range[1]
        || range.iter().any(|value| !value.is_finite())
        || nurbs.knots.iter().any(|value| !value.is_finite())
        || !knots_nondecreasing(&nurbs.knots)
    {
        return Err(CodecError::Malformed(
            "IGES NURBS degree, knot vector, or parameter range is invalid".into(),
        ));
    }
    let domain = [nurbs.knots[degree], nurbs.knots[control_count]];
    if range[0] < domain[0] || range[1] > domain[1] {
        return Err(CodecError::Malformed(
            "IGES NURBS parameter range lies outside its knot domain".into(),
        ));
    }
    if nurbs
        .control_points
        .iter()
        .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
    {
        return Err(CodecError::Malformed(
            "IGES NURBS control point is non-finite".into(),
        ));
    }
    let weights = match &nurbs.weights {
        Some(weights) if weights.len() == control_count => {
            if weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
            {
                return Err(CodecError::Malformed(
                    "IGES NURBS weights must be finite and positive".into(),
                ));
            }
            weights.clone()
        }
        Some(_) => {
            return Err(CodecError::Malformed(
                "IGES NURBS weight count does not match control points".into(),
            ));
        }
        None => alloc_filled(control_count, 1.0, "iges NURBS weights")?,
    };
    let polynomial = weights
        .first()
        .is_some_and(|first| weights.iter().all(|weight| weight == first));
    let plane_normal = nurbs_plane_normal(&nurbs.control_points);
    let planar = plane_normal.is_some();
    let closed = nurbs_is_closed(nurbs, &weights, domain);
    let k = control_count - 1;
    let mut parameters = format!(
        "126,{k},{},{},{},{},{}",
        nurbs.degree,
        i32::from(planar),
        i32::from(closed),
        i32::from(polynomial),
        i32::from(nurbs.periodic)
    );
    for value in &nurbs.knots {
        parameters.push(',');
        parameters.push_str(&number(*value));
    }
    for weight in weights {
        parameters.push(',');
        parameters.push_str(&number(weight));
    }
    for point in &nurbs.control_points {
        for value in [point.x, point.y, point.z] {
            parameters.push(',');
            parameters.push_str(&number(value));
        }
    }
    parameters.push(',');
    parameters.push_str(&number(range[0]));
    parameters.push(',');
    parameters.push_str(&number(range[1]));
    let normal = plane_normal.unwrap_or(Vector3::new(0.0, 0.0, 0.0));
    for value in [normal.x, normal.y, normal.z] {
        parameters.push(',');
        parameters.push_str(&unit_normal_number(value));
    }
    parameters.push(';');
    let status = if label == "PCURVE" {
        PARAMETER_CURVE_STATUS
    } else {
        "00000000"
    };
    Ok(Entity {
        type_code: 126,
        form: 0,
        label,
        status,
        parameters: parameters.into_bytes(),
        transform: None,
    })
}

fn nurbs_plane_normal(points: &[Point3]) -> Option<Vector3> {
    let origin = points.first().copied()?;
    let distances = points
        .iter()
        .map(|point| point.distance(origin))
        .collect::<Vec<_>>();
    if distances.iter().any(|distance| !distance.is_finite()) {
        return None;
    }
    let scale = distances.into_iter().fold(1.0, f64::max);
    let tolerance = NURBS_PLANE_COMPUTATION_TOLERANCE * scale;
    let mut first_direction = None;
    for point in points.iter().skip(1) {
        let direction = point.vector_from(origin);
        let length = direction.norm();
        if !length.is_finite() {
            return None;
        }
        if length > tolerance {
            first_direction = Some(direction);
            break;
        }
    }
    let first_direction = first_direction?;
    let normal_threshold = NURBS_PLANE_COMPUTATION_TOLERANCE * scale * first_direction.norm();
    let mut normal = None;
    for point in points.iter().skip(1) {
        let candidate = first_direction.cross(point.vector_from(origin));
        let length = candidate.norm();
        if !length.is_finite() {
            return None;
        }
        if length > normal_threshold {
            normal = Some(candidate);
            break;
        }
    }
    let normal = normal?;
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= f64::EPSILON {
        return None;
    }
    let unit_normal = normal.scale(1.0 / normal_length);
    points
        .iter()
        .all(|point| unit_normal.dot(point.vector_from(origin)).abs() <= tolerance)
        .then_some(unit_normal)
}

fn nurbs_is_closed(nurbs: &NurbsCurve, weights: &[f64], domain: [f64; 2]) -> bool {
    let Some(start) = cadmpeg_ir::eval::nurbs_curve_point(
        nurbs.degree,
        &nurbs.knots,
        &nurbs.control_points,
        Some(weights),
        domain[0],
    ) else {
        return false;
    };
    let Some(end) = cadmpeg_ir::eval::nurbs_curve_point(
        nurbs.degree,
        &nurbs.knots,
        &nurbs.control_points,
        Some(weights),
        domain[1],
    ) else {
        return false;
    };
    let scale = nurbs
        .control_points
        .iter()
        .map(|point| point.distance(start))
        .filter(|distance| distance.is_finite())
        .fold(1.0, f64::max);
    start.distance(end) <= NURBS_CLOSEDNESS_TOLERANCE * scale
}

fn flatten_curve(geometry: &CurveGeometry) -> Result<CurveGeometry, CodecError> {
    match geometry {
        CurveGeometry::Transformed { basis, transform } => {
            if !transform.is_proper_rigid() {
                return Err(CodecError::NotImplemented(
                    "IGES semantic writer only applies proper-rigid curve transforms".into(),
                ));
            }
            let basis = flatten_curve(basis)?;
            apply_rigid_transform(basis, *transform)
        }
        _ => Ok(geometry.clone()),
    }
}

fn apply_rigid_transform(
    geometry: CurveGeometry,
    transform: cadmpeg_ir::transform::Transform,
) -> Result<CurveGeometry, CodecError> {
    let point = |value: Point3| transform.apply_point(value);
    let vector = |value: Vector3, label: &str| unit(transform.apply_vector(value), label);
    Ok(match geometry {
        CurveGeometry::Line { origin, direction } => CurveGeometry::Line {
            origin: point(origin),
            direction: vector(direction, "transformed line direction")?,
        },
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => CurveGeometry::Circle {
            center: point(center),
            axis: vector(axis, "transformed circle axis")?,
            ref_direction: vector(ref_direction, "transformed circle reference")?,
            radius,
        },
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => CurveGeometry::Ellipse {
            center: point(center),
            axis: vector(axis, "transformed ellipse axis")?,
            major_direction: vector(major_direction, "transformed ellipse major")?,
            major_radius,
            minor_radius,
        },
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => CurveGeometry::Parabola {
            vertex: point(vertex),
            axis: vector(axis, "transformed parabola axis")?,
            major_direction: vector(major_direction, "transformed parabola major")?,
            focal_distance,
        },
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => CurveGeometry::Hyperbola {
            center: point(center),
            axis: vector(axis, "transformed hyperbola axis")?,
            major_direction: vector(major_direction, "transformed hyperbola major")?,
            major_radius,
            minor_radius,
        },
        CurveGeometry::Degenerate { point: value } => CurveGeometry::Degenerate {
            point: point(value),
        },
        CurveGeometry::Nurbs(mut nurbs) => {
            nurbs.control_points = nurbs.control_points.into_iter().map(point).collect();
            CurveGeometry::Nurbs(nurbs)
        }
        CurveGeometry::Polyline {
            points,
            parameters,
            chordal_deflection,
        } => CurveGeometry::Polyline {
            points: points.into_iter().map(point).collect(),
            parameters,
            chordal_deflection,
        },
        other => {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer cannot flatten curve geometry {other:?}"
            )))
        }
    })
}

fn unit(vector: Vector3, label: &str) -> Result<Vector3, CodecError> {
    let norm = vector.norm();
    if !norm.is_finite() || norm <= f64::EPSILON {
        return Err(CodecError::malformed(format_args!(
            "IGES {label} is degenerate"
        )));
    }
    Ok(vector.scale(1.0 / norm))
}

fn orthonormal_pair(
    primary: Vector3,
    reference: Vector3,
    label: &str,
) -> Result<(Vector3, Vector3), CodecError> {
    let primary = unit(primary, label)?;
    let reference = unit(reference, label)?;
    let residual = primary.dot(reference);
    if residual.abs() > FRAME_REPAIR_DOT_LIMIT {
        return Err(CodecError::malformed(format_args!(
            "IGES {label} exceeds the frame repair bound"
        )));
    }
    let reference = unit(reference - primary.scale(residual), label)?;
    Ok((primary, reference))
}

fn placement(
    origin: Point3,
    x_axis: Vector3,
    y_axis: Vector3,
    z_axis: Vector3,
) -> Result<Placement, CodecError> {
    ensure_finite_point(origin, "placement origin")?;
    let (x_axis, y_axis) = orthonormal_pair(x_axis, y_axis, "placement x/y axes")?;
    let supplied_z = unit(z_axis, "placement z axis")?;
    let z_axis = unit(x_axis.cross(y_axis), "placement derived z axis")?;
    if z_axis.dot(supplied_z) <= 0.0 || z_axis.cross(supplied_z).norm() > FRAME_REPAIR_DOT_LIMIT {
        return Err(CodecError::Malformed(
            "IGES placement z axis exceeds the frame repair bound".into(),
        ));
    }
    Ok(Placement {
        rows: [
            [x_axis.x, y_axis.x, z_axis.x, origin.x],
            [x_axis.y, y_axis.y, z_axis.y, origin.y],
            [x_axis.z, y_axis.z, z_axis.z, origin.z],
        ],
    })
}

fn validate_arc_sweep(range: [f64; 2]) -> Result<(), CodecError> {
    let sweep = range[1] - range[0];
    if !(0.0..=TAU + ANGULAR_TOLERANCE).contains(&sweep) || sweep == 0.0 {
        return Err(CodecError::NotImplemented(
            "IGES conic writer requires a non-zero ordered span no larger than one revolution"
                .into(),
        ));
    }
    Ok(())
}

fn is_full_arc(span: Option<&CurveSpan>) -> bool {
    span.is_none_or(|span| span.start == span.end)
}

fn parabola_point(focal_distance: f64, parameter: f64) -> Result<[f64; 2], CodecError> {
    let point = [
        -2.0 * focal_distance * parameter,
        focal_distance * parameter * parameter,
    ];
    point
        .iter()
        .all(|value| value.is_finite())
        .then_some(point)
        .ok_or_else(|| CodecError::Malformed("IGES parabola endpoint is non-finite".into()))
}

fn hyperbola_point(
    major_radius: f64,
    minor_radius: f64,
    parameter: f64,
) -> Result<[f64; 2], CodecError> {
    let point = [
        major_radius * parameter.cosh(),
        minor_radius * parameter.sinh(),
    ];
    point
        .iter()
        .all(|value| value.is_finite())
        .then_some(point)
        .ok_or_else(|| CodecError::Malformed("IGES hyperbola endpoint is non-finite".into()))
}

fn nurbs_domain(nurbs: &NurbsCurve) -> Result<[f64; 2], CodecError> {
    let degree = usize::try_from(nurbs.degree)
        .map_err(|_| CodecError::Malformed("IGES NURBS degree overflows usize".into()))?;
    let end = nurbs.control_points.len();
    if nurbs.knots.len() <= end || degree >= nurbs.knots.len() {
        return Err(CodecError::Malformed(
            "IGES NURBS knot vector cannot provide a domain".into(),
        ));
    }
    Ok([nurbs.knots[degree], nurbs.knots[end]])
}

fn polyline_parameters(count: usize, parameters: Option<&[f64]>) -> Result<Vec<f64>, CodecError> {
    if count < 2 {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer requires at least two polyline points".into(),
        ));
    }
    let values = parameters.map_or_else(
        || (0..count).map(|value| value as f64).collect(),
        <[f64]>::to_vec,
    );
    if values.len() != count
        || values.iter().any(|value| !value.is_finite())
        || values.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(CodecError::Malformed(
            "IGES polyline parameters must be finite and strictly increasing".into(),
        ));
    }
    Ok(values)
}

fn polyline_knots(parameters: &[f64]) -> Vec<f64> {
    let mut knots = Vec::with_capacity(parameters.len() + 2);
    knots.extend([parameters[0], parameters[0]]);
    knots.extend_from_slice(&parameters[1..parameters.len() - 1]);
    knots.extend([*parameters.last().expect("polyline has points"); 2]);
    knots
}

fn close_point(left: Point3, right: Point3) -> bool {
    close_point_with_tolerance(left, right, 0.0)
}

fn close_point_with_tolerance(left: Point3, right: Point3, explicit_tolerance: f64) -> bool {
    let scale = left
        .x
        .abs()
        .max(left.y.abs())
        .max(left.z.abs())
        .max(right.x.abs())
        .max(right.y.abs())
        .max(right.z.abs())
        .max(1.0);
    let tolerance = (scale * WRITER_ENDPOINT_RELATIVE_TOLERANCE).max(explicit_tolerance);
    (left.x - right.x).abs() <= tolerance
        && (left.y - right.y).abs() <= tolerance
        && (left.z - right.z).abs() <= tolerance
}

fn ensure_finite_point(point: Point3, label: &str) -> Result<(), CodecError> {
    if [point.x, point.y, point.z]
        .iter()
        .all(|value| value.is_finite())
    {
        Ok(())
    } else {
        Err(CodecError::malformed(format_args!(
            "IGES point {label} has non-finite coordinates"
        )))
    }
}

#[derive(Clone)]
struct Entity {
    type_code: u32,
    form: i64,
    label: &'static str,
    status: &'static str,
    parameters: Vec<u8>,
    transform: Option<Placement>,
}

fn encode_file(
    entities: &[Entity],
    version: crate::IgesVersion,
    minimum_resolution: f64,
) -> Result<Vec<u8>, CodecError> {
    let generation_timestamp = generation_timestamp(SystemTime::now(), version)?;
    let maximum_coordinate = generated_maximum_coordinate(entities);
    let global = generated_global(
        version,
        &generation_timestamp,
        minimum_resolution,
        maximum_coordinate,
    );
    let global_cards = crate::global::layout_global_cards(&global)?;
    let global_count = global_cards.len();
    let mut expanded = Vec::with_capacity(entities.len() * 2);
    for entity in entities {
        if let Some(placement) = entity.transform {
            let transform_parameters = placement
                .rows
                .iter()
                .flatten()
                .map(|value| number(*value))
                .collect::<Vec<_>>()
                .join(",");
            expanded.push((
                Entity {
                    type_code: 124,
                    form: 0,
                    label: "XFORM",
                    status: "00000000",
                    parameters: format!("124,{transform_parameters};").into_bytes(),
                    transform: None,
                },
                0_u32,
            ));
            let transform_sequence = u32::try_from(expanded.len())
                .ok()
                .and_then(|value| value.checked_mul(2).and_then(|value| value.checked_sub(1)))
                .ok_or_else(|| {
                    CodecError::Malformed("IGES transformation sequence overflows".into())
                })?;
            let mut entity = entity.clone();
            entity.transform = None;
            expanded.push((entity, transform_sequence));
        } else {
            expanded.push((entity.clone(), 0));
        }
    }
    let mut parameter_sequence = 1_u32;
    let mut directory = Vec::with_capacity(expanded.len() * 2);
    let mut parameters = Vec::new();
    for (index, (entity, transform_sequence)) in expanded.iter().enumerate() {
        let directory_sequence = u32::try_from(index)
            .ok()
            .and_then(|value| value.checked_mul(2))
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| CodecError::Malformed("IGES directory sequence overflows".into()))?;
        let fragments = crate::parameter::layout_parameter_cards(&entity.parameters)?;
        let parameter_count = fragments.len();
        let parameter_count = u32::try_from(parameter_count)
            .map_err(|_| CodecError::Malformed("IGES parameter count overflows".into()))?;
        directory.push(directory_card(
            [
                entity.type_code.to_string(),
                parameter_sequence.to_string(),
                "0".into(),
                generated_line_font(version, entity.type_code, entity.form).to_string(),
                "0".into(),
                "0".into(),
                transform_sequence.to_string(),
                "0".into(),
                entity.status.into(),
            ],
            directory_sequence,
        )?);
        directory.push(directory_card(
            [
                entity.type_code.to_string(),
                "0".into(),
                "0".into(),
                parameter_count.to_string(),
                entity.form.to_string(),
                String::new(),
                String::new(),
                entity.label.to_owned(),
                "0".into(),
            ],
            directory_sequence + 1,
        )?);
        for chunk in fragments {
            parameters.push(parameter_card(
                &chunk,
                directory_sequence,
                parameter_sequence,
            )?);
            parameter_sequence = parameter_sequence
                .checked_add(1)
                .ok_or_else(|| CodecError::Malformed("IGES parameter sequence overflows".into()))?;
        }
    }
    let mut bytes = Vec::new();
    bytes.extend(card(b"Generated by cadmpeg", b'S', 1)?);
    for (index, chunk) in global_cards.iter().enumerate() {
        bytes.extend(card(
            chunk,
            b'G',
            u32::try_from(index + 1).unwrap_or(u32::MAX),
        )?);
    }
    for card_bytes in directory {
        bytes.extend(card_bytes);
    }
    for card_bytes in parameters {
        bytes.extend(card_bytes);
    }
    let directory_count = expanded
        .len()
        .checked_mul(2)
        .ok_or_else(|| CodecError::Malformed("IGES directory count overflows".into()))?;
    let directory_count = u32::try_from(directory_count)
        .map_err(|_| CodecError::Malformed("IGES directory count overflows".into()))?;
    let parameter_count = parameter_sequence - 1;
    let terminate = format!(
        "S{start_count:07}G{global_count:07}D{directory_count:07}P{parameter_count:07}",
        start_count = 1
    );
    bytes.extend(card(terminate.as_bytes(), b'T', 1)?);
    Ok(bytes)
}

fn generated_line_font(version: crate::IgesVersion, entity_type: u32, form: i64) -> i64 {
    if version != crate::IgesVersion::V4_0 {
        return 0;
    }
    match entity_type {
        106 => i64::from(!matches!(form, 1..=3)),
        116 | 124 => 0,
        100 | 102 | 104 | 108 | 110 | 112 | 114 | 118 | 120 | 122 | 126 | 128 | 130 | 140 | 142
        | 144 => 1,
        _ => 0,
    }
}

fn global_hollerith(value: &str) -> String {
    debug_assert!(value.is_ascii());
    format!("{}H{value}", value.len())
}

fn generated_global(
    version: crate::IgesVersion,
    generation_timestamp: &str,
    minimum_resolution: f64,
    maximum_coordinate: f64,
) -> Vec<u8> {
    let mut fields = vec![
        "1H,".to_owned(),
        "1H;".to_owned(),
        global_hollerith(WRITER_SENDER_PRODUCT),
        global_hollerith(WRITER_NATIVE_FILE_NAME),
        global_hollerith(WRITER_NATIVE_SYSTEM_ID),
        global_hollerith(WRITER_PREPROCESSOR_VERSION),
        WRITER_INTEGER_REPRESENTATION_BITS.to_string(),
        WRITER_SINGLE_PRECISION_MAGNITUDE.to_string(),
        WRITER_SINGLE_PRECISION_SIGNIFICANCE.to_string(),
        WRITER_DOUBLE_PRECISION_MAGNITUDE.to_string(),
        WRITER_DOUBLE_PRECISION_SIGNIFICANCE.to_string(),
        global_hollerith(match version {
            crate::IgesVersion::V4_0 => WRITER_SENDER_PRODUCT,
            crate::IgesVersion::V5_0
            | crate::IgesVersion::V5_1
            | crate::IgesVersion::V5_2
            | crate::IgesVersion::V5_3 => "",
        }),
        WRITER_MODEL_SPACE_SCALE.to_owned(),
        WRITER_UNITS_FLAG.to_string(),
        global_hollerith(WRITER_UNITS_NAME),
        WRITER_MAXIMUM_LINE_WEIGHT_GRADATIONS.to_string(),
        WRITER_MAXIMUM_LINE_WIDTH.to_owned(),
        global_hollerith(generation_timestamp),
        number(minimum_resolution),
        number(maximum_coordinate),
        global_hollerith(WRITER_AUTHOR_NAME),
        global_hollerith(WRITER_AUTHOR_ORGANIZATION),
        version.global_flag().to_string(),
        WRITER_DRAFTING_STANDARD_FLAG.to_string(),
    ];
    match version {
        crate::IgesVersion::V4_0 => {}
        crate::IgesVersion::V5_0 => fields.push(global_hollerith("")),
        crate::IgesVersion::V5_1 | crate::IgesVersion::V5_2 | crate::IgesVersion::V5_3 => {
            fields.push(global_hollerith(""));
            fields.push(global_hollerith(""));
        }
    }
    let mut global = fields.join(",");
    global.push(';');
    global.into_bytes()
}

fn generated_maximum_coordinate(entities: &[Entity]) -> f64 {
    entities
        .iter()
        .try_fold(0.0_f64, |bound, entity| {
            generated_entity_coordinate_bound(entity).map(|value| bound.max(value))
        })
        .unwrap_or(0.0)
}

fn generated_entity_coordinate_bound(entity: &Entity) -> Option<f64> {
    if entity.transform.is_some() {
        return None;
    }
    let values = entity
        .parameters
        .split(|byte| matches!(byte, b',' | b';'))
        .filter(|token| !token.is_empty())
        .map(|token| {
            std::str::from_utf8(token)
                .ok()?
                .replace(['D', 'd'], "E")
                .parse::<f64>()
                .ok()
        })
        .collect::<Option<Vec<_>>>()?;
    let coordinates: &[f64] = match entity.type_code {
        110 => values.get(1..=6)?,
        116 => values.get(1..=3)?,
        502 => values.get(2..)?,
        123 | 141 | 142 | 143 | 144 | 186 | 190 | 192 | 194 | 196 | 198 | 504 | 508 | 510 | 514 => {
            &[]
        }
        _ => return None,
    };
    coordinates
        .iter()
        .map(|value| value.abs())
        .filter(|value| value.is_finite())
        .max_by(f64::total_cmp)
        .or(Some(0.0))
}

fn generation_timestamp(
    now: SystemTime,
    version: crate::IgesVersion,
) -> Result<String, CodecError> {
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CodecError::Malformed("IGES generation time precedes 1970".into()))?
        .as_secs();
    let days = i64::try_from(seconds / 86_400)
        .map_err(|_| CodecError::Malformed("IGES generation time is out of range".into()))?;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_date_from_unix_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = seconds_of_day % 3_600 / 60;
    let second = seconds_of_day % 60;
    if !(0..=9999).contains(&year) {
        return Err(CodecError::Malformed(
            "IGES generation year is outside the four-digit timestamp range".into(),
        ));
    }
    let year = match version {
        crate::IgesVersion::V4_0 | crate::IgesVersion::V5_0 => year % 100,
        crate::IgesVersion::V5_1 | crate::IgesVersion::V5_2 | crate::IgesVersion::V5_3 => year,
    };
    let width = match version {
        crate::IgesVersion::V4_0 | crate::IgesVersion::V5_0 => 2,
        crate::IgesVersion::V5_1 | crate::IgesVersion::V5_2 | crate::IgesVersion::V5_3 => 4,
    };
    Ok(format!(
        "{year:0width$}{month:02}{day:02}.{hour:02}{minute:02}{second:02}"
    ))
}

fn civil_date_from_unix_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn directory_card(fields: [String; 9], sequence: u32) -> Result<Vec<u8>, CodecError> {
    let mut payload = Vec::with_capacity(72);
    for field in fields {
        if field.len() > 8 {
            return Err(CodecError::malformed(format_args!(
                "IGES Directory field is wider than eight bytes: {field}"
            )));
        }
        payload.extend_from_slice(format!("{field:>8}").as_bytes());
    }
    card(&payload, b'D', sequence)
}

fn parameter_card(
    data: &[u8],
    directory_sequence: u32,
    sequence: u32,
) -> Result<Vec<u8>, CodecError> {
    if data.len() > 64 {
        return Err(CodecError::Malformed(
            "IGES Parameter Data payload exceeds 64 bytes".into(),
        ));
    }
    let mut payload = vec![b' '; 72];
    payload[..data.len()].copy_from_slice(data);
    let pointer = format!("{directory_sequence:>8}");
    payload[64..].copy_from_slice(pointer.as_bytes());
    card(&payload, b'P', sequence)
}

fn card(data: &[u8], section: u8, sequence: u32) -> Result<Vec<u8>, CodecError> {
    let width = 72;
    if data.len() > width {
        return Err(CodecError::Malformed(
            "IGES card payload exceeds 72 bytes".into(),
        ));
    }
    let mut payload = vec![b' '; 80];
    payload[..data.len()].copy_from_slice(data);
    payload[72] = section;
    let sequence = format!("{sequence:>7}");
    payload[73..].copy_from_slice(sequence.as_bytes());
    payload.push(b'\n');
    Ok(payload)
}

fn number(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        format!("{value:.16e}").replace('e', "D")
    }
}

fn unit_normal_number(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        format!("{value:.16e}").replace('e', "D")
    }
}

#[cfg(test)]
mod tests;
