// SPDX-License-Identifier: Apache-2.0
//! Bounded IGES Fixed ASCII writing.
//!
//! An unchanged decode with a verified document baseline replays its retained
//! source image byte for byte. Otherwise the semantic writer emits the current
//! supported neutral profile and refuses unsupported models or native records.

use crate::entities::curve_conversion::ANGULAR_TOLERANCE;
use crate::entities::{affine_parameter_map, line_directrix};
use crate::loss::IgesLossCode;
use cadmpeg_core::decode::alloc_filled;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{ExportBody, WritePath};
use cadmpeg_ir::eval::{curve_point, model_surface_point, pcurve_uv, EvaluationFailure};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsError, NurbsSurface},
    pcurve::{Pcurve, PcurveGeometry},
    sampled::{GeometryLayoutError, PolylineCurve},
    CurveGeometry, ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, PointId, ShellId, SurfaceId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::native::NativeNamespace;
use cadmpeg_ir::report::{
    export::{CensusBasis, EntityCensus},
    loss::LossNote,
};
use cadmpeg_ir::scalar::{FiniteReal, Length};
use cadmpeg_ir::topology::{
    BodyKind, Color, Edge, IncreasingParameterInterval, Loop, LoopBoundaryRole, PcurveUse, Region,
    Sense,
};
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;
use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

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
const NURBS_CLOSEDNESS_TOLERANCE: f64 = EPS_WRITE_DEGENERATE;
// Roundoff guard for geometric plane classification. This is not serialized
// as an IGES tolerance and never supplies a normal for a non-unique plane.
const NURBS_PLANE_COMPUTATION_TOLERANCE: f64 = 64.0 * f64::EPSILON;
const WRITER_ENDPOINT_RELATIVE_TOLERANCE: f64 = EPS_WRITE_POSITION;
#[derive(Clone, Copy)]
enum EntityStatus {
    Independent,
    Definition,
    PhysicallyDependent,
    PhysicallyDependentEdgeList,
    ParameterCurve,
}

impl EntityStatus {
    const fn as_field(self) -> &'static str {
        match self {
            Self::Independent => "00000000",
            Self::Definition => "00000200",
            Self::PhysicallyDependent => "00010000",
            Self::PhysicallyDependentEdgeList => "00010001",
            Self::ParameterCurve => "00010500",
        }
    }
}

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

/// States that `value` holds ASCII bytes only.
const fn is_ascii_text(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii() {
            return false;
        }
        index += 1;
    }
    true
}

/// Every writer global string is ASCII. `global_hollerith` states a byte count,
/// and a Hollerith count states characters, so the two agree only for ASCII.
const _: () = assert!(
    is_ascii_text(WRITER_SENDER_PRODUCT)
        && is_ascii_text(WRITER_NATIVE_FILE_NAME)
        && is_ascii_text(WRITER_NATIVE_SYSTEM_ID)
        && is_ascii_text(WRITER_PREPROCESSOR_VERSION)
        && is_ascii_text(WRITER_UNITS_NAME)
        && is_ascii_text(WRITER_AUTHOR_NAME)
        && is_ascii_text(WRITER_AUTHOR_ORGANIZATION)
);

const WRITER_ENTITY_TYPES: &[u32] = &[
    100, 102, 104, 108, 110, 116, 120, 122, 123, 124, 126, 128, 141, 142, 143, 144, 186, 190, 192,
    194, 196, 198, 314, 406, 502, 504, 508, 510, 514,
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
            counts: cadmpeg_ir::CensusKey::count_map(counts),
        },
        write_path,
        losses,
        notes: vec![note.into()],
    }
}

fn counts_for_ir(ir: &CadIr) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    if let Some(namespace) = ir.native.namespace("iges") {
        if let Some(records) = namespace.arenas().get("entities") {
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

struct BodyPresentation {
    label: Option<String>,
    name: Option<String>,
    color: BodyColor,
    visible: Option<bool>,
}

#[derive(Clone, Copy)]
enum BodyColor {
    Standard(i64),
    Custom(Color),
    Definition(usize),
}

fn body_presentation(
    body: &cadmpeg_ir::topology::Body,
    losses: &mut Vec<LossNote>,
) -> BodyPresentation {
    let name = body.name.as_deref().and_then(|name| {
        if !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        {
            Some(name.to_owned())
        } else {
            losses.push(IgesLossCode::WriterBodyNameNotRepresented.note(format!(
                "IGES body {} name {:?} cannot be encoded as a Type 406 Form 15 name",
                body.id, name
            )));
            None
        }
    });
    let label = body.name.as_deref().and_then(|name| {
        if !name.is_empty()
            && name.len() <= 8
            && name.trim() == name
            && name
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        {
            Some(name.to_owned())
        } else {
            None
        }
    });
    let color = body.color.map_or(BodyColor::Standard(0), |color| {
        let rgb = (color.r(), color.g(), color.b());
        let number = match rgb {
            (0.0, 0.0, 0.0) => 1,
            (1.0, 0.0, 0.0) => 2,
            (0.0, 1.0, 0.0) => 3,
            (0.0, 0.0, 1.0) => 4,
            (1.0, 1.0, 0.0) => 5,
            (1.0, 0.0, 1.0) => 6,
            (0.0, 1.0, 1.0) => 7,
            (1.0, 1.0, 1.0) => 8,
            _ => 0,
        };
        if color.a() != 1.0 {
            losses.push(IgesLossCode::WriterBodyOpacityNotRepresented.note(format!(
                "IGES body {} opacity {} has no Directory representation",
                body.id,
                color.a()
            )));
        }
        if number == 0 {
            BodyColor::Custom(color)
        } else {
            BodyColor::Standard(number)
        }
    });
    BodyPresentation {
        label,
        name,
        color,
        visible: body.visible,
    }
}

fn append_color_definitions(
    entities: &mut Vec<Entity>,
    presentations: &mut BTreeMap<usize, BodyPresentation>,
) -> Result<(), CodecError> {
    for presentation in presentations.values_mut() {
        let BodyColor::Custom(color) = presentation.color else {
            continue;
        };
        let components = [color.r(), color.g(), color.b()]
            .map(|component| finite(f64::from(component) * 100.0, "color percentage"))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        let entity_index = entities.len();
        entities.push(Entity {
            type_code: 314,
            form: 0,
            label: "COLOR",
            status: EntityStatus::Definition,
            parameter_body: format!(
                "{},{},{},;",
                number(components[0]),
                number(components[1]),
                number(components[2])
            )
            .into_bytes(),
            transform: None,
        });
        presentation.color = BodyColor::Definition(entity_index);
    }
    Ok(())
}

fn append_name_properties(
    entities: &mut Vec<Entity>,
    presentations: &BTreeMap<usize, BodyPresentation>,
) -> Result<(), CodecError> {
    if presentations
        .values()
        .all(|presentation| presentation.name.is_none())
    {
        return Ok(());
    }
    let expanded_count = entities
        .iter()
        .try_fold(0_usize, |count, entity| {
            count.checked_add(usize::from(entity.transform.is_some()) + 1)
        })
        .ok_or_else(|| CodecError::NotImplemented("IGES directory count overflows".into()))?;
    let mut next_sequence = u32::try_from(expanded_count)
        .ok()
        .and_then(|count| count.checked_mul(2))
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| CodecError::NotImplemented("IGES directory sequence overflows".into()))?;
    for (owner_index, presentation) in presentations {
        let Some(name) = &presentation.name else {
            continue;
        };
        let owner = entities.get_mut(*owner_index).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES body owner index {owner_index} is missing"
            ))
        })?;
        if !owner.parameter_body.ends_with(b";") {
            return Err(CodecError::Malformed(
                "IGES body owner has no Parameter Data terminator".into(),
            ));
        }
        owner.parameter_body.pop();
        owner
            .parameter_body
            .extend_from_slice(format!(",0,1,{next_sequence};").as_bytes());
        entities.push(Entity {
            type_code: 406,
            form: 15,
            label: "NAME",
            status: EntityStatus::PhysicallyDependent,
            parameter_body: format!("1,{}H{name};", name.len()).into_bytes(),
            transform: None,
        });
        next_sequence = next_sequence.checked_add(2).ok_or_else(|| {
            CodecError::NotImplemented("IGES directory sequence overflows".into())
        })?;
    }
    Ok(())
}

fn synthesize(ir: &CadIr, version: crate::IgesVersion) -> Result<Synthesis, CodecError> {
    reject_unsupported_model(ir)?;
    reject_owned_wire_topology(ir)?;
    validate_analytic_surface_context(ir)?;
    let mut losses = procedural_reduction_losses(ir)?;
    losses.extend(reject_unsupported_native(ir)?);
    for body in ir
        .model
        .bodies
        .iter()
        .filter(|body| is_decoder_free_geometry_body(body))
    {
        if body.color.is_some() {
            losses.push(IgesLossCode::WriterBodyColorNotRepresented.note(format!(
                "IGES free-geometry body {} has no owning Directory Entry for color",
                body.id
            )));
        }
        if body.visible.is_some() {
            losses.push(
                IgesLossCode::WriterBodyVisibilityNotRepresented.note(format!(
                    "IGES free-geometry body {} has no owning Directory Entry for visibility",
                    body.id
                )),
            );
        }
    }
    let mut body_presentations = BTreeMap::new();

    let mut entities = if has_brep_topology(ir) {
        brep_entities(
            validate_brep_topology(ir, version)?,
            &mut body_presentations,
            &mut losses,
        )?
    } else if has_trimmed_sheet_topology(ir) {
        topology_entities(
            validate_trimmed_sheet_topology(ir, version)?,
            &mut body_presentations,
            &mut losses,
        )?
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
            let SurfaceGeometry::Procedural {
                construction,
                cache: None,
            } = &surface.geometry
            else {
                return None;
            };
            ir.model
                .procedural_surfaces
                .iter()
                .find(|procedural| procedural.id == *construction)
                .and_then(|procedural| match procedural.definition() {
                    ProceduralSurfaceDefinition::Revolution(definition_payload) => {
                        let directrix = definition_payload.directrix();
                        Some(directrix)
                    }
                    ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
                        let directrix = definition_payload.directrix();
                        Some(directrix)
                    }
                    _ => None,
                })
        }) {
            mark_curve_descendants(ir, directrix, &mut consumed_curves, &mut BTreeSet::new())?;
        }
        let mut edges = ir.model.edges.iter().collect::<Vec<_>>();
        edges.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        for edge in edges {
            let curve_id = edge.curve().ok_or_else(|| {
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
            let geometry = flatten_curve(curve.geometry.solved().ok_or_else(|| {
                CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
            })?)?;
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
                    status: EntityStatus::Independent,
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
            let geometry = flatten_curve(curve.geometry.solved().ok_or_else(|| {
                CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
            })?)?;
            append_curve_entity(
                &mut entities,
                ir,
                CurveEntityRequest {
                    version,
                    curve_id: &curve.id,
                    geometry: &geometry,
                    span: None,
                    sense: Sense::Forward,
                    status: EntityStatus::Independent,
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
            entities.push(point_entity(point.position()));
        }
        entities
    };
    append_color_definitions(&mut entities, &mut body_presentations)?;
    ensure_version_support(&entities, version)?;
    resolve_entity_references(&mut entities)?;
    append_name_properties(&mut entities, &body_presentations)?;
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
        bytes: encode_file(&entities, &body_presentations, version, minimum_resolution)?,
        counts,
        losses,
    })
}

fn reject_owned_wire_topology(ir: &CadIr) -> Result<(), CodecError> {
    if !ir
        .model
        .bodies
        .iter()
        .any(|body| body.kind == BodyKind::Wire)
    {
        return Ok(());
    }
    let mut unsupported_wire = None;
    for body in ir
        .model
        .bodies
        .iter()
        .filter(|body| body.kind == BodyKind::Wire)
    {
        if body.regions.is_empty() {
            return Err(CodecError::malformed(format_args!(
                "IGES wire body {} has no region",
                body.id
            )));
        }
        let free_geometry = is_decoder_free_geometry_body(body);
        let mut edge_ids = BTreeSet::new();
        let mut vertex_owners = BTreeMap::new();
        for region_id in &body.regions {
            let region = ir
                .model
                .regions
                .iter()
                .find(|region| region.id == *region_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES wire body {} references missing region {}",
                        body.id, region_id
                    ))
                })?;
            if region.body != body.id {
                return Err(CodecError::malformed(format_args!(
                    "IGES wire region {} is not owned by body {}",
                    region.id, body.id
                )));
            }
            if region.shells.is_empty() {
                return Err(CodecError::malformed(format_args!(
                    "IGES wire region {} has no shell",
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
                            "IGES wire region {} references missing shell {}",
                            region.id, shell_id
                        ))
                    })?;
                if shell.region != region.id {
                    return Err(CodecError::malformed(format_args!(
                        "IGES wire shell {} is not owned by region {}",
                        shell.id, region.id
                    )));
                }
                for edge_id in shell.wire_edges() {
                    if !edge_ids.insert(edge_id) {
                        return Err(CodecError::malformed(format_args!(
                            "IGES wire body {} repeats edge {}",
                            body.id, edge_id
                        )));
                    }
                    let edge = ir
                        .model
                        .edges
                        .iter()
                        .find(|edge| edge.id == *edge_id)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "IGES wire shell {} references missing edge {}",
                                shell.id, edge_id
                            ))
                        })?;
                    for vertex_id in [&edge.start, &edge.end] {
                        if !ir
                            .model
                            .vertices
                            .iter()
                            .any(|vertex| vertex.id == *vertex_id)
                        {
                            return Err(CodecError::malformed(format_args!(
                                "IGES wire edge {} references missing vertex {}",
                                edge.id, vertex_id
                            )));
                        }
                        if let Some(owner) = vertex_owners.insert(vertex_id, edge_id) {
                            if owner != edge_id && free_geometry && unsupported_wire.is_none() {
                                unsupported_wire = Some(format!(
                                    "IGES semantic writer does not encode shared wire vertex {} in body {}",
                                    vertex_id, body.id
                                ));
                            }
                        }
                    }
                }
            }
        }
        if !free_geometry && unsupported_wire.is_none() {
            unsupported_wire = Some(format!(
                "IGES semantic writer does not encode owned wire body {}",
                body.id
            ));
        }
    }
    let edge_vertices = ir
        .model
        .edges
        .iter()
        .flat_map(|edge| [&edge.start, &edge.end])
        .collect::<BTreeSet<_>>();
    let loop_vertices = ir
        .model
        .loops
        .iter()
        .flat_map(Loop::vertices)
        .collect::<BTreeSet<_>>();
    let mut free_vertices = BTreeSet::new();
    for shell in &ir.model.shells {
        for vertex_id in shell.free_vertices() {
            if !ir
                .model
                .vertices
                .iter()
                .any(|vertex| vertex.id == *vertex_id)
            {
                return Err(CodecError::malformed(format_args!(
                    "IGES shell {} references missing free vertex {}",
                    shell.id, vertex_id
                )));
            }
            if edge_vertices.contains(vertex_id) || !free_vertices.insert(vertex_id) {
                return Err(CodecError::malformed(format_args!(
                    "IGES free vertex {vertex_id} has inconsistent ownership"
                )));
            }
        }
    }
    if let Some(vertex) = ir.model.vertices.iter().find(|vertex| {
        !edge_vertices.contains(&vertex.id)
            && !loop_vertices.contains(&vertex.id)
            && !free_vertices.contains(&vertex.id)
    }) {
        return Err(CodecError::malformed(format_args!(
            "IGES vertex {} has no wire or face owner",
            vertex.id
        )));
    }
    if let Some(message) = unsupported_wire {
        return Err(CodecError::NotImplemented(message));
    }
    Ok(())
}

fn validate_analytic_surface_context(ir: &CadIr) -> Result<(), CodecError> {
    let writes_brep = has_brep_topology(ir);
    if let Some(surface) = ir.model.surfaces.iter().find(|surface| {
        (matches!(
            surface.geometry,
            SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(_)
                    | SolvedSurfaceGeometry::Cone(_)
                    | SolvedSurfaceGeometry::Sphere(_)
                    | SolvedSurfaceGeometry::Torus(_)
            )
        )) && (!writes_brep || !ir.model.faces.iter().any(|face| face.surface == surface.id))
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
                    100 | 102 | 108 | 110 | 116 | 120 | 122 | 124 | 126 | 128 | 142 | 144 | 314,
                    0
                ) | (104, 0 | 2 | 3)
                    | (406, 15)
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
                        | 144
                        | 314,
                    0
                ) | (104, 1..=3)
                    | (406, 15)
            ),
            crate::IgesVersion::V5_1 | crate::IgesVersion::V5_2 | crate::IgesVersion::V5_3 => {
                match entity.type_code {
                    100 | 102 | 110 | 116 | 120 | 122 | 123 | 124 | 126 | 128 | 141 | 142 | 143
                    | 144 | 186 | 314 => entity.form == 0,
                    104 => matches!(entity.form, 0 | 2 | 3),
                    190 | 192 | 194 | 196 | 198 => entity.form == 1,
                    502 | 504 | 508 | 510 => entity.form == 1,
                    406 => entity.form == 15,
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
            .find(|shell| shell.faces().iter().any(|face_id| face_id == &face.id))
            .is_some_and(|shell| shell.faces().len() > 1)
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
        let owner = ir
            .model
            .procedural_surface_owner(&procedural.id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES procedural surface {} has no unique carrier",
                    procedural.id
                ))
            })?;
        let surface = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *owner)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES procedural surface {} references missing solved surface {}",
                    procedural.id, owner
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
            surface.geometry.solved(),
            Some(
                SolvedSurfaceGeometry::Plane(_)
                    | SolvedSurfaceGeometry::Nurbs(_)
                    | SolvedSurfaceGeometry::Cylinder(_)
                    | SolvedSurfaceGeometry::Cone(_)
                    | SolvedSurfaceGeometry::Sphere(_)
                    | SolvedSurfaceGeometry::Torus(_)
            )
        ) {
            return Err(CodecError::NotImplemented(format!(
                "IGES procedural surface {} has no writable solved carrier",
                procedural.id
            )));
        }
    }
    for procedural in &ir.model.procedural_curves {
        let owner = ir
            .model
            .procedural_curve_owner(&procedural.id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES procedural curve {} has no unique carrier",
                    procedural.id
                ))
            })?;
        let curve = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *owner)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES procedural curve {} references missing solved curve {}",
                    procedural.id, owner
                ))
            })?;
        let geometry = flatten_curve(curve.geometry.solved().ok_or_else(|| {
            CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
        })?)?;
        if !matches!(
            geometry,
            CurveGeometry::Solved(
                SolvedCurveGeometry::Line(_)
                    | SolvedCurveGeometry::Circle(_)
                    | SolvedCurveGeometry::Ellipse(_)
                    | SolvedCurveGeometry::Parabola(_)
                    | SolvedCurveGeometry::Hyperbola(_)
                    | SolvedCurveGeometry::Nurbs(_)
                    | SolvedCurveGeometry::Polyline(_)
            )
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
            let Some(surface) = ir.model.surfaces.iter().find(|surface| {
                ir.model.procedural_surface_owner(&procedural.id) == Some(&surface.id)
            }) else {
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
            construction: owner, cache: None,
        } if owner == construction
    ) {
        return false;
    }
    match definition {
        ProceduralSurfaceDefinition::Revolution(matched_payload) => matches!(
            (
                &matched_payload.angular_parameter_interval(),
                &matched_payload.parameter_interval(),
                matched_payload.transposed(),
                matched_payload.revision_form(),
            ),
            (None, Some(_), false, None,)
        ),
        ProceduralSurfaceDefinition::Extrusion(matched_payload) => matches!(
            (
                &matched_payload.parameter_interval(),
                matched_payload.revision_form(),
            ),
            (Some(_), None,)
        ),
        _ => false,
    }
}

struct ValidatedTopology<'a> {
    ir: &'a CadIr,
    version: crate::IgesVersion,
    loops: BTreeMap<&'a str, &'a Loop>,
    coedges: BTreeMap<&'a str, &'a cadmpeg_ir::topology::Coedge>,
    pcurves: BTreeMap<&'a str, &'a Pcurve>,
    surfaces: BTreeMap<&'a str, &'a cadmpeg_ir::geometry::Surface>,
}

fn validate_brep_topology(
    ir: &CadIr,
    version: crate::IgesVersion,
) -> Result<ValidatedTopology<'_>, CodecError> {
    let loops = ir
        .model
        .loops
        .iter()
        .rev()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let coedges = ir
        .model
        .coedges
        .iter()
        .rev()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let pcurves = ir
        .model
        .pcurves
        .iter()
        .rev()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .rev()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();

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
            // discarded-value: a solid region must state an exterior shell; ? states the refusal and the split has no reader here
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
            if shell.region != region.id || shell.faces().is_empty() {
                return Err(CodecError::malformed(format_args!(
                    "IGES shell {} is not a nonempty shell of region {}",
                    shell.id, region.id
                )));
            }
            if !shell.wire_edges().is_empty() || !shell.free_vertices().is_empty() {
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
            for face_id in shell.faces() {
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
                    .any(|loop_| face.loop_role(&loop_.id) == LoopBoundaryRole::Unspecified);
                let has_outer_loop = face_loops
                    .iter()
                    .any(|loop_| face.loop_role(&loop_.id) == LoopBoundaryRole::Outer);
                let has_inner_loop = face_loops
                    .iter()
                    .any(|loop_| face.loop_role(&loop_.id) == LoopBoundaryRole::Inner);
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
                let surface = surfaces
                    .get(face.surface.as_str())
                    .copied()
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES face {} references missing surface {}",
                            face.id, face.surface
                        ))
                    })?;
                surface_entities_for_ir(ir, &surface.geometry, 0, version)?;
                if matches!(
                    surface.geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_))
                ) && face.loops.iter().any(|loop_id| {
                    let Some(loop_) = loops.get(loop_id.as_str()).copied() else {
                        return false;
                    };
                    loop_.coedges().len() == 2
                        && loop_
                            .coedges()
                            .iter()
                            .filter_map(|coedge_id| {
                                coedges
                                    .get(coedge_id.as_str())
                                    .copied()
                                    .map(|coedge| coedge.edge.as_str())
                            })
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            == 1
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "IGES B-rep writer refuses cylindrical face {} with a boundary loop that repeats one seam edge without axial bounds",
                        face.id
                    )));
                }
                for loop_id in &face.loops {
                    let loop_ = loops.get(loop_id.as_str()).copied().ok_or_else(|| {
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
                    for coedge_id in loop_.coedges() {
                        let coedge = coedges.get(coedge_id.as_str()).copied().ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "IGES loop {} references missing coedge {}",
                                loop_.id, coedge_id
                            ))
                        })?;
                        if coedge.owner_loop != loop_.id || coedge.use_curve.is_some() {
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
                        let curve_id = edge.curve().ok_or_else(|| {
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
                        let geometry =
                            flatten_curve(curve.geometry.solved().ok_or_else(|| {
                                CodecError::NotImplemented(
                                    "IGES curve carrier has no solved geometry".into(),
                                )
                            })?)?;
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
                        let position = point_position(ir, &vertex.point)?.get();
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
            let coedge = coedges.get(current.as_str()).copied().ok_or_else(|| {
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
                coedges
                    .get(coedge_id.as_str())
                    .copied()
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
    let used_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| face.surface.as_str())
        .collect::<BTreeSet<_>>();
    let used_pcurves = used_brep_pcurve_ids(ir);
    Ok(ValidatedTopology {
        ir,
        version,
        loops: loops
            .into_iter()
            .filter(|(id, _)| used_loops.contains(*id))
            .collect(),
        coedges: coedges
            .into_iter()
            .filter(|(id, _)| used_coedges.contains(*id))
            .collect(),
        pcurves: pcurves
            .into_iter()
            .filter(|(id, _)| used_pcurves.contains(*id))
            .collect(),
        surfaces: surfaces
            .into_iter()
            .filter(|(id, _)| used_surfaces.contains(*id))
            .collect(),
    })
}

fn brep_entities(
    topology: ValidatedTopology<'_>,
    body_presentations: &mut BTreeMap<usize, BodyPresentation>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<Entity>, CodecError> {
    let ir = topology.ir;
    let version = topology.version;
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
        .filter_map(|edge| edge.curve().map(|curve| curve.as_str().to_owned()))
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
        let curve_id = edge.curve().ok_or_else(|| {
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
        let geometry = flatten_curve(curve.geometry.solved().ok_or_else(|| {
            CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
        })?)?;
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
                status: EntityStatus::Independent,
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
        let geometry = flatten_curve(curve.geometry.solved().ok_or_else(|| {
            CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
        })?)?;
        append_curve_entity(
            &mut entities,
            ir,
            CurveEntityRequest {
                version,
                curve_id: &curve.id,
                geometry: &geometry,
                span: None,
                sense: Sense::Forward,
                status: EntityStatus::Independent,
                reference_offset: 0,
            },
        )?;
        mark_curve_descendants(ir, &curve.id, &mut consumed_curve_ids, &mut BTreeSet::new())?;
    }

    let mut pcurve_indices = BTreeMap::new();
    for (pcurve_id, pcurve) in topology.pcurves {
        let index = entities.len();
        entities.push(pcurve_entity(ir, pcurve)?);
        pcurve_indices.insert(pcurve_id.to_owned(), index);
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
            for face_id in shell.faces() {
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
                    let loop_ = topology
                        .loops
                        .get(loop_id.as_str())
                        .copied()
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "IGES topology emission references unvalidated loop {}",
                                loop_id.as_str()
                            ))
                        })?;
                    body_loop_ids.push(loop_.id.clone());
                    for coedge_id in loop_.coedges() {
                        let coedge = topology
                            .coedges
                            .get(coedge_id.as_str())
                            .copied()
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "IGES topology emission references unvalidated coedge {}",
                                    coedge_id.as_str()
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
        let mut parameters = vertex_ids.len().to_string();
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
            for value in point_position(ir, &vertex.point)?.coordinates() {
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
            status: EntityStatus::PhysicallyDependent,
            parameter_body: parameters.into_bytes(),
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
            let mut parameters = edge_ids.len().to_string();
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
                write!(
                    parameters,
                    ",{},{},{},{},{}",
                    reference_marker(curve_index),
                    reference_marker(vertex_list_index),
                    start_index + 1,
                    reference_marker(vertex_list_index),
                    end_index + 1
                )
                .map_err(CodecError::malformed)?;
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 504,
                form: 1,
                label: "EDGES",
                status: EntityStatus::PhysicallyDependentEdgeList,
                parameter_body: parameters.into_bytes(),
                transform: None,
            });
            Some(index)
        };

        let mut loop_indices = BTreeMap::new();
        for loop_id in &body_loop_ids {
            let loop_ = topology
                .loops
                .get(loop_id.as_str())
                .copied()
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES topology emission references unvalidated loop {}",
                        loop_id.as_str()
                    ))
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
            let surface = topology
                .surfaces
                .get(face.surface.as_str())
                .copied()
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES topology emission references unvalidated surface {}",
                        face.surface.as_str()
                    ))
                })?;
            let use_count = loop_
                .coedges()
                .len()
                .checked_add(loop_.vertices().count())
                .ok_or_else(|| CodecError::Malformed("IGES loop use count overflows".into()))?;
            let mut parameters = use_count.to_string();
            for coedge_id in loop_.coedges() {
                let coedge = topology
                    .coedges
                    .get(coedge_id.as_str())
                    .copied()
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "IGES topology emission references unvalidated coedge {}",
                            coedge_id.as_str()
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
                let curve_id = edge.curve().ok_or_else(|| {
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
                let geometry = flatten_curve(curve.geometry.solved().ok_or_else(|| {
                    CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
                })?)?;
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
                write!(
                    parameters,
                    ",0,{},{},{},{}",
                    reference_marker(edge_list_index),
                    edge_index + 1,
                    sense,
                    coedge.pcurves.len()
                )
                .map_err(CodecError::malformed)?;
                for (pcurve_use, pcurve_index) in pcurve_entities {
                    write!(
                        parameters,
                        ",{},{}",
                        isoparametric_flag(pcurve_use, loop_.id.as_str())?,
                        reference_marker(pcurve_index)
                    )
                    .map_err(CodecError::malformed)?;
                }
                for vertex_use in loop_
                    .anchored_vertex_uses()
                    .iter()
                    .filter(|vertex_use| vertex_use.after == coedge.id)
                {
                    let vertex_index = vertex_indices[vertex_use.vertex.as_str()];
                    write!(
                        parameters,
                        ",1,{},{},{},{}",
                        reference_marker(vertex_list_index),
                        vertex_index + 1,
                        0,
                        vertex_use.pcurves.len()
                    )
                    .map_err(CodecError::malformed)?;
                    for pcurve_use in &vertex_use.pcurves {
                        write!(
                            parameters,
                            ",{},{}",
                            isoparametric_flag(pcurve_use, loop_.id.as_str())?,
                            reference_marker(pcurve_indices[pcurve_use.pcurve.as_str()])
                        )
                        .map_err(CodecError::malformed)?;
                    }
                }
            }
            if let Some((vertex, pcurves)) = loop_.singular_vertex() {
                let vertex_index = vertex_indices[vertex.as_str()];
                write!(
                    parameters,
                    ",1,{},{},{},{}",
                    reference_marker(vertex_list_index),
                    vertex_index + 1,
                    0,
                    pcurves.len()
                )
                .map_err(CodecError::malformed)?;
                for pcurve_use in pcurves {
                    write!(
                        parameters,
                        ",{},{}",
                        isoparametric_flag(pcurve_use, loop_.id.as_str())?,
                        reference_marker(pcurve_indices[pcurve_use.pcurve.as_str()])
                    )
                    .map_err(CodecError::malformed)?;
                }
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 508,
                form: 1,
                label: "LOOP",
                status: EntityStatus::PhysicallyDependent,
                parameter_body: parameters.into_bytes(),
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
            let has_outer = face_outer_loop(face, &loops).is_some();
            let mut parameters = format!(
                "{},{},{}",
                reference_marker(surface_indices[face.surface.as_str()]),
                loops.len(),
                i32::from(has_outer)
            );
            for loop_ in loops {
                write!(
                    parameters,
                    ",{}",
                    reference_marker(loop_indices[loop_.id.as_str()])
                )
                .map_err(CodecError::malformed)?;
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 510,
                form: 1,
                label: "FACE",
                status: EntityStatus::PhysicallyDependent,
                parameter_body: parameters.into_bytes(),
                transform: None,
            });
            face_indices.insert(face_id.as_str().to_owned(), index);
        }

        let mut shell_indices = BTreeMap::new();
        for shell in &shells {
            let mut parameters = shell.faces().len().to_string();
            for face_id in shell.faces() {
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
                write!(
                    parameters,
                    ",{},{}",
                    reference_marker(face_indices[face.id.as_str()]),
                    brep_sense(face.sense)
                )
                .map_err(CodecError::malformed)?;
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 514,
                form: if body.kind == BodyKind::Solid { 1 } else { 2 },
                label: "SHELL",
                status: if body.kind == BodyKind::Solid {
                    EntityStatus::PhysicallyDependent
                } else {
                    EntityStatus::Independent
                },
                parameter_body: parameters.into_bytes(),
                transform: None,
            });
            if body.kind == BodyKind::Sheet {
                body_presentations.insert(index, body_presentation(body, losses));
            }
            shell_indices.insert(shell.id.as_str(), index);
        }
        if body.kind == BodyKind::Solid {
            let (exterior_shell, void_shells) = solid_shell_roles(region)?;
            let mut parameters = format!(
                "{},1,{}",
                reference_marker(shell_indices[exterior_shell.as_str()]),
                void_shells.len()
            );
            for void_shell in void_shells {
                write!(
                    parameters,
                    ",{},1",
                    reference_marker(shell_indices[void_shell.as_str()])
                )
                .map_err(CodecError::malformed)?;
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 186,
                form: 0,
                label: "SOLID",
                status: EntityStatus::Independent,
                parameter_body: parameters.into_bytes(),
                transform: None,
            });
            body_presentations.insert(index, body_presentation(body, losses));
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
        entities.push(point_entity(point.position()));
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
                        .free_vertices()
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
        let Some(curve_id) = edge.curve() else {
            continue;
        };
        let Some(curve) = ir.model.curves.iter().find(|curve| curve.id == *curve_id) else {
            continue;
        };
        let matching_topology_tolerance = topology_edges
            .iter()
            .filter(|topology_edge| {
                topology_edge.curve() == Some(curve_id)
                    && topology_edge
                        .param_range()
                        .zip(edge.param_range())
                        .is_some_and(|(topology_range, edge_range)| {
                            same_range(topology_range.get(), edge_range.get())
                        })
            })
            .map(|topology_edge| topology_edge_explicit_tolerance(ir, topology_edge))
            .fold(0.0, f64::max);
        let is_model_carrier = topology_edges.iter().any(|topology_edge| {
            topology_edge.curve() == Some(curve_id)
                && topology_edge
                    .param_range()
                    .zip(edge.param_range())
                    .is_some_and(|(topology_range, edge_range)| {
                        same_range(topology_range.get(), edge_range.get())
                    })
                && vertex_position(ir, &topology_edge.start)
                    .zip(vertex_position(ir, &edge.start))
                    .is_some_and(|(topology_start, edge_start)| {
                        same_point_with_tolerance(
                            topology_start.get(),
                            edge_start.get(),
                            topology_edge_explicit_tolerance(ir, topology_edge),
                        )
                    })
                && vertex_position(ir, &topology_edge.end)
                    .zip(vertex_position(ir, &edge.end))
                    .is_some_and(|(topology_end, edge_end)| {
                        same_point_with_tolerance(
                            topology_end.get(),
                            edge_end.get(),
                            topology_edge_explicit_tolerance(ir, topology_edge),
                        )
                    })
        });
        let is_pcurve_carrier = ir.model.pcurves.iter().any(|pcurve| {
            pcurve.parameter_range().is_some_and(|range| {
                curve_matches_pcurve(&curve.geometry, range.get(), pcurve)
                    && edge
                        .param_range()
                        .is_some_and(|edge_range| same_range(edge_range.get(), range.get()))
                    && vertex_position(ir, &edge.start)
                        .zip(curve_point(&curve.geometry, range[0]).ok())
                        .is_some_and(|(start, evaluated)| {
                            same_point_with_tolerance(
                                start.get(),
                                evaluated.get(),
                                matching_topology_tolerance,
                            )
                        })
                    && vertex_position(ir, &edge.end)
                        .zip(curve_point(&curve.geometry, range[1]).ok())
                        .is_some_and(|(end, evaluated)| {
                            same_point_with_tolerance(
                                end.get(),
                                evaluated.get(),
                                matching_topology_tolerance,
                            )
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
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = curve else {
        return false;
    };
    let PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry else {
        return false;
    };
    curve.degree() == nurbs.degree()
        && curve.periodic() == nurbs.periodic()
        && curve.knots().len() == nurbs.knots().len()
        && curve
            .knots()
            .iter()
            .zip(nurbs.knots())
            .all(|(left, right)| same_float(*left, *right))
        && curve.control_points().len() == nurbs.control_points().len()
        && curve
            .control_points()
            .iter()
            .zip(nurbs.control_points())
            .all(|(left, right)| {
                same_float(left.x, right.u)
                    && same_float(left.y, right.v)
                    && same_float(left.z, 0.0)
            })
        && match (curve.weights(), nurbs.weights()) {
            (None, None) => true,
            (Some(left), Some(right)) if left.len() == right.len() => left
                .iter()
                .zip(right)
                .all(|(left, right)| same_float(left.get(), right.get())),
            _ => false,
        }
        && pcurve
            .parameter_range()
            .is_some_and(|candidate| same_range(candidate.get(), range))
}

fn same_float(left: f64, right: f64) -> bool {
    (left - right).abs() <= left.abs().max(right.abs()).max(1.0) * EPS_WRITE_DEGENERATE
}

fn topology_entities(
    topology: ValidatedTopology<'_>,
    body_presentations: &mut BTreeMap<usize, BodyPresentation>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<Entity>, CodecError> {
    let ir = topology.ir;
    let version = topology.version;
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
        let curve_id = edge.curve().ok_or_else(|| {
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
        let geometry = flatten_curve(curve.geometry.solved().ok_or_else(|| {
            CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
        })?)?;
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
                status: EntityStatus::PhysicallyDependent,
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
        let geometry = flatten_curve(curve.geometry.solved().ok_or_else(|| {
            CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
        })?)?;
        append_curve_entity(
            &mut entities,
            ir,
            CurveEntityRequest {
                version,
                curve_id: &curve.id,
                geometry: &geometry,
                span: None,
                sense: Sense::Forward,
                status: EntityStatus::Independent,
                reference_offset: 0,
            },
        )?;
        mark_curve_descendants(ir, &curve.id, &mut consumed_curves, &mut BTreeSet::new())?;
    }

    let mut pcurve_indices = BTreeMap::new();
    for (pcurve_id, pcurve) in topology.pcurves {
        let index = entities.len();
        entities.push(pcurve_entity(ir, pcurve)?);
        pcurve_indices.insert(pcurve_id.to_owned(), index);
    }

    let mut boundary_indices = BTreeMap::new();
    let mut curve_on_surface_indices = BTreeMap::new();
    let mut faces = ir.model.faces.iter().collect::<Vec<_>>();
    faces.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for face in &faces {
        let surface_index = surface_indices[face.surface.as_str()];
        let surface = topology
            .surfaces
            .get(face.surface.as_str())
            .copied()
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES topology emission references unvalidated surface {}",
                    face.surface.as_str()
                ))
            })?;
        let loops = face_loop_order(ir, face)?;
        let bounded = loops
            .iter()
            .all(|loop_| face.loop_role(&loop_.id) == LoopBoundaryRole::Unspecified);
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
        let surface_index = surface_indices[face.surface.as_str()];
        let loops = face_loop_order(ir, face)?;
        let bounded = loops
            .iter()
            .all(|loop_| face.loop_role(&loop_.id) == LoopBoundaryRole::Unspecified);
        let mut parameters = if bounded {
            let representation = loops
                .first()
                .and_then(|loop_| loop_.coedges().first())
                .map(|coedge_id| {
                    topology
                        .coedges
                        .get(coedge_id.as_str())
                        .copied()
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "IGES topology emission references unvalidated coedge {}",
                                coedge_id.as_str()
                            ))
                        })
                })
                .transpose()?
                .map_or(0, |coedge| i32::from(!coedge.pcurves.is_empty()));
            format!(
                "{representation},{},{}",
                reference_marker(surface_index),
                loops.len()
            )
        } else {
            let outer = face_outer_loop(face, &loops);
            let inner = if outer.is_some() {
                &loops[1..]
            } else {
                &loops[..]
            };
            let mut parameters = format!(
                "{},{},{},{}",
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
        let body = ir
            .model
            .shells
            .iter()
            .find(|shell| shell.id == face.shell)
            .and_then(|shell| {
                ir.model
                    .regions
                    .iter()
                    .find(|region| region.id == shell.region)
            })
            .and_then(|region| ir.model.bodies.iter().find(|body| body.id == region.body))
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES sheet face {} has no owning body",
                    face.id
                ))
            })?;
        let index = entities.len();
        entities.push(Entity {
            type_code: if bounded { 143 } else { 144 },
            form: 0,
            label: if bounded { "BOUNDED" } else { "TRIMMED" },
            status: EntityStatus::Independent,
            parameter_body: parameters.into_bytes(),
            transform: None,
        });
        body_presentations.insert(index, body_presentation(body, losses));
    }

    let mut points = ir.model.points.iter().collect::<Vec<_>>();
    points.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for point in points {
        if consumed_points.contains(point.id.as_str())
            || ignored_carriers.points.contains(point.id.as_str())
        {
            continue;
        }
        entities.push(point_entity(point.position()));
    }
    Ok(entities)
}

fn validate_trimmed_sheet_topology(
    ir: &CadIr,
    version: crate::IgesVersion,
) -> Result<ValidatedTopology<'_>, CodecError> {
    let loops = ir
        .model
        .loops
        .iter()
        .rev()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let coedges = ir
        .model
        .coedges
        .iter()
        .rev()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let pcurves = ir
        .model
        .pcurves
        .iter()
        .rev()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .rev()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();

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
            || shell.faces().len() != 1
            || !shell.wire_edges().is_empty()
            || !shell.free_vertices().is_empty()
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
            .find(|candidate| candidate.id == shell.faces()[0])
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES shell {} references missing face {}",
                    shell.id,
                    shell.faces()[0]
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
        let surface = surfaces
            .get(face.surface.as_str())
            .copied()
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
            .any(|loop_| face.loop_role(&loop_.id) == LoopBoundaryRole::Unspecified);
        let has_explicit_loop = loops
            .iter()
            .any(|loop_| face.loop_role(&loop_.id) != LoopBoundaryRole::Unspecified);
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
                coedges
                    .get(coedge_id.as_str())
                    .copied()
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
            for coedge_id in loop_.coedges() {
                let coedge = coedges.get(coedge_id.as_str()).copied().ok_or_else(|| {
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
                if coedge.owner_loop != loop_.id
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
                let curve_id = edge.curve().ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "IGES semantic writer does not encode carrier-less edge {}",
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
                let span = edge_span(
                    ir,
                    edge,
                    &flatten_curve(curve.geometry.solved().ok_or_else(|| {
                        CodecError::NotImplemented(
                            "IGES curve carrier has no solved geometry".into(),
                        )
                    })?)?,
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
                    let pcurve = pcurves
                        .get(pcurve_use.pcurve.as_str())
                        .copied()
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "IGES coedge {} references missing pcurve {}",
                                coedge.id, pcurve_use.pcurve
                            ))
                        })?;
                    if pcurve.wrapper_reversed().is_some() || pcurve.native_tail_flags().is_some() {
                        return Err(CodecError::NotImplemented(format!(
                            "IGES semantic writer does not encode pcurve wrapper metadata {}",
                            pcurve.id
                        )));
                    }
                    let Some(parameter_range) = pcurve.parameter_range() else {
                        return Err(CodecError::NotImplemented(format!(
                            "IGES semantic writer requires a parameter range for pcurve {}",
                            pcurve.id
                        )));
                    };
                    if pcurve_use
                        .parameter_range
                        .is_some_and(|range| !same_range(range.endpoints(), parameter_range.get()))
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
    let used_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| face.surface.as_str())
        .collect::<BTreeSet<_>>();
    Ok(ValidatedTopology {
        ir,
        version,
        loops: loops
            .into_iter()
            .filter(|(id, _)| used_loops.contains(*id))
            .collect(),
        coedges: coedges
            .into_iter()
            .filter(|(id, _)| used_coedges.contains(*id))
            .collect(),
        pcurves: pcurves
            .into_iter()
            .filter(|(id, _)| used_pcurves.contains(*id))
            .collect(),
        surfaces: surfaces
            .into_iter()
            .filter(|(id, _)| used_surfaces.contains(*id))
            .collect(),
    })
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
        .filter(|loop_| face.loop_role(&loop_.id) == LoopBoundaryRole::Outer)
        .count()
        > 1
    {
        return Err(CodecError::NotImplemented(format!(
            "IGES Type 144 supports one explicit outer loop per face ({})",
            face.id
        )));
    }
    loops.sort_by_key(|loop_| match face.loop_role(&loop_.id) {
        LoopBoundaryRole::Outer => 0,
        LoopBoundaryRole::Unspecified => 1,
        LoopBoundaryRole::Inner => 2,
    });
    Ok(loops)
}

fn face_outer_loop<'a>(face: &cadmpeg_ir::topology::Face, loops: &'a [&Loop]) -> Option<&'a Loop> {
    loops
        .first()
        .copied()
        .filter(|loop_| face.loop_role(&loop_.id) == LoopBoundaryRole::Outer)
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
        "{representation},{BOUNDARY_PREFERENCE_MODEL_CURVES},{},{}",
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
        write!(parameters, ",{sense},{}", coedge.pcurves.len()).map_err(CodecError::malformed)?;
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
            let curve_id = edge.curve().ok_or_else(|| {
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
            let geometry = flatten_curve(curve.geometry.solved().ok_or_else(|| {
                CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
            })?)?;
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
        status: EntityStatus::PhysicallyDependent,
        parameter_body: parameters.into_bytes(),
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
        let curve_id = edge.curve().ok_or_else(|| {
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
        let geometry = flatten_curve(curve.geometry.solved().ok_or_else(|| {
            CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
        })?)?;
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
                    status: EntityStatus::PhysicallyDependent,
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
            EntityStatus::PhysicallyDependent,
        )?
    };
    let parameter_curve = if pcurve_children.len() == 1 {
        pcurve_children[0]
    } else {
        push_composite_entity(
            entities,
            &pcurve_children,
            "PCURVE",
            EntityStatus::ParameterCurve,
        )?
    };
    Ok(Entity {
        type_code: 142,
        form: 0,
        label: "CURVSURF",
        status: EntityStatus::PhysicallyDependent,
        parameter_body: format!(
            "{CURVE_ON_SURFACE_CREATION_UNSPECIFIED},{},{},{},{CURVE_ON_SURFACE_PREFERENCE_MODEL_CURVE};",
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
    status: EntityStatus,
) -> Result<usize, CodecError> {
    push_composite_entity_with_reference_offset(entities, children, label, status, 0)
}

fn push_composite_entity_with_reference_offset(
    entities: &mut Vec<Entity>,
    children: &[usize],
    label: &'static str,
    status: EntityStatus,
    reference_offset: usize,
) -> Result<usize, CodecError> {
    let children = flatten_composite_children(entities, children)?;
    if children.is_empty() {
        return Err(CodecError::Malformed(
            "IGES composite curve has no children".into(),
        ));
    }
    let mut parameters = children.len().to_string();
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
        parameter_body: parameters.into_bytes(),
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
    let text = std::str::from_utf8(&entity.parameter_body).map_err(|_| {
        CodecError::Malformed("IGES emitted composite curve parameters are not UTF-8".into())
    })?;
    let mut fields = text.trim_end_matches(';').split(',');
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
        let mut entity = curve_entity(
            geometry.solved().ok_or_else(|| {
                CodecError::NotImplemented("IGES carrier has no solved geometry".into())
            })?,
            Some(span),
            version,
        )?;
        entity.status = EntityStatus::PhysicallyDependent;
        return Ok(entity);
    }
    let reversed_span = CurveSpan {
        range: span.range,
        start: span.end,
        end: span.start,
    };
    let mut entity = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_)) => curve_entity(
            geometry.solved().ok_or_else(|| {
                CodecError::NotImplemented("IGES carrier has no solved geometry".into())
            })?,
            Some(&reversed_span),
            version,
        )?,
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            let (reversed, range) = reverse_nurbs(nurbs, span.range)?;
            let reversed_span = CurveSpan {
                range,
                ..reversed_span
            };
            curve_entity(
                &SolvedCurveGeometry::Nurbs(reversed),
                Some(&reversed_span),
                version,
            )?
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius();
            let reversed = crate::entities::curve_conversion::circular_arc_nurbs(
                center,
                *axis,
                *ref_direction,
                radius,
                span.range,
            )
            .map_err(|error| CodecError::malformed(format_args!("circular: {error}")))?
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
                &SolvedCurveGeometry::Nurbs(reversed),
                Some(&reversed_span),
                version,
            )?
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius();
            let minor_radius = ellipse_curve.minor_radius();
            let reversed = crate::entities::curve_conversion::elliptical_arc_nurbs(
                center,
                *axis,
                *major_direction,
                major_radius,
                minor_radius,
                span.range,
            )
            .map_err(|error| CodecError::malformed(format_args!("elliptical: {error}")))?
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
                &SolvedCurveGeometry::Nurbs(reversed),
                Some(&reversed_span),
                version,
            )?
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Parabola(parabola_curve)) => {
            let vertex = parabola_curve.vertex().get();
            let axis = parabola_curve.frame().axis().as_raw();
            let major_direction = parabola_curve.frame().reference().as_raw();
            let focal_distance = parabola_curve.focal_distance();
            let reversed = crate::entities::curve_conversion::parabolic_arc_nurbs(
                vertex,
                *axis,
                *major_direction,
                focal_distance,
                span.range,
            )
            .map_err(|error| CodecError::malformed(format_args!("parabolic: {error}")))?
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
                &SolvedCurveGeometry::Nurbs(reversed),
                Some(&reversed_span),
                version,
            )?
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Polyline(polyline)) => {
            let values = polyline_parameters(polyline)?;
            let original = NurbsCurve::from_lanes(
                1,
                polyline_knots(&values),
                polyline.points().collect(),
                None,
                false,
            )
            .map_err(|error| CodecError::malformed(format_args!("polyline: {error}")))?;
            let (reversed, range) = reverse_nurbs(&original, span.range)?;
            let reversed_span = CurveSpan {
                range,
                ..reversed_span
            };
            curve_entity(
                &SolvedCurveGeometry::Nurbs(reversed),
                Some(&reversed_span),
                version,
            )?
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(hyperbola_curve)) => {
            // Reversing the frame axis maps p(u) to p(-u).
            let mut frame = *hyperbola_curve.frame();
            frame.reverse_axis();
            let reversed_geometry = CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
                cadmpeg_ir::geometry::analytic::HyperbolaCurve::new(
                    hyperbola_curve.center(),
                    frame,
                    hyperbola_curve.major_radius(),
                    hyperbola_curve.minor_radius(),
                ),
            ));
            let reversed_range = [-span.range[1], -span.range[0]];
            let reversed_span = CurveSpan {
                range: reversed_range,
                start: span.end,
                end: span.start,
            };
            curve_entity(
                reversed_geometry.solved().ok_or_else(|| {
                    CodecError::NotImplemented("IGES carrier has no solved geometry".into())
                })?,
                Some(&reversed_span),
                version,
            )?
        }
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "IGES reversed Type 142 edge carrier is unsupported ({geometry:?})"
            )))
        }
    };
    entity.status = EntityStatus::PhysicallyDependent;
    Ok(entity)
}

fn procedural_pcurve_source_map(
    ir: &CadIr,
    surface_id: &SurfaceId,
) -> Result<Option<(f64, f64, f64, f64)>, CodecError> {
    let Some(procedural) =
        ir.model.procedural_surfaces.iter().find(|procedural| {
            ir.model.procedural_surface_owner(&procedural.id) == Some(surface_id)
        })
    else {
        return Ok(None);
    };
    let (directrix, fallback_interval) = match procedural.definition() {
        ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
            let directrix = definition_payload.directrix();
            let parameter_interval = definition_payload
                .parameter_interval()
                .map(cadmpeg_ir::units::FiniteVector::get);
            (directrix, parameter_interval.unwrap_or([0.0, 1.0]))
        }
        ProceduralSurfaceDefinition::Revolution(definition_payload) => {
            let directrix = definition_payload.directrix();
            let parameter_interval = definition_payload
                .parameter_interval()
                .map(IncreasingParameterInterval::endpoints);
            (directrix, parameter_interval.unwrap_or([0.0, 1.0]))
        }
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
    let geometry = flatten_curve(source_curve.geometry.solved().ok_or_else(|| {
        CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
    })?)?;
    let carrier_interval =
        construction_carrier_interval(ir, directrix, &geometry, procedural, fallback_interval)?;
    let mut u_map;
    let mut v_map = (1.0, 0.0);
    match procedural.definition() {
        ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
            let directrix = definition_payload.directrix();
            let parameter_interval = definition_payload
                .parameter_interval()
                .map(cadmpeg_ir::units::FiniteVector::get);
            {
                let source_interval = if line_directrix(ir, directrix) {
                    parameter_interval.unwrap_or([0.0, 1.0])
                } else {
                    parameter_interval.unwrap_or(carrier_interval)
                };
                u_map =
                    affine_parameter_map(carrier_interval, source_interval).ok_or_else(|| {
                        CodecError::Malformed(
                            "IGES procedural surface parameter domains are invalid".into(),
                        )
                    })?;
            }
        }
        ProceduralSurfaceDefinition::Revolution(definition_payload) => {
            let directrix = definition_payload.directrix();
            let angular_interval = definition_payload.angular_interval().endpoints();
            let angular_parameter_interval = definition_payload
                .angular_parameter_interval()
                .map(IncreasingParameterInterval::endpoints);
            let parameter_interval = definition_payload
                .parameter_interval()
                .map(IncreasingParameterInterval::endpoints);
            let transposed = definition_payload.transposed();
            {
                let source_interval = if line_directrix(ir, directrix) {
                    parameter_interval.unwrap_or([0.0, 1.0])
                } else {
                    parameter_interval.unwrap_or(carrier_interval)
                };
                u_map =
                    affine_parameter_map(carrier_interval, source_interval).ok_or_else(|| {
                        CodecError::Malformed(
                            "IGES procedural surface parameter domains are invalid".into(),
                        )
                    })?;
                if let Some(parameter_interval) = angular_parameter_interval {
                    v_map = affine_parameter_map(angular_interval, parameter_interval).ok_or_else(
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
    let PcurveGeometry::Nurbs { nurbs } = &mut pcurve.geometry else {
        return Ok(pcurve);
    };
    nurbs
        .edit_control_points(|point| {
            point.u = point.u.mul_add(u_factor, u_offset);
            point.v = point.v.mul_add(v_factor, v_offset);
            Ok(())
        })
        .map_err(|error| {
            CodecError::malformed(format_args!(
                "pcurve {} parameter mapping: {error}",
                pcurve.id
            ))
        })?;
    Ok(pcurve)
}

fn oriented_pcurve_entity(ir: &CadIr, pcurve: &Pcurve) -> Result<Entity, CodecError> {
    let pcurve = source_pcurve(ir, pcurve)?;
    let range = pcurve.parameter_range().ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "IGES semantic writer requires a parameter range for pcurve {}",
            pcurve.id
        ))
    })?;
    let PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry else {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer only encodes NURBS pcurves ({})",
            pcurve.id
        )));
    };
    let nurbs = nurbs
        .lift(|point| Point3::new(point.u, point.v, 0.0))
        .map_err(|error| CodecError::malformed(format_args!("pcurve {}: {error}", pcurve.id)))?;
    let (reversed, range) = reverse_nurbs(&nurbs, range.get())?;
    encode_nurbs(&reversed, range, "PCURVE")
}

fn reverse_nurbs(
    nurbs: &NurbsCurve,
    range: [f64; 2],
) -> Result<(NurbsCurve, [f64; 2]), CodecError> {
    let [start, end] = nurbs_domain(nurbs)?;
    let invalid =
        || CodecError::Malformed("IGES reversed NURBS domain or parameter range is invalid".into());
    let [Some(range_start), Some(range_end)] = range.map(FiniteReal::new) else {
        return Err(invalid());
    };
    if range_start > range_end || range_start < start || range_end > end {
        return Err(invalid());
    }
    let reflect = |parameter| {
        cadmpeg_ir::math::reflect_parameter(parameter, start, end)
            .map(FiniteReal::get)
            .ok_or_else(|| {
                CodecError::malformed("IGES reversed NURBS knot or parameter is non-finite")
            })
    };
    let knots = nurbs
        .knots()
        .finite_knots()
        .rev()
        .map(reflect)
        .collect::<Result<Vec<_>, _>>()?;
    let reversed_range = [reflect(range_end)?, reflect(range_start)?];
    let mut poles = nurbs.pole_rows().clone();
    poles.reverse();
    let reversed = NurbsCurve::new(nurbs.degree(), knots, poles, nurbs.periodic())
        .map_err(|error| CodecError::malformed(format_args!("reversed NURBS: {error}")))?;
    Ok((reversed, reversed_range))
}

fn pcurve_entity(ir: &CadIr, pcurve: &Pcurve) -> Result<Entity, CodecError> {
    let pcurve = source_pcurve(ir, pcurve)?;
    let range = pcurve.parameter_range().ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "IGES semantic writer requires a parameter range for pcurve {}",
            pcurve.id
        ))
    })?;
    let PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry else {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer only encodes NURBS pcurves ({})",
            pcurve.id
        )));
    };
    let curve = nurbs
        .lift(|point| Point3::new(point.u, point.v, 0.0))
        .map_err(|error| CodecError::malformed(format_args!("pcurve {}: {error}", pcurve.id)))?;
    encode_nurbs(&curve, range.get(), "PCURVE")
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
        let mut resolved = Vec::with_capacity(entity.parameter_body.len());
        let mut index = 0;
        while index < entity.parameter_body.len() {
            if entity.parameter_body[index] == b'@'
                && entity.parameter_body.get(index + 1) == Some(&b'R')
            {
                let Some(end_offset) = entity.parameter_body[index + 2..]
                    .iter()
                    .position(|byte| *byte == b'@')
                else {
                    return Err(CodecError::Malformed(
                        "IGES entity contains an unterminated pointer reference".into(),
                    ));
                };
                let end = index + 2 + end_offset;
                let target = std::str::from_utf8(&entity.parameter_body[index + 2..end])
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
                resolved.push(entity.parameter_body[index]);
                index += 1;
            }
        }
        entity.parameter_body = resolved;
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
        let range = pcurve.parameter_range().ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "IGES B-rep {} requires a parameter range for pcurve {}",
                orientation.owner, pcurve.id
            ))
        })?;
        if pcurve_use
            .parameter_range
            .is_some_and(|use_range| !same_range(use_range.endpoints(), range.get()))
        {
            return Err(CodecError::NotImplemented(format!(
                "IGES B-rep {} cannot restrict pcurve use {}",
                orientation.owner, pcurve_use.pcurve
            )));
        }
        if pcurve.wrapper_reversed().is_some() || pcurve.native_tail_flags().is_some() {
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
            let range = pcurve.parameter_range().ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "IGES {} requires a parameter range for pcurve {}",
                    self.owner, pcurve.id
                ))
            })?;
            if range[0] > range[1] {
                return Err(CodecError::malformed(format_args!(
                    "IGES {} pcurve {} has an invalid parameter range",
                    self.owner, pcurve.id
                )));
            }
            // A non-finite pcurve point is evaluated on the support as a finite
            // one is.
            let pcurve_point = |parameter, position| match pcurve_uv(&pcurve.geometry, parameter) {
                Ok(uv) => Ok(uv.get()),
                Err(failure) => failure.non_finite().ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "IGES {} pcurve {} {position} cannot be evaluated",
                        self.owner, pcurve.id
                    ))
                }),
            };
            let start_uv = pcurve_point(range[0], "start")?;
            let end_uv = pcurve_point(range[1], "end")?;
            let start = model_surface_point(self.ir, self.surface, start_uv.u, start_uv.v);
            let end = model_surface_point(self.ir, self.surface, end_uv.u, end_uv.v);
            // Both ends are refused outside the support before either is
            // refused as non-finite.
            for (point, position) in [(&start, "start"), (&end, "end")] {
                if matches!(point, Err(EvaluationFailure::NoValue)) {
                    return Err(CodecError::malformed(format_args!(
                        "IGES {} pcurve {} {position} is outside its support",
                        self.owner, pcurve.id
                    )));
                }
            }
            let finite = |point: Result<FinitePoint3, EvaluationFailure<Point3>>,
                          position: &str| {
                point.map_err(|_| {
                    CodecError::malformed(format_args!(
                        "IGES point {} pcurve {} {position} has non-finite coordinates",
                        self.owner, pcurve.id
                    ))
                })
            };
            mapped.push((finite(start, "start")?.get(), finite(end, "end")?.get()));
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
    let mut tolerance = edge
        .tolerance
        .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get);
    for vertex_id in [&edge.start, &edge.end] {
        if let Some(vertex) = ir
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == *vertex_id)
        {
            tolerance = tolerance.max(
                vertex
                    .tolerance
                    .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
            );
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
        .map(cadmpeg_ir::scalar::PositiveReal::get)
        .map(effective_topology_tolerance)
        .fold(cadmpeg_ir::units::COINCIDENCE_TOLERANCE, f64::max);
    let endpoint_scale = generated_endpoint_coordinate_scale(ir);
    topology_tolerance.max(endpoint_scale * WRITER_ENDPOINT_RELATIVE_TOLERANCE)
}

fn minimum_resolution_for_output(ir: &CadIr) -> f64 {
    let generated = generated_minimum_resolution(ir);
    generated.max(ir.tolerances.linear.get())
}

fn minimum_resolution_loss(ir: &CadIr, emitted: f64) -> Option<LossNote> {
    ir.source.as_ref()?;
    let declared = ir.tolerances.linear.get();
    if emitted <= declared {
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
        let Some(curve_id) = edge.curve() else {
            continue;
        };
        let Some(range) = edge.param_range() else {
            continue;
        };
        let Some(curve) = ir.model.curves.iter().find(|curve| curve.id == *curve_id) else {
            continue;
        };
        for parameter in range {
            if let Ok(point) = curve_point(&curve.geometry, parameter) {
                scale = scale.max(point_coordinate_scale(point));
            }
        }
    }
    scale
}

fn point_coordinate_scale(point: FinitePoint3) -> f64 {
    [point.x, point.y, point.z]
        .into_iter()
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
            314 => "314_color_definition",
            406 => "406_name_property",
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

fn represented_name_properties(
    ir: &CadIr,
    namespace: &NativeNamespace,
) -> Option<BTreeSet<String>> {
    let product_properties = namespace.arenas().get("product_properties");
    let properties = namespace.arenas().get("properties");
    if product_properties.is_none_or(Vec::is_empty) && properties.is_none_or(Vec::is_empty) {
        return Some(BTreeSet::new());
    }
    let native_entities = namespace
        .arenas()
        .get("entities")
        .into_iter()
        .flatten()
        .map(|record| (record.id(), record))
        .collect::<BTreeMap<_, _>>();
    let named_bodies = ir
        .model
        .bodies
        .iter()
        .filter(|body| body.name.is_some())
        .map(|body| body.id.clone())
        .collect::<BTreeSet<_>>();
    let mut product_sources = BTreeSet::new();
    let mut application_sources = BTreeSet::new();
    for (records, sources) in [
        (product_properties, &mut product_sources),
        (properties, &mut application_sources),
    ] {
        for record in records.into_iter().flatten() {
            if record.field("form").and_then(|value| value.as_i64()) != Some(15) {
                return None;
            }
            let source = record
                .field("source_entity")
                .and_then(|value| value.as_str().map(str::to_owned));
            let value = record.field("value");
            let owners = record.field("owners");
            let represented = source.as_ref().is_some_and(|source| {
                native_entities.get(source.as_str()).is_some_and(|entity| {
                    entity.field("entity_type").and_then(|value| value.as_i64()) == Some(406)
                        && entity.field("form").and_then(|value| value.as_i64()) == Some(15)
                })
            }) && value
                .as_ref()
                .and_then(|value| value.as_array())
                .is_some_and(|bytes| {
                    !bytes.is_empty()
                        && bytes.iter().all(|byte| {
                            byte.as_u64().is_some_and(|byte| {
                                u8::try_from(byte)
                                    .is_ok_and(|byte| byte.is_ascii_graphic() || byte == b' ')
                            })
                        })
                })
                && record
                    .field("property_kind")
                    .is_some_and(|value| value.as_str() == Some("name"))
                && record
                    .field("declared_value_count")
                    .is_none_or(|value| value.as_i64() == Some(1))
                && owners
                    .as_ref()
                    .and_then(|value| value.as_array())
                    .is_some_and(|owners| {
                        !owners.is_empty()
                            && owners.iter().all(|owner| {
                                let Some(owner_id) = owner.as_str() else {
                                    return false;
                                };
                                let Some(sequence) = owner_id
                                    .strip_prefix("iges:entity:directory#")
                                    .and_then(|sequence| sequence.parse::<u32>().ok())
                                else {
                                    return false;
                                };
                                if !native_entities.contains_key(owner_id) {
                                    return false;
                                }
                                let stems = [
                                    crate::ids::Stem::directory(sequence),
                                    crate::ids::Stem::word_directory(
                                        crate::ids::Word::BoundedPlane,
                                        sequence,
                                    ),
                                    crate::ids::Stem::word_directory(
                                        crate::ids::Word::LegacySingleParent,
                                        sequence,
                                    ),
                                ];
                                stems
                                    .iter()
                                    .any(|stem| named_bodies.contains(&crate::ids::body(stem)))
                            })
                    });
            if !represented {
                return None;
            }
            if let Some(source) = source {
                sources.insert(source);
            }
        }
    }
    if product_sources != application_sources {
        return None;
    }
    Some(product_sources)
}

fn reject_unsupported_native(ir: &CadIr) -> Result<Vec<LossNote>, CodecError> {
    let Some(namespace) = ir.native.namespace("iges") else {
        return Ok(Vec::new());
    };
    let represented_name_properties = represented_name_properties(ir, namespace);
    if let Some((arena, _)) = namespace.arenas().iter().find(|(arena, records)| {
        if records.is_empty() || ALLOWED_NATIVE_ARENAS.contains(&arena.as_str()) {
            return false;
        }
        if matches!(arena.as_str(), "product_properties" | "properties") {
            return represented_name_properties.is_none();
        }
        true
    }) {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer cannot preserve native arena {arena}"
        )));
    }
    if let Some(record) = namespace
        .arenas()
        .get("entities")
        .into_iter()
        .flatten()
        .find(|record| {
            let entity_type = record.field("entity_type").and_then(|value| value.as_i64());
            let name_property_represented = represented_name_properties
                .as_ref()
                .is_some_and(|properties| properties.contains(record.id()));
            (entity_type == Some(406) && !name_property_represented)
                || !matches!(
                    entity_type,
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
                            | 406
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
    let mut native_entities = namespace.arenas().get("entities").into_iter().flatten();
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
        let object_id = crate::entities::geometry::SourceObjectId::new(sequence).text();
        if !ir.model.curves.iter().any(|curve| {
            curve.source_object.as_ref().is_some_and(|source| {
                source.format == cadmpeg_ir::CodecFormat::Iges
                    && source.object_id.as_str() == object_id
            })
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
        let object_id = crate::entities::geometry::SourceObjectId::new(sequence).text();
        if !ir.model.surfaces.iter().any(|surface| {
            surface.source_object.as_ref().is_some_and(|source| {
                source.format == cadmpeg_ir::CodecFormat::Iges
                    && source.object_id.as_str() == object_id
            })
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
    for (arena, records) in namespace.arenas() {
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
    rows: [[FiniteReal; 4]; 3],
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
    match procedural
        .record_bounds()
        .map(cadmpeg_ir::geometry::RecordBounds::get)
    {
        None => {
            if matches!(
                geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
            ) && ir
                .model
                .edges
                .iter()
                .any(|edge| edge.curve() == Some(directrix))
            {
                curve_reference_span(ir, directrix, geometry).map(|span| span.range)
            } else {
                Ok(fallback)
            }
        }
        Some([Some(start), Some(end), _, _]) if start < end => Ok([start, end]),
        Some(_) => Err(CodecError::Malformed(
            "IGES procedural surface directrix bounds are invalid".into(),
        )),
    }
}

fn point_entity(position: FinitePoint3) -> Entity {
    point_entity_with_status(position, EntityStatus::Independent)
}

fn point_entity_with_status(position: FinitePoint3, status: EntityStatus) -> Entity {
    let [x, y, z] = position.coordinates();
    Entity {
        type_code: 116,
        form: 0,
        label: "POINT",
        status,
        parameter_body: format!("{},{},{};", number(x), number(y), number(z)).into_bytes(),
        transform: None,
    }
}

fn direction_entity(direction: Vector3) -> Result<Entity, CodecError> {
    let [x, y, z] = [
        finite(direction.x, "direction x component")?,
        finite(direction.y, "direction y component")?,
        finite(direction.z, "direction z component")?,
    ];
    Ok(Entity {
        type_code: 123,
        form: 0,
        label: "DIRECTN",
        status: EntityStatus::PhysicallyDependent,
        parameter_body: format!("{},{},{};", number(x), number(y), number(z)).into_bytes(),
        transform: None,
    })
}

fn pointer_surface_support(
    base_index: usize,
    location: FinitePoint3,
    frame: &OrthonormalFrame3,
) -> Result<(Vec<Entity>, usize, usize, usize), CodecError> {
    let (axis, reference) = orthonormal_pair(frame);
    let location_index = base_index;
    let axis_index = base_index
        .checked_add(1)
        .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
    let reference_index = base_index
        .checked_add(2)
        .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
    Ok((
        vec![
            point_entity_with_status(location, EntityStatus::PhysicallyDependent),
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

fn analytic_surface_family(geometry: &SolvedSurfaceGeometry) -> Option<AnalyticSurfaceFamily> {
    match geometry {
        SolvedSurfaceGeometry::Plane(_) => Some(AnalyticSurfaceFamily::Plane),
        SolvedSurfaceGeometry::Cylinder(_) => Some(AnalyticSurfaceFamily::Cylinder),
        SolvedSurfaceGeometry::Cone(_) => Some(AnalyticSurfaceFamily::Cone),
        SolvedSurfaceGeometry::Sphere(_) => Some(AnalyticSurfaceFamily::Sphere),
        SolvedSurfaceGeometry::Torus(_) => Some(AnalyticSurfaceFamily::Torus),
        SolvedSurfaceGeometry::Nurbs(_) => None,
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
    if let Some(geometry) = geometry.solved() {
        return surface_entities(geometry, base_index, version);
    }
    match geometry {
        SurfaceGeometry::Procedural { construction, .. } => {
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
                ProceduralSurfaceDefinition::Revolution(_) => {
                    revolution_surface_entities(ir, construction, base_index, version)
                }
                ProceduralSurfaceDefinition::Extrusion(_) => {
                    extrusion_surface_entities(ir, construction, base_index, version)
                }
                _ => Err(CodecError::NotImplemented(
                    "IGES semantic writer only encodes procedural Revolution and Extrusion surfaces as native entities".into(),
                )),
            }
        }
        SurfaceGeometry::Solved(geometry) => surface_entities(geometry, base_index, version),
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
    let ProceduralSurfaceDefinition::Extrusion(definition_payload) = procedural.definition() else {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer only encodes Extrusion surfaces as Type 122".into(),
        ));
    };
    let directrix = definition_payload.directrix();
    let parameter_interval = definition_payload
        .parameter_interval()
        .map(cadmpeg_ir::units::FiniteVector::get);
    let direction = definition_payload.direction();
    let native_position = definition_payload.native_position();
    let revision_form = definition_payload.revision_form();
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
    if start_parameter >= terminate_parameter {
        return Err(CodecError::Malformed(
            "IGES Type 122 directrix parameter interval is invalid".into(),
        ));
    }
    if direction.norm() <= 0.0 {
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
    let geometry = flatten_curve(source_curve.geometry.solved().ok_or_else(|| {
        CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
    })?)?;
    let carrier_interval = construction_carrier_interval(
        ir,
        directrix,
        &geometry,
        procedural,
        [start_parameter, terminate_parameter],
    )?;
    let (start, end) = if matches!(
        &geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
    ) {
        let span = curve_reference_span(ir, directrix, &geometry)?;
        if !same_range(span.range, [start_parameter, terminate_parameter]) {
            return Err(CodecError::NotImplemented(
                "IGES Type 122 composite directrix range is not its canonical Type 102 range"
                    .into(),
            ));
        }
        (
            admitted_point(span.start, "Type 122 directrix start")?,
            admitted_point(span.end, "Type 122 directrix terminate")?,
        )
    } else {
        let evaluation_interval = if matches!(
            &geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
        ) {
            carrier_interval
        } else {
            [start_parameter, terminate_parameter]
        };
        // A directrix end outside the finite range is refused as the composite
        // directrix's non-finite end is.
        let directrix_end = |parameter, label: &str| match curve_point(&geometry, parameter) {
            Ok(point) => Ok(point),
            Err(EvaluationFailure::NonFinite(_)) => Err(non_finite_point(label)),
            Err(EvaluationFailure::NoValue) => Err(CodecError::malformed(format_args!(
                "IGES {label} cannot be evaluated"
            ))),
        };
        (
            directrix_end(evaluation_interval[0], "Type 122 directrix start")?,
            directrix_end(evaluation_interval[1], "Type 122 directrix terminate")?,
        )
    };
    let inferred_target = admitted_point(
        start.translated(direction.get(), 1.0),
        "Type 122 inferred terminate point",
    )?;
    let target = native_position.unwrap_or(inferred_target);
    if !same_point(target.get(), inferred_target.get()) {
        return Err(CodecError::Malformed(
            "IGES Type 122 native terminate point disagrees with its sweep direction".into(),
        ));
    }

    let directrix_span = CurveSpan {
        range: [start_parameter, terminate_parameter],
        start: start.get(),
        end: end.get(),
    };
    let mut entities = Vec::new();
    let directrix_local_index = append_curve_entity(
        &mut entities,
        ir,
        CurveEntityRequest {
            version,
            curve_id: directrix,
            geometry: &geometry,
            span: Some(&directrix_span),
            sense: Sense::Forward,
            status: EntityStatus::PhysicallyDependent,
            reference_offset: base_index,
        },
    )?;
    let directrix_index = base_index
        .checked_add(directrix_local_index)
        .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
    let [target_x, target_y, target_z] = target.coordinates();
    entities.push(Entity {
        type_code: 122,
        form: 0,
        label: "TABULATE",
        status: EntityStatus::Independent,
        parameter_body: format!(
            "{},{},{},{};",
            reference_marker(directrix_index),
            number(target_x),
            number(target_y),
            number(target_z)
        )
        .into_bytes(),
        transform: None,
    });
    Ok(entities)
}

/// Class of the revolution angular interval that IGES Type 120 output carries.
///
/// A sweep within [`ANGULAR_TOLERANCE`] of a full turn is a full turn, and the
/// writer states it as `TAU`. Every other sweep is written as declared. A sweep
/// outside `(0, TAU + ANGULAR_TOLERANCE]` is not a Type 120 revolution.
enum RevolutionSweep {
    /// `0 < sweep < TAU`. The writer states this sweep unchanged.
    Partial(f64),
    /// `TAU <= sweep <= TAU + ANGULAR_TOLERANCE`. The writer states `TAU`.
    Full,
}

impl RevolutionSweep {
    /// Classifies one `angular_interval`, or refuses it. The admitted interval
    /// is finite and strictly increasing, so its sweep is positive; the sweep
    /// overflows only when the endpoints lie far apart.
    fn classify(angular_interval: IncreasingParameterInterval) -> Result<Self, CodecError> {
        let [start_angle, terminate_angle] = angular_interval.endpoints();
        let sweep = terminate_angle - start_angle;
        if !sweep.is_finite() {
            return Err(CodecError::InvalidInput(format!(
                "IGES Type 120 angular_interval [{start_angle}, {terminate_angle}] sweep overflows"
            )));
        }
        if sweep > TAU + ANGULAR_TOLERANCE {
            return Err(CodecError::InvalidInput(format!(
                "IGES Type 120 angular_interval sweep {sweep} is outside (0, {TAU} + {ANGULAR_TOLERANCE}]"
            )));
        }
        if sweep < TAU {
            Ok(Self::Partial(sweep))
        } else {
            Ok(Self::Full)
        }
    }

    /// Returns the terminate angle that the Type 120 record states.
    fn terminate_angle(&self, start_angle: f64) -> f64 {
        match self {
            Self::Partial(sweep) => start_angle + sweep,
            Self::Full => start_angle + TAU,
        }
    }
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
    let ProceduralSurfaceDefinition::Revolution(definition_payload) = procedural.definition()
    else {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer only encodes procedural Revolution surfaces as Type 120".into(),
        ));
    };
    let directrix = definition_payload.directrix();
    let axis_origin = definition_payload.axis_origin();
    let axis_direction = definition_payload.axis_direction();
    let angular_interval = definition_payload.angular_interval();
    let angular_parameter_interval = definition_payload.angular_parameter_interval();
    let parameter_interval = definition_payload
        .parameter_interval()
        .map(IncreasingParameterInterval::endpoints);
    let transposed = definition_payload.transposed();
    let revision_form = definition_payload.revision_form();
    if angular_parameter_interval.is_some() || *transposed || revision_form.is_some() {
        return Err(CodecError::NotImplemented(
            "IGES Type 120 output requires the default revolution parameterization".into(),
        ));
    }
    let start_angle = angular_interval.lower();
    let terminate_angle = RevolutionSweep::classify(angular_interval)?.terminate_angle(start_angle);
    let [start_parameter, terminate_parameter] = parameter_interval.ok_or_else(|| {
        CodecError::NotImplemented(
            "IGES Type 120 output requires a bounded generatrix parameter interval".into(),
        )
    })?;
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
    let geometry = flatten_curve(source_curve.geometry.solved().ok_or_else(|| {
        CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
    })?)?;
    let carrier_interval = construction_carrier_interval(
        ir,
        directrix,
        &geometry,
        procedural,
        [start_parameter, terminate_parameter],
    )?;
    let evaluation_interval = if matches!(
        &geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
    ) {
        carrier_interval
    } else {
        [start_parameter, terminate_parameter]
    };
    let start = curve_point(&geometry, evaluation_interval[0]).map_err(|_| {
        CodecError::Malformed("IGES Type 120 generatrix start cannot be evaluated".into())
    })?;
    let end = curve_point(&geometry, evaluation_interval[1]).map_err(|_| {
        CodecError::Malformed("IGES Type 120 generatrix terminate cannot be evaluated".into())
    })?;
    let axis_direction = axis_direction.to_unit_length();
    let axis_end = axis_origin.translated(*axis_direction.as_raw(), 1.0);
    let axis_geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::new(axis_origin, axis_direction),
    ));
    let axis_span = CurveSpan {
        range: [0.0, 1.0],
        start: *axis_origin,
        end: axis_end,
    };
    let generatrix_span = CurveSpan {
        range: [start_parameter, terminate_parameter],
        start: start.get(),
        end: end.get(),
    };
    let directrix_index = base_index
        .checked_add(1)
        .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
    Ok(vec![
        curve_entity(
            axis_geometry.solved().ok_or_else(|| {
                CodecError::NotImplemented("IGES carrier has no solved geometry".into())
            })?,
            Some(&axis_span),
            version,
        )?,
        curve_entity(
            geometry.solved().ok_or_else(|| {
                CodecError::NotImplemented("IGES carrier has no solved geometry".into())
            })?,
            Some(&generatrix_span),
            version,
        )?,
        Entity {
            type_code: 120,
            form: 0,
            label: "REVOLVE",
            status: EntityStatus::Independent,
            parameter_body: format!(
                "{},{},{},{};",
                reference_marker(base_index),
                reference_marker(directrix_index),
                number(finite(start_angle, "Type 120 start angle")?),
                number(finite(terminate_angle, "Type 120 terminate angle")?)
            )
            .into_bytes(),
            transform: None,
        },
    ])
}

fn surface_entities(
    geometry: &SolvedSurfaceGeometry,
    base_index: usize,
    version: crate::IgesVersion,
) -> Result<Vec<Entity>, CodecError> {
    let analytic_type_code =
        analytic_surface_family(geometry).map(AnalyticSurfaceFamily::type_code);
    match geometry {
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin().get();
            if matches!(version, crate::IgesVersion::V4_0 | crate::IgesVersion::V5_0) {
                let (normal, u_axis) = orthonormal_pair(plane_surface.frame());
                let v_axis = normal.cross(u_axis);
                return Ok(vec![Entity {
                    // Type 108 Form 0 is the unbounded plane carrier in the
                    // V4.0 and V5.0 profiles.  Keep the neutral plane frame
                    // in its exact local Z=0 form and apply the frame through
                    // the standard rigid Directory transformation.
                    type_code: 108,
                    form: 0,
                    label: "PLANE",
                    status: EntityStatus::Independent,
                    parameter_body: b"0,0,1,0,0,0,0,0,0;".to_vec(),
                    transform: Some(placement(origin, u_axis, v_axis)?),
                }]);
            }
            let (mut entities, location, axis, reference) =
                pointer_surface_support(base_index, plane_surface.origin(), plane_surface.frame())?;
            entities.push(Entity {
                type_code: analytic_type_code.ok_or_else(|| {
                    CodecError::Malformed("IGES plane has no analytic surface family".into())
                })?,
                form: 1,
                label: "PLANE",
                status: EntityStatus::PhysicallyDependent,
                parameter_body: format!(
                    "{},{},{};",
                    reference_marker(location),
                    reference_marker(axis),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            });
            Ok(entities)
        }
        SolvedSurfaceGeometry::Nurbs(nurbs) => Ok(vec![encode_nurbs_surface(nurbs)?]),
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let radius = cylinder_surface.radius().magnitude();
            let (mut entities, location, axis, reference) = pointer_surface_support(
                base_index,
                cylinder_surface.origin(),
                cylinder_surface.frame(),
            )?;
            let surface = Entity {
                type_code: analytic_type_code.ok_or_else(|| {
                    CodecError::Malformed("IGES cylinder has no analytic surface family".into())
                })?,
                form: 1,
                label: "CYLINDER",
                status: EntityStatus::Independent,
                parameter_body: format!(
                    "{},{},{},{};",
                    reference_marker(location),
                    reference_marker(axis),
                    number(radius),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            };
            entities.push(surface);
            Ok(entities)
        }
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            if !same_float(ratio, 1.0) {
                return Err(CodecError::NotImplemented(
                    "IGES analytic cone writer only encodes circular cones".into(),
                ));
            }
            if half_angle <= 0.0 || half_angle >= std::f64::consts::FRAC_PI_2 {
                return Err(CodecError::NotImplemented(
                    "IGES cone semi-angle must be in (0, 90) degrees".into(),
                ));
            }
            let (mut entities, location, axis, reference) =
                pointer_surface_support(base_index, cone_surface.origin(), cone_surface.frame())?;
            let surface = Entity {
                type_code: analytic_type_code.ok_or_else(|| {
                    CodecError::Malformed("IGES cone has no analytic surface family".into())
                })?,
                form: 1,
                label: "CONE",
                status: EntityStatus::Independent,
                parameter_body: format!(
                    "{},{},{},{},{};",
                    reference_marker(location),
                    reference_marker(axis),
                    number(finite(radius, "cone radius")?),
                    number(finite(
                        half_angle.to_degrees(),
                        "cone semi-angle in degrees"
                    )?),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            };
            entities.push(surface);
            Ok(entities)
        }
        SolvedSurfaceGeometry::Sphere(sphere_surface) => {
            let radius = sphere_surface.radius().get();
            if radius <= 0.0 {
                return Err(CodecError::NotImplemented(
                    "IGES sphere radius must be positive".into(),
                ));
            }
            let (mut entities, location, axis, reference) = pointer_surface_support(
                base_index,
                sphere_surface.center(),
                sphere_surface.frame(),
            )?;
            let surface = Entity {
                type_code: analytic_type_code.ok_or_else(|| {
                    CodecError::Malformed("IGES sphere has no analytic surface family".into())
                })?,
                form: 1,
                label: "SPHERE",
                status: EntityStatus::Independent,
                parameter_body: format!(
                    "{},{},{},{};",
                    reference_marker(location),
                    number(finite(radius, "sphere radius")?),
                    reference_marker(axis),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            };
            entities.push(surface);
            Ok(entities)
        }
        SolvedSurfaceGeometry::Torus(torus_surface) => {
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            if minor_radius <= 0.0 || minor_radius >= major_radius {
                return Err(CodecError::NotImplemented(
                    "IGES torus radii must satisfy 0 < minor < major".into(),
                ));
            }
            let (mut entities, location, axis, reference) =
                pointer_surface_support(base_index, torus_surface.center(), torus_surface.frame())?;
            let surface = Entity {
                type_code: analytic_type_code.ok_or_else(|| {
                    CodecError::Malformed("IGES torus has no analytic surface family".into())
                })?,
                form: 1,
                label: "TORUS",
                status: EntityStatus::Independent,
                parameter_body: format!(
                    "{},{},{},{},{};",
                    reference_marker(location),
                    reference_marker(axis),
                    number(finite(major_radius, "torus major radius")?),
                    number(finite(minor_radius, "torus minor radius")?),
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
    let u_count = nurbs.u_count();
    let v_count = nurbs.v_count();
    let u_degree = usize::try_from(nurbs.u_degree())
        .map_err(|_| CodecError::Malformed("IGES surface u degree overflows usize".into()))?;
    let v_degree = usize::try_from(nurbs.v_degree())
        .map_err(|_| CodecError::Malformed("IGES surface v degree overflows usize".into()))?;
    // The admitted rectangular pole grid already contains this many resident
    // elements, so its dimensions cannot overflow the host address space.
    let pole_count = u_count * v_count;
    let weights = match nurbs.pole_weights() {
        Some(values) => {
            if values.iter().any(|weight| weight.get() <= 0.0) {
                return Err(CodecError::NotImplemented(
                    "IGES NURBS surface weights must be finite and positive".into(),
                ));
            }
            values.into_iter().map(FiniteReal::from).collect()
        }
        None => alloc_filled(pole_count, FiniteReal::ONE, "iges NURBS surface weights")?,
    };
    let u_range = [nurbs.u_knots()[u_degree], nurbs.u_knots()[u_count]];
    let v_range = [nurbs.v_knots()[v_degree], nurbs.v_knots()[v_count]];
    if u_range[0] >= u_range[1] || v_range[0] >= v_range[1] {
        return Err(CodecError::NotImplemented(
            "IGES NURBS surface has an empty parameter domain".into(),
        ));
    }
    let closed_u = nurbs.u_periodic() || nurbs_surface_closed_u(nurbs, u_range, v_range);
    let closed_v = nurbs.v_periodic() || nurbs_surface_closed_v(nurbs, u_range, v_range);
    let mut parameters = format!(
        "{},{},{},{},{},{},{},{},{}",
        u_count - 1,
        v_count - 1,
        nurbs.u_degree(),
        nurbs.v_degree(),
        i32::from(closed_u),
        i32::from(closed_v),
        i32::from(nurbs.weights().is_none()),
        i32::from(nurbs.u_periodic()),
        i32::from(nurbs.v_periodic())
    );
    for value in nurbs.u_knots().iter().chain(nurbs.v_knots().iter()) {
        parameters.push(',');
        parameters.push_str(&number(finite(*value, "NURBS surface knot")?));
    }
    for v in 0..v_count {
        for u in 0..u_count {
            parameters.push(',');
            parameters.push_str(&number(weights[u * v_count + v]));
        }
    }
    for v in 0..v_count {
        for u in 0..u_count {
            for value in nurbs.control_grid()[u][v].coordinates() {
                parameters.push(',');
                parameters.push_str(&number(value));
            }
        }
    }
    for value in [u_range[0], u_range[1], v_range[0], v_range[1]] {
        parameters.push(',');
        parameters.push_str(&number(finite(value, "NURBS surface parameter bound")?));
    }
    parameters.push(';');
    Ok(Entity {
        type_code: 128,
        form: 0,
        label: "NURBS",
        status: EntityStatus::Independent,
        parameter_body: parameters.into_bytes(),
        transform: None,
    })
}

fn nurbs_surface_closed_u(nurbs: &NurbsSurface, u_range: [f64; 2], v_range: [f64; 2]) -> bool {
    [v_range[0], v_range[0].midpoint(v_range[1]), v_range[1]]
        .into_iter()
        .all(|v| {
            let Ok(start) = cadmpeg_ir::eval::nurbs_surface_point(nurbs, u_range[0], v) else {
                return false;
            };
            let Ok(end) = cadmpeg_ir::eval::nurbs_surface_point(nurbs, u_range[1], v) else {
                return false;
            };
            close_point(start.get(), end.get())
        })
}

fn nurbs_surface_closed_v(nurbs: &NurbsSurface, u_range: [f64; 2], v_range: [f64; 2]) -> bool {
    [u_range[0], u_range[0].midpoint(u_range[1]), u_range[1]]
        .into_iter()
        .all(|u| {
            let Ok(start) = cadmpeg_ir::eval::nurbs_surface_point(nurbs, u, v_range[0]) else {
                return false;
            };
            let Ok(end) = cadmpeg_ir::eval::nurbs_surface_point(nurbs, u, v_range[1]) else {
                return false;
            };
            close_point(start.get(), end.get())
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

fn point_position(ir: &CadIr, point_id: &PointId) -> Result<FinitePoint3, CodecError> {
    ir.model
        .points
        .iter()
        .find(|point| point.id == *point_id)
        .map(cadmpeg_ir::topology::Point::position)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES topology references missing point {point_id}"
            ))
        })
}

fn vertex_position(ir: &CadIr, vertex_id: &VertexId) -> Option<FinitePoint3> {
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
        .map(cadmpeg_ir::topology::Point::position)
}

#[derive(Clone, Copy)]
struct CurveEntityRequest<'a> {
    version: crate::IgesVersion,
    curve_id: &'a CurveId,
    geometry: &'a CurveGeometry,
    span: Option<&'a CurveSpan>,
    sense: Sense,
    status: EntityStatus,
    reference_offset: usize,
}

fn append_curve_entity(
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
        status: EntityStatus,
    ) -> Result<usize, CodecError> {
        if !self.active.insert(curve_id.clone()) {
            return Err(CodecError::malformed(format_args!(
                "IGES composite curve graph contains a cycle at {curve_id}"
            )));
        }
        let result = match geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. }) => {
                let children = self.append_composite_constituents(segments, sense)?;
                push_composite_entity_with_reference_offset(
                    self.entities,
                    &children,
                    "COMPOSIT",
                    status,
                    self.reference_offset,
                )
            }
            _ => {
                let mut entity = match sense {
                    Sense::Forward => curve_entity(
                        geometry.solved().ok_or_else(|| {
                            CodecError::NotImplemented("IGES carrier has no solved geometry".into())
                        })?,
                        span,
                        self.version,
                    )?,
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
            let child_geometry = flatten_curve(child.geometry.solved().ok_or_else(|| {
                CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
            })?)?;
            let child_sense = match (sense, segment.same_sense) {
                (Sense::Forward, true) | (Sense::Reversed, false) => Sense::Forward,
                (Sense::Forward, false) | (Sense::Reversed, true) => Sense::Reversed,
            };
            if let CurveGeometry::Solved(SolvedCurveGeometry::Composite {
                segments: nested, ..
            }) = &child_geometry
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
                    EntityStatus::PhysicallyDependent,
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
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. }) => {
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
                let child_geometry = flatten_curve(child.geometry.solved().ok_or_else(|| {
                    CodecError::NotImplemented("IGES curve carrier has no solved geometry".into())
                })?)?;
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
                .filter(|edge| edge.curve() == Some(curve_id))
                .collect::<Vec<_>>();
            match matching_edges.split_first() {
                None => Ok(CurveSpan {
                    range: derived_range,
                    start: derived_start,
                    end: derived_end,
                }),
                Some((first_edge, rest_edges)) => {
                    let endpoints = |edge: &Edge| -> Result<(Point3, Point3, f64), CodecError> {
                        if let Some(range) = edge.param_range() {
                            if !same_range(range.get(), derived_range) {
                                return Err(CodecError::NotImplemented(format!(
                                    "IGES composite curve {curve_id} has an edge parameter range that cannot be represented by Type 102"
                                )));
                            }
                        }
                        let start = point_position(ir, &vertex_point_id(ir, &edge.start)?)?.get();
                        let end = point_position(ir, &vertex_point_id(ir, &edge.end)?)?.get();
                        let edge_tolerance = edge_topology_tolerance(ir, edge)?;
                        if !close_point_with_tolerance(start, derived_start, edge_tolerance)
                            || !close_point_with_tolerance(end, derived_end, edge_tolerance)
                        {
                            return Err(CodecError::malformed(format_args!(
                                "IGES composite curve {curve_id} endpoints disagree with its child sequence"
                            )));
                        }
                        Ok((start, end, edge_tolerance))
                    };
                    let (start, end, mut tolerance) = endpoints(first_edge)?;
                    for edge in rest_edges {
                        let (edge_start, edge_end, edge_tolerance) = endpoints(edge)?;
                        tolerance = tolerance.max(edge_tolerance);
                        if !close_point_with_tolerance(edge_start, start, tolerance)
                            || !close_point_with_tolerance(edge_end, end, tolerance)
                        {
                            return Err(CodecError::NotImplemented(format!(
                                "IGES composite curve {curve_id} has ambiguous edge endpoints"
                            )));
                        }
                    }
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
                .filter(|edge| edge.curve() == Some(curve_id))
                .collect::<Vec<_>>();
            match matching_edges.split_first() {
                None => {
                    let range = default_range(geometry.solved().ok_or_else(|| {
                        CodecError::NotImplemented("IGES carrier has no solved geometry".into())
                    })?)?;
                    let start = curve_point(geometry, range[0]).map_err(|_| {
                        CodecError::NotImplemented(format!(
                            "IGES composite child curve {curve_id} has no evaluable start"
                        ))
                    })?;
                    let end = curve_point(geometry, range[1]).map_err(|_| {
                        CodecError::NotImplemented(format!(
                            "IGES composite child curve {curve_id} has no evaluable end"
                        ))
                    })?;
                    Ok(CurveSpan {
                        range,
                        start: start.get(),
                        end: end.get(),
                    })
                }
                Some((first_edge, rest_edges)) => {
                    if matching_edges
                        .iter()
                        .any(|edge| edge.param_range().is_none())
                    {
                        return Err(CodecError::NotImplemented(format!(
                            "IGES composite child curve {curve_id} requires a parameter range"
                        )));
                    }
                    let first = edge_span(ir, first_edge, geometry)?;
                    let rest_spans = rest_edges
                        .iter()
                        .map(|edge| edge_span(ir, edge, geometry))
                        .collect::<Result<Vec<_>, _>>()?;
                    let tolerance = matching_edges
                        .iter()
                        .map(|edge| edge_topology_tolerance(ir, edge))
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .fold(0.0, f64::max);
                    if rest_spans.iter().any(|span| {
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
    if let Some(SolvedCurveGeometry::Composite { segments, .. }) = curve.geometry.solved() {
        for segment in segments {
            mark_curve_descendants(ir, &segment.curve, consumed, active)?;
        }
    }
    active.remove(curve_id);
    Ok(())
}

fn edge_span(ir: &CadIr, edge: &Edge, geometry: &CurveGeometry) -> Result<CurveSpan, CodecError> {
    if matches!(
        geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
    ) {
        let curve_id = edge.curve().ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES composite edge {} has no curve reference",
                edge.id
            ))
        })?;
        return curve_reference_span(ir, curve_id, geometry);
    }
    let range = edge.param_range().ok_or_else(|| {
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
    let start = point_position(ir, &vertex_point_id(ir, &edge.start)?)?.get();
    let end = point_position(ir, &vertex_point_id(ir, &edge.end)?)?.get();
    if matches!(
        geometry,
        CurveGeometry::Solved(
            SolvedCurveGeometry::Circle(_)
                | SolvedCurveGeometry::Ellipse(_)
                | SolvedCurveGeometry::Parabola(_)
                | SolvedCurveGeometry::Hyperbola(_)
                | SolvedCurveGeometry::Nurbs(_)
                | SolvedCurveGeometry::Polyline(_)
        )
    ) {
        let evaluated_start = curve_point(geometry, range[0]).map_err(|_| {
            CodecError::malformed(format_args!(
                "IGES edge {} start cannot be evaluated on its curve",
                edge.id
            ))
        })?;
        let evaluated_end = curve_point(geometry, range[1]).map_err(|_| {
            CodecError::malformed(format_args!(
                "IGES edge {} end cannot be evaluated on its curve",
                edge.id
            ))
        })?;
        let tolerance = edge_topology_tolerance(ir, edge)?;
        if !close_point_with_tolerance(start, evaluated_start.get(), tolerance)
            || !close_point_with_tolerance(end, evaluated_end.get(), tolerance)
        {
            return Err(CodecError::malformed(format_args!(
                "IGES edge {} endpoints disagree with its curve parameter range",
                edge.id
            )));
        }
    }
    Ok(CurveSpan {
        range: range.get(),
        start,
        end,
    })
}

fn edge_topology_tolerance(ir: &CadIr, edge: &Edge) -> Result<f64, CodecError> {
    let mut tolerance = edge
        .tolerance
        .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get);
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
        tolerance = tolerance.max(
            vertex
                .tolerance
                .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
        );
    }
    Ok(tolerance)
}

fn default_range(geometry: &SolvedCurveGeometry) -> Result<[f64; 2], CodecError> {
    match geometry {
        SolvedCurveGeometry::Circle(_) => Ok([0.0, TAU]),
        SolvedCurveGeometry::Ellipse(_) => Ok([0.0, TAU]),
        SolvedCurveGeometry::Nurbs(nurbs) => nurbs_domain(nurbs).map(FiniteReal::raw_array),
        SolvedCurveGeometry::Polyline(polyline) => {
            let values = polyline_parameters(polyline)?;
            Ok([values.first, values.last])
        }
        SolvedCurveGeometry::Line(_) => Err(CodecError::NotImplemented(
            "IGES semantic writer requires a finite curve parameter range".into(),
        )),
        SolvedCurveGeometry::Parabola(_) => Err(CodecError::NotImplemented(
            "IGES semantic writer requires a finite curve parameter range".into(),
        )),
        SolvedCurveGeometry::Hyperbola(_) => Err(CodecError::NotImplemented(
            "IGES semantic writer requires a finite curve parameter range".into(),
        )),
        SolvedCurveGeometry::Degenerate(_) => Err(CodecError::NotImplemented(
            "IGES semantic writer requires a finite curve parameter range".into(),
        )),
        SolvedCurveGeometry::Composite { .. } => Err(CodecError::NotImplemented(
            "IGES semantic writer requires a finite curve parameter range".into(),
        )),
        SolvedCurveGeometry::Unknown { .. } => Err(CodecError::NotImplemented(
            "IGES semantic writer requires a finite curve parameter range".into(),
        )),
        SolvedCurveGeometry::Transformed(_) => Err(CodecError::Malformed(
            "IGES transformed curve was not flattened before encoding".into(),
        )),
    }
}

// A common power-of-two factor leaves the conic equation unchanged.
fn conic_coefficients(major: f64, minor: f64) -> Result<[FiniteReal; 3], CodecError> {
    let nonzero = |value: Option<FiniteReal>| value.filter(|value| value.get() != 0.0);
    let ordinary = [1.0 / (major * major), 1.0 / (minor * minor), -1.0]
        .map(|value| nonzero(FiniteReal::new(value)));
    if let [Some(a), Some(c), Some(f)] = ordinary {
        return Ok([a, c, f]);
    }
    let split = |radius: f64| {
        let exponent = radius.log2().floor() as i32;
        let half = exponent / 2;
        let mantissa = (radius * 2.0_f64.powi(-half)) * 2.0_f64.powi(half - exponent);
        (mantissa, exponent)
    };
    let (a, a_exponent) = split(major);
    let (b, b_exponent) = split(minor);
    let exponents = [-2 * a_exponent, -2 * b_exponent, 0];
    let minimum = exponents.into_iter().min().unwrap_or(0);
    let maximum = exponents.into_iter().max().unwrap_or(0);
    let normal_bounds = (-1022 - minimum, 1023 - maximum);
    let (lower, upper) = if normal_bounds.0 <= normal_bounds.1 {
        normal_bounds
    } else {
        (-1074 - minimum, 1023 - maximum)
    };
    if lower > upper {
        return Err(CodecError::NotImplemented(
            "IGES conic coefficient range is not representable".into(),
        ));
    }
    // Center the coefficient exponents so subnormal rounding cannot discard
    // the radius significand merely because zero was the preferred shift.
    // `lower..=upper` states every shift that keeps all three coefficients
    // inside the `f64` exponent range, and the refusal above states that it is
    // not empty. A centered shift outside that interval is not an error: the
    // nearest admitted shift is then the one that leaves the most significand.
    let shift = (-(minimum + maximum) / 2).clamp(lower, upper);
    let scale = cadmpeg_ir::math::scale_power_of_two;
    let coefficients = [
        scale(1.0 / (a * a), shift + exponents[0]),
        scale(1.0 / (b * b), shift + exponents[1]),
        scale(-1.0, shift),
    ];
    let [Some(a), Some(c), Some(f)] = coefficients.map(nonzero) else {
        return Err(CodecError::NotImplemented(
            "IGES conic coefficient range is not representable".into(),
        ));
    };
    Ok([a, c, f])
}

fn curve_entity(
    geometry: &SolvedCurveGeometry,
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
        SolvedCurveGeometry::Line(_) => {
            let span = span.ok_or_else(|| {
                CodecError::NotImplemented(
                    "IGES semantic writer cannot bound an unreferenced line".into(),
                )
            })?;
            let [start_x, start_y, start_z] =
                admitted_point(span.start, "line start")?.coordinates();
            let [end_x, end_y, end_z] = admitted_point(span.end, "line end")?.coordinates();
            Ok(Entity {
                type_code: 110,
                form: 0,
                label: "LINE",
                status: EntityStatus::Independent,
                parameter_body: format!(
                    "{},{},{},{},{},{};",
                    number(start_x),
                    number(start_y),
                    number(start_z),
                    number(end_x),
                    number(end_y),
                    number(end_z)
                )
                .into_bytes(),
                transform: None,
            })
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center().get();
            let radius = circle_curve.radius().get();
            let (axis, reference) = orthonormal_pair(circle_curve.frame());
            let y_axis = axis.cross(reference);
            validate_arc_sweep(range)?;
            let start_xy = [
                finite(radius * range[0].cos(), "arc start x")?,
                finite(radius * range[0].sin(), "arc start y")?,
            ];
            let end_xy = if is_full_arc(span) {
                start_xy
            } else {
                [
                    finite(radius * range[1].cos(), "arc terminate x")?,
                    finite(radius * range[1].sin(), "arc terminate y")?,
                ]
            };
            Ok(Entity {
                type_code: 100,
                form: 0,
                label: "ARC",
                status: EntityStatus::Independent,
                parameter_body: format!(
                    "0,0,0,{},{},{},{};",
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(center, reference, y_axis)?),
            })
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center().get();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            let (axis, major) = orthonormal_pair(ellipse_curve.frame());
            let y_axis = axis.cross(major);
            validate_arc_sweep(range)?;
            let start_xy = [
                finite(major_radius * range[0].cos(), "ellipse start x")?,
                finite(minor_radius * range[0].sin(), "ellipse start y")?,
            ];
            let end_xy = if is_full_arc(span) {
                start_xy
            } else {
                [
                    finite(major_radius * range[1].cos(), "ellipse terminate x")?,
                    finite(minor_radius * range[1].sin(), "ellipse terminate y")?,
                ]
            };
            // V5.0 identifies the coefficient-defined ellipse as Form 1.  The
            // Parameter Data is identical to the compatibility Form 0 used by
            // V4.0 and V5.1 through V5.3.
            let form = i64::from(version == crate::IgesVersion::V5_0);
            let coefficients = conic_coefficients(major_radius, minor_radius)?;
            Ok(Entity {
                type_code: 104,
                form,
                label: "CONIC",
                status: EntityStatus::Independent,
                parameter_body: format!(
                    "{},0,{},0,0,{},0,{},{},{},{};",
                    number(coefficients[0]),
                    number(coefficients[1]),
                    if coefficients[2].get() == -1.0 {
                        "-1".to_owned()
                    } else {
                        number(coefficients[2])
                    },
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(center, major, y_axis)?),
            })
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let vertex = parabola_curve.vertex().get();
            let focal_distance = parabola_curve.focal_distance().get();
            if range[0] == range[1] {
                return Err(CodecError::NotImplemented(
                    "IGES parabola requires a finite non-zero parameter span".into(),
                ));
            }
            let (axis, major) = orthonormal_pair(parabola_curve.frame());
            let x_axis = major.cross(axis);
            let start_xy = parabola_point(focal_distance, range[0])?;
            let end_xy = parabola_point(focal_distance, range[1])?;
            Ok(Entity {
                type_code: 104,
                form: 3,
                label: "CONIC",
                status: EntityStatus::Independent,
                parameter_body: format!(
                    "1,0,0,0,{},0,0,{},{},{},{};",
                    number(finite(-4.0 * focal_distance, "parabola coefficient -4f")?),
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(vertex, x_axis, major)?),
            })
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let center = hyperbola_curve.center().get();
            let major_radius = hyperbola_curve.major_radius().get();
            let minor_radius = hyperbola_curve.minor_radius().get();
            if range[0] == range[1] {
                return Err(CodecError::NotImplemented(
                    "IGES hyperbola requires a finite non-zero parameter span".into(),
                ));
            }
            let (axis, major) = orthonormal_pair(hyperbola_curve.frame());
            let y_axis = axis.cross(major);
            let radius_scales = [
                hyperbola_curve.major_radius().magnitude(),
                Length::from(hyperbola_curve.minor_radius()).magnitude(),
            ];
            let start_xy = hyperbola_point(radius_scales[0], radius_scales[1], range[0])?;
            let end_xy = hyperbola_point(radius_scales[0], radius_scales[1], range[1])?;
            let coefficients = conic_coefficients(major_radius, minor_radius)?;
            Ok(Entity {
                type_code: 104,
                form: 2,
                label: "CONIC",
                status: EntityStatus::Independent,
                parameter_body: format!(
                    "{},0,{},0,0,{},0,{},{},{},{};",
                    number(coefficients[0]),
                    number(coefficients[1].negated()),
                    if coefficients[2].get() == -1.0 {
                        "-1".to_owned()
                    } else {
                        number(coefficients[2])
                    },
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(center, major, y_axis)?),
            })
        }
        SolvedCurveGeometry::Nurbs(nurbs) => encode_nurbs(nurbs, range, "NURBS"),
        SolvedCurveGeometry::Polyline(polyline) => {
            let values = polyline_parameters(polyline)?;
            let nurbs = NurbsCurve::from_lanes(
                1,
                polyline_knots(&values),
                polyline.points().collect(),
                None,
                false,
            )
            .map_err(|error| CodecError::malformed(format_args!("polyline: {error}")))?;
            encode_nurbs(&nurbs, range, "POLYLINE")
        }
        SolvedCurveGeometry::Degenerate(_) => Err(CodecError::NotImplemented(
            "IGES semantic writer does not encode this curve geometry".into(),
        )),
        SolvedCurveGeometry::Composite { .. } => Err(CodecError::NotImplemented(
            "IGES semantic writer does not encode this curve geometry".into(),
        )),
        SolvedCurveGeometry::Unknown { .. } => Err(CodecError::NotImplemented(
            "IGES semantic writer does not encode this curve geometry".into(),
        )),
        SolvedCurveGeometry::Transformed(_) => Err(CodecError::NotImplemented(
            "IGES semantic writer does not encode this curve geometry".into(),
        )),
    }
}

fn encode_nurbs(
    nurbs: &NurbsCurve,
    range: [f64; 2],
    label: &'static str,
) -> Result<Entity, CodecError> {
    let control_count = nurbs.control_points().len();
    let degree = usize::try_from(nurbs.degree())
        .map_err(|_| CodecError::Malformed("IGES NURBS degree overflows usize".into()))?;
    if range[0] > range[1] || range.iter().any(|value| !value.is_finite()) {
        return Err(CodecError::Malformed(
            "IGES NURBS parameter range is invalid".into(),
        ));
    }
    let domain = [nurbs.knots()[degree], nurbs.knots()[control_count]];
    if range[0] < domain[0] || range[1] > domain[1] {
        return Err(CodecError::Malformed(
            "IGES NURBS parameter range lies outside its knot domain".into(),
        ));
    }
    let weights = match nurbs.weights() {
        Some(weights) => {
            if weights.iter().any(|weight| weight.get() <= 0.0) {
                return Err(CodecError::NotImplemented(
                    "IGES NURBS weights must be finite and positive".into(),
                ));
            }
            weights.into_iter().map(FiniteReal::from).collect()
        }
        None => alloc_filled(control_count, FiniteReal::ONE, "iges NURBS weights")?,
    };
    let polynomial = weights
        .first()
        .is_some_and(|first| weights.iter().all(|weight| weight == first));
    let control_points = nurbs.pole_rows().raw_points();
    let plane_normal = nurbs_plane_normal(&control_points);
    let planar = plane_normal.is_some();
    let closed = nurbs_is_closed(nurbs, domain);
    let k = control_count - 1;
    let mut parameters = format!(
        "{k},{},{},{},{},{}",
        nurbs.degree(),
        i32::from(planar),
        i32::from(closed),
        i32::from(polynomial),
        i32::from(nurbs.periodic())
    );
    for value in nurbs.knots() {
        parameters.push(',');
        parameters.push_str(&number(finite(*value, "NURBS knot")?));
    }
    for weight in weights {
        parameters.push(',');
        parameters.push_str(&number(weight));
    }
    for point in nurbs.control_points() {
        for value in point.coordinates() {
            parameters.push(',');
            parameters.push_str(&number(value));
        }
    }
    for value in range {
        parameters.push(',');
        parameters.push_str(&number(finite(value, "NURBS parameter bound")?));
    }
    let normal = plane_normal.unwrap_or(Vector3::new(0.0, 0.0, 0.0));
    for value in [normal.x, normal.y, normal.z] {
        parameters.push(',');
        parameters.push_str(&number(finite(value, "NURBS plane normal component")?));
    }
    parameters.push(';');
    let status = if label == "PCURVE" {
        EntityStatus::ParameterCurve
    } else {
        EntityStatus::Independent
    };
    Ok(Entity {
        type_code: 126,
        form: 0,
        label,
        status,
        parameter_body: parameters.into_bytes(),
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

fn nurbs_is_closed(nurbs: &NurbsCurve, domain: [f64; 2]) -> bool {
    let Ok(start) = cadmpeg_ir::eval::nurbs_curve_point_at(nurbs, domain[0]) else {
        return false;
    };
    let Ok(end) = cadmpeg_ir::eval::nurbs_curve_point_at(nurbs, domain[1]) else {
        return false;
    };
    let scale = nurbs
        .control_points()
        .iter()
        .map(|point| point.distance(start.get()))
        .filter(|distance| distance.is_finite())
        .fold(1.0, f64::max);
    start.distance(end.get()) <= NURBS_CLOSEDNESS_TOLERANCE * scale
}

fn flatten_curve(geometry: &SolvedCurveGeometry) -> Result<CurveGeometry, CodecError> {
    match geometry {
        SolvedCurveGeometry::Transformed(placed) => {
            let transform = placed.transform();
            if !transform.is_proper_rigid() {
                return Err(CodecError::NotImplemented(
                    "IGES semantic writer only applies proper-rigid curve transforms".into(),
                ));
            }
            let basis = flatten_curve(placed.basis())?;
            apply_rigid_transform(basis, *transform)
        }
        _ => Ok(CurveGeometry::Solved(geometry.clone())),
    }
}

fn apply_rigid_transform(
    geometry: CurveGeometry,
    transform: cadmpeg_ir::transform::Transform,
) -> Result<CurveGeometry, CodecError> {
    let point = |value: FinitePoint3| -> Result<FinitePoint3, CodecError> {
        transform.apply_point(value.get()).ok_or_else(|| {
            CodecError::malformed("transformed curve point has a non-finite coordinate")
        })
    };
    let vector = |value: Vector3, label: &str| -> Result<UnitVector3, CodecError> {
        let placed = transform.apply_vector(value).ok_or_else(|| {
            CodecError::malformed(format_args!("IGES {label} has a non-finite component"))
        })?;
        UnitVector3::normalized(placed.get())
            .ok_or_else(|| CodecError::malformed(format_args!("IGES {label} is degenerate")))
    };
    Ok(match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let direction = *line_curve.direction().as_raw();
            CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::new(
                    point(line_curve.origin())?,
                    vector(direction, "transformed line direction")?,
                ),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let center = point(circle_curve.center())?;
            let frame = OrthonormalFrame3::from_units(
                vector(
                    *circle_curve.frame().axis().as_raw(),
                    "transformed circle axis",
                )?,
                vector(
                    *circle_curve.frame().reference().as_raw(),
                    "transformed circle reference",
                )?,
            )
            .ok_or_else(|| {
                CodecError::malformed(
                    "CircleCurve.axis/ref_direction must form an orthonormal frame",
                )
            })?;
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(
                    center,
                    frame,
                    circle_curve.radius(),
                ),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let center = point(ellipse_curve.center())?;
            let frame = OrthonormalFrame3::from_units(
                vector(
                    *ellipse_curve.frame().axis().as_raw(),
                    "transformed ellipse axis",
                )?,
                vector(
                    *ellipse_curve.frame().reference().as_raw(),
                    "transformed ellipse major",
                )?,
            )
            .ok_or_else(|| {
                CodecError::malformed(
                    "EllipseCurve.axis/major_direction must form an orthonormal frame",
                )
            })?;
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                cadmpeg_ir::geometry::analytic::EllipseCurve::try_from_parts(
                    center,
                    frame,
                    ellipse_curve.major_radius(),
                    ellipse_curve.minor_radius(),
                )
                .map_err(CodecError::malformed)?,
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Parabola(parabola_curve)) => {
            let vertex = point(parabola_curve.vertex())?;
            let frame = OrthonormalFrame3::from_units(
                vector(
                    *parabola_curve.frame().axis().as_raw(),
                    "transformed parabola axis",
                )?,
                vector(
                    *parabola_curve.frame().reference().as_raw(),
                    "transformed parabola major",
                )?,
            )
            .ok_or_else(|| {
                CodecError::malformed(
                    "ParabolaCurve.axis/major_direction must form an orthonormal frame",
                )
            })?;
            CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
                cadmpeg_ir::geometry::analytic::ParabolaCurve::new(
                    vertex,
                    frame,
                    parabola_curve.focal_distance(),
                ),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(hyperbola_curve)) => {
            let center = point(hyperbola_curve.center())?;
            let frame = OrthonormalFrame3::from_units(
                vector(
                    *hyperbola_curve.frame().axis().as_raw(),
                    "transformed hyperbola axis",
                )?,
                vector(
                    *hyperbola_curve.frame().reference().as_raw(),
                    "transformed hyperbola major",
                )?,
            )
            .ok_or_else(|| {
                CodecError::malformed(
                    "HyperbolaCurve.axis/major_direction must form an orthonormal frame",
                )
            })?;
            CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
                cadmpeg_ir::geometry::analytic::HyperbolaCurve::new(
                    center,
                    frame,
                    hyperbola_curve.major_radius(),
                    hyperbola_curve.minor_radius(),
                ),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate_curve)) => {
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
                cadmpeg_ir::geometry::analytic::DegenerateCurve::new(point(
                    degenerate_curve.point(),
                )?),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(mut nurbs)) => {
            nurbs
                .map_control_points(|control_point| {
                    transform.apply_point(control_point.get()).ok_or_else(|| {
                        NurbsError::EditRefused(
                            "transformed NURBS curve control point has a non-finite coordinate"
                                .to_string(),
                        )
                    })
                })
                .map_err(|error| match error {
                    NurbsError::EditRefused(message) => CodecError::malformed(message),
                    error => {
                        CodecError::malformed(format_args!("transformed NURBS curve: {error}"))
                    }
                })?;
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Polyline(mut polyline)) => {
            polyline
                .edit_samples(|samples| {
                    samples.edit_points(|sample| {
                        *sample = transform
                            .apply_point(*sample)
                            .ok_or_else(|| {
                                GeometryLayoutError::EditRefused(
                                    "transformed polyline sample has a non-finite coordinate"
                                        .to_string(),
                                )
                            })?
                            .get();
                        Ok(())
                    })
                })
                .map_err(|error| CodecError::malformed(error.to_string()))?;
            CurveGeometry::Solved(SolvedCurveGeometry::Polyline(polyline))
        }
        other => {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer cannot flatten curve geometry {other:?}"
            )))
        }
    })
}

/// The axis of `frame` at unit length, and the reference minus its component
/// along that axis, divided by its length.
///
/// IGES reads a Type 124 matrix at its printed precision of 17 significant
/// digits, which is finer than the `1e-9` unit-length and perpendicularity
/// tolerance of the frame. The projected reference has a length within about
/// `2e-9` of one, so the division is by a finite nonzero length, and the two
/// results are unit length and perpendicular to rounding.
fn orthonormal_pair(frame: &OrthonormalFrame3) -> (Vector3, Vector3) {
    let primary = *frame.axis().to_unit_length().as_raw();
    let reference = *frame.reference().as_raw();
    let projected = reference - primary.scale(primary.dot(reference));
    let length = projected.norm();
    (
        primary,
        Vector3::new(
            projected.x / length,
            projected.y / length,
            projected.z / length,
        ),
    )
}

/// The Type 124 matrix of a right-handed frame placed at `origin`: the x axis
/// divided by its length, the y axis minus its component along x divided by
/// its length, and their cross product divided by its length.
///
/// Every caller passes perpendicular directions of unit length to rounding,
/// one of them a cross product, so each length is within rounding of one. The
/// divisions give the unit columns that a reader checks at the printed
/// precision of 17 significant digits.
fn placement(origin: Point3, x_axis: Vector3, y_axis: Vector3) -> Result<Placement, CodecError> {
    let origin = admitted_point(origin, "placement origin")?.coordinates();
    let divided = |vector: Vector3| {
        let length = vector.norm();
        Vector3::new(vector.x / length, vector.y / length, vector.z / length)
    };
    let x_axis = divided(x_axis);
    let y_axis = divided(y_axis - x_axis.scale(x_axis.dot(y_axis)));
    let z_axis = divided(x_axis.cross(y_axis));
    let column = |axis: Vector3, field: &str| {
        cadmpeg_ir::features::FiniteVector3::new(axis)
            .map(cadmpeg_ir::features::FiniteVector3::components)
            .ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "IGES writer computed the non-finite placement {field} {axis:?}, which \
                     IGES cannot state"
                ))
            })
    };
    let [x_axis, y_axis, z_axis] = [
        column(x_axis, "x axis")?,
        column(y_axis, "y axis")?,
        column(z_axis, "z axis")?,
    ];
    Ok(Placement {
        rows: std::array::from_fn(|row| [x_axis[row], y_axis[row], z_axis[row], origin[row]]),
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

fn parabola_point(focal_distance: f64, parameter: f64) -> Result<[FiniteReal; 2], CodecError> {
    let point = [
        FiniteReal::new(-2.0 * focal_distance * parameter),
        FiniteReal::new(focal_distance * parameter * parameter),
    ];
    let [Some(x), Some(y)] = point else {
        return Err(CodecError::NotImplemented(
            "IGES parabola endpoint is non-finite".into(),
        ));
    };
    Ok([x, y])
}

fn hyperbola_point(
    major_radius: FiniteReal,
    minor_radius: FiniteReal,
    parameter: f64,
) -> Result<[FiniteReal; 2], CodecError> {
    (|| {
        let parameter = FiniteReal::new(parameter)?;
        let (_, major_cosh) = cadmpeg_ir::math::scaled_sinh_cosh(major_radius, parameter).ok()?;
        let minor_sinh = match cadmpeg_ir::math::scaled_sinh_cosh(minor_radius, parameter) {
            Ok((sinh, _)) => Some(sinh),
            Err((sinh, _)) => FiniteReal::new(sinh),
        }?;
        Some([major_cosh, minor_sinh])
    })()
    .ok_or_else(|| CodecError::NotImplemented("IGES hyperbola endpoint is non-finite".into()))
}

fn nurbs_domain(nurbs: &NurbsCurve) -> Result<[FiniteReal; 2], CodecError> {
    let degree = usize::try_from(nurbs.degree())
        .map_err(|_| CodecError::Malformed("IGES NURBS degree overflows usize".into()))?;
    let end = nurbs.control_points().len();
    let knots = nurbs.knots().finite_knots().collect::<Vec<_>>();
    Ok([knots[degree], knots[end]])
}

struct PolylineParameters {
    first: f64,
    interior: Vec<f64>,
    last: f64,
}

fn polyline_parameters(polyline: &PolylineCurve) -> Result<PolylineParameters, CodecError> {
    let count = polyline.point_count();
    let values: Vec<f64> = polyline.parameters().map_or_else(
        || (0..count).map(|value| value as f64).collect(),
        |parameters| {
            parameters
                .map(cadmpeg_ir::scalar::FiniteReal::get)
                .collect()
        },
    );
    if !values.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(CodecError::NotImplemented(
            "IGES polyline parameters must be finite and strictly increasing".into(),
        ));
    }
    Ok(PolylineParameters {
        first: values[0],
        interior: values[1..count - 1].to_vec(),
        last: values[count - 1],
    })
}

fn polyline_knots(parameters: &PolylineParameters) -> Vec<f64> {
    let mut knots = Vec::with_capacity(parameters.interior.len() + 4);
    knots.extend([parameters.first; 2]);
    knots.extend_from_slice(&parameters.interior);
    knots.extend([parameters.last; 2]);
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

fn admitted_point(point: Point3, label: &str) -> Result<FinitePoint3, CodecError> {
    FinitePoint3::new(point).ok_or_else(|| non_finite_point(label))
}

/// The refusal of a point `label` whose coordinates are not finite.
fn non_finite_point(label: &str) -> CodecError {
    CodecError::malformed(format_args!(
        "IGES point {label} has non-finite coordinates"
    ))
}

#[derive(Clone)]
struct Entity {
    type_code: u32,
    form: i64,
    label: &'static str,
    status: EntityStatus,
    parameter_body: Vec<u8>,
    transform: Option<Placement>,
}

impl Entity {
    fn parameter_text(&self) -> Vec<u8> {
        let mut text = format!("{},", self.type_code).into_bytes();
        text.extend_from_slice(&self.parameter_body);
        text
    }
}

fn encode_file(
    entities: &[Entity],
    body_presentations: &BTreeMap<usize, BodyPresentation>,
    version: crate::IgesVersion,
    minimum_resolution: f64,
) -> Result<Vec<u8>, CodecError> {
    let generation_timestamp = generation_timestamp(SystemTime::now(), version)?;
    let maximum_coordinate = generated_maximum_coordinate(entities);
    let global = generated_global(
        version,
        &generation_timestamp,
        finite(minimum_resolution, "Global minimum resolution")?,
        finite(maximum_coordinate, "Global maximum coordinate")?,
    );
    let global_cards = crate::global::layout_global_cards(&global)?;
    let global_count = global_cards.len();
    let mut expanded = Vec::with_capacity(entities.len() * 2);
    let mut expanded_index_by_entity = Vec::with_capacity(entities.len());
    for (index, entity) in entities.iter().enumerate() {
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
                    status: EntityStatus::Independent,
                    parameter_body: format!("{transform_parameters};").into_bytes(),
                    transform: None,
                },
                0_u32,
                None,
            ));
            let transform_sequence = u32::try_from(expanded.len())
                .ok()
                .and_then(|value| value.checked_mul(2).and_then(|value| value.checked_sub(1)))
                .ok_or_else(|| {
                    CodecError::NotImplemented("IGES transformation sequence overflows".into())
                })?;
            let mut entity = entity.clone();
            entity.transform = None;
            expanded_index_by_entity.push(expanded.len());
            expanded.push((entity, transform_sequence, body_presentations.get(&index)));
        } else {
            expanded_index_by_entity.push(expanded.len());
            expanded.push((entity.clone(), 0, body_presentations.get(&index)));
        }
    }
    let mut parameter_sequence = 1_u32;
    let directory_capacity = expanded
        .len()
        .checked_mul(2)
        .ok_or_else(|| CodecError::NotImplemented("IGES directory count overflows".into()))?;
    let directory_count = u32::try_from(directory_capacity)
        .map_err(|_| CodecError::NotImplemented("IGES directory count overflows".into()))?;
    let mut directory = Vec::with_capacity(directory_capacity);
    let mut parameters = Vec::new();
    for (index, (entity, transform_sequence, presentation)) in expanded.iter().enumerate() {
        let directory_sequence = u32::try_from(index)
            .ok()
            .and_then(|value| value.checked_mul(2))
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| {
                CodecError::NotImplemented("IGES directory sequence overflows".into())
            })?;
        let fragments = crate::parameter::layout_parameter_cards(&entity.parameter_text())?;
        let parameter_count = fragments.len();
        let parameter_count = u32::try_from(parameter_count)
            .map_err(|_| CodecError::NotImplemented("IGES parameter count overflows".into()))?;
        let color_number = match presentation.map(|presentation| presentation.color) {
            None | Some(BodyColor::Standard(0)) => 0,
            Some(BodyColor::Standard(number)) => number,
            Some(BodyColor::Custom(_)) => {
                return Err(CodecError::NotImplemented(
                    "IGES color definition was not emitted".into(),
                ));
            }
            Some(BodyColor::Definition(entity_index)) => {
                let expanded_index =
                    expanded_index_by_entity.get(entity_index).ok_or_else(|| {
                        CodecError::NotImplemented(
                            "IGES color definition reference is outside emitted entities".into(),
                        )
                    })?;
                let sequence = u32::try_from(*expanded_index)
                    .ok()
                    .and_then(|index| index.checked_mul(2))
                    .and_then(|index| index.checked_add(1))
                    .ok_or_else(|| {
                        CodecError::NotImplemented(
                            "IGES color definition sequence overflows".into(),
                        )
                    })?;
                -i64::from(sequence)
            }
        };
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
                match presentation.and_then(|presentation| presentation.visible) {
                    Some(false) => format!("01{}", &entity.status.as_field()[2..]),
                    _ => entity.status.as_field().into(),
                },
            ],
            directory_sequence,
        )?);
        directory.push(directory_card(
            [
                entity.type_code.to_string(),
                "0".into(),
                color_number.to_string(),
                parameter_count.to_string(),
                entity.form.to_string(),
                String::new(),
                String::new(),
                presentation
                    .and_then(|presentation| presentation.label.as_deref())
                    .unwrap_or(entity.label)
                    .to_owned(),
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
            parameter_sequence = parameter_sequence.checked_add(1).ok_or_else(|| {
                CodecError::NotImplemented("IGES parameter sequence overflows".into())
            })?;
        }
    }
    let mut bytes = Vec::new();
    bytes.extend(card(b"Generated by cadmpeg", b'S', 1)?);
    for (index, chunk) in global_cards.iter().enumerate() {
        let sequence = u32::try_from(index + 1)
            .map_err(|_| CodecError::NotImplemented("IGES global sequence overflows".into()))?;
        bytes.extend(card(chunk, b'G', sequence)?);
    }
    for card_bytes in directory {
        bytes.extend(card_bytes);
    }
    for card_bytes in parameters {
        bytes.extend(card_bytes);
    }
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

/// Renders one IGES global Hollerith field.
///
/// Each caller passes an ASCII writer constant, an empty string, or the
/// generated timestamp, which holds digits and one period. The byte count of
/// each is therefore the character count that the Hollerith prefix states.
fn global_hollerith(value: &str) -> String {
    format!("{}H{value}", value.len())
}

fn generated_global(
    version: crate::IgesVersion,
    generation_timestamp: &str,
    minimum_resolution: FiniteReal,
    maximum_coordinate: FiniteReal,
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
    // A partial maximum cannot bound the full file. Use the Global field's
    // default zero when any entity has no bound.
    entities
        .iter()
        .try_fold(0.0_f64, |bound, entity| {
            generated_entity_coordinate_bound(entity).map(|value| bound.max(value))
        })
        .unwrap_or(0.0)
}

fn generated_entity_coordinate_bound(entity: &Entity) -> Option<f64> {
    if entity.type_code == 406 {
        return Some(0.0);
    }
    if entity.transform.is_some() {
        return None;
    }
    let values = entity
        .parameter_body
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
        110 => values.get(0..6)?,
        116 => values.get(0..3)?,
        502 => values.get(1..)?,
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
        payload.extend_from_slice(&crate::directory::render_field(field.as_bytes())?);
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
    if pointer.len() > 8 {
        return Err(CodecError::NotImplemented(
            "IGES directory sequence overflows".into(),
        ));
    }
    payload[64..].copy_from_slice(pointer.as_bytes());
    card(&payload, b'P', sequence)
}

fn card(data: &[u8], section: u8, sequence: u32) -> Result<Vec<u8>, CodecError> {
    let width = 72;
    if data.len() > width {
        return Err(CodecError::NotImplemented(
            "IGES card payload exceeds 72 bytes".into(),
        ));
    }
    let mut payload = vec![b' '; 80];
    payload[..data.len()].copy_from_slice(data);
    payload[72] = section;
    let sequence = format!("{sequence:>7}");
    if sequence.len() > 7 {
        return Err(CodecError::NotImplemented(
            "IGES card sequence exceeds seven digits".into(),
        ));
    }
    payload[73..].copy_from_slice(sequence.as_bytes());
    payload.push(b'\n');
    Ok(payload)
}

/// An IGES real literal. The value is finite, so the literal is an IGES
/// real.
fn number(value: FiniteReal) -> String {
    let value = value.get();
    if value == 0.0 {
        "0".into()
    } else {
        format!("{value:.16e}").replace('e', "D")
    }
}

/// Admit the value the writer read or computed for `field`. IGES states no
/// non-finite real.
fn finite(value: f64, field: &str) -> Result<FiniteReal, CodecError> {
    FiniteReal::new(value).ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "IGES writer computed the non-finite {field} {value}, which IGES cannot state"
        ))
    })
}

#[cfg(test)]
mod tests;
