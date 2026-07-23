// SPDX-License-Identifier: Apache-2.0
//! Conversion from a PSB container to [`CadIr`].
//!
//! Decode transfers standard datum planes as derived plane surfaces and
//! preserves each geometry section as an [`UnknownRecord`]. Source metadata
//! records the layout, namespace census, active units, and counts of decoded
//! structural rows.
//!
//! Surface and curve namespaces contain useful topology and prototype data, but
//! the placed body model is incomplete. The report therefore records blocking
//! geometry and topology losses instead of emitting a partial B-rep.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use cadmpeg_ir::codec::{CodecError, DecodeResult};
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::features::{
    Angle, BooleanOp, ChamferSpec, DesignParameter, DimensionDisplay, EdgeSelection, ExtrudeExtent,
    ExtrudeSide, FaceSelection, Feature, FeatureDefinition as IrFeatureDefinition,
    FeatureId as IrFeatureId, FeatureSourceContent, FeatureTreeNodeRole, HoleBottom, HoleForm,
    HoleKind, Length, ParameterId, ParameterValue, PatternForm, PatternKind, ProfileRef,
    RadiusForm, RadiusSpec, RevolutionAxis, RevolutionConstruction, RevolveExtent, Termination,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, OccurrenceId, PcurveId, PointId,
    ProceduralCurveId, ProceduralSurfaceId, ProductId, RegionId, ShellId, SurfaceId, UnknownId,
    VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::product::{OccurrenceParent, Product, ProductOccurrence};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchCoordinateAxis,
    SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus,
    SketchNativeOperand,
};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, PcurveUse, Point, Region, Sense, Shell,
    Vertex,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::{Exactness, SourceObjectAssociation};
use serde::Serialize;

use crate::container::{self, role, ContainerScan};
use crate::topology::HalfEdgeId;

mod native;
mod records;
use native::{annotate, emit_arena, store_arena};
#[allow(clippy::wildcard_imports)]
use records::*;

fn unique_owned_feature_definition(
    definitions: &[crate::feature::FeatureDefinition],
    feature_id: u32,
) -> Option<&crate::feature::FeatureDefinition> {
    let mut matches = definitions
        .iter()
        .filter(|definition| definition.owner_feature_id == Some(feature_id));
    let definition = matches.next()?;
    matches.next().is_none().then_some(definition)
}

fn unique_feature_section_transform(
    transforms: &[crate::placement::FeatureSectionTransform],
    definition_id: u32,
    section_offset: usize,
) -> Option<&crate::placement::FeatureSectionTransform> {
    let mut matches = transforms.iter().filter(|transform| {
        transform.definition_id == definition_id && transform.offset == section_offset
    });
    let transform = matches.next()?;
    matches.next().is_none().then_some(())?;
    if let Some(feature_id) = transform.feature_id {
        let feature_matches = transforms
            .iter()
            .filter(|candidate| candidate.feature_id == Some(feature_id))
            .count();
        (feature_matches == 1).then_some(())?;
    }
    Some(transform)
}

fn unique_feature_definition_for_transform<'a>(
    definitions: &'a [crate::feature::FeatureDefinition],
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<&'a crate::feature::FeatureDefinition> {
    let mut matches = definitions.iter().filter(|definition| {
        definition.id == transform.definition_id
            && definition
                .section_3d
                .as_ref()
                .is_some_and(|section| section.offset == transform.offset)
    });
    let definition = matches.next()?;
    matches.next().is_none().then_some(definition)
}

fn unique_feature_datum_plane(
    datums: &[crate::datum::DatumPlane],
    feature_id: u32,
) -> Option<&crate::datum::DatumPlane> {
    let mut matches = datums.iter().filter(|datum| datum.feature_id == feature_id);
    let datum = matches.next()?;
    matches.next().is_none().then_some(datum)
}

#[derive(Serialize)]
struct CreoSketchSectionPoint {
    point_id: u32,
    u: Option<f64>,
    v: Option<f64>,
    state: &'static str,
}

#[derive(Serialize)]
struct CreoSketchTableHeader {
    kind: &'static str,
    declared_count: Option<u32>,
    entity_ref: Option<u32>,
    entry_ref: Option<u32>,
    buckets: Vec<CreoSketchBucketHeader>,
    row_count: usize,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchBucketHeader {
    index: u32,
    declared_entry_count: u32,
    decoded_entry_count: u32,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchSection3d {
    sketch_plane_entity_id: Option<u32>,
    sketch_plane_flip: Option<bool>,
    reference_plane_entity_ids: Vec<u32>,
    reference_plane_datum_geometry_id: Option<u32>,
    orientation: CreoSketchSectionOrientation,
    dimension_ids: Vec<u32>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchSectionOrientation {
    section_flip: Option<bool>,
    reference_type: Option<u32>,
    segment_id: Option<u32>,
    reference_flip: Option<bool>,
}

#[derive(Serialize)]
struct CreoFeatureParameterFrame {
    kind: &'static str,
    body: Vec<u8>,
    decoded_values: Option<Vec<f64>>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoFeatureOutline {
    phase: &'static str,
    local_values: Vec<Option<f64>>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchTrimEntity {
    external_id: u32,
    mode: Option<u32>,
    vertices: [u32; 2],
    center_vertex: Option<u32>,
    kind: &'static str,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchTrimVertex {
    vertex_id: u32,
    entities: Vec<u32>,
    section_coordinates: Option<[f64; 2]>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchOrderRow {
    external_id: u32,
    internal_id: u32,
    bitmask: u32,
    offset: usize,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CreoSketchSavedEntity {
    Line {
        entity_id: u32,
        references: Vec<u32>,
        attributes: Vec<[u8; 5]>,
        endpoints: [[Option<f64>; 3]; 2],
        offset: usize,
    },
    Arc {
        entity_id: u32,
        center: [Option<f64>; 3],
        radius: Option<f64>,
        endpoints: [[Option<f64>; 3]; 2],
        parameters: [Option<f64>; 2],
        offset: usize,
    },
    Circle {
        entity_id: u32,
        center: [Option<f64>; 3],
        radius: Option<f64>,
        offset: usize,
    },
    Spline {
        entity_id: Option<u32>,
        declared_point_count: Option<u32>,
        interpolation_points: Vec<[f64; 3]>,
        endpoint_tangents: Option<[[f64; 3]; 2]>,
        parameters: Option<Vec<f64>>,
        offset: usize,
    },
    Dummy {
        entity_id: Option<u32>,
        offset: usize,
    },
}

#[derive(Serialize)]
struct CreoSketchVariable {
    variable_type: u32,
    key: u32,
    value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_value: Option<f64>,
    guess: Option<f64>,
    known: Option<u32>,
    homogeneity: Option<u32>,
    uvar_id: Option<u32>,
    dimension_driven: bool,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchSegment {
    external_id: u32,
    kind: &'static str,
    point_ids: [u32; 2],
    center_id: Option<u32>,
    directions: [Option<u32>; 3],
    arc_orientation: Option<u32>,
    vertical_horizontal_constraint: Option<u32>,
    radius_dimension_id: Option<u32>,
    secondary_radius_dimension_id: Option<u32>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchOpaqueSegment {
    external_id: u32,
    kind: u32,
    point_ids: [Option<u32>; 2],
    center_id: Option<u32>,
    directions: [Option<u32>; 3],
    arc_orientation: Option<u32>,
    vertical_horizontal_constraint: Option<u32>,
    radius_dimension_id: Option<u32>,
    secondary_radius_dimension_id: Option<u32>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchDimension {
    external_id: u32,
    dimension_type: u32,
    value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unresolved_value_token: Option<Vec<u8>>,
    unit: &'static str,
    direction_byte: u8,
    auxiliary_value: Option<f64>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchRelation {
    relation_id: u32,
    used: u32,
    operands: Vec<u8>,
    operand_vectors: Option<[[Option<u32>; 4]; 3]>,
    sign: u32,
    dimension_id: u32,
    relation_type: u32,
    body: Vec<u8>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchSkamp {
    id: u32,
    kind: u32,
    flags: u32,
    status: u32,
    items: Vec<CreoSketchSkampItem>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchSkampItem {
    entity_id: u32,
    sense: u32,
}

#[derive(Serialize)]
struct CreoSketchRelationTriple {
    #[serde(rename = "relation_id")]
    relation: Option<u32>,
    #[serde(rename = "equation_id")]
    equation: Option<u32>,
    #[serde(rename = "skamp_id")]
    skamp: Option<u32>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoCurveExpressionLocalSystem {
    dimensions: u32,
    count: u32,
    body: Vec<u8>,
    explicit_slots: Option<[f64; 12]>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoCurveExpressionLine {
    text: String,
    offset: usize,
}

#[derive(Serialize)]
struct CreoCurveExpressionAssignment {
    name: String,
    declared_unit: Option<String>,
    expression: String,
    dependencies: Vec<String>,
    value: Option<crate::curve::CurveExpressionValue>,
    activation: &'static str,
    offset: usize,
}

#[derive(Serialize)]
struct CreoFeatureOperationState {
    id: String,
    feature_id: u32,
    state_ordinal: usize,
    current: bool,
    family: String,
    display_name_stored: bool,
    stored_name: Option<String>,
    stored_name_bytes: Option<Vec<u8>>,
    identifier_keyword: Option<String>,
    stored_name_prefix: Option<String>,
    recipe: Option<&'static str>,
    root_schema_class: Option<u32>,
    parent_feature_id: Option<u32>,
    offset: usize,
    state_offset: usize,
}

#[derive(Serialize)]
struct CreoFeatureSurfaceReplayAssociation {
    id: String,
    owner_feature_id: u32,
    visible_surface_id: u32,
    replay_surface_id: u32,
    replay_ordinal: usize,
    surface_family: String,
    table_offset: usize,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CreoFeatureFieldValue {
    Empty,
    CompactInt {
        value: u32,
    },
    CompactIntArray {
        values: Vec<u32>,
    },
    EntityReference {
        entity_id: u32,
        terminated: bool,
    },
    ScalarArray {
        dimensions: u32,
        count: u32,
        body: Vec<u8>,
        decoded_values: Option<Vec<f64>>,
    },
    Raw {
        bytes: Vec<u8>,
    },
}

#[derive(Serialize, Clone)]
struct CreoHalfEdgeRef {
    curve_id: u32,
    side: u8,
}

fn attach_expanded_sections(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    // The whole expansion namespace is gated on there being expanded sections at
    // all: with none, the double-xar and primitive-scalar arenas are skipped even
    // when their scan tables are non-empty. Preserve that early return.
    let records = expanded_section_records(scan);
    if records.is_empty() {
        return Ok(());
    }
    emit_arena(
        ir,
        annotations,
        "expanded_sections",
        &records,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.name,
                record.source_offset as u64,
                "unix_compress_expanded_section",
                Exactness::Derived,
            );
        },
    )?;
    let tables = scan
        .primitives
        .double_xar_tables
        .iter()
        .map(|table| CreoDoubleXarTableRecord {
            id: format!(
                "creo:{}:double_xar#{}:{}",
                table.section_name, table.section_source_offset, table.expanded_offset
            ),
            section_name: table.section_name.clone(),
            section_source_offset: table.section_source_offset,
            expanded_offset: table.expanded_offset,
            count: table.count,
            entries: table
                .entries
                .iter()
                .map(|entry| CreoDoubleXarEntryRecord {
                    index: entry.index,
                    raw: entry.raw.clone(),
                    value: entry.value,
                    kind: entry.kind,
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    emit_arena(
        ir,
        annotations,
        "double_xar_tables",
        &tables,
        |annotations, table| {
            annotate(
                annotations,
                &table.id,
                &table.section_name,
                table.section_source_offset as u64,
                "model_scalar_dictionary",
                Exactness::ByteExact,
            );
        },
    )?;
    let primitive_arrays = scan
        .primitives
        .scalar_arrays
        .iter()
        .map(|array| CreoPrimitiveScalarArrayRecord {
            id: format!(
                "creo:solid_primdata:scalar_array#{}:{}",
                array.field, array.offset
            ),
            field: array.field.clone(),
            expanded_offset: array.offset,
            count: array.count,
            values: array.values.clone(),
        })
        .collect::<Vec<_>>();
    store_arena(ir, "primitive_scalar_arrays", &primitive_arrays)?;
    Ok(())
}

#[derive(Serialize)]
struct CreoFcCurveCoordinateToken {
    value_mm: f64,
    raw: Vec<u8>,
    offset: usize,
    length: usize,
}

#[derive(Serialize)]
struct CreoFcCurveOpaqueSpan {
    raw: Vec<u8>,
    offset: usize,
    length: usize,
}

#[derive(Serialize)]
struct CreoFc05CircleRecord {
    id: String,
    curve_id: u32,
    center_row_frame: [f64; 2],
    radius_mm: f64,
    sample_direction_row_frame: [f64; 2],
    reference_direction_row_frame: Option<[f64; 2]>,
    parameter_sign: Option<i8>,
    cap_ordinate_row_frame: Option<f64>,
    point_count: usize,
    max_residual: f64,
    angle_parameter_consistent: bool,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoFc05CylinderCapPairRecord {
    id: String,
    surface_id: u32,
    curve_ids: Vec<u32>,
    cap_plane_ids: Vec<u32>,
    curve_cap_ordinates_row_frame: Vec<f64>,
    center_row_frame: [f64; 2],
    radius_mm: f64,
    reference_direction_row_frame: [f64; 2],
    parameter_sign: i8,
    cap_ordinates_row_frame: Vec<f64>,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CreoPlaneEnvelope {
    Standard {
        bounds_2d: [[Option<f64>; 2]; 2],
        corners_3d: [[Option<f64>; 3]; 2],
    },
    Compact {
        prefix: [Option<f64>; 3],
        corners_3d: [[Option<f64>; 3]; 2],
    },
}

fn feature_surface_replay_associations(
    scan: &ContainerScan,
) -> Vec<CreoFeatureSurfaceReplayAssociation> {
    let mut associations = Vec::new();
    for table in &scan.features.entity_tables {
        let Some(owner_feature_id) = table.feature_id else {
            continue;
        };
        let visible_ids = table
            .entries
            .iter()
            .take_while(|entry| entry.class_id == 254)
            .map(|entry| entry.entity_id)
            .collect::<Vec<_>>();
        if visible_ids.is_empty() {
            continue;
        }
        let visible_rows = visible_ids
            .iter()
            .map(|id| crate::surface::unique_surface_row(&scan.surfaces.rows, *id))
            .collect::<Option<Vec<_>>>();
        let Some(visible_rows) = visible_rows else {
            continue;
        };
        let replay_entries = &table.entries[visible_ids.len()..];
        let mut replay_ordinal = 0;
        let mut cursor = 0;
        while cursor + visible_rows.len() <= replay_entries.len() {
            let candidate_entries = &replay_entries[cursor..cursor + visible_rows.len()];
            if candidate_entries.iter().any(|entry| entry.class_id != 214) {
                cursor += 1;
                continue;
            }
            let candidate_rows = candidate_entries
                .iter()
                .map(|entry| {
                    crate::surface::unique_surface_row(
                        &scan.surfaces.nonvisible_rows,
                        entry.entity_id,
                    )
                })
                .collect::<Option<Vec<_>>>();
            let Some(candidate_rows) = candidate_rows else {
                cursor += 1;
                continue;
            };
            if visible_rows
                .iter()
                .zip(&candidate_rows)
                .all(|(visible, replay)| {
                    visible.feature_id == owner_feature_id
                        && replay.feature_id == owner_feature_id
                        && visible.kind == replay.kind
                })
            {
                associations.extend(visible_rows.iter().zip(candidate_rows).map(
                    |(visible, replay)| CreoFeatureSurfaceReplayAssociation {
                        id: format!(
                            "creo:allfeatur:surface_replay#{}:{}:{}:{}",
                            owner_feature_id, table.offset, replay_ordinal, visible.id
                        ),
                        owner_feature_id,
                        visible_surface_id: visible.id,
                        replay_surface_id: replay.id,
                        replay_ordinal,
                        surface_family: surface_family(visible.kind).to_string(),
                        table_offset: table.offset,
                    },
                ));
                replay_ordinal += 1;
                cursor += visible_rows.len();
            } else {
                cursor += 1;
            }
        }
    }
    associations
}

fn affected_kind(kind: crate::feature::AffectedIdKind) -> &'static str {
    match kind {
        crate::feature::AffectedIdKind::Geometry => "geometry",
        crate::feature::AffectedIdKind::Edges => "edges",
        crate::feature::AffectedIdKind::StrongParents => "strong_parents",
        crate::feature::AffectedIdKind::Parents => "parents",
        crate::feature::AffectedIdKind::Contours => "contours",
    }
}

fn extent_source(source: crate::feature::ReplayExtentSource) -> &'static str {
    match source {
        crate::feature::ReplayExtentSource::Explicit => "explicit",
        crate::feature::ReplayExtentSource::Inherited => "inherited",
    }
}

fn half_edge_ref(id: crate::topology::HalfEdgeId) -> CreoHalfEdgeRef {
    CreoHalfEdgeRef {
        curve_id: id.curve_id,
        side: id.side,
    }
}

fn fc05_circle_records(scan: &ContainerScan) -> Vec<CreoFc05CircleRecord> {
    scan.curves
        .fc05_circles
        .iter()
        .map(|record| CreoFc05CircleRecord {
            id: format!("creo:curve:fc05_circle#{}", record.curve_id),
            curve_id: record.curve_id,
            center_row_frame: record.center_row_frame,
            radius_mm: record.radius_mm,
            sample_direction_row_frame: record.sample_direction_row_frame,
            reference_direction_row_frame: record.reference_direction_row_frame,
            parameter_sign: record.parameter_sign,
            cap_ordinate_row_frame: record.cap_ordinate_row_frame,
            point_count: record.point_count,
            max_residual: record.max_residual,
            angle_parameter_consistent: record.angle_parameter_consistent,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn fc05_cylinder_cap_pair_records(scan: &ContainerScan) -> Vec<CreoFc05CylinderCapPairRecord> {
    scan.curves
        .fc05_cylinder_cap_pairs
        .iter()
        .map(|record| CreoFc05CylinderCapPairRecord {
            id: format!("creo:surface:fc05_cylinder_cap_pair#{}", record.surface_id),
            surface_id: record.surface_id,
            curve_ids: record.curve_ids.clone(),
            cap_plane_ids: record.cap_plane_ids.clone(),
            curve_cap_ordinates_row_frame: record.curve_cap_ordinates_row_frame.clone(),
            center_row_frame: record.center_row_frame,
            radius_mm: record.radius_mm,
            reference_direction_row_frame: record.reference_direction_row_frame,
            parameter_sign: record.parameter_sign,
            cap_ordinates_row_frame: record.cap_ordinates_row_frame.clone(),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

#[derive(Serialize)]
struct CreoTabulatedCylinderFrame {
    values: [f64; 6],
    prefixes: [u8; 6],
}

#[derive(Serialize)]
struct CreoPositionalCylinderFrame {
    origin: [f64; 3],
    axis: [f64; 3],
    ref_direction: [f64; 3],
    radius: f64,
    length: Option<f64>,
}

#[derive(Serialize)]
struct CreoPositionalConeFrame {
    apex: [f64; 3],
    axis: [f64; 3],
    ref_direction: [f64; 3],
    half_angle: f64,
}

#[derive(Serialize)]
struct CreoPositionalTorusFrame {
    center: [f64; 3],
    axis: [f64; 3],
    ref_direction: [f64; 3],
    major_radius: f64,
    minor_radius: f64,
}

#[derive(Serialize)]
struct CreoTorusOutlineFrame {
    values: [f64; 6],
    selector: u32,
    offset: usize,
}

#[derive(Serialize)]
struct CreoType26FiveCoordinateEnvelope {
    values: [f64; 5],
    offset: usize,
}

#[derive(Serialize)]
struct CreoType26SplitCoordinateEnvelope {
    values: [f64; 4],
    offset: usize,
}

#[derive(Serialize)]
struct CreoTorusRadiusOverrides {
    radius1: f64,
    radius2: f64,
    radius2_encoding: &'static str,
    offset: usize,
}

#[derive(Serialize)]
struct CreoConeHalfAngleOverride {
    radians: f64,
    offset: usize,
}

#[derive(Serialize)]
struct CreoCurveParameterScalar {
    value: f64,
    raw: Vec<u8>,
    offset: usize,
    length: usize,
}

#[derive(Serialize)]
struct CreoCurveParameterReference {
    entity_id: u32,
    offset: usize,
    length: usize,
}

#[derive(Serialize)]
struct CreoCurveParameterOpaqueSpan {
    raw: Vec<u8>,
    offset: usize,
    length: usize,
}

#[derive(Serialize)]
struct CreoSurfaceParameterScalarFrame {
    offset: usize,
    slots: Vec<CreoSurfaceParameterSlot>,
}

#[derive(Serialize)]
struct CreoSurfaceParameterOpaqueSpan {
    raw: Vec<u8>,
    offset: usize,
    length: usize,
}

#[derive(Serialize)]
struct CreoSurfaceParameterSlot {
    value: Option<f64>,
    raw: Vec<u8>,
    offset: usize,
    length: usize,
}

fn source_section(scan: &ContainerScan, offset: usize) -> String {
    scan.framing
        .sections
        .iter()
        .find(|section| offset >= section.offset && offset < section.offset + section.length)
        .map_or("unknown", |section| section.name.as_str())
        .to_string()
}

fn surface_family(kind: crate::surface::SurfaceKind) -> &'static str {
    match kind {
        crate::surface::SurfaceKind::Plane => "plane",
        crate::surface::SurfaceKind::Cylinder => "cylinder",
        crate::surface::SurfaceKind::Cone => "cone",
        crate::surface::SurfaceKind::TorusOrSphere => "torus_or_sphere",
        crate::surface::SurfaceKind::Spline => "spline",
        crate::surface::SurfaceKind::Fillet => "fillet",
        crate::surface::SurfaceKind::Extrusion => "extrusion",
    }
}

#[derive(Default)]
struct SurfaceTransferCoverage {
    unique_rows: usize,
    transferred_rows: usize,
    ambiguous_rows: usize,
    by_family: BTreeMap<&'static str, (usize, usize)>,
}

#[derive(Default)]
struct CurveTransferCoverage {
    unique_rows: usize,
    transferred_rows: usize,
    ambiguous_rows: usize,
    by_type: BTreeMap<u8, (usize, usize)>,
}

#[derive(Default)]
struct DesignConstraintTransferCoverage {
    transferred: usize,
    native: usize,
    active: usize,
    active_native: usize,
}

impl DesignConstraintTransferCoverage {
    fn typed(&self) -> usize {
        self.transferred.saturating_sub(self.native)
    }

    fn active_typed(&self) -> usize {
        self.active.saturating_sub(self.active_native)
    }
}

fn design_constraint_transfer_coverage(
    constraints: &[SketchConstraint],
    id_marker: &str,
    native_kind_prefix: &str,
) -> DesignConstraintTransferCoverage {
    constraints
        .iter()
        .filter(|constraint| constraint.id.0.contains(id_marker))
        .fold(
            DesignConstraintTransferCoverage::default(),
            |mut coverage, constraint| {
                coverage.transferred += 1;
                let native = matches!(
                    &constraint.definition,
                    SketchConstraintDefinition::Native { native_kind, .. }
                        if native_kind.starts_with(native_kind_prefix)
                );
                if native {
                    coverage.native += 1;
                }
                if constraint.active == Some(true) {
                    coverage.active += 1;
                    if native {
                        coverage.active_native += 1;
                    }
                }
                coverage
            },
        )
}

fn curve_transfer_coverage(
    rows: &[crate::curve::CurveTopologyRow],
    curves: &[Curve],
) -> CurveTransferCoverage {
    let unique_rows = crate::topology::uniquely_identified_rows(rows);
    let transferred_ids = curves
        .iter()
        .filter(|curve| !matches!(curve.geometry, CurveGeometry::Unknown { .. }))
        .filter_map(|curve| {
            curve
                .source_object
                .as_ref()
                .filter(|source| source.format == "creo")?
                .object_id
                .strip_prefix("VisibGeom:")?
                .parse::<u32>()
                .ok()
        })
        .collect::<BTreeSet<_>>();
    let mut coverage = CurveTransferCoverage {
        unique_rows: unique_rows.len(),
        ambiguous_rows: rows.len().saturating_sub(unique_rows.len()),
        ..CurveTransferCoverage::default()
    };
    for row in unique_rows {
        let transferred = usize::from(transferred_ids.contains(&row.id));
        coverage.transferred_rows += transferred;
        let type_coverage = coverage.by_type.entry(row.type_byte).or_default();
        type_coverage.0 += 1;
        type_coverage.1 += transferred;
    }
    coverage
}

fn surface_transfer_coverage(
    rows: &[crate::surface::SurfaceRow],
    surfaces: &[Surface],
    procedural_surfaces: &[ProceduralSurface],
) -> SurfaceTransferCoverage {
    let unique_rows = crate::surface::uniquely_identified_rows(rows);
    let extrusion_surfaces = procedural_surfaces
        .iter()
        .filter(|procedural| {
            matches!(
                procedural.definition,
                ProceduralSurfaceDefinition::Extrusion { .. }
            )
        })
        .map(|procedural| &procedural.surface)
        .collect::<BTreeSet<_>>();
    let transferred = surfaces
        .iter()
        .filter_map(|surface| {
            let id = surface
                .source_object
                .as_ref()
                .filter(|source| source.format == "creo")?
                .object_id
                .strip_prefix("VisibGeom:")?
                .parse::<u32>()
                .ok()?;
            let mut kinds = vec![surface_kind_for_geometry(&surface.geometry)?];
            if extrusion_surfaces.contains(&surface.id) {
                kinds.push(crate::surface::SurfaceKind::Extrusion);
            }
            Some((id, kinds))
        })
        .collect::<Vec<_>>();
    let mut coverage = SurfaceTransferCoverage {
        unique_rows: unique_rows.len(),
        ambiguous_rows: rows.len().saturating_sub(unique_rows.len()),
        ..SurfaceTransferCoverage::default()
    };
    for kind in [
        crate::surface::SurfaceKind::Plane,
        crate::surface::SurfaceKind::Cylinder,
        crate::surface::SurfaceKind::Cone,
        crate::surface::SurfaceKind::TorusOrSphere,
        crate::surface::SurfaceKind::Spline,
        crate::surface::SurfaceKind::Fillet,
        crate::surface::SurfaceKind::Extrusion,
    ] {
        coverage.by_family.insert(surface_family(kind), (0, 0));
    }
    for row in unique_rows {
        let is_transferred = transferred
            .iter()
            .any(|(id, kinds)| *id == row.id && kinds.contains(&row.kind));
        coverage.transferred_rows += usize::from(is_transferred);
        let family = surface_family(row.kind);
        let family_coverage = coverage.by_family.entry(family).or_default();
        family_coverage.0 += 1;
        family_coverage.1 += usize::from(is_transferred);
    }
    coverage
}

fn surface_variant(type_byte: u8) -> Option<&'static str> {
    match type_byte {
        0x2a => Some("ruled_surface"),
        0x2c => Some("tabulated_cylinder"),
        _ => None,
    }
}

fn surface_prototype_family_name(family: &crate::surface::SurfacePrototypeFamily) -> String {
    match family {
        crate::surface::SurfacePrototypeFamily::Plane => "plane".to_string(),
        crate::surface::SurfacePrototypeFamily::Cylinder => "cylinder".to_string(),
        crate::surface::SurfacePrototypeFamily::Cone => "cone".to_string(),
        crate::surface::SurfacePrototypeFamily::Torus => "torus_or_sphere".to_string(),
        crate::surface::SurfacePrototypeFamily::Spline => "spline".to_string(),
        crate::surface::SurfacePrototypeFamily::Fillet => "fillet".to_string(),
        crate::surface::SurfacePrototypeFamily::Extrusion => "extrusion".to_string(),
        crate::surface::SurfacePrototypeFamily::Other(name) => format!("other:{name}"),
    }
}

fn surface_named_parameter_record(
    parameter: &crate::surface::SurfaceNamedParameter,
) -> CreoSurfaceNamedParameterRecord {
    let (
        value_kind,
        compact_values,
        scalar_dimensions,
        scalar_count,
        scalar_values,
        scalar_tokens,
        opaque,
    ) = match &parameter.value {
        crate::surface::SurfaceNamedValue::CompactInt(value) => (
            "compact_int",
            vec![*value],
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::CompactIntArray(values) => (
            "compact_int_array",
            values.clone(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::ContiguousEntityReferences { entity_ids, .. } => (
            "contiguous_entity_references",
            entity_ids.clone(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::ScalarArray {
            dimensions,
            count,
            values,
            tokens,
        } => (
            "scalar_array",
            Vec::new(),
            Some(*dimensions),
            Some(*count),
            values.clone(),
            tokens.clone(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::CountedScalarArray {
            count,
            values,
            tokens,
        } => (
            "counted_scalar_array",
            Vec::new(),
            None,
            Some(*count),
            values.clone(),
            tokens.clone(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::ScalarSequence(values) => (
            "scalar_sequence",
            Vec::new(),
            None,
            None,
            values.iter().copied().map(Some).collect(),
            Vec::new(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::Opaque(value) => (
            "opaque",
            Vec::new(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            value.clone(),
        ),
    };
    CreoSurfaceNamedParameterRecord {
        name: parameter.name.clone(),
        value_kind,
        compact_values,
        scalar_dimensions,
        scalar_count,
        scalar_values,
        scalar_tokens,
        opaque,
        body: parameter.body.clone(),
        offset: parameter.offset,
        value_offset: parameter.value_offset,
    }
}

fn family_table_record(scan: &ContainerScan) -> Option<CreoFamilyTableRecord> {
    let record = scan.framing.family_table?;
    let (pointer_kind, table_entity_id) = match record.pointer {
        crate::container::FamilyTablePointer::Null => ("null", None),
        crate::container::FamilyTablePointer::Entity(id) => ("entity_reference", Some(id)),
    };
    Some(CreoFamilyTableRecord {
        id: "creo:family_info:driver_table#root",
        pointer_kind,
        table_entity_id,
        offset: record.offset,
    })
}

fn curve_expression_record_id(record: &crate::curve::CurveExpressionRecord) -> String {
    format!(
        "creo:depdb:curve_expression#{}-{}-{}",
        if record.backup { "backup" } else { "active" },
        record.entity_id,
        record.offset
    )
}

fn curve_expression_helix_definition(
    record: &crate::curve::CurveExpressionRecord,
) -> Option<ProceduralCurveDefinition> {
    let helix = crate::curve::expression_helix(record)?;
    let slots = record.local_system.as_ref()?.explicit_slots?;
    let u = Vector3::new(slots[0], slots[1], slots[2]);
    let v = Vector3::new(slots[6], slots[7], slots[8]);
    let u_norm = u.norm();
    let v_norm = v.norm();
    let scale = u_norm.max(v_norm).max(1.0);
    if !u_norm.is_finite()
        || !v_norm.is_finite()
        || u_norm <= 1e-12
        || v_norm <= 1e-12
        || (u_norm - v_norm).abs() > 1e-9 * scale
        || (u.x * v.x + u.y * v.y + u.z * v.z).abs() > 1e-9 * u_norm * v_norm
        || slots[3..6].iter().any(|value| value.abs() > 1e-12)
    {
        return None;
    }
    let u = Vector3::new(u.x / u_norm, u.y / u_norm, u.z / u_norm);
    let v = Vector3::new(v.x / v_norm, v.y / v_norm, v.z / v_norm);
    let axis = Vector3::new(
        u.y * v.z - u.z * v.y,
        u.z * v.x - u.x * v.z,
        u.x * v.y - u.y * v.x,
    );
    let origin = Point3::new(slots[9], slots[10], slots[11]);
    let (sin, cos) = helix.start_angle.sin_cos();
    let major_direction = Vector3::new(
        u.x * cos + v.x * sin,
        u.y * cos + v.y * sin,
        u.z * cos + v.z * sin,
    );
    let tangent_direction = Vector3::new(
        -u.x * sin + v.x * cos,
        -u.y * sin + v.y * cos,
        -u.z * sin + v.z * cos,
    );
    let minor_direction = if helix.clockwise {
        Vector3::new(
            -tangent_direction.x,
            -tangent_direction.y,
            -tangent_direction.z,
        )
    } else {
        tangent_direction
    };
    Some(ProceduralCurveDefinition::Helix {
        angle_range: [0.0, helix.revolutions * std::f64::consts::TAU],
        center: Point3::new(
            origin.x + axis.x * helix.z_start,
            origin.y + axis.y * helix.z_start,
            origin.z + axis.z * helix.z_start,
        ),
        major: Vector3::new(
            major_direction.x * helix.radius,
            major_direction.y * helix.radius,
            major_direction.z * helix.radius,
        ),
        minor: Vector3::new(
            minor_direction.x * helix.radius,
            minor_direction.y * helix.radius,
            minor_direction.z * helix.radius,
        ),
        pitch: Vector3::new(
            axis.x * helix.height / helix.revolutions,
            axis.y * helix.height / helix.revolutions,
            axis.z * helix.height / helix.revolutions,
        ),
        apex_factor: 0.0,
        axis,
    })
}

fn expression_dependency_reaches(dependencies: &[Vec<usize>], start: usize, target: usize) -> bool {
    let mut pending = vec![start];
    let mut visited = BTreeSet::new();
    while let Some(index) = pending.pop() {
        if index == target {
            return true;
        }
        if visited.insert(index) {
            pending.extend(dependencies[index].iter().copied());
        }
    }
    false
}

fn curve_expression_parameter_order(
    record: &crate::curve::CurveExpressionRecord,
    unique_assignment_indices: &BTreeMap<String, usize>,
) -> (Vec<u32>, BTreeSet<(usize, usize)>) {
    let dependencies = record
        .assignments
        .iter()
        .map(|assignment| {
            let mut seen = BTreeSet::new();
            assignment
                .dependencies
                .iter()
                .filter_map(|name| {
                    unique_assignment_indices
                        .get(&crate::curve::expression_identifier_key(name))
                        .copied()
                })
                .filter(|index| seen.insert(*index))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut cyclic_edges = BTreeSet::new();
    for (consumer, dependency_indices) in dependencies.iter().enumerate() {
        for &dependency in dependency_indices {
            if expression_dependency_reaches(&dependencies, dependency, consumer) {
                cyclic_edges.insert((consumer, dependency));
            }
        }
    }
    let mut ordinals = vec![u32::MAX; dependencies.len()];
    for ordinal in 0..dependencies.len() {
        let index = (0..dependencies.len())
            .find(|&candidate| {
                ordinals[candidate] == u32::MAX
                    && dependencies[candidate].iter().all(|dependency| {
                        cyclic_edges.contains(&(candidate, *dependency))
                            || ordinals[*dependency] != u32::MAX
                    })
            })
            .expect("removing cyclic edges leaves an acyclic assignment graph");
        ordinals[index] = ordinal as u32;
    }
    (ordinals, cyclic_edges)
}

fn curve_expression_parameter_names(
    assignments: &[crate::curve::CurveExpressionAssignment],
) -> Vec<String> {
    let counts = assignments
        .iter()
        .fold(BTreeMap::new(), |mut counts, assignment| {
            *counts
                .entry(crate::curve::expression_identifier_key(&assignment.name))
                .or_insert(0usize) += 1;
            counts
        });
    let mut occurrences = BTreeMap::new();
    assignments
        .iter()
        .map(|assignment| {
            let key = crate::curve::expression_identifier_key(&assignment.name);
            if counts[&key] == 1 {
                return assignment.name.clone();
            }
            let occurrence = occurrences.entry(key).or_insert(0usize);
            *occurrence += 1;
            format!("{}#{occurrence}", assignment.name)
        })
        .collect()
}

fn transfer_curve_expression_features(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    dimension_parameters: &BTreeMap<String, ParameterId>,
) -> usize {
    let ordinal_base = ir
        .model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
        .map_or(0, |value| value + 1);
    let mut transferred_assignment_count = 0;
    for (expression_ordinal, record) in scan
        .curves
        .expressions
        .iter()
        .filter(|record| !record.backup)
        .enumerate()
    {
        let ordinal = ordinal_base + expression_ordinal as u64;
        let feature_id = IrFeatureId(format!(
            "creo:depdb:curve_expression_feature#{}-{}",
            record.entity_id, record.offset
        ));
        let mut assignment_indices_by_name = BTreeMap::<String, Option<usize>>::new();
        for (assignment_ordinal, assignment) in record.assignments.iter().enumerate() {
            if assignment.activation == crate::curve::CurveExpressionActivation::Inactive {
                continue;
            }
            assignment_indices_by_name
                .entry(crate::curve::expression_identifier_key(&assignment.name))
                .and_modify(|index| *index = None)
                .or_insert(Some(assignment_ordinal));
        }
        let unique_assignment_indices = assignment_indices_by_name
            .iter()
            .filter_map(|(name, index)| index.map(|index| (name.clone(), index)))
            .collect::<BTreeMap<_, _>>();
        let (parameter_ordinals, cyclic_edges) =
            curve_expression_parameter_order(record, &unique_assignment_indices);
        let parameter_names = curve_expression_parameter_names(&record.assignments);
        let mut emitted_assignment_indices = (0..record.assignments.len()).collect::<Vec<_>>();
        emitted_assignment_indices.sort_by_key(|index| parameter_ordinals[*index]);
        let emitted_ordinals = emitted_assignment_indices
            .into_iter()
            .enumerate()
            .map(|(ordinal, index)| (index, ordinal as u32))
            .collect::<BTreeMap<_, _>>();
        let mut source_content = Vec::with_capacity(emitted_ordinals.len());
        for (assignment_ordinal, assignment) in record.assignments.iter().enumerate() {
            let Some(&ordinal) = emitted_ordinals.get(&assignment_ordinal) else {
                continue;
            };
            let parameter_id = ParameterId(format!(
                "creo:depdb:curve_expression_parameter#{}-{}-{}",
                record.entity_id, record.offset, assignment_ordinal
            ));
            let mut dependencies = assignment
                .dependencies
                .iter()
                .filter_map(|name| {
                    unique_assignment_indices
                        .get(&crate::curve::expression_identifier_key(name))
                        .copied()
                })
                .filter(|dependency| !cyclic_edges.contains(&(assignment_ordinal, *dependency)))
                .scan(BTreeSet::new(), |seen, dependency| {
                    seen.insert(dependency).then_some(dependency)
                })
                .map(|dependency| {
                    ParameterId(format!(
                        "creo:depdb:curve_expression_parameter#{}-{}-{}",
                        record.entity_id, record.offset, dependency
                    ))
                })
                .collect::<Vec<_>>();
            dependencies.extend(assignment.dependencies.iter().filter_map(|name| {
                let key = crate::curve::expression_identifier_key(name);
                if assignment_indices_by_name.contains_key(&key) {
                    None
                } else {
                    dimension_parameters.get(&key).cloned()
                }
            }));
            let external_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| {
                    let key = crate::curve::expression_identifier_key(name);
                    key != "t"
                        && !assignment_indices_by_name.contains_key(&key)
                        && !dimension_parameters.contains_key(&key)
                })
                .cloned()
                .collect::<Vec<_>>();
            let ambiguous_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| {
                    matches!(
                        assignment_indices_by_name
                            .get(&crate::curve::expression_identifier_key(name)),
                        Some(None)
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            let intrinsic_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| crate::curve::expression_identifier_key(name) == "t")
                .cloned()
                .collect::<Vec<_>>();
            let mut properties = BTreeMap::new();
            if !external_dependencies.is_empty() {
                properties.insert(
                    "external_dependencies".to_string(),
                    external_dependencies.join(","),
                );
            }
            if !ambiguous_dependencies.is_empty() {
                properties.insert(
                    "ambiguous_dependencies".to_string(),
                    ambiguous_dependencies.join(","),
                );
            }
            properties.insert(
                "source_assignment_ordinal".to_string(),
                assignment_ordinal.to_string(),
            );
            properties.insert(
                "activation".to_string(),
                assignment.activation.token().to_string(),
            );
            if let Some(unit) = &assignment.declared_unit {
                properties.insert("declared_unit".to_string(), unit.clone());
            }
            if let Some(crate::curve::CurveExpressionValue::Quantity(quantity)) = &assignment.value
            {
                properties.insert(
                    "evaluated_canonical_value".to_string(),
                    quantity.value.to_string(),
                );
                properties.insert(
                    "evaluated_dimension".to_string(),
                    format!(
                        "length:{},mass:{},time:{},angle:{},temperature:{}",
                        quantity.length_power,
                        quantity.mass_power,
                        quantity.time_power,
                        quantity.angle_power,
                        quantity.temperature_power
                    ),
                );
            }
            if parameter_names[assignment_ordinal] != assignment.name {
                properties.insert("source_name".to_string(), assignment.name.clone());
            }
            if !intrinsic_dependencies.is_empty() {
                properties.insert(
                    "independent_variables".to_string(),
                    intrinsic_dependencies.join(","),
                );
            }
            let cyclic_dependencies = assignment
                .dependencies
                .iter()
                .filter_map(|name| {
                    let key = crate::curve::expression_identifier_key(name);
                    unique_assignment_indices
                        .get(&key)
                        .filter(|dependency| {
                            cyclic_edges.contains(&(assignment_ordinal, **dependency))
                        })
                        .map(|_| name.clone())
                })
                .collect::<BTreeSet<_>>();
            if !cyclic_dependencies.is_empty() {
                properties.insert(
                    "cyclic_dependencies".to_string(),
                    cyclic_dependencies
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            annotate(
                annotations,
                &parameter_id.0,
                "DEPDB_DATA",
                assignment.offset as u64,
                "curve_expression_assignment",
                Exactness::Derived,
            );
            ir.model.parameters.push(DesignParameter {
                id: parameter_id.clone(),
                owner: Some(feature_id.clone()),
                ordinal,
                name: parameter_names[assignment_ordinal].clone(),
                expression: assignment.expression.clone(),
                display: None,
                value: assignment.value.as_ref().and_then(|value| match value {
                    crate::curve::CurveExpressionValue::Number(value) => {
                        Some(ParameterValue::Real(*value))
                    }
                    crate::curve::CurveExpressionValue::Length(value) => {
                        Some(ParameterValue::Length(cadmpeg_ir::features::Length(*value)))
                    }
                    crate::curve::CurveExpressionValue::Angle(value) => Some(
                        ParameterValue::Angle(cadmpeg_ir::features::Angle(value.to_radians())),
                    ),
                    crate::curve::CurveExpressionValue::Quantity(_) => None,
                    crate::curve::CurveExpressionValue::String(value) => {
                        Some(ParameterValue::String(value.clone()))
                    }
                }),
                dependencies,
                properties,
                pmi: None,
                native_ref: Some(curve_expression_record_id(record)),
            });
            transferred_assignment_count += 1;
            source_content.push(FeatureSourceContent::Parameter(parameter_id.clone()));
        }
        annotate(
            annotations,
            &feature_id.0,
            "DEPDB_DATA",
            record.expression_offset as u64,
            "curve_expression_feature",
            Exactness::Derived,
        );
        let helix = crate::curve::expression_helix(record);
        let placed_helix = curve_expression_helix_definition(record);
        if let Some(procedural_definition) = placed_helix {
            let curve_id = CurveId(format!(
                "creo:depdb:curve_expression_curve#{}-{}",
                record.entity_id, record.offset
            ));
            let procedural_id = ProceduralCurveId(format!(
                "creo:depdb:curve_expression_helix#{}-{}",
                record.entity_id, record.offset
            ));
            annotate(
                annotations,
                &curve_id.0,
                "DEPDB_DATA",
                record.offset as u64,
                "curve_expression_carrier",
                Exactness::Unknown,
            );
            annotate(
                annotations,
                &procedural_id.0,
                "DEPDB_DATA",
                record.offset as u64,
                "curve_expression_helix",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Unknown { record: None },
                source_object: None,
            });
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedural_id,
                curve: curve_id,
                definition: procedural_definition,
                cache_fit_tolerance: None,
            });
        }
        let definition = helix.map_or_else(
            || IrFeatureDefinition::Native {
                kind: "CurveFromEquation".to_string(),
                parameters: BTreeMap::from([
                    ("entity_id".to_string(), record.entity_id.to_string()),
                    (
                        "assignment_count".to_string(),
                        record.assignments.len().to_string(),
                    ),
                ]),
                properties: BTreeMap::new(),
            },
            |helix| IrFeatureDefinition::HelixNativeAxis {
                axis_native_ref: curve_expression_record_id(record),
                axial_rise: Length(helix.height),
                pitch: Length(helix.height / helix.revolutions),
                revolutions: helix.revolutions,
                start_angle: Angle(helix.start_angle),
                clockwise: helix.clockwise,
            },
        );
        ir.model.features.push(Feature {
            id: feature_id,
            ordinal,
            name: Some(format!("Curve Equation {}", record.entity_id)),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("crv_fr_eqn".to_string()),
            source_text: Some(
                record
                    .lines
                    .iter()
                    .map(|line| line.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            source_content,
            outputs: Vec::new(),
            definition,
            native_ref: Some(curve_expression_record_id(record)),
        });
    }
    transferred_assignment_count
}

fn feature_definition_has_sketch_design(definition: &crate::feature::FeatureDefinition) -> bool {
    definition.variables.is_some()
        || definition.segments.is_some()
        || definition.trim_entities.is_some()
        || definition.trim_vertices.is_some()
        || definition.order_table.is_some()
        || definition.section_3d.is_some()
        || definition.saved_section.is_some()
        || definition.dimensions.is_some()
        || definition.relations.is_some()
}

fn sketch_table_headers(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<CreoSketchTableHeader> {
    let mut headers = Vec::new();
    let mut push = |kind, declared_count, entity_ref, entry_ref, buckets, row_count, offset| {
        headers.push(CreoSketchTableHeader {
            kind,
            declared_count,
            entity_ref,
            entry_ref,
            buckets,
            row_count,
            offset,
        });
    };
    if let Some(table) = &definition.variables {
        push(
            "variables",
            Some(table.declared_count),
            table.entity_ref,
            None,
            Vec::new(),
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.segments {
        push(
            "segments",
            Some(table.declared_count),
            table.entity_ref,
            None,
            Vec::new(),
            table.rows.len() + table.opaque_rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.trim_entities {
        push(
            "trim_entities",
            table.declared_count,
            table.entity_ref,
            table.entry_ref,
            table
                .buckets
                .iter()
                .map(|bucket| CreoSketchBucketHeader {
                    index: bucket.index,
                    declared_entry_count: bucket.declared_entry_count,
                    decoded_entry_count: bucket.decoded_entry_count,
                    offset: bucket.offset,
                })
                .collect(),
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.trim_vertices {
        push(
            "trim_vertices",
            table.declared_count,
            table.entity_ref,
            table.entry_ref,
            table
                .buckets
                .iter()
                .map(|bucket| CreoSketchBucketHeader {
                    index: bucket.index,
                    declared_entry_count: bucket.declared_entry_count,
                    decoded_entry_count: bucket.decoded_entry_count,
                    offset: bucket.offset,
                })
                .collect(),
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.order_table {
        push(
            "order",
            Some(table.declared_count),
            table.entity_ref,
            None,
            Vec::new(),
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.dimensions {
        push(
            "dimensions",
            Some(table.declared_count),
            table.entity_ref,
            None,
            Vec::new(),
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.relations {
        push(
            "relations",
            Some(table.declared_count),
            table.entity_ref,
            None,
            Vec::new(),
            table.rows.len(),
            table.offset,
        );
        if let Some(header) = &table.skamp_header {
            push(
                "solver_incidences",
                Some(header.declared_count),
                Some(header.entity_ref),
                None,
                Vec::new(),
                table.skamps.len(),
                header.offset,
            );
        }
        if let Some(header) = &table.triples_header {
            push(
                "relation_triples",
                Some(header.declared_count),
                Some(header.entity_ref),
                None,
                Vec::new(),
                table.triples.len(),
                header.offset,
            );
        }
    }
    if let Some(table) = &definition.saved_section {
        push(
            "saved_entities",
            None,
            None,
            None,
            Vec::new(),
            table.entities.len(),
            table.offset,
        );
    }
    headers.sort_by_key(|header| header.offset);
    headers
}

fn binary_flag_value(flag: crate::feature::BinaryFlag) -> bool {
    match flag {
        crate::feature::BinaryFlag::Clear => false,
        crate::feature::BinaryFlag::Set => true,
    }
}

fn feature_definition_record_id(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
) -> String {
    if scan
        .features
        .definitions
        .iter()
        .filter(|candidate| candidate.id == definition.id)
        .count()
        != 1
        || (definition.id == 0 && definition.owner_feature_id.is_none())
    {
        format!(
            "creo:featdefs:feature_definition#offset:{}",
            definition.offset
        )
    } else {
        format!("creo:featdefs:feature_definition#{}", definition.id)
    }
}

fn feature_sketch_record_id_in_scan(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
) -> String {
    if scan
        .features
        .definitions
        .iter()
        .filter(|candidate| candidate.id == definition.id)
        .count()
        != 1
        || (definition.id == 0 && definition.owner_feature_id.is_none())
    {
        format!("creo:featdefs:sketch#offset:{}", definition.offset)
    } else {
        format!("creo:featdefs:sketch#{}", definition.id)
    }
}

fn model_sketch_id(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
) -> SketchId {
    let native_id = feature_sketch_record_id_in_scan(scan, definition);
    SketchId(native_id.replacen("creo:featdefs:sketch#", "creo:model:sketch#", 1))
}

fn sketch_identity_scope(sketch: &SketchId) -> &str {
    sketch
        .0
        .strip_prefix("creo:model:sketch#")
        .unwrap_or(&sketch.0)
}

fn sketch_entity_id(sketch: &SketchId, suffix: impl std::fmt::Display) -> SketchEntityId {
    SketchEntityId(format!(
        "creo:featdefs:sketch_entity#{}:{suffix}",
        sketch_identity_scope(sketch)
    ))
}

fn sketch_constraint_id(sketch: &SketchId, suffix: impl std::fmt::Display) -> SketchConstraintId {
    SketchConstraintId(format!(
        "creo:featdefs:sketch_constraint#{}:{suffix}",
        sketch_identity_scope(sketch)
    ))
}

fn sketch_native_ref(sketch: &SketchId) -> String {
    format!("creo:featdefs:sketch#{}", sketch_identity_scope(sketch))
}

fn sketch_section_curve_id(sketch: &SketchId, suffix: impl std::fmt::Display) -> String {
    format!(
        "creo:featdefs:section_curve#{}:{suffix}",
        sketch_identity_scope(sketch)
    )
}

fn sketch_point_ref(sketch: &SketchId, point: u32) -> String {
    format!("{}:point#{point}", sketch_native_ref(sketch))
}

fn sketch_feature_id(sketch: &SketchId) -> IrFeatureId {
    IrFeatureId(format!(
        "creo:model:sketch_feature#{}",
        sketch_identity_scope(sketch)
    ))
}

fn section_owner_feature_id(
    scan: &ContainerScan,
    definition_id: u32,
    sketch: &SketchId,
) -> IrFeatureId {
    owned_section_feature_id(scan, definition_id).map_or_else(
        || sketch_feature_id(sketch),
        |feature_id| IrFeatureId(format!("creo:model:feature#{feature_id}")),
    )
}

fn owning_feature_definition_ref(scan: &ContainerScan, feature_id: u32) -> Option<String> {
    let definitions = scan
        .features
        .definitions
        .iter()
        .filter(|definition| definition.owner_feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return None;
    };
    Some(feature_definition_record_id(scan, definition))
}

fn section_line_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    let start = points.get(&segment.point_ids[0])?;
    let end = points.get(&segment.point_ids[1])?;
    Some(SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    })
}

fn section_point_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Point).then_some(())?;
    let position = points.get(&segment.point_ids[0])?;
    Some(SketchGeometry::Point {
        position: cadmpeg_ir::math::Point2::new(position[0], position[1]),
    })
}

fn section_arc_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Arc && segment.arc_orientation == Some(0))
        .then_some(())?;
    let center = points.get(&segment.center_id?)?;
    let first = points.get(&segment.point_ids[0])?;
    let second = points.get(&segment.point_ids[1])?;
    let offset = |point: &[f64; 2]| [point[0] - center[0], point[1] - center[1]];
    let first_offset = offset(first);
    let second_offset = offset(second);
    let first_radius = first_offset[0].hypot(first_offset[1]);
    let second_radius = second_offset[0].hypot(second_offset[1]);
    let scale = first_radius.max(second_radius).max(1.0);
    if first_radius <= 1e-12 || (first_radius - second_radius).abs() > 1e-9 * scale {
        return None;
    }
    let start = second_offset[1].atan2(second_offset[0]);
    let mut end = first_offset[1].atan2(first_offset[0]);
    while end <= start {
        end += std::f64::consts::TAU;
    }
    Some(SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(center[0], center[1]),
        radius: Length(first_radius),
        start_angle: Angle(start),
        end_angle: Angle(end),
    })
}

fn section_opaque_circle_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    radii: &BTreeMap<u32, f64>,
    segment: &crate::feature::FeatureOpaqueSegment,
) -> Option<SketchGeometry> {
    (segment.kind == 10).then_some(())?;
    let center = points.get(&segment.center_id?)?;
    let radius = *radii.get(&segment.radius_ref?)?;
    Some(SketchGeometry::Circle {
        center: Point2::new(center[0], center[1]),
        radius: Length(radius),
    })
}

fn section_opaque_point_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureOpaqueSegment,
) -> Option<SketchGeometry> {
    (segment.kind == 1).then_some(())?;
    let point = points.get(&segment.center_id?)?;
    Some(SketchGeometry::Point {
        position: Point2::new(point[0], point[1]),
    })
}

fn section_opaque_centered_line_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureOpaqueSegment,
) -> Option<SketchGeometry> {
    (segment.kind == 47
        && segment.directions == [Some(0); 3]
        && segment.point_ids == [None, Some(1)]
        && segment.center_id == Some(2))
    .then_some(())?;
    let start = points.get(&0)?;
    let end = points.get(&1)?;
    let center = points.get(&2)?;
    let scale = start
        .iter()
        .chain(end)
        .chain(center)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    ((end[0] - start[0]).hypot(end[1] - start[1]) > 1e-12 * scale).then_some(())?;
    ((start[0] + end[0] - 2.0 * center[0]).hypot(start[1] + end[1] - 2.0 * center[1])
        <= 1e-9 * scale)
        .then_some(())?;
    Some(SketchGeometry::Line {
        start: Point2::new(start[0], start[1]),
        end: Point2::new(end[0], end[1]),
    })
}

fn section_segment_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    section_line_geometry(points, segment)
        .or_else(|| section_arc_geometry(points, segment))
        .or_else(|| section_point_geometry(points, segment))
}

fn saved_section_line_geometry(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    let order_table = definition.order_table.as_ref()?;
    let saved_section = definition.saved_section.as_ref()?;
    let internal_id = order_table
        .internal_id(segment.external_id)
        .or_else(|| {
            let segment_table = definition.segments.as_ref()?;
            segment_table.is_complete().then_some(())?;
            let segments = &segment_table.rows;
            let position = segments
                .iter()
                .position(|candidate| candidate.external_id == segment.external_id)?;
            let previous = segments[..position]
                .iter()
                .rev()
                .find_map(|candidate| order_table.internal_id(candidate.external_id))?;
            let next = segments[position + 1..]
                .iter()
                .find_map(|candidate| order_table.internal_id(candidate.external_id))?;
            let internal_id = previous.checked_add(1)?;
            (next == internal_id.checked_add(1)?
                && saved_section.entities.iter().any(|entity| {
                    matches!(entity, crate::feature::FeatureSavedEntity::Line(line) if line.entity_id == internal_id)
                }))
            .then_some(internal_id)
        })
        .or_else(|| {
            order_table.is_complete().then_some(())?;
            let trimmed = definition.trim_entities.as_ref()?;
            (trimmed.has_complete_bucket_frame() && trimmed.has_unique_external_ids())
                .then_some(())?;
            let segment_table = definition.segments.as_ref()?;
            segment_table.is_complete().then_some(())?;
            let trimmed_external_ids = trimmed
                .rows
                .iter()
                .filter_map(|row| trim_segment_id(definition, row))
                .collect::<BTreeSet<_>>();
            let ordered_external_ids = order_table
                .rows
                .iter()
                .map(|row| row.external_id)
                .collect::<BTreeSet<_>>();
            let ordered_internal_ids = order_table
                .rows
                .iter()
                .map(|row| row.internal_id)
                .collect::<BTreeSet<_>>();
            let segment_ids = segment_table
                .rows
                .iter()
                .filter(|candidate| {
                    candidate.kind == crate::feature::FeatureSegmentKind::Line
                        && trimmed_external_ids.contains(&candidate.external_id)
                        && !ordered_external_ids.contains(&candidate.external_id)
                })
                .map(|candidate| candidate.external_id)
                .collect::<Vec<_>>();
            let saved_ids = saved_section
                .entities
                .iter()
                .filter_map(|entity| match entity {
                    crate::feature::FeatureSavedEntity::Line(line)
                        if !ordered_internal_ids.contains(&line.entity_id) =>
                    {
                        Some(line.entity_id)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            match (segment_ids.as_slice(), saved_ids.as_slice()) {
                ([external_id], [internal_id]) if *external_id == segment.external_id => {
                    Some(*internal_id)
                }
                _ => None,
            }
        });
    let internal_id = internal_id?;
    unique_saved_section_internal_ids(definition)
        .contains(&internal_id)
        .then_some(())?;
    let line = saved_section
        .entities
        .iter()
        .find_map(|entity| match entity {
            crate::feature::FeatureSavedEntity::Line(line) if line.entity_id == internal_id => {
                Some(line)
            }
            _ => None,
        })?;
    let [[Some(start_u), Some(start_v), _], [Some(end_u), Some(end_v), _]] = line.endpoints else {
        return None;
    };
    Some(SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start_u, start_v),
        end: cadmpeg_ir::math::Point2::new(end_u, end_v),
    })
}

fn saved_section_arc_record<'a>(
    definition: &'a crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<&'a crate::feature::FeatureSavedArc> {
    (segment.kind == crate::feature::FeatureSegmentKind::Arc && segment.arc_orientation == Some(0))
        .then_some(())?;
    let internal_id = definition
        .order_table
        .as_ref()?
        .internal_id(segment.external_id)?;
    unique_saved_section_internal_ids(definition)
        .contains(&internal_id)
        .then_some(())?;
    definition
        .saved_section
        .as_ref()?
        .entities
        .iter()
        .find_map(|entity| match entity {
            crate::feature::FeatureSavedEntity::Arc(arc) if arc.entity_id == internal_id => {
                Some(arc)
            }
            _ => None,
        })
}

fn saved_section_arc_carrier(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<([f64; 2], f64)> {
    let arc = saved_section_arc_record(definition, segment)?;
    let [center_u, center_v, _] = arc.center;
    if let ([Some(center_u), Some(center_v)], Some(radius)) = (
        [center_u, center_v],
        arc.radius.filter(|radius| *radius > 1e-12),
    ) {
        return Some(([center_u, center_v], radius));
    }
    let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] = arc.endpoints
    else {
        return None;
    };
    let scale = [first_u, first_v, second_u, second_v]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
    let [center_u, center_v] = match [center_u, center_v] {
        [Some(u), Some(v)] => [u, v],
        [Some(u), None] => {
            let denominator = 2.0 * (second_v - first_v);
            if denominator.abs() <= 1e-12 * scale {
                return None;
            }
            let v = ((second_u - u).mul_add(
                second_u - u,
                second_v * second_v - (first_u - u) * (first_u - u) - first_v * first_v,
            )) / denominator;
            [u, v]
        }
        [None, Some(v)] => {
            let denominator = 2.0 * (second_u - first_u);
            if denominator.abs() <= 1e-12 * scale {
                return None;
            }
            let u = ((second_v - v).mul_add(
                second_v - v,
                second_u * second_u - (first_v - v) * (first_v - v) - first_u * first_u,
            )) / denominator;
            [u, v]
        }
        [None, None] => return None,
    };
    let first_radius = (first_u - center_u).hypot(first_v - center_v);
    let second_radius = (second_u - center_u).hypot(second_v - center_v);
    let radial_scale = first_radius.max(second_radius).max(1.0);
    if first_radius <= 1e-12
        || (first_radius - second_radius).abs() > 1e-9 * radial_scale
        || arc.radius.is_some_and(|stored| {
            (stored - first_radius).abs() > 1e-9 * stored.max(first_radius).max(1.0)
        })
    {
        return None;
    }
    let radius = arc.radius.unwrap_or(first_radius);
    Some(([center_u, center_v], radius))
}

fn saved_section_arc_geometry(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    let arc = saved_section_arc_record(definition, segment)?;
    let ([center_u, center_v], radius) = saved_section_arc_carrier(definition, segment)?;
    let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] = arc.endpoints
    else {
        return None;
    };
    let first = [first_u - center_u, first_v - center_v];
    let second = [second_u - center_u, second_v - center_v];
    let first_radius = first[0].hypot(first[1]);
    let second_radius = second[0].hypot(second[1]);
    let scale = radius.max(first_radius).max(second_radius).max(1.0);
    if (first_radius - radius).abs() > 1e-9 * scale || (second_radius - radius).abs() > 1e-9 * scale
    {
        return None;
    }
    let start = second[1].atan2(second[0]);
    let mut end = first[1].atan2(first[0]);
    while end <= start {
        end += std::f64::consts::TAU;
    }
    Some(SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(center_u, center_v),
        radius: Length(radius),
        start_angle: Angle(start),
        end_angle: Angle(end),
    })
}

fn saved_section_entity_geometry(
    entity: &crate::feature::FeatureSavedEntity,
) -> Option<(u32, SketchGeometry, usize)> {
    match entity {
        crate::feature::FeatureSavedEntity::Line(line) => {
            let [[Some(start_u), Some(start_v), _], [Some(end_u), Some(end_v), _]] = line.endpoints
            else {
                return None;
            };
            Some((
                line.entity_id,
                SketchGeometry::Line {
                    start: Point2::new(start_u, start_v),
                    end: Point2::new(end_u, end_v),
                },
                line.offset,
            ))
        }
        crate::feature::FeatureSavedEntity::Arc(arc) => {
            let ([Some(center_u), Some(center_v)], Some(radius)) = (
                [arc.center[0], arc.center[1]],
                arc.radius.filter(|radius| *radius > 1e-12),
            ) else {
                return None;
            };
            let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] =
                arc.endpoints
            else {
                return None;
            };
            let first = [first_u - center_u, first_v - center_v];
            let second = [second_u - center_u, second_v - center_v];
            let scale = radius
                .max(first[0].hypot(first[1]))
                .max(second[0].hypot(second[1]))
                .max(1.0);
            if (first[0].hypot(first[1]) - radius).abs() > 1e-9 * scale
                || (second[0].hypot(second[1]) - radius).abs() > 1e-9 * scale
            {
                return None;
            }
            let start_angle = second[1].atan2(second[0]);
            let mut end_angle = first[1].atan2(first[0]);
            while end_angle <= start_angle {
                end_angle += std::f64::consts::TAU;
            }
            Some((
                arc.entity_id,
                SketchGeometry::Arc {
                    center: Point2::new(center_u, center_v),
                    radius: Length(radius),
                    start_angle: Angle(start_angle),
                    end_angle: Angle(end_angle),
                },
                arc.offset,
            ))
        }
        crate::feature::FeatureSavedEntity::Circle(circle) => {
            let ([Some(center_u), Some(center_v)], Some(radius)) = (
                [circle.center[0], circle.center[1]],
                circle.radius.filter(|radius| *radius > 1e-12),
            ) else {
                return None;
            };
            Some((
                circle.entity_id,
                SketchGeometry::Circle {
                    center: Point2::new(center_u, center_v),
                    radius: Length(radius),
                },
                circle.offset,
            ))
        }
        crate::feature::FeatureSavedEntity::Spline(_)
        | crate::feature::FeatureSavedEntity::Dummy(_) => None,
    }
}

fn is_full_circle_geometry(geometry: &SketchGeometry) -> bool {
    matches!(geometry, SketchGeometry::Circle { .. })
        || matches!(
            geometry,
            SketchGeometry::Arc {
                start_angle,
                end_angle,
                ..
            } if (end_angle.0 - start_angle.0 - std::f64::consts::TAU).abs() <= 1e-12
        )
}

fn saved_geometry_endpoints(geometry: &SketchGeometry) -> Option<[[f64; 2]; 2]> {
    match geometry {
        SketchGeometry::Line { start, end } => Some([[start.u, start.v], [end.u, end.v]]),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } if !is_full_circle_geometry(geometry) => Some([
            [
                center.u + radius.0 * start_angle.0.cos(),
                center.v + radius.0 * start_angle.0.sin(),
            ],
            [
                center.u + radius.0 * end_angle.0.cos(),
                center.v + radius.0 * end_angle.0.sin(),
            ],
        ]),
        SketchGeometry::Nurbs { control_points, .. } => {
            let first = control_points.first()?;
            let last = control_points.last()?;
            Some([[first.u, first.v], [last.u, last.v]])
        }
        _ => None,
    }
}

fn saved_section_missing_line_geometry(
    definition: &crate::feature::FeatureDefinition,
) -> Option<(usize, SketchGeometry)> {
    let order = definition.order_table.as_ref()?;
    order.is_complete().then_some(())?;
    let segments = definition.segments.as_ref()?;
    segments.is_complete().then_some(())?;
    let trim = definition.trim_entities.as_ref()?;
    (trim.has_complete_bucket_frame() && trim.has_unique_external_ids()).then_some(())?;
    let trimmed_external_ids = trim
        .rows
        .iter()
        .filter_map(|row| trim_segment_id(definition, row))
        .collect::<BTreeSet<_>>();
    let missing = segments
        .rows
        .iter()
        .filter(|candidate| {
            candidate.kind == crate::feature::FeatureSegmentKind::Line
                && order.internal_id(candidate.external_id).is_none()
                && trimmed_external_ids.contains(&candidate.external_id)
        })
        .collect::<Vec<_>>();
    let [missing] = missing.as_slice() else {
        return None;
    };

    let saved = definition.saved_section.as_ref()?;
    let geometries = saved
        .entities
        .iter()
        .filter_map(saved_section_entity_geometry)
        .filter(|(internal_id, _, _)| order.rows.iter().any(|row| row.internal_id == *internal_id))
        .collect::<Vec<_>>();
    let ordered_ids = order
        .rows
        .iter()
        .map(|row| row.internal_id)
        .collect::<BTreeSet<_>>();
    let geometry_ids = geometries
        .iter()
        .map(|(internal_id, _, _)| *internal_id)
        .collect::<BTreeSet<_>>();
    (ordered_ids.len() == order.rows.len()
        && geometry_ids.len() == geometries.len()
        && geometry_ids == ordered_ids)
        .then_some(())?;
    let endpoints = geometries
        .iter()
        .filter_map(|(_, geometry, _)| saved_geometry_endpoints(geometry))
        .flatten()
        .collect::<Vec<_>>();
    (endpoints.len() == 2 * geometries.len()).then_some(())?;
    let mate_counts = endpoints
        .iter()
        .enumerate()
        .map(|(index, endpoint)| {
            endpoints
                .iter()
                .enumerate()
                .filter(|(candidate_index, candidate)| {
                    *candidate_index != index && saved_points_coincide(*endpoint, **candidate)
                })
                .count()
        })
        .collect::<Vec<_>>();
    (mate_counts.iter().filter(|count| **count == 0).count() == 2
        && mate_counts.iter().all(|count| *count <= 1))
    .then_some(())?;
    let open = endpoints
        .iter()
        .zip(mate_counts)
        .filter(|(_, count)| *count == 0)
        .map(|(endpoint, _)| *endpoint)
        .collect::<Vec<_>>();
    let [start, end] = open.as_slice() else {
        return None;
    };
    Some((
        missing.offset,
        SketchGeometry::Line {
            start: Point2::new(start[0], start[1]),
            end: Point2::new(end[0], end[1]),
        },
    ))
}

fn saved_points_coincide(first: [f64; 2], second: [f64; 2]) -> bool {
    let scale = first
        .into_iter()
        .chain(second)
        .map(f64::abs)
        .fold(1.0, f64::max);
    first
        .into_iter()
        .zip(second)
        .all(|(left, right)| (left - right).abs() <= 1e-9 * scale)
}

fn saved_profile_chains(
    sketch: &SketchId,
    geometries: &[(u32, SketchGeometry)],
) -> Vec<Vec<SketchEntityUse>> {
    let mut profiles = geometries
        .iter()
        .filter(|(_, geometry)| is_full_circle_geometry(geometry))
        .map(|(external_id, _)| {
            vec![SketchEntityUse {
                entity: sketch_entity_id(sketch, external_id),
                reversed: false,
            }]
        })
        .collect::<Vec<_>>();
    let rows = geometries
        .iter()
        .filter_map(|(external_id, geometry)| {
            Some((*external_id, saved_geometry_endpoints(geometry)?))
        })
        .collect::<Vec<_>>();
    let mut mates = vec![[None; 2]; rows.len()];
    for (row_index, (_, endpoints)) in rows.iter().enumerate() {
        for endpoint_index in 0..2 {
            let matches = rows
                .iter()
                .enumerate()
                .flat_map(|(candidate_row, (_, candidate_endpoints))| {
                    (0..2).map(move |candidate_endpoint| {
                        (candidate_row, candidate_endpoint, candidate_endpoints)
                    })
                })
                .filter(|(candidate_row, candidate_endpoint, candidate_endpoints)| {
                    (*candidate_row != row_index || *candidate_endpoint != endpoint_index)
                        && saved_points_coincide(
                            endpoints[endpoint_index],
                            candidate_endpoints[*candidate_endpoint],
                        )
                })
                .map(|(candidate_row, candidate_endpoint, _)| (candidate_row, candidate_endpoint))
                .collect::<Vec<_>>();
            if let [mate] = matches.as_slice() {
                mates[row_index][endpoint_index] = Some(*mate);
            }
        }
    }
    let mut remaining = (0..rows.len()).collect::<BTreeSet<_>>();
    while let Some(seed) = remaining
        .iter()
        .min_by_key(|index| rows[**index].0)
        .copied()
    {
        if mates[seed].iter().any(Option::is_none) {
            remaining.remove(&seed);
            continue;
        }
        let mut uses = Vec::new();
        let mut used = BTreeSet::new();
        let mut row = seed;
        let mut reversed = false;
        loop {
            if !used.insert(row) {
                break;
            }
            uses.push(SketchEntityUse {
                entity: sketch_entity_id(sketch, rows[row].0),
                reversed,
            });
            let outgoing = usize::from(!reversed);
            let Some((next_row, next_endpoint)) = mates[row][outgoing] else {
                break;
            };
            row = next_row;
            reversed = next_endpoint == 1;
            if row == seed {
                if !reversed {
                    profiles.push(uses);
                }
                break;
            }
        }
        remaining.retain(|index| !used.contains(index));
    }
    profiles
}

fn resolved_section_segment_geometry(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    let missing_line = saved_section_missing_line_geometry(definition);
    resolved_section_segment_geometry_with_missing_line(
        definition,
        points,
        segment,
        missing_line.as_ref(),
    )
}

fn resolved_section_segment_geometry_with_missing_line(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
    missing_line: Option<&(usize, SketchGeometry)>,
) -> Option<SketchGeometry> {
    let stored = section_segment_geometry(points, segment);
    let saved = saved_section_line_geometry(definition, segment)
        .or_else(|| saved_section_arc_geometry(definition, segment))
        .or_else(|| {
            missing_line
                .filter(|(offset, _)| *offset == segment.offset)
                .map(|(_, geometry)| geometry.clone())
        });
    match (stored, saved) {
        (Some(stored), Some(saved)) => {
            let agree = match (&stored, &saved) {
                (
                    SketchGeometry::Line {
                        start: stored_start,
                        end: stored_end,
                    },
                    SketchGeometry::Line {
                        start: saved_start,
                        end: saved_end,
                    },
                ) => {
                    saved_points_coincide(
                        [stored_start.u, stored_start.v],
                        [saved_start.u, saved_start.v],
                    ) && saved_points_coincide(
                        [stored_end.u, stored_end.v],
                        [saved_end.u, saved_end.v],
                    )
                }
                (
                    SketchGeometry::Arc {
                        center: stored_center,
                        radius: stored_radius,
                        ..
                    },
                    SketchGeometry::Arc {
                        center: saved_center,
                        radius: saved_radius,
                        ..
                    },
                ) => {
                    let radius_scale = stored_radius.0.max(saved_radius.0).max(1.0);
                    saved_points_coincide(
                        [stored_center.u, stored_center.v],
                        [saved_center.u, saved_center.v],
                    ) && (stored_radius.0 - saved_radius.0).abs() <= 1e-9 * radius_scale
                        && saved_geometry_endpoints(&stored)
                            .zip(saved_geometry_endpoints(&saved))
                            .is_some_and(|(stored, saved)| {
                                stored
                                    .into_iter()
                                    .zip(saved)
                                    .all(|(stored, saved)| saved_points_coincide(stored, saved))
                            })
                }
                _ => false,
            };
            agree.then_some(stored)
        }
        (Some(geometry), None) | (None, Some(geometry)) => Some(geometry),
        (None, None) => None,
    }
}

pub(crate) fn resolved_section_coordinates(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, [Option<f64>; 2]> {
    let Some(variables) = &definition.variables else {
        return BTreeMap::new();
    };
    if !variables.is_complete() {
        return BTreeMap::new();
    }
    let (points, ambiguous_point_ids) = variables.reconciled_points();
    let mut segment_counts = BTreeMap::new();
    for segment in definition.segments.iter().flat_map(|table| &table.rows) {
        *segment_counts.entry(segment.external_id).or_insert(0usize) += 1;
    }
    let segments = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line)
        .filter(|segment| segment_counts[&segment.external_id] == 1)
        .filter(|segment| {
            segment
                .point_ids
                .iter()
                .all(|point_id| !ambiguous_point_ids.contains(point_id))
        })
        .collect::<Vec<_>>();
    let coincident_points = active_complete_section_skamps(definition)
        .filter_map(|skamp| {
            let [first, second] = skamp.items.as_slice() else {
                return None;
            };
            let pair = match skamp.kind {
                0 => Some([
                    section_skamp_selected_point(definition, first)?,
                    section_skamp_selected_point(definition, second)?,
                ]),
                3 => {
                    let first_point = section_skamp_point_entity_id(definition, first);
                    let second_point = section_skamp_point_entity_id(definition, second);
                    match (first_point, second_point) {
                        (Some(point), None) => Some([
                            SectionPointSource::Point(point),
                            section_skamp_selected_point(definition, second)?,
                        ]),
                        (None, Some(point)) => Some([
                            section_skamp_selected_point(definition, first)?,
                            SectionPointSource::Point(point),
                        ]),
                        _ => None,
                    }
                }
                _ => None,
            }?;
            (pair
                .iter()
                .any(|point| matches!(point, SectionPointSource::Point(_)))
                && pair.iter().all(|point| match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                }))
            .then_some(pair)
        })
        .collect::<Vec<_>>();
    let same_coordinate_points = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_same_coordinate_sources(definition, skamp))
        .filter(|(pair, _)| {
            pair.iter()
                .any(|point| matches!(point, SectionPointSource::Point(_)))
                && pair.iter().all(|point| match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                })
        })
        .collect::<Vec<_>>();
    let point_on_line_coordinates = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_point_on_line(definition, skamp))
        .filter(|(first, second, _)| {
            !ambiguous_point_ids.contains(first) && !ambiguous_point_ids.contains(second)
        })
        .collect::<Vec<_>>();
    let saved_point_on_line_coordinates = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_saved_point_on_line(definition, skamp))
        .filter(|(point_id, _, _)| !ambiguous_point_ids.contains(point_id))
        .collect::<Vec<_>>();
    let symmetric_point_constraints = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_axis_symmetry(definition, skamp))
        .filter(|(axis, first, second, _)| {
            [first, second]
                .into_iter()
                .any(|point| matches!(point, SectionPointSource::Point(_)))
                && [first, second].into_iter().all(|point| match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                })
                && match axis {
                    SectionSymmetryAxis::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionSymmetryAxis::Value(_) => true,
                }
        })
        .collect::<Vec<_>>();
    let point_symmetric_constraints = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_point_symmetry(definition, skamp))
        .filter(|(center, first, second)| {
            !ambiguous_point_ids.contains(center)
                && [first, second].into_iter().all(|point| match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                })
        })
        .collect::<Vec<_>>();
    let signed_dimension_candidates = definition
        .relations
        .iter()
        .filter(|table| feature_relation_table_complete(table))
        .flat_map(|table| &table.rows)
        .filter_map(|relation| {
            if relation.relation_type != 0 {
                return None;
            }
            let vectors = relation.operand_vectors?;
            if !section_linear_distance_vectors(vectors) {
                return None;
            }
            let [Some(first), Some(second), _, _] = vectors[0] else {
                return None;
            };
            let mut matching_segments = segments.iter().filter(|segment| {
                segment.point_ids == [first, second] || segment.point_ids == [second, first]
            });
            let segment = matching_segments.next()?;
            matching_segments.next().is_none().then_some(())?;
            let coordinate =
                1usize.checked_sub(section_line_fixed_coordinate(definition, segment)?)?;
            let magnitude = section_relation_length_dimension(definition, relation)?
                .value
                .filter(|value| value.is_finite() && *value >= 0.0)?;
            let delta = match relation.sign {
                1 => magnitude,
                0xf6 => -magnitude,
                0 => match segment.directions[0] {
                    Some(1) => magnitude,
                    None => -magnitude,
                    _ => return None,
                },
                _ => return None,
            };
            Some((first, second, coordinate, delta))
        })
        .collect::<Vec<_>>();
    let mut signed_dimensions = BTreeMap::<(u32, u32, usize), Option<f64>>::new();
    for (first, second, coordinate, delta) in signed_dimension_candidates {
        let (key, canonical_delta) = if first <= second {
            ((first, second, coordinate), delta)
        } else {
            ((second, first, coordinate), -delta)
        };
        signed_dimensions
            .entry(key)
            .and_modify(|stored| {
                if stored.is_some_and(|stored| stored != canonical_delta) {
                    *stored = None;
                }
            })
            .or_insert(Some(canonical_delta));
    }
    let signed_dimensions = signed_dimensions
        .into_iter()
        .filter_map(|((first, second, coordinate), delta)| {
            Some((first, second, coordinate, delta?))
        })
        .collect::<Vec<_>>();
    let mut equations = Vec::new();
    for (&point_id, coordinates) in &points {
        for (coordinate, value) in coordinates.iter().copied().enumerate() {
            if let Some(value) = value {
                equations.push(SectionCoordinateEquation::point_value(
                    point_id, coordinate, value,
                ));
            }
        }
    }
    for segment in &segments {
        if let Some(coordinate) = section_line_fixed_coordinate(definition, segment) {
            equations.push(SectionCoordinateEquation::point_difference(
                segment.point_ids[0],
                segment.point_ids[1],
                coordinate,
                0.0,
            ));
        }
    }
    for &(first, second, coordinate, delta) in &signed_dimensions {
        equations.push(SectionCoordinateEquation::point_difference(
            first, second, coordinate, delta,
        ));
    }
    for &[first, second] in &coincident_points {
        for coordinate in 0..2 {
            equations.push(SectionCoordinateEquation::source_difference(
                first, second, coordinate, 0.0,
            ));
        }
    }
    for &([first, second], coordinate) in &same_coordinate_points {
        equations.push(SectionCoordinateEquation::source_difference(
            first, second, coordinate, 0.0,
        ));
    }
    for &(first, second, coordinate) in &point_on_line_coordinates {
        equations.push(SectionCoordinateEquation::point_difference(
            first, second, coordinate, 0.0,
        ));
    }
    for &(point, coordinate, value) in &saved_point_on_line_coordinates {
        equations.push(SectionCoordinateEquation::point_value(
            point, coordinate, value,
        ));
    }
    for &(axis, first, second, fixed_coordinate) in &symmetric_point_constraints {
        let parallel_coordinate = 1usize.saturating_sub(fixed_coordinate);
        equations.push(SectionCoordinateEquation::source_difference(
            first,
            second,
            parallel_coordinate,
            0.0,
        ));
        let mut equation = SectionCoordinateEquation::default();
        equation.add_source(first, fixed_coordinate, 1.0);
        equation.add_source(second, fixed_coordinate, 1.0);
        match axis {
            SectionSymmetryAxis::Point(point_id) => {
                equation.add_point(point_id, fixed_coordinate, -2.0);
            }
            SectionSymmetryAxis::Value(value) => equation.rhs += 2.0 * value,
        }
        equations.push(equation);
    }
    for &(center, first, second) in &point_symmetric_constraints {
        for coordinate in 0..2 {
            let mut equation = SectionCoordinateEquation::default();
            equation.add_source(first, coordinate, 1.0);
            equation.add_source(second, coordinate, 1.0);
            equation.add_point(center, coordinate, -2.0);
            equations.push(equation);
        }
    }
    let stored_coordinates = points
        .into_iter()
        .flat_map(|(point, coordinates)| {
            coordinates
                .into_iter()
                .enumerate()
                .filter_map(move |(coordinate, value)| Some(((point, coordinate), value?)))
        })
        .collect();
    solve_section_coordinate_equations(&equations, &stored_coordinates)
}

pub(crate) fn resolved_section_points(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, [f64; 2]> {
    resolved_section_coordinates(definition)
        .into_iter()
        .filter_map(|(point, [u, v])| Some((point, [u?, v?])))
        .collect()
}

type SectionCoordinateVariable = (u32, usize);

#[derive(Default)]
struct SectionCoordinateEquation {
    terms: BTreeMap<SectionCoordinateVariable, f64>,
    rhs: f64,
}

impl SectionCoordinateEquation {
    fn point_value(point: u32, coordinate: usize, value: f64) -> Self {
        let mut equation = Self::default();
        equation.add_point(point, coordinate, 1.0);
        equation.rhs = value;
        equation
    }

    fn point_difference(first: u32, second: u32, coordinate: usize, delta: f64) -> Self {
        let mut equation = Self::default();
        equation.add_point(first, coordinate, -1.0);
        equation.add_point(second, coordinate, 1.0);
        equation.rhs = delta;
        equation
    }

    fn source_difference(
        first: SectionPointSource,
        second: SectionPointSource,
        coordinate: usize,
        delta: f64,
    ) -> Self {
        let mut equation = Self::default();
        equation.add_source(first, coordinate, -1.0);
        equation.add_source(second, coordinate, 1.0);
        equation.rhs += delta;
        equation
    }

    fn add_point(&mut self, point: u32, coordinate: usize, coefficient: f64) {
        *self.terms.entry((point, coordinate)).or_default() += coefficient;
    }

    fn add_source(&mut self, source: SectionPointSource, coordinate: usize, coefficient: f64) {
        match source {
            SectionPointSource::Point(point) => self.add_point(point, coordinate, coefficient),
            SectionPointSource::Value(value) => self.rhs -= coefficient * value[coordinate],
        }
    }
}

fn solve_section_coordinate_equations(
    equations: &[SectionCoordinateEquation],
    stored_coordinates: &BTreeMap<SectionCoordinateVariable, f64>,
) -> BTreeMap<u32, [Option<f64>; 2]> {
    let variables = equations
        .iter()
        .flat_map(|equation| equation.terms.keys().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let indices = variables
        .iter()
        .enumerate()
        .map(|(index, variable)| (*variable, index))
        .collect::<BTreeMap<_, _>>();
    let mut adjacency = vec![BTreeSet::new(); variables.len()];
    let mut variable_equations = vec![BTreeSet::new(); variables.len()];
    for (equation_index, equation) in equations.iter().enumerate() {
        let members = equation
            .terms
            .keys()
            .filter_map(|variable| indices.get(variable).copied())
            .collect::<Vec<_>>();
        for &first in &members {
            adjacency[first].extend(members.iter().copied().filter(|second| *second != first));
            variable_equations[first].insert(equation_index);
        }
    }
    let mut solved = BTreeMap::<SectionCoordinateVariable, f64>::new();
    let mut remaining = (0..variables.len()).collect::<BTreeSet<_>>();
    while let Some(seed) = remaining.pop_first() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = std::collections::VecDeque::from([seed]);
        while let Some(variable) = pending.pop_front() {
            for &neighbor in &adjacency[variable] {
                if component.insert(neighbor) {
                    remaining.remove(&neighbor);
                    pending.push_back(neighbor);
                }
            }
        }
        let columns = component.iter().copied().collect::<Vec<_>>();
        let local_columns = columns
            .iter()
            .enumerate()
            .map(|(local, global)| (*global, local))
            .collect::<BTreeMap<_, _>>();
        let component_equations = component
            .iter()
            .flat_map(|variable| variable_equations[*variable].iter().copied())
            .collect::<BTreeSet<_>>();
        let mut matrix = component_equations
            .into_iter()
            .map(|equation_index| &equations[equation_index])
            .map(|equation| {
                let mut row = SectionLinearRow {
                    coefficients: BTreeMap::new(),
                    rhs: equation.rhs,
                };
                for (variable, coefficient) in &equation.terms {
                    let global = indices[variable];
                    if *coefficient != 0.0 {
                        row.coefficients
                            .insert(local_columns[&global], *coefficient);
                    }
                }
                row
            })
            .collect::<Vec<_>>();
        let Some(component_solution) = uniquely_solved_linear_variables(&mut matrix, columns.len())
        else {
            for global in columns {
                let variable = variables[global];
                if let Some(value) = stored_coordinates.get(&variable) {
                    solved.insert(variable, *value);
                }
            }
            continue;
        };
        for (local, value) in component_solution {
            solved.insert(variables[columns[local]], value);
        }
    }
    let mut points = BTreeMap::<u32, [Option<f64>; 2]>::new();
    for ((point, coordinate), value) in solved {
        points.entry(point).or_insert([None; 2])[coordinate] = Some(value);
    }
    points
}

struct SectionLinearRow {
    coefficients: BTreeMap<usize, f64>,
    rhs: f64,
}

fn uniquely_solved_linear_variables(
    matrix: &mut [SectionLinearRow],
    variable_count: usize,
) -> Option<Vec<(usize, f64)>> {
    let coefficient_scale = matrix
        .iter()
        .flat_map(|row| row.coefficients.values())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let rhs_scale = matrix.iter().map(|row| row.rhs.abs()).fold(1.0, f64::max);
    let coefficient_tolerance = 1e-12 * coefficient_scale;
    let residual_tolerance = 1e-9 * rhs_scale;
    let mut pivot_rows = BTreeMap::new();
    let mut pivot_row = 0;
    for column in 0..variable_count {
        let Some(selected) = (pivot_row..matrix.len()).max_by(|&first, &second| {
            matrix[first]
                .coefficients
                .get(&column)
                .copied()
                .unwrap_or(0.0)
                .abs()
                .total_cmp(
                    &matrix[second]
                        .coefficients
                        .get(&column)
                        .copied()
                        .unwrap_or(0.0)
                        .abs(),
                )
        }) else {
            break;
        };
        let divisor = matrix[selected]
            .coefficients
            .get(&column)
            .copied()
            .unwrap_or(0.0);
        if divisor.abs() <= coefficient_tolerance {
            continue;
        }
        matrix.swap(pivot_row, selected);
        for value in matrix[pivot_row].coefficients.values_mut() {
            *value /= divisor;
        }
        matrix[pivot_row].rhs /= divisor;
        let pivot_coefficients = matrix[pivot_row].coefficients.clone();
        let pivot_rhs = matrix[pivot_row].rhs;
        for (row, target) in matrix.iter_mut().enumerate() {
            if row == pivot_row {
                continue;
            }
            let factor = target.coefficients.get(&column).copied().unwrap_or(0.0);
            if factor.abs() <= coefficient_tolerance {
                continue;
            }
            for (&index, &pivot_value) in &pivot_coefficients {
                let value = target.coefficients.entry(index).or_default();
                *value -= factor * pivot_value;
                if value.abs() <= coefficient_tolerance {
                    target.coefficients.remove(&index);
                }
            }
            target.rhs -= factor * pivot_rhs;
        }
        pivot_rows.insert(column, pivot_row);
        pivot_row += 1;
    }
    if matrix
        .iter()
        .any(|row| row.coefficients.is_empty() && row.rhs.abs() > residual_tolerance)
    {
        return None;
    }
    let free_columns = (0..variable_count)
        .filter(|column| !pivot_rows.contains_key(column))
        .collect::<Vec<_>>();
    Some(
        pivot_rows
            .into_iter()
            .filter_map(|(column, row)| {
                free_columns
                    .iter()
                    .all(|free| !matrix[row].coefficients.contains_key(free))
                    .then_some((column, matrix[row].rhs))
            })
            .collect(),
    )
}

fn section_line_fixed_coordinate(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<usize> {
    let segment = unique_section_skamp_segment(definition, segment.external_id)?;
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    section_line_entity_fixed_coordinate(definition, segment.external_id)
}

fn section_line_entity_fixed_coordinate(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> Option<usize> {
    let mut adjacency = BTreeMap::<u32, Vec<(u32, usize)>>::new();
    for skamp in active_complete_section_skamps(definition) {
        let (parity, first, second) = match (skamp.kind, skamp.items.as_slice()) {
            (5 | 7, [first, second]) if first.sense == 0 && second.sense == 0 => {
                ((skamp.kind == 5) as usize, first, second)
            }
            _ => continue,
        };
        if !section_skamp_is_line(definition, first) || !section_skamp_is_line(definition, second) {
            continue;
        }
        adjacency
            .entry(first.entity_id)
            .or_default()
            .push((second.entity_id, parity));
        adjacency
            .entry(second.entity_id)
            .or_default()
            .push((first.entity_id, parity));
    }
    let mut parities = BTreeMap::from([(entity_id, 0usize)]);
    let mut pending = std::collections::VecDeque::from([entity_id]);
    while let Some(entity_id) = pending.pop_front() {
        let parity = parities[&entity_id];
        for &(neighbor, edge_parity) in adjacency.get(&entity_id).into_iter().flatten() {
            let neighbor_parity = parity ^ edge_parity;
            match parities.get(&neighbor) {
                Some(stored) if *stored != neighbor_parity => return None,
                Some(_) => {}
                None => {
                    parities.insert(neighbor, neighbor_parity);
                    pending.push_back(neighbor);
                }
            }
        }
    }
    let mut coordinates = BTreeSet::new();
    for (entity_id, parity) in parities {
        coordinates.extend(
            section_line_direct_fixed_coordinates(definition, entity_id)
                .into_iter()
                .map(|coordinate| coordinate ^ parity),
        );
    }
    coordinates
        .first()
        .copied()
        .filter(|_| coordinates.len() == 1)
}

fn section_line_direct_fixed_coordinates(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> BTreeSet<usize> {
    let mut coordinates = unique_section_skamp_segment(definition, entity_id)
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line)
        .and_then(|segment| segment.vertical_horizontal)
        .and_then(|selector| match selector {
            0 => Some(0),
            1 => Some(1),
            _ => None,
        })
        .into_iter()
        .collect::<BTreeSet<_>>();
    coordinates.extend(
        active_complete_section_skamps(definition).filter_map(|skamp| {
            match (skamp.kind, skamp.items.as_slice()) {
                (1, [item]) if item.sense == 0 && item.entity_id == entity_id => Some(1),
                (2, [item]) if item.sense == 0 && item.entity_id == entity_id => Some(0),
                _ => None,
            }
        }),
    );
    if let Some(crate::feature::FeatureSavedEntity::Line(line)) =
        section_saved_entity(definition, entity_id)
    {
        let [[Some(x0), Some(y0), _], [Some(x1), Some(y1), _]] = line.endpoints else {
            return coordinates;
        };
        let scale = [x0, y0, x1, y1]
            .into_iter()
            .map(f64::abs)
            .fold(1.0, f64::max);
        let tolerance = 1e-9 * scale;
        match [(x0 - x1).abs() <= tolerance, (y0 - y1).abs() <= tolerance] {
            [true, false] => {
                coordinates.insert(0);
            }
            [false, true] => {
                coordinates.insert(1);
            }
            _ => {}
        }
    }
    coordinates
}

fn section_skamp_point_on_line(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(u32, u32, usize)> {
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let pair = match skamp.kind {
        3 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                let line = unique_section_skamp_segment(definition, line_item.entity_id)?;
                (line_item.sense == 0 && line.kind == crate::feature::FeatureSegmentKind::Line)
                    .then_some((
                        line,
                        section_skamp_selected_point_id(definition, point_item)?,
                    ))
            }),
        9 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                let line = unique_section_skamp_segment(definition, line_item.entity_id)?;
                let point = unique_section_skamp_segment(definition, point_item.entity_id)?;
                (line_item.sense == 0
                    && point_item.sense == 0
                    && line.kind == crate::feature::FeatureSegmentKind::Line
                    && point.kind == crate::feature::FeatureSegmentKind::Point)
                    .then_some((line, point.point_ids[0]))
            }),
        _ => None,
    }?;
    let coordinate = section_line_fixed_coordinate(definition, pair.0)?;
    Some((pair.0.point_ids[0], pair.1, coordinate))
}

fn section_skamp_saved_point_on_line(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(u32, usize, f64)> {
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let (line_item, point_id) = match skamp.kind {
        3 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                if line_item.sense != 0 {
                    return None;
                }
                Some((
                    line_item,
                    section_skamp_selected_point_id(definition, point_item)?,
                ))
            }),
        9 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                if line_item.sense != 0
                    || point_item.sense != 0
                    || !section_skamp_is_point(definition, point_item)
                {
                    return None;
                }
                Some((
                    line_item,
                    unique_section_skamp_segment(definition, point_item.entity_id)?.point_ids[0],
                ))
            }),
        _ => None,
    }?;
    if definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == line_item.entity_id)
    {
        return None;
    }
    let crate::feature::FeatureSavedEntity::Line(line) =
        section_saved_entity(definition, line_item.entity_id)?
    else {
        return None;
    };
    let coordinate = section_line_entity_fixed_coordinate(definition, line_item.entity_id)?;
    Some((
        point_id,
        coordinate,
        saved_line_fixed_coordinate_value(line, coordinate)?,
    ))
}

#[derive(Clone, Copy)]
enum SectionSymmetryAxis {
    Point(u32),
    Value(f64),
}

fn section_skamp_axis_symmetry(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(
    SectionSymmetryAxis,
    SectionPointSource,
    SectionPointSource,
    usize,
)> {
    let (14, [axis_item, first_item, second_item]) = (skamp.kind, skamp.items.as_slice()) else {
        return None;
    };
    (axis_item.sense == 0 && section_skamp_is_line(definition, axis_item)).then_some(())?;
    let coordinate = section_line_entity_fixed_coordinate(definition, axis_item.entity_id)?;
    let axis = if let Some(segment) = unique_section_skamp_segment(definition, axis_item.entity_id)
    {
        SectionSymmetryAxis::Point(segment.point_ids[0])
    } else {
        let crate::feature::FeatureSavedEntity::Line(line) =
            section_saved_entity(definition, axis_item.entity_id)?
        else {
            return None;
        };
        SectionSymmetryAxis::Value(saved_line_fixed_coordinate_value(line, coordinate)?)
    };
    Some((
        axis,
        section_skamp_selected_point(definition, first_item)?,
        section_skamp_selected_point(definition, second_item)?,
        coordinate,
    ))
}

fn section_skamp_point_symmetry(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(u32, SectionPointSource, SectionPointSource)> {
    let (14, [center, first, second]) = (skamp.kind, skamp.items.as_slice()) else {
        return None;
    };
    Some((
        section_skamp_point_entity_id(definition, center)?,
        section_skamp_selected_point(definition, first)?,
        section_skamp_selected_point(definition, second)?,
    ))
}

fn saved_line_fixed_coordinate_value(
    line: &crate::feature::FeatureSavedLine,
    coordinate: usize,
) -> Option<f64> {
    let [Some(first), Some(second)] =
        [line.endpoints[0][coordinate], line.endpoints[1][coordinate]]
    else {
        return None;
    };
    let scale = first.abs().max(second.abs()).max(1.0);
    ((first - second).abs() <= 1e-9 * scale).then_some(first)
}

#[derive(Clone, Copy)]
enum SectionPointSource {
    Point(u32),
    Value([f64; 2]),
}

fn unique_section_skamp_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureSegment> {
    definition.segments.as_ref()?.segment(external_id)
}

fn unique_decoded_section_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureSegment> {
    let segments = definition.segments.as_ref()?;
    let mut matches = segments
        .rows
        .iter()
        .filter(|segment| segment.external_id == external_id);
    let segment = matches.next()?;
    (matches.next().is_none()
        && !segments
            .opaque_rows
            .iter()
            .any(|row| row.external_id == external_id))
    .then_some(segment)
}

fn section_segment_rows(
    definition: &crate::feature::FeatureDefinition,
) -> &[crate::feature::FeatureSegment] {
    definition
        .segments
        .as_ref()
        .map_or(&[], |table| table.rows.as_slice())
}

fn complete_section_segment_rows(
    definition: &crate::feature::FeatureDefinition,
) -> &[crate::feature::FeatureSegment] {
    definition
        .segments
        .as_ref()
        .filter(|table| table.is_complete())
        .map_or(&[], |table| table.rows.as_slice())
}

fn section_skamp_point_entity_id(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<u32> {
    let segment = unique_section_skamp_segment(definition, item.entity_id)?;
    (item.sense == 0 && segment.kind == crate::feature::FeatureSegmentKind::Point)
        .then_some(segment.point_ids[0])
}

fn section_skamp_selected_point_id(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<u32> {
    let segment = unique_section_skamp_segment(definition, item.entity_id)?;
    match item.sense {
        2 => Some(segment.point_ids[0]),
        3 => Some(segment.point_ids[1]),
        4 => segment.center_id,
        _ => None,
    }
}

fn section_skamp_selected_point(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SectionPointSource> {
    section_skamp_selected_point_id(definition, item)
        .map(SectionPointSource::Point)
        .or_else(|| saved_section_point(definition, item).map(SectionPointSource::Value))
}

fn saved_section_point(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<[f64; 2]> {
    if definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == item.entity_id)
    {
        return None;
    }
    let coordinates = match (
        section_saved_entity(definition, item.entity_id)?,
        item.sense,
    ) {
        (crate::feature::FeatureSavedEntity::Line(line), 2) => line.endpoints[0],
        (crate::feature::FeatureSavedEntity::Line(line), 3) => line.endpoints[1],
        (crate::feature::FeatureSavedEntity::Arc(arc), 2) => arc.endpoints[0],
        (crate::feature::FeatureSavedEntity::Arc(arc), 3) => arc.endpoints[1],
        (crate::feature::FeatureSavedEntity::Arc(arc), 4) => arc.center,
        (crate::feature::FeatureSavedEntity::Circle(circle), 4) => circle.center,
        _ => return None,
    };
    let [Some(u), Some(v), _] = coordinates else {
        return None;
    };
    (u.is_finite() && v.is_finite()).then_some([u, v])
}

pub(crate) fn resolved_section_radii(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, f64> {
    let mut candidates = BTreeMap::<u32, Vec<f64>>::new();
    for row in definition
        .variables
        .iter()
        .filter(|table| table.is_complete())
        .flat_map(|table| &table.rows)
    {
        if row.variable_type == 3 {
            if let Some(value) = row.value.filter(|value| value.is_finite() && *value > 0.0) {
                candidates.entry(row.key).or_default().push(value);
            }
        }
    }
    for relation in definition
        .relations
        .iter()
        .filter(|table| feature_relation_table_complete(table))
        .flat_map(|table| &table.rows)
    {
        if relation.relation_type != 14 || relation.sign != 1 {
            continue;
        }
        let Some(vectors) = relation.operand_vectors else {
            continue;
        };
        let [Some(radius_id), Some(0), Some(0), Some(0)] = vectors[0] else {
            continue;
        };
        if vectors[1] != [Some(0); 4] || vectors[2] != [Some(15), Some(0), Some(0), Some(0)] {
            continue;
        }
        let Some(dimension) = section_relation_length_dimension(definition, relation) else {
            continue;
        };
        let Some(value) = dimension
            .value
            .filter(|value| value.is_finite() && *value > 0.0)
        else {
            continue;
        };
        let value = if dimension.dimension_type == 4 {
            value / 2.0
        } else {
            value
        };
        candidates.entry(radius_id).or_default().push(value);
    }
    if let Some(dimensions) = definition
        .dimensions
        .as_ref()
        .filter(|dimensions| feature_dimension_table_complete(dimensions))
    {
        for circle in definition
            .segments
            .iter()
            .flat_map(|segments| &segments.opaque_rows)
            .filter(|segment| {
                segment.kind == 10
                    && unique_opaque_section_segment(definition, segment.external_id, 10)
                        .is_some_and(|candidate| candidate == *segment)
            })
        {
            let Some(radius_id) = circle.radius_ref else {
                continue;
            };
            let Some(dimension) = dimensions
                .rows
                .get(usize::try_from(radius_id).unwrap_or(usize::MAX))
            else {
                continue;
            };
            let Some(value) = dimension
                .value
                .filter(|value| value.is_finite() && *value > 0.0)
            else {
                continue;
            };
            let radius = match dimension.dimension_type {
                3 => value,
                4 => value / 2.0,
                _ => continue,
            };
            candidates.entry(radius_id).or_default().push(radius);
        }
    }
    let points = resolved_section_points(definition);
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
    {
        if unique_section_skamp_segment(definition, segment.external_id) != Some(segment) {
            continue;
        }
        let Some(radius_id) = segment.radius_ref else {
            continue;
        };
        let Some(center) = segment.center_id.and_then(|id| points.get(&id)) else {
            continue;
        };
        let endpoint_radii = segment
            .point_ids
            .iter()
            .filter_map(|id| points.get(id))
            .map(|point| (point[0] - center[0]).hypot(point[1] - center[1]))
            .filter(|radius| radius.is_finite() && *radius > 1e-12)
            .collect::<Vec<_>>();
        let Some(radius) = endpoint_radii.first().copied() else {
            continue;
        };
        let scale = endpoint_radii
            .iter()
            .copied()
            .fold(radius.max(1.0), f64::max);
        if endpoint_radii
            .iter()
            .all(|candidate| (*candidate - radius).abs() <= 1e-9 * scale)
        {
            candidates.entry(radius_id).or_default().push(radius);
        }
    }
    let mut adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    for skamp in active_complete_section_skamps(definition) {
        let [first, second] = skamp.items.as_slice() else {
            continue;
        };
        if skamp.kind != 6 || first.sense != 0 || second.sense != 0 {
            continue;
        }
        let Some(first_radius) = section_skamp_radius_source(definition, first) else {
            continue;
        };
        let Some(second_radius) = section_skamp_radius_source(definition, second) else {
            continue;
        };
        match (first_radius, second_radius) {
            (SectionRadiusSource::Reference(first), SectionRadiusSource::Reference(second)) => {
                adjacency.entry(first).or_default().insert(second);
                adjacency.entry(second).or_default().insert(first);
            }
            (SectionRadiusSource::Reference(reference), SectionRadiusSource::Value(value))
            | (SectionRadiusSource::Value(value), SectionRadiusSource::Reference(reference)) => {
                candidates.entry(reference).or_default().push(value);
            }
            (SectionRadiusSource::Value(_), SectionRadiusSource::Value(_)) => {}
        }
    }
    let mut remaining = candidates
        .keys()
        .chain(adjacency.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut radii = BTreeMap::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = std::collections::VecDeque::from([seed]);
        while let Some(radius_id) = pending.pop_front() {
            for neighbor in adjacency.get(&radius_id).into_iter().flatten() {
                if component.insert(*neighbor) {
                    pending.push_back(*neighbor);
                }
            }
        }
        let values = component
            .iter()
            .flat_map(|radius_id| candidates.get(radius_id).into_iter().flatten())
            .copied()
            .collect::<Vec<_>>();
        if let Some(value) = values.first().copied() {
            let scale = values.iter().copied().fold(value.max(1.0), f64::max);
            if !values
                .iter()
                .all(|candidate| (*candidate - value).abs() <= 1e-9 * scale)
            {
                remaining.retain(|radius_id| !component.contains(radius_id));
                continue;
            }
            radii.extend(component.iter().map(|radius_id| (*radius_id, value)));
        }
        remaining.retain(|radius_id| !component.contains(radius_id));
    }
    radii
}

fn section_relation_length_dimension<'a>(
    definition: &'a crate::feature::FeatureDefinition,
    relation: &crate::feature::FeatureRelation,
) -> Option<&'a crate::feature::FeatureDimension> {
    let dimension = definition
        .dimensions
        .as_ref()?
        .rows
        .get(usize::try_from(relation.dimension_id).ok()?)?;
    (dimension.value_unit == crate::feature::DimensionUnit::Millimeters).then_some(dimension)
}

#[derive(Clone, Copy)]
enum SectionRadiusSource {
    Reference(u32),
    Value(f64),
}

fn section_skamp_radius_source(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SectionRadiusSource> {
    if let Some(circle) = unique_opaque_section_segment(definition, item.entity_id, 10) {
        return circle.radius_ref.map(SectionRadiusSource::Reference);
    }
    if let Some(segment) = unique_section_skamp_segment(definition, item.entity_id) {
        return (segment.kind == crate::feature::FeatureSegmentKind::Arc)
            .then_some(segment.radius_ref)
            .flatten()
            .map(SectionRadiusSource::Reference);
    }
    if definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == item.entity_id)
    {
        return None;
    }
    let radius = match section_saved_entity(definition, item.entity_id)? {
        crate::feature::FeatureSavedEntity::Arc(arc) => arc.radius,
        crate::feature::FeatureSavedEntity::Circle(circle) => circle.radius,
        _ => None,
    }?;
    (radius.is_finite() && radius > 0.0).then_some(SectionRadiusSource::Value(radius))
}

fn section_arc_carrier(
    radii: &BTreeMap<u32, f64>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<([f64; 2], f64)> {
    (segment.kind == crate::feature::FeatureSegmentKind::Arc).then_some(())?;
    let center = *points.get(&segment.center_id?)?;
    let radius = *radii.get(&segment.radius_ref?)?;
    Some((center, radius))
}

#[derive(Clone)]
struct SectionIntersectionCarrier {
    geometry: SketchGeometry,
    line_is_bounded: bool,
}

fn section_axis_line_carrier_with_points(
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    let endpoint = |id| variable_points.get(&id);
    let [first, second] = segment.point_ids.map(endpoint);
    let (Some(first), Some(second)) = (first, second) else {
        return None;
    };
    let scale = first[0]
        .into_iter()
        .chain(first[1])
        .chain(second[0])
        .chain(second[1])
        .map(f64::abs)
        .fold(1.0, f64::max);
    if segment.directions[0] == Some(0) {
        let (Some(first_u), Some(second_u)) = (first[0], second[0]) else {
            return None;
        };
        ((first_u - second_u).abs() <= 1e-9 * scale).then(|| SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(first_u, -scale),
            end: cadmpeg_ir::math::Point2::new(first_u, scale),
        })
    } else if segment.directions[1] == Some(0) {
        let (Some(first_v), Some(second_v)) = (first[1], second[1]) else {
            return None;
        };
        ((first_v - second_v).abs() <= 1e-9 * scale).then(|| SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-scale, first_v),
            end: cadmpeg_ir::math::Point2::new(scale, first_v),
        })
    } else {
        None
    }
}

fn section_segment_intersection_carrier_with_missing_line(
    definition: &crate::feature::FeatureDefinition,
    radii: &BTreeMap<u32, f64>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
    missing_line: Option<&(usize, SketchGeometry)>,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Option<SectionIntersectionCarrier> {
    if let Some(geometry) = resolved_section_segment_geometry_with_missing_line(
        definition,
        points,
        segment,
        missing_line,
    ) {
        return Some(SectionIntersectionCarrier {
            line_is_bounded: matches!(geometry, SketchGeometry::Line { .. }),
            geometry,
        });
    }
    if let Some(geometry) = section_axis_line_carrier_with_points(variable_points, segment) {
        return Some(SectionIntersectionCarrier {
            geometry,
            line_is_bounded: false,
        });
    }
    let ([center_u, center_v], radius) = section_arc_carrier(radii, points, segment)
        .or_else(|| saved_section_arc_carrier(definition, segment))?;
    Some(SectionIntersectionCarrier {
        geometry: SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(center_u, center_v),
            radius: Length(radius),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::TAU),
        },
        line_is_bounded: false,
    })
}

fn trim_segment_id(
    definition: &crate::feature::FeatureDefinition,
    row: &crate::feature::FeatureTrimEntity,
) -> Option<u32> {
    let trim_table = definition.trim_entities.as_ref()?;
    (trim_table.has_complete_bucket_frame() && trim_table.has_unique_external_ids())
        .then_some(())?;
    let Some(segment_table) = &definition.segments else {
        return Some(row.external_id);
    };
    segment_table.is_complete().then_some(())?;
    let segments = &segment_table.rows;
    let trim_rows = &trim_table.rows;
    let matching_segment_count = segments
        .iter()
        .filter(|segment| segment.external_id == row.external_id)
        .count();
    let matching_trim_count = trim_rows
        .iter()
        .filter(|trim| trim.external_id == row.external_id)
        .count();
    if matching_segment_count == 1 && matching_trim_count == 1 {
        return Some(row.external_id);
    }
    if matching_segment_count != 0 || matching_trim_count != 1 {
        return None;
    }
    let unmatched_segments = segments
        .iter()
        .filter(|segment| {
            !trim_rows
                .iter()
                .any(|trim| trim.external_id == segment.external_id)
        })
        .map(|segment| segment.external_id)
        .collect::<Vec<_>>();
    let unmatched_rows = trim_rows
        .iter()
        .filter(|trim| {
            !segments
                .iter()
                .any(|segment| segment.external_id == trim.external_id)
        })
        .collect::<Vec<_>>();
    match (unmatched_segments.as_slice(), unmatched_rows.as_slice()) {
        ([segment_id], [unmatched]) if std::ptr::eq(*unmatched, row) => Some(*segment_id),
        _ => None,
    }
}

fn intersect_section_lines(first: &SketchGeometry, second: &SketchGeometry) -> Option<[f64; 2]> {
    let (
        SketchGeometry::Line {
            start: first_start,
            end: first_end,
        },
        SketchGeometry::Line {
            start: second_start,
            end: second_end,
        },
    ) = (first, second)
    else {
        return None;
    };
    let denominator = (first_start.u - first_end.u).mul_add(
        second_start.v - second_end.v,
        -(first_start.v - first_end.v) * (second_start.u - second_end.u),
    );
    let scale = (first_start.u - first_end.u)
        .abs()
        .max((first_start.v - first_end.v).abs())
        .max((second_start.u - second_end.u).abs())
        .max((second_start.v - second_end.v).abs())
        .max(1.0);
    if denominator.abs() <= 1e-12 * scale * scale {
        return None;
    }
    let first_cross = first_start
        .u
        .mul_add(first_end.v, -(first_start.v * first_end.u));
    let second_cross = second_start
        .u
        .mul_add(second_end.v, -(second_start.v * second_end.u));
    Some([
        first_cross.mul_add(
            second_start.u - second_end.u,
            -(first_start.u - first_end.u) * second_cross,
        ) / denominator,
        first_cross.mul_add(
            second_start.v - second_end.v,
            -(first_start.v - first_end.v) * second_cross,
        ) / denominator,
    ])
}

fn intersect_section_line_arc(first: &SketchGeometry, second: &SketchGeometry) -> Option<[f64; 2]> {
    let (
        (line @ SketchGeometry::Line { .. }, arc @ SketchGeometry::Arc { .. })
        | (arc @ SketchGeometry::Arc { .. }, line @ SketchGeometry::Line { .. }),
    ) = ((first, second),)
    else {
        return None;
    };
    let SketchGeometry::Line { start, end } = line else {
        return None;
    };
    let SketchGeometry::Arc { center, radius, .. } = arc else {
        return None;
    };
    let direction = [end.u - start.u, end.v - start.v];
    let length = direction[0].hypot(direction[1]);
    if length <= 1e-12 || radius.0 <= 1e-12 {
        return None;
    }
    let direction = direction.map(|value| value / length);
    let relative = [start.u - center.u, start.v - center.v];
    let projection = -(relative[0] * direction[0] + relative[1] * direction[1]);
    let closest = [
        start.u + projection * direction[0],
        start.v + projection * direction[1],
    ];
    let distance_squared = (closest[0] - center.u).mul_add(
        closest[0] - center.u,
        (closest[1] - center.v) * (closest[1] - center.v),
    );
    let radial_squared = radius.0 * radius.0;
    let scale = radial_squared.max(1.0);
    if distance_squared > radial_squared + 1e-10 * scale {
        return None;
    }
    let travel = (radial_squared - distance_squared).max(0.0).sqrt();
    let candidates = [
        [
            closest[0] + travel * direction[0],
            closest[1] + travel * direction[1],
        ],
        [
            closest[0] - travel * direction[0],
            closest[1] - travel * direction[1],
        ],
    ];
    if travel <= 1e-10 * radius.0.max(1.0) {
        let parameter = projection / length;
        return (-1e-10..=1.0 + 1e-10)
            .contains(&parameter)
            .then_some(candidates[0]);
    }
    let parameters = [
        (projection + travel) / length,
        (projection - travel) / length,
    ];
    let inside = parameters
        .into_iter()
        .enumerate()
        .filter(|(_, parameter)| (-1e-10..=1.0 + 1e-10).contains(parameter))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [index] = inside.as_slice() else {
        return None;
    };
    Some(candidates[*index])
}

fn intersect_tangent_section_arcs(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (
        SketchGeometry::Arc {
            center: first_center,
            radius: first_radius,
            ..
        },
        SketchGeometry::Arc {
            center: second_center,
            radius: second_radius,
            ..
        },
    ) = (first, second)
    else {
        return None;
    };
    if first_radius.0 <= 1e-12 || second_radius.0 <= 1e-12 {
        return None;
    }
    let delta = [
        second_center.u - first_center.u,
        second_center.v - first_center.v,
    ];
    let distance = delta[0].hypot(delta[1]);
    let scale = distance.max(first_radius.0).max(second_radius.0).max(1.0);
    if distance <= 1e-12 * scale {
        return None;
    }
    let offset = (first_radius
        .0
        .mul_add(first_radius.0, -(second_radius.0 * second_radius.0))
        + distance * distance)
        / (2.0 * distance);
    let height_squared = first_radius.0.mul_add(first_radius.0, -(offset * offset));
    if height_squared.abs() > 1e-9 * scale * scale {
        return None;
    }
    Some([
        first_center.u + offset * delta[0] / distance,
        first_center.v + offset * delta[1] / distance,
    ])
}

fn intersect_section_carriers(
    first: &SectionIntersectionCarrier,
    second: &SectionIntersectionCarrier,
) -> Option<[f64; 2]> {
    let line_arc_is_bounded = match (&first.geometry, &second.geometry) {
        (SketchGeometry::Line { .. }, SketchGeometry::Arc { .. }) => first.line_is_bounded,
        (SketchGeometry::Arc { .. }, SketchGeometry::Line { .. }) => second.line_is_bounded,
        _ => false,
    };
    intersect_section_lines(&first.geometry, &second.geometry)
        .or_else(|| {
            line_arc_is_bounded
                .then(|| intersect_section_line_arc(&first.geometry, &second.geometry))
                .flatten()
        })
        .or_else(|| intersect_tangent_section_arcs(&first.geometry, &second.geometry))
}

fn intersect_incident_section_carriers(
    carriers: &[SectionIntersectionCarrier],
) -> Option<[f64; 2]> {
    (carriers.len() >= 2).then_some(())?;
    let mut candidates = Vec::new();
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            candidates.push((
                0,
                intersect_section_carriers(&carriers[first], &carriers[second])?,
            ));
        }
    }
    let (coordinates, ambiguous) = reconciled_section_coordinates(candidates);
    ambiguous.is_empty().then_some(())?;
    coordinates.get(&0).copied()
}

fn resolved_trim_vertex_coordinates(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
) -> BTreeMap<u32, [f64; 2]> {
    let Some(segments) = &definition.segments else {
        return BTreeMap::new();
    };
    let radii = resolved_section_radii(definition);
    let missing_line = saved_section_missing_line_geometry(definition);
    let variable_points = definition
        .variables
        .as_ref()
        .map(|variables| variables.reconciled_points().0)
        .unwrap_or_default();
    let mut seen_vertex_ids = BTreeSet::new();
    let duplicate_vertex_ids = definition
        .trim_vertices
        .iter()
        .filter(|table| table.has_complete_bucket_frame())
        .flat_map(|table| &table.rows)
        .filter_map(|vertex| {
            (!seen_vertex_ids.insert(vertex.vertex_id)).then_some(vertex.vertex_id)
        })
        .collect::<BTreeSet<_>>();
    let mut coordinate_candidates = definition
        .trim_vertices
        .iter()
        .filter(|table| table.has_complete_bucket_frame())
        .flat_map(|table| &table.rows)
        .filter_map(|vertex| Some((vertex.vertex_id, vertex.section_coordinates?)))
        .collect::<Vec<_>>();
    for trim in definition
        .trim_entities
        .iter()
        .flat_map(|table| &table.rows)
    {
        let Some(external_id) = trim_segment_id(definition, trim) else {
            continue;
        };
        let Some(segment) = segments.segment(external_id) else {
            continue;
        };
        let Some(([center_u, center_v], radius)) = saved_section_arc_carrier(definition, segment)
        else {
            continue;
        };
        let Some(arc) = saved_section_arc_record(definition, segment) else {
            continue;
        };
        for (vertex, endpoint) in trim.vertices.into_iter().zip(arc.endpoints) {
            let [Some(u), Some(v), _] = endpoint else {
                continue;
            };
            let candidate = [u, v];
            let candidate_radius = (u - center_u).hypot(v - center_v);
            let radial_scale = radius.max(candidate_radius).max(1.0);
            if (candidate_radius - radius).abs() > 1e-9 * radial_scale {
                continue;
            }
            coordinate_candidates.push((vertex, candidate));
        }
    }
    let mut incident = BTreeMap::<u32, Vec<u32>>::new();
    for entity in definition
        .trim_entities
        .iter()
        .flat_map(|table| &table.rows)
    {
        let Some(external_id) = trim_segment_id(definition, entity) else {
            continue;
        };
        for vertex in entity.vertices {
            incident.entry(vertex).or_default().push(external_id);
        }
    }
    let explicit_incident = definition
        .trim_vertices
        .as_ref()
        .filter(|table| table.has_complete_bucket_frame())
        .map(|table| {
            let mut result = BTreeMap::<u32, Vec<u32>>::new();
            for vertex in &table.rows {
                let mut resolved = Vec::new();
                for entity_id in &vertex.entities {
                    let matches = definition
                        .trim_entities
                        .iter()
                        .flat_map(|table| &table.rows)
                        .filter(|entity| entity.external_id == *entity_id)
                        .collect::<Vec<_>>();
                    let external_id = match matches.as_slice() {
                        [entity] => trim_segment_id(definition, entity),
                        [] => segments
                            .segment(*entity_id)
                            .map(|segment| segment.external_id),
                        _ => None,
                    };
                    if let Some(external_id) = external_id {
                        resolved.push(external_id);
                    }
                }
                resolved.sort_unstable();
                if resolved.len() == vertex.entities.len() {
                    result.entry(vertex.vertex_id).or_default().extend(resolved);
                }
            }
            result
        });
    if let Some(explicit) = &explicit_incident {
        for (vertex, entities) in explicit {
            if entities.len() < 2 || entities.windows(2).any(|pair| pair[0] == pair[1]) {
                continue;
            }
            let mut derived = incident.get(vertex).cloned().unwrap_or_default();
            derived.sort_unstable();
            derived.dedup();
            if derived
                .iter()
                .any(|external_id| !entities.contains(external_id))
            {
                continue;
            }
            incident.insert(*vertex, entities.clone());
            let common_points = entities
                .iter()
                .filter_map(|external_id| segments.segment(*external_id))
                .map(|segment| segment.point_ids.into_iter().collect::<BTreeSet<_>>())
                .reduce(|common, points| common.intersection(&points).copied().collect());
            let Some(common_points) = common_points else {
                continue;
            };
            let common_points = common_points.into_iter().collect::<Vec<_>>();
            let [point_id] = common_points.as_slice() else {
                continue;
            };
            if let Some(coordinate) = points.get(point_id) {
                coordinate_candidates.push((*vertex, *coordinate));
            }
        }
    }
    let intersection_carriers = incident
        .values()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|external_id| {
            let segment = segments.segment(external_id)?;
            let carrier = section_segment_intersection_carrier_with_missing_line(
                definition,
                &radii,
                points,
                segment,
                missing_line.as_ref(),
                &variable_points,
            )?;
            Some((external_id, carrier))
        })
        .collect::<BTreeMap<_, _>>();
    for (vertex, mut entities) in incident {
        entities.sort_unstable();
        if entities.len() < 2 || entities.windows(2).any(|pair| pair[0] == pair[1]) {
            continue;
        }
        if explicit_incident
            .as_ref()
            .is_some_and(|explicit| explicit.get(&vertex) != Some(&entities))
        {
            continue;
        }
        let carriers = entities
            .iter()
            .map(|external_id| intersection_carriers.get(external_id).cloned())
            .collect::<Option<Vec<_>>>();
        let Some(carriers) = carriers else {
            continue;
        };
        if let Some(coordinate) = intersect_incident_section_carriers(&carriers) {
            coordinate_candidates.push((vertex, coordinate));
        }
    }
    let (mut coordinates, mut ambiguous_vertices) =
        reconciled_section_coordinates(coordinate_candidates);
    ambiguous_vertices.extend(duplicate_vertex_ids);
    coordinates.retain(|vertex, _| !ambiguous_vertices.contains(vertex));
    loop {
        let mut additions = Vec::new();
        for trim in definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
        {
            let Some(external_id) = trim_segment_id(definition, trim) else {
                continue;
            };
            let Some(segment) = segments.segment(external_id) else {
                continue;
            };
            let Some(SketchGeometry::Line { start, end }) =
                resolved_section_segment_geometry_with_missing_line(
                    definition,
                    points,
                    segment,
                    missing_line.as_ref(),
                )
            else {
                continue;
            };
            let stored = [[start.u, start.v], [end.u, end.v]];
            let known = trim
                .vertices
                .map(|vertex| coordinates.get(&vertex).copied());
            let (known_point, missing_index) = match known {
                [Some(point), None] => (point, 1),
                [None, Some(point)] => (point, 0),
                _ => continue,
            };
            let distances =
                stored.map(|point| (point[0] - known_point[0]).hypot(point[1] - known_point[1]));
            let scale = stored
                .iter()
                .flatten()
                .map(|value| value.abs())
                .fold(1.0, f64::max);
            let matched = if distances[0] <= 1e-9 * scale && distances[1] > 1e-9 * scale {
                0
            } else if distances[1] <= 1e-9 * scale && distances[0] > 1e-9 * scale {
                1
            } else {
                continue;
            };
            additions.push((trim.vertices[missing_index], stored[1 - matched]));
        }
        let (additions, conflicts) = reconciled_section_coordinates(additions);
        ambiguous_vertices.extend(conflicts);
        let mut changed = false;
        for (vertex, coordinate) in additions {
            if ambiguous_vertices.contains(&vertex) {
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = coordinates.entry(vertex) {
                entry.insert(coordinate);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    coordinates
}

fn reconciled_section_coordinates(
    candidates: impl IntoIterator<Item = (u32, [f64; 2])>,
) -> (BTreeMap<u32, [f64; 2]>, BTreeSet<u32>) {
    let mut grouped = BTreeMap::<u32, Vec<[f64; 2]>>::new();
    for (vertex, coordinate) in candidates {
        grouped.entry(vertex).or_default().push(coordinate);
    }
    let mut coordinates = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for (vertex, values) in grouped {
        let first = values[0];
        let scale = values
            .iter()
            .flatten()
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        if values.iter().all(|candidate| {
            (candidate[0] - first[0]).hypot(candidate[1] - first[1]) <= 1e-9 * scale
        }) {
            coordinates.insert(vertex, first);
        } else {
            ambiguous.insert(vertex);
        }
    }
    (coordinates, ambiguous)
}

fn trimmed_section_segment_geometry_with_missing_line(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    trim_vertices: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
    missing_line: Option<&(usize, SketchGeometry)>,
) -> Option<SketchGeometry> {
    let trim = definition
        .trim_entities
        .as_ref()?
        .rows
        .iter()
        .find(|row| trim_segment_id(definition, row) == Some(segment.external_id))?;
    let start = trim_vertices.get(&trim.vertices[0])?;
    let end = trim_vertices.get(&trim.vertices[1])?;
    if let Some(SketchGeometry::Line {
        start: carrier_start,
        end: carrier_end,
    }) = resolved_section_segment_geometry_with_missing_line(
        definition,
        points,
        segment,
        missing_line,
    ) {
        let scale = [
            carrier_start.u,
            carrier_start.v,
            carrier_end.u,
            carrier_end.v,
            start[0],
            start[1],
            end[0],
            end[1],
        ]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
        let direction = [
            carrier_end.u / scale - carrier_start.u / scale,
            carrier_end.v / scale - carrier_start.v / scale,
        ];
        let direction_norm = direction[0].hypot(direction[1]);
        if direction_norm <= 1e-12
            || [start, end].into_iter().any(|point| {
                let offset = [
                    point[0] / scale - carrier_start.u / scale,
                    point[1] / scale - carrier_start.v / scale,
                ];
                (offset[0] * direction[1] - offset[1] * direction[0]).abs() > 1e-9 * direction_norm
            })
        {
            return None;
        }
    } else if let Some(([center_u, center_v], radius)) =
        section_arc_carrier(&resolved_section_radii(definition), points, segment)
            .or_else(|| saved_section_arc_carrier(definition, segment))
    {
        let first = [start[0] - center_u, start[1] - center_v];
        let second = [end[0] - center_u, end[1] - center_v];
        let first_radius = first[0].hypot(first[1]);
        let second_radius = second[0].hypot(second[1]);
        let scale = radius.max(first_radius).max(second_radius).max(1.0);
        if (first_radius - radius).abs() > 1e-9 * scale
            || (second_radius - radius).abs() > 1e-9 * scale
        {
            return None;
        }
        let start_angle = second[1].atan2(second[0]);
        let mut end_angle = first[1].atan2(first[0]);
        while end_angle <= start_angle {
            end_angle += std::f64::consts::TAU;
        }
        return Some(SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(center_u, center_v),
            radius: Length(radius),
            start_angle: Angle(start_angle),
            end_angle: Angle(end_angle),
        });
    } else {
        let scale = start
            .iter()
            .chain(end)
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        let orientation_matches = match section_line_fixed_coordinate(definition, segment) {
            Some(0) => (start[0] - end[0]).abs() <= 1e-9 * scale,
            Some(1) => (start[1] - end[1]).abs() <= 1e-9 * scale,
            _ => false,
        };
        orientation_matches.then_some(())?;
    }
    Some(SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    })
}

fn section_point_in_model(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 2],
) -> [f64; 3] {
    std::array::from_fn(|axis| {
        transform.origin[axis]
            + point[0] * transform.u_axis[axis]
            + point[1] * transform.v_axis[axis]
    })
}

fn section_xyz_in_model(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 3],
) -> [f64; 3] {
    std::array::from_fn(|axis| {
        transform.origin[axis]
            + point[0] * transform.u_axis[axis]
            + point[1] * transform.v_axis[axis]
            + point[2] * transform.normal[axis]
    })
}

fn normalized(vector: [f64; 3]) -> Option<[f64; 3]> {
    let magnitude = vector
        .iter()
        .fold(0.0_f64, |norm, value| norm.hypot(*value));
    (magnitude.is_finite() && magnitude > 1e-12).then(|| vector.map(|value| value / magnitude))
}

fn feature_plane_equations(scan: &ContainerScan, feature_id: u32) -> Vec<([f64; 3], [f64; 3])> {
    let ids = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .map(|row| row.id)
        .collect::<BTreeSet<_>>();
    ids.into_iter()
        .filter_map(|id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, id)?;
            let outlines = scan
                .planes
                .outlines
                .iter()
                .filter(|plane| plane.surface_id == id)
                .collect::<Vec<_>>();
            match outlines.as_slice() {
                [plane] => Some((plane.origin, plane.normal)),
                [] => {
                    let frames = scan
                        .planes
                        .local_systems
                        .iter()
                        .filter(|frame| frame.surface_id == id)
                        .collect::<Vec<_>>();
                    let [frame] = frames.as_slice() else {
                        return None;
                    };
                    Some((frame.origin?, frame.normal?))
                }
                _ => None,
            }
        })
        .collect()
}

fn feature_outline_planes(scan: &ContainerScan, feature_id: u32) -> Vec<(u32, [f64; 3], [f64; 3])> {
    scan.surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .map(|row| row.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, id)?;
            let outlines = scan
                .planes
                .outlines
                .iter()
                .filter(|plane| plane.surface_id == id)
                .collect::<Vec<_>>();
            let [plane] = outlines.as_slice() else {
                return None;
            };
            Some((id, plane.origin, plane.normal))
        })
        .collect()
}

fn generated_arc_cylinder_extent(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let feature_id = definition.owner_feature_id?;
    definition.segments.as_ref()?.is_complete().then_some(())?;
    let mut frames = Vec::new();
    let mut surface_ids = BTreeSet::new();
    for entry in scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| &table.entries)
    {
        let Some(source_id) = entry.source_entity_id else {
            continue;
        };
        let Some(segment) = definition.segments.as_ref()?.segment(source_id) else {
            continue;
        };
        if segment.kind != crate::feature::FeatureSegmentKind::Arc {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
            .filter(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
            })
        else {
            continue;
        };
        let frame = crate::surface::unique_surface_parameter(&scan.surfaces.parameters, row.id)?
            .positional_cylinder_frame?;
        surface_ids.insert(row.id).then_some(())?;
        frames.push(frame);
    }
    (!frames.is_empty()).then_some(())?;
    agreed_generated_cylinder_extent(transform, &frames)
}

fn agreed_generated_cylinder_extent(
    transform: &crate::placement::FeatureSectionTransform,
    frames: &[crate::surface::PositionalCylinderFrame],
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let normal = normalized(transform.normal)?;
    let first = *frames.first()?;
    let length = first.length.filter(|length| *length > 0.0)?;
    let direction = normalized(first.axis)?;
    let close =
        |left: f64, right: f64| (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0);
    frames
        .iter()
        .all(|frame| {
            frame
                .length
                .is_some_and(|candidate| close(candidate, length))
                && normalized(frame.axis).is_some_and(|axis| {
                    axis.iter()
                        .zip(direction)
                        .all(|(left, right)| close(*left, right))
                })
                && close(
                    dot(
                        std::array::from_fn(|index| frame.origin[index] - transform.origin[index]),
                        normal,
                    ),
                    0.0,
                )
        })
        .then_some(())?;
    close(dot(direction, normal).abs(), 1.0).then_some(())?;
    Some((
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(length),
                },
                draft: None,
                offset: None,
            },
        },
        direction,
    ))
}

fn directed_blind_extrusion_span(
    profile_direction: [f64; 3],
    extrusion_direction: [f64; 3],
    length: f64,
) -> Option<ExtrusionSpan> {
    (length.is_finite() && length > 0.0).then_some(())?;
    let profile_direction = normalized(profile_direction)?;
    let extrusion_direction = normalized(extrusion_direction)?;
    let alignment = dot(profile_direction, extrusion_direction);
    (alignment.abs() >= 1.0 - 1e-9).then_some(())?;
    Some(if alignment.is_sign_positive() {
        ExtrusionSpan {
            lower: 0.0,
            upper: length,
        }
    } else {
        ExtrusionSpan {
            lower: -length,
            upper: 0.0,
        }
    })
}

fn resolved_feature_extrusion_span(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<ExtrusionSpan> {
    generated_arc_cylinder_extent(scan, definition, transform)
        .and_then(|(extent, direction)| match extent {
            ExtrudeExtent::OneSided {
                side:
                    ExtrudeSide {
                        termination: Termination::Blind { length },
                        ..
                    },
            } => directed_blind_extrusion_span(transform.normal, direction, length.0),
            _ => None,
        })
        .or_else(|| {
            extrusion_span(
                transform.origin,
                transform.normal,
                feature_plane_equations(scan, definition.owner_feature_id?),
            )
        })
}

fn extruded_geometry_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
) -> Option<SurfaceGeometry> {
    match geometry {
        SketchGeometry::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let line = normalized(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            let normal = normalized(cross(line, transform.normal))?;
            Some(SurfaceGeometry::Plane {
                origin: Point3::new(start[0], start[1], start[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(line[0], line[1], line[2]),
            })
        }
        SketchGeometry::Arc { center, radius, .. } | SketchGeometry::Circle { center, radius } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            Some(SurfaceGeometry::Cylinder {
                origin: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(
                    transform.normal[0],
                    transform.normal[1],
                    transform.normal[2],
                ),
                ref_direction: Vector3::new(
                    transform.u_axis[0],
                    transform.u_axis[1],
                    transform.u_axis[2],
                ),
                radius: radius.0,
            })
        }
        _ => None,
    }
}

fn revolved_section_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
    revolution_axis: RevolutionAxis,
) -> Option<SurfaceGeometry> {
    let axis = normalized([
        revolution_axis.direction.x,
        revolution_axis.direction.y,
        revolution_axis.direction.z,
    ])?;
    let axis_origin = [
        revolution_axis.origin.x,
        revolution_axis.origin.y,
        revolution_axis.origin.z,
    ];
    let project = |point: [f64; 3]| {
        let displacement = std::array::from_fn(|index| point[index] - axis_origin[index]);
        let axial = dot(displacement, axis);
        let on_axis = std::array::from_fn(|index| axis_origin[index] + axial * axis[index]);
        let radial = std::array::from_fn(|index| point[index] - on_axis[index]);
        (on_axis, radial)
    };
    let vector = |values: [f64; 3]| Vector3::new(values[0], values[1], values[2]);
    let point = |values: [f64; 3]| Point3::new(values[0], values[1], values[2]);
    match geometry {
        SketchGeometry::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let direction = normalized(std::array::from_fn(|index| end[index] - start[index]))?;
            let (mut on_axis, mut radial) = project(start);
            let mut radius = dot(radial, radial).sqrt();
            if radius <= 1e-10 {
                (on_axis, radial) = project(end);
                radius = dot(radial, radial).sqrt();
            }
            let axial_rate = dot(direction, axis);
            let radial_rate =
                std::array::from_fn(|index| direction[index] - axial_rate * axis[index]);
            let radial_speed = dot(radial_rate, radial_rate).sqrt();
            let scale = radius.max(1.0);
            if radius > 1e-10 {
                let coplanar_residual = dot(cross(radial, radial_rate), axis).abs();
                (coplanar_residual <= 1e-9 * scale).then_some(())?;
            }
            let reference = normalized(radial).or_else(|| normalized(radial_rate))?;
            if radial_speed <= 1e-10 {
                (radius > 1e-10).then_some(())?;
                return Some(SurfaceGeometry::Cylinder {
                    origin: point(on_axis),
                    axis: vector(axis),
                    ref_direction: vector(reference),
                    radius,
                });
            }
            if axial_rate.abs() <= 1e-10 {
                return Some(SurfaceGeometry::Plane {
                    origin: point(on_axis),
                    normal: vector(axis),
                    u_axis: vector(reference),
                });
            }
            let radial_rate = dot(radial_rate, reference);
            let cone_axis = if radial_rate / axial_rate < 0.0 {
                std::array::from_fn(|index| -axis[index])
            } else {
                axis
            };
            Some(SurfaceGeometry::Cone {
                origin: point(on_axis),
                axis: vector(cone_axis),
                ref_direction: vector(reference),
                radius,
                ratio: 1.0,
                half_angle: radial_rate.abs().atan2(axial_rate.abs()),
            })
        }
        SketchGeometry::Arc { center, radius, .. } | SketchGeometry::Circle { center, radius } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            let (on_axis, radial) = project(center);
            let major_radius = dot(radial, radial).sqrt();
            let reference = normalized(radial).or_else(|| {
                [transform.u_axis, transform.v_axis]
                    .into_iter()
                    .find_map(|candidate| {
                        let axial = dot(candidate, axis);
                        normalized(std::array::from_fn(|index| {
                            candidate[index] - axial * axis[index]
                        }))
                    })
            })?;
            if major_radius <= 1e-10 {
                Some(SurfaceGeometry::Sphere {
                    center: point(center),
                    axis: vector(axis),
                    ref_direction: vector(reference),
                    radius: radius.0,
                })
            } else {
                Some(SurfaceGeometry::Torus {
                    center: point(on_axis),
                    axis: vector(axis),
                    ref_direction: vector(reference),
                    major_radius,
                    minor_radius: radius.0,
                })
            }
        }
        _ => None,
    }
}

fn placed_section_geometry_curve(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
) -> Option<CurveGeometry> {
    match geometry {
        SketchGeometry::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let direction = normalized(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            Some(CurveGeometry::Line {
                origin: Point3::new(start[0], start[1], start[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SketchGeometry::Arc { center, radius, .. } | SketchGeometry::Circle { center, radius } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            Some(CurveGeometry::Circle {
                center: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(
                    transform.normal[0],
                    transform.normal[1],
                    transform.normal[2],
                ),
                ref_direction: Vector3::new(
                    transform.u_axis[0],
                    transform.u_axis[1],
                    transform.u_axis[2],
                ),
                radius: radius.0,
            })
        }
        _ => None,
    }
}

fn placed_sketch_curve_ref(
    transform: Option<&crate::placement::FeatureSectionTransform>,
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
    geometry: &SketchGeometry,
) -> Option<String> {
    placed_section_geometry_curve(transform?, geometry)?;
    Some(sketch_section_curve_id(sketch, suffix))
}

fn bspline_basis(index: usize, degree: usize, parameter: f64, knots: &[f64], count: usize) -> f64 {
    if parameter == *knots.last().expect("nonempty knots") {
        return if index + 1 == count { 1.0 } else { 0.0 };
    }
    if degree == 0 {
        return if knots[index] <= parameter && parameter < knots[index + 1] {
            1.0
        } else {
            0.0
        };
    }
    let left_denominator = knots[index + degree] - knots[index];
    let right_denominator = knots[index + degree + 1] - knots[index + 1];
    let left = if left_denominator > 0.0 {
        (parameter - knots[index]) / left_denominator
            * bspline_basis(index, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        (knots[index + degree + 1] - parameter) / right_denominator
            * bspline_basis(index + 1, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    left + right
}

fn bspline_basis_derivative(
    index: usize,
    degree: usize,
    parameter: f64,
    knots: &[f64],
    count: usize,
) -> f64 {
    let left_denominator = knots[index + degree] - knots[index];
    let right_denominator = knots[index + degree + 1] - knots[index + 1];
    let left = if left_denominator > 0.0 {
        degree as f64 / left_denominator * bspline_basis(index, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        degree as f64 / right_denominator
            * bspline_basis(index + 1, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    left - right
}

fn solve_vector_system(
    mut matrix: Vec<Vec<f64>>,
    mut values: Vec<[f64; 3]>,
) -> Option<Vec<[f64; 3]>> {
    let count = matrix.len();
    (values.len() == count && matrix.iter().all(|row| row.len() == count)).then_some(())?;
    for column in 0..count {
        let pivot = (column..count).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        })?;
        (matrix[pivot][column].abs() > 1e-14).then_some(())?;
        matrix.swap(column, pivot);
        values.swap(column, pivot);
        let scale = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= scale;
        }
        values[column] = values[column].map(|value| value / scale);
        let pivot_row = matrix[column].clone();
        let pivot_value = values[column];
        for row in 0..count {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            if factor == 0.0 {
                continue;
            }
            for (entry, pivot_entry) in matrix[row][column..].iter_mut().zip(&pivot_row[column..]) {
                *entry -= factor * pivot_entry;
            }
            for (value, pivot) in values[row].iter_mut().zip(pivot_value) {
                *value -= factor * pivot;
            }
        }
    }
    Some(values)
}

fn interpolation_curve_data(
    points: &[[f64; 3]],
    parameters: &[f64],
    endpoint_derivatives: [[f64; 3]; 2],
) -> Option<(Vec<f64>, Vec<[f64; 3]>)> {
    const DEGREE: usize = 3;
    let point_count = points.len();
    (point_count >= 2 && parameters.len() == point_count).then_some(())?;
    parameters
        .windows(2)
        .all(|pair| pair[0].is_finite() && pair[0] < pair[1])
        .then_some(())?;
    parameters.last()?.is_finite().then_some(())?;
    let control_count = point_count + 2;
    let mut knots = vec![parameters[0]; DEGREE + 1];
    knots.extend_from_slice(&parameters[1..point_count - 1]);
    knots.extend(std::iter::repeat_n(parameters[point_count - 1], DEGREE + 1));
    let mut matrix = Vec::with_capacity(control_count);
    for parameter in parameters {
        matrix.push(
            (0..control_count)
                .map(|index| bspline_basis(index, DEGREE, *parameter, &knots, control_count))
                .collect(),
        );
    }
    for parameter in [parameters[0], parameters[point_count - 1]] {
        matrix.push(
            (0..control_count)
                .map(|index| {
                    bspline_basis_derivative(index, DEGREE, parameter, &knots, control_count)
                })
                .collect(),
        );
    }
    let mut values = points.to_vec();
    values.extend(endpoint_derivatives);
    Some((knots, solve_vector_system(matrix, values)?))
}

fn saved_spline_nurbs(spline: &crate::feature::FeatureSavedSpline) -> Option<NurbsCurve> {
    (usize::try_from(spline.declared_point_count?).ok()? == spline.interpolation_points.len())
        .then_some(())?;
    let parameters = spline.parameters.as_ref()?;
    let tangents = spline.endpoint_tangents?;
    let (knots, control_points) =
        interpolation_curve_data(&spline.interpolation_points, parameters, tangents)?;
    let control_points = control_points
        .into_iter()
        .map(|point| Point3::new(point[0], point[1], point[2]))
        .collect();
    Some(NurbsCurve {
        degree: 3,
        knots,
        control_points,
        weights: None,
        periodic: false,
    })
}

fn saved_spline_sketch_geometry(
    spline: &crate::feature::FeatureSavedSpline,
) -> Option<SketchGeometry> {
    let nurbs = saved_spline_nurbs(spline)?;
    nurbs
        .control_points
        .iter()
        .all(|point| point.z.abs() <= 1e-12)
        .then(|| SketchGeometry::Nurbs {
            degree: nurbs.degree,
            knots: nurbs.knots,
            control_points: nurbs
                .control_points
                .into_iter()
                .map(|point| cadmpeg_ir::math::Point2::new(point.x, point.y))
                .collect(),
            weights: nurbs.weights,
            periodic: nurbs.periodic,
        })
}

fn interpolation_spline_surface(
    points: &[[f64; 3]],
    u_parameters: &[f64],
    v_parameters: &[f64],
    end_u_derivatives: &[[f64; 3]],
    end_v_derivatives: &[[f64; 3]],
    corner_mixed_derivatives: &[[f64; 3]],
) -> Option<NurbsSurface> {
    let u_sample_count = u_parameters.len();
    let v_sample_count = v_parameters.len();
    let point_count = u_sample_count.checked_mul(v_sample_count)?;
    let u_boundary_derivative_count = v_sample_count.checked_mul(2)?;
    let v_boundary_derivative_count = u_sample_count.checked_mul(2)?;
    (points.len() == point_count
        && end_u_derivatives.len() == u_boundary_derivative_count
        && end_v_derivatives.len() == v_boundary_derivative_count
        && corner_mixed_derivatives.len() == 4)
        .then_some(())?;

    let u_control_count = u_sample_count.checked_add(2)?;
    let v_control_count = v_sample_count.checked_add(2)?;
    let mut position_controls = vec![vec![[0.0; 3]; v_sample_count]; u_control_count];
    let mut u_knots = None;
    for v in 0..v_sample_count {
        let samples = (0..u_sample_count)
            .map(|u| points[u * v_sample_count + v])
            .collect::<Vec<_>>();
        let (knots, controls) = interpolation_curve_data(
            &samples,
            u_parameters,
            [end_u_derivatives[v], end_u_derivatives[v_sample_count + v]],
        )?;
        u_knots.get_or_insert(knots);
        for (u, control) in controls.into_iter().enumerate() {
            position_controls[u][v] = control;
        }
    }

    let mut v_derivative_controls = vec![vec![[0.0; 3]; u_control_count]; 2];
    for v_boundary in 0..2 {
        let samples = (0..u_sample_count)
            .map(|u| end_v_derivatives[v_boundary * u_sample_count + u])
            .collect::<Vec<_>>();
        let (_, controls) = interpolation_curve_data(
            &samples,
            u_parameters,
            [
                corner_mixed_derivatives[v_boundary * 2],
                corner_mixed_derivatives[v_boundary * 2 + 1],
            ],
        )?;
        v_derivative_controls[v_boundary] = controls;
    }

    let mut control_points = Vec::with_capacity(u_control_count * v_control_count);
    let mut v_knots = None;
    for u in 0..u_control_count {
        let (knots, controls) = interpolation_curve_data(
            &position_controls[u],
            v_parameters,
            [v_derivative_controls[0][u], v_derivative_controls[1][u]],
        )?;
        v_knots.get_or_insert(knots);
        control_points.extend(
            controls
                .into_iter()
                .map(|point| Point3::new(point[0], point[1], point[2])),
        );
    }

    Some(NurbsSurface {
        u_degree: 3,
        v_degree: 3,
        u_knots: u_knots?,
        v_knots: v_knots?,
        u_count: u32::try_from(u_control_count).ok()?,
        v_count: u32::try_from(v_control_count).ok()?,
        control_points,
        weights: None,
        u_periodic: false,
        v_periodic: false,
    })
}

fn placed_section_nurbs(
    transform: &crate::placement::FeatureSectionTransform,
    nurbs: &NurbsCurve,
) -> NurbsCurve {
    NurbsCurve {
        degree: nurbs.degree,
        knots: nurbs.knots.clone(),
        control_points: nurbs
            .control_points
            .iter()
            .map(|point| {
                let placed = section_xyz_in_model(transform, [point.x, point.y, point.z]);
                Point3::new(placed[0], placed[1], placed[2])
            })
            .collect(),
        weights: nurbs.weights.clone(),
        periodic: nurbs.periodic,
    }
}

fn translated_nurbs_curve(curve: &NurbsCurve, translation: [f64; 3]) -> NurbsCurve {
    NurbsCurve {
        degree: curve.degree,
        knots: curve.knots.clone(),
        control_points: curve
            .control_points
            .iter()
            .map(|point| {
                Point3::new(
                    point.x + translation[0],
                    point.y + translation[1],
                    point.z + translation[2],
                )
            })
            .collect(),
        weights: curve.weights.clone(),
        periodic: curve.periodic,
    }
}

fn extruded_nurbs_surface(directrix: &NurbsCurve, sweep: [f64; 3]) -> Option<NurbsSurface> {
    if directrix
        .weights
        .as_ref()
        .is_some_and(|weights| weights.len() != directrix.control_points.len())
    {
        return None;
    }
    let mut control_points = Vec::with_capacity(directrix.control_points.len() * 2);
    let mut weights = directrix
        .weights
        .as_ref()
        .map(|_| Vec::with_capacity(control_points.capacity()));
    for (index, point) in directrix.control_points.iter().enumerate() {
        control_points.push(*point);
        control_points.push(Point3::new(
            point.x + sweep[0],
            point.y + sweep[1],
            point.z + sweep[2],
        ));
        if let (Some(source), Some(target)) = (&directrix.weights, &mut weights) {
            target.extend([source[index], source[index]]);
        }
    }
    Some(NurbsSurface {
        u_degree: directrix.degree,
        v_degree: 1,
        u_knots: directrix.knots.clone(),
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: u32::try_from(directrix.control_points.len()).ok()?,
        v_count: 2,
        control_points,
        weights,
        u_periodic: directrix.periodic,
        v_periodic: false,
    })
}

fn signed_unit_chart(local: [f64; 2], frame: [f64; 2], offset: f64) -> Option<(f64, f64)> {
    let close = |left: f64, right: f64| {
        (left - right).abs() <= 1.0e-9 * left.abs().max(right.abs()).max(1.0)
    };
    let mut matches = Vec::new();
    for first_sign in [-1.0, 1.0] {
        for second_sign in [-1.0, 1.0] {
            let frame = [first_sign * frame[0], second_sign * frame[1]];
            for reversed in [false, true] {
                let target = if reversed {
                    [frame[1], frame[0]]
                } else {
                    frame
                };
                let slope = if reversed { -1.0 } else { 1.0 };
                let intercept = target[0] - slope * local[0];
                if close(target[1], slope * local[1] + intercept)
                    && close(intercept.abs(), offset)
                    && !matches.contains(&(slope, intercept))
                {
                    matches.push((slope, intercept));
                }
            }
        }
    }
    let [mapping] = matches.as_slice() else {
        return None;
    };
    Some(*mapping)
}

fn placed_tabulated_cylinder_directrix(
    replay: &crate::surface::TabulatedCylinderCurveReplay,
    parameters: &crate::surface::SurfaceParameterRecord,
) -> Option<(NurbsCurve, [f64; 3])> {
    #[derive(Clone, Copy)]
    enum FrameLayout {
        LegacyReflected,
        SignedPlanar {
            first_offset: f64,
            reflect_sweep: bool,
        },
        OffsetSelectedPlanar,
    }
    if parameters.boundary != crate::surface::SurfaceBodyBoundary::CompoundClose {
        return None;
    }
    let direction = parameters.extrusion_direction(0x2c)?;
    (direction.iter().map(|value| value * value).sum::<f64>() > 0.0).then_some(())?;
    let points = replay
        .control_points
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()?;
    let (values, layout) = (|| {
        let frame = parameters.tabulated_cylinder_frame?;
        let values = frame.values.to_vec();
        let heads = frame.prefixes;
        let offset_planar_layout = matches!(heads.as_slice(), [_, 0x46, _, _, 0x46, _]);
        let zero_offset_layout = matches!(heads.as_slice(), [_, 0x42, _, _, 0x18, _]);
        if offset_planar_layout {
            Some((
                values,
                FrameLayout::SignedPlanar {
                    first_offset: 30.0,
                    reflect_sweep: false,
                },
            ))
        } else if zero_offset_layout {
            Some((
                values,
                FrameLayout::SignedPlanar {
                    first_offset: 0.0,
                    reflect_sweep: false,
                },
            ))
        } else if matches!(heads.as_slice(), [_, 0x2d, _, _, 0x2d, _]) {
            Some((values, FrameLayout::OffsetSelectedPlanar))
        } else {
            None
        }
    })()
    .or_else(|| {
        let [_, frame] = parameters.scalar_frames.as_slice() else {
            return None;
        };
        let values = frame
            .slots
            .iter()
            .map(|slot| slot.value)
            .collect::<Option<Vec<_>>>()?;
        Some((values, FrameLayout::LegacyReflected))
    })?;
    let [a0, a1, a2, b0, b1, b2] = values.as_slice() else {
        return None;
    };
    let first = [*a0, *a1, *a2];
    let second = [*b0, *b1, *b2];
    let local_start = points.first()?;
    let local_end = points.last()?;
    let local_span = [
        (local_end[0] - local_start[0]).abs(),
        (local_end[1] - local_start[1]).abs(),
    ];
    if local_span
        .iter()
        .any(|span| !span.is_finite() || *span <= 0.0)
    {
        return None;
    }
    let close = |left: f64, right: f64| {
        (left - right).abs() <= 1.0e-9 * left.abs().max(right.abs()).max(1.0)
    };
    let axis_matches = |axis: usize, coordinate: usize| match layout {
        FrameLayout::LegacyReflected => {
            close((second[axis] - first[axis]).abs(), local_span[coordinate])
        }
        FrameLayout::SignedPlanar { first_offset, .. } => signed_unit_chart(
            [local_start[coordinate], local_end[coordinate]],
            [first[axis], second[axis]],
            if coordinate == 0 { first_offset } else { 0.0 },
        )
        .is_some(),
        FrameLayout::OffsetSelectedPlanar => {
            let offsets: &[f64] = if coordinate == 0 {
                &[0.0, 30.0]
            } else {
                &[0.0]
            };
            offsets.iter().any(|offset| {
                signed_unit_chart(
                    [local_start[coordinate], local_end[coordinate]],
                    [first[axis], second[axis]],
                    *offset,
                )
                .is_some()
            })
        }
    };
    let assignments = (0..3)
        .flat_map(|first_axis| {
            (0..3)
                .filter(move |&second_axis| {
                    first_axis != second_axis
                        && axis_matches(first_axis, 0)
                        && axis_matches(second_axis, 1)
                })
                .map(move |second_axis| (first_axis, second_axis, 3 - first_axis - second_axis))
        })
        .collect::<Vec<_>>();
    let [(first_axis, second_axis, sweep_axis)] = assignments.as_slice() else {
        return None;
    };
    let (signed_chart, reflect_sweep) = match layout {
        FrameLayout::LegacyReflected => (None, false),
        FrameLayout::SignedPlanar {
            first_offset,
            reflect_sweep,
        } => (
            Some((
                signed_unit_chart(
                    [local_start[0], local_end[0]],
                    [first[*first_axis], second[*first_axis]],
                    first_offset,
                )?,
                signed_unit_chart(
                    [local_start[1], local_end[1]],
                    [first[*second_axis], second[*second_axis]],
                    0.0,
                )?,
            )),
            reflect_sweep,
        ),
        FrameLayout::OffsetSelectedPlanar => {
            let candidates = [(0.0, false), (30.0, true)]
                .into_iter()
                .filter_map(|(first_offset, reflect_sweep)| {
                    Some((
                        (
                            signed_unit_chart(
                                [local_start[0], local_end[0]],
                                [first[*first_axis], second[*first_axis]],
                                first_offset,
                            )?,
                            signed_unit_chart(
                                [local_start[1], local_end[1]],
                                [first[*second_axis], second[*second_axis]],
                                0.0,
                            )?,
                        ),
                        reflect_sweep,
                    ))
                })
                .collect::<Vec<_>>();
            let [(chart, reflect_sweep)] = candidates.as_slice() else {
                return None;
            };
            (Some(*chart), *reflect_sweep)
        }
    };
    let control_points = points
        .iter()
        .map(|point| {
            let mut placed = [0.0; 3];
            match signed_chart {
                Some(((first_slope, first_intercept), (second_slope, second_intercept))) => {
                    placed[*first_axis] = first_slope * point[0] + first_intercept;
                    placed[*second_axis] = second_slope * point[1] + second_intercept;
                    placed[*sweep_axis] = if reflect_sweep {
                        -first[*sweep_axis]
                    } else {
                        first[*sweep_axis]
                    };
                }
                None => {
                    let chart_first =
                        first[*first_axis].max(second[*first_axis]) - (point[0] - local_start[0]);
                    let chart_second =
                        first[*second_axis].min(second[*second_axis]) + (point[1] - local_start[1]);
                    placed[*first_axis] = if *first_axis < 2 {
                        -chart_first
                    } else {
                        chart_first
                    };
                    placed[*second_axis] = if *second_axis < 2 {
                        -chart_second
                    } else {
                        chart_second
                    };
                    placed[*sweep_axis] = first[*sweep_axis];
                }
            }
            Point3::new(placed[0], placed[1], placed[2])
        })
        .collect();
    let mut sweep = [0.0; 3];
    sweep[*sweep_axis] = if reflect_sweep {
        first[*sweep_axis] - second[*sweep_axis]
    } else {
        second[*sweep_axis] - first[*sweep_axis]
    };
    (sweep[*sweep_axis].is_finite() && sweep[*sweep_axis] != 0.0).then_some((
        NurbsCurve {
            degree: 3,
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            control_points,
            weights: None,
            periodic: false,
        },
        sweep,
    ))
}

fn transfer_saved_spline_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        for spline in definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(|entity| match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
        {
            let Some(nurbs) = saved_spline_nurbs(spline) else {
                continue;
            };
            let suffix = spline.entity_id.map_or_else(
                || format!("offset{}", spline.offset),
                |entity_id| entity_id.to_string(),
            );
            let curve_id = CurveId(format!(
                "creo:featdefs:saved_spline_curve#{}:{suffix}",
                definition.id
            ));
            if ir.model.curves.iter().any(|curve| curve.id == curve_id) {
                continue;
            }
            annotate(
                annotations,
                &curve_id,
                "FeatDefs",
                spline.offset as u64,
                "placed_saved_interpolation_spline",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id: curve_id,
                geometry: CurveGeometry::Nurbs(placed_section_nurbs(transform, &nurbs)),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("FeatDefs:saved_spline#{suffix}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

fn revolved_nurbs_surface(directrix: &NurbsCurve, axis: RevolutionAxis) -> Option<NurbsSurface> {
    if directrix
        .weights
        .as_ref()
        .is_some_and(|weights| weights.len() != directrix.control_points.len())
    {
        return None;
    }
    let axis_direction = normalized([axis.direction.x, axis.direction.y, axis.direction.z])?;
    let axis_origin = [axis.origin.x, axis.origin.y, axis.origin.z];
    let angular_poles = [
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [-1.0, 1.0],
        [-1.0, 0.0],
        [-1.0, -1.0],
        [0.0, -1.0],
        [1.0, -1.0],
        [1.0, 0.0],
    ];
    let diagonal_weight = std::f64::consts::FRAC_1_SQRT_2;
    let angular_weights = [
        1.0,
        diagonal_weight,
        1.0,
        diagonal_weight,
        1.0,
        diagonal_weight,
        1.0,
        diagonal_weight,
        1.0,
    ];
    let mut control_points = Vec::with_capacity(directrix.control_points.len() * 9);
    let mut weights = Vec::with_capacity(directrix.control_points.len() * 9);
    for (index, point) in directrix.control_points.iter().enumerate() {
        let relative = [
            point.x - axis_origin[0],
            point.y - axis_origin[1],
            point.z - axis_origin[2],
        ];
        let axial_distance = dot(relative, axis_direction);
        let center: [f64; 3] = std::array::from_fn(|component| {
            axis_origin[component] + axial_distance * axis_direction[component]
        });
        let radial = [
            point.x - center[0],
            point.y - center[1],
            point.z - center[2],
        ];
        let tangent = cross(axis_direction, radial);
        let directrix_weight = directrix
            .weights
            .as_ref()
            .map_or(1.0, |curve_weights| curve_weights[index]);
        for ([radial_scale, tangent_scale], angular_weight) in
            angular_poles.into_iter().zip(angular_weights)
        {
            control_points.push(Point3::new(
                center[0] + radial_scale * radial[0] + tangent_scale * tangent[0],
                center[1] + radial_scale * radial[1] + tangent_scale * tangent[1],
                center[2] + radial_scale * radial[2] + tangent_scale * tangent[2],
            ));
            weights.push(directrix_weight * angular_weight);
        }
    }
    Some(NurbsSurface {
        u_degree: directrix.degree,
        v_degree: 2,
        u_knots: directrix.knots.clone(),
        v_knots: vec![
            0.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            std::f64::consts::PI,
            3.0 * std::f64::consts::FRAC_PI_2,
            3.0 * std::f64::consts::FRAC_PI_2,
            std::f64::consts::TAU,
            std::f64::consts::TAU,
            std::f64::consts::TAU,
        ],
        u_count: u32::try_from(directrix.control_points.len()).ok()?,
        v_count: 9,
        control_points,
        weights: Some(weights),
        u_periodic: false,
        v_periodic: false,
    })
}

fn revolved_section_circle(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 2],
    axis: RevolutionAxis,
) -> Option<CurveGeometry> {
    let axis_direction = normalized([axis.direction.x, axis.direction.y, axis.direction.z])?;
    let axis_origin = [axis.origin.x, axis.origin.y, axis.origin.z];
    let point = section_point_in_model(transform, point);
    let relative: [f64; 3] =
        std::array::from_fn(|component| point[component] - axis_origin[component]);
    let axial_distance = dot(relative, axis_direction);
    let center: [f64; 3] = std::array::from_fn(|component| {
        axis_origin[component] + axial_distance * axis_direction[component]
    });
    let radial: [f64; 3] = std::array::from_fn(|component| point[component] - center[component]);
    let radius = dot(radial, radial).sqrt();
    let scale = point
        .iter()
        .chain(&axis_origin)
        .map(|coordinate| coordinate.abs())
        .fold(1.0, f64::max);
    (radius > 1e-10 * scale).then_some(())?;
    let reference = radial.map(|component| component / radius);
    Some(CurveGeometry::Circle {
        center: Point3::new(center[0], center[1], center[2]),
        axis: Vector3::new(axis_direction[0], axis_direction[1], axis_direction[2]),
        ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
        radius,
    })
}

fn extruded_section_line(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 2],
) -> Option<CurveGeometry> {
    let direction = normalized(transform.normal)?;
    let origin = section_point_in_model(transform, point);
    Some(CurveGeometry::Line {
        origin: Point3::new(origin[0], origin[1], origin[2]),
        direction: Vector3::new(direction[0], direction[1], direction[2]),
    })
}

fn transfer_feature_extrusion_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_linear_extrusion(scan, feature_id) {
            continue;
        }
        let Some(order_table) = &definition.order_table else {
            continue;
        };
        let points = resolved_section_points(definition);
        let solved = definition
            .trim_entities
            .iter()
            .flat_map(|trim_entities| &trim_entities.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        for segment in complete_section_segment_rows(definition)
            .iter()
            .filter(|segment| solved.contains(&segment.external_id))
        {
            let Some(section_geometry) =
                resolved_section_segment_geometry(definition, &points, segment)
            else {
                continue;
            };
            let Some(geometry) = extruded_geometry_surface(transform, &section_geometry) else {
                continue;
            };
            let Some(surface_id) = analytic_surface_id_for_feature(
                &scan.surfaces.rows,
                &scan.features.entity_tables,
                feature_id,
                segment.external_id,
                &geometry,
            ) else {
                continue;
            };
            let id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                segment.offset as u64,
                "protextrude_section_carrier",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{surface_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }

        for (internal_id, section_geometry, offset) in definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(saved_section_entity_geometry)
        {
            let Some(external_id) = order_table.external_id(internal_id) else {
                continue;
            };
            let Some(native_surface_id) = generated_surface_id_for_feature(
                &scan.features.entity_tables,
                feature_id,
                external_id,
            ) else {
                continue;
            };
            let Some(geometry) = extruded_geometry_surface(transform, &section_geometry) else {
                continue;
            };
            let Some(expected_kind) = surface_kind_for_geometry(&geometry) else {
                continue;
            };
            if !scan.surfaces.rows.iter().any(|row| {
                row.id == native_surface_id
                    && row.feature_id == feature_id
                    && row.kind == expected_kind
            }) {
                continue;
            }
            let id = SurfaceId(format!("creo:visibgeom:surface#{native_surface_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                offset as u64,
                "protextrude_saved_section_carrier",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{native_surface_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }

        let splines = definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(|entity| match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
            .filter_map(|spline| {
                let internal_id = spline.entity_id?;
                let external_id = order_table.external_id(internal_id)?;
                let surface_id = generated_surface_id_for_feature(
                    &scan.features.entity_tables,
                    feature_id,
                    external_id,
                )?;
                scan.surfaces
                    .rows
                    .iter()
                    .any(|row| {
                        row.id == surface_id
                            && row.feature_id == feature_id
                            && row.kind == crate::surface::SurfaceKind::Extrusion
                    })
                    .then_some((surface_id, spline))
            })
            .collect::<Vec<_>>();
        let Some(span) = resolved_feature_extrusion_span(scan, definition, transform) else {
            continue;
        };
        let lower_translation = transform.normal.map(|value| value * span.lower);
        let sweep = transform
            .normal
            .map(|value| value * (span.upper - span.lower));
        for (native_surface_id, spline) in splines {
            let Some(section_curve) = saved_spline_nurbs(spline) else {
                continue;
            };
            let placed = placed_section_nurbs(transform, &section_curve);
            let directrix = translated_nurbs_curve(&placed, lower_translation);
            let Some(surface) = extruded_nurbs_surface(&directrix, sweep) else {
                continue;
            };
            let suffix = spline
                .entity_id
                .expect("ordered saved spline has an entity id")
                .to_string();
            let curve_id = CurveId(format!(
                "creo:feature:extrusion_directrix#{feature_id}:{suffix}"
            ));
            if !ir.model.curves.iter().any(|curve| curve.id == curve_id) {
                annotate(
                    annotations,
                    &curve_id,
                    "FeatDefs",
                    spline.offset as u64,
                    "protextrude_spline_directrix",
                    Exactness::Derived,
                );
                ir.model.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Nurbs(directrix.clone()),
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!("FeatDefs:saved_spline#{suffix}"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{native_surface_id}"));
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
            let procedural_id = ProceduralSurfaceId(format!(
                "creo:feature:extrusion_construction#{feature_id}:{suffix}"
            ));
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                spline.offset as u64,
                "protextrude_spline_surface",
                Exactness::Derived,
            );
            annotate(
                annotations,
                &procedural_id,
                "FeatDefs",
                spline.offset as u64,
                "protextrude_spline_surface_construction",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Nurbs(surface),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{native_surface_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: surface_id,
                definition: ProceduralSurfaceDefinition::Extrusion {
                    directrix: curve_id,
                    parameter_interval: Some([
                        *directrix.knots.first().expect("validated spline knots"),
                        *directrix.knots.last().expect("validated spline knots"),
                    ]),
                    direction: Vector3::new(sweep[0], sweep[1], sweep[2]),
                    native_position: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            transferred += 1;
        }
    }
    transferred
}

fn sketch_geometry_endpoints(geometry: &SketchGeometry) -> Option<([f64; 2], [f64; 2])> {
    match geometry {
        SketchGeometry::Line { start, end } => Some(([start.u, start.v], [end.u, end.v])),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some((
            [
                center.u + radius.0 * start_angle.0.cos(),
                center.v + radius.0 * start_angle.0.sin(),
            ],
            [
                center.u + radius.0 * end_angle.0.cos(),
                center.v + radius.0 * end_angle.0.sin(),
            ],
        )),
        _ => None,
    }
}

fn connected_sketch_profile_vertices(
    ir: &CadIr,
    sketch_id: &SketchId,
) -> Vec<(usize, Vec<[f64; 2]>)> {
    let Some(sketch) = ir
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id == *sketch_id)
    else {
        return Vec::new();
    };
    let entities = ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch_id)
        .map(|entity| (entity.id.clone(), &entity.geometry))
        .collect::<BTreeMap<_, _>>();
    sketch
        .profiles
        .iter()
        .enumerate()
        .filter_map(|(profile_index, profile)| {
            (!profile.is_empty()).then_some(())?;
            let uses = profile
                .iter()
                .map(|entity_use| {
                    let geometry = entities.get(&entity_use.entity)?;
                    let (mut start, mut end) = sketch_geometry_endpoints(geometry)?;
                    if entity_use.reversed {
                        std::mem::swap(&mut start, &mut end);
                    }
                    Some((start, end))
                })
                .collect::<Option<Vec<_>>>()?;
            let scale = uses
                .iter()
                .flat_map(|(start, end)| start.iter().chain(end))
                .map(|coordinate| coordinate.abs())
                .fold(1.0, f64::max);
            uses.windows(2)
                .all(|adjacent| {
                    let end = adjacent[0].1;
                    let next = adjacent[1].0;
                    (end[0] - next[0]).hypot(end[1] - next[1]) <= 1e-9 * scale
                })
                .then(|| {
                    let mut vertices = uses.iter().map(|(start, _)| *start).collect::<Vec<_>>();
                    let first = uses[0].0;
                    let terminal = uses.last().expect("profile is not empty").1;
                    if (terminal[0] - first[0]).hypot(terminal[1] - first[1]) > 1e-9 * scale {
                        vertices.push(terminal);
                    }
                    (profile_index, vertices)
                })
        })
        .collect()
}

fn oriented_arc_parameterization(reversed: bool, start: f64, end: f64) -> (f64, [f64; 2]) {
    let (axis_sign, raw_start, raw_end) = if reversed {
        (-1.0, -end, -start)
    } else {
        (1.0, start, end)
    };
    let start = raw_start.rem_euclid(std::f64::consts::TAU);
    let mut end = raw_end.rem_euclid(std::f64::consts::TAU);
    if end < start {
        end += std::f64::consts::TAU;
    }
    (axis_sign, [start, end])
}

fn line_pcurve(start: [f64; 2], end: [f64; 2]) -> PcurveGeometry {
    PcurveGeometry::Line {
        origin: Point2::new(start[0], start[1]),
        direction: Point2::new(end[0] - start[0], end[1] - start[1]),
    }
}

fn circular_pcurve(
    center: [f64; 2],
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> PcurveGeometry {
    let segment_count = ((end_angle - start_angle).abs() / std::f64::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
    let step = (end_angle - start_angle) / segment_count as f64;
    let mut control_points = Vec::with_capacity(2 * segment_count + 1);
    let mut weights = Vec::with_capacity(2 * segment_count + 1);
    for segment in 0..segment_count {
        let first = start_angle + segment as f64 * step;
        let second = first + step;
        let middle = 0.5 * (first + second);
        let middle_weight = (0.5 * step).cos();
        if segment == 0 {
            control_points.push(Point2::new(
                center[0] + radius * first.cos(),
                center[1] + radius * first.sin(),
            ));
            weights.push(1.0);
        }
        control_points.push(Point2::new(
            center[0] + radius * middle.cos() / middle_weight,
            center[1] + radius * middle.sin() / middle_weight,
        ));
        weights.push(middle_weight);
        control_points.push(Point2::new(
            center[0] + radius * second.cos(),
            center[1] + radius * second.sin(),
        ));
        weights.push(1.0);
    }
    let mut knots = vec![0.0; 3];
    for boundary in 1..segment_count {
        knots.extend([boundary as f64 / segment_count as f64; 2]);
    }
    knots.extend([1.0; 3]);
    PcurveGeometry::Nurbs {
        degree: 2,
        knots,
        control_points,
        weights: Some(weights),
        periodic: false,
    }
}

fn extrusion_cap_pcurve(
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
) -> PcurveGeometry {
    match geometry {
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let [start_angle, end_angle] = if reversed {
                [end_angle.0, start_angle.0]
            } else {
                [start_angle.0, end_angle.0]
            };
            circular_pcurve([center.u, center.v], radius.0, start_angle, end_angle)
        }
        _ => line_pcurve(start, end),
    }
}

fn extrusion_side_uvs(
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
    span: ExtrusionSpan,
) -> [[[f64; 2]; 2]; 4] {
    let [first, second] = match geometry {
        SketchGeometry::Arc {
            start_angle,
            end_angle,
            ..
        } if reversed => [end_angle.0, start_angle.0],
        SketchGeometry::Arc {
            start_angle,
            end_angle,
            ..
        } => [start_angle.0, end_angle.0],
        _ => [0.0, (end[0] - start[0]).hypot(end[1] - start[1])],
    };
    [
        [[first, span.lower], [second, span.lower]],
        [[second, span.lower], [second, span.upper]],
        [[first, span.upper], [second, span.upper]],
        [[first, span.lower], [first, span.upper]],
    ]
}

fn extrusion_profile_signed_area(
    profile: &[(SketchGeometry, bool, [f64; 2], [f64; 2])],
) -> Option<f64> {
    let area_twice = profile
        .iter()
        .map(|(geometry, reversed, start, end)| {
            let chord = start[0].mul_add(end[1], -(start[1] * end[0]));
            let SketchGeometry::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } = geometry
            else {
                return chord;
            };
            let forward_delta = (end_angle.0 - start_angle.0).rem_euclid(std::f64::consts::TAU);
            let delta = if *reversed {
                -forward_delta
            } else {
                forward_delta
            };
            center.u.mul_add(
                end[1] - start[1],
                -(center.v * (end[0] - start[0])) + radius.0 * radius.0 * delta,
            )
        })
        .sum::<f64>();
    let scale = profile
        .iter()
        .flat_map(|(_, _, start, end)| start.iter().chain(end))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (area_twice.abs() > 1e-12 * scale * scale).then_some(0.5 * area_twice)
}

type ExtrusionProfile = Vec<(SketchGeometry, bool, [f64; 2], [f64; 2])>;

fn resolved_sketch_profiles(
    ir: &CadIr,
    sketch_id: &SketchId,
    minimum_entity_count: usize,
) -> Option<Vec<ExtrusionProfile>> {
    let sketch = ir
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id == *sketch_id)?;
    (!sketch.profiles.is_empty()).then_some(())?;
    let entities = ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch_id)
        .map(|entity| (entity.id.clone(), entity))
        .collect::<BTreeMap<_, _>>();
    let mut profiles = Vec::new();
    for profile in &sketch.profiles {
        let mut geometries = Vec::new();
        for entity_use in profile {
            let entity = entities.get(&entity_use.entity)?;
            let (mut start, mut end) = sketch_geometry_endpoints(&entity.geometry)?;
            if entity_use.reversed {
                std::mem::swap(&mut start, &mut end);
            }
            geometries.push((entity.geometry.clone(), entity_use.reversed, start, end));
        }
        (geometries.len() >= minimum_entity_count).then_some(())?;
        let scale = geometries
            .iter()
            .flat_map(|(_, _, start, end)| start.iter().chain(end))
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        geometries
            .iter()
            .enumerate()
            .all(|(index, (_, _, _, end))| {
                let next = geometries[(index + 1) % geometries.len()].2;
                (end[0] - next[0]).hypot(end[1] - next[1]) <= 1e-9 * scale
            })
            .then_some(())?;
        profiles.push(geometries);
    }
    Some(profiles)
}

fn segments_intersect(first: [[f64; 2]; 2], second: [[f64; 2]; 2], tolerance: f64) -> bool {
    let orient = |a: [f64; 2], b: [f64; 2], point: [f64; 2]| {
        (b[0] - a[0]).mul_add(point[1] - a[1], -((b[1] - a[1]) * (point[0] - a[0])))
    };
    let on_segment = |segment: [[f64; 2]; 2], point: [f64; 2]| {
        point[0] >= segment[0][0].min(segment[1][0]) - tolerance
            && point[0] <= segment[0][0].max(segment[1][0]) + tolerance
            && point[1] >= segment[0][1].min(segment[1][1]) - tolerance
            && point[1] <= segment[0][1].max(segment[1][1]) + tolerance
    };
    let orientations = [
        orient(first[0], first[1], second[0]),
        orient(first[0], first[1], second[1]),
        orient(second[0], second[1], first[0]),
        orient(second[0], second[1], first[1]),
    ];
    let first_length = (first[1][0] - first[0][0]).hypot(first[1][1] - first[0][1]);
    let second_length = (second[1][0] - second[0][0]).hypot(second[1][1] - second[0][1]);
    let first_cross_tolerance = tolerance * first_length.max(1.0);
    let second_cross_tolerance = tolerance * second_length.max(1.0);
    let opposite = |left: f64, right: f64, cross_tolerance: f64| {
        (left > cross_tolerance && right < -cross_tolerance)
            || (left < -cross_tolerance && right > cross_tolerance)
    };
    if opposite(orientations[0], orientations[1], first_cross_tolerance)
        && opposite(orientations[2], orientations[3], second_cross_tolerance)
    {
        return true;
    }
    (orientations[0].abs() <= first_cross_tolerance && on_segment(first, second[0]))
        || (orientations[1].abs() <= first_cross_tolerance && on_segment(first, second[1]))
        || (orientations[2].abs() <= second_cross_tolerance && on_segment(second, first[0]))
        || (orientations[3].abs() <= second_cross_tolerance && on_segment(second, first[1]))
}

fn profile_arc(
    segment: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
) -> Option<([f64; 2], f64, f64, f64)> {
    let SketchGeometry::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = &segment.0
    else {
        return None;
    };
    let forward_delta = (end_angle.0 - start_angle.0).rem_euclid(std::f64::consts::TAU);
    let delta = if segment.1 {
        -forward_delta
    } else {
        forward_delta
    };
    Some((
        [center.u, center.v],
        radius.0,
        (segment.2[1] - center.v).atan2(segment.2[0] - center.u),
        delta,
    ))
}

fn point_on_profile_arc(point: [f64; 2], arc: ([f64; 2], f64, f64, f64), tolerance: f64) -> bool {
    let (center, radius, start, delta) = arc;
    let relative = [point[0] - center[0], point[1] - center[1]];
    let distance = relative[0].hypot(relative[1]);
    if (distance - radius).abs() > tolerance {
        return false;
    }
    let angle = relative[1].atan2(relative[0]);
    let travel = if delta >= 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU)
    } else {
        (start - angle).rem_euclid(std::f64::consts::TAU)
    };
    travel <= delta.abs() + tolerance / radius.max(1.0)
}

fn line_arc_intersect(line: [[f64; 2]; 2], arc: ([f64; 2], f64, f64, f64), tolerance: f64) -> bool {
    let direction = [line[1][0] - line[0][0], line[1][1] - line[0][1]];
    let relative = [line[0][0] - arc.0[0], line[0][1] - arc.0[1]];
    let a = direction[0].mul_add(direction[0], direction[1] * direction[1]);
    let b = 2.0 * direction[0].mul_add(relative[0], direction[1] * relative[1]);
    let c = relative[0].mul_add(relative[0], relative[1] * relative[1]) - arc.1 * arc.1;
    let discriminant = b.mul_add(b, -(4.0 * a * c));
    if a <= tolerance * tolerance || discriminant < -tolerance * tolerance {
        return false;
    }
    let root = discriminant.max(0.0).sqrt();
    [-root, root].into_iter().any(|signed_root| {
        let parameter = (-b + signed_root) / (2.0 * a);
        parameter >= -tolerance
            && parameter <= 1.0 + tolerance
            && point_on_profile_arc(
                [
                    line[0][0] + parameter * direction[0],
                    line[0][1] + parameter * direction[1],
                ],
                arc,
                tolerance,
            )
    })
}

fn arcs_intersect(
    first: ([f64; 2], f64, f64, f64),
    second: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let displacement = [second.0[0] - first.0[0], second.0[1] - first.0[1]];
    let distance = displacement[0].hypot(displacement[1]);
    if distance <= tolerance && (first.1 - second.1).abs() <= tolerance {
        let endpoints = |arc: ([f64; 2], f64, f64, f64)| {
            [
                [
                    arc.0[0] + arc.1 * arc.2.cos(),
                    arc.0[1] + arc.1 * arc.2.sin(),
                ],
                [
                    arc.0[0] + arc.1 * (arc.2 + arc.3).cos(),
                    arc.0[1] + arc.1 * (arc.2 + arc.3).sin(),
                ],
            ]
        };
        return endpoints(first)
            .into_iter()
            .any(|point| point_on_profile_arc(point, second, tolerance))
            || endpoints(second)
                .into_iter()
                .any(|point| point_on_profile_arc(point, first, tolerance));
    }
    if distance <= tolerance
        || distance > first.1 + second.1 + tolerance
        || distance < (first.1 - second.1).abs() - tolerance
    {
        return false;
    }
    let along = (first.1 * first.1 - second.1 * second.1 + distance * distance) / (2.0 * distance);
    let height_squared = first.1 * first.1 - along * along;
    if height_squared < -tolerance * tolerance {
        return false;
    }
    let base = [
        first.0[0] + along * displacement[0] / distance,
        first.0[1] + along * displacement[1] / distance,
    ];
    let height = height_squared.max(0.0).sqrt();
    let offset = [
        -height * displacement[1] / distance,
        height * displacement[0] / distance,
    ];
    [-1.0, 1.0].into_iter().any(|sign| {
        let point = [base[0] + sign * offset[0], base[1] + sign * offset[1]];
        point_on_profile_arc(point, first, tolerance)
            && point_on_profile_arc(point, second, tolerance)
    })
}

fn profile_segments_intersect(
    first: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    second: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    tolerance: f64,
) -> bool {
    match (profile_arc(first), profile_arc(second)) {
        (None, None) => segments_intersect([first.2, first.3], [second.2, second.3], tolerance),
        (None, Some(arc)) => line_arc_intersect([first.2, first.3], arc, tolerance),
        (Some(arc), None) => line_arc_intersect([second.2, second.3], arc, tolerance),
        (Some(first), Some(second)) => arcs_intersect(first, second, tolerance),
    }
}

fn profile_strictly_contains(profile: &ExtrusionProfile, point: [f64; 2]) -> bool {
    let mut winding = 0.0;
    for segment in profile {
        let mut accumulate = |first: [f64; 2], second: [f64; 2]| {
            let first = [first[0] - point[0], first[1] - point[1]];
            let second = [second[0] - point[0], second[1] - point[1]];
            winding += first[0]
                .mul_add(second[1], -(first[1] * second[0]))
                .atan2(first[0].mul_add(second[0], first[1] * second[1]));
        };
        if let Some((center, radius, start, delta)) = profile_arc(segment) {
            let pieces = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
            for piece in 0..pieces {
                let first = start + delta * piece as f64 / pieces as f64;
                let second = start + delta * (piece + 1) as f64 / pieces as f64;
                accumulate(
                    [
                        center[0] + radius * first.cos(),
                        center[1] + radius * first.sin(),
                    ],
                    [
                        center[0] + radius * second.cos(),
                        center[1] + radius * second.sin(),
                    ],
                );
            }
        } else {
            accumulate(segment.2, segment.3);
        }
    }
    winding.abs() > std::f64::consts::PI
}

fn ordered_extrusion_profiles(
    mut profiles: Vec<ExtrusionProfile>,
) -> Option<(Vec<ExtrusionProfile>, f64)> {
    let scale = profiles
        .iter()
        .flatten()
        .flat_map(|(_, _, start, end)| start.iter().chain(end))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let tolerance = 1e-9 * scale;
    for profile in &profiles {
        for first in 0..profile.len() {
            for second in first + 1..profile.len() {
                if second == first + 1 || (first == 0 && second + 1 == profile.len()) {
                    continue;
                }
                if profile_segments_intersect(&profile[first], &profile[second], tolerance) {
                    return None;
                }
            }
        }
    }
    for first in 0..profiles.len() {
        for second in first + 1..profiles.len() {
            for first_segment in &profiles[first] {
                for second_segment in &profiles[second] {
                    if profile_segments_intersect(first_segment, second_segment, tolerance) {
                        return None;
                    }
                }
            }
        }
    }
    let outer = profiles
        .iter()
        .enumerate()
        .filter(|(candidate, profile)| {
            profiles.iter().enumerate().all(|(index, inner)| {
                index == *candidate || profile_strictly_contains(profile, inner[0].2)
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [outer] = outer.as_slice() else {
        return None;
    };
    for first in 0..profiles.len() {
        if first == *outer {
            continue;
        }
        for second in first + 1..profiles.len() {
            if second == *outer {
                continue;
            }
            if profile_strictly_contains(&profiles[first], profiles[second][0].2)
                || profile_strictly_contains(&profiles[second], profiles[first][0].2)
            {
                return None;
            }
        }
    }
    let outer_area = extrusion_profile_signed_area(&profiles[*outer])?;
    if profiles.iter().enumerate().any(|(index, profile)| {
        index != *outer
            && extrusion_profile_signed_area(profile)
                .is_none_or(|area| area.is_sign_positive() == outer_area.is_sign_positive())
    }) {
        return None;
    }
    profiles.swap(0, *outer);
    Some((profiles, outer_area))
}

fn add_extrusion_pcurve(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    id: PcurveId,
    source_offset: usize,
    geometry: PcurveGeometry,
) -> PcurveId {
    annotate(
        annotations,
        &id,
        "FeatDefs",
        source_offset as u64,
        "extrusion_trim_pcurve",
        Exactness::Derived,
    );
    ir.model.pcurves.push(Pcurve {
        id: id.clone(),
        geometry,
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: Some([0.0, 1.0]),
        fit_tolerance: None,
    });
    id
}

fn revolution_boundary_pcurve(
    surface: &SurfaceGeometry,
    point: [f64; 3],
    axis: RevolutionAxis,
) -> Option<PcurveGeometry> {
    let axis_direction = normalized([axis.direction.x, axis.direction.y, axis.direction.z])?;
    let axis_origin = [axis.origin.x, axis.origin.y, axis.origin.z];
    let point_from = |origin: Point3| {
        [
            point[0] - origin.x,
            point[1] - origin.y,
            point[2] - origin.z,
        ]
    };
    let vector = |value: Vector3| [value.x, value.y, value.z];
    let azimuth = |relative: [f64; 3], carrier_axis: [f64; 3], reference: [f64; 3]| {
        let tangent = cross(carrier_axis, reference);
        dot(relative, tangent).atan2(dot(relative, reference))
    };
    match surface {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let normal = vector(*normal);
            let u_axis = vector(*u_axis);
            let v_axis = cross(normal, u_axis);
            let axis_relative = [
                axis_origin[0] - origin.x,
                axis_origin[1] - origin.y,
                axis_origin[2] - origin.z,
            ];
            let center = [dot(axis_relative, u_axis), dot(axis_relative, v_axis)];
            let relative = point_from(*origin);
            let uv = [dot(relative, u_axis), dot(relative, v_axis)];
            let radial = [uv[0] - center[0], uv[1] - center[1]];
            let radius = radial[0].hypot(radial[1]);
            (radius > 1e-12).then_some(())?;
            let start = radial[1].atan2(radial[0]);
            let direction = if dot(normal, axis_direction).is_sign_negative() {
                -std::f64::consts::TAU
            } else {
                std::f64::consts::TAU
            };
            Some(circular_pcurve(center, radius, start, start + direction))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            ..
        }
        | SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            ..
        } => {
            let carrier_axis = vector(*axis);
            let relative = point_from(*origin);
            let u = azimuth(relative, carrier_axis, vector(*ref_direction));
            let v = dot(relative, carrier_axis);
            let direction = if dot(carrier_axis, axis_direction).is_sign_negative() {
                -std::f64::consts::TAU
            } else {
                std::f64::consts::TAU
            };
            Some(line_pcurve([u, v], [u + direction, v]))
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            ..
        } => {
            let carrier_axis = vector(*axis);
            let relative = point_from(*center);
            let u = azimuth(relative, carrier_axis, vector(*ref_direction));
            let axial = dot(relative, carrier_axis);
            let radial = std::array::from_fn::<_, 3, _>(|index| {
                relative[index] - axial * carrier_axis[index]
            });
            let v = axial.atan2(dot(radial, radial).sqrt());
            Some(line_pcurve([u, v], [u + std::f64::consts::TAU, v]))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            let carrier_axis = vector(*axis);
            let reference = vector(*ref_direction);
            let relative = point_from(*center);
            let axial = dot(relative, carrier_axis);
            let radial = std::array::from_fn::<_, 3, _>(|index| {
                relative[index] - axial * carrier_axis[index]
            });
            let radial_distance = dot(radial, radial).sqrt();
            let positive_residual = ((radial_distance - major_radius)
                .mul_add(radial_distance - major_radius, axial * axial)
                - minor_radius * minor_radius)
                .abs();
            let negative_residual = ((-radial_distance - major_radius)
                .mul_add(-radial_distance - major_radius, axial * axial)
                - minor_radius * minor_radius)
                .abs();
            let base_u = azimuth(relative, carrier_axis, reference);
            let (u, signed_ring) = if negative_residual < positive_residual {
                (base_u + std::f64::consts::PI, -radial_distance)
            } else {
                (base_u, radial_distance)
            };
            let scale = minor_radius.abs().max(radial_distance).max(1.0);
            (positive_residual.min(negative_residual) <= 1e-9 * scale * scale).then_some(())?;
            let v = axial.atan2(signed_ring - major_radius);
            Some(line_pcurve([u, v], [u + std::f64::consts::TAU, v]))
        }
        SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

fn revolution_face_sense(
    transform: &crate::placement::FeatureSectionTransform,
    segment: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    surface: &SurfaceGeometry,
    axis: RevolutionAxis,
    profile_area: f64,
) -> Option<Sense> {
    let (point, tangent) = if let Some((center, radius, start, delta)) = profile_arc(segment) {
        let angle = start + 0.5 * delta;
        (
            [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ],
            [-delta.signum() * angle.sin(), delta.signum() * angle.cos()],
        )
    } else {
        (
            [
                0.5 * (segment.2[0] + segment.3[0]),
                0.5 * (segment.2[1] + segment.3[1]),
            ],
            [segment.3[0] - segment.2[0], segment.3[1] - segment.2[1]],
        )
    };
    let outward = if profile_area.is_sign_positive() {
        [tangent[1], -tangent[0]]
    } else {
        [-tangent[1], tangent[0]]
    };
    let outward = normalized(std::array::from_fn(|index| {
        outward[0] * transform.u_axis[index] + outward[1] * transform.v_axis[index]
    }))?;
    let model_point = section_point_in_model(transform, point);
    let pcurve = revolution_boundary_pcurve(surface, model_point, axis)?;
    let uv = cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.0)?;
    let epsilon = 1e-6;
    let before_u = cadmpeg_ir::eval::surface_point(surface, uv.u - epsilon, uv.v)?;
    let after_u = cadmpeg_ir::eval::surface_point(surface, uv.u + epsilon, uv.v)?;
    let before_v = cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v - epsilon)?;
    let after_v = cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v + epsilon)?;
    let du = [
        after_u.x - before_u.x,
        after_u.y - before_u.y,
        after_u.z - before_u.z,
    ];
    let dv = [
        after_v.x - before_v.x,
        after_v.y - before_v.y,
        after_v.z - before_v.z,
    ];
    let carrier_normal = normalized(cross(du, dv))?;
    let alignment = dot(carrier_normal, outward);
    (alignment.abs() > 1e-8).then_some(())?;
    Some(if alignment.is_sign_positive() {
        Sense::Forward
    } else {
        Sense::Reversed
    })
}

fn transfer_resolved_revolution_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if current_additive_feature_recipe(&scan.features.operations, feature_id)
            != Some(crate::feature::FeatureRecipeKind::Revolve)
            || !feature_is_first_material_operation(scan, feature_id)
            || unique_feature_revolution_extent_kind(&scan.features.revolution_extents, feature_id)
                != Some(crate::feature::FeatureRevolutionExtentKind::FullTurn)
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let Some(axis) = resolved_revolution_axis(definition, transform) else {
            continue;
        };
        let sketch_id = model_sketch_id(scan, definition);
        let Some(mut profiles) = resolved_sketch_profiles(ir, &sketch_id, 2) else {
            continue;
        };
        let [profile] = profiles.as_mut_slice() else {
            continue;
        };
        let Some(area) = extrusion_profile_signed_area(profile) else {
            continue;
        };
        let vertex_curves = profile
            .iter()
            .map(|(_, _, point, _)| revolved_section_circle(transform, *point, axis))
            .collect::<Vec<_>>();
        let surface_geometries = profile
            .iter()
            .map(|(geometry, _, _, _)| revolved_section_surface(transform, geometry, axis))
            .collect::<Option<Vec<_>>>();
        let Some(surface_geometries) = surface_geometries else {
            continue;
        };
        let boundaries_are_complete =
            profile
                .iter()
                .enumerate()
                .all(|(index, (_, _, start, end))| {
                    let next = (index + 1) % profile.len();
                    (vertex_curves[index].is_some() || vertex_curves[next].is_some())
                        && [
                            (*start, vertex_curves[index].is_some()),
                            (*end, vertex_curves[next].is_some()),
                        ]
                        .into_iter()
                        .all(|(section_point, present)| {
                            !present
                                || revolution_boundary_pcurve(
                                    &surface_geometries[index],
                                    section_point_in_model(transform, section_point),
                                    axis,
                                )
                                .is_some()
                        })
                });
        if !boundaries_are_complete {
            continue;
        }
        let face_senses = profile
            .iter()
            .zip(&surface_geometries)
            .map(|(segment, surface)| {
                revolution_face_sense(transform, segment, surface, axis, area)
            })
            .collect::<Option<Vec<_>>>();
        let Some(face_senses) = face_senses else {
            continue;
        };
        let prefix = format!("creo:feature:revolution#{feature_id}");
        let body_id = BodyId(format!("{prefix}:body"));
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id = RegionId(format!("{prefix}:region"));
        let shell_id = ShellId(format!("{prefix}:shell"));
        let count = profile.len();
        let mut edges = vec![None; count];
        for (index, ((_, _, point, _), curve_geometry)) in
            profile.iter().zip(vertex_curves).enumerate()
        {
            let Some(curve_geometry) = curve_geometry else {
                continue;
            };
            let CurveGeometry::Circle {
                center,
                axis: curve_axis,
                ref_direction,
                radius,
            } = curve_geometry
            else {
                unreachable!();
            };
            let curve_id = CurveId(format!("{prefix}:curve:vertex:{index}"));
            let point_id = PointId(format!("{prefix}:point:vertex:{index}"));
            let vertex_id = VertexId(format!("{prefix}:vertex:{index}"));
            let edge_id = EdgeId(format!("{prefix}:edge:vertex:{index}"));
            let position = section_point_in_model(transform, *point);
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Circle {
                    center,
                    axis: curve_axis,
                    ref_direction,
                    radius,
                },
                source_object: None,
            });
            ir.model.points.push(Point {
                id: point_id.clone(),
                position: Point3::new(position[0], position[1], position[2]),
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                curve: Some(curve_id),
                start: vertex_id.clone(),
                end: vertex_id,
                param_range: Some([0.0, std::f64::consts::TAU]),
                tolerance: None,
            });
            edges[index] = Some(edge_id);
        }
        let mut faces = Vec::new();
        for (index, (((_, _, start, end), surface_geometry), face_sense)) in profile
            .iter()
            .zip(surface_geometries)
            .zip(face_senses)
            .enumerate()
        {
            let next = (index + 1) % count;
            let surface_id = SurfaceId(format!("{prefix}:surface:{index}"));
            let face_id = FaceId(format!("{prefix}:face:{index}"));
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: surface_geometry.clone(),
                source_object: None,
            });
            let mut loops = Vec::new();
            for (boundary, vertex_index, section_point, sense) in [
                ("start", index, *start, Sense::Reversed),
                ("end", next, *end, Sense::Forward),
            ] {
                let Some(edge_id) = edges[vertex_index].clone() else {
                    continue;
                };
                let loop_id = LoopId(format!("{prefix}:loop:{index}:{boundary}"));
                let coedge_id = CoedgeId(format!("{prefix}:coedge:{index}:{boundary}"));
                let radial_index = if boundary == "start" {
                    (index + count - 1) % count
                } else {
                    next
                };
                let radial_boundary = if boundary == "start" { "end" } else { "start" };
                let point = section_point_in_model(transform, section_point);
                let pcurve_geometry = revolution_boundary_pcurve(&surface_geometry, point, axis)
                    .expect("revolution boundary was prevalidated");
                let pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    PcurveId(format!("{prefix}:pcurve:{index}:{boundary}")),
                    transform.offset,
                    pcurve_geometry,
                );
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face_id.clone(),
                    boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                    coedges: vec![coedge_id.clone()],
                    vertex_uses: Vec::new(),
                });
                ir.model.coedges.push(Coedge {
                    id: coedge_id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge_id,
                    next: coedge_id.clone(),
                    previous: coedge_id,
                    radial_next: CoedgeId(format!(
                        "{prefix}:coedge:{radial_index}:{radial_boundary}"
                    )),
                    sense,
                    pcurves: vec![PcurveUse {
                        pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
                loops.push(loop_id);
            }
            ir.model.faces.push(Face {
                id: face_id.clone(),
                shell: shell_id.clone(),
                surface: surface_id,
                sense: face_sense,
                loops,
                name: None,
                color: None,
                tolerance: None,
            });
            faces.push(face_id);
        }
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        ir.model.bodies.push(Body {
            id: body_id,
            kind: BodyKind::Solid,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        transferred += 1;
    }
    transferred
}

fn transfer_resolved_circular_extrusion_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_additive_linear_extrusion(scan, feature_id)
            || !feature_is_first_material_operation(scan, feature_id)
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let sketch_id = model_sketch_id(scan, definition);
        let Some((section_center, radius)) =
            resolved_circular_extrusion_profile(scan, ir, transform, feature_id, &sketch_id)
        else {
            continue;
        };
        let Some(span) = extrusion_span(
            transform.origin,
            transform.normal,
            feature_plane_equations(scan, feature_id),
        ) else {
            continue;
        };
        let prefix = format!("creo:feature:extrusion#{feature_id}");
        let body_id = BodyId(format!("{prefix}:body"));
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id = RegionId(format!("{prefix}:region"));
        let shell_id = ShellId(format!("{prefix}:shell"));
        let center = section_point_in_model(transform, section_center);
        let seam =
            std::array::from_fn::<_, 3, _>(|axis| center[axis] + radius * transform.u_axis[axis]);
        let sides = [("bottom", span.lower), ("top", span.upper)];
        let mut face_ids = Vec::new();
        let mut cap_coedges = Vec::new();
        let mut side_coedges = Vec::new();
        for (side_index, (side, offset)) in sides.into_iter().enumerate() {
            let cap_surface = SurfaceId(format!("{prefix}:surface:{side}"));
            let cap_face = FaceId(format!("{prefix}:face:{side}"));
            let cap_loop = LoopId(format!("{prefix}:loop:{side}"));
            let curve_id = CurveId(format!("{prefix}:curve:{side}"));
            let edge_id = EdgeId(format!("{prefix}:edge:{side}"));
            let point_id = PointId(format!("{prefix}:point:{side}"));
            let vertex_id = VertexId(format!("{prefix}:vertex:{side}"));
            let cap_coedge = CoedgeId(format!("{prefix}:coedge:{side}:cap"));
            let side_coedge = CoedgeId(format!("{prefix}:coedge:{side}:side"));
            let cap_pcurve = add_extrusion_pcurve(
                ir,
                annotations,
                PcurveId(format!("{prefix}:pcurve:{side}:cap")),
                transform.offset,
                circular_pcurve(section_center, radius, 0.0, std::f64::consts::TAU),
            );
            let side_pcurve = add_extrusion_pcurve(
                ir,
                annotations,
                PcurveId(format!("{prefix}:pcurve:{side}:side")),
                transform.offset,
                line_pcurve([0.0, offset], [std::f64::consts::TAU, offset]),
            );
            ir.model.surfaces.push(Surface {
                id: cap_surface.clone(),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(
                        transform.origin[0] + offset * transform.normal[0],
                        transform.origin[1] + offset * transform.normal[1],
                        transform.origin[2] + offset * transform.normal[2],
                    ),
                    normal: Vector3::new(
                        transform.normal[0],
                        transform.normal[1],
                        transform.normal[2],
                    ),
                    u_axis: Vector3::new(
                        transform.u_axis[0],
                        transform.u_axis[1],
                        transform.u_axis[2],
                    ),
                },
                source_object: None,
            });
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Circle {
                    center: Point3::new(
                        center[0] + offset * transform.normal[0],
                        center[1] + offset * transform.normal[1],
                        center[2] + offset * transform.normal[2],
                    ),
                    axis: Vector3::new(
                        transform.normal[0],
                        transform.normal[1],
                        transform.normal[2],
                    ),
                    ref_direction: Vector3::new(
                        transform.u_axis[0],
                        transform.u_axis[1],
                        transform.u_axis[2],
                    ),
                    radius,
                },
                source_object: None,
            });
            ir.model.points.push(Point {
                id: point_id.clone(),
                position: Point3::new(
                    seam[0] + offset * transform.normal[0],
                    seam[1] + offset * transform.normal[1],
                    seam[2] + offset * transform.normal[2],
                ),
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                curve: Some(curve_id),
                start: vertex_id.clone(),
                end: vertex_id,
                param_range: Some([0.0, std::f64::consts::TAU]),
                tolerance: None,
            });
            ir.model.loops.push(IrLoop {
                id: cap_loop.clone(),
                face: cap_face.clone(),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Outer,
                coedges: vec![cap_coedge.clone()],
                vertex_uses: Vec::new(),
            });
            ir.model.coedges.push(Coedge {
                id: cap_coedge.clone(),
                owner_loop: cap_loop.clone(),
                edge: edge_id.clone(),
                next: cap_coedge.clone(),
                previous: cap_coedge.clone(),
                radial_next: side_coedge.clone(),
                sense: if side_index == 0 {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                pcurves: vec![PcurveUse {
                    pcurve: cap_pcurve,
                    isoparametric: None,
                    parameter_range: None,
                }],
                use_curve: None,
                use_curve_parameter_range: None,
            });
            ir.model.faces.push(Face {
                id: cap_face.clone(),
                shell: shell_id.clone(),
                surface: cap_surface,
                sense: if side_index == 0 {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                loops: vec![cap_loop],
                name: None,
                color: None,
                tolerance: None,
            });
            face_ids.push(cap_face);
            cap_coedges.push(cap_coedge);
            side_coedges.push((side_coedge, edge_id, side_pcurve));
        }
        let side_surface = SurfaceId(format!("{prefix}:surface:side"));
        let side_face = FaceId(format!("{prefix}:face:side"));
        let mut side_loops = Vec::new();
        ir.model.surfaces.push(Surface {
            id: side_surface.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(
                    transform.normal[0],
                    transform.normal[1],
                    transform.normal[2],
                ),
                ref_direction: Vector3::new(
                    transform.u_axis[0],
                    transform.u_axis[1],
                    transform.u_axis[2],
                ),
                radius,
            },
            source_object: None,
        });
        for (side_index, ((side, _), (coedge, edge, pcurve))) in
            sides.into_iter().zip(side_coedges).enumerate()
        {
            let loop_id = LoopId(format!("{prefix}:loop:side:{side}"));
            ir.model.loops.push(IrLoop {
                id: loop_id.clone(),
                face: side_face.clone(),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                coedges: vec![coedge.clone()],
                vertex_uses: Vec::new(),
            });
            ir.model.coedges.push(Coedge {
                id: coedge.clone(),
                owner_loop: loop_id.clone(),
                edge,
                next: coedge.clone(),
                previous: coedge.clone(),
                radial_next: cap_coedges[side_index].clone(),
                sense: if side_index == 0 {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: vec![PcurveUse {
                    pcurve,
                    isoparametric: None,
                    parameter_range: None,
                }],
                use_curve: None,
                use_curve_parameter_range: None,
            });
            side_loops.push(loop_id);
        }
        ir.model.faces.push(Face {
            id: side_face.clone(),
            shell: shell_id.clone(),
            surface: side_surface,
            sense: Sense::Forward,
            loops: side_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        face_ids.push(side_face);
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: face_ids,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        ir.model.bodies.push(Body {
            id: body_id,
            kind: BodyKind::Solid,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        transferred += 1;
    }
    transferred
}

fn resolved_circular_extrusion_profile(
    scan: &ContainerScan,
    ir: &CadIr,
    transform: &crate::placement::FeatureSectionTransform,
    feature_id: u32,
    sketch_id: &SketchId,
) -> Option<([f64; 2], f64)> {
    if let Some(sketch) = ir
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id == *sketch_id)
    {
        if let [profile] = sketch.profiles.as_slice() {
            if let [entity_use] = profile.as_slice() {
                if let Some(SketchGeometry::Circle { center, radius }) = ir
                    .model
                    .sketch_entities
                    .iter()
                    .find(|entity| entity.id == entity_use.entity && entity.sketch == *sketch_id)
                    .map(|entity| &entity.geometry)
                {
                    return Some(([center.u, center.v], radius.0));
                }
            }
        }
    }
    let sweep = circular_sweep_geometry(scan, feature_id)?;
    sweep
        .section_definition_id
        .is_none_or(|definition_id| definition_id == transform.definition_id)
        .then_some(())?;
    circular_section_profile_from_cylinder(transform, &sweep.geometry)
}

fn circular_section_profile_from_cylinder(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SurfaceGeometry,
) -> Option<([f64; 2], f64)> {
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        radius,
        ..
    } = geometry
    else {
        return None;
    };
    let axis = normalized([axis.x, axis.y, axis.z])?;
    (dot(axis, transform.normal).abs() >= 1.0 - 1e-9 && radius.is_finite() && *radius > 0.0)
        .then_some(())?;
    let delta = [
        origin.x - transform.origin[0],
        origin.y - transform.origin[1],
        origin.z - transform.origin[2],
    ];
    Some((
        [dot(delta, transform.u_axis), dot(delta, transform.v_axis)],
        *radius,
    ))
}

fn sketch_profiles_cover_generated_extrusion_sides(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
    feature_id: u32,
    sketch: &Sketch,
) -> bool {
    let expected_entities = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| &table.entries)
        .filter_map(|entry| {
            let external_id = entry.source_entity_id?;
            scan.surfaces
                .rows
                .iter()
                .any(|row| {
                    row.id == entry.entity_id
                        && row.feature_id == feature_id
                        && matches!(
                            row.kind,
                            crate::surface::SurfaceKind::Plane
                                | crate::surface::SurfaceKind::Cylinder
                                | crate::surface::SurfaceKind::Extrusion
                        )
                })
                .then(|| {
                    SketchEntityId(format!(
                        "creo:featdefs:sketch_entity#{}:{external_id}",
                        definition.id
                    ))
                })
        })
        .collect::<Vec<_>>();
    let expected = expected_entities.iter().cloned().collect::<BTreeSet<_>>();
    let profile_entities = sketch
        .profiles
        .iter()
        .flatten()
        .map(|entity_use| entity_use.entity.clone())
        .collect::<Vec<_>>();
    !expected.is_empty()
        && expected_entities.len() == expected.len()
        && profile_entities.len() == expected.len()
        && profile_entities.into_iter().collect::<BTreeSet<_>>() == expected
}

fn transfer_resolved_extrusion_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_additive_linear_extrusion(scan, feature_id)
            || !feature_is_first_material_operation(scan, feature_id)
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let sketch_id = model_sketch_id(scan, definition);
        let Some(span) = resolved_feature_extrusion_span(scan, definition, transform) else {
            continue;
        };
        let length = span.upper - span.lower;
        let Some(sketch) = ir
            .model
            .sketches
            .iter()
            .find(|sketch| sketch.id == sketch_id)
        else {
            continue;
        };
        if !sketch_profiles_cover_generated_extrusion_sides(scan, definition, feature_id, sketch) {
            continue;
        }
        let Some(profiles) = resolved_sketch_profiles(ir, &sketch_id, 2) else {
            continue;
        };
        let Some((profiles, outer_area)) = ordered_extrusion_profiles(profiles) else {
            continue;
        };
        if profiles.iter().flatten().any(|(geometry, _, start, end)| {
            matches!(geometry, SketchGeometry::Line { .. }) && start == end
        }) {
            continue;
        }
        if profiles
            .iter()
            .flatten()
            .any(|(geometry, _, _, _)| extruded_geometry_surface(transform, geometry).is_none())
        {
            continue;
        }
        let forward_caps = outer_area > 0.0;

        let prefix = format!("creo:feature:extrusion#{feature_id}");
        let body_id = BodyId(format!("{prefix}:body"));
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id = RegionId(format!("{prefix}:region"));
        let shell_id = ShellId(format!("{prefix}:shell"));
        let bottom_surface = SurfaceId(format!("{prefix}:surface:bottom"));
        let top_surface = SurfaceId(format!("{prefix}:surface:top"));
        for (id, offset) in [(&bottom_surface, span.lower), (&top_surface, span.upper)] {
            annotate(
                annotations,
                id,
                "FeatDefs",
                transform.offset as u64,
                "extrusion_cap_plane",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(
                        transform.origin[0] + offset * transform.normal[0],
                        transform.origin[1] + offset * transform.normal[1],
                        transform.origin[2] + offset * transform.normal[2],
                    ),
                    normal: Vector3::new(
                        transform.normal[0],
                        transform.normal[1],
                        transform.normal[2],
                    ),
                    u_axis: Vector3::new(
                        transform.u_axis[0],
                        transform.u_axis[1],
                        transform.u_axis[2],
                    ),
                },
                source_object: None,
            });
        }

        let bottom_face = FaceId(format!("{prefix}:face:bottom"));
        let top_face = FaceId(format!("{prefix}:face:top"));
        let mut shell_faces = vec![bottom_face.clone(), top_face.clone()];
        let mut bottom_loops = Vec::new();
        let mut top_loops = Vec::new();
        for (profile_index, profile) in profiles.iter().enumerate() {
            let count = profile.len();
            let mut bottom_vertices = Vec::new();
            let mut top_vertices = Vec::new();
            for (index, (_, _, start, _)) in profile.iter().enumerate() {
                for (side, offset, arena) in [
                    ("bottom", span.lower, &mut bottom_vertices),
                    ("top", span.upper, &mut top_vertices),
                ] {
                    let position = section_point_in_model(transform, *start);
                    let point_id =
                        PointId(format!("{prefix}:point:{profile_index}:{index}:{side}"));
                    let vertex_id =
                        VertexId(format!("{prefix}:vertex:{profile_index}:{index}:{side}"));
                    ir.model.points.push(Point {
                        id: point_id.clone(),
                        position: Point3::new(
                            position[0] + offset * transform.normal[0],
                            position[1] + offset * transform.normal[1],
                            position[2] + offset * transform.normal[2],
                        ),
                        source_object: None,
                    });
                    ir.model.vertices.push(Vertex {
                        id: vertex_id.clone(),
                        point: point_id,
                        tolerance: None,
                    });
                    arena.push(vertex_id);
                }
            }

            let mut bottom_edges = Vec::new();
            let mut top_edges = Vec::new();
            let mut vertical_edges = Vec::new();
            for (index, (geometry, reversed, start, end)) in profile.iter().enumerate() {
                let next = (index + 1) % count;
                for (side, offset, vertices, arena) in [
                    ("bottom", span.lower, &bottom_vertices, &mut bottom_edges),
                    ("top", span.upper, &top_vertices, &mut top_edges),
                ] {
                    let curve_id =
                        CurveId(format!("{prefix}:curve:{profile_index}:{index}:{side}"));
                    let edge_id = EdgeId(format!("{prefix}:edge:{profile_index}:{index}:{side}"));
                    let curve = match geometry {
                        SketchGeometry::Line { .. } => {
                            let placed_start = section_point_in_model(transform, *start);
                            let placed_end = section_point_in_model(transform, *end);
                            let Some(direction) = normalized(std::array::from_fn(|axis| {
                                placed_end[axis] - placed_start[axis]
                            })) else {
                                continue;
                            };
                            CurveGeometry::Line {
                                origin: Point3::new(
                                    placed_start[0] + offset * transform.normal[0],
                                    placed_start[1] + offset * transform.normal[1],
                                    placed_start[2] + offset * transform.normal[2],
                                ),
                                direction: Vector3::new(direction[0], direction[1], direction[2]),
                            }
                        }
                        SketchGeometry::Arc { center, radius, .. }
                        | SketchGeometry::Circle { center, radius } => {
                            let center = section_point_in_model(transform, [center.u, center.v]);
                            let (axis_sign, _) = oriented_arc_parameterization(*reversed, 0.0, 0.0);
                            CurveGeometry::Circle {
                                center: Point3::new(
                                    center[0] + offset * transform.normal[0],
                                    center[1] + offset * transform.normal[1],
                                    center[2] + offset * transform.normal[2],
                                ),
                                axis: Vector3::new(
                                    axis_sign * transform.normal[0],
                                    axis_sign * transform.normal[1],
                                    axis_sign * transform.normal[2],
                                ),
                                ref_direction: Vector3::new(
                                    transform.u_axis[0],
                                    transform.u_axis[1],
                                    transform.u_axis[2],
                                ),
                                radius: radius.0,
                            }
                        }
                        _ => unreachable!("profile family checked above"),
                    };
                    ir.model.curves.push(Curve {
                        id: curve_id.clone(),
                        geometry: curve,
                        source_object: None,
                    });
                    let param_range = match geometry {
                        SketchGeometry::Line { .. } => {
                            Some([0.0, (end[0] - start[0]).hypot(end[1] - start[1])])
                        }
                        SketchGeometry::Arc {
                            start_angle,
                            end_angle,
                            ..
                        } => Some(
                            oriented_arc_parameterization(*reversed, start_angle.0, end_angle.0).1,
                        ),
                        _ => None,
                    };
                    ir.model.edges.push(Edge {
                        id: edge_id.clone(),
                        curve: Some(curve_id),
                        start: vertices[index].clone(),
                        end: vertices[next].clone(),
                        param_range,
                        tolerance: None,
                    });
                    arena.push(edge_id);
                }
                let curve_id = CurveId(format!("{prefix}:curve:{profile_index}:{index}:vertical"));
                let edge_id = EdgeId(format!("{prefix}:edge:{profile_index}:{index}:vertical"));
                let origin = section_point_in_model(transform, *start);
                ir.model.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Line {
                        origin: Point3::new(
                            origin[0] + span.lower * transform.normal[0],
                            origin[1] + span.lower * transform.normal[1],
                            origin[2] + span.lower * transform.normal[2],
                        ),
                        direction: Vector3::new(
                            transform.normal[0],
                            transform.normal[1],
                            transform.normal[2],
                        ),
                    },
                    source_object: None,
                });
                ir.model.edges.push(Edge {
                    id: edge_id.clone(),
                    curve: Some(curve_id),
                    start: bottom_vertices[index].clone(),
                    end: top_vertices[index].clone(),
                    param_range: Some([0.0, length]),
                    tolerance: None,
                });
                vertical_edges.push(edge_id);
            }

            let bottom_loop = LoopId(format!("{prefix}:loop:{profile_index}:bottom"));
            let top_loop = LoopId(format!("{prefix}:loop:{profile_index}:top"));
            bottom_loops.push(bottom_loop.clone());
            top_loops.push(top_loop.clone());
            let bottom_coedges = (0..count)
                .rev()
                .map(|index| {
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{index}:bottom-cap"
                    ))
                })
                .collect::<Vec<_>>();
            let top_coedges = (0..count)
                .map(|index| CoedgeId(format!("{prefix}:coedge:{profile_index}:{index}:top-cap")))
                .collect::<Vec<_>>();
            ir.model.loops.push(IrLoop {
                id: bottom_loop.clone(),
                face: bottom_face.clone(),
                boundary_role: if profile_index == 0 {
                    cadmpeg_ir::topology::LoopBoundaryRole::Outer
                } else {
                    cadmpeg_ir::topology::LoopBoundaryRole::Inner
                },
                coedges: bottom_coedges.clone(),
                vertex_uses: Vec::new(),
            });
            ir.model.loops.push(IrLoop {
                id: top_loop.clone(),
                face: top_face.clone(),
                boundary_role: if profile_index == 0 {
                    cadmpeg_ir::topology::LoopBoundaryRole::Outer
                } else {
                    cadmpeg_ir::topology::LoopBoundaryRole::Inner
                },
                coedges: top_coedges.clone(),
                vertex_uses: Vec::new(),
            });
            for ring_index in 0..count {
                let edge_index = count - 1 - ring_index;
                let id = bottom_coedges[ring_index].clone();
                let (geometry, reversed, start, end) = &profile[edge_index];
                let bottom_pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    PcurveId(format!(
                        "{prefix}:pcurve:{profile_index}:{edge_index}:bottom-cap"
                    )),
                    transform.offset,
                    extrusion_cap_pcurve(geometry, *reversed, *start, *end),
                );
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: bottom_loop.clone(),
                    edge: bottom_edges[edge_index].clone(),
                    next: bottom_coedges[(ring_index + 1) % count].clone(),
                    previous: bottom_coedges[(ring_index + count - 1) % count].clone(),
                    radial_next: CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{edge_index}:side-bottom"
                    )),
                    sense: Sense::Reversed,
                    pcurves: vec![PcurveUse {
                        pcurve: bottom_pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
                let id = top_coedges[ring_index].clone();
                let (geometry, reversed, start, end) = &profile[ring_index];
                let top_pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    PcurveId(format!(
                        "{prefix}:pcurve:{profile_index}:{ring_index}:top-cap"
                    )),
                    transform.offset,
                    extrusion_cap_pcurve(geometry, *reversed, *start, *end),
                );
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: top_loop.clone(),
                    edge: top_edges[ring_index].clone(),
                    next: top_coedges[(ring_index + 1) % count].clone(),
                    previous: top_coedges[(ring_index + count - 1) % count].clone(),
                    radial_next: CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{ring_index}:side-top"
                    )),
                    sense: Sense::Forward,
                    pcurves: vec![PcurveUse {
                        pcurve: top_pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
            }

            let forward_sides = extrusion_profile_signed_area(profile)
                .expect("validated extrusion profile has nonzero area")
                > 0.0;
            for (index, (geometry, _, start, _)) in profile.iter().enumerate() {
                let next = (index + 1) % count;
                let surface_id =
                    SurfaceId(format!("{prefix}:surface:{profile_index}:side:{index}"));
                let section_geometry = match geometry {
                    SketchGeometry::Line { .. } => SketchGeometry::Line {
                        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
                        end: cadmpeg_ir::math::Point2::new(
                            profile[index].3[0],
                            profile[index].3[1],
                        ),
                    },
                    value => value.clone(),
                };
                let Some(surface_geometry) =
                    extruded_geometry_surface(transform, &section_geometry)
                else {
                    break;
                };
                ir.model.surfaces.push(Surface {
                    id: surface_id.clone(),
                    geometry: surface_geometry,
                    source_object: None,
                });
                let face_id = FaceId(format!("{prefix}:face:{profile_index}:side:{index}"));
                let loop_id = LoopId(format!("{prefix}:loop:{profile_index}:side:{index}"));
                let coedges = [
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{index}:side-bottom"
                    )),
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{next}:side-vertical-out"
                    )),
                    CoedgeId(format!("{prefix}:coedge:{profile_index}:{index}:side-top")),
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{index}:side-vertical-in"
                    )),
                ];
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face_id.clone(),
                    boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Outer,
                    coedges: coedges.to_vec(),
                    vertex_uses: Vec::new(),
                });
                let edge_uses = [
                    (bottom_edges[index].clone(), Sense::Forward),
                    (vertical_edges[next].clone(), Sense::Forward),
                    (top_edges[index].clone(), Sense::Reversed),
                    (vertical_edges[index].clone(), Sense::Reversed),
                ];
                let side_uvs =
                    extrusion_side_uvs(geometry, profile[index].1, *start, profile[index].3, span);
                for use_index in 0..4 {
                    let radial_next = match use_index {
                        0 => bottom_coedges[count - 1 - index].clone(),
                        1 => CoedgeId(format!(
                            "{prefix}:coedge:{profile_index}:{next}:side-vertical-in"
                        )),
                        2 => top_coedges[index].clone(),
                        3 => CoedgeId(format!(
                            "{prefix}:coedge:{profile_index}:{index}:side-vertical-out"
                        )),
                        _ => unreachable!(),
                    };
                    let pcurve = add_extrusion_pcurve(
                        ir,
                        annotations,
                        PcurveId(format!(
                            "{prefix}:pcurve:{profile_index}:{index}:side:{use_index}"
                        )),
                        transform.offset,
                        line_pcurve(side_uvs[use_index][0], side_uvs[use_index][1]),
                    );
                    ir.model.coedges.push(Coedge {
                        id: coedges[use_index].clone(),
                        owner_loop: loop_id.clone(),
                        edge: edge_uses[use_index].0.clone(),
                        next: coedges[(use_index + 1) % 4].clone(),
                        previous: coedges[(use_index + 3) % 4].clone(),
                        radial_next,
                        sense: edge_uses[use_index].1,
                        pcurves: vec![PcurveUse {
                            pcurve,
                            isoparametric: None,
                            parameter_range: None,
                        }],
                        use_curve: None,
                        use_curve_parameter_range: None,
                    });
                }
                ir.model.faces.push(Face {
                    id: face_id.clone(),
                    shell: shell_id.clone(),
                    surface: surface_id,
                    sense: if forward_sides {
                        Sense::Forward
                    } else {
                        Sense::Reversed
                    },
                    loops: vec![loop_id],
                    name: None,
                    color: None,
                    tolerance: None,
                });
                shell_faces.push(face_id);
            }
        }
        ir.model.faces.push(Face {
            id: bottom_face,
            shell: shell_id.clone(),
            surface: bottom_surface,
            sense: if forward_caps {
                Sense::Reversed
            } else {
                Sense::Forward
            },
            loops: bottom_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.faces.push(Face {
            id: top_face,
            shell: shell_id.clone(),
            surface: top_surface,
            sense: if forward_caps {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            loops: top_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: shell_faces,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        ir.model.bodies.push(Body {
            id: body_id,
            kind: BodyKind::Solid,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        transferred += 1;
    }
    transferred
}

fn feature_recipe(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipeKind> {
    current_feature_recipe(&scan.features.operations, feature_id)
        .map(crate::feature::FeatureRecipe::kind)
}

fn feature_recipe_effect(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipeEffect> {
    current_feature_recipe(&scan.features.operations, feature_id)
        .map(crate::feature::FeatureRecipe::effect)
}

fn current_additive_feature_recipe(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipeKind> {
    let recipe = current_feature_recipe(operations, feature_id)?;
    (recipe.effect() == crate::feature::FeatureRecipeEffect::Protrude).then(|| recipe.kind())
}

fn feature_is_first_material_operation(scan: &ContainerScan, feature_id: u32) -> bool {
    let Some(target) = current_feature_operation(&scan.features.operations, feature_id) else {
        return false;
    };
    scan.features
        .operations
        .iter()
        .map(|operation| operation.feature_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|candidate| *candidate != feature_id)
        .filter_map(|candidate| {
            let operation = current_feature_operation(&scan.features.operations, candidate)?;
            let recipe_is_material = operation.recipe.is_some_and(|recipe| {
                matches!(
                    recipe.effect(),
                    crate::feature::FeatureRecipeEffect::Protrude
                        | crate::feature::FeatureRecipeEffect::Cut
                )
            });
            (recipe_is_material || matches!(feature_schema_class(scan, candidate), Some(916 | 917)))
                .then_some(operation)
        })
        .all(|operation| operation.state_offset > target.state_offset)
}

fn current_feature_recipe(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipe> {
    current_feature_operation(operations, feature_id)?.recipe
}

fn current_feature_recipe_parent(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<u32> {
    let operation = current_feature_operation(operations, feature_id)?;
    operation.recipe?;
    operation.parent_feature_id
}

fn current_feature_operation(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<&crate::feature::FeatureOperation> {
    let mut matches = operations
        .iter()
        .filter(|operation| operation.feature_id == feature_id);
    let operation = matches.next()?;
    matches.next().is_none().then_some(operation)
}

fn feature_schema_class(scan: &ContainerScan, feature_id: u32) -> Option<u32> {
    resolved_feature_schema_class_from_classes(
        &scan.features.operations,
        feature_row_schema_classes(scan, feature_id),
        feature_id,
    )
}

fn resolved_feature_schema_class_from_classes(
    operations: &[crate::feature::FeatureOperation],
    classes: BTreeSet<u32>,
    feature_id: u32,
) -> Option<u32> {
    if let Some(schema_class) = current_feature_operation(operations, feature_id)
        .and_then(|operation| operation.root_schema_class)
    {
        return Some(schema_class);
    }
    if !classes.is_empty() {
        let mut classes = classes.into_iter();
        let schema_class = classes.next()?;
        return classes.next().is_none().then_some(schema_class);
    }
    None
}

fn feature_row_schema_classes(scan: &ContainerScan, feature_id: u32) -> BTreeSet<u32> {
    row_feature_schema_classes(&scan.features.rows, feature_id)
        .into_iter()
        .chain(row_feature_schema_classes(
            &scan.features.depdb_recipe_rows,
            feature_id,
        ))
        .collect()
}

fn row_feature_schema_classes(
    rows: &[crate::feature::FeatureRow],
    feature_id: u32,
) -> BTreeSet<u32> {
    rows.iter()
        .filter(|row| row.feature_id == feature_id)
        .filter_map(|row| row.root_schema_class)
        .collect()
}

fn feature_revolution_extent(scan: &ContainerScan, feature_id: u32) -> Option<RevolveExtent> {
    unique_feature_revolution_extent_kind(&scan.features.revolution_extents, feature_id).map(
        |kind| match kind {
            crate::feature::FeatureRevolutionExtentKind::FullTurn => RevolveExtent::OneSided {
                termination: Termination::Angle {
                    angle: Angle(std::f64::consts::TAU),
                },
            },
        },
    )
}

fn unique_feature_revolution_extent_kind(
    records: &[crate::feature::FeatureRevolutionExtent],
    feature_id: u32,
) -> Option<crate::feature::FeatureRevolutionExtentKind> {
    let mut kinds = records
        .iter()
        .filter(|record| record.feature_id == feature_id)
        .map(|record| record.kind);
    let kind = kinds.next()?;
    kinds.all(|candidate| candidate == kind).then_some(kind)
}

fn line_orientation_definition(
    segment: &crate::feature::FeatureSegment,
    entity: SketchEntityId,
) -> Option<SketchConstraintDefinition> {
    if segment.kind != crate::feature::FeatureSegmentKind::Line {
        return None;
    }
    match segment.vertical_horizontal {
        Some(0) => Some(SketchConstraintDefinition::Vertical { entity }),
        Some(1) => Some(SketchConstraintDefinition::Horizontal { entity }),
        _ => None,
    }
}

fn reconcile_constraint_entity_references(
    definition: &mut SketchConstraintDefinition,
    emitted: &BTreeSet<SketchEntityId>,
) -> bool {
    let locus_emitted = |locus: &SketchLocus| match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => emitted.contains(entity),
    };
    match definition {
        SketchConstraintDefinition::Native { entities, .. } => {
            entities.retain(|entity| emitted.contains(entity));
            true
        }
        SketchConstraintDefinition::Coincident { entities }
        | SketchConstraintDefinition::Distance { entities, .. } => {
            entities.iter().all(|entity| emitted.contains(entity))
        }
        SketchConstraintDefinition::CoincidentLoci { loci } => loci.iter().all(locus_emitted),
        SketchConstraintDefinition::SameCoordinate { first, second, .. }
        | SketchConstraintDefinition::TangentLoci { first, second }
        | SketchConstraintDefinition::DistanceLoci { first, second, .. }
        | SketchConstraintDefinition::HorizontalDistance { first, second, .. }
        | SketchConstraintDefinition::VerticalDistance { first, second, .. } => {
            locus_emitted(first) && locus_emitted(second)
        }
        SketchConstraintDefinition::Midpoint { point, entity } => {
            locus_emitted(point) && emitted.contains(entity)
        }
        SketchConstraintDefinition::AtIntersection {
            point,
            first,
            second,
        } => locus_emitted(point) && emitted.contains(first) && emitted.contains(second),
        SketchConstraintDefinition::PointOnObject { point, entity } => {
            locus_emitted(point) && emitted.contains(entity)
        }
        SketchConstraintDefinition::Symmetric {
            first,
            second,
            axis,
        } => locus_emitted(first) && locus_emitted(second) && emitted.contains(axis),
        SketchConstraintDefinition::PointSymmetric {
            first,
            second,
            center,
        } => locus_emitted(first) && locus_emitted(second) && locus_emitted(center),
        SketchConstraintDefinition::Concentric { first, second }
        | SketchConstraintDefinition::Coradial { first, second }
        | SketchConstraintDefinition::Collinear { first, second }
        | SketchConstraintDefinition::Parallel { first, second }
        | SketchConstraintDefinition::Perpendicular { first, second }
        | SketchConstraintDefinition::Tangent { first, second }
        | SketchConstraintDefinition::Equal { first, second }
        | SketchConstraintDefinition::Angle { first, second, .. } => {
            emitted.contains(first) && emitted.contains(second)
        }
        SketchConstraintDefinition::Horizontal { entity }
        | SketchConstraintDefinition::Vertical { entity }
        | SketchConstraintDefinition::Fixed { entity }
        | SketchConstraintDefinition::Radius { entity, .. }
        | SketchConstraintDefinition::Diameter { entity, .. } => emitted.contains(entity),
        SketchConstraintDefinition::HorizontalPoints { first, second }
        | SketchConstraintDefinition::VerticalPoints { first, second } => {
            locus_emitted(first) && locus_emitted(second)
        }
        SketchConstraintDefinition::ArcAngle { entity, .. }
        | SketchConstraintDefinition::EllipseAngle { entity, .. } => emitted.contains(entity),
        SketchConstraintDefinition::SnellsLaw {
            incident,
            refracted,
            interface,
            ..
        } => locus_emitted(incident) && locus_emitted(refracted) && emitted.contains(interface),
        SketchConstraintDefinition::Weight { entity, .. } => emitted.contains(entity),
        SketchConstraintDefinition::InternalAlignment { helper, parent, .. } => {
            emitted.contains(helper) && emitted.contains(parent)
        }
        SketchConstraintDefinition::Group { elements }
        | SketchConstraintDefinition::Text { elements, .. } => elements.iter().all(locus_emitted),
        SketchConstraintDefinition::Disabled => true,
        _ => true,
    }
}

fn reconcile_constraint_parameter_reference(
    definition: &mut SketchConstraintDefinition,
    emitted: &BTreeSet<ParameterId>,
) -> bool {
    match definition {
        SketchConstraintDefinition::Native { parameter, .. } => {
            if parameter
                .as_ref()
                .is_some_and(|parameter| !emitted.contains(parameter))
            {
                *parameter = None;
            }
            true
        }
        SketchConstraintDefinition::Distance { parameter, .. }
        | SketchConstraintDefinition::DistanceLoci { parameter, .. }
        | SketchConstraintDefinition::HorizontalDistance { parameter, .. }
        | SketchConstraintDefinition::VerticalDistance { parameter, .. }
        | SketchConstraintDefinition::Angle { parameter, .. }
        | SketchConstraintDefinition::Radius { parameter, .. }
        | SketchConstraintDefinition::Diameter { parameter, .. } => emitted.contains(parameter),
        SketchConstraintDefinition::SnellsLaw { parameter, .. }
        | SketchConstraintDefinition::Weight { parameter, .. } => emitted.contains(parameter),
        SketchConstraintDefinition::Coincident { .. }
        | SketchConstraintDefinition::CoincidentLoci { .. }
        | SketchConstraintDefinition::SameCoordinate { .. }
        | SketchConstraintDefinition::Midpoint { .. }
        | SketchConstraintDefinition::Concentric { .. }
        | SketchConstraintDefinition::Coradial { .. }
        | SketchConstraintDefinition::Collinear { .. }
        | SketchConstraintDefinition::Symmetric { .. }
        | SketchConstraintDefinition::PointSymmetric { .. }
        | SketchConstraintDefinition::Horizontal { .. }
        | SketchConstraintDefinition::Vertical { .. }
        | SketchConstraintDefinition::Parallel { .. }
        | SketchConstraintDefinition::Perpendicular { .. }
        | SketchConstraintDefinition::Tangent { .. }
        | SketchConstraintDefinition::TangentLoci { .. }
        | SketchConstraintDefinition::Equal { .. }
        | SketchConstraintDefinition::Fixed { .. } => true,
        SketchConstraintDefinition::Disabled
        | SketchConstraintDefinition::PointOnObject { .. }
        | SketchConstraintDefinition::AtIntersection { .. }
        | SketchConstraintDefinition::HorizontalPoints { .. }
        | SketchConstraintDefinition::VerticalPoints { .. }
        | SketchConstraintDefinition::ArcAngle { .. }
        | SketchConstraintDefinition::EllipseAngle { .. }
        | SketchConstraintDefinition::InternalAlignment { .. }
        | SketchConstraintDefinition::Group { .. }
        | SketchConstraintDefinition::Text { .. } => true,
        _ => true,
    }
}

fn close_sketch_constraint_parameter_references(ir: &mut CadIr) {
    let emitted = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.id.clone())
        .collect::<BTreeSet<_>>();
    ir.model.sketch_constraints.retain_mut(|constraint| {
        reconcile_constraint_parameter_reference(&mut constraint.definition, &emitted)
    });
}

fn joined_relation_incidence(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Option<&crate::feature::FeatureSkamp> {
    let Some(relations) = &definition.relations else {
        return None;
    };
    if !feature_solver_table_complete(relations.triples_header.as_ref(), relations.triples.len())
        || !feature_solver_table_complete(relations.skamp_header.as_ref(), relations.skamps.len())
    {
        return None;
    }
    let incidence_ids = relations
        .triples
        .iter()
        .filter(|triple| triple.relation_id == Some(relation_id))
        .filter_map(|triple| triple.skamp_id)
        .collect::<Vec<_>>();
    let [incidence_id] = incidence_ids.as_slice() else {
        return None;
    };
    let incidences = relations
        .skamps
        .iter()
        .filter(|skamp| skamp.id == *incidence_id)
        .collect::<Vec<_>>();
    let [incidence] = incidences.as_slice() else {
        return None;
    };
    Some(*incidence)
}

fn relation_incidence(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Option<&crate::feature::FeatureSkamp> {
    let incidence = joined_relation_incidence(definition, relation_id)?;
    section_skamp_active(incidence.status).then_some(incidence)
}

fn relation_incidence_entities(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Vec<SketchEntityId> {
    let Some(incidence) = relation_incidence(definition, relation_id) else {
        return Vec::new();
    };
    incidence
        .items
        .iter()
        .map(|item| sketch_entity_id(sketch, item.entity_id))
        .collect()
}

fn joined_relation_incidence_entities(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Vec<SketchEntityId> {
    let Some(incidence) = joined_relation_incidence(definition, relation_id) else {
        return Vec::new();
    };
    incidence
        .items
        .iter()
        .map(|item| sketch_entity_id(sketch, item.entity_id))
        .collect()
}

fn relation_incidence_loci(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Option<[SketchLocus; 2]> {
    let incidence = relation_incidence(definition, relation_id)?;
    let [first, second] = incidence.items.as_slice() else {
        return None;
    };
    Some([
        section_skamp_locus(definition, sketch, first)?,
        section_skamp_locus(definition, sketch, second)?,
    ])
}

fn section_angular_entities(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    segments: &[crate::feature::FeatureSegment],
    vectors: [[Option<u32>; 4]; 3],
    known_entities: &BTreeSet<u32>,
) -> Option<[SketchEntityId; 2]> {
    let [Some(first_internal), Some(second_internal), None, Some(1)] = vectors[0] else {
        return None;
    };
    let order_table = definition.order_table.as_ref()?;
    let external_id = |internal_id| {
        let external_id = order_table.external_id(internal_id)?;
        let matching_segments = segments
            .iter()
            .filter(|segment| {
                segment.external_id == external_id
                    && segment.kind == crate::feature::FeatureSegmentKind::Line
            })
            .collect::<Vec<_>>();
        (known_entities.contains(&external_id) && matching_segments.len() == 1)
            .then_some(external_id)
    };
    let [first, second] = [first_internal, second_internal].map(external_id);
    let [Some(first), Some(second)] = [first, second] else {
        return None;
    };
    (first != second)
        .then(|| [first, second].map(|external_id| sketch_entity_id(sketch, external_id)))
}

fn section_circle_size_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return Vec::new();
    };
    definition
        .segments
        .iter()
        .flat_map(|segments| &segments.opaque_rows)
        .filter(|segment| segment.kind == 10)
        .filter(|segment| {
            unique_opaque_section_segment(definition, segment.external_id, 10)
                .is_some_and(|candidate| candidate == *segment)
        })
        .filter_map(|segment| Some((segment, usize::try_from(segment.radius_ref?).ok()?)))
        .filter_map(|(circle, ordinal)| {
            let (dimension, parameter) =
                resolved_feature_dimension_parameter(sketch, dimensions, ordinal)?;
            let kind = match dimension.dimension_type {
                3 => "radius",
                4 => "diameter",
                _ => return None,
            };
            Some((
                SketchConstraint {
                    id: sketch_constraint_id(sketch, format_args!("{kind}:{}", circle.external_id)),
                    sketch: sketch.clone(),
                    definition: circular_dimension_constraint(
                        sketch_entity_id(sketch, circle.external_id),
                        parameter,
                        dimension.dimension_type,
                    ),
                    name: None,
                    driving: None,
                    active: None,
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(sketch_native_ref(sketch)),
                },
                dimension.offset,
            ))
        })
        .collect()
}

fn circular_dimension_constraint(
    entity: SketchEntityId,
    parameter: ParameterId,
    dimension_type: u32,
) -> SketchConstraintDefinition {
    if dimension_type == 4 {
        SketchConstraintDefinition::Diameter { entity, parameter }
    } else {
        SketchConstraintDefinition::Radius { entity, parameter }
    }
}

fn section_dimension_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let Some(relations) = &definition.relations else {
        return Vec::new();
    };
    let segments = section_segment_rows(definition);
    let known_entities = section_entity_external_ids(definition);
    relations
        .rows
        .iter()
        .map(|relation| {
            let unique_relation_id = feature_relation_table_complete(relations)
                && relations
                    .rows
                    .iter()
                    .filter(|candidate| candidate.relation_id == relation.relation_id)
                    .count()
                    == 1;
            let dimension = definition.dimensions.as_ref().and_then(|dimensions| {
                resolved_feature_dimension_parameter(
                    sketch,
                    dimensions,
                    usize::try_from(relation.dimension_id).ok()?,
                )
            });
            let parameter = dimension.as_ref().map(|(_, parameter)| parameter.clone());
            let joined_incidence = unique_relation_id
                .then(|| joined_relation_incidence(definition, relation.relation_id))
                .flatten();
            let typed = (|| {
                unique_relation_id.then_some(())?;
                let (dimension, _) = dimension.as_ref()?;
                let parameter = parameter.clone()?;
                if relation.relation_type == 1
                    && dimension.value_unit == crate::feature::DimensionUnit::Radians
                {
                    let [first, second] = section_angular_entities(
                        definition,
                        sketch,
                        segments,
                        relation.operand_vectors?,
                        &known_entities,
                    )?;
                    return Some(SketchConstraintDefinition::Angle {
                        first,
                        second,
                        parameter,
                    });
                }
                if dimension.value_unit != crate::feature::DimensionUnit::Millimeters {
                    return None;
                }
                if relation.relation_type == 5
                    && relation.sign == 1
                    && matches!(dimension.dimension_type, 3 | 4)
                {
                    let vectors = relation.operand_vectors?;
                    let [Some(first_point), Some(0), Some(second_point), Some(0)] = vectors[0]
                    else {
                        return None;
                    };
                    let [Some(center), Some(10), Some(0), Some(1)] = vectors[1] else {
                        return None;
                    };
                    if vectors[2] != [Some(16), Some(15), Some(0), Some(0)] {
                        return None;
                    }
                    let matching = segments
                        .iter()
                        .filter(|segment| {
                            segment.kind == crate::feature::FeatureSegmentKind::Arc
                                && segment.radius_ref == Some(relation.dimension_id)
                                && segment.center_id == Some(center)
                                && (segment.point_ids == [first_point, second_point]
                                    || segment.point_ids == [second_point, first_point])
                        })
                        .collect::<Vec<_>>();
                    let [segment] = matching.as_slice() else {
                        return None;
                    };
                    known_entities
                        .contains(&segment.external_id)
                        .then_some(())?;
                    return Some(circular_dimension_constraint(
                        sketch_entity_id(sketch, segment.external_id),
                        parameter,
                        dimension.dimension_type,
                    ));
                }
                if relation.relation_type == 14
                    && relation.sign == 1
                    && matches!(dimension.dimension_type, 3 | 4)
                    && relation.operand_vectors?[1] == [Some(0); 4]
                    && relation.operand_vectors?[2] == [Some(15), Some(0), Some(0), Some(0)]
                {
                    let vectors = relation.operand_vectors?;
                    let [Some(radius_id), Some(0), Some(0), Some(0)] = vectors[0] else {
                        return None;
                    };
                    let matching = segments
                        .iter()
                        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
                        .map(|segment| (segment.external_id, segment.radius_ref))
                        .chain(
                            definition
                                .segments
                                .iter()
                                .flat_map(|table| &table.opaque_rows)
                                .filter(|segment| segment.kind == 10)
                                .map(|segment| (segment.external_id, segment.radius_ref)),
                        )
                        .filter(|(_, radius_ref)| *radius_ref == Some(radius_id))
                        .collect::<Vec<_>>();
                    let [(external_id, _)] = matching.as_slice() else {
                        return None;
                    };
                    known_entities.contains(external_id).then_some(())?;
                    return Some(circular_dimension_constraint(
                        sketch_entity_id(sketch, *external_id),
                        parameter,
                        dimension.dimension_type,
                    ));
                }
                if relation.relation_type != 0 || !matches!(relation.sign, 0 | 1 | 0xf6) {
                    return None;
                }
                if let Some(vectors) = relation.operand_vectors {
                    if section_linear_distance_vectors(vectors) {
                        if let [Some(first_id), Some(second_id), _, _] = vectors[0] {
                            let matching = segments
                                .iter()
                                .filter(|segment| {
                                    segment.point_ids == [first_id, second_id]
                                        || segment.point_ids == [second_id, first_id]
                                })
                                .collect::<Vec<_>>();
                            if let [measured] = matching.as_slice() {
                                if known_entities.contains(&measured.external_id) {
                                    let entity = sketch_entity_id(sketch, measured.external_id);
                                    let [first, second] =
                                        if measured.point_ids == [first_id, second_id] {
                                            [
                                                SketchLocus::Start(entity.clone()),
                                                SketchLocus::End(entity),
                                            ]
                                        } else {
                                            [
                                                SketchLocus::End(entity.clone()),
                                                SketchLocus::Start(entity),
                                            ]
                                        };
                                    match section_line_fixed_coordinate(definition, measured) {
                                        Some(0) => {
                                            return Some(
                                                SketchConstraintDefinition::VerticalDistance {
                                                    first,
                                                    second,
                                                    parameter,
                                                },
                                            );
                                        }
                                        Some(1) => {
                                            return Some(
                                                SketchConstraintDefinition::HorizontalDistance {
                                                    first,
                                                    second,
                                                    parameter,
                                                },
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            let points = resolved_section_points(definition);
                            if let (Some(first_point), Some(second_point)) =
                                (points.get(&first_id), points.get(&second_id))
                            {
                                let scale = first_point
                                    .iter()
                                    .chain(second_point)
                                    .map(|coordinate| coordinate.abs())
                                    .fold(1.0, f64::max);
                                let same_u =
                                    (first_point[0] - second_point[0]).abs() <= 1e-9 * scale;
                                let same_v =
                                    (first_point[1] - second_point[1]).abs() <= 1e-9 * scale;
                                if same_u != same_v {
                                    if let (Some(first), Some(second)) = (
                                        section_point_locus(definition, sketch, first_id),
                                        section_point_locus(definition, sketch, second_id),
                                    ) {
                                        return Some(if same_u {
                                            SketchConstraintDefinition::VerticalDistance {
                                                first,
                                                second,
                                                parameter,
                                            }
                                        } else {
                                            SketchConstraintDefinition::HorizontalDistance {
                                                first,
                                                second,
                                                parameter,
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some([first, second]) =
                    relation_incidence_loci(definition, sketch, relation.relation_id)
                {
                    return Some(SketchConstraintDefinition::DistanceLoci {
                        first,
                        second,
                        parameter,
                    });
                }
                if let Some(incidence) =
                    joined_incidence.filter(|incidence| !section_skamp_active(incidence.status))
                {
                    if let [first, second] = incidence.items.as_slice() {
                        if let (Some(first), Some(second)) = (
                            section_skamp_locus(definition, sketch, first),
                            section_skamp_locus(definition, sketch, second),
                        ) {
                            return Some(SketchConstraintDefinition::DistanceLoci {
                                first,
                                second,
                                parameter,
                            });
                        }
                    }
                    if !incidence.items.is_empty() {
                        return Some(SketchConstraintDefinition::Distance {
                            entities: incidence
                                .items
                                .iter()
                                .map(|item| sketch_entity_id(sketch, item.entity_id))
                                .collect(),
                            parameter,
                        });
                    }
                }
                let entities =
                    relation_incidence_entities(definition, sketch, relation.relation_id);
                (!entities.is_empty()).then_some(SketchConstraintDefinition::Distance {
                    entities,
                    parameter,
                })
            })();
            let incidence_entities = if unique_relation_id {
                joined_relation_incidence_entities(definition, sketch, relation.relation_id)
            } else {
                Vec::new()
            };
            let active = joined_incidence.map(|incidence| section_skamp_active(incidence.status));
            let constraint_definition =
                typed.unwrap_or_else(|| SketchConstraintDefinition::Native {
                    native_kind: format!("creo:relation:{}", relation.relation_type),
                    native_state: None,
                    entities: incidence_entities,
                    parameter,
                    operands: vec![SketchNativeOperand {
                        native_kind: "relat_ptr".to_string(),
                        native_field: None,
                        native_role: None,
                        object_index: relation.relation_id,
                        native_ref: Some(sketch_native_ref(sketch)),
                    }],
                });
            (
                SketchConstraint {
                    id: if unique_relation_id {
                        sketch_constraint_id(
                            sketch,
                            format_args!("relation:{}", relation.relation_id),
                        )
                    } else {
                        sketch_constraint_id(
                            sketch,
                            format_args!("relation:offset:{}", relation.offset),
                        )
                    },
                    sketch: sketch.clone(),
                    definition: constraint_definition,
                    name: None,
                    driving: None,
                    active,
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(sketch_native_ref(sketch)),
                },
                relation.offset,
            )
        })
        .collect()
}

fn section_linear_distance_vectors(vectors: [[Option<u32>; 4]; 3]) -> bool {
    vectors[0][2..] == [None, Some(1)]
        && matches!(
            vectors[1],
            [Some(0), Some(0), Some(0), Some(0)] | [Some(1), Some(1), Some(0), Some(1)]
        )
        && vectors[2] == [Some(15), Some(16), Some(15), Some(1)]
}

fn section_point_locus(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    point_id: u32,
) -> Option<SketchLocus> {
    let unique_entities = unique_section_segment_external_ids(definition);
    definition
        .segments
        .as_ref()?
        .rows
        .iter()
        .filter(|segment| unique_entities.contains(&segment.external_id))
        .filter_map(|segment| {
            let entity = sketch_entity_id(sketch, segment.external_id);
            let locus = match segment.kind {
                crate::feature::FeatureSegmentKind::Point
                    if segment.point_ids[0] == point_id && segment.point_ids[1] == point_id =>
                {
                    SketchLocus::Entity(entity)
                }
                crate::feature::FeatureSegmentKind::Line if segment.point_ids[0] == point_id => {
                    SketchLocus::Start(entity)
                }
                crate::feature::FeatureSegmentKind::Line if segment.point_ids[1] == point_id => {
                    SketchLocus::End(entity)
                }
                crate::feature::FeatureSegmentKind::Arc if segment.point_ids[0] == point_id => {
                    SketchLocus::End(entity)
                }
                crate::feature::FeatureSegmentKind::Arc if segment.point_ids[1] == point_id => {
                    SketchLocus::Start(entity)
                }
                _ => return None,
            };
            Some((segment.offset, locus))
        })
        .min_by_key(|(offset, _)| *offset)
        .map(|(_, locus)| locus)
}

fn unique_opaque_section_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
    kind: u32,
) -> Option<&crate::feature::FeatureOpaqueSegment> {
    let mut matches = definition.segments.iter().flat_map(|segments| {
        segments
            .opaque_rows
            .iter()
            .filter(|segment| segment.external_id == external_id)
    });
    let segment = matches.next()?;
    (segment.kind == kind
        && matches.next().is_none()
        && !definition
            .segments
            .iter()
            .flat_map(|segments| &segments.rows)
            .any(|segment| segment.external_id == external_id))
    .then_some(segment)
}

fn section_skamp_locus(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    let entity = sketch_entity_id(sketch, item.entity_id);
    if let Some(family) = solver_only_section_entity_family(definition, item.entity_id) {
        return section_entity_family_locus(family, entity, item.sense);
    }
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        return match (segment.kind, item.sense) {
            (_, 0) => Some(SketchLocus::Entity(entity)),
            (crate::feature::FeatureSegmentKind::Arc, 2) => Some(SketchLocus::End(entity)),
            (crate::feature::FeatureSegmentKind::Arc, 3) => Some(SketchLocus::Start(entity)),
            (crate::feature::FeatureSegmentKind::Arc, 4) => Some(SketchLocus::Center(entity)),
            (_, 2) => Some(SketchLocus::Start(entity)),
            (_, 3) => Some(SketchLocus::End(entity)),
            _ => None,
        };
    }
    if unique_opaque_section_segment(definition, item.entity_id, 1).is_some() {
        return matches!(item.sense, 0 | 4).then_some(SketchLocus::Entity(entity));
    }
    if unique_opaque_section_segment(definition, item.entity_id, 47).is_some() {
        return match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            2 => Some(SketchLocus::Start(entity)),
            3 => Some(SketchLocus::End(entity)),
            _ => None,
        };
    }
    if unique_opaque_section_segment(definition, item.entity_id, 10).is_some() {
        return match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            4 => Some(SketchLocus::Center(entity)),
            _ => None,
        };
    }
    if definition
        .segments
        .iter()
        .flat_map(|segments| {
            segments
                .rows
                .iter()
                .map(|segment| segment.external_id)
                .chain(
                    segments
                        .opaque_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
        })
        .any(|external_id| external_id == item.entity_id)
    {
        return section_incidence_curve_locus(definition, entity, item);
    }
    let saved = section_saved_entity(definition, item.entity_id)?;
    match (saved, item.sense) {
        (_, 0) => Some(SketchLocus::Entity(entity)),
        (crate::feature::FeatureSavedEntity::Line(_), 2) => Some(SketchLocus::Start(entity)),
        (crate::feature::FeatureSavedEntity::Line(_), 3) => Some(SketchLocus::End(entity)),
        (crate::feature::FeatureSavedEntity::Arc(_), 2) => Some(SketchLocus::End(entity)),
        (crate::feature::FeatureSavedEntity::Arc(_), 3) => Some(SketchLocus::Start(entity)),
        (
            crate::feature::FeatureSavedEntity::Arc(_)
            | crate::feature::FeatureSavedEntity::Circle(_),
            4,
        ) => Some(SketchLocus::Center(entity)),
        _ => None,
    }
}

fn section_incidence_curve_locus(
    definition: &crate::feature::FeatureDefinition,
    entity: SketchEntityId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    section_entity_family_locus(
        unique_section_incidence_curve_family(definition, item.entity_id)?,
        entity,
        item.sense,
    )
}

fn section_entity_family_locus(
    family: SectionEntityIncidenceFamily,
    entity: SketchEntityId,
    sense: u32,
) -> Option<SketchLocus> {
    match (family, sense) {
        (SectionEntityIncidenceFamily::Point, 0) => Some(SketchLocus::Entity(entity)),
        (
            SectionEntityIncidenceFamily::BoundedCurve
            | SectionEntityIncidenceFamily::Line
            | SectionEntityIncidenceFamily::Arc,
            0,
        ) => Some(SketchLocus::Entity(entity)),
        (
            SectionEntityIncidenceFamily::BoundedCurve
            | SectionEntityIncidenceFamily::Line
            | SectionEntityIncidenceFamily::Arc,
            2,
        ) => Some(SketchLocus::Start(entity)),
        (
            SectionEntityIncidenceFamily::BoundedCurve
            | SectionEntityIncidenceFamily::Line
            | SectionEntityIncidenceFamily::Arc,
            3,
        ) => Some(SketchLocus::End(entity)),
        (SectionEntityIncidenceFamily::Arc | SectionEntityIncidenceFamily::Circular, 4) => {
            Some(SketchLocus::Center(entity))
        }
        (SectionEntityIncidenceFamily::Circular, 0) => Some(SketchLocus::Entity(entity)),
        _ => None,
    }
}

fn section_skamp_endpoint(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    matches!(item.sense, 2 | 3)
        .then(|| section_skamp_locus(definition, sketch, item))
        .flatten()
}

fn section_skamp_point_locus(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    if item.sense == 0 && section_skamp_is_point(definition, item) {
        return section_skamp_locus(definition, sketch, item);
    }
    matches!(item.sense, 2..=4)
        .then(|| section_skamp_locus(definition, sketch, item))
        .flatten()
}

fn section_skamp_incidence_locus(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Option<SketchLocus> {
    section_skamp_point_locus(definition, sketch, item).or_else(|| {
        let entity = sketch_entity_id(sketch, item.entity_id);
        let locus = match item.sense {
            2 => SketchLocus::Start(entity.clone()),
            3 => SketchLocus::End(entity.clone()),
            _ => return None,
        };
        geometry?
            .get(&entity)
            .is_some_and(|geometry| matches!(geometry, SketchGeometry::Native { .. }))
            .then_some(locus)
    })
}

fn section_skamp_line_pair(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    first: &crate::feature::FeatureSkampItem,
    second: &crate::feature::FeatureSkampItem,
) -> Option<[SketchEntityId; 2]> {
    if first.sense != 0
        || second.sense != 0
        || !section_skamp_is_line(definition, first)
        || !section_skamp_is_line(definition, second)
    {
        return None;
    }
    Some([first, second].map(|item| sketch_entity_id(sketch, item.entity_id)))
}

fn section_skamp_oriented_line(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Option<SketchEntityId> {
    (item.sense == 0).then_some(())?;
    let entity = sketch_entity_id(sketch, item.entity_id);
    if section_skamp_is_line(definition, item) {
        return Some(entity);
    }
    let line_role_evidence = active_complete_section_skamps(definition).any(|skamp| {
        skamp.items.iter().any(|candidate| {
            candidate.entity_id == item.entity_id && matches!(candidate.sense, 2 | 3)
        }) || match (skamp.kind, skamp.items.as_slice()) {
            (35, [first, second]) => {
                (first.entity_id == item.entity_id
                    && first.sense == 0
                    && matches!(second.sense, 2..=4))
                    || (second.entity_id == item.entity_id
                        && second.sense == 0
                        && matches!(first.sense, 2..=4))
            }
            _ => false,
        }
    });
    (line_role_evidence
        && geometry?
            .get(&entity)
            .is_some_and(|geometry| matches!(geometry, SketchGeometry::Native { .. })))
    .then_some(entity)
}

fn section_skamp_same_coordinate(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    skamp: &crate::feature::FeatureSkamp,
    require_satisfied: bool,
) -> Option<(SketchLocus, SketchLocus, SketchCoordinateAxis)> {
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let first_locus = section_skamp_point_locus(definition, sketch, first)?;
    let second_locus = section_skamp_point_locus(definition, sketch, second)?;
    let coordinate = section_skamp_same_coordinate_axis(skamp)?;
    let axis = [SketchCoordinateAxis::U, SketchCoordinateAxis::V][coordinate];
    if require_satisfied {
        let ([first_source, second_source], _) =
            section_skamp_same_coordinate_sources(definition, skamp)?;
        let points = resolved_section_points(definition);
        let point = |source| {
            Some(match source {
                SectionPointSource::Point(point_id) => *points.get(&point_id)?,
                SectionPointSource::Value(point) => point,
            })
        };
        if let (Some(first_point), Some(second_point)) = (point(first_source), point(second_source))
        {
            let scale = first_point
                .iter()
                .chain(&second_point)
                .map(|coordinate| coordinate.abs())
                .fold(1.0, f64::max);
            ((first_point[coordinate] - second_point[coordinate]).abs() <= 1e-9 * scale)
                .then_some(())?;
        }
    }
    Some((first_locus, second_locus, axis))
}

fn section_skamp_same_coordinate_sources(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<([SectionPointSource; 2], usize)> {
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let coordinate = section_skamp_same_coordinate_axis(skamp)?;
    Some((
        [
            section_skamp_selected_point(definition, first)?,
            section_skamp_selected_point(definition, second)?,
        ],
        coordinate,
    ))
}

fn section_skamp_same_coordinate_axis(skamp: &crate::feature::FeatureSkamp) -> Option<usize> {
    Some(match (skamp.kind, skamp.flags) {
        (17, 1) => 0,
        (17, 2) => 1,
        (30, _) => 1,
        (31, _) => 0,
        _ => return None,
    })
}

fn section_skamp_is_line(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    if solver_only_section_entity_family(definition, item.entity_id)
        == Some(SectionEntityIncidenceFamily::Line)
    {
        return true;
    }
    if unique_opaque_section_segment(definition, item.entity_id, 47).is_some() {
        return true;
    }
    let has_segment = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == item.entity_id);
    if has_segment {
        return unique_decoded_section_segment(definition, item.entity_id)
            .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line);
    }
    section_saved_entity(definition, item.entity_id)
        .is_some_and(|entity| matches!(entity, crate::feature::FeatureSavedEntity::Line(_)))
}

fn section_skamp_is_point(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    solver_only_section_entity_family(definition, item.entity_id)
        == Some(SectionEntityIncidenceFamily::Point)
        || unique_opaque_section_segment(definition, item.entity_id, 1).is_some()
        || unique_decoded_section_segment(definition, item.entity_id)
            .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Point)
}

fn section_skamp_is_arc(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    let has_segment = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == item.entity_id);
    if has_segment {
        return unique_decoded_section_segment(definition, item.entity_id)
            .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc);
    }
    section_saved_entity(definition, item.entity_id)
        .is_some_and(|entity| matches!(entity, crate::feature::FeatureSavedEntity::Arc(_)))
}

fn section_skamp_curve_entity(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchEntityId> {
    if item.sense != 0 {
        return None;
    }
    let is_curve = section_skamp_is_line(definition, item)
        || section_skamp_is_circular(definition, item)
        || section_saved_entity(definition, item.entity_id)
            .is_some_and(|entity| matches!(entity, crate::feature::FeatureSavedEntity::Spline(_)));
    is_curve.then(|| sketch_entity_id(sketch, item.entity_id))
}

fn section_skamp_midpoint(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    first: &crate::feature::FeatureSkampItem,
    second: &crate::feature::FeatureSkampItem,
) -> Option<(SketchLocus, SketchEntityId)> {
    let target = |item: &crate::feature::FeatureSkampItem| {
        (item.sense == 0
            && (section_skamp_is_line(definition, item) || section_skamp_is_arc(definition, item)))
        .then(|| sketch_entity_id(sketch, item.entity_id))
    };
    let point = |item| section_skamp_point_locus(definition, sketch, item);
    match (target(first), point(second), target(second), point(first)) {
        (Some(entity), Some(point), None, _) => Some((point, entity)),
        (None, _, Some(entity), Some(point)) => Some((point, entity)),
        _ => None,
    }
}

fn section_saved_entity(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureSavedEntity> {
    let internal_id = definition.order_table.as_ref()?.internal_id(external_id)?;
    let mut matches = definition
        .saved_section
        .as_ref()?
        .entities
        .iter()
        .filter(|entity| match entity {
            crate::feature::FeatureSavedEntity::Line(line) => line.entity_id == internal_id,
            crate::feature::FeatureSavedEntity::Arc(arc) => arc.entity_id == internal_id,
            crate::feature::FeatureSavedEntity::Circle(circle) => circle.entity_id == internal_id,
            crate::feature::FeatureSavedEntity::Spline(spline) => {
                spline.entity_id == Some(internal_id)
            }
            crate::feature::FeatureSavedEntity::Dummy(dummy) => {
                dummy.entity_id == Some(internal_id)
            }
        });
    let entity = matches.next()?;
    matches.next().is_none().then_some(entity)
}

fn section_skamp_circular_entity(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchEntityId> {
    if item.sense != 0 {
        return None;
    }
    section_skamp_is_circular(definition, item).then(|| sketch_entity_id(sketch, item.entity_id))
}

fn section_skamp_center_entity(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchEntityId> {
    (item.sense == 4 && section_skamp_is_circular(definition, item))
        .then(|| sketch_entity_id(sketch, item.entity_id))
}

fn section_skamp_is_circular(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    if solver_only_section_entities(definition).contains_key(&item.entity_id) {
        return solver_only_section_entity_family(definition, item.entity_id).is_some_and(
            |family| {
                matches!(
                    family,
                    SectionEntityIncidenceFamily::Arc | SectionEntityIncidenceFamily::Circular
                )
            },
        );
    }
    if matches!(
        unique_section_incidence_curve_family(definition, item.entity_id),
        Some(SectionEntityIncidenceFamily::Arc | SectionEntityIncidenceFamily::Circular)
    ) {
        return true;
    }
    if unique_opaque_section_segment(definition, item.entity_id, 10).is_some() {
        return true;
    }
    let has_segment = definition
        .segments
        .iter()
        .flat_map(|segments| &segments.rows)
        .any(|segment| segment.external_id == item.entity_id);
    if has_segment {
        unique_decoded_section_segment(definition, item.entity_id)
            .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
    } else {
        section_saved_entity(definition, item.entity_id).is_some_and(|entity| {
            matches!(
                entity,
                crate::feature::FeatureSavedEntity::Arc(_)
                    | crate::feature::FeatureSavedEntity::Circle(_)
            )
        })
    }
}

fn section_skamp_active(status: u32) -> bool {
    status & 1 != 0
}

fn active_complete_section_skamps(
    definition: &crate::feature::FeatureDefinition,
) -> impl Iterator<Item = &crate::feature::FeatureSkamp> {
    definition
        .relations
        .iter()
        .filter(|relations| feature_skamp_table_complete(relations))
        .flat_map(|relations| &relations.skamps)
        .filter(|skamp| section_skamp_active(skamp.status))
}

fn section_skamp_constraints_for_geometry(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Vec<(SketchConstraint, usize)> {
    let Some(relations) = &definition.relations else {
        return Vec::new();
    };
    let complete_skamps =
        feature_solver_table_complete(relations.skamp_header.as_ref(), relations.skamps.len());
    let skamp_id_counts =
        relations
            .skamps
            .iter()
            .fold(BTreeMap::<u32, usize>::new(), |mut counts, skamp| {
                *counts.entry(skamp.id).or_default() += 1;
                counts
            });
    let section_entities = section_entity_external_ids(definition);
    let available_entities = geometry.map_or_else(
        || section_entities.clone(),
        |geometry| {
            relations
                .skamps
                .iter()
                .flat_map(|skamp| &skamp.items)
                .map(|item| item.entity_id)
                .filter(|entity_id| geometry.contains_key(&sketch_entity_id(sketch, *entity_id)))
                .collect()
        },
    );
    relations
        .skamps
        .iter()
        .filter_map(|skamp| {
            let unique_skamp_id = complete_skamps && skamp_id_counts.get(&skamp.id) == Some(&1);
            let active = section_skamp_active(skamp.status);
            let native_constraint = || {
                let entities = skamp
                    .items
                    .iter()
                    .filter(|item| available_entities.contains(&item.entity_id))
                    .map(|item| sketch_entity_id(sketch, item.entity_id))
                    .collect::<Vec<_>>();
                Some(SketchConstraintDefinition::Native {
                    native_kind: format!("creo:skamp:{}", skamp.kind),
                    native_state: None,
                    entities,
                    parameter: None,
                    operands: skamp
                        .items
                        .iter()
                        .map(|item| SketchNativeOperand {
                            native_kind: format!("sense:{}", item.sense),
                            native_field: None,
                            native_role: None,
                            object_index: item.entity_id,
                            native_ref: None,
                        })
                        .collect(),
                })
            };
            let tangent_locus = |item| {
                if active {
                    section_skamp_endpoint(definition, sketch, item)
                } else if matches!(item.sense, 2 | 3) {
                    section_skamp_incidence_locus(definition, sketch, item, geometry)
                } else {
                    None
                }
            };
            let item_geometry = |item: &crate::feature::FeatureSkampItem| {
                let entity = sketch_entity_id(sketch, item.entity_id);
                geometry?.get(&entity)
            };
            let inactive_curve_entity = |item: &crate::feature::FeatureSkampItem| {
                (!active && item.sense == 0 && item_geometry(item).is_some_and(|geometry| {
                    matches!(
                        geometry,
                        SketchGeometry::Line { .. }
                            | SketchGeometry::ReferenceLine { .. }
                            | SketchGeometry::Circle { .. }
                            | SketchGeometry::Arc { .. }
                            | SketchGeometry::Nurbs { .. }
                    ) || matches!(
                        geometry,
                        SketchGeometry::Native { native_kind }
                            if matches!(native_kind.as_str(), "line" | "arc" | "circle" | "spline")
                    )
                }))
                .then(|| sketch_entity_id(sketch, item.entity_id))
            };
            let inactive_incidence_locus = |item: &crate::feature::FeatureSkampItem| {
                section_skamp_incidence_locus(definition, sketch, item, geometry).or_else(|| {
                    (!active
                        && item.sense == 4
                        && item_geometry(item).is_some_and(|geometry| {
                            matches!(
                                geometry,
                                SketchGeometry::Circle { .. } | SketchGeometry::Arc { .. }
                            ) || matches!(
                                geometry,
                                SketchGeometry::Native { native_kind }
                                    if matches!(native_kind.as_str(), "arc" | "circle")
                            )
                        }))
                    .then(|| SketchLocus::Center(sketch_entity_id(sketch, item.entity_id)))
                })
            };
            let inactive_point_entity = |item: &crate::feature::FeatureSkampItem| {
                (!active
                    && item.sense == 0
                    && item_geometry(item).is_some_and(|geometry| {
                        matches!(geometry, SketchGeometry::Point { .. })
                            || matches!(
                                geometry,
                                SketchGeometry::Native { native_kind } if native_kind == "point"
                            )
                    }))
                .then(|| sketch_entity_id(sketch, item.entity_id))
            };
            let inactive_point_locus = |item: &crate::feature::FeatureSkampItem| {
                section_skamp_point_locus(definition, sketch, item)
                    .or_else(|| inactive_point_entity(item).map(SketchLocus::Entity))
                    .or_else(|| inactive_incidence_locus(item))
            };
            let mut constraint_definition = if unique_skamp_id {
                match (skamp.kind, skamp.items.as_slice()) {
                    (0, [first, second])
                        if section_skamp_center_entity(definition, sketch, first).is_some()
                            && section_skamp_center_entity(definition, sketch, second)
                                .is_some() =>
                    {
                        SketchConstraintDefinition::Concentric {
                            first: section_skamp_center_entity(definition, sketch, first)?,
                            second: section_skamp_center_entity(definition, sketch, second)?,
                        }
                    }
                    (0, [first, second])
                        if section_skamp_incidence_locus(definition, sketch, first, geometry)
                            .is_some()
                            && section_skamp_incidence_locus(
                                definition, sketch, second, geometry,
                            )
                            .is_some() =>
                    {
                        SketchConstraintDefinition::CoincidentLoci {
                            loci: vec![
                                section_skamp_incidence_locus(definition, sketch, first, geometry)?,
                                section_skamp_incidence_locus(
                                    definition, sketch, second, geometry,
                                )?,
                            ],
                        }
                    }
                    (3, [first, second]) => {
                        let directed = [(first, second), (second, first)];
                        let point_on_curve = directed
                            .into_iter()
                            .filter_map(|(curve, point)| {
                                Some((
                                    section_skamp_curve_entity(definition, sketch, curve)
                                        .or_else(|| inactive_curve_entity(curve))?,
                                    inactive_incidence_locus(point)?,
                                ))
                            })
                            .collect::<Vec<_>>();
                        if let [(entity, point)] = point_on_curve.as_slice() {
                            SketchConstraintDefinition::PointOnObject {
                                point: point.clone(),
                                entity: entity.clone(),
                            }
                        } else {
                            let point_coincidence = directed
                                .into_iter()
                                .filter_map(|(point, locus)| {
                                    (point.sense == 0 && section_skamp_is_point(definition, point))
                                        .then(|| {
                                            Some([
                                                section_skamp_locus(definition, sketch, point)?,
                                                inactive_incidence_locus(locus)?,
                                            ])
                                        })
                                        .flatten()
                                })
                                .collect::<Vec<_>>();
                            if let [loci] = point_coincidence.as_slice() {
                                SketchConstraintDefinition::CoincidentLoci {
                                    loci: loci.to_vec(),
                                }
                            } else {
                                native_constraint()?
                            }
                        }
                    }
                    (kind @ (1 | 2), [item]) => {
                        match section_skamp_oriented_line(definition, sketch, item, geometry) {
                            Some(entity) if kind == 1 => {
                                SketchConstraintDefinition::Horizontal { entity }
                            }
                            Some(entity) => SketchConstraintDefinition::Vertical { entity },
                            None => native_constraint()?,
                        }
                    }
                    (4, [first, second])
                        if tangent_locus(first).is_some() && tangent_locus(second).is_some() =>
                    {
                        SketchConstraintDefinition::TangentLoci {
                            first: tangent_locus(first)?,
                            second: tangent_locus(second)?,
                        }
                    }
                    (4, [first, second])
                        if section_skamp_curve_entity(definition, sketch, first).is_some()
                            && section_skamp_curve_entity(definition, sketch, second).is_some() =>
                    {
                        SketchConstraintDefinition::Tangent {
                            first: section_skamp_curve_entity(definition, sketch, first)?,
                            second: section_skamp_curve_entity(definition, sketch, second)?,
                        }
                    }
                    (5, [first, second])
                        if section_skamp_line_pair(definition, sketch, first, second).is_some() =>
                    {
                        let [first, second] =
                            section_skamp_line_pair(definition, sketch, first, second)?;
                        SketchConstraintDefinition::Perpendicular { first, second }
                    }
                    (6, [first, second])
                        if section_skamp_circular_entity(definition, sketch, first).is_some()
                            && section_skamp_circular_entity(definition, sketch, second)
                                .is_some() =>
                    {
                        SketchConstraintDefinition::Equal {
                            first: section_skamp_circular_entity(definition, sketch, first)?,
                            second: section_skamp_circular_entity(definition, sketch, second)?,
                        }
                    }
                    (7, [first, second])
                        if section_skamp_line_pair(definition, sketch, first, second).is_some() =>
                    {
                        let [first, second] =
                            section_skamp_line_pair(definition, sketch, first, second)?;
                        SketchConstraintDefinition::Parallel { first, second }
                    }
                    (8, [first, second])
                        if section_skamp_line_pair(definition, sketch, first, second).is_some() =>
                    {
                        let [first, second] =
                            section_skamp_line_pair(definition, sketch, first, second)?;
                        SketchConstraintDefinition::Equal { first, second }
                    }
                    (9, [first, second])
                        if section_skamp_line_pair(definition, sketch, first, second).is_some() =>
                    {
                        let [first, second] =
                            section_skamp_line_pair(definition, sketch, first, second)?;
                        SketchConstraintDefinition::Collinear { first, second }
                    }
                    (9, [first, second])
                        if first.sense == 0
                            && second.sense == 0
                            && ((section_skamp_is_line(definition, first)
                                && section_skamp_is_point(definition, second))
                                || (section_skamp_is_point(definition, first)
                                    && section_skamp_is_line(definition, second))) =>
                    {
                        let (line, point) = if section_skamp_is_line(definition, first) {
                            (first, second)
                        } else {
                            (second, first)
                        };
                        SketchConstraintDefinition::PointOnObject {
                            point: section_skamp_locus(definition, sketch, point)?,
                            entity: sketch_entity_id(sketch, line.entity_id),
                        }
                    }
                    (14, [axis, first, second])
                        if axis.sense == 0
                            && section_skamp_is_line(definition, axis)
                            && section_skamp_point_locus(definition, sketch, first).is_some()
                            && section_skamp_point_locus(definition, sketch, second).is_some() =>
                    {
                        SketchConstraintDefinition::Symmetric {
                            first: section_skamp_point_locus(definition, sketch, first)?,
                            second: section_skamp_point_locus(definition, sketch, second)?,
                            axis: sketch_entity_id(sketch, axis.entity_id),
                        }
                    }
                    (14, [center, first, second])
                        if (center.sense == 0 && section_skamp_is_point(definition, center)
                            || inactive_point_entity(center).is_some())
                            && inactive_point_locus(first).is_some()
                            && inactive_point_locus(second).is_some() =>
                    {
                        SketchConstraintDefinition::PointSymmetric {
                            first: inactive_point_locus(first)?,
                            second: inactive_point_locus(second)?,
                            center: section_skamp_locus(definition, sketch, center).or_else(
                                || inactive_point_entity(center).map(SketchLocus::Entity),
                            )?,
                        }
                    }
                    (17 | 30 | 31, [_, _]) => {
                        if let Some((first, second, axis)) =
                            section_skamp_same_coordinate(definition, sketch, skamp, active)
                        {
                            SketchConstraintDefinition::SameCoordinate {
                                first,
                                second,
                                axis,
                            }
                        } else {
                            native_constraint()?
                        }
                    }
                    (35, [first, second]) => {
                        if let Some((point, entity)) =
                            section_skamp_midpoint(definition, sketch, first, second)
                        {
                            SketchConstraintDefinition::Midpoint { point, entity }
                        } else {
                            native_constraint()?
                        }
                    }
                    _ => native_constraint()?,
                }
            } else {
                native_constraint()?
            };
            if active
                && geometry.is_some_and(|geometry| {
                    !sketch_constraint_loci_compatible(&constraint_definition, geometry)
                })
            {
                constraint_definition = native_constraint()?;
            }
            Some((
                SketchConstraint {
                    id: if unique_skamp_id {
                        sketch_constraint_id(sketch, format_args!("skamp:{}", skamp.id))
                    } else {
                        sketch_constraint_id(sketch, format_args!("skamp:offset:{}", skamp.offset))
                    },
                    sketch: sketch.clone(),
                    definition: constraint_definition,
                    name: None,
                    driving: None,
                    active: Some(active),
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(sketch_native_ref(sketch)),
                },
                skamp.offset,
            ))
        })
        .collect()
}

fn sketch_constraint_loci_compatible(
    definition: &SketchConstraintDefinition,
    geometry: &BTreeMap<SketchEntityId, SketchGeometry>,
) -> bool {
    let locus_compatible = |locus: &SketchLocus| {
        let entity = match locus {
            SketchLocus::Entity(entity)
            | SketchLocus::Start(entity)
            | SketchLocus::End(entity)
            | SketchLocus::Center(entity) => entity,
        };
        geometry.get(entity).is_some_and(|geometry| match locus {
            SketchLocus::Entity(_) => true,
            SketchLocus::Start(_) | SketchLocus::End(_) => {
                !matches!(
                    geometry,
                    SketchGeometry::Point { .. } | SketchGeometry::Circle { .. }
                ) && !matches!(
                        geometry,
                        SketchGeometry::Native { native_kind }
                            if !matches!(
                                native_kind.as_str(),
                                "bounded_curve" | "line" | "arc" | "spline"
                            )
                )
            }
            SketchLocus::Center(_) => {
                matches!(
                    geometry,
                    SketchGeometry::Circle { .. }
                        | SketchGeometry::Arc { .. }
                        | SketchGeometry::Ellipse { .. }
                ) || matches!(
                    geometry,
                    SketchGeometry::Native { native_kind }
                        if matches!(native_kind.as_str(), "circle" | "arc")
                )
            }
        })
    };
    match definition {
        SketchConstraintDefinition::CoincidentLoci { loci }
        | SketchConstraintDefinition::Group { elements: loci }
        | SketchConstraintDefinition::Text { elements: loci, .. } => {
            loci.iter().all(locus_compatible)
        }
        SketchConstraintDefinition::SameCoordinate { first, second, .. }
        | SketchConstraintDefinition::TangentLoci { first, second }
        | SketchConstraintDefinition::DistanceLoci { first, second, .. }
        | SketchConstraintDefinition::HorizontalDistance { first, second, .. }
        | SketchConstraintDefinition::VerticalDistance { first, second, .. } => {
            locus_compatible(first) && locus_compatible(second)
        }
        SketchConstraintDefinition::Midpoint { point, entity }
        | SketchConstraintDefinition::PointOnObject { point, entity } => {
            locus_compatible(point) && geometry.contains_key(entity)
        }
        SketchConstraintDefinition::Symmetric { first, second, .. } => {
            locus_compatible(first) && locus_compatible(second)
        }
        SketchConstraintDefinition::PointSymmetric {
            first,
            second,
            center,
        } => locus_compatible(first) && locus_compatible(second) && locus_compatible(center),
        SketchConstraintDefinition::SnellsLaw {
            incident,
            refracted,
            ..
        } => locus_compatible(incident) && locus_compatible(refracted),
        _ => true,
    }
}

fn section_entity_external_ids(definition: &crate::feature::FeatureDefinition) -> BTreeSet<u32> {
    let mut ids = unique_section_segment_external_ids(definition);
    let Some(order) = &definition.order_table else {
        return ids;
    };
    let ambiguous_segment_ids = ambiguous_section_segment_external_ids(definition);
    let unique_saved_ids = unique_saved_section_internal_ids(definition);
    ids.extend(
        definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(|entity| saved_section_entity_identity(entity).0)
            .filter_map(|internal_id| {
                saved_section_external_id(
                    order,
                    &unique_saved_ids,
                    &ambiguous_segment_ids,
                    internal_id,
                )
            }),
    );
    ids
}

fn section_segment_external_id_counts(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, usize> {
    definition
        .segments
        .as_ref()
        .map_or_else(BTreeMap::new, |table| {
            table
                .rows
                .iter()
                .map(|row| row.external_id)
                .chain(table.opaque_rows.iter().map(|row| row.external_id))
                .fold(BTreeMap::new(), |mut counts, external_id| {
                    *counts.entry(external_id).or_insert(0) += 1;
                    counts
                })
        })
}

fn unique_section_segment_external_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    section_segment_external_id_counts(definition)
        .into_iter()
        .filter_map(|(external_id, count)| (count == 1).then_some(external_id))
        .collect()
}

fn ambiguous_section_segment_external_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    section_segment_external_id_counts(definition)
        .into_iter()
        .filter_map(|(external_id, count)| (count > 1).then_some(external_id))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SavedSectionEntityKind {
    Line,
    Arc,
    Circle,
    Spline,
    Dummy,
}

impl SavedSectionEntityKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Arc => "arc",
            Self::Circle => "circle",
            Self::Spline => "spline",
            Self::Dummy => "dummy",
        }
    }
}

fn saved_section_entity_identity(
    entity: &crate::feature::FeatureSavedEntity,
) -> (Option<u32>, usize, SavedSectionEntityKind) {
    match entity {
        crate::feature::FeatureSavedEntity::Line(line) => (
            Some(line.entity_id),
            line.offset,
            SavedSectionEntityKind::Line,
        ),
        crate::feature::FeatureSavedEntity::Arc(arc) => {
            (Some(arc.entity_id), arc.offset, SavedSectionEntityKind::Arc)
        }
        crate::feature::FeatureSavedEntity::Circle(circle) => (
            Some(circle.entity_id),
            circle.offset,
            SavedSectionEntityKind::Circle,
        ),
        crate::feature::FeatureSavedEntity::Spline(spline) => (
            spline.entity_id,
            spline.offset,
            SavedSectionEntityKind::Spline,
        ),
        crate::feature::FeatureSavedEntity::Dummy(dummy) => {
            (dummy.entity_id, dummy.offset, SavedSectionEntityKind::Dummy)
        }
    }
}

fn unresolved_saved_section_entity(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    saved: &crate::feature::FeatureSavedEntity,
    unique_saved_ids: &BTreeSet<u32>,
    ambiguous_segment_ids: &BTreeSet<u32>,
) -> (SketchEntity, usize) {
    let (internal_id, offset, kind) = saved_section_entity_identity(saved);
    let unique_internal_id = internal_id.is_some_and(|id| unique_saved_ids.contains(&id));
    let external_id = if unique_internal_id {
        definition.order_table.as_ref().and_then(|order| {
            saved_section_external_id(order, unique_saved_ids, ambiguous_segment_ids, internal_id?)
        })
    } else {
        None
    };
    let suffix = if unique_internal_id {
        external_id.map_or_else(
            || {
                let internal_id = internal_id.expect("unique saved entity has an id");
                match kind {
                    SavedSectionEntityKind::Spline | SavedSectionEntityKind::Dummy => {
                        internal_id.to_string()
                    }
                    _ => format!("saved{internal_id}"),
                }
            },
            |external_id| external_id.to_string(),
        )
    } else {
        format!("saved:offset:{offset}")
    };
    let id = external_id.map_or_else(
        || match kind {
            SavedSectionEntityKind::Spline => SketchEntityId(format!(
                "creo:featdefs:saved_spline#{}:{suffix}",
                sketch_identity_scope(sketch)
            )),
            SavedSectionEntityKind::Dummy => SketchEntityId(format!(
                "creo:featdefs:saved_dummy#{}:{suffix}",
                sketch_identity_scope(sketch)
            )),
            _ => sketch_entity_id(sketch, &suffix),
        },
        |external_id| sketch_entity_id(sketch, external_id),
    );
    (
        SketchEntity {
            id,
            sketch: sketch.clone(),
            construction: true,
            native_ref: Some(sketch_native_ref(sketch)),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Native {
                native_kind: format!("saved_{}", kind.name()),
            },
        },
        offset,
    )
}

fn unique_saved_section_internal_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    definition
        .saved_section
        .iter()
        .flat_map(|saved| &saved.entities)
        .filter_map(|entity| saved_section_entity_identity(entity).0)
        .fold(BTreeMap::new(), |mut counts, internal_id| {
            *counts.entry(internal_id).or_insert(0usize) += 1;
            counts
        })
        .into_iter()
        .filter_map(|(internal_id, count)| (count == 1).then_some(internal_id))
        .collect()
}

fn materialized_saved_section_external_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    let unique_saved_ids = unique_saved_section_internal_ids(definition);
    let ambiguous_segment_ids = ambiguous_section_segment_external_ids(definition);
    definition
        .saved_section
        .iter()
        .flat_map(|saved| &saved.entities)
        .filter_map(|entity| {
            match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => {
                    saved_spline_sketch_geometry(spline)?;
                }
                _ => {
                    saved_section_entity_geometry(entity)?;
                }
            }
            let internal_id = saved_section_entity_identity(entity).0?;
            unique_saved_ids.contains(&internal_id).then_some(())?;
            definition.order_table.as_ref().and_then(|order| {
                saved_section_external_id(
                    order,
                    &unique_saved_ids,
                    &ambiguous_segment_ids,
                    internal_id,
                )
            })
        })
        .collect()
}

fn saved_section_external_id(
    order: &crate::feature::FeatureOrderTable,
    unique_saved_ids: &BTreeSet<u32>,
    ambiguous_segment_ids: &BTreeSet<u32>,
    internal_id: u32,
) -> Option<u32> {
    unique_saved_ids.contains(&internal_id).then_some(())?;
    let external_id = order.external_id(internal_id)?;
    (!ambiguous_segment_ids.contains(&external_id)).then_some(external_id)
}

fn section_segment_identity_suffix(
    unique_external_ids: &BTreeSet<u32>,
    segment: &crate::feature::FeatureSegment,
) -> String {
    if unique_external_ids.contains(&segment.external_id) {
        segment.external_id.to_string()
    } else {
        format!("offset:{}", segment.offset)
    }
}

fn resolved_profile_chains(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    emitted: &BTreeSet<u32>,
) -> Vec<Vec<SketchEntityUse>> {
    let Some(table) = &definition.trim_entities else {
        return resolved_segment_profile_chains(definition, sketch, emitted);
    };
    if !table.has_complete_bucket_frame() || !table.has_unique_external_ids() {
        return resolved_segment_profile_chains(definition, sketch, emitted);
    }
    let rows = table
        .rows
        .iter()
        .filter_map(|row| Some((row, trim_segment_id(definition, row)?)))
        .collect::<Vec<_>>();
    let trimmed_ids = rows
        .iter()
        .map(|(_, external_id)| *external_id)
        .collect::<BTreeSet<_>>();
    let trimmed_points = definition
        .segments
        .iter()
        .flat_map(|segments| &segments.rows)
        .filter(|segment| trimmed_ids.contains(&segment.external_id))
        .flat_map(|segment| segment.point_ids)
        .collect::<BTreeSet<_>>();
    if definition
        .segments
        .iter()
        .flat_map(|segments| &segments.rows)
        .filter(|segment| {
            emitted.contains(&segment.external_id)
                && !trimmed_ids.contains(&segment.external_id)
                && matches!(
                    segment.kind,
                    crate::feature::FeatureSegmentKind::Line
                        | crate::feature::FeatureSegmentKind::Arc
                )
        })
        .any(|segment| {
            segment
                .point_ids
                .into_iter()
                .any(|point| trimmed_points.contains(&point))
        })
    {
        return resolved_segment_profile_chains(definition, sketch, emitted);
    }
    let mut incident = BTreeMap::<u32, Vec<usize>>::new();
    for (index, row) in rows.iter().enumerate() {
        for vertex in row.0.vertices {
            incident.entry(vertex).or_default().push(index);
        }
    }
    let mut remaining = (0..rows.len()).collect::<BTreeSet<_>>();
    let mut profiles = Vec::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut frontier = vec![seed];
        while let Some(index) = frontier.pop() {
            for vertex in rows[index].0.vertices {
                for adjacent in &incident[&vertex] {
                    if component.insert(*adjacent) {
                        frontier.push(*adjacent);
                    }
                }
            }
        }
        remaining.retain(|index| !component.contains(index));
        if incident
            .values()
            .any(|rows| rows.iter().filter(|row| component.contains(row)).count() > 2)
        {
            continue;
        }
        if component
            .iter()
            .any(|index| !emitted.contains(&rows[*index].1))
        {
            continue;
        }
        let endpoints = incident
            .iter()
            .filter(|(_, rows)| rows.iter().filter(|row| component.contains(row)).count() == 1)
            .map(|(vertex, _)| *vertex)
            .collect::<Vec<_>>();
        if !matches!(endpoints.len(), 0 | 2) {
            continue;
        }
        let first_row = component
            .iter()
            .min_by_key(|index| rows[**index].1)
            .copied()
            .expect("component contains seed");
        let mut vertex = endpoints
            .iter()
            .min()
            .copied()
            .unwrap_or(rows[first_row].0.vertices[0]);
        let start_vertex = vertex;
        let mut unused = component;
        let mut profile = Vec::new();
        while !unused.is_empty() {
            let candidates = incident[&vertex]
                .iter()
                .filter(|index| unused.contains(index))
                .copied()
                .collect::<Vec<_>>();
            let index = if profile.is_empty() && endpoints.is_empty() {
                if candidates.contains(&first_row) {
                    first_row
                } else {
                    break;
                }
            } else if candidates.len() == 1 {
                candidates[0]
            } else {
                break;
            };
            let (row, external_id) = rows[index];
            let row_reversed = row.vertices[1] == vertex;
            if !row_reversed && row.vertices[0] != vertex {
                break;
            }
            let arc_orientation_reversed = definition
                .segments
                .as_ref()
                .and_then(|table| table.segment(external_id))
                .is_some_and(|segment| {
                    segment.kind == crate::feature::FeatureSegmentKind::Arc
                        && segment.arc_orientation == Some(0)
                });
            profile.push(SketchEntityUse {
                entity: sketch_entity_id(sketch, external_id),
                reversed: row_reversed ^ arc_orientation_reversed,
            });
            vertex = if row_reversed {
                row.vertices[0]
            } else {
                row.vertices[1]
            };
            unused.remove(&index);
        }
        let terminal_ok = if endpoints.is_empty() {
            vertex == start_vertex
        } else {
            endpoints.contains(&vertex) && vertex != start_vertex
        };
        if unused.is_empty() && terminal_ok {
            profiles.push(profile);
        }
    }
    profiles
}

fn resolved_segment_profile_chains(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    emitted: &BTreeSet<u32>,
) -> Vec<Vec<SketchEntityUse>> {
    let Some(table) = definition
        .segments
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let rows = table
        .rows
        .iter()
        .filter(|segment| {
            emitted.contains(&segment.external_id)
                && matches!(
                    segment.kind,
                    crate::feature::FeatureSegmentKind::Line
                        | crate::feature::FeatureSegmentKind::Arc
                )
        })
        .collect::<Vec<_>>();
    let mut incident = BTreeMap::<u32, Vec<usize>>::new();
    for (index, segment) in rows.iter().enumerate() {
        for point in segment.point_ids {
            incident.entry(point).or_default().push(index);
        }
    }
    let mut remaining = (0..rows.len()).collect::<BTreeSet<_>>();
    let mut profiles = Vec::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut frontier = vec![seed];
        while let Some(index) = frontier.pop() {
            for point in rows[index].point_ids {
                for adjacent in &incident[&point] {
                    if component.insert(*adjacent) {
                        frontier.push(*adjacent);
                    }
                }
            }
        }
        remaining.retain(|index| !component.contains(index));
        if component.iter().any(|index| {
            rows[*index].point_ids.into_iter().any(|point| {
                incident[&point]
                    .iter()
                    .filter(|row| component.contains(row))
                    .count()
                    != 2
            })
        }) {
            continue;
        }
        let first = component
            .iter()
            .min_by_key(|index| rows[**index].external_id)
            .copied()
            .expect("component contains seed");
        let mut point = rows[first].point_ids[0].min(rows[first].point_ids[1]);
        let start = point;
        let mut unused = component;
        let mut profile = Vec::new();
        while !unused.is_empty() {
            let candidates = incident[&point]
                .iter()
                .filter(|index| unused.contains(index))
                .copied()
                .collect::<BTreeSet<_>>();
            let index = if profile.is_empty() && candidates.contains(&first) {
                first
            } else if candidates.len() == 1 {
                *candidates.first().expect("one candidate")
            } else {
                break;
            };
            let segment = rows[index];
            let traversal_reversed = segment.point_ids[1] == point;
            if !traversal_reversed && segment.point_ids[0] != point {
                break;
            }
            let analytic_reversed = segment.kind == crate::feature::FeatureSegmentKind::Arc
                && segment.arc_orientation == Some(0);
            profile.push(SketchEntityUse {
                entity: sketch_entity_id(sketch, segment.external_id),
                reversed: traversal_reversed ^ analytic_reversed,
            });
            point = if traversal_reversed {
                segment.point_ids[0]
            } else {
                segment.point_ids[1]
            };
            unused.remove(&index);
        }
        if unused.is_empty() && point == start {
            profiles.push(profile);
        }
    }
    profiles
}

fn solver_only_section_entities(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, usize> {
    let declared_segment_ids = definition
        .segments
        .iter()
        .flat_map(|table| {
            table
                .rows
                .iter()
                .map(|segment| segment.external_id)
                .chain(table.opaque_rows.iter().map(|segment| segment.external_id))
        })
        .collect::<BTreeSet<_>>();
    definition
        .relations
        .iter()
        .flat_map(|relations| &relations.skamps)
        .flat_map(|skamp| {
            skamp
                .items
                .iter()
                .map(move |item| (item.entity_id, skamp.offset))
        })
        .filter(|(entity_id, _)| !declared_segment_ids.contains(entity_id))
        .fold(
            BTreeMap::<u32, usize>::new(),
            |mut entities, (id, offset)| {
                entities
                    .entry(id)
                    .and_modify(|first_offset| *first_offset = (*first_offset).min(offset))
                    .or_insert(offset);
                entities
            },
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SectionEntityIncidenceFamily {
    Point,
    BoundedCurve,
    Line,
    Arc,
    Circular,
}

fn section_skamp_has_proven_point_locus(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    if item.sense == 0 {
        return unique_opaque_section_segment(definition, item.entity_id, 1).is_some()
            || unique_decoded_section_segment(definition, item.entity_id)
                .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Point);
    }
    let solver_family = section_incidence_curve_family_evidence(definition, item.entity_id);
    if solver_family.len() == 1
        && ((solver_family.contains(&SectionEntityIncidenceFamily::BoundedCurve)
            || solver_family.contains(&SectionEntityIncidenceFamily::Line)
            || solver_family.contains(&SectionEntityIncidenceFamily::Arc))
            && matches!(item.sense, 2 | 3)
            || (solver_family.contains(&SectionEntityIncidenceFamily::Arc)
                || solver_family.contains(&SectionEntityIncidenceFamily::Circular))
                && matches!(item.sense, 2..=4))
    {
        return true;
    }
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        return matches!(
            (segment.kind, item.sense),
            (crate::feature::FeatureSegmentKind::Line, 2 | 3)
                | (crate::feature::FeatureSegmentKind::Arc, 2..=4)
        );
    }
    if unique_opaque_section_segment(definition, item.entity_id, 47).is_some() {
        return matches!(item.sense, 2 | 3);
    }
    if unique_opaque_section_segment(definition, item.entity_id, 10).is_some() {
        return item.sense == 4;
    }
    matches!(
        (section_saved_entity(definition, item.entity_id), item.sense),
        (Some(crate::feature::FeatureSavedEntity::Line(_)), 2 | 3)
            | (Some(crate::feature::FeatureSavedEntity::Arc(_)), 2..=4)
            | (Some(crate::feature::FeatureSavedEntity::Circle(_)), 4)
    )
}

fn section_incidence_curve_family_evidence(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> BTreeSet<SectionEntityIncidenceFamily> {
    let mut evidence = BTreeSet::new();
    for skamp in definition
        .relations
        .iter()
        .filter(|relations| feature_skamp_table_complete(relations))
        .flat_map(|relations| &relations.skamps)
        .filter(|skamp| section_skamp_active(skamp.status))
    {
        for item in &skamp.items {
            if item.entity_id == entity_id && matches!(item.sense, 2 | 3) {
                evidence.insert(SectionEntityIncidenceFamily::BoundedCurve);
            }
            if item.entity_id == entity_id && item.sense == 4 {
                evidence.insert(SectionEntityIncidenceFamily::Circular);
            }
        }
        match (skamp.kind, skamp.items.as_slice()) {
            (5 | 7 | 8, [first, second])
                if first.sense == 0
                    && second.sense == 0
                    && (first.entity_id == entity_id || second.entity_id == entity_id) =>
            {
                evidence.insert(SectionEntityIncidenceFamily::Line);
            }
            (6, [first, second])
                if first.sense == 0
                    && second.sense == 0
                    && (first.entity_id == entity_id || second.entity_id == entity_id) =>
            {
                evidence.insert(SectionEntityIncidenceFamily::Circular);
            }
            _ => {}
        }
    }
    normalize_section_incidence_curve_family_evidence(&mut evidence);
    evidence
}

fn unique_section_incidence_curve_family(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> Option<SectionEntityIncidenceFamily> {
    let mut evidence = section_incidence_curve_family_evidence(definition, entity_id).into_iter();
    let family = evidence.next()?;
    evidence.next().is_none().then_some(family)
}

fn normalize_section_incidence_curve_family_evidence(
    evidence: &mut BTreeSet<SectionEntityIncidenceFamily>,
) {
    if evidence.contains(&SectionEntityIncidenceFamily::Line) {
        evidence.remove(&SectionEntityIncidenceFamily::BoundedCurve);
    } else if evidence.contains(&SectionEntityIncidenceFamily::Circular)
        && evidence.remove(&SectionEntityIncidenceFamily::BoundedCurve)
    {
        evidence.remove(&SectionEntityIncidenceFamily::Circular);
        evidence.insert(SectionEntityIncidenceFamily::Arc);
    }
}

fn solver_only_section_entity_family(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> Option<SectionEntityIncidenceFamily> {
    solver_only_section_entities(definition)
        .contains_key(&entity_id)
        .then_some(())?;
    let mut evidence = section_incidence_curve_family_evidence(definition, entity_id);
    for skamp in definition
        .relations
        .iter()
        .filter(|relations| feature_skamp_table_complete(relations))
        .flat_map(|relations| &relations.skamps)
        .filter(|skamp| section_skamp_active(skamp.status))
    {
        if let (0, [first, second]) = (skamp.kind, skamp.items.as_slice()) {
            if first.entity_id == entity_id
                && first.sense == 0
                && section_skamp_has_proven_point_locus(definition, second)
            {
                evidence.insert(SectionEntityIncidenceFamily::Point);
            }
            if second.entity_id == entity_id
                && second.sense == 0
                && section_skamp_has_proven_point_locus(definition, first)
            {
                evidence.insert(SectionEntityIncidenceFamily::Point);
            }
        }
    }
    let mut evidence = evidence.into_iter();
    let family = evidence.next()?;
    evidence.next().is_none().then_some(family)
}

fn transfer_sketches(scan: &ContainerScan, ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    for definition in scan
        .features
        .definitions
        .iter()
        .filter(|definition| feature_definition_has_sketch_design(definition))
    {
        let transform = definition.section_3d.as_ref().and_then(|section| {
            unique_feature_section_transform(
                &scan.features.section_transforms,
                definition.id,
                section.offset,
            )
        });
        let sketch_id = model_sketch_id(scan, definition);
        let segments = section_segment_rows(definition);
        let unique_segment_ids = unique_section_segment_external_ids(definition);
        let ambiguous_segment_ids = ambiguous_section_segment_external_ids(definition);
        let unique_saved_ids = unique_saved_section_internal_ids(definition);
        let complete_segment_table = definition
            .segments
            .as_ref()
            .is_some_and(crate::feature::FeatureSegmentTable::is_complete);
        let points = resolved_section_points(definition);
        let radii = resolved_section_radii(definition);
        let missing_line_geometry = saved_section_missing_line_geometry(definition);
        let solved = definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        let trim_vertex_coordinates = resolved_trim_vertex_coordinates(definition, &points);
        let resolved_segment_geometries = segments
            .iter()
            .map(|segment| {
                (
                    segment.offset,
                    resolved_section_segment_geometry_with_missing_line(
                        definition,
                        &points,
                        segment,
                        missing_line_geometry.as_ref(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let segment_geometries = segments
            .iter()
            .map(|segment| {
                let geometry = if unique_segment_ids.contains(&segment.external_id)
                    && solved.contains(&segment.external_id)
                {
                    trimmed_section_segment_geometry_with_missing_line(
                        definition,
                        &points,
                        &trim_vertex_coordinates,
                        segment,
                        missing_line_geometry.as_ref(),
                    )
                } else {
                    resolved_segment_geometries
                        .get(&segment.offset)
                        .cloned()
                        .flatten()
                };
                (segment.offset, geometry)
            })
            .collect::<BTreeMap<_, _>>();
        let segment_geometry = |segment: &crate::feature::FeatureSegment| {
            segment_geometries.get(&segment.offset).cloned().flatten()
        };
        let opaque_geometries = definition
            .segments
            .iter()
            .flat_map(|table| &table.opaque_rows)
            .filter_map(|segment| {
                Some((
                    segment.offset,
                    section_opaque_point_geometry(&points, segment)
                        .or_else(|| section_opaque_centered_line_geometry(&points, segment))
                        .or_else(|| section_opaque_circle_geometry(&points, &radii, segment))?,
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let emitted = segments
            .iter()
            .filter(|segment| {
                unique_segment_ids.contains(&segment.external_id)
                    && segment_geometry(segment).is_some()
            })
            .map(|segment| segment.external_id)
            .collect::<BTreeSet<_>>();
        let resolved_segment_offsets = segments
            .iter()
            .filter(|segment| segment_geometry(segment).is_some())
            .map(|segment| segment.offset)
            .collect::<BTreeSet<_>>();
        let materialized_saved_section_external_ids =
            materialized_saved_section_external_ids(definition);
        let mut profiles = resolved_profile_chains(definition, &sketch_id, &emitted);
        let generated_profile_geometries = segments
            .iter()
            .filter(|segment| {
                unique_segment_ids.contains(&segment.external_id)
                    && emitted.contains(&segment.external_id)
            })
            .filter_map(|segment| {
                let geometry = segment_geometry(segment)?;
                let expected_kinds = section_generated_profile_surface_kinds(&geometry)?;
                section_entity_is_generated_profile(
                    complete_segment_table,
                    definition.owner_feature_id,
                    segment.external_id,
                    expected_kinds,
                    &scan.features.entity_tables,
                    &scan.surfaces.rows,
                )
                .then_some((segment.external_id, geometry))
            })
            .chain(
                definition
                    .segments
                    .iter()
                    .flat_map(|table| &table.opaque_rows)
                    .filter(|segment| {
                        segment.kind == 10 && unique_segment_ids.contains(&segment.external_id)
                    })
                    .filter_map(|segment| {
                        let geometry = opaque_geometries.get(&segment.offset)?.clone();
                        let expected_kinds = section_generated_profile_surface_kinds(&geometry)?;
                        section_entity_is_generated_profile(
                            complete_segment_table,
                            definition.owner_feature_id,
                            segment.external_id,
                            expected_kinds,
                            &scan.features.entity_tables,
                            &scan.surfaces.rows,
                        )
                        .then_some((segment.external_id, geometry))
                    }),
            )
            .collect::<Vec<_>>();
        let mut profile_entities = profiles
            .iter()
            .flatten()
            .map(|entity_use| entity_use.entity.clone())
            .collect::<BTreeSet<_>>();
        for profile in saved_profile_chains(&sketch_id, &generated_profile_geometries) {
            if profile
                .iter()
                .all(|entity_use| !profile_entities.contains(&entity_use.entity))
            {
                profile_entities.extend(profile.iter().map(|entity_use| entity_use.entity.clone()));
                profiles.push(profile);
            }
        }
        let mut entities = segments
            .iter()
            .filter_map(|segment| {
                let geometry = segment_geometry(segment)?;
                let suffix = section_segment_identity_suffix(&unique_segment_ids, segment);
                let id = sketch_entity_id(&sketch_id, &suffix);
                annotate(
                    annotations,
                    &id.0,
                    "FeatDefs",
                    segment.offset as u64,
                    match segment.kind {
                        crate::feature::FeatureSegmentKind::Line => "solved_section_line",
                        crate::feature::FeatureSegmentKind::Arc => "solved_section_arc",
                        crate::feature::FeatureSegmentKind::Point => "solved_section_point",
                    },
                    Exactness::Derived,
                );
                let construction = !unique_segment_ids.contains(&segment.external_id)
                    || (!solved.contains(&segment.external_id) && !profile_entities.contains(&id));
                Some(SketchEntity {
                    id,
                    sketch: sketch_id.clone(),
                    construction,
                    native_ref: Some(sketch_native_ref(&sketch_id)),
                    geometry_ref: placed_sketch_curve_ref(transform, &sketch_id, suffix, &geometry),
                    endpoint_refs: match segment.kind {
                        crate::feature::FeatureSegmentKind::Arc => {
                            vec![segment.point_ids[1], segment.point_ids[0]]
                        }
                        crate::feature::FeatureSegmentKind::Line => segment.point_ids.to_vec(),
                        crate::feature::FeatureSegmentKind::Point => vec![segment.point_ids[0]],
                    }
                    .into_iter()
                    .map(|point| sketch_point_ref(&sketch_id, point))
                    .collect(),
                    geometry,
                })
            })
            .collect::<Vec<_>>();
        for segment in segments
            .iter()
            .filter(|segment| !resolved_segment_offsets.contains(&segment.offset))
        {
            let id = sketch_entity_id(
                &sketch_id,
                section_segment_identity_suffix(&unique_segment_ids, segment),
            );
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                segment.offset as u64,
                "unresolved_section_segment",
                Exactness::ByteExact,
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: true,
                native_ref: Some(sketch_native_ref(&sketch_id)),
                geometry_ref: None,
                endpoint_refs: match segment.kind {
                    crate::feature::FeatureSegmentKind::Arc => {
                        vec![segment.point_ids[1], segment.point_ids[0]]
                    }
                    crate::feature::FeatureSegmentKind::Line => segment.point_ids.to_vec(),
                    crate::feature::FeatureSegmentKind::Point => vec![segment.point_ids[0]],
                }
                .into_iter()
                .map(|point| sketch_point_ref(&sketch_id, point))
                .collect(),
                geometry: SketchGeometry::Native {
                    native_kind: match segment.kind {
                        crate::feature::FeatureSegmentKind::Line => "line",
                        crate::feature::FeatureSegmentKind::Arc => "arc",
                        crate::feature::FeatureSegmentKind::Point => "point",
                    }
                    .to_string(),
                },
            });
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| &table.opaque_rows)
        {
            let unique_external_id = unique_segment_ids.contains(&segment.external_id);
            if unique_external_id
                && materialized_saved_section_external_ids.contains(&segment.external_id)
            {
                continue;
            }
            let suffix = if unique_external_id {
                segment.external_id.to_string()
            } else {
                format!("opaque:offset:{}", segment.offset)
            };
            let id = sketch_entity_id(&sketch_id, suffix);
            let geometry = if matches!(segment.kind, 1 | 10 | 47) {
                opaque_geometries
                    .get(&segment.offset)
                    .cloned()
                    .unwrap_or_else(|| SketchGeometry::Native {
                        native_kind: match segment.kind {
                            1 => "point",
                            10 => "circle",
                            47 => "line",
                            _ => unreachable!(),
                        }
                        .to_string(),
                    })
            } else if unique_external_id {
                let native_kind =
                    match unique_section_incidence_curve_family(definition, segment.external_id) {
                        Some(SectionEntityIncidenceFamily::BoundedCurve) => {
                            "bounded_curve".to_string()
                        }
                        Some(SectionEntityIncidenceFamily::Line) => "line".to_string(),
                        Some(SectionEntityIncidenceFamily::Arc) => "arc".to_string(),
                        Some(SectionEntityIncidenceFamily::Circular) => "circle".to_string(),
                        _ => format!("segment_type:{}", segment.kind),
                    };
                SketchGeometry::Native { native_kind }
            } else {
                SketchGeometry::Native {
                    native_kind: format!("segment_type:{}", segment.kind),
                }
            };
            let solved_geometry = matches!(
                &geometry,
                SketchGeometry::Point { .. }
                    | SketchGeometry::Circle { .. }
                    | SketchGeometry::Line { .. }
            );
            let construction = !unique_external_id || !profile_entities.contains(&id);
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                segment.offset as u64,
                if solved_geometry {
                    if segment.kind == 1 {
                        "solved_section_point"
                    } else if segment.kind == 47 {
                        "solved_section_line"
                    } else {
                        "solved_section_circle"
                    }
                } else {
                    "opaque_section_segment"
                },
                if solved_geometry {
                    Exactness::Derived
                } else {
                    Exactness::ByteExact
                },
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction,
                native_ref: Some(sketch_native_ref(&sketch_id)),
                geometry_ref: placed_sketch_curve_ref(
                    transform,
                    &sketch_id,
                    if unique_external_id {
                        segment.external_id.to_string()
                    } else {
                        format!("opaque:offset:{}", segment.offset)
                    },
                    &geometry,
                ),
                endpoint_refs: Vec::new(),
                geometry,
            });
        }
        let mut saved_section_geometries = Vec::new();
        let mut generated_saved_geometries = Vec::new();
        for (internal_id, geometry, offset) in definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(saved_section_entity_geometry)
        {
            let unique_internal_id = unique_saved_ids.contains(&internal_id);
            let external_id = if unique_internal_id {
                definition.order_table.as_ref().and_then(|order| {
                    saved_section_external_id(
                        order,
                        &unique_saved_ids,
                        &ambiguous_segment_ids,
                        internal_id,
                    )
                })
            } else {
                None
            };
            let suffix = if unique_internal_id {
                external_id.map_or_else(
                    || format!("saved{internal_id}"),
                    |external_id| external_id.to_string(),
                )
            } else {
                format!("saved:offset:{offset}")
            };
            let entity_id = sketch_entity_id(&sketch_id, &suffix);
            if entities.iter().any(|entity| entity.id == entity_id) {
                continue;
            }
            let Some(expected_kinds) = section_generated_profile_surface_kinds(&geometry) else {
                continue;
            };
            let generated = external_id.is_some_and(|external_id| {
                section_entity_is_generated_profile(
                    complete_segment_table,
                    definition.owner_feature_id,
                    external_id,
                    expected_kinds,
                    &scan.features.entity_tables,
                    &scan.surfaces.rows,
                )
            });
            let curve_id = CurveId(sketch_section_curve_id(&sketch_id, &suffix));
            annotate(
                annotations,
                &entity_id.0,
                "FeatDefs",
                offset as u64,
                "saved_section_entity",
                Exactness::Derived,
            );
            if let Some(external_id) = external_id.filter(|_| generated) {
                generated_saved_geometries.push((external_id, geometry.clone()));
            }
            entities.push(SketchEntity {
                id: entity_id,
                sketch: sketch_id.clone(),
                construction: !generated,
                native_ref: Some(format!(
                    "{}:saved_entity#{internal_id}",
                    sketch_native_ref(&sketch_id)
                )),
                geometry_ref: placed_sketch_curve_ref(transform, &sketch_id, &suffix, &geometry),
                endpoint_refs: Vec::new(),
                geometry: geometry.clone(),
            });
            saved_section_geometries.push((internal_id, external_id, geometry, offset, curve_id));
        }
        for spline in definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(|entity| match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
        {
            let Some(geometry) = saved_spline_sketch_geometry(spline) else {
                continue;
            };
            let unique_internal_id = spline
                .entity_id
                .is_some_and(|id| unique_saved_ids.contains(&id));
            let suffix = if unique_internal_id {
                spline
                    .entity_id
                    .expect("unique saved spline has an internal id")
                    .to_string()
            } else {
                format!("offset{}", spline.offset)
            };
            let external_id = if unique_internal_id {
                definition.order_table.as_ref().and_then(|order| {
                    saved_section_external_id(
                        order,
                        &unique_saved_ids,
                        &ambiguous_segment_ids,
                        spline.entity_id?,
                    )
                })
            } else {
                None
            };
            let generated = external_id.is_some_and(|external_id| {
                let Some(expected_kinds) = section_generated_profile_surface_kinds(&geometry)
                else {
                    return false;
                };
                section_entity_is_generated_profile(
                    complete_segment_table,
                    definition.owner_feature_id,
                    external_id,
                    expected_kinds,
                    &scan.features.entity_tables,
                    &scan.surfaces.rows,
                )
            });
            let entity_id = external_id.map_or_else(
                || {
                    SketchEntityId(format!(
                        "creo:featdefs:saved_spline#{}:{suffix}",
                        sketch_identity_scope(&sketch_id)
                    ))
                },
                |external_id| sketch_entity_id(&sketch_id, external_id),
            );
            let curve_id = CurveId(format!(
                "creo:featdefs:saved_spline_curve#{}:{suffix}",
                sketch_identity_scope(&sketch_id)
            ));
            if entities.iter().any(|entity| entity.id == entity_id) {
                continue;
            }
            annotate(
                annotations,
                &entity_id.0,
                "FeatDefs",
                spline.offset as u64,
                "saved_interpolation_spline",
                Exactness::Derived,
            );
            entities.push(SketchEntity {
                id: entity_id,
                sketch: sketch_id.clone(),
                construction: !generated,
                native_ref: Some(format!(
                    "{}:saved_spline#{suffix}",
                    sketch_native_ref(&sketch_id)
                )),
                geometry_ref: transform.map(|_| curve_id.0.clone()),
                endpoint_refs: Vec::new(),
                geometry: geometry.clone(),
            });
            if let Some(external_id) = external_id.filter(|_| generated) {
                generated_saved_geometries.push((external_id, geometry));
            }
        }
        for saved in definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
        {
            let (entity, offset) = unresolved_saved_section_entity(
                definition,
                &sketch_id,
                saved,
                &unique_saved_ids,
                &ambiguous_segment_ids,
            );
            if entities.iter().any(|existing| existing.id == entity.id) {
                continue;
            }
            annotate(
                annotations,
                &entity.id.0,
                "FeatDefs",
                offset as u64,
                "unresolved_saved_section_entity",
                Exactness::ByteExact,
            );
            entities.push(entity);
        }
        profiles.extend(saved_profile_chains(
            &sketch_id,
            &generated_saved_geometries,
        ));
        if let Some(transform) = transform {
            for segment in segments {
                let Some(section_geometry) = resolved_segment_geometries
                    .get(&segment.offset)
                    .cloned()
                    .flatten()
                else {
                    continue;
                };
                let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry)
                else {
                    continue;
                };
                let suffix = section_segment_identity_suffix(&unique_segment_ids, segment);
                let id = CurveId(sketch_section_curve_id(&sketch_id, &suffix));
                if ir.model.curves.iter().any(|existing| existing.id == id) {
                    continue;
                }
                annotate(
                    annotations,
                    &id,
                    "FeatDefs",
                    segment.offset as u64,
                    "placed_section_curve",
                    Exactness::Derived,
                );
                ir.model.curves.push(Curve {
                    id,
                    geometry,
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!(
                            "FeatDefs:section#{}:{suffix}",
                            sketch_identity_scope(&sketch_id)
                        ),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
            for segment in definition
                .segments
                .iter()
                .flat_map(|segments| &segments.opaque_rows)
                .filter(|segment| matches!(segment.kind, 10 | 47))
            {
                let Some(section_geometry) = opaque_geometries.get(&segment.offset).cloned() else {
                    continue;
                };
                let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry)
                else {
                    continue;
                };
                let suffix = if unique_segment_ids.contains(&segment.external_id) {
                    segment.external_id.to_string()
                } else {
                    format!("opaque:offset:{}", segment.offset)
                };
                let id = CurveId(sketch_section_curve_id(&sketch_id, &suffix));
                if ir.model.curves.iter().any(|existing| existing.id == id) {
                    continue;
                }
                annotate(
                    annotations,
                    &id,
                    "FeatDefs",
                    segment.offset as u64,
                    if segment.kind == 47 {
                        "placed_section_line"
                    } else {
                        "placed_section_circle"
                    },
                    Exactness::Derived,
                );
                ir.model.curves.push(Curve {
                    id,
                    geometry,
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!(
                            "FeatDefs:section#{}:{suffix}",
                            sketch_identity_scope(&sketch_id)
                        ),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
            for (internal_id, external_id, section_geometry, offset, id) in saved_section_geometries
            {
                if ir.model.curves.iter().any(|existing| existing.id == id) {
                    continue;
                }
                let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry)
                else {
                    continue;
                };
                annotate(
                    annotations,
                    &id,
                    "FeatDefs",
                    offset as u64,
                    "placed_saved_section_curve",
                    Exactness::Derived,
                );
                ir.model.curves.push(Curve {
                    id,
                    geometry,
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: external_id.map_or_else(
                            || format!("FeatDefs:saved_entity#{internal_id}"),
                            |external_id| {
                                format!(
                                    "FeatDefs:section#{}:{external_id}",
                                    sketch_identity_scope(&sketch_id)
                                )
                            },
                        ),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
        }
        for (external_id, offset) in solver_only_section_entities(definition) {
            let id = sketch_entity_id(&sketch_id, external_id);
            if entities.iter().any(|entity| entity.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                offset as u64,
                "solver_only_section_entity",
                Exactness::ByteExact,
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: true,
                native_ref: Some(sketch_native_ref(&sketch_id)),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Native {
                    native_kind: match solver_only_section_entity_family(definition, external_id) {
                        Some(SectionEntityIncidenceFamily::Point) => "point",
                        Some(SectionEntityIncidenceFamily::BoundedCurve) => "bounded_curve",
                        Some(SectionEntityIncidenceFamily::Line) => "line",
                        Some(SectionEntityIncidenceFamily::Arc) => "arc",
                        Some(SectionEntityIncidenceFamily::Circular) => "circle",
                        None => "solver_only_section_entity",
                    }
                    .to_string(),
                },
            });
        }
        let emitted_entity_ids = entities
            .iter()
            .map(|entity| entity.id.clone())
            .collect::<BTreeSet<_>>();
        let emitted_entity_geometry = entities
            .iter()
            .map(|entity| (entity.id.clone(), entity.geometry.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut constraints = segments
            .iter()
            .filter_map(|segment| {
                let suffix = section_segment_identity_suffix(&unique_segment_ids, segment);
                let entity = sketch_entity_id(&sketch_id, &suffix);
                let mut constraint_definition = line_orientation_definition(segment, entity)?;
                reconcile_constraint_entity_references(
                    &mut constraint_definition,
                    &emitted_entity_ids,
                )
                .then_some(())?;
                let id = sketch_constraint_id(&sketch_id, format_args!("verhor:{suffix}"));
                annotate(
                    annotations,
                    &id.0,
                    "FeatDefs",
                    segment.offset as u64,
                    "section_line_orientation_constraint",
                    Exactness::ByteExact,
                );
                Some(SketchConstraint {
                    id,
                    sketch: sketch_id.clone(),
                    definition: constraint_definition,
                    name: None,
                    driving: None,
                    active: None,
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(sketch_native_ref(&sketch_id)),
                })
            })
            .collect::<Vec<_>>();
        for (mut constraint, offset) in section_dimension_constraints(definition, &sketch_id) {
            if !reconcile_constraint_entity_references(
                &mut constraint.definition,
                &emitted_entity_ids,
            ) {
                continue;
            }
            annotate(
                annotations,
                &constraint.id.0,
                "FeatDefs",
                offset as u64,
                "section_dimension_constraint",
                Exactness::ByteExact,
            );
            constraints.push(constraint);
        }
        for (mut constraint, offset) in section_circle_size_constraints(definition, &sketch_id) {
            if !reconcile_constraint_entity_references(
                &mut constraint.definition,
                &emitted_entity_ids,
            ) {
                continue;
            }
            annotate(
                annotations,
                &constraint.id.0,
                "FeatDefs",
                offset as u64,
                "section_circle_size_constraint",
                Exactness::ByteExact,
            );
            constraints.push(constraint);
        }
        for (mut constraint, offset) in section_skamp_constraints_for_geometry(
            definition,
            &sketch_id,
            Some(&emitted_entity_geometry),
        ) {
            if !reconcile_constraint_entity_references(
                &mut constraint.definition,
                &emitted_entity_ids,
            ) {
                continue;
            }
            annotate(
                annotations,
                &constraint.id.0,
                "FeatDefs",
                offset as u64,
                "section_solver_constraint",
                Exactness::ByteExact,
            );
            constraints.push(constraint);
        }
        ir.model.sketch_entities.extend(entities);
        ir.model.sketch_constraints.extend(constraints);
        let source_offset = transform.map_or(definition.offset, |transform| transform.offset);
        annotate(
            annotations,
            &sketch_id.0,
            "FeatDefs",
            source_offset as u64,
            if transform.is_some() {
                "datum_placed_section"
            } else {
                "unplaced_section"
            },
            Exactness::Derived,
        );
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            placement: transform.map_or(
                cadmpeg_ir::sketches::SketchPlacement::Unresolved,
                |transform| cadmpeg_ir::sketches::SketchPlacement::Resolved {
                    origin: Point3::new(
                        transform.origin[0],
                        transform.origin[1],
                        transform.origin[2],
                    ),
                    normal: Vector3::new(
                        transform.normal[0],
                        transform.normal[1],
                        transform.normal[2],
                    ),
                    u_axis: Vector3::new(
                        transform.u_axis[0],
                        transform.u_axis[1],
                        transform.u_axis[2],
                    ),
                },
            ),
            profiles,
            native_ref: Some(sketch_native_ref(&sketch_id)),
        });
        if owned_section_feature_id(scan, definition.id).is_none() {
            let feature_id = sketch_feature_id(&sketch_id);
            annotate(
                annotations,
                &feature_id.0,
                "FeatDefs",
                source_offset as u64,
                "section_sketch_feature",
                Exactness::Derived,
            );
            ir.model.features.push(Feature {
                id: feature_id,
                ordinal: ir.model.features.len() as u64,
                name: None,
                suppressed: Some(false),
                parent: None,
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: Some("section".to_string()),
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition: IrFeatureDefinition::Sketch {
                    space: cadmpeg_ir::features::SketchSpace::default(),
                    sketch: Some(sketch_id.clone()),
                },
                native_ref: Some(sketch_native_ref(&sketch_id)),
            });
        }
    }
}

fn link_feature_sketch_history(scan: &ContainerScan, ir: &mut CadIr) {
    let links = scan
        .features
        .section_transforms
        .iter()
        .filter(|transform| {
            unique_feature_section_transform(
                &scan.features.section_transforms,
                transform.definition_id,
                transform.offset,
            )
            .is_some()
        })
        .filter_map(|transform| {
            let owner = IrFeatureId(format!("creo:model:feature#{}", transform.feature_id?));
            let definition =
                unique_feature_definition_for_transform(&scan.features.definitions, transform)?;
            let sketch = model_sketch_id(scan, definition);
            let sketch_feature = section_owner_feature_id(scan, transform.definition_id, &sketch);
            ir.model
                .features
                .iter()
                .any(|feature| feature.id == sketch_feature)
                .then_some((owner, sketch_feature))
        })
        .collect::<Vec<_>>();
    for (owner, sketch_feature) in links {
        let Some(feature) = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.id == owner)
        else {
            continue;
        };
        if !feature.dependencies.contains(&sketch_feature) {
            feature.dependencies.push(sketch_feature);
        }
    }
}

fn surface_kind_for_geometry(geometry: &SurfaceGeometry) -> Option<crate::surface::SurfaceKind> {
    match geometry {
        SurfaceGeometry::Plane { .. } => Some(crate::surface::SurfaceKind::Plane),
        SurfaceGeometry::Cylinder { .. } => Some(crate::surface::SurfaceKind::Cylinder),
        SurfaceGeometry::Cone { .. } => Some(crate::surface::SurfaceKind::Cone),
        SurfaceGeometry::Sphere { .. } | SurfaceGeometry::Torus { .. } => {
            Some(crate::surface::SurfaceKind::TorusOrSphere)
        }
        SurfaceGeometry::Nurbs(_) => Some(crate::surface::SurfaceKind::Spline),
        SurfaceGeometry::Transformed { basis, .. } => surface_kind_for_geometry(basis),
        SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

fn generated_surface_id_for_feature(
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
    source_entity_id: u32,
) -> Option<u32> {
    let mut matches = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| {
            table
                .entries
                .iter()
                .filter(|entry| entry.source_entity_id == Some(source_entity_id))
                .filter(|entry| table.surface_ids.contains(&entry.entity_id))
                .map(|entry| entry.entity_id)
        });
    let surface_id = matches.next()?;
    matches.next().is_none().then_some(surface_id)
}

fn section_entity_is_generated_profile(
    segment_table_complete: bool,
    feature_id: Option<u32>,
    source_entity_id: u32,
    expected_kinds: &[crate::surface::SurfaceKind],
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> bool {
    if !segment_table_complete {
        return false;
    }
    let Some(feature_id) = feature_id else {
        return false;
    };
    let direct = generated_surface_id_for_feature(tables, feature_id, source_entity_id)
        .is_some_and(|surface_id| {
            crate::surface::unique_surface_row(rows, surface_id).is_some_and(|row| {
                row.feature_id == feature_id && expected_kinds.contains(&row.kind)
            })
        });
    if direct {
        return true;
    }
    if !expected_kinds.contains(&crate::surface::SurfaceKind::Cylinder) {
        return false;
    }
    let mut blind_cylinders = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .filter_map(|table| {
            let [rowless_cap, cap, profile, cylinder] = table.entries.as_slice() else {
                return None;
            };
            ([
                rowless_cap.class_id,
                cap.class_id,
                profile.class_id,
                cylinder.class_id,
            ] == [204, 203, 200, 200]
                && profile.source_entity_id == Some(source_entity_id)
                && cylinder.source_entity_id.is_none()
                && table.non_surface_entity_ids.contains(&profile.entity_id)
                && crate::surface::unique_surface_row(rows, cylinder.entity_id).is_some_and(
                    |row| {
                        row.feature_id == feature_id
                            && row.kind == crate::surface::SurfaceKind::Cylinder
                    },
                ))
            .then_some(cylinder.entity_id)
        });
    blind_cylinders.next().is_some() && blind_cylinders.next().is_none()
}

fn section_generated_profile_surface_kinds(
    geometry: &SketchGeometry,
) -> Option<&'static [crate::surface::SurfaceKind]> {
    match geometry {
        SketchGeometry::Line { .. } => Some(&[crate::surface::SurfaceKind::Plane]),
        SketchGeometry::Arc { .. } | SketchGeometry::Circle { .. } => {
            Some(&[crate::surface::SurfaceKind::Cylinder])
        }
        SketchGeometry::Nurbs { .. } => Some(&[
            crate::surface::SurfaceKind::Spline,
            crate::surface::SurfaceKind::Extrusion,
        ]),
        _ => None,
    }
}

fn ordered_analytic_surface_id_for_feature(
    surface_rows: &[crate::surface::SurfaceRow],
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
    order: &crate::feature::FeatureOrderTable,
    external_id: u32,
    geometry: &SurfaceGeometry,
) -> Option<u32> {
    order.internal_id(external_id)?;
    analytic_surface_id_for_feature(surface_rows, tables, feature_id, external_id, geometry)
}

fn analytic_surface_id_for_feature(
    surface_rows: &[crate::surface::SurfaceRow],
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
    external_id: u32,
    geometry: &SurfaceGeometry,
) -> Option<u32> {
    let surface_id = generated_surface_id_for_feature(tables, feature_id, external_id)?;
    let expected_kind = surface_kind_for_geometry(geometry)?;
    crate::surface::unique_surface_row(surface_rows, surface_id)
        .is_some_and(|row| row.feature_id == feature_id && row.kind == expected_kind)
        .then_some(surface_id)
}

fn ordered_family_surface_bindings_for_feature(
    surface_rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    order: &crate::feature::FeatureOrderTable,
    external_ids: impl IntoIterator<Item = u32>,
    expected_kind: crate::surface::SurfaceKind,
) -> BTreeMap<u32, u32> {
    let mut bindings = BTreeMap::new();
    let mut bound_surfaces = BTreeSet::new();
    for external_id in external_ids {
        if order.internal_id(external_id).is_none() {
            return BTreeMap::new();
        }
        let Some(surface_id) = generated_surface_id_for_feature(tables, feature_id, external_id)
        else {
            return BTreeMap::new();
        };
        if !crate::surface::unique_surface_row(surface_rows, surface_id)
            .is_some_and(|row| row.feature_id == feature_id && row.kind == expected_kind)
            || !bound_surfaces.insert(surface_id)
        {
            return BTreeMap::new();
        }
        bindings.insert(external_id, surface_id);
    }
    bindings
}

fn profile_segment_ids(
    definition_id: u32,
    segments: &[crate::feature::FeatureSegment],
    profiles: &[Vec<SketchEntityUse>],
) -> BTreeSet<u32> {
    segments
        .iter()
        .filter(|segment| {
            let entity_id = SketchEntityId(format!(
                "creo:featdefs:sketch_entity#{definition_id}:{}",
                segment.external_id
            ));
            profiles
                .iter()
                .flatten()
                .any(|entity_use| entity_use.entity == entity_id)
        })
        .map(|segment| segment.external_id)
        .collect()
}

fn transfer_resolved_revolution_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Revolve) {
            continue;
        }
        if unique_feature_revolution_extent_kind(&scan.features.revolution_extents, feature_id)
            .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let Some(axis) = resolved_revolution_axis(definition, transform) else {
            continue;
        };
        let points = resolved_section_points(definition);
        let mut generating_ids = definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        let sketch_id = SketchId(format!("creo:model:sketch#{}", definition.id));
        if let Some(sketch) = ir
            .model
            .sketches
            .iter()
            .find(|sketch| sketch.id == sketch_id)
        {
            let segments = complete_section_segment_rows(definition).to_vec();
            generating_ids.extend(profile_segment_ids(
                definition.id,
                &segments,
                &sketch.profiles,
            ));
        }
        let arc_bindings = definition
            .order_table
            .as_ref()
            .map_or_else(BTreeMap::new, |order| {
                ordered_family_surface_bindings_for_feature(
                    &scan.surfaces.rows,
                    feature_id,
                    &scan.features.entity_tables,
                    order,
                    complete_section_segment_rows(definition)
                        .iter()
                        .filter(|segment| {
                            generating_ids.contains(&segment.external_id)
                                && segment.kind == crate::feature::FeatureSegmentKind::Arc
                        })
                        .map(|segment| segment.external_id),
                    crate::surface::SurfaceKind::TorusOrSphere,
                )
            });
        let spline_bindings = definition
            .order_table
            .as_ref()
            .map_or_else(BTreeMap::new, |order| {
                ordered_family_surface_bindings_for_feature(
                    &scan.surfaces.rows,
                    feature_id,
                    &scan.features.entity_tables,
                    order,
                    definition
                        .saved_section
                        .iter()
                        .flat_map(|saved| &saved.entities)
                        .filter_map(|entity| match entity {
                            crate::feature::FeatureSavedEntity::Spline(spline) => {
                                order.external_id(spline.entity_id?)
                            }
                            _ => None,
                        }),
                    crate::surface::SurfaceKind::Spline,
                )
            });
        for segment in complete_section_segment_rows(definition)
            .iter()
            .filter(|segment| generating_ids.contains(&segment.external_id))
        {
            let Some(geometry) = resolved_section_segment_geometry(definition, &points, segment)
            else {
                continue;
            };
            let Some(surface) = revolved_section_surface(transform, &geometry, axis) else {
                continue;
            };
            let native_surface = match segment.kind {
                crate::feature::FeatureSegmentKind::Line => {
                    definition.order_table.as_ref().and_then(|order| {
                        ordered_analytic_surface_id_for_feature(
                            &scan.surfaces.rows,
                            &scan.features.entity_tables,
                            feature_id,
                            order,
                            segment.external_id,
                            &surface,
                        )
                    })
                }
                crate::feature::FeatureSegmentKind::Arc => {
                    arc_bindings.get(&segment.external_id).copied()
                }
                crate::feature::FeatureSegmentKind::Point => None,
            };
            let surface_id = native_surface.map_or_else(
                || {
                    SurfaceId(format!(
                        "creo:feature:revolution_surface#{feature_id}:segment{}",
                        segment.external_id
                    ))
                },
                |id| SurfaceId(format!("creo:visibgeom:surface#{id}")),
            );
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                segment.offset as u64,
                "evaluated_analytic_revolution_surface",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: surface_id,
                geometry: surface,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: native_surface.map_or_else(
                        || {
                            format!(
                                "FeatDefs:revolution#{feature_id}:segment{}",
                                segment.external_id
                            )
                        },
                        |id| format!("VisibGeom:{id}"),
                    ),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
        if let Some(order) = definition.order_table.as_ref() {
            for (internal_id, section_geometry, offset) in definition
                .saved_section
                .iter()
                .flat_map(|saved| &saved.entities)
                .filter_map(saved_section_entity_geometry)
            {
                let Some(external_id) = order.external_id(internal_id) else {
                    continue;
                };
                let Some(surface) = revolved_section_surface(transform, &section_geometry, axis)
                else {
                    continue;
                };
                let Some(native_surface) = ordered_analytic_surface_id_for_feature(
                    &scan.surfaces.rows,
                    &scan.features.entity_tables,
                    feature_id,
                    order,
                    external_id,
                    &surface,
                ) else {
                    continue;
                };
                let surface_id = SurfaceId(format!("creo:visibgeom:surface#{native_surface}"));
                if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                    continue;
                }
                annotate(
                    annotations,
                    &surface_id,
                    "FeatDefs",
                    offset as u64,
                    "evaluated_saved_analytic_revolution_surface",
                    Exactness::Derived,
                );
                ir.model.surfaces.push(Surface {
                    id: surface_id,
                    geometry: surface,
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!("VisibGeom:{native_surface}"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
                transferred += 1;
            }
        }
        for spline in definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(|entity| match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
        {
            let suffix = spline.entity_id.map_or_else(
                || format!("offset{}", spline.offset),
                |entity_id| entity_id.to_string(),
            );
            let curve_id = CurveId(format!(
                "creo:featdefs:saved_spline_curve#{}:{suffix}",
                definition.id
            ));
            let Some(CurveGeometry::Nurbs(directrix)) = ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == curve_id)
                .map(|curve| &curve.geometry)
            else {
                continue;
            };
            let Some(surface) = revolved_nurbs_surface(directrix, axis) else {
                continue;
            };
            let native_surface = definition
                .order_table
                .as_ref()
                .and_then(|order| order.external_id(spline.entity_id?))
                .and_then(|external_id| spline_bindings.get(&external_id).copied());
            let Some(native_surface) = native_surface else {
                continue;
            };
            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{native_surface}"));
            let procedural_id = ProceduralSurfaceId(format!(
                "creo:feature:revolution_construction#{feature_id}:{suffix}"
            ));
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                spline.offset as u64,
                "evaluated_revolution_surface",
                Exactness::Derived,
            );
            annotate(
                annotations,
                &procedural_id,
                "FeatDefs",
                spline.offset as u64,
                "revolution_surface_construction",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Nurbs(surface),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{native_surface}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: surface_id,
                definition: ProceduralSurfaceDefinition::Revolution {
                    directrix: curve_id,
                    axis_origin: axis.origin,
                    axis_direction: axis.direction,
                    angular_interval: [0.0, std::f64::consts::TAU],
                    parameter_interval: [
                        *directrix.knots.first().expect("validated spline knots"),
                        *directrix.knots.last().expect("validated spline knots"),
                    ]
                    .into(),
                    transposed: false,
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            transferred += 1;
        }
    }
    transferred
}

fn transfer_resolved_revolution_vertex_orbit_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut pending = Vec::new();
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Revolve) {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let Some(axis) = resolved_revolution_axis(definition, transform) else {
            continue;
        };
        let sketch_id = SketchId(format!("creo:model:sketch#{}", definition.id));
        for (profile_index, vertices) in connected_sketch_profile_vertices(ir, &sketch_id) {
            for (vertex_index, point) in vertices.iter().enumerate() {
                let Some(geometry) = revolved_section_circle(transform, *point, axis) else {
                    continue;
                };
                pending.push((
                    CurveId(format!(
                        "creo:feature:revolution_vertex_orbit#{feature_id}:profile{profile_index}:vertex{vertex_index}"
                    )),
                    geometry,
                    transform.offset,
                    format!(
                        "FeatDefs:revolution#{feature_id}:profile{profile_index}:vertex{vertex_index}"
                    ),
                ));
            }
        }
    }
    let mut transferred = 0;
    for (id, geometry, offset, object_id) in pending {
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "FeatDefs",
            offset as u64,
            "evaluated_revolution_profile_vertex_orbit",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

fn transfer_resolved_extrusion_vertex_orbit_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut pending = Vec::new();
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_linear_extrusion(scan, feature_id) {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let sketch_id = model_sketch_id(scan, definition);
        for (profile_index, vertices) in connected_sketch_profile_vertices(ir, &sketch_id) {
            for (vertex_index, point) in vertices.iter().enumerate() {
                let Some(geometry) = extruded_section_line(transform, *point) else {
                    continue;
                };
                pending.push((
                    CurveId(format!(
                        "creo:feature:extrusion_vertex_orbit#{feature_id}:profile{profile_index}:vertex{vertex_index}"
                    )),
                    geometry,
                    transform.offset,
                    format!(
                        "FeatDefs:extrusion#{feature_id}:profile{profile_index}:vertex{vertex_index}"
                    ),
                ));
            }
        }
    }
    let mut transferred = 0;
    for (id, geometry, offset, object_id) in pending {
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "FeatDefs",
            offset as u64,
            "evaluated_extrusion_profile_vertex_orbit",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

fn feature_dimension_parameter_id(sketch: &SketchId, external_id: u32) -> ParameterId {
    ParameterId(format!(
        "creo:featdefs:parameter#{}:{external_id}",
        sketch_identity_scope(sketch),
    ))
}

fn feature_dimension_parameter_row_id(
    sketch: &SketchId,
    external_id: u32,
    occurrence: Option<usize>,
) -> ParameterId {
    occurrence.map_or_else(
        || feature_dimension_parameter_id(sketch, external_id),
        |occurrence| {
            ParameterId(format!(
                "creo:featdefs:parameter#{}:{external_id}:{}",
                sketch_identity_scope(sketch),
                occurrence + 1
            ))
        },
    )
}

fn resolved_feature_dimension_parameter<'a>(
    sketch: &SketchId,
    table: &'a crate::feature::FeatureDimensionTable,
    ordinal: usize,
) -> Option<(&'a crate::feature::FeatureDimension, ParameterId)> {
    feature_dimension_table_complete(table).then_some(())?;
    let dimension = table.rows.get(ordinal)?;
    (table
        .rows
        .iter()
        .filter(|candidate| candidate.external_id == dimension.external_id)
        .count()
        == 1)
        .then(|| {
            (
                dimension,
                feature_dimension_parameter_id(sketch, dimension.external_id),
            )
        })
}

fn feature_dimension_table_complete(table: &crate::feature::FeatureDimensionTable) -> bool {
    usize::try_from(table.declared_count).ok() == Some(table.rows.len())
}

fn feature_dimension_display(dimension_type: u32) -> Option<DimensionDisplay> {
    match dimension_type {
        0x03 => Some(DimensionDisplay::Radius),
        0x04 => Some(DimensionDisplay::Diameter),
        _ => None,
    }
}

fn feature_relation_table_complete(table: &crate::feature::FeatureRelationTable) -> bool {
    table
        .declared_count
        .checked_sub(2)
        .and_then(|count| usize::try_from(count).ok())
        == Some(table.rows.len())
}

fn feature_solver_table_complete(
    header: Option<&crate::feature::FeatureSolverTableHeader>,
    row_count: usize,
) -> bool {
    header.map_or(row_count == 0, |header| {
        usize::try_from(header.declared_count).ok() == Some(row_count)
    })
}

fn feature_skamp_table_complete(table: &crate::feature::FeatureRelationTable) -> bool {
    feature_solver_table_complete(table.skamp_header.as_ref(), table.skamps.len())
}

fn feature_dimension_parameter_layout(
    keys: &[(SketchId, u32)],
) -> Option<Vec<(u32, String, Option<usize>)>> {
    let mut name_counts = BTreeMap::new();
    let mut local_counts = BTreeMap::new();
    for (sketch, external_id) in keys {
        *name_counts
            .entry((sketch.clone(), *external_id))
            .or_insert(0usize) += 1;
    }
    for key in keys {
        *local_counts.entry(key.clone()).or_insert(0usize) += 1;
    }
    let mut next_ordinals = BTreeMap::<SketchId, u32>::new();
    let mut local_occurrences = BTreeMap::new();
    keys.iter()
        .map(|key @ (sketch, external_id)| {
            let ordinal = next_ordinals.entry(sketch.clone()).or_default();
            let assigned = *ordinal;
            *ordinal = ordinal.checked_add(1)?;
            let occurrence = (local_counts[key] > 1).then(|| {
                let occurrence = local_occurrences.entry(key.clone()).or_insert(0usize);
                let assigned = *occurrence;
                *occurrence += 1;
                assigned
            });
            let name = if name_counts[&(sketch.clone(), *external_id)] == 1 {
                format!("d{external_id}")
            } else if let Some(occurrence) = occurrence {
                format!(
                    "d{}_{}_{}",
                    sketch_identity_scope(sketch),
                    external_id,
                    occurrence + 1
                )
            } else {
                format!("d{}_{}", sketch_identity_scope(sketch), external_id)
            };
            Some((assigned, name, occurrence))
        })
        .collect()
}

fn transfer_feature_dimensions(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> (usize, BTreeMap<String, ParameterId>) {
    let feature_ids = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<BTreeSet<_>>();
    let mut candidates = Vec::new();
    for definition in &scan.features.definitions {
        let sketch = model_sketch_id(scan, definition);
        let owner = section_owner_feature_id(scan, definition.id, &sketch);
        if !feature_ids.contains(&owner) {
            continue;
        }
        let Some(table) = &definition.dimensions else {
            continue;
        };
        for (source_ordinal, dimension) in table.rows.iter().enumerate() {
            candidates.push((sketch.clone(), definition, source_ordinal, dimension));
        }
    }
    candidates.sort_by_key(|(_, definition, source_ordinal, _)| {
        (definition.offset, definition.id, *source_ordinal)
    });
    let keys = candidates
        .iter()
        .map(|(sketch, _, _, dimension)| (sketch.clone(), dimension.external_id))
        .collect::<Vec<_>>();
    let Some(layout) = feature_dimension_parameter_layout(&keys) else {
        return (0, BTreeMap::new());
    };
    let unique_external_ids = keys
        .iter()
        .fold(BTreeMap::new(), |mut counts, (_, external_id)| {
            *counts.entry(*external_id).or_insert(0usize) += 1;
            counts
        });
    let transferred = layout.len();
    let mut relation_parameters = BTreeMap::new();
    for ((sketch, definition, source_ordinal, dimension), (ordinal, name, occurrence)) in
        candidates.into_iter().zip(layout)
    {
        let owner_id = section_owner_feature_id(scan, definition.id, &sketch);
        let id = feature_dimension_parameter_row_id(&sketch, dimension.external_id, occurrence);
        if unique_external_ids[&dimension.external_id] == 1 {
            relation_parameters.insert(format!("d{}", dimension.external_id), id.clone());
        }
        annotate(
            annotations,
            &id.0,
            "FeatDefs",
            dimension.offset as u64,
            "section_dimension",
            Exactness::Derived,
        );
        let mut properties = BTreeMap::from([
            ("definition_id".to_string(), definition.id.to_string()),
            ("source_ordinal".to_string(), source_ordinal.to_string()),
            ("external_id".to_string(), dimension.external_id.to_string()),
            (
                "dimension_type".to_string(),
                dimension.dimension_type.to_string(),
            ),
            (
                "direction_byte".to_string(),
                dimension.direction_byte.to_string(),
            ),
        ]);
        if let Some(auxiliary) = dimension.auxiliary_value {
            properties.insert("auxiliary_value".to_string(), auxiliary.to_string());
        }
        if dimension.value.is_none() {
            properties.insert("value_state".to_string(), "unresolved".to_string());
        }
        if let Some(token) = &dimension.unresolved_value_token {
            let encoding = match token.as_slice() {
                [0x00, _, _] => Some("three_byte_placeholder"),
                [0x01, _, _, _] => Some("four_byte_placeholder"),
                _ => None,
            };
            if let Some(encoding) = encoding {
                properties.insert("value_encoding".to_string(), encoding.to_string());
                let value_token = token.iter().fold(
                    String::with_capacity(token.len() * 2),
                    |mut encoded, byte| {
                        write!(encoded, "{byte:02x}").expect("writing to a string cannot fail");
                        encoded
                    },
                );
                properties.insert("value_token".to_string(), value_token);
            }
        }
        let expression = dimension
            .value
            .map_or_else(String::new, |value| value.to_string());
        let value = dimension.value.map(|value| match dimension.value_unit {
            crate::feature::DimensionUnit::Radians => ParameterValue::Angle(Angle(value)),
            crate::feature::DimensionUnit::Millimeters => ParameterValue::Length(Length(value)),
            crate::feature::DimensionUnit::SchemaDefined => ParameterValue::Real(value),
        });
        ir.model.parameters.push(DesignParameter {
            id: id.clone(),
            owner: Some(owner_id.clone()),
            ordinal,
            name,
            expression,
            display: feature_dimension_display(dimension.dimension_type),
            value,
            dependencies: Vec::new(),
            properties,
            pmi: None,
            native_ref: Some(feature_sketch_record_id_in_scan(scan, definition)),
        });
        if let Some(feature) = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.id == owner_id)
        {
            feature
                .source_content
                .push(FeatureSourceContent::Parameter(id));
        }
    }
    (transferred, relation_parameters)
}

fn feature_output_bodies(scan: &ContainerScan, ir: &CadIr, feature_id: u32) -> Vec<BodyId> {
    let affected_geometry = agreed_feature_affected_ids(
        &scan.features.affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Geometry,
    );
    let generated_surfaces = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .map(|row| SurfaceId(format!("creo:visibgeom:surface#{}", row.id)))
        .chain(
            scan.features
                .entity_tables
                .iter()
                .filter(|table| table.feature_id == Some(feature_id))
                .flat_map(|table| &table.surface_ids)
                .map(|surface_id| SurfaceId(format!("creo:visibgeom:surface#{surface_id}"))),
        )
        .chain(
            affected_geometry
                .into_iter()
                .flatten()
                .map(|surface_id| SurfaceId(format!("creo:visibgeom:surface#{surface_id}"))),
        );
    let mut outputs = evaluated_sweep_output_bodies(ir, feature_id);
    let edge_outputs = match feature_edge_selection(scan, ir, feature_id) {
        Some(EdgeSelection::Resolved { edges, .. }) => bodies_containing_edges(ir, &edges),
        _ => Vec::new(),
    };
    if edge_outputs.is_empty() {
        for surface in generated_surfaces {
            for face in ir.model.faces.iter().filter(|face| face.surface == surface) {
                let Some(shell) = ir.model.shells.iter().find(|shell| shell.id == face.shell)
                else {
                    continue;
                };
                let Some(region) = ir
                    .model
                    .regions
                    .iter()
                    .find(|region| region.id == shell.region)
                else {
                    continue;
                };
                if !outputs.contains(&region.body) {
                    outputs.push(region.body.clone());
                }
            }
        }
    } else {
        for body in edge_outputs {
            if !outputs.contains(&body) {
                outputs.push(body);
            }
        }
    }
    outputs
}

fn bodies_containing_edges(ir: &CadIr, edges: &[EdgeId]) -> Vec<BodyId> {
    let selected = edges.iter().collect::<BTreeSet<_>>();
    let mut shell_ids = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| selected.contains(&coedge.edge))
        .filter_map(|coedge| {
            let lp = ir
                .model
                .loops
                .iter()
                .find(|lp| lp.id == coedge.owner_loop)?;
            ir.model
                .faces
                .iter()
                .find(|face| face.id == lp.face)
                .map(|face| face.shell.clone())
        })
        .collect::<BTreeSet<_>>();
    shell_ids.extend(
        ir.model
            .shells
            .iter()
            .filter(|shell| shell.wire_edges.iter().any(|edge| selected.contains(edge)))
            .map(|shell| shell.id.clone()),
    );
    ir.model
        .shells
        .iter()
        .filter(|shell| shell_ids.contains(&shell.id))
        .filter_map(|shell| {
            let region = ir
                .model
                .regions
                .iter()
                .find(|region| region.id == shell.region)?;
            ir.model
                .bodies
                .iter()
                .any(|body| body.id == region.body)
                .then(|| region.body.clone())
        })
        .fold(Vec::new(), |mut bodies, body| {
            if !bodies.contains(&body) {
                bodies.push(body);
            }
            bodies
        })
}

fn evaluated_sweep_output_bodies(ir: &CadIr, feature_id: u32) -> Vec<BodyId> {
    ["extrusion", "revolution"]
        .into_iter()
        .map(|family| BodyId(format!("creo:feature:{family}#{feature_id}:body")))
        .filter(|id| ir.model.bodies.iter().any(|body| body.id == *id))
        .collect()
}

fn evaluated_sweep_body_kind(ir: &CadIr, family: &str, feature_id: u32) -> Option<BodyKind> {
    let id = BodyId(format!("creo:feature:{family}#{feature_id}:body"));
    ir.model
        .bodies
        .iter()
        .find(|body| body.id == id)
        .map(|body| body.kind)
}

fn feature_field_text(value: &crate::feature::FeatureFieldValue) -> Option<String> {
    match value {
        crate::feature::FeatureFieldValue::Empty => Some("empty".to_string()),
        crate::feature::FeatureFieldValue::CompactInt(value) => Some(value.to_string()),
        crate::feature::FeatureFieldValue::CompactIntArray(values) => Some(
            values
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        crate::feature::FeatureFieldValue::EntityReference {
            entity_id,
            terminated,
        } => Some(format!(
            "entity:{entity_id}{}",
            if *terminated { ":terminated" } else { "" }
        )),
        crate::feature::FeatureFieldValue::ScalarArray {
            decoded_values: Some(values),
            ..
        } => Some(
            values
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        crate::feature::FeatureFieldValue::ScalarArray {
            decoded_values: None,
            ..
        }
        | crate::feature::FeatureFieldValue::Raw(_) => None,
    }
}

fn insert_feature_parameter(parameters: &mut BTreeMap<String, String>, base: &str, value: String) {
    if let std::collections::btree_map::Entry::Vacant(entry) = parameters.entry(base.to_string()) {
        entry.insert(value);
        return;
    }
    let mut occurrence = 2;
    loop {
        let name = format!("{base}#{occurrence}");
        if let std::collections::btree_map::Entry::Vacant(entry) = parameters.entry(name) {
            entry.insert(value);
            return;
        }
        occurrence += 1;
    }
}

fn feature_parameters(scan: &ContainerScan, feature_id: u32) -> BTreeMap<String, String> {
    let mut parameters = BTreeMap::new();
    for field in scan
        .features
        .choice_fields
        .iter()
        .filter(|field| field.feature_id == feature_id)
    {
        let Some(value) = feature_field_text(&field.value) else {
            continue;
        };
        insert_feature_parameter(
            &mut parameters,
            &format!("choice.{}.{}", field.choice_label, field.name),
            value,
        );
    }
    for affected in scan
        .features
        .affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        let name = match affected.kind {
            crate::feature::AffectedIdKind::Geometry => "affected_geometry_ids",
            crate::feature::AffectedIdKind::Edges => "affected_edge_ids",
            crate::feature::AffectedIdKind::StrongParents => "strong_parent_feature_ids",
            crate::feature::AffectedIdKind::Parents => "parent_feature_ids",
            crate::feature::AffectedIdKind::Contours => "contour_ids",
        };
        insert_feature_parameter(
            &mut parameters,
            name,
            affected
                .ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    for affected in scan
        .features
        .replay_affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        insert_feature_parameter(
            &mut parameters,
            "replay_affected_geometry_ids",
            affected
                .geometry_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_affected_edge_ids",
            affected
                .edge_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_geometry_extent",
            match affected.geometry_extent {
                crate::feature::ReplayExtentSource::Explicit => "explicit",
                crate::feature::ReplayExtentSource::Inherited => "inherited",
            }
            .to_string(),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_edge_extent",
            match affected.edge_extent {
                crate::feature::ReplayExtentSource::Explicit => "explicit",
                crate::feature::ReplayExtentSource::Inherited => "inherited",
            }
            .to_string(),
        );
    }
    for direction in scan
        .features
        .loop_restore_directions
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        let name = match direction.lane {
            crate::feature::LoopRestoreDirectionLane::Primary => "direction",
            crate::feature::LoopRestoreDirectionLane::Secondary => "direction2",
        };
        insert_feature_parameter(
            &mut parameters,
            &format!("loop_restore.{name}"),
            direction.value.to_string(),
        );
    }
    if let Some(extent) =
        unique_feature_revolution_extent_kind(&scan.features.revolution_extents, feature_id)
    {
        parameters.insert(
            "revolution_extent".to_string(),
            match extent {
                crate::feature::FeatureRevolutionExtentKind::FullTurn => "full_turn",
            }
            .to_string(),
        );
    }
    for table in scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
    {
        for entry in &table.entries {
            let Some(source_entity_id) = entry.source_entity_id else {
                continue;
            };
            insert_feature_parameter(
                &mut parameters,
                &format!(
                    "generated_entity.{}.source_section_entity_id",
                    entry.entity_id
                ),
                source_entity_id.to_string(),
            );
            insert_feature_parameter(
                &mut parameters,
                &format!("generated_entity.{}.entry_class", entry.entity_id),
                entry.class_id.to_string(),
            );
        }
    }
    let owned_definitions = scan
        .features
        .definitions
        .iter()
        .filter(|definition| definition.owner_feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    if let [definition] = owned_definitions.as_slice() {
        parameters.insert(
            "sketch_segment_count".to_string(),
            definition
                .segments
                .as_ref()
                .map_or(0, |segments| segments.rows.len())
                .to_string(),
        );
        parameters.insert(
            "dimension_count".to_string(),
            definition
                .dimensions
                .as_ref()
                .map_or(0, |dimensions| dimensions.rows.len())
                .to_string(),
        );
    }
    for transform in scan
        .features
        .section_transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
    {
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        insert_feature_parameter(
            &mut parameters,
            "profile_sketch",
            model_sketch_id(scan, definition).0,
        );
        if feature_recipe(scan, feature_id) == Some(crate::feature::FeatureRecipeKind::Extrude) {
            insert_feature_parameter(
                &mut parameters,
                "sweep_direction",
                transform
                    .normal
                    .iter()
                    .map(f64::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
    }
    parameters
}

fn schema_operation_kind(schema_class: u32) -> Option<&'static str> {
    match schema_class {
        911 => Some("Hole"),
        913 => Some("Round"),
        914 => Some("Chamfer"),
        916 => Some("Cut"),
        917 => Some("Protrusion"),
        923 => Some("Datum Plane"),
        926 => Some("Section"),
        927 => Some("Draft"),
        946 => Some("Surface Merge"),
        _ => None,
    }
}

fn feature_reference_name(scan: &ContainerScan, feature_id: u32) -> Option<&str> {
    let mut records = scan
        .features
        .reference_names
        .iter()
        .filter(|record| record.feature_id == feature_id);
    let record = records.next()?;
    records
        .all(|candidate| candidate.name_bytes.as_slice() == record.name_bytes.as_slice())
        .then_some(record.name.as_str())
}

fn owned_section_feature_id(scan: &ContainerScan, definition_id: u32) -> Option<u32> {
    let definitions = scan
        .features
        .definitions
        .iter()
        .filter(|definition| definition.id == definition_id)
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return None;
    };
    let rows = scan
        .features
        .rows
        .iter()
        .filter(|row| {
            row.root_schema_class == Some(926)
                && definition.offset >= row.body_offset
                && definition.offset < row.body_offset.saturating_add(row.body.len())
        })
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return None;
    };
    Some(row.feature_id)
}

fn section_definition_for_history_feature(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<&crate::feature::FeatureDefinition> {
    let rows = scan
        .features
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id && row.root_schema_class == Some(926))
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return None;
    };
    let definitions = scan
        .features
        .definitions
        .iter()
        .filter(|definition| {
            definition.offset >= row.body_offset
                && definition.offset < row.body_offset.saturating_add(row.body.len())
        })
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return None;
    };
    Some(*definition)
}

fn feature_source_properties(scan: &ContainerScan, feature_id: u32) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    if let Some(recipe) = current_feature_recipe(&scan.features.operations, feature_id) {
        properties.insert("recipe".to_string(), recipe.name().to_string());
    }
    let schema_class = feature_schema_class(scan, feature_id);
    if let Some(schema_class) = schema_class {
        properties.insert(
            "featdefs_schema_class".to_string(),
            schema_class.to_string(),
        );
    }
    let row_schema_classes = feature_row_schema_classes(scan, feature_id);
    if !row_schema_classes.is_empty() {
        properties.insert(
            "featdefs_row_schema_classes".to_string(),
            row_schema_classes
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    if schema_class.is_none() && !row_schema_classes.is_empty() {
        properties.insert("featdefs_schema_state".to_string(), "ambiguous".to_string());
    }
    properties
}

fn feature_dependencies(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    prototype_dependencies: &BTreeMap<u32, Vec<u32>>,
) -> Vec<IrFeatureId> {
    agreed_feature_parent_ids(&scan.features.affected_ids, feature_id)
        .into_iter()
        .chain(current_feature_recipe_parent(
            &scan.features.operations,
            feature_id,
        ))
        .chain(
            prototype_dependencies
                .get(&feature_id)
                .into_iter()
                .flatten()
                .copied(),
        )
        .chain(feature_entity_dependencies(
            &scan.features.entity_tables,
            feature_id,
        ))
        .filter_map(|dependency| {
            let id = IrFeatureId(format!("creo:model:feature#{dependency}"));
            ir.model
                .features
                .iter()
                .any(|feature| feature.id == id)
                .then_some(id)
        })
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

fn feature_entity_dependencies(
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
) -> Vec<u32> {
    let producers = tables
        .iter()
        .filter_map(|table| table.feature_id.map(|owner| (owner, table)))
        .flat_map(|(owner, table)| {
            table
                .entries
                .iter()
                .filter(|entry| entry.source_entity_id.is_some())
                .map(move |entry| (entry.entity_id, owner))
        })
        .fold(
            BTreeMap::<u32, BTreeSet<u32>>::new(),
            |mut owners, (entity, owner)| {
                owners.entry(entity).or_default().insert(owner);
                owners
            },
        );
    tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 100)
        .flat_map(|table| table.entries.iter())
        .filter_map(|entry| {
            let owners = producers.get(&entry.entity_id)?;
            let mut owners = owners.iter().copied();
            let owner = owners.next()?;
            if owners.next().is_some() {
                return None;
            }
            (owner != feature_id).then_some(owner)
        })
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

fn agreed_feature_affected_ids(
    records: &[crate::feature::FeatureAffectedIds],
    feature_id: u32,
    kind: crate::feature::AffectedIdKind,
) -> Option<&[u32]> {
    let mut matches = records
        .iter()
        .filter(|record| record.feature_id == feature_id && record.kind == kind);
    let ids = matches.next()?.ids.as_slice();
    matches
        .all(|record| record.ids.as_slice() == ids)
        .then_some(ids)
}

fn has_feature_affected_ids(
    records: &[crate::feature::FeatureAffectedIds],
    feature_id: u32,
    kind: crate::feature::AffectedIdKind,
) -> bool {
    records
        .iter()
        .any(|record| record.feature_id == feature_id && record.kind == kind)
}

fn agreed_feature_parent_ids(
    records: &[crate::feature::FeatureAffectedIds],
    feature_id: u32,
) -> Vec<u32> {
    let mut emitted_kinds = Vec::new();
    let mut ids = Vec::new();
    for record in records.iter().filter(|record| {
        record.feature_id == feature_id
            && matches!(
                record.kind,
                crate::feature::AffectedIdKind::StrongParents
                    | crate::feature::AffectedIdKind::Parents
            )
    }) {
        if emitted_kinds.contains(&record.kind) {
            continue;
        }
        emitted_kinds.push(record.kind);
        if let Some(agreed) = agreed_feature_affected_ids(records, feature_id, record.kind) {
            ids.extend_from_slice(agreed);
        }
    }
    ids
}

fn surface_prototype_feature_dependencies(scan: &ContainerScan) -> BTreeMap<u32, Vec<u32>> {
    let mut dependencies = BTreeMap::new();
    for (prototype, row, _) in unique_surface_prototype_associations(scan) {
        let mut fields = prototype
            .parameters
            .iter()
            .filter(|field| field.name == "parent_feats");
        let Some(field) = fields.next() else {
            continue;
        };
        if fields.next().is_some() {
            continue;
        }
        let crate::surface::SurfaceNamedValue::CompactIntArray(consumers) = &field.value else {
            continue;
        };
        add_surface_prototype_feature_dependencies(&mut dependencies, row.feature_id, consumers);
    }
    dependencies
}

fn add_surface_prototype_feature_dependencies(
    dependencies: &mut BTreeMap<u32, Vec<u32>>,
    producer: u32,
    consumers: &[u32],
) {
    for &consumer in consumers {
        if consumer == 0 || consumer == producer {
            continue;
        }
        let producers = dependencies.entry(consumer).or_default();
        if !producers.contains(&producer) {
            producers.push(producer);
        }
    }
}

fn agreed_feature_replay_geometry_ids(
    records: &[crate::feature::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Option<&[u32]> {
    let mut matches = records
        .iter()
        .filter(|record| record.feature_id == feature_id);
    let ids = matches.next()?.geometry_ids.as_slice();
    matches
        .all(|record| record.geometry_ids.as_slice() == ids)
        .then_some(ids)
}

fn agreed_feature_replay_edge_ids(
    records: &[crate::feature::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Option<&[u32]> {
    let mut matches = records
        .iter()
        .filter(|record| record.feature_id == feature_id);
    let ids = matches.next()?.edge_ids.as_slice();
    matches
        .all(|record| record.edge_ids.as_slice() == ids)
        .then_some(ids)
}

fn reconcile_feature_links(
    scan: &ContainerScan,
    ir: &mut CadIr,
    prototype_dependencies: &BTreeMap<u32, Vec<u32>>,
) {
    let emitted = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<BTreeSet<_>>();
    for feature in &mut ir.model.features {
        let Some(feature_id) = feature
            .id
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let native_dependencies =
            agreed_feature_parent_ids(&scan.features.affected_ids, feature_id)
                .into_iter()
                .chain(current_feature_recipe_parent(
                    &scan.features.operations,
                    feature_id,
                ))
                .chain(
                    prototype_dependencies
                        .get(&feature_id)
                        .into_iter()
                        .flatten()
                        .copied(),
                )
                .map(|dependency| IrFeatureId(format!("creo:model:feature#{dependency}")))
                .filter(|dependency| emitted.contains(dependency))
                .filter(|dependency| *dependency != feature.id)
                .fold(Vec::new(), |mut dependencies, dependency| {
                    if !dependencies.contains(&dependency) {
                        dependencies.push(dependency);
                    }
                    dependencies
                });
        feature.dependencies = reconciled_dependencies(
            &feature.id,
            &feature.dependencies,
            native_dependencies,
            &emitted,
        );
        if feature.parent.is_none() {
            feature.parent = current_feature_recipe_parent(&scan.features.operations, feature_id)
                .map(|parent| IrFeatureId(format!("creo:model:feature#{parent}")))
                .filter(|parent| *parent != feature.id && emitted.contains(parent));
        }
    }
    let mut remaining = (0..ir.model.features.len()).collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(remaining.len());
    let mut preceding = BTreeSet::new();
    while !remaining.is_empty() {
        let Some(position) = remaining.iter().position(|index| {
            let feature = &ir.model.features[*index];
            feature
                .dependencies
                .iter()
                .chain(feature.parent.iter())
                .all(|required| !emitted.contains(required) || preceding.contains(required))
        }) else {
            break;
        };
        let index = remaining.remove(position);
        preceding.insert(ir.model.features[index].id.clone());
        ordered.push(index);
    }
    ordered.extend(remaining);
    for (ordinal, index) in ordered.into_iter().enumerate() {
        ir.model.features[index].ordinal = ordinal as u64;
    }
}

fn reconciled_dependencies(
    feature_id: &IrFeatureId,
    established: &[IrFeatureId],
    native: impl IntoIterator<Item = IrFeatureId>,
    emitted: &BTreeSet<IrFeatureId>,
) -> Vec<IrFeatureId> {
    established
        .iter()
        .cloned()
        .chain(native)
        .filter(|dependency| emitted.contains(dependency))
        .filter(|dependency| dependency != feature_id)
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

fn resolved_revolution_axis(
    definition: &crate::feature::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<RevolutionAxis> {
    definition.variables.as_ref()?;
    let segments = definition.segments.as_ref()?;
    segments.is_complete().then_some(())?;
    let points = resolved_section_points(definition);
    let candidates = segments
        .rows
        .iter()
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line)
        .filter_map(|segment| {
            let start = points.get(&segment.point_ids[0])?;
            let end = points.get(&segment.point_ids[1])?;
            if start[0] != 0.0 || end[0] != 0.0 || start == end {
                return None;
            }
            let start = section_point_in_model(transform, *start);
            let end = section_point_in_model(transform, *end);
            let direction = normalized(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            Some(RevolutionAxis {
                origin: Point3::new(start[0], start[1], start[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        })
        .collect::<Vec<_>>();
    let [axis] = candidates.as_slice() else {
        return None;
    };
    Some(*axis)
}

fn section_profile_ref(ir: &CadIr, native_ref: String) -> ProfileRef {
    let sketch_id = SketchId(native_ref.replacen("creo:featdefs:sketch#", "creo:model:sketch#", 1));
    let Some(sketch) = ir
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id == sketch_id)
    else {
        return ProfileRef::Native(native_ref);
    };
    if sketch.profiles.is_empty() {
        ProfileRef::Native(native_ref)
    } else {
        ProfileRef::Sketch(sketch_id)
    }
}

fn feature_edge_selection(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<EdgeSelection> {
    let (ids, native) = if let Some(ids) = agreed_feature_affected_ids(
        &scan.features.affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Edges,
    ) {
        if ids.is_empty() {
            return None;
        }
        let native = format!(
            "creo:allfeatur:edgs_affected#{feature_id}:{}",
            ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
        );
        (ids, native)
    } else {
        if has_feature_affected_ids(
            &scan.features.affected_ids,
            feature_id,
            crate::feature::AffectedIdKind::Edges,
        ) {
            return None;
        }
        let ids = agreed_feature_replay_edge_ids(&scan.features.replay_affected_ids, feature_id)?;
        if ids.is_empty() {
            return None;
        }
        let native = format!(
            "creo:allfeatur:replay_edgs_affected#{feature_id}:{}",
            ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
        );
        (ids, native)
    };
    let edges = ids
        .iter()
        .map(|id| EdgeId(format!("creo:visibgeom:edge#{id}")))
        .collect::<Vec<_>>();
    let unique = edges.iter().collect::<BTreeSet<_>>().len() == edges.len();
    if unique
        && edges
            .iter()
            .all(|edge| ir.model.edges.iter().any(|candidate| candidate.id == *edge))
    {
        Some(EdgeSelection::Resolved { edges, native })
    } else {
        Some(EdgeSelection::Native(native))
    }
}

fn parallel_support_radius(planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>) -> Option<f64> {
    let planes = planes.into_iter().collect::<Vec<_>>();
    let mut radii = Vec::new();
    for first in 0..planes.len() {
        for second in first + 1..planes.len() {
            let first_normal = normalized(planes[first].1)?;
            let second_normal = normalized(planes[second].1)?;
            let alignment = first_normal
                .iter()
                .zip(second_normal)
                .map(|(first, second)| first * second)
                .sum::<f64>();
            if alignment.abs() < 1.0 - 1e-9 {
                continue;
            }
            let gap = planes[second]
                .0
                .iter()
                .zip(planes[first].0)
                .zip(first_normal)
                .map(|((second, first), normal)| (second - first) * normal)
                .sum::<f64>()
                .abs();
            let scale = planes[first]
                .0
                .iter()
                .chain(&planes[second].0)
                .map(|value| value.abs())
                .fold(1.0, f64::max);
            if gap > 1e-9 * scale {
                radii.push(0.5 * gap);
            }
        }
    }
    let radius = *radii.first()?;
    let scale = radius.abs().max(1.0);
    radii
        .iter()
        .all(|candidate| (candidate - radius).abs() <= 1e-9 * scale)
        .then_some(radius)
}

fn slot_fillet_cylinder(
    cap_planes: [PlaneEquation; 2],
    support_planes: &[PlaneEquation],
) -> Option<CylinderEquation> {
    let axis = normalized(cap_planes[0].normal)?;
    let second_cap_normal = normalized(cap_planes[1].normal)?;
    if (dot(axis, second_cap_normal).abs() - 1.0).abs() > 1e-10 {
        return None;
    }
    let cap_gap = dot(
        axis,
        std::array::from_fn(|index| cap_planes[1].origin[index] - cap_planes[0].origin[index]),
    )
    .abs();
    if cap_gap <= 1e-9 {
        return None;
    }
    let mut midplanes = Vec::<(PlaneEquation, f64)>::new();
    for first in 0..support_planes.len() {
        let first_normal = normalized(support_planes[first].normal)?;
        if dot(first_normal, axis).abs() > 1e-9 {
            return None;
        }
        for second in first + 1..support_planes.len() {
            let second_normal = normalized(support_planes[second].normal)?;
            if (dot(first_normal, second_normal).abs() - 1.0).abs() > 1e-10 {
                continue;
            }
            let gap = dot(
                first_normal,
                std::array::from_fn(|index| {
                    support_planes[second].origin[index] - support_planes[first].origin[index]
                }),
            )
            .abs();
            if gap <= 1e-9 {
                continue;
            }
            midplanes.push((
                PlaneEquation {
                    origin: std::array::from_fn(|index| {
                        0.5 * (support_planes[first].origin[index]
                            + support_planes[second].origin[index])
                    }),
                    normal: first_normal,
                },
                0.5 * gap,
            ));
        }
    }
    let mut candidates = Vec::<CylinderEquation>::new();
    for first in 0..midplanes.len() {
        for second in first + 1..midplanes.len() {
            let radius = midplanes[first].1;
            let scale = radius.max(midplanes[second].1).max(1.0);
            if (midplanes[second].1 - radius).abs() > 1e-9 * scale
                || dot(midplanes[first].0.normal, midplanes[second].0.normal).abs() > 1.0 - 1e-9
            {
                continue;
            }
            let origin = solve_planes(&[cap_planes[0], midplanes[first].0, midplanes[second].0])?;
            let tangent_to_all = support_planes.iter().all(|plane| {
                let Some(normal) = normalized(plane.normal) else {
                    return false;
                };
                let distance = dot(
                    normal,
                    std::array::from_fn(|index| origin[index] - plane.origin[index]),
                )
                .abs();
                (distance - radius).abs() <= 1e-8 * scale
            });
            if tangent_to_all {
                candidates.push(CylinderEquation {
                    origin,
                    axis,
                    ref_direction: midplanes[first].0.normal,
                    radius,
                });
            }
        }
    }
    let first = *candidates.first()?;
    let scale = first.radius.max(1.0);
    candidates
        .iter()
        .all(|candidate| {
            let origin_delta: [f64; 3] =
                std::array::from_fn(|index| candidate.origin[index] - first.origin[index]);
            (candidate.radius - first.radius).abs() <= 1e-9 * scale
                && (dot(candidate.axis, first.axis).abs() - 1.0).abs() <= 1e-10
                && dot(
                    cross(origin_delta, first.axis),
                    cross(origin_delta, first.axis),
                )
                .sqrt()
                    <= 1e-8 * scale
        })
        .then_some(first)
}

fn outline_has_unique_radius_delta(frame: crate::surface::TorusOutlineFrame, radius: f64) -> bool {
    let scale = frame
        .values
        .iter()
        .map(|value| value.abs())
        .fold(radius.abs().max(1.0), f64::max);
    frame.values[..3]
        .iter()
        .zip(&frame.values[3..])
        .filter(|(first, second)| ((*second - *first).abs() - radius).abs() <= 1e-9 * scale)
        .count()
        == 1
}

fn coordinate_pair_proves_torus_radii(
    first: [f64; 2],
    second: [f64; 2],
    major_radius: f64,
    minor_radius: f64,
) -> bool {
    let scale = first.iter().chain(&second).map(|value| value.abs()).fold(
        major_radius.abs().max(minor_radius.abs()).max(1.0),
        f64::max,
    );
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let proves = |outer: f64, minor: f64| {
        close(outer.abs(), 2.0 * (major_radius + minor_radius)) && close(minor.abs(), minor_radius)
    };
    let direct = proves(second[0] - first[0], second[1] - first[1]);
    let swapped = proves(second[1] - first[0], second[0] - first[1]);
    direct ^ swapped
}

fn five_coordinate_envelope_proves_torus_radii(
    envelope: crate::surface::Type26FiveCoordinateEnvelope,
    major_radius: f64,
    minor_radius: f64,
) -> bool {
    let [a1, a2, b0, b1, b2] = envelope.values;
    let scale = envelope.values.iter().map(|value| value.abs()).fold(
        major_radius.abs().max(minor_radius.abs()).max(1.0),
        f64::max,
    );
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    close(a1, b0)
        && coordinate_pair_proves_torus_radii([a1, a2], [b1, b2], major_radius, minor_radius)
}

fn paired_five_coordinate_sphere_center(
    envelopes: [crate::surface::Type26FiveCoordinateEnvelope; 2],
    radius: f64,
) -> Option<[f64; 3]> {
    (radius.is_finite() && radius > 0.0).then_some(())?;
    let scale = envelopes
        .iter()
        .flat_map(|envelope| envelope.values)
        .map(f64::abs)
        .fold(radius.max(1.0), f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let decoded = envelopes.map(|envelope| {
        let [x_min, z0, y_min, radial_max, z1] = envelope.values;
        (close(x_min, y_min)
            && close(radial_max - x_min, 2.0 * radius)
            && close((z1 - z0).abs(), radius))
        .then_some(([x_min, radial_max], [z0, z1]))
    });
    let [Some((first_radial, first_axial)), Some((second_radial, second_axial))] = decoded else {
        return None;
    };
    (close(first_radial[0], second_radial[0]) && close(first_radial[1], second_radial[1]))
        .then_some(())?;
    let shared = [first_axial[0], first_axial[1]]
        .into_iter()
        .filter(|candidate| {
            [second_axial[0], second_axial[1]]
                .into_iter()
                .any(|other| close(*candidate, other))
        })
        .collect::<Vec<_>>();
    let [center_z] = shared.as_slice() else {
        return None;
    };
    let axial_min = first_axial
        .into_iter()
        .chain(second_axial)
        .fold(f64::INFINITY, f64::min);
    let axial_max = first_axial
        .into_iter()
        .chain(second_axial)
        .fold(f64::NEG_INFINITY, f64::max);
    (close(axial_max - axial_min, 2.0 * radius)
        && close(*center_z - axial_min, radius)
        && close(axial_max - *center_z, radius))
    .then_some([
        0.5 * (first_radial[0] + first_radial[1]),
        0.5 * (first_radial[0] + first_radial[1]),
        *center_z,
    ])
}

fn unique_surface_parameter_record<'a>(
    scan: &'a ContainerScan,
    row: &crate::surface::SurfaceRow,
) -> Option<&'a crate::surface::SurfaceParameterRecord> {
    let mut records = scan
        .surfaces
        .parameters
        .iter()
        .filter(|record| record.offset == row.offset);
    let record = records.next()?;
    records.next().is_none().then_some(record)
}

fn prototype_envelope_round_radius(
    scan: &ContainerScan,
    rows: &[&crate::surface::SurfaceRow],
) -> Option<f64> {
    (scan.framing.layout == crate::container::Layout::Nd).then_some(())?;
    let feature_id = rows.first()?.feature_id;
    let prototype_radii = unique_surface_prototype_associations(scan)
        .into_iter()
        .filter(|(record, row, _)| {
            record.family == crate::surface::SurfacePrototypeFamily::Torus
                && row.feature_id == feature_id
                && rows.iter().any(|candidate| candidate.offset == row.offset)
        })
        .filter_map(|(record, _, _)| {
            Some((
                prototype_scalar(record, "radius1")?,
                prototype_scalar(record, "radius2")?,
            ))
        })
        .collect::<Vec<_>>();
    let &(radius1, radius2) = prototype_radii.first()?;
    let scale = radius1.abs().max(radius2.abs()).max(1.0);
    (radius1.is_finite()
        && radius1 >= 0.0
        && radius2.is_finite()
        && radius2 > 0.0
        && prototype_radii.iter().all(|candidate| {
            (candidate.0 - radius1).abs() <= 1e-9 * scale
                && (candidate.1 - radius2).abs() <= 1e-9 * scale
        }))
    .then_some(())?;
    rows.iter()
        .all(|row| {
            let Some(record) = unique_surface_parameter_record(scan, row) else {
                return false;
            };
            record.torus_radius_overrides(row.type_byte).is_none()
                && (record
                    .torus_outline_frame(row.type_byte)
                    .is_some_and(|frame| outline_has_unique_radius_delta(frame, radius2))
                    || record
                        .type26_five_coordinate_envelope(row.type_byte)
                        .is_some_and(|envelope| {
                            five_coordinate_envelope_proves_torus_radii(envelope, radius1, radius2)
                        })
                    || record
                        .type26_split_coordinate_envelope(row.type_byte)
                        .is_some_and(|envelope| {
                            let [a1, a2, b1, b2] = envelope.values;
                            coordinate_pair_proves_torus_radii([a1, a2], [b1, b2], radius1, radius2)
                        }))
        })
        .then_some(radius2)
}

fn round_constant_radius(scan: &ContainerScan, ir: &CadIr, feature_id: u32) -> Option<f64> {
    if let Some(radius) = round_direct_radii(scan, feature_id)
        .as_deref()
        .and_then(unique_positive_length)
    {
        return Some(radius);
    }
    let cylinder_rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
        .collect::<Vec<_>>();
    if cylinder_rows.is_empty() {
        let generated_rows = scan
            .surfaces
            .rows
            .iter()
            .filter(|row| row.feature_id == feature_id)
            .collect::<Vec<_>>();
        if generated_rows.is_empty()
            || generated_rows
                .iter()
                .any(|row| row.kind != crate::surface::SurfaceKind::TorusOrSphere)
        {
            return None;
        }
        return prototype_envelope_round_radius(scan, &generated_rows);
    }
    let generated_row_count = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .count();
    let cylinder_radii = cylinder_rows
        .iter()
        .filter_map(|row| {
            let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
            ir.model
                .surfaces
                .iter()
                .find(|surface| surface.id == id)
                .and_then(|surface| match surface.geometry {
                    SurfaceGeometry::Cylinder { radius, .. } => Some(radius),
                    _ => None,
                })
        })
        .collect::<Vec<_>>();
    if cylinder_rows.len() == generated_row_count && cylinder_radii.len() == cylinder_rows.len() {
        return unique_positive_length(&cylinder_radii);
    }
    let named_ids = agreed_feature_affected_ids(
        &scan.features.affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Geometry,
    );
    let named_present = has_feature_affected_ids(
        &scan.features.affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Geometry,
    );
    let replay_ids =
        agreed_feature_replay_geometry_ids(&scan.features.replay_affected_ids, feature_id);
    let affected_ids = match (named_ids, replay_ids) {
        (Some(ids), _) => ids,
        (None, Some(ids)) if !named_present => ids,
        _ => return None,
    };
    let support_ids = affected_ids.get(2..)?;
    let support_planes = support_ids
        .iter()
        .filter_map(|id| {
            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{id}"));
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == surface_id)?;
            match &surface.geometry {
                SurfaceGeometry::Plane { origin, normal, .. } => Some((
                    [origin.x, origin.y, origin.z],
                    [normal.x, normal.y, normal.z],
                )),
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    (support_planes.len() == support_ids.len()).then(|| parallel_support_radius(support_planes))?
}

fn round_direct_radii(scan: &ContainerScan, feature_id: u32) -> Option<Vec<f64>> {
    let generated_rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    let first_kind = generated_rows.first()?.kind;
    if generated_rows.iter().any(|row| row.kind != first_kind) {
        return None;
    }
    match first_kind {
        crate::surface::SurfaceKind::Cylinder => generated_rows
            .iter()
            .map(|row| {
                unique_surface_parameter_record(scan, row)?.type24_round_radius(row.type_byte)
            })
            .collect(),
        crate::surface::SurfaceKind::TorusOrSphere => generated_rows
            .iter()
            .map(|row| {
                unique_surface_parameter_record(scan, row)?
                    .torus_radius_overrides(row.type_byte)
                    .map(|overrides| overrides.radius2)
            })
            .collect(),
        _ => None,
    }
}

fn differing_positive_lengths(values: &[f64]) -> bool {
    let Some(&first) = values.first() else {
        return false;
    };
    if values
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return false;
    }
    let scale = values
        .iter()
        .copied()
        .map(f64::abs)
        .fold(first.abs().max(1.0), f64::max);
    values
        .iter()
        .any(|value| (*value - first).abs() > 1e-9 * scale)
}

fn unique_positive_length(values: &[f64]) -> Option<f64> {
    let value = *values.first()?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let scale = values
        .iter()
        .copied()
        .map(f64::abs)
        .fold(value.abs().max(1.0), f64::max);
    values
        .iter()
        .all(|candidate| {
            candidate.is_finite() && *candidate > 0.0 && (*candidate - value).abs() <= 1e-9 * scale
        })
        .then_some(value)
}

fn schema_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    schema_class: u32,
    kind: &str,
) -> IrFeatureDefinition {
    if let Some(definition) = reference_named_feature_definition(kind) {
        return definition;
    }
    if schema_class == 926 {
        let sketch =
            section_definition_for_history_feature(scan, feature_id).and_then(|definition| {
                let section = definition.section_3d.as_ref()?;
                unique_feature_section_transform(
                    &scan.features.section_transforms,
                    definition.id,
                    section.offset,
                )?;
                let sketch = model_sketch_id(scan, definition);
                ir.model
                    .sketches
                    .iter()
                    .any(|candidate| candidate.id == sketch)
                    .then_some(sketch)
            });
        return IrFeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::default(),
            sketch,
        };
    }
    if schema_class == 911 {
        let unresolved_form = stepped_hole_form(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        );
        let stepped_dimensions = (unresolved_form == Some(HoleForm::Counterbore))
            .then(|| counterbore_dimensions(scan, ir, feature_id))
            .flatten();
        let placement = hole_placement(feature_outline_planes(scan, feature_id));
        let compact_cylinder_id = compact_simple_hole_cylinder_id(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        );
        let solved = simple_hole_geometry(scan, feature_id)
            .or_else(|| compact_simple_hole_geometry(scan, feature_id));
        let simple_form = solved.is_some() || compact_cylinder_id.is_some();
        let face_selection = |surface_id| {
            let native = format!("creo:visibgeom:surface#{surface_id}");
            let face = FaceId(format!("creo:visibgeom:face#{surface_id}"));
            if ir.model.faces.iter().any(|candidate| candidate.id == face) {
                FaceSelection::Resolved {
                    faces: vec![face],
                    native,
                }
            } else {
                FaceSelection::Native(native)
            }
        };
        let (face, position, direction, diameter, extent, bottom) = solved.map_or_else(
            || {
                placement.map_or(
                    (None, None, None, None, None, None),
                    |(entry_surface_id, direction, extent)| {
                        (
                            Some(face_selection(entry_surface_id)),
                            None,
                            Some(Vector3::new(direction[0], direction[1], direction[2])),
                            None,
                            Some(extent),
                            None,
                        )
                    },
                )
            },
            |hole| {
                let SurfaceGeometry::Cylinder { origin, radius, .. } = hole.geometry else {
                    unreachable!("simple hole helper returns a cylinder")
                };
                (
                    hole.entry_surface_id.map(face_selection),
                    Some(origin),
                    Some(Vector3::new(
                        hole.direction[0],
                        hole.direction[1],
                        hole.direction[2],
                    )),
                    Some(Length(2.0 * radius)),
                    Some(hole.extent),
                    Some(HoleBottom::Flat),
                )
            },
        );
        return IrFeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face,
            position,
            direction,
            placements: Vec::new(),
            kind: if simple_form {
                HoleKind::Simple
            } else {
                HoleKind::Unresolved {
                    form: unresolved_form,
                    counterbore_diameter: stepped_dimensions
                        .map(|(_, diameter, _)| Length(diameter)),
                    counterbore_depth: stepped_dimensions.map(|(_, _, depth)| Length(depth)),
                    countersink_diameter: None,
                    countersink_angle: None,
                }
            },
            exit_kind: None,
            diameter: diameter
                .or_else(|| stepped_dimensions.map(|(diameter, _, _)| Length(diameter))),
            extent,
            bottom,
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        };
    }
    if schema_class == 913 {
        let radius = round_constant_radius(scan, ir, feature_id).map_or_else(
            || RadiusSpec::Unresolved {
                form: round_direct_radii(scan, feature_id)
                    .as_deref()
                    .is_some_and(differing_positive_lengths)
                    .then_some(RadiusForm::Variable),
            },
            |radius| RadiusSpec::Constant {
                radius: Length(radius),
            },
        );
        return IrFeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: feature_edge_selection(scan, ir, feature_id)
                    .unwrap_or(EdgeSelection::Unresolved),
                radius,
                tangency_weight: None,
            }],
        };
    }
    if schema_class == 914 {
        return IrFeatureDefinition::Chamfer {
            groups: vec![cadmpeg_ir::features::ChamferGroup {
                edges: feature_edge_selection(scan, ir, feature_id)
                    .unwrap_or(EdgeSelection::Unresolved),
                spec: ChamferSpec::Unresolved { form: None },
            }],
            flip_direction: false,
        };
    }
    if schema_class == 927 {
        return IrFeatureDefinition::Draft {
            faces: FaceSelection::Unresolved,
            neutral_plane: FaceSelection::Unresolved,
            pull_direction: None,
            angle: None,
            outward: None,
        };
    }
    if schema_class == 917
        && section_sweep_allows_linear_extrusion(schema_class, feature_recipe(scan, feature_id))
    {
        if let Some(sweep) = circular_sweep_geometry(scan, feature_id) {
            let definition =
                unique_owned_feature_definition(&scan.features.definitions, feature_id).filter(
                    |definition| {
                        sweep
                            .section_definition_id
                            .is_none_or(|definition_id| definition_id == definition.id)
                    },
                );
            let profile = definition.map_or_else(
                || ProfileRef::Unresolved(format!("creo:model:feature#{feature_id}")),
                |definition| {
                    section_profile_ref(ir, feature_sketch_record_id_in_scan(scan, definition))
                },
            );
            let output_kind = evaluated_sweep_body_kind(ir, "extrusion", feature_id);
            return circular_sweep_feature_definition(
                profile,
                &sweep,
                section_sweep_boolean_operation(
                    feature_recipe_effect(scan, feature_id),
                    kind,
                    output_kind.is_some(),
                    preceding_features_establish_body(ir),
                ),
                (output_kind == Some(BodyKind::Solid)).then_some(true),
            );
        }
    }
    if feature_recipe(scan, feature_id) == Some(crate::feature::FeatureRecipeKind::Revolve) {
        let extent = feature_revolution_extent(scan, feature_id);
        let transforms = scan
            .features
            .section_transforms
            .iter()
            .filter(|transform| transform.feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        let (definition, transform) = match transforms.as_slice() {
            [transform] => (
                unique_feature_definition_for_transform(&scan.features.definitions, transform),
                Some(*transform),
            ),
            [] => (
                unique_owned_feature_definition(&scan.features.definitions, feature_id),
                None,
            ),
            _ => (None, None),
        };
        let profile = definition.map(|definition| {
            section_profile_ref(ir, feature_sketch_record_id_in_scan(scan, definition))
        });
        let axis = definition
            .zip(transform)
            .and_then(|(definition, transform)| resolved_revolution_axis(definition, transform));
        let output_kind = evaluated_sweep_body_kind(ir, "revolution", feature_id);
        return IrFeatureDefinition::Revolve {
            construction: RevolutionConstruction {
                profile,
                axis,
                extent,
                axis_reference: None,
                solid: (output_kind == Some(BodyKind::Solid)).then_some(true),
                face_maker_class: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            },
            op: section_sweep_boolean_operation(
                feature_recipe_effect(scan, feature_id),
                kind,
                output_kind.is_some(),
                preceding_features_establish_body(ir),
            ),
        };
    }
    let recipe = feature_recipe(scan, feature_id);
    if section_sweep_allows_linear_extrusion(schema_class, recipe) {
        let transforms = scan
            .features
            .section_transforms
            .iter()
            .filter(|transform| transform.feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        let definition = match transforms.as_slice() {
            [transform] => {
                unique_feature_definition_for_transform(&scan.features.definitions, transform)
            }
            [] => unique_owned_feature_definition(&scan.features.definitions, feature_id),
            _ => None,
        };
        let profile = definition.map(|definition| {
            section_profile_ref(ir, feature_sketch_record_id_in_scan(scan, definition))
        });
        let construction = if let ([transform], Some(profile), Some(definition)) =
            (transforms.as_slice(), profile.clone(), definition)
        {
            generated_arc_cylinder_extent(scan, definition, transform)
                .or_else(|| {
                    extrusion_extent_and_direction(
                        transform.origin,
                        transform.normal,
                        feature_plane_equations(scan, feature_id),
                    )
                })
                .map(|(extent, direction)| {
                    (
                        profile,
                        Some(Vector3::new(direction[0], direction[1], direction[2])),
                        extent,
                    )
                })
        } else {
            None
        };
        let (profile, direction, extent) = construction.unwrap_or((
            profile.unwrap_or_else(|| {
                ProfileRef::Unresolved(format!("creo:model:feature#{feature_id}"))
            }),
            None,
            unresolved_extrude_extent(),
        ));
        let output_kind = evaluated_sweep_body_kind(ir, "extrusion", feature_id);
        return IrFeatureDefinition::Extrude {
            profile,
            direction: direction.map_or(
                cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
                cadmpeg_ir::features::ExtrudeDirection::Explicit,
            ),
            start: cadmpeg_ir::features::ExtrudeStart::default(),
            extent,
            op: section_sweep_boolean_operation(
                feature_recipe_effect(scan, feature_id),
                kind,
                output_kind.is_some(),
                preceding_features_establish_body(ir),
            ),
            direction_source: None,
            solid: (output_kind == Some(BodyKind::Solid)).then_some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        };
    }
    if schema_class == 923 {
        if let Some(datum) = unique_feature_datum_plane(&scan.planes.datums, feature_id) {
            return datum_plane_feature_definition(datum);
        }
        if scan
            .planes
            .datums
            .iter()
            .any(|datum| datum.feature_id == feature_id)
        {
            return IrFeatureDefinition::DatumPlaneUnresolved;
        }
        let plane_ids = scan
            .surfaces
            .rows
            .iter()
            .filter(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
            })
            .map(|row| row.id)
            .collect::<BTreeSet<_>>();
        let plane_ids = plane_ids.into_iter().collect::<Vec<_>>();
        if plane_ids.len() > 1 {
            return IrFeatureDefinition::DatumPlaneUnresolved;
        }
        if let [surface_id] = plane_ids.as_slice() {
            if crate::surface::unique_surface_row(&scan.surfaces.rows, *surface_id).is_none() {
                return IrFeatureDefinition::DatumPlaneUnresolved;
            }
            if let Some(plane) = placed_planes(scan).get(surface_id) {
                let normal = Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]);
                return IrFeatureDefinition::DatumPlane {
                    origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                    normal,
                    u_axis: cadmpeg_ir::geometry::derive_reference_direction(normal),
                };
            }
            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
            let planes = ir
                .model
                .surfaces
                .iter()
                .filter(|surface| surface.id == surface_id)
                .filter_map(|surface| match surface.geometry {
                    SurfaceGeometry::Plane {
                        origin,
                        normal,
                        u_axis,
                    } => Some((origin, normal, u_axis)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if let [(origin, normal, u_axis)] = planes.as_slice() {
                return IrFeatureDefinition::DatumPlane {
                    origin: *origin,
                    normal: *normal,
                    u_axis: *u_axis,
                };
            }
            return IrFeatureDefinition::DatumPlaneUnresolved;
        }
        let definitions = scan
            .features
            .definitions
            .iter()
            .filter(|definition| definition.owner_feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        if let [definition] = definitions.as_slice() {
            if let Some(values) = crate::placement::unique_complete_local_system(definition) {
                let raw_normal: [f64; 3] = values[6..9].try_into().expect("three values");
                let raw_u_axis: [f64; 3] = values[0..3].try_into().expect("three values");
                if let (Some(normal), Some(u_axis)) =
                    (normalized(raw_normal), normalized(raw_u_axis))
                {
                    if dot(normal, u_axis).abs() <= 1e-12 {
                        let origin: [f64; 3] = values[9..12].try_into().expect("three values");
                        return IrFeatureDefinition::DatumPlane {
                            origin: Point3::new(origin[0], origin[1], origin[2]),
                            normal: Vector3::new(normal[0], normal[1], normal[2]),
                            u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
                        };
                    }
                }
            }
        }
        return IrFeatureDefinition::DatumPlaneUnresolved;
    }
    if schema_class == 946 {
        return unresolved_surface_merge_feature_definition();
    }
    if schema_class == 979 && kind == "PRT_CSYS_DEF" {
        return IrFeatureDefinition::DatumCoordinateSystemUnresolved;
    }
    if numbered_feature_name_has_family(kind, "Extrude") {
        return unresolved_extrude_feature_definition(feature_id);
    }
    if schema_class == 942
        && class_942_boundary_surface_entity_graph(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        )
    {
        return IrFeatureDefinition::BoundarySurfaceUnresolved;
    }
    if schema_operation_kind(schema_class).is_none() {
        if let Some(definition) = named_or_referenced_feature_definition(scan, ir, feature_id, kind)
        {
            return definition;
        }
        if let Some(definition) = unbounded_feature_plane_definition(scan, ir, feature_id) {
            return definition;
        }
    }
    IrFeatureDefinition::Native {
        kind: kind.to_string(),
        parameters: feature_parameters(scan, feature_id),
        properties: BTreeMap::new(),
    }
}

fn datum_plane_feature_definition(datum: &crate::datum::DatumPlane) -> IrFeatureDefinition {
    IrFeatureDefinition::DatumPlane {
        origin: Point3::new(
            datum.normal[0] * datum.offset,
            datum.normal[1] * datum.offset,
            datum.normal[2] * datum.offset,
        ),
        normal: Vector3::new(datum.normal[0], datum.normal[1], datum.normal[2]),
        u_axis: cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
            datum.normal[0],
            datum.normal[1],
            datum.normal[2],
        )),
    }
}

fn unbounded_feature_plane_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<IrFeatureDefinition> {
    let rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return None;
    };
    (row.boundary_type == 1
        && row.next_surface == 0
        && crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) == Some(*row))
    .then_some(())?;
    let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| surface.id == id)
        .collect::<Vec<_>>();
    let [surface] = surfaces.as_slice() else {
        return None;
    };
    let SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = surface.geometry
    else {
        return None;
    };
    Some(IrFeatureDefinition::DatumPlane {
        origin,
        normal,
        u_axis,
    })
}

fn numbered_feature_name_has_family(name: &str, family: &str) -> bool {
    name.strip_prefix(family)
        .and_then(|suffix| suffix.strip_prefix(' '))
        .is_some_and(|ordinal| {
            !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn section_sweep_allows_linear_extrusion(
    schema_class: u32,
    recipe: Option<crate::feature::FeatureRecipeKind>,
) -> bool {
    recipe == Some(crate::feature::FeatureRecipeKind::Extrude)
        || (matches!(schema_class, 916 | 917)
            && recipe != Some(crate::feature::FeatureRecipeKind::Revolve))
}

fn feature_allows_linear_extrusion(scan: &ContainerScan, feature_id: u32) -> bool {
    feature_schema_class(scan, feature_id).is_some_and(|schema_class| {
        section_sweep_allows_linear_extrusion(schema_class, feature_recipe(scan, feature_id))
    })
}

fn feature_allows_additive_linear_extrusion(scan: &ContainerScan, feature_id: u32) -> bool {
    feature_schema_class(scan, feature_id) == Some(917)
        && section_sweep_allows_linear_extrusion(917, feature_recipe(scan, feature_id))
        && feature_recipe_effect(scan, feature_id)
            .is_none_or(|effect| effect == crate::feature::FeatureRecipeEffect::Protrude)
}

fn preceding_features_establish_body(ir: &CadIr) -> bool {
    ir.model.features.iter().any(|feature| {
        feature.suppressed != Some(true)
            && (!feature.outputs.is_empty()
                || matches!(
                    feature.definition,
                    IrFeatureDefinition::Extrude {
                        op: BooleanOp::NewBody,
                        ..
                    } | IrFeatureDefinition::Revolve {
                        op: BooleanOp::NewBody,
                        ..
                    }
                ))
    })
}

fn section_sweep_boolean_operation(
    recipe_effect: Option<crate::feature::FeatureRecipeEffect>,
    kind: &str,
    has_evaluated_body: bool,
    prior_body: bool,
) -> BooleanOp {
    match recipe_effect {
        Some(crate::feature::FeatureRecipeEffect::Protrude) if prior_body => BooleanOp::Join,
        Some(crate::feature::FeatureRecipeEffect::Protrude) => BooleanOp::NewBody,
        Some(crate::feature::FeatureRecipeEffect::Cut) => BooleanOp::Cut,
        None if kind == "Protrusion" && prior_body => BooleanOp::Join,
        None if kind == "Protrusion" => BooleanOp::NewBody,
        None if kind == "Cut" => BooleanOp::Cut,
        None if has_evaluated_body => BooleanOp::NewBody,
        _ => BooleanOp::Unresolved,
    }
}

fn class_942_boundary_surface_entity_graph(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> bool {
    let mut generated_surfaces = surface_rows
        .iter()
        .filter(|row| row.feature_id == feature_id);
    let Some(surface) = generated_surfaces.next() else {
        return false;
    };
    if generated_surfaces.next().is_some() || surface.kind != crate::surface::SurfaceKind::Extrusion
    {
        return false;
    }
    let owned = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let unique_table = |class_id| {
        let mut matches = owned
            .iter()
            .copied()
            .filter(|table| table.table_class_id == class_id);
        let table = matches.next()?;
        matches.next().is_none().then_some(table)
    };
    let Some(generated) = unique_table(29) else {
        return false;
    };
    let Some(topology) = unique_table(94) else {
        return false;
    };
    let Some(owner) = unique_table(67) else {
        return false;
    };
    let Some(output) = unique_table(100) else {
        return false;
    };
    let [owner_entry] = owner.entries.as_slice() else {
        return false;
    };
    matches!(
        generated.entries.as_slice(),
        [entry]
            if entry.class_id == 200
                && entry.entity_id == surface.id
                && entry.source_entity_id == Some(0)
                && generated.surface_ids.as_slice() == [surface.id]
    ) && topology
        .entries
        .iter()
        .map(|entry| entry.class_id)
        .eq([221, 222, 220, 220])
        && owner_entry.class_id == 200
        && owner_entry.source_entity_id == Some(feature_id)
        && matches!(
            output.entries.as_slice(),
            [entry]
                if entry.entity_id == owner_entry.entity_id
                    && entry.class_id == surface.id
        )
}

fn named_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    kind: &str,
) -> Option<IrFeatureDefinition> {
    if let Some(definition) = reference_named_feature_definition(kind) {
        return Some(definition);
    }
    if matches!(kind, "Protrusion" | "Cut") {
        return Some(unresolved_extrude_feature_definition_with_op(
            feature_id,
            section_sweep_boolean_operation(
                feature_recipe_effect(scan, feature_id),
                kind,
                false,
                preceding_features_establish_body(ir),
            ),
        ));
    }
    if let Some(role) = match kind {
        "Annotation Feature" => Some(FeatureTreeNodeRole::Annotations),
        "Cross Section" | "Querschnitt" => Some(FeatureTreeNodeRole::CrossSections),
        _ => None,
    } {
        return Some(IrFeatureDefinition::TreeNode {
            role,
            children: Vec::new(),
            active_child: None,
        });
    }
    if kind == "Mirror" {
        return Some(IrFeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Mirror),
            },
        });
    }
    if kind == "Extrude" || numbered_feature_name_has_family(kind, "Extrude") {
        return Some(unresolved_extrude_feature_definition(feature_id));
    }
    if kind == "Revolve" {
        return Some(IrFeatureDefinition::Revolve {
            construction: RevolutionConstruction {
                profile: None,
                axis: None,
                extent: None,
                axis_reference: None,
                solid: None,
                face_maker_class: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            },
            op: BooleanOp::Unresolved,
        });
    }
    let schema_class = match kind {
        "Datum Plane" | "Bezugsebene" => 923,
        "Hole" => 911,
        "Round" | "Rundung" => 913,
        "Chamfer" => 914,
        "Draft" | "Schräge" => 927,
        _ => return None,
    };
    Some(schema_feature_definition(
        scan,
        ir,
        feature_id,
        schema_class,
        kind,
    ))
}

fn named_or_referenced_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    kind: &str,
) -> Option<IrFeatureDefinition> {
    named_feature_definition(scan, ir, feature_id, kind).or_else(|| {
        feature_reference_name(scan, feature_id)
            .filter(|reference_name| *reference_name != kind)
            .and_then(|reference_name| {
                named_feature_definition(scan, ir, feature_id, reference_name)
            })
    })
}

fn unresolved_extrude_feature_definition(feature_id: u32) -> IrFeatureDefinition {
    unresolved_extrude_feature_definition_with_op(feature_id, BooleanOp::Unresolved)
}

fn unresolved_extrude_feature_definition_with_op(
    feature_id: u32,
    op: BooleanOp,
) -> IrFeatureDefinition {
    IrFeatureDefinition::Extrude {
        profile: ProfileRef::Unresolved(format!("creo:model:feature#{feature_id}")),
        direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
        start: cadmpeg_ir::features::ExtrudeStart::default(),
        extent: unresolved_extrude_extent(),
        op,
        direction_source: None,
        solid: None,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    }
}

fn unresolved_extrude_extent() -> ExtrudeExtent {
    ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination: Termination::Unresolved,
            draft: None,
            offset: None,
        },
    }
}

fn reference_named_feature_definition(kind: &str) -> Option<IrFeatureDefinition> {
    if numbered_feature_name_has_family(kind, "Boundary Blend") {
        return Some(IrFeatureDefinition::BoundarySurfaceUnresolved);
    }
    if numbered_feature_name_has_family(kind, "Thicken") {
        return Some(IrFeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: None,
            side: None,
        });
    }
    if numbered_feature_name_has_family(kind, "Merge") {
        return Some(unresolved_surface_merge_feature_definition());
    }
    numbered_feature_name_has_family(kind, "Fill").then_some(IrFeatureDefinition::FilledSurface {
        boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Unresolved),
        support_faces: FaceSelection::Unresolved,
        continuity: None,
        merge_result: None,
    })
}

fn unresolved_surface_merge_feature_definition() -> IrFeatureDefinition {
    IrFeatureDefinition::KnitSurface {
        faces: FaceSelection::Unresolved,
        merge_entities: None,
        create_solid: None,
        gap_tolerance: None,
    }
}

fn retain_native_feature_parameters(
    source_properties: &mut BTreeMap<String, String>,
    definition: &IrFeatureDefinition,
    parameters: &BTreeMap<String, String>,
) {
    if matches!(definition, IrFeatureDefinition::Native { .. }) {
        return;
    }
    for (name, value) in parameters {
        source_properties.insert(format!("native_parameter.{name}"), value.clone());
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ExtrusionSpan {
    lower: f64,
    upper: f64,
}

fn hole_extent_and_direction(
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<([f64; 3], Termination)> {
    let planes = planes.into_iter().collect::<Vec<_>>();
    let [(first_origin, first_normal), (second_origin, second_normal)] = planes.as_slice() else {
        return None;
    };
    let first_normal = normalized(*first_normal)?;
    let second_normal = normalized(*second_normal)?;
    let alignment = first_normal
        .iter()
        .zip(second_normal)
        .map(|(first, second)| first * second)
        .sum::<f64>()
        .abs();
    if (alignment - 1.0).abs() > 1e-9 {
        return None;
    }
    let signed_length = second_origin
        .iter()
        .zip(first_origin)
        .zip(first_normal)
        .map(|((second, first), axis)| (second - first) * axis)
        .sum::<f64>();
    let scale = second_origin
        .iter()
        .chain(first_origin)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if signed_length.abs() <= 1e-9 * scale {
        return None;
    }
    Some((
        first_normal.map(|value| value * signed_length.signum()),
        Termination::Blind {
            length: Length(signed_length.abs()),
        },
    ))
}

fn hole_placement(
    planes: impl IntoIterator<Item = (u32, [f64; 3], [f64; 3])>,
) -> Option<(u32, [f64; 3], Termination)> {
    let planes = planes.into_iter().collect::<Vec<_>>();
    let [(entry_id, entry_origin, entry_normal), (_, termination_origin, termination_normal)] =
        planes.as_slice()
    else {
        return None;
    };
    let (direction, extent) = hole_extent_and_direction([
        (*entry_origin, *entry_normal),
        (*termination_origin, *termination_normal),
    ])?;
    Some((*entry_id, direction, extent))
}

fn plane_envelope_corners(envelope: &crate::surface::PlaneEnvelope) -> Option<[[f64; 3]; 2]> {
    let corners = match envelope {
        crate::surface::PlaneEnvelope::Standard { corners_3d, .. }
        | crate::surface::PlaneEnvelope::Compact { corners_3d, .. } => corners_3d,
    };
    Some([
        [corners[0][0]?, corners[0][1]?, corners[0][2]?],
        [corners[1][0]?, corners[1][1]?, corners[1][2]?],
    ])
}

type HoleCapOutline = (u32, [f64; 3], [f64; 3], [[f64; 3]; 2]);
type PartialCapOutline = (u32, [f64; 3], [f64; 3], Option<[[f64; 3]; 2]>);

fn cap_square_center_radius(corners: [[f64; 3]; 2], axis_index: usize) -> Option<([f64; 3], f64)> {
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let spans = [
        (corners[1][radial[0]] - corners[0][radial[0]]).abs(),
        (corners[1][radial[1]] - corners[0][radial[1]]).abs(),
    ];
    let scale = spans[0]
        .max(spans[1])
        .max(corners[0][axis_index].abs())
        .max(corners[1][axis_index].abs())
        .max(1.0);
    if (corners[1][axis_index] - corners[0][axis_index]).abs() > 1e-9 * scale
        || spans[0] <= 1e-9
        || (spans[0] - spans[1]).abs() > 1e-9 * scale
    {
        return None;
    }
    Some((
        std::array::from_fn(|index| 0.5 * (corners[0][index] + corners[1][index])),
        0.5 * spans[0],
    ))
}

fn cylinder_from_single_cap_outline(cap: PartialCapOutline) -> Option<SurfaceGeometry> {
    let (_, _, axis, corners) = cap;
    let axis = normalized(axis)?;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - 1e-9
            && (0..3).all(|other| other == *index || axis[other].abs() < 1e-9)
    })?;
    let (center, radius) = cap_square_center_radius(corners?, axis_index)?;
    let radial_axis = (0..3).find(|index| *index != axis_index)?;
    let mut ref_direction = [0.0; 3];
    ref_direction[radial_axis] = 1.0;
    Some(SurfaceGeometry::Cylinder {
        origin: Point3::new(center[0], center[1], center[2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
        ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
        radius,
    })
}

fn hole_cylinder_from_cap_outlines(caps: [HoleCapOutline; 2]) -> Option<SurfaceGeometry> {
    let placement = hole_placement(caps.map(|(id, origin, normal, _)| (id, origin, normal)))?;
    let axis = placement.1;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - 1e-9
            && (0..3).all(|other| other == *index || axis[other].abs() < 1e-9)
    })?;
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let mut centers = Vec::<[f64; 3]>::new();
    let mut radii = Vec::new();
    for (_, _, _, corners) in caps {
        let (center, radius) = cap_square_center_radius(corners, axis_index)?;
        centers.push(center);
        radii.push(radius);
    }
    let scale = centers
        .iter()
        .flatten()
        .chain(&radii)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if radial
        .iter()
        .any(|index| (centers[0][*index] - centers[1][*index]).abs() > 1e-9 * scale)
        || (radii[0] - radii[1]).abs() > 1e-9 * scale
    {
        return None;
    }
    let mut ref_direction = [0.0; 3];
    ref_direction[radial[0]] = 1.0;
    Some(SurfaceGeometry::Cylinder {
        origin: Point3::new(centers[0][0], centers[0][1], centers[0][2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
        ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
        radius: radii[0],
    })
}

fn cylinder_from_complementary_outline_bounds(
    plane: &SurfaceGeometry,
    bounds: [[[f64; 2]; 2]; 2],
) -> Option<SurfaceGeometry> {
    let SurfaceGeometry::Plane { origin, normal, .. } = plane else {
        return None;
    };
    let axis = normalized([normal.x, normal.y, normal.z])?;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - 1e-9
            && (0..3).all(|other| other == *index || axis[other].abs() < 1e-9)
    })?;
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let scale = bounds
        .iter()
        .flatten()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    if bounds
        .iter()
        .any(|rectangle| (0..2).any(|index| rectangle[1][index] <= rectangle[0][index]))
    {
        return None;
    }
    let union = if close(bounds[0][0][0], bounds[1][0][0])
        && close(bounds[0][1][0], bounds[1][1][0])
        && (close(bounds[0][1][1], bounds[1][0][1]) || close(bounds[1][1][1], bounds[0][0][1]))
    {
        [
            [bounds[0][0][0], bounds[0][0][1].min(bounds[1][0][1])],
            [bounds[0][1][0], bounds[0][1][1].max(bounds[1][1][1])],
        ]
    } else if close(bounds[0][0][1], bounds[1][0][1])
        && close(bounds[0][1][1], bounds[1][1][1])
        && (close(bounds[0][1][0], bounds[1][0][0]) || close(bounds[1][1][0], bounds[0][0][0]))
    {
        [
            [bounds[0][0][0].min(bounds[1][0][0]), bounds[0][0][1]],
            [bounds[0][1][0].max(bounds[1][1][0]), bounds[0][1][1]],
        ]
    } else {
        return None;
    };
    let spans = [union[1][0] - union[0][0], union[1][1] - union[0][1]];
    if spans.iter().any(|span| !span.is_finite() || *span <= 0.0) || !close(spans[0], spans[1]) {
        return None;
    }
    let mut center = [origin.x, origin.y, origin.z];
    for (coordinate, index) in radial.iter().enumerate() {
        center[*index] = 0.5 * (union[0][coordinate] + union[1][coordinate]);
    }
    let mut ref_direction = [0.0; 3];
    ref_direction[radial[0]] = 1.0;
    Some(SurfaceGeometry::Cylinder {
        origin: Point3::new(center[0], center[1], center[2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
        ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
        radius: 0.5 * spans[0],
    })
}

#[derive(Debug, Clone, PartialEq)]
struct SimpleHoleGeometry {
    entry_surface_id: Option<u32>,
    cylinder_ids: Vec<u32>,
    direction: [f64; 3],
    extent: Termination,
    geometry: SurfaceGeometry,
}

fn stepped_hole_form(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Option<HoleForm> {
    let tables = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 29)
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let mut generated_by_source = BTreeMap::<u32, Vec<Option<crate::surface::SurfaceKind>>>::new();
    for entry in table.entries.iter().filter(|entry| entry.class_id == 200) {
        let source_id = entry.source_entity_id?;
        let kind = if table.surface_ids.contains(&entry.entity_id) {
            Some(
                crate::surface::unique_surface_row(rows, entry.entity_id)
                    .filter(|row| row.feature_id == feature_id)?
                    .kind,
            )
        } else {
            table
                .non_surface_entity_ids
                .contains(&entry.entity_id)
                .then_some(())?;
            None
        };
        generated_by_source.entry(source_id).or_default().push(kind);
    }
    let cylinder_sources = generated_by_source
        .values()
        .filter(|entries| {
            matches!(
                entries.as_slice(),
                [
                    Some(crate::surface::SurfaceKind::Cylinder),
                    Some(crate::surface::SurfaceKind::Cylinder)
                ]
            )
        })
        .count();
    let planar_support_sources = generated_by_source
        .values()
        .filter(|entries| {
            entries.len() == 2
                && entries
                    .iter()
                    .filter(|kind| **kind == Some(crate::surface::SurfaceKind::Plane))
                    .count()
                    == 1
                && entries.iter().filter(|kind| kind.is_none()).count() == 1
        })
        .count();
    let has_cone = generated_by_source
        .values()
        .flatten()
        .any(|kind| *kind == Some(crate::surface::SurfaceKind::Cone));
    (cylinder_sources == 2 && planar_support_sources == 1 && !has_cone)
        .then_some(HoleForm::Counterbore)
}

fn counterbore_dimensions(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<(f64, f64, f64)> {
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 29)
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let generated_cylinders = table
        .surface_ids
        .iter()
        .copied()
        .filter(|surface_id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, *surface_id).is_some_and(
                |row| {
                    row.feature_id == feature_id
                        && row.kind == crate::surface::SurfaceKind::Cylinder
                },
            )
        })
        .collect::<BTreeSet<_>>();
    let generated_radii = ir
        .model
        .surfaces
        .iter()
        .filter_map(|surface| {
            let surface_id = surface
                .id
                .0
                .strip_prefix("creo:visibgeom:surface#")?
                .parse::<u32>()
                .ok()?;
            generated_cylinders.contains(&surface_id).then_some(())?;
            let SurfaceGeometry::Cylinder { radius, .. } = surface.geometry else {
                return None;
            };
            Some(radius)
        })
        .collect::<Vec<_>>();
    counterbore_dimension_values(
        scan.features
            .definitions
            .iter()
            .filter(|definition| definition.id == 911)
            .filter_map(|definition| definition.dimensions.as_ref()),
        &generated_radii,
    )
}

fn counterbore_dimension_values<'a>(
    tables: impl Iterator<Item = &'a crate::feature::FeatureDimensionTable>,
    generated_radii: &[f64],
) -> Option<(f64, f64, f64)> {
    let mut candidates = Vec::new();
    for table in tables {
        if usize::try_from(table.declared_count).ok() != Some(table.rows.len())
            || table.rows.len() != 4
        {
            continue;
        }
        let value = |external_id, dimension_type| {
            let rows = table
                .rows
                .iter()
                .filter(|row| {
                    row.external_id == external_id && row.dimension_type == dimension_type
                })
                .collect::<Vec<_>>();
            let [row] = rows.as_slice() else {
                return None;
            };
            row.value.filter(|value| value.is_finite() && *value > 0.0)
        };
        let (Some(bore_radius), Some(placement_distance), Some(depth), Some(counterbore_radius)) =
            (value(0, 2), value(1, 2), value(2, 1), value(3, 2))
        else {
            continue;
        };
        if bore_radius >= counterbore_radius
            || placement_distance <= 0.0
            || !generated_radii.iter().any(|radius| {
                (*radius - counterbore_radius).abs()
                    <= 1e-9 * radius.abs().max(counterbore_radius.abs()).max(1.0)
            })
        {
            continue;
        }
        candidates.push((2.0 * bore_radius, 2.0 * counterbore_radius, depth));
    }
    let first = *candidates.first()?;
    candidates
        .iter()
        .all(|candidate| {
            [
                candidate.0 - first.0,
                candidate.1 - first.1,
                candidate.2 - first.2,
            ]
            .iter()
            .all(|delta| delta.abs() <= 1e-9)
        })
        .then_some(first)
}

fn counterbore_patch_geometries(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<Vec<(u32, SurfaceGeometry)>> {
    let (bore_diameter, counterbore_diameter, _) = counterbore_dimensions(scan, ir, feature_id)?;
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 29)
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let mut cylinders_by_source = BTreeMap::<u32, Vec<u32>>::new();
    for entry in table.entries.iter().filter(|entry| entry.class_id == 200) {
        let source_id = entry.source_entity_id?;
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
        else {
            continue;
        };
        if row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder {
            cylinders_by_source
                .entry(source_id)
                .or_default()
                .push(entry.entity_id);
        }
    }
    let cylinder_sources = cylinders_by_source
        .values()
        .filter(|ids| ids.len() == 2)
        .cloned()
        .collect::<Vec<_>>();
    let existing_geometries = ir
        .model
        .surfaces
        .iter()
        .filter_map(|surface| {
            let id = surface
                .id
                .0
                .strip_prefix("creo:visibgeom:surface#")?
                .parse::<u32>()
                .ok()?;
            Some((id, surface.geometry.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    counterbore_source_patch_geometries(
        &cylinder_sources,
        &existing_geometries,
        bore_diameter,
        counterbore_diameter,
    )
}

fn counterbore_source_patch_geometries(
    cylinder_sources: &[Vec<u32>],
    existing_geometries: &BTreeMap<u32, SurfaceGeometry>,
    bore_diameter: f64,
    counterbore_diameter: f64,
) -> Option<Vec<(u32, SurfaceGeometry)>> {
    let [first_source, second_source] = cylinder_sources else {
        return None;
    };
    let counterbore_radius = 0.5 * counterbore_diameter;
    let source_carrier = |ids: &[u32]| {
        let carriers = ids
            .iter()
            .filter_map(|id| existing_geometries.get(id))
            .filter(|geometry| {
                matches!(geometry, SurfaceGeometry::Cylinder { radius, .. } if (*radius - counterbore_radius).abs() <= 1e-9)
            })
            .collect::<Vec<_>>();
        let first = (*carriers.first()?).clone();
        carriers
            .iter()
            .all(|candidate| **candidate == first)
            .then_some(first)
    };
    let (counterbore_source, bore_source, carrier) =
        match (source_carrier(first_source), source_carrier(second_source)) {
            (Some(carrier), None) => (first_source, second_source, carrier),
            (None, Some(carrier)) => (second_source, first_source, carrier),
            _ => return None,
        };
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        ..
    } = carrier
    else {
        return None;
    };
    let geometry = |radius| SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    };
    Some(
        counterbore_source
            .iter()
            .map(|id| (*id, geometry(counterbore_radius)))
            .chain(
                bore_source
                    .iter()
                    .map(|id| (*id, geometry(0.5 * bore_diameter))),
            )
            .collect(),
    )
}

fn simple_hole_geometry(scan: &ContainerScan, feature_id: u32) -> Option<SimpleHoleGeometry> {
    let cap_rows = feature_outline_planes(scan, feature_id)
        .into_iter()
        .map(|(id, origin, normal)| {
            let envelopes = scan
                .planes
                .envelopes
                .iter()
                .filter(|envelope| envelope.surface_id == id)
                .collect::<Vec<_>>();
            let [envelope] = envelopes.as_slice() else {
                return None;
            };
            Some((
                id,
                origin,
                normal,
                plane_envelope_corners(&envelope.envelope)?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let [first, second] = cap_rows.as_slice() else {
        return None;
    };
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && !table.surface_ids.is_empty())
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let [entry_plane, termination_plane, first_cylinder, second_cylinder] =
        table.entry_ids.as_slice()
    else {
        return None;
    };
    if *entry_plane != first.0 || *termination_plane != second.0 {
        return None;
    }
    let cylinder_ids = [*first_cylinder, *second_cylinder];
    if cylinder_ids.iter().any(|id| {
        !crate::surface::unique_surface_row(&scan.surfaces.rows, *id).is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    }) {
        return None;
    }
    let (_, direction, extent) =
        hole_placement([*first, *second].map(|(id, origin, normal, _)| (id, origin, normal)))?;
    Some(SimpleHoleGeometry {
        entry_surface_id: Some(*entry_plane),
        cylinder_ids: cylinder_ids.to_vec(),
        direction,
        extent,
        geometry: hole_cylinder_from_cap_outlines([*first, *second])?,
    })
}

fn compact_simple_hole_cylinder_id(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Option<u32> {
    let ids = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .filter_map(|table| {
            let [topology_a, topology_b, bottom, side] = table.entries.as_slice() else {
                return None;
            };
            (table.entry_ids
                == [
                    topology_a.entity_id,
                    topology_b.entity_id,
                    bottom.entity_id,
                    side.entity_id,
                ]
                && [
                    topology_a.class_id,
                    topology_b.class_id,
                    bottom.class_id,
                    side.class_id,
                ] == [204, 203, 200, 200]
                && topology_a.source_entity_id.is_none()
                && topology_b.source_entity_id.is_none()
                && bottom.source_entity_id == Some(0)
                && side.source_entity_id.is_none()
                && !rows.iter().any(|row| {
                    row.id == topology_a.entity_id
                        || row.id == topology_b.entity_id
                        || row.id == bottom.entity_id
                })
                && rows.iter().filter(|row| row.id == side.entity_id).count() == 1
                && rows.iter().any(|row| {
                    row.id == side.entity_id
                        && row.feature_id == feature_id
                        && row.kind == crate::surface::SurfaceKind::Cylinder
                }))
            .then_some(side.entity_id)
        })
        .collect::<Vec<_>>();
    let [id] = ids.as_slice() else {
        return None;
    };
    Some(*id)
}

fn compact_simple_hole_geometry(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<SimpleHoleGeometry> {
    let cylinder_id = compact_simple_hole_cylinder_id(
        feature_id,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    )?;
    let frame = crate::surface::unique_surface_parameter(&scan.surfaces.parameters, cylinder_id)?
        .positional_cylinder_frame?;
    let length = frame.length?;
    Some(SimpleHoleGeometry {
        entry_surface_id: None,
        cylinder_ids: vec![cylinder_id],
        direction: frame.axis,
        extent: Termination::Blind {
            length: Length(length),
        },
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(frame.origin[0], frame.origin[1], frame.origin[2]),
            axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
            ref_direction: Vector3::new(
                frame.ref_direction[0],
                frame.ref_direction[1],
                frame.ref_direction[2],
            ),
            radius: frame.radius,
        },
    })
}

fn circular_sweep_cylinder_from_cap_outlines(
    caps: [PartialCapOutline; 2],
) -> Option<SurfaceGeometry> {
    let (_, axis, _) = hole_placement(caps.map(|(id, origin, normal, _)| (id, origin, normal)))?;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - 1e-9
            && (0..3).all(|other| other == *index || axis[other].abs() < 1e-9)
    })?;
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let circles = caps
        .iter()
        .filter_map(|(_, _, _, corners)| cap_square_center_radius((*corners)?, axis_index))
        .collect::<Vec<_>>();
    let [_, _] = circles.as_slice() else {
        return None;
    };
    let (center, radius) = *circles.first()?;
    let scale = center
        .iter()
        .chain(std::iter::once(&radius))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if circles.iter().skip(1).any(|(other_center, other_radius)| {
        radial
            .iter()
            .any(|index| (center[*index] - other_center[*index]).abs() > 1e-9 * scale)
            || (radius - other_radius).abs() > 1e-9 * scale
    }) {
        return None;
    }
    let mut ref_direction = [0.0; 3];
    ref_direction[radial[0]] = 1.0;
    Some(SurfaceGeometry::Cylinder {
        origin: Point3::new(center[0], center[1], center[2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
        ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
        radius,
    })
}

#[derive(Debug, Clone, PartialEq)]
struct CircularSweepGeometry {
    cylinder_ids: Vec<u32>,
    section_definition_id: Option<u32>,
    direction: [f64; 3],
    extent: ExtrudeExtent,
    geometry: SurfaceGeometry,
}

fn single_cap_circular_sweep_geometry(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<CircularSweepGeometry> {
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && !table.surface_ids.is_empty())
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let [rowless_cap, cap_id, profile_id, cylinder_id] = table.entries.as_slice() else {
        return None;
    };
    ([
        rowless_cap.class_id,
        cap_id.class_id,
        profile_id.class_id,
        cylinder_id.class_id,
    ] == [204, 203, 200, 200]
        && profile_id.source_entity_id.is_some()
        && cylinder_id.source_entity_id.is_none()
        && table
            .non_surface_entity_ids
            .contains(&rowless_cap.entity_id)
        && table.non_surface_entity_ids.contains(&profile_id.entity_id))
    .then_some(())?;
    crate::surface::unique_surface_row(&scan.surfaces.rows, cap_id.entity_id)
        .is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .then_some(())?;
    crate::surface::unique_surface_row(&scan.surfaces.rows, cylinder_id.entity_id)
        .is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
        .then_some(())?;
    let planes = feature_outline_planes(scan, feature_id)
        .into_iter()
        .filter(|plane| plane.0 == cap_id.entity_id)
        .collect::<Vec<_>>();
    let [plane] = planes.as_slice() else {
        return None;
    };
    let envelopes = scan
        .planes
        .envelopes
        .iter()
        .filter(|envelope| envelope.surface_id == cap_id.entity_id)
        .collect::<Vec<_>>();
    let [envelope] = envelopes.as_slice() else {
        return None;
    };
    let cap = (
        plane.0,
        plane.1,
        plane.2,
        plane_envelope_corners(&envelope.envelope),
    );
    let transforms = scan
        .features
        .section_transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let [transform] = transforms.as_slice() else {
        return None;
    };
    let (extent, direction) =
        extrusion_extent_and_direction(transform.origin, transform.normal, [(plane.1, plane.2)])?;
    Some(CircularSweepGeometry {
        cylinder_ids: vec![cylinder_id.entity_id],
        section_definition_id: Some(transform.definition_id),
        direction,
        extent,
        geometry: cylinder_from_single_cap_outline(cap)?,
    })
}

fn circular_sweep_feature_definition(
    profile: ProfileRef,
    sweep: &CircularSweepGeometry,
    op: BooleanOp,
    solid: Option<bool>,
) -> IrFeatureDefinition {
    IrFeatureDefinition::Extrude {
        profile,
        direction: cadmpeg_ir::features::ExtrudeDirection::Explicit(Vector3::new(
            sweep.direction[0],
            sweep.direction[1],
            sweep.direction[2],
        )),
        start: cadmpeg_ir::features::ExtrudeStart::default(),
        extent: sweep.extent.clone(),
        op,
        direction_source: None,
        solid,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    }
}

fn circular_sweep_geometry(scan: &ContainerScan, feature_id: u32) -> Option<CircularSweepGeometry> {
    two_cap_circular_sweep_geometry(scan, feature_id)
        .or_else(|| single_cap_circular_sweep_geometry(scan, feature_id))
}

fn two_cap_circular_sweep_geometry(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<CircularSweepGeometry> {
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && !table.surface_ids.is_empty())
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    if table
        .entries
        .iter()
        .map(|entry| entry.class_id)
        .eq([204, 203, 200, 200])
    {
        return None;
    }
    let [first_plane, second_plane, first_cylinder, second_cylinder] = table.entry_ids.as_slice()
    else {
        return None;
    };
    let placed_planes = feature_outline_planes(scan, feature_id);
    let [first, second] = placed_planes.as_slice() else {
        return None;
    };
    if first.0 != *first_plane || second.0 != *second_plane {
        return None;
    }
    let cap = |plane: &(u32, [f64; 3], [f64; 3])| {
        let envelopes = scan
            .planes
            .envelopes
            .iter()
            .filter(|envelope| envelope.surface_id == plane.0)
            .collect::<Vec<_>>();
        let corners = match envelopes.as_slice() {
            [envelope] => plane_envelope_corners(&envelope.envelope),
            _ => None,
        };
        (plane.0, plane.1, plane.2, corners)
    };
    let cylinder_ids = [*first_cylinder, *second_cylinder];
    if cylinder_ids.iter().any(|id| {
        !crate::surface::unique_surface_row(&scan.surfaces.rows, *id).is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    }) {
        return None;
    }
    let (_, direction, termination) = hole_placement([*first, *second])?;
    Some(CircularSweepGeometry {
        cylinder_ids: cylinder_ids.to_vec(),
        section_definition_id: None,
        direction,
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination,
                draft: None,
                offset: None,
            },
        },
        geometry: circular_sweep_cylinder_from_cap_outlines([cap(first), cap(second)])?,
    })
}

fn extrusion_span(
    profile_origin: [f64; 3],
    direction: [f64; 3],
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<ExtrusionSpan> {
    let direction_length = direction
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    if direction_length <= f64::EPSILON {
        return None;
    }
    let direction = direction.map(|value| value / direction_length);
    let mut offsets = Vec::<f64>::new();
    for (origin, normal) in planes {
        let normal_length = normal.iter().map(|value| value * value).sum::<f64>().sqrt();
        if normal_length <= f64::EPSILON {
            continue;
        }
        let parallel = normal
            .iter()
            .zip(direction)
            .map(|(left, right)| left * right)
            .sum::<f64>()
            .abs();
        if (parallel / normal_length - 1.0).abs() > 1e-9 {
            continue;
        }
        let offset = origin
            .iter()
            .zip(profile_origin)
            .zip(direction)
            .map(|((coordinate, base), axis)| (coordinate - base) * axis)
            .sum::<f64>();
        if offset.abs() <= 1e-12 {
            continue;
        }
        let scale = offset.abs().max(1.0);
        if !offsets
            .iter()
            .any(|known| (known - offset).abs() <= 1e-9 * scale)
        {
            offsets.push(offset);
        }
    }
    let lower = offsets
        .iter()
        .copied()
        .filter(|offset| *offset < 0.0)
        .min_by(f64::total_cmp);
    let upper = offsets
        .iter()
        .copied()
        .filter(|offset| *offset > 0.0)
        .max_by(f64::total_cmp);
    match (lower, upper) {
        (Some(lower), Some(upper)) => Some(ExtrusionSpan { lower, upper }),
        (Some(lower), None) => Some(ExtrusionSpan { lower, upper: 0.0 }),
        (None, Some(upper)) => Some(ExtrusionSpan { lower: 0.0, upper }),
        (None, None) => None,
    }
}

fn extrusion_extent_and_direction(
    profile_origin: [f64; 3],
    direction: [f64; 3],
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let span = extrusion_span(profile_origin, direction, planes)?;
    let direction = normalized(direction)?;
    if span.lower == 0.0 || span.upper == 0.0 {
        let signed_length = if span.upper == 0.0 {
            span.lower
        } else {
            span.upper
        };
        return Some((
            ExtrudeExtent::OneSided {
                side: blind_extrude_side(signed_length.abs()),
            },
            direction.map(|value| value * signed_length.signum()),
        ));
    }
    let first = span.upper;
    let second = -span.lower;
    let scale = first.max(second).max(1.0);
    let extent = if (first - second).abs() <= 1e-9 * scale {
        ExtrudeExtent::Symmetric {
            side: blind_extrude_side(first + second),
        }
    } else {
        ExtrudeExtent::TwoSided {
            first: blind_extrude_side(first),
            second: blind_extrude_side(second),
        }
    };
    Some((extent, direction))
}

fn blind_extrude_side(length: f64) -> ExtrudeSide {
    ExtrudeSide {
        termination: Termination::Blind {
            length: Length(length),
        },
        draft: None,
        offset: None,
    }
}

#[cfg(test)]
mod resolved_sketch_tests;

#[derive(Clone, Copy)]
struct PlaneEquation {
    origin: [f64; 3],
    normal: [f64; 3],
}

#[derive(Clone, Copy)]
struct CylinderEquation {
    origin: [f64; 3],
    axis: [f64; 3],
    ref_direction: [f64; 3],
    radius: f64,
}

#[derive(Clone, Copy)]
struct ConeEquation {
    origin: [f64; 3],
    axis: [f64; 3],
    ref_direction: [f64; 3],
    radius: f64,
    ratio: f64,
    half_angle: f64,
}

fn circular_cone(cone: ConeEquation) -> bool {
    cone.ratio.is_finite() && (cone.ratio - 1.0).abs() <= 1e-12
}

#[derive(Clone, Copy)]
struct SphereEquation {
    center: [f64; 3],
    ref_direction: [f64; 3],
    radius: f64,
}

#[derive(Clone, Copy)]
struct TorusEquation {
    center: [f64; 3],
    axis: [f64; 3],
    ref_direction: [f64; 3],
    major_radius: f64,
    minor_radius: f64,
}

#[derive(Clone, Copy)]
enum CarrierEquation {
    Plane(PlaneEquation),
    Cylinder(CylinderEquation),
    Cone(ConeEquation),
    Sphere(SphereEquation),
    Torus(TorusEquation),
}

#[derive(Clone, Copy)]
struct QuadricEquation {
    matrix: [[f64; 3]; 3],
    linear: [f64; 3],
    constant: f64,
}

#[derive(Clone, Copy)]
struct PlaneConicEquation {
    uu: f64,
    uv: f64,
    vv: f64,
    u: f64,
    v: f64,
    constant: f64,
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1].mul_add(right[2], -(left[2] * right[1])),
        left[2].mul_add(right[0], -(left[0] * right[2])),
        left[0].mul_add(right[1], -(left[1] * right[0])),
    ]
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0].mul_add(right[0], left[1].mul_add(right[1], left[2] * right[2]))
}

fn matrix_vector(matrix: [[f64; 3]; 3], vector: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| dot(row, vector))
}

fn outer_product(left: [f64; 3], right: [f64; 3]) -> [[f64; 3]; 3] {
    left.map(|left| right.map(|right| left * right))
}

fn carrier_quadric(carrier: CarrierEquation) -> Option<QuadricEquation> {
    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    match carrier {
        CarrierEquation::Cylinder(cylinder) => {
            let axis = normalized(cylinder.axis)?;
            if !cylinder.radius.is_finite() || cylinder.radius <= 0.0 {
                return None;
            }
            let axis_projection = outer_product(axis, axis);
            let matrix = std::array::from_fn(|row| {
                std::array::from_fn(|column| identity[row][column] - axis_projection[row][column])
            });
            let matrix_origin = matrix_vector(matrix, cylinder.origin);
            Some(QuadricEquation {
                matrix,
                linear: matrix_origin.map(|value| -2.0 * value),
                constant: dot(cylinder.origin, matrix_origin) - cylinder.radius * cylinder.radius,
            })
        }
        CarrierEquation::Cone(cone) => {
            let axis = normalized(cone.axis)?;
            let x_axis = normalized(cone.ref_direction)?;
            if dot(axis, x_axis).abs() > 1e-10
                || !cone.ratio.is_finite()
                || cone.ratio <= 0.0
                || !cone.radius.is_finite()
                || !(0.0..std::f64::consts::FRAC_PI_2).contains(&cone.half_angle)
            {
                return None;
            }
            let y_axis = cross(axis, x_axis);
            let slope = cone.half_angle.tan();
            let x_projection = outer_product(x_axis, x_axis);
            let y_projection = outer_product(y_axis, y_axis);
            let axis_projection = outer_product(axis, axis);
            let ratio_squared = cone.ratio * cone.ratio;
            let matrix = std::array::from_fn(|row| {
                std::array::from_fn(|column| {
                    x_projection[row][column] + y_projection[row][column] / ratio_squared
                        - slope * slope * axis_projection[row][column]
                })
            });
            let matrix_origin = matrix_vector(matrix, cone.origin);
            let radius_slope = cone.radius * slope;
            Some(QuadricEquation {
                matrix,
                linear: std::array::from_fn(|index| {
                    -2.0 * matrix_origin[index] - 2.0 * radius_slope * axis[index]
                }),
                constant: dot(cone.origin, matrix_origin)
                    + 2.0 * radius_slope * dot(axis, cone.origin)
                    - cone.radius * cone.radius,
            })
        }
        CarrierEquation::Sphere(sphere) => {
            if !sphere.radius.is_finite() || sphere.radius <= 0.0 {
                return None;
            }
            Some(QuadricEquation {
                matrix: identity,
                linear: sphere.center.map(|value| -2.0 * value),
                constant: dot(sphere.center, sphere.center) - sphere.radius * sphere.radius,
            })
        }
        CarrierEquation::Plane(_) | CarrierEquation::Torus(_) => None,
    }
}

fn restrict_quadric_to_plane(
    quadric: QuadricEquation,
    origin: [f64; 3],
    u_axis: [f64; 3],
    v_axis: [f64; 3],
) -> PlaneConicEquation {
    let matrix_origin = matrix_vector(quadric.matrix, origin);
    let matrix_u = matrix_vector(quadric.matrix, u_axis);
    let matrix_v = matrix_vector(quadric.matrix, v_axis);
    PlaneConicEquation {
        uu: dot(u_axis, matrix_u),
        uv: 2.0 * dot(u_axis, matrix_v),
        vv: dot(v_axis, matrix_v),
        u: 2.0 * dot(u_axis, matrix_origin) + dot(quadric.linear, u_axis),
        v: 2.0 * dot(v_axis, matrix_origin) + dot(quadric.linear, v_axis),
        constant: dot(origin, matrix_origin) + dot(quadric.linear, origin) + quadric.constant,
    }
}

fn solve_planes(planes: &[PlaneEquation]) -> Option<[f64; 3]> {
    for first in 0..planes.len() {
        for second in first + 1..planes.len() {
            for third in second + 1..planes.len() {
                let a = planes[first];
                let b = planes[second];
                let c = planes[third];
                let b_cross_c = cross(b.normal, c.normal);
                let determinant = dot(a.normal, b_cross_c);
                if determinant.abs() <= 1e-9 {
                    continue;
                }
                let distances = [
                    dot(a.normal, a.origin),
                    dot(b.normal, b.origin),
                    dot(c.normal, c.origin),
                ];
                let c_cross_a = cross(c.normal, a.normal);
                let a_cross_b = cross(a.normal, b.normal);
                let point = [0, 1, 2].map(|axis| {
                    (distances[0] * b_cross_c[axis]
                        + distances[1] * c_cross_a[axis]
                        + distances[2] * a_cross_b[axis])
                        / determinant
                });
                if point.iter().all(|value| value.is_finite())
                    && planes.iter().all(|plane| {
                        (dot(plane.normal, point) - dot(plane.normal, plane.origin)).abs() <= 1e-6
                    })
                {
                    return Some(point);
                }
            }
        }
    }
    None
}

fn plane_intersection_line(
    first: PlaneEquation,
    second: PlaneEquation,
) -> Option<([f64; 3], [f64; 3])> {
    let direction = cross(first.normal, second.normal);
    let denominator = dot(direction, direction);
    if denominator <= 1e-18 {
        return None;
    }
    let first_distance = dot(first.normal, first.origin);
    let second_distance = dot(second.normal, second.origin);
    let second_cross_direction = cross(second.normal, direction);
    let direction_cross_first = cross(direction, first.normal);
    let origin = std::array::from_fn(|index| {
        (first_distance * second_cross_direction[index]
            + second_distance * direction_cross_first[index])
            / denominator
    });
    Some((origin, normalized(direction)?))
}

fn intersect_two_planes_with_quadric(
    first: PlaneEquation,
    second: PlaneEquation,
    carrier: CarrierEquation,
) -> Vec<[f64; 3]> {
    let Some((line_origin, direction)) = plane_intersection_line(first, second) else {
        return Vec::new();
    };
    let Some(quadric) = carrier_quadric(carrier) else {
        return Vec::new();
    };
    let matrix_origin = matrix_vector(quadric.matrix, line_origin);
    let matrix_direction = matrix_vector(quadric.matrix, direction);
    let quadratic = dot(direction, matrix_direction);
    let linear = 2.0 * dot(line_origin, matrix_direction) + dot(quadric.linear, direction);
    let constant =
        dot(line_origin, matrix_origin) + dot(quadric.linear, line_origin) + quadric.constant;
    quadratic_real_roots(quadratic, linear, constant)
        .into_iter()
        .map(|parameter| {
            std::array::from_fn(|index| line_origin[index] + parameter * direction[index])
        })
        .filter(|point| {
            point.iter().all(|value| value.is_finite())
                && point_on_carrier(*point, CarrierEquation::Plane(first))
                && point_on_carrier(*point, CarrierEquation::Plane(second))
                && point_on_carrier(*point, carrier)
        })
        .collect()
}

fn polynomial_value(coefficients: &[f64], parameter: f64) -> f64 {
    coefficients.iter().rev().fold(0.0, |value, coefficient| {
        value.mul_add(parameter, *coefficient)
    })
}

fn real_polynomial_roots(coefficients: &[f64]) -> Vec<f64> {
    let scale = coefficients
        .iter()
        .copied()
        .map(f64::abs)
        .fold(0.0, f64::max);
    if scale == 0.0 || !scale.is_finite() {
        return Vec::new();
    }
    let mut coefficients = coefficients
        .iter()
        .map(|coefficient| coefficient / scale)
        .collect::<Vec<_>>();
    while coefficients.len() > 1
        && coefficients
            .last()
            .is_some_and(|value| value.abs() <= 1e-14)
    {
        coefficients.pop();
    }
    let degree = coefficients.len() - 1;
    if degree == 0 {
        return Vec::new();
    }
    if degree == 1 {
        return vec![-coefficients[0] / coefficients[1]];
    }
    let derivative = coefficients
        .iter()
        .enumerate()
        .skip(1)
        .map(|(power, coefficient)| *coefficient * power as f64)
        .collect::<Vec<_>>();
    let leading = coefficients[degree].abs();
    let bound = 1.0
        + coefficients[..degree]
            .iter()
            .copied()
            .map(f64::abs)
            .fold(0.0, f64::max)
            / leading;
    let mut boundaries = vec![-bound];
    boundaries.extend(
        real_polynomial_roots(&derivative)
            .into_iter()
            .filter(|root| root.is_finite() && *root > -bound && *root < bound),
    );
    boundaries.push(bound);
    boundaries.sort_by(f64::total_cmp);
    let value_tolerance = 1e-11;
    let mut roots = boundaries
        .iter()
        .copied()
        .filter(|parameter| polynomial_value(&coefficients, *parameter).abs() <= value_tolerance)
        .collect::<Vec<_>>();
    for interval in boundaries.windows(2) {
        let (mut lower, mut upper) = (interval[0], interval[1]);
        let mut lower_value = polynomial_value(&coefficients, lower);
        let upper_value = polynomial_value(&coefficients, upper);
        if lower_value * upper_value >= 0.0 {
            continue;
        }
        for _ in 0..80 {
            let midpoint = 0.5 * (lower + upper);
            let midpoint_value = polynomial_value(&coefficients, midpoint);
            if lower_value * midpoint_value <= 0.0 {
                upper = midpoint;
            } else {
                lower = midpoint;
                lower_value = midpoint_value;
            }
        }
        roots.push(0.5 * (lower + upper));
    }
    roots.sort_by(f64::total_cmp);
    roots
        .into_iter()
        .fold(Vec::<f64>::new(), |mut unique, root| {
            if let Some(previous) = unique.last_mut() {
                let tolerance = 1e-7 * previous.abs().max(root.abs()).max(1.0);
                if (*previous - root).abs() <= tolerance {
                    if polynomial_value(&coefficients, root).abs()
                        < polynomial_value(&coefficients, *previous).abs()
                    {
                        *previous = root;
                    }
                    return unique;
                }
            }
            unique.push(root);
            unique
        })
}

fn polynomial_product(first: &[f64], second: &[f64]) -> Vec<f64> {
    let mut product = vec![0.0; first.len() + second.len() - 1];
    for (first_power, first_coefficient) in first.iter().enumerate() {
        for (second_power, second_coefficient) in second.iter().enumerate() {
            product[first_power + second_power] += first_coefficient * second_coefficient;
        }
    }
    product
}

const QUARTIC_RESULTANT_PERMUTATIONS: [([usize; 4], f64); 24] = [
    ([0, 1, 2, 3], 1.0),
    ([0, 1, 3, 2], -1.0),
    ([0, 2, 1, 3], -1.0),
    ([0, 2, 3, 1], 1.0),
    ([0, 3, 1, 2], 1.0),
    ([0, 3, 2, 1], -1.0),
    ([1, 0, 2, 3], -1.0),
    ([1, 0, 3, 2], 1.0),
    ([1, 2, 0, 3], 1.0),
    ([1, 2, 3, 0], -1.0),
    ([1, 3, 0, 2], -1.0),
    ([1, 3, 2, 0], 1.0),
    ([2, 0, 1, 3], 1.0),
    ([2, 0, 3, 1], -1.0),
    ([2, 1, 0, 3], -1.0),
    ([2, 1, 3, 0], 1.0),
    ([2, 3, 0, 1], 1.0),
    ([2, 3, 1, 0], -1.0),
    ([3, 0, 1, 2], -1.0),
    ([3, 0, 2, 1], 1.0),
    ([3, 1, 0, 2], 1.0),
    ([3, 1, 2, 0], -1.0),
    ([3, 2, 0, 1], -1.0),
    ([3, 2, 1, 0], 1.0),
];

fn conic_resultant(first: PlaneConicEquation, second: PlaneConicEquation) -> Vec<f64> {
    let zero = vec![0.0];
    let first_y2 = vec![first.vv];
    let first_y = vec![first.v, first.uv];
    let first_constant = vec![first.constant, first.u, first.uu];
    let second_y2 = vec![second.vv];
    let second_y = vec![second.v, second.uv];
    let second_constant = vec![second.constant, second.u, second.uu];
    let matrix = [
        [
            first_y2.clone(),
            first_y.clone(),
            first_constant.clone(),
            zero.clone(),
        ],
        [zero.clone(), first_y2, first_y, first_constant],
        [
            second_y2.clone(),
            second_y.clone(),
            second_constant.clone(),
            zero.clone(),
        ],
        [zero, second_y2, second_y, second_constant],
    ];
    let mut determinant = vec![0.0; 9];
    for (permutation, sign) in QUARTIC_RESULTANT_PERMUTATIONS {
        let term = (0..4).fold(vec![1.0], |term, row| {
            polynomial_product(&term, &matrix[row][permutation[row]])
        });
        for (power, coefficient) in term.into_iter().enumerate() {
            determinant[power] += sign * coefficient;
        }
    }
    determinant
}

fn quadratic_real_roots(quadratic: f64, linear: f64, constant: f64) -> Vec<f64> {
    let scale = quadratic
        .abs()
        .max(linear.abs())
        .max(constant.abs())
        .max(1.0);
    if quadratic.abs() <= 1e-14 * scale {
        return if linear.abs() > 1e-14 * scale {
            vec![-constant / linear]
        } else {
            Vec::new()
        };
    }
    let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
    if discriminant < -1e-12 * scale * scale {
        return Vec::new();
    }
    let root = if discriminant.abs() <= 1e-12 * scale * scale {
        0.0
    } else {
        discriminant.sqrt()
    };
    let mut roots = vec![(-linear - root) / (2.0 * quadratic)];
    if root > 1e-12 * scale {
        roots.push((-linear + root) / (2.0 * quadratic));
    }
    roots
}

fn plane_conic_value(conic: PlaneConicEquation, u: f64, v: f64) -> f64 {
    conic.uu * u * u
        + conic.uv * u * v
        + conic.vv * v * v
        + conic.u * u
        + conic.v * v
        + conic.constant
}

fn refine_plane_conic_intersection(
    first: PlaneConicEquation,
    second: PlaneConicEquation,
    mut u: f64,
    mut v: f64,
) -> [f64; 2] {
    for _ in 0..12 {
        let first_value = plane_conic_value(first, u, v);
        let second_value = plane_conic_value(second, u, v);
        let first_u = 2.0 * first.uu * u + first.uv * v + first.u;
        let first_v = first.uv * u + 2.0 * first.vv * v + first.v;
        let second_u = 2.0 * second.uu * u + second.uv * v + second.u;
        let second_v = second.uv * u + 2.0 * second.vv * v + second.v;
        let determinant = first_u.mul_add(second_v, -(first_v * second_u));
        let scale = first_u
            .abs()
            .max(first_v.abs())
            .max(second_u.abs())
            .max(second_v.abs())
            .max(1.0);
        if determinant.abs() <= 1e-14 * scale * scale {
            break;
        }
        let delta_u = (-first_value).mul_add(second_v, first_v * second_value) / determinant;
        let delta_v = first_value.mul_add(second_u, -(first_u * second_value)) / determinant;
        u += delta_u;
        v += delta_v;
        if delta_u.abs().max(delta_v.abs()) <= 1e-13 * u.abs().max(v.abs()).max(1.0) {
            break;
        }
    }
    [u, v]
}

fn common_plane_conic_parameters(
    first: PlaneConicEquation,
    second: PlaneConicEquation,
) -> Vec<[f64; 2]> {
    let resultant = conic_resultant(first, second);
    let mut parameters = Vec::<[f64; 2]>::new();
    for u in real_polynomial_roots(&resultant) {
        let first_v_roots = quadratic_real_roots(
            first.vv,
            first.uv.mul_add(u, first.v),
            first.uu * u * u + first.u * u + first.constant,
        );
        let second_v_roots = quadratic_real_roots(
            second.vv,
            second.uv.mul_add(u, second.v),
            second.uu * u * u + second.u * u + second.constant,
        );
        for v in first_v_roots.into_iter().chain(second_v_roots) {
            let candidate = refine_plane_conic_intersection(first, second, u, v);
            let scale = candidate[0].abs().max(candidate[1].abs()).max(1.0);
            let coefficient_scale = [
                first.uu,
                first.uv,
                first.vv,
                first.u,
                first.v,
                first.constant,
                second.uu,
                second.uv,
                second.vv,
                second.u,
                second.v,
                second.constant,
            ]
            .into_iter()
            .map(f64::abs)
            .fold(1.0, f64::max);
            let tolerance = 1e-8 * coefficient_scale * scale * scale;
            if plane_conic_value(first, candidate[0], candidate[1]).abs() <= tolerance
                && plane_conic_value(second, candidate[0], candidate[1]).abs() <= tolerance
                && !parameters.iter().any(|known| {
                    (known[0] - candidate[0])
                        .abs()
                        .max((known[1] - candidate[1]).abs())
                        <= 1e-7 * scale
                })
            {
                parameters.push(candidate);
            }
        }
    }
    parameters
}

fn intersect_plane_with_two_quadrics(
    plane: PlaneEquation,
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<[f64; 3]> {
    let Some(normal) = normalized(plane.normal) else {
        return Vec::new();
    };
    let reference = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        .into_iter()
        .min_by(|left, right| {
            dot(normal, *left)
                .abs()
                .total_cmp(&dot(normal, *right).abs())
        })
        .expect("three reference axes");
    let Some(u_axis) = normalized(cross(normal, reference)) else {
        return Vec::new();
    };
    let v_axis = cross(normal, u_axis);
    let Some(first_quadric) = carrier_quadric(first) else {
        return Vec::new();
    };
    let Some(second_quadric) = carrier_quadric(second) else {
        return Vec::new();
    };
    let first_conic = restrict_quadric_to_plane(first_quadric, plane.origin, u_axis, v_axis);
    let second_conic = restrict_quadric_to_plane(second_quadric, plane.origin, u_axis, v_axis);
    common_plane_conic_parameters(first_conic, second_conic)
        .into_iter()
        .map(|[u, v]| {
            std::array::from_fn(|index| plane.origin[index] + u * u_axis[index] + v * v_axis[index])
        })
        .filter(|point| point_on_carrier(*point, first) && point_on_carrier(*point, second))
        .collect()
}

fn intersect_two_planes_with_torus(
    first: PlaneEquation,
    second: PlaneEquation,
    torus: TorusEquation,
) -> Vec<[f64; 3]> {
    let Some((line_origin, direction)) = plane_intersection_line(first, second) else {
        return Vec::new();
    };
    let Some(axis) = normalized(torus.axis) else {
        return Vec::new();
    };
    if torus.major_radius <= 0.0 || torus.minor_radius <= 0.0 {
        return Vec::new();
    }
    let relative: [f64; 3] = std::array::from_fn(|index| line_origin[index] - torus.center[index]);
    let squared_distance = [dot(relative, relative), 2.0 * dot(relative, direction), 1.0];
    let axial = [dot(relative, axis), dot(direction, axis)];
    let axial_squared = [
        axial[0] * axial[0],
        2.0 * axial[0] * axial[1],
        axial[1] * axial[1],
    ];
    let mut shifted_distance = squared_distance;
    shifted_distance[0] +=
        torus.major_radius * torus.major_radius - torus.minor_radius * torus.minor_radius;
    let mut polynomial = [0.0; 5];
    for (left_power, left) in shifted_distance.into_iter().enumerate() {
        for (right_power, right) in shifted_distance.into_iter().enumerate() {
            polynomial[left_power + right_power] += left * right;
        }
    }
    let radial_scale = 4.0 * torus.major_radius * torus.major_radius;
    for power in 0..=2 {
        polynomial[power] -= radial_scale * (squared_distance[power] - axial_squared[power]);
    }
    let coordinate_scale = torus
        .center
        .into_iter()
        .chain(line_origin)
        .map(f64::abs)
        .fold(
            torus.major_radius.max(torus.minor_radius).max(1.0),
            f64::max,
        );
    real_polynomial_roots(&polynomial)
        .into_iter()
        .map(|parameter| {
            std::array::from_fn(|index| {
                let coordinate = line_origin[index] + parameter * direction[index];
                if coordinate.abs() <= 1e-14 * coordinate_scale {
                    0.0
                } else {
                    coordinate
                }
            })
        })
        .filter(|point| point_on_carrier(*point, CarrierEquation::Torus(torus)))
        .collect()
}

fn intersect_plane_with_circle(
    plane: PlaneEquation,
    center: [f64; 3],
    circle_axis: [f64; 3],
    radius: f64,
) -> Vec<[f64; 3]> {
    let (Some(plane_normal), Some(circle_normal)) =
        (normalized(plane.normal), normalized(circle_axis))
    else {
        return Vec::new();
    };
    let line_direction = cross(plane_normal, circle_normal);
    let denominator = dot(line_direction, line_direction);
    if denominator <= 1e-18 || radius <= 0.0 {
        return Vec::new();
    }
    let plane_distance = dot(plane_normal, plane.origin);
    let circle_distance = dot(circle_normal, center);
    let weighted = std::array::from_fn(|index| {
        plane_distance * circle_normal[index] - circle_distance * plane_normal[index]
    });
    let line_origin = cross(weighted, line_direction).map(|value| value / denominator);
    let relative: [f64; 3] = std::array::from_fn(|index| line_origin[index] - center[index]);
    let parameter_at_nearest = -dot(relative, line_direction) / denominator;
    let nearest: [f64; 3] = std::array::from_fn(|index| {
        line_origin[index] + parameter_at_nearest * line_direction[index]
    });
    let center_to_nearest: [f64; 3] = std::array::from_fn(|index| nearest[index] - center[index]);
    let remaining = radius.mul_add(radius, -dot(center_to_nearest, center_to_nearest));
    let scale = radius.max(1.0);
    if remaining < -1e-12 * scale * scale {
        return Vec::new();
    }
    let parameter_delta = if remaining.abs() <= 1e-12 * scale * scale {
        0.0
    } else {
        remaining.sqrt() / denominator.sqrt()
    };
    let mut points = vec![std::array::from_fn(|index| {
        nearest[index] - parameter_delta * line_direction[index]
    })];
    if parameter_delta > 1e-12 * scale {
        points.push(std::array::from_fn(|index| {
            nearest[index] + parameter_delta * line_direction[index]
        }));
    }
    points
}

fn circle_parameters(geometry: &CurveGeometry) -> Option<([f64; 3], [f64; 3], f64)> {
    let CurveGeometry::Circle {
        center,
        axis,
        radius,
        ..
    } = geometry
    else {
        return None;
    };
    Some((
        [center.x, center.y, center.z],
        [axis.x, axis.y, axis.z],
        *radius,
    ))
}

fn plane_cone_conic(
    plane: PlaneEquation,
    cone: ConeEquation,
) -> Option<(CurveGeometry, &'static str)> {
    let normal = normalized(plane.normal)?;
    let axis = normalized(cone.axis)?;
    let x_axis = normalized(cone.ref_direction)?;
    let slope = cone.half_angle.tan();
    if slope <= 1e-12
        || !slope.is_finite()
        || cone.radius < 0.0
        || cone.ratio <= 0.0
        || !cone.ratio.is_finite()
        || dot(axis, x_axis).abs() > 1e-10
    {
        return None;
    }
    let y_axis = cross(axis, x_axis);
    let alignment = dot(normal, axis);
    let plane_u = normalized(std::array::from_fn(|index| {
        axis[index] - alignment * normal[index]
    }))?;
    let plane_v = normalized(cross(normal, plane_u))?;
    let relative: [f64; 3] = std::array::from_fn(|index| plane.origin[index] - cone.origin[index]);
    let coordinates = |vector: [f64; 3]| {
        [
            dot(vector, x_axis),
            dot(vector, y_axis) / cone.ratio,
            dot(vector, axis),
        ]
    };
    let origin = coordinates(relative);
    let u_coordinates = coordinates(plane_u);
    let v_coordinates = coordinates(plane_v);
    let origin_radius = cone.radius + slope * origin[2];
    let quadratic = |first: [f64; 3], second: [f64; 3]| {
        first[0].mul_add(
            second[0],
            first[1] * second[1] - slope * slope * first[2] * second[2],
        )
    };
    let linear = |direction: [f64; 3]| {
        2.0 * (origin[0].mul_add(
            direction[0],
            origin[1] * direction[1] - origin_radius * slope * direction[2],
        ))
    };
    let quadratic_uu = quadratic(u_coordinates, u_coordinates);
    let quadratic_uv = quadratic(u_coordinates, v_coordinates);
    let quadratic_vv = quadratic(v_coordinates, v_coordinates);
    let linear_u_source = linear(u_coordinates);
    let linear_v_source = linear(v_coordinates);
    let constant = origin[0].mul_add(
        origin[0],
        origin[1] * origin[1] - origin_radius * origin_radius,
    );
    let angle = 0.5 * (2.0 * quadratic_uv).atan2(quadratic_uu - quadratic_vv);
    let (sine, cosine) = angle.sin_cos();
    let first_direction =
        std::array::from_fn::<_, 3, _>(|index| cosine * plane_u[index] + sine * plane_v[index]);
    let second_direction =
        std::array::from_fn::<_, 3, _>(|index| -sine * plane_u[index] + cosine * plane_v[index]);
    let first_quadratic = quadratic_uu * cosine * cosine
        + 2.0 * quadratic_uv * cosine * sine
        + quadratic_vv * sine * sine;
    let second_quadratic = quadratic_uu * sine * sine - 2.0 * quadratic_uv * cosine * sine
        + quadratic_vv * cosine * cosine;
    let first_linear = linear_u_source * cosine + linear_v_source * sine;
    let second_linear = -linear_u_source * sine + linear_v_source * cosine;
    let opposite_signs = first_quadratic.is_sign_negative() != second_quadratic.is_sign_negative();
    let keep_first = if opposite_signs {
        first_quadratic.is_sign_negative()
    } else {
        first_quadratic.abs() <= second_quadratic.abs()
    };
    let (quadratic_u, quadratic_v, linear_u, linear_v, principal_u, principal_v) = if keep_first {
        (
            first_quadratic,
            second_quadratic,
            first_linear,
            second_linear,
            first_direction,
            second_direction,
        )
    } else {
        (
            second_quadratic,
            first_quadratic,
            second_linear,
            first_linear,
            second_direction,
            first_direction,
        )
    };
    let coefficient_scale = quadratic_u
        .abs()
        .max(quadratic_v.abs())
        .max(linear_u.abs())
        .max(linear_v.abs())
        .max(constant.abs())
        .max(1.0);
    let point = |u_parameter: f64, v_parameter: f64| {
        Point3::new(
            plane.origin[0] + u_parameter * principal_u[0] + v_parameter * principal_v[0],
            plane.origin[1] + u_parameter * principal_u[1] + v_parameter * principal_v[1],
            plane.origin[2] + u_parameter * principal_u[2] + v_parameter * principal_v[2],
        )
    };
    let axis_vector = Vector3::new(normal[0], normal[1], normal[2]);
    if quadratic_u.abs() <= 1e-12 * coefficient_scale {
        if linear_u.abs() <= 1e-12 * coefficient_scale {
            return None;
        }
        let vertex_v = -linear_v / (2.0 * quadratic_v);
        let shifted_constant = constant - linear_v * linear_v / (4.0 * quadratic_v);
        let vertex_u = -shifted_constant / linear_u;
        let opening = -linear_u / quadratic_v;
        if opening.abs() <= 1e-12 || !opening.is_finite() {
            return None;
        }
        let direction = principal_u.map(|value| value * opening.signum());
        return Some((
            CurveGeometry::Parabola {
                vertex: point(vertex_u, vertex_v),
                axis: axis_vector,
                major_direction: Vector3::new(direction[0], direction[1], direction[2]),
                focal_distance: opening.abs() / 4.0,
            },
            "plane_cone_parabola",
        ));
    }
    let center_u = -linear_u / (2.0 * quadratic_u);
    let center_v = -linear_v / (2.0 * quadratic_v);
    let shifted_constant = constant
        - linear_u * linear_u / (4.0 * quadratic_u)
        - linear_v * linear_v / (4.0 * quadratic_v);
    let value_scale = shifted_constant.abs().max(coefficient_scale).max(1.0);
    if shifted_constant.abs() <= 1e-12 * value_scale {
        return None;
    }
    let center = point(center_u, center_v);
    if quadratic_u > 0.0 {
        if shifted_constant >= 0.0 {
            return None;
        }
        let u_radius = (-shifted_constant / quadratic_u).sqrt();
        let v_radius = (-shifted_constant / quadratic_v).sqrt();
        let (major_direction, major_radius, minor_radius) = if u_radius >= v_radius {
            (principal_u, u_radius, v_radius)
        } else {
            (principal_v, v_radius, u_radius)
        };
        return Some((
            CurveGeometry::Ellipse {
                center,
                axis: axis_vector,
                major_direction: Vector3::new(
                    major_direction[0],
                    major_direction[1],
                    major_direction[2],
                ),
                major_radius,
                minor_radius,
            },
            "plane_cone_ellipse",
        ));
    }
    let (major_direction, major_radius, minor_radius) = if shifted_constant > 0.0 {
        (
            principal_u,
            (shifted_constant / -quadratic_u).sqrt(),
            (shifted_constant / quadratic_v).sqrt(),
        )
    } else {
        (
            principal_v,
            (-shifted_constant / quadratic_v).sqrt(),
            (-shifted_constant / -quadratic_u).sqrt(),
        )
    };
    Some((
        CurveGeometry::Hyperbola {
            center,
            axis: axis_vector,
            major_direction: Vector3::new(
                major_direction[0],
                major_direction[1],
                major_direction[2],
            ),
            major_radius,
            minor_radius,
        },
        "plane_cone_hyperbola",
    ))
}

fn point_on_carrier(point: [f64; 3], carrier: CarrierEquation) -> bool {
    match carrier {
        CarrierEquation::Plane(plane) => {
            let residual = dot(plane.normal, point) - dot(plane.normal, plane.origin);
            residual.abs() <= 1e-7
        }
        CarrierEquation::Cylinder(cylinder) => {
            let Some(axis) = normalized(cylinder.axis) else {
                return false;
            };
            let relative = std::array::from_fn(|index| point[index] - cylinder.origin[index]);
            let axial = dot(relative, axis);
            let radial = std::array::from_fn(|index| relative[index] - axial * axis[index]);
            (dot(radial, radial).sqrt() - cylinder.radius).abs() <= 1e-7 * cylinder.radius.max(1.0)
        }
        CarrierEquation::Cone(cone) => {
            let (Some(axis), Some(x_axis)) =
                (normalized(cone.axis), normalized(cone.ref_direction))
            else {
                return false;
            };
            if cone.ratio <= 0.0 || !cone.ratio.is_finite() || dot(axis, x_axis).abs() > 1e-10 {
                return false;
            }
            let y_axis = cross(axis, x_axis);
            let relative = std::array::from_fn(|index| point[index] - cone.origin[index]);
            let axial = dot(relative, axis);
            let radius = cone.radius + axial * cone.half_angle.tan();
            let radial_x = dot(relative, x_axis);
            let radial_y = dot(relative, y_axis) / cone.ratio;
            (radial_x.hypot(radial_y) - radius.abs()).abs() <= 1e-7 * radius.abs().max(1.0)
        }
        CarrierEquation::Sphere(sphere) => {
            let relative = std::array::from_fn(|index| point[index] - sphere.center[index]);
            (dot(relative, relative).sqrt() - sphere.radius).abs() <= 1e-7 * sphere.radius.max(1.0)
        }
        CarrierEquation::Torus(torus) => {
            let Some(axis) = normalized(torus.axis) else {
                return false;
            };
            let relative = std::array::from_fn(|index| point[index] - torus.center[index]);
            let axial = dot(relative, axis);
            let radial = std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let tube_distance = (dot(radial, radial).sqrt() - torus.major_radius).hypot(axial);
            (tube_distance - torus.minor_radius).abs()
                <= 1e-7 * torus.minor_radius.max(torus.major_radius).max(1.0)
        }
    }
}

fn tangent_sphere_point(first: SphereEquation, second: SphereEquation) -> Option<[f64; 3]> {
    let delta: [f64; 3] = std::array::from_fn(|index| second.center[index] - first.center[index]);
    let distance = dot(delta, delta).sqrt();
    if distance <= 1e-12 || first.radius <= 0.0 || second.radius <= 0.0 {
        return None;
    }
    let external = first.radius + second.radius;
    let internal = (first.radius - second.radius).abs();
    let scale = external.max(distance).max(1.0);
    if (distance - external).abs() > 1e-9 * scale && (distance - internal).abs() > 1e-9 * scale {
        return None;
    }
    let axial = (distance * distance + first.radius * first.radius - second.radius * second.radius)
        / (2.0 * distance);
    Some(std::array::from_fn(|index| {
        first.center[index] + axial * delta[index] / distance
    }))
}

fn tangent_plane_sphere_point(plane: PlaneEquation, sphere: SphereEquation) -> Option<[f64; 3]> {
    let normal = normalized(plane.normal)?;
    let signed_distance = dot(
        normal,
        std::array::from_fn(|index| sphere.center[index] - plane.origin[index]),
    );
    let scale = sphere.radius.max(1.0);
    if sphere.radius <= 0.0 || (signed_distance.abs() - sphere.radius).abs() > 1e-9 * scale {
        return None;
    }
    Some(std::array::from_fn(|index| {
        sphere.center[index] - signed_distance * normal[index]
    }))
}

fn solve_carriers(carriers: &[CarrierEquation]) -> Option<[f64; 3]> {
    let mut candidates = Vec::new();
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            match (carriers[first], carriers[second]) {
                (CarrierEquation::Plane(plane), CarrierEquation::Sphere(sphere))
                | (CarrierEquation::Sphere(sphere), CarrierEquation::Plane(plane)) => {
                    if let Some(point) = tangent_plane_sphere_point(plane, sphere) {
                        candidates.push(point);
                    }
                }
                (CarrierEquation::Sphere(first), CarrierEquation::Sphere(second)) => {
                    if let Some(point) = tangent_sphere_point(first, second) {
                        candidates.push(point);
                    }
                }
                _ => {}
            }
        }
    }
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            for third in second + 1..carriers.len() {
                let triple = [carriers[first], carriers[second], carriers[third]];
                let mut planes = Vec::new();
                let mut cylinders = Vec::new();
                let mut cones = Vec::new();
                let mut spheres = Vec::new();
                let mut tori = Vec::new();
                for carrier in triple {
                    match carrier {
                        CarrierEquation::Plane(plane) => planes.push(plane),
                        CarrierEquation::Cylinder(cylinder) => cylinders.push(cylinder),
                        CarrierEquation::Cone(cone) => cones.push(cone),
                        CarrierEquation::Sphere(sphere) => spheres.push(sphere),
                        CarrierEquation::Torus(torus) => tori.push(torus),
                    }
                }
                if planes.len() == 3 {
                    if let Some(point) = solve_planes(&planes) {
                        candidates.push(point);
                    }
                } else if planes.len() == 1
                    && tori.is_empty()
                    && cylinders.len() + cones.len() + spheres.len() == 2
                {
                    let reduced = if let [first, second] = cones.as_slice() {
                        intersect_plane_with_carrier_components(
                            planes[0],
                            CarrierEquation::Cone(*first),
                            CarrierEquation::Cone(*second),
                        )
                    } else {
                        Vec::new()
                    };
                    if reduced.is_empty() {
                        let quadrics = cylinders
                            .iter()
                            .copied()
                            .map(CarrierEquation::Cylinder)
                            .chain(cones.iter().copied().map(CarrierEquation::Cone))
                            .chain(spheres.iter().copied().map(CarrierEquation::Sphere))
                            .collect::<Vec<_>>();
                        candidates.extend(intersect_plane_with_two_quadrics(
                            planes[0],
                            quadrics[0],
                            quadrics[1],
                        ));
                    } else {
                        candidates.extend(reduced);
                    }
                } else if planes.len() == 2
                    && tori.is_empty()
                    && cylinders.len() + cones.len() + spheres.len() == 1
                {
                    let quadric = cylinders
                        .first()
                        .copied()
                        .map(CarrierEquation::Cylinder)
                        .or_else(|| cones.first().copied().map(CarrierEquation::Cone))
                        .or_else(|| spheres.first().copied().map(CarrierEquation::Sphere))
                        .expect("one quadric carrier");
                    candidates.extend(intersect_two_planes_with_quadric(
                        planes[0], planes[1], quadric,
                    ));
                } else if let ([first, second], [torus]) = (planes.as_slice(), tori.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && spheres.is_empty() {
                        candidates.extend(intersect_two_planes_with_torus(*first, *second, *torus));
                    }
                } else if let ([plane], [cylinder], [torus]) =
                    (planes.as_slice(), cylinders.as_slice(), tori.as_slice())
                {
                    if cones.is_empty() && spheres.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Cylinder(*cylinder),
                            CarrierEquation::Torus(*torus),
                        ));
                    }
                } else if let ([plane], [cone], [sphere]) =
                    (planes.as_slice(), cones.as_slice(), spheres.as_slice())
                {
                    if cylinders.is_empty() && tori.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Cone(*cone),
                            CarrierEquation::Sphere(*sphere),
                        ));
                    }
                } else if let ([plane], [cone], [torus]) =
                    (planes.as_slice(), cones.as_slice(), tori.as_slice())
                {
                    if cylinders.is_empty() && spheres.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Cone(*cone),
                            CarrierEquation::Torus(*torus),
                        ));
                    }
                } else if let ([plane], [sphere], [torus]) =
                    (planes.as_slice(), spheres.as_slice(), tori.as_slice())
                {
                    if cylinders.is_empty() && cones.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Sphere(*sphere),
                            CarrierEquation::Torus(*torus),
                        ));
                    }
                } else if let ([plane], [first, second]) = (planes.as_slice(), tori.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && spheres.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Torus(*first),
                            CarrierEquation::Torus(*second),
                        ));
                    }
                }
            }
        }
    }
    candidates.retain(|point| {
        carriers
            .iter()
            .all(|carrier| point_on_carrier(*point, *carrier))
    });
    let mut unique = Vec::<[f64; 3]>::new();
    for candidate in candidates {
        if !unique.iter().any(|known| {
            known
                .iter()
                .zip(candidate)
                .all(|(left, right)| (left - right).abs() <= 1e-7)
        }) {
            unique.push(candidate);
        }
    }
    let [point] = unique.as_slice() else {
        return None;
    };
    Some(*point)
}

fn is_axis_aligned(vector: [f64; 3]) -> bool {
    vector.iter().filter(|value| value.abs() > 1e-9).count() == 1
}

fn canonical_plane(plane: PlaneEquation) -> Option<PlaneEquation> {
    let mut normal = normalized(plane.normal)?;
    let mut distance = dot(normal, plane.origin);
    if !distance.is_finite() {
        return None;
    }
    let sign = normal
        .iter()
        .find(|coordinate| coordinate.abs() > 1e-12)?
        .signum();
    if sign < 0.0 {
        normal = normal.map(|coordinate| -coordinate);
        distance = -distance;
    }
    Some(PlaneEquation {
        origin: normal.map(|coordinate| coordinate * distance),
        normal,
    })
}

fn agreed_plane(candidates: &[PlaneEquation]) -> Option<PlaneEquation> {
    let planes = candidates
        .iter()
        .copied()
        .map(canonical_plane)
        .collect::<Option<Vec<_>>>()?;
    let first = *planes.first()?;
    let first_distance = dot(first.normal, first.origin);
    planes
        .iter()
        .all(|plane| {
            let distance = dot(plane.normal, plane.origin);
            let scale = first_distance.abs().max(distance.abs()).max(1.0);
            first
                .normal
                .iter()
                .zip(plane.normal)
                .all(|(left, right)| (left - right).abs() <= 1e-9)
                && (first_distance - distance).abs() <= 1e-9 * scale
        })
        .then_some(first)
}

#[derive(Clone, Copy)]
struct PlaneCandidate {
    equation: PlaneEquation,
    chart: Option<PlaneChart>,
    offset: usize,
}

#[derive(Clone, Copy)]
struct PlaneChart {
    origin: [f64; 3],
    normal: [f64; 3],
    u_axis: [f64; 3],
}

fn agreed_plane_surface(candidates: &[PlaneCandidate]) -> Option<(PlaneEquation, [f64; 3], usize)> {
    agreed_plane(
        &candidates
            .iter()
            .map(|candidate| candidate.equation)
            .collect::<Vec<_>>(),
    )?;
    let charts = candidates
        .iter()
        .filter_map(|candidate| {
            let chart = candidate.chart?;
            let normal = normalized(chart.normal)?;
            let u_axis = normalized(chart.u_axis)?;
            (dot(normal, u_axis).abs() <= 1e-9).then_some((
                chart.origin,
                normal,
                u_axis,
                candidate.offset,
            ))
        })
        .collect::<Vec<_>>();
    let representative = charts.iter().min_by_key(|(_, _, _, offset)| *offset)?;
    charts
        .iter()
        .all(|(origin, normal, u_axis, _)| {
            representative.0.iter().zip(origin).all(|(left, right)| {
                (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
            }) && representative
                .1
                .iter()
                .zip(normal)
                .all(|(left, right)| (left - right).abs() <= 1e-9)
                && representative
                    .2
                    .iter()
                    .zip(u_axis)
                    .all(|(left, right)| (left - right).abs() <= 1e-9)
        })
        .then_some((
            PlaneEquation {
                origin: representative.0,
                normal: representative.1,
            },
            representative.2,
            representative.3,
        ))
}

#[cfg(test)]
mod plane_reconciliation_tests;

fn plane_candidates(scan: &ContainerScan) -> BTreeMap<u32, Vec<PlaneCandidate>> {
    let held_planes = scan
        .planes
        .envelopes
        .iter()
        .filter_map(|envelope| Some((envelope.surface_id, held_coordinate_plane(envelope)?)))
        .fold(
            BTreeMap::<u32, Vec<PlaneEquation>>::new(),
            |mut planes, (surface_id, plane)| {
                planes.entry(surface_id).or_default().push(plane);
                planes
            },
        )
        .into_iter()
        .filter_map(|(surface_id, planes)| agreed_plane(&planes).map(|plane| (surface_id, plane)))
        .collect::<BTreeMap<_, _>>();
    let frame_bound_outlines = crate::surface::frame_bound_outline_planes(
        &scan.planes.envelopes,
        &scan.planes.local_systems,
    )
    .into_iter()
    .fold(
        BTreeMap::<u32, Vec<crate::surface::OutlinePlane>>::new(),
        |mut outlines, outline| {
            outlines
                .entry(outline.surface_id)
                .or_default()
                .push(outline);
            outlines
        },
    );
    let mut candidates = BTreeMap::<u32, Vec<PlaneCandidate>>::new();
    for frame in &scan.planes.local_systems {
        let (Some(origin), Some(normal)) = (frame.origin, frame.normal) else {
            continue;
        };
        let Some(u_axis) = frame.u_axis else {
            continue;
        };
        let frame_candidate = PlaneCandidate {
            equation: PlaneEquation { origin, normal },
            chart: Some(PlaneChart {
                origin,
                normal,
                u_axis,
            }),
            offset: frame.offset,
        };
        let candidate = frame_bound_outlines
            .get(&frame.surface_id)
            .and_then(|outlines| {
                let [outline] = outlines.as_slice() else {
                    return None;
                };
                frame_bound_outline_plane_candidate(frame, outline)
            })
            .or_else(|| {
                held_planes
                    .get(&frame.surface_id)
                    .filter(|held| agreed_plane(&[frame_candidate.equation, **held]).is_none())
                    .and_then(|held| envelope_reconciled_plane_candidate(frame, *held))
            })
            .unwrap_or(frame_candidate);
        candidates
            .entry(frame.surface_id)
            .or_default()
            .push(candidate);
    }
    let local_chart_ids = scan
        .planes
        .local_systems
        .iter()
        .filter(|frame| frame.origin.is_some() && frame.normal.is_some() && frame.u_axis.is_some())
        .map(|frame| frame.surface_id)
        .collect::<BTreeSet<_>>();
    for outline in &scan.planes.outlines {
        candidates
            .entry(outline.surface_id)
            .or_default()
            .push(PlaneCandidate {
                equation: PlaneEquation {
                    origin: outline.origin,
                    normal: outline.normal,
                },
                chart: (!local_chart_ids.contains(&outline.surface_id)).then_some(PlaneChart {
                    origin: outline.origin,
                    normal: outline.normal,
                    u_axis: outline.u_axis,
                }),
                offset: outline.offset,
            });
    }
    for envelope in &scan.planes.envelopes {
        let Some(equation) = held_coordinate_plane(envelope) else {
            continue;
        };
        candidates
            .entry(envelope.surface_id)
            .or_default()
            .push(PlaneCandidate {
                equation,
                chart: None,
                offset: envelope.offset,
            });
    }
    for plane in &scan.planes.positional_frames {
        if candidates.contains_key(&plane.surface_id) {
            continue;
        }
        candidates.insert(
            plane.surface_id,
            vec![PlaneCandidate {
                equation: PlaneEquation {
                    origin: plane.origin,
                    normal: plane.normal,
                },
                chart: Some(PlaneChart {
                    origin: plane.origin,
                    normal: plane.normal,
                    u_axis: plane.u_axis,
                }),
                offset: plane.offset,
            }],
        );
    }
    candidates
        .into_iter()
        .filter(|(id, _)| {
            scan.surfaces
                .rows
                .iter()
                .filter(|row| row.id == *id)
                .take(2)
                .count()
                < 2
        })
        .collect()
}

fn frame_bound_outline_plane_candidate(
    frame: &crate::surface::PlaneLocalSystem,
    outline: &crate::surface::OutlinePlane,
) -> Option<PlaneCandidate> {
    (frame.surface_id == outline.surface_id).then_some(())?;
    let frame_normal = normalized(frame.normal?)?;
    let frame_u_axis = normalized(frame.u_axis?)?;
    let outline_normal = normalized(outline.normal)?;
    let outline_u_axis = normalized(outline.u_axis)?;
    (dot(frame_normal, outline_normal) >= 1.0 - 1e-9).then_some(())?;
    (dot(frame_u_axis, outline_u_axis) >= 1.0 - 1e-9).then_some(())?;
    let frame_origin = frame.origin?;
    let displacement = dot(outline_normal, outline.origin) - dot(outline_normal, frame_origin);
    let chart_origin =
        std::array::from_fn(|axis| displacement.mul_add(outline_normal[axis], frame_origin[axis]));
    Some(PlaneCandidate {
        equation: PlaneEquation {
            origin: outline.origin,
            normal: outline.normal,
        },
        chart: Some(PlaneChart {
            origin: chart_origin,
            normal: frame.normal?,
            u_axis: frame.u_axis?,
        }),
        offset: frame.offset,
    })
}

fn envelope_reconciled_plane_candidate(
    frame: &crate::surface::PlaneLocalSystem,
    equation: PlaneEquation,
) -> Option<PlaneCandidate> {
    let origin = frame.origin?;
    let normal = normalized(equation.normal)?;
    let origin_scale = origin
        .iter()
        .chain(equation.origin.iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    ((dot(normal, origin) - dot(normal, equation.origin)).abs() <= 1e-9 * origin_scale)
        .then_some(())?;
    let slots: [f64; 12] = frame
        .slots
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    let supports = [
        <[f64; 3]>::try_from(&slots[0..3]).ok()?,
        <[f64; 3]>::try_from(&slots[3..6]).ok()?,
        <[f64; 3]>::try_from(&slots[6..9]).ok()?,
    ];
    let support_scale = supports
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let nonzero = supports
        .into_iter()
        .filter_map(|support| {
            let magnitude = dot(support, support).sqrt();
            (magnitude > 1e-9 * support_scale).then_some((support, magnitude))
        })
        .collect::<Vec<_>>();
    let [first, second] = nonzero.as_slice() else {
        return None;
    };
    let role = |(support, magnitude): &([f64; 3], f64)| {
        let alignment = dot(*support, normal).abs() / *magnitude;
        if alignment <= 1e-9 {
            Some((false, support.map(|value| value / *magnitude)))
        } else if (alignment - 1.0).abs() <= 1e-9 {
            Some((true, support.map(|value| value / *magnitude)))
        } else {
            None
        }
    };
    let (first_parallel, first_direction) = role(first)?;
    let (second_parallel, second_direction) = role(second)?;
    (first_parallel != second_parallel).then_some(())?;
    let u_axis = if first_parallel {
        second_direction
    } else {
        first_direction
    };
    Some(PlaneCandidate {
        equation,
        chart: Some(PlaneChart {
            origin,
            normal,
            u_axis,
        }),
        offset: frame.offset,
    })
}

fn held_coordinate_plane(envelope: &crate::surface::PlaneEnvelopeRecord) -> Option<PlaneEquation> {
    let corners = plane_envelope_corners(&envelope.envelope)?;
    let held = envelope
        .corner_coordinate_equal
        .iter()
        .enumerate()
        .filter_map(|(axis, equal)| (*equal == Some(true)).then_some(axis))
        .collect::<Vec<_>>();
    let [axis] = held.as_slice() else {
        return None;
    };
    envelope
        .corner_coordinate_equal
        .iter()
        .enumerate()
        .all(|(candidate, equal)| candidate == *axis || *equal == Some(false))
        .then_some(())?;
    let mut normal = [0.0; 3];
    normal[*axis] = 1.0;
    Some(PlaneEquation {
        origin: corners[0],
        normal,
    })
}

fn placed_planes(scan: &ContainerScan) -> BTreeMap<u32, PlaneEquation> {
    plane_candidates(scan)
        .into_iter()
        .filter_map(|(id, candidates)| {
            agreed_plane(
                &candidates
                    .iter()
                    .map(|candidate| candidate.equation)
                    .collect::<Vec<_>>(),
            )
            .map(|plane| (id, plane))
        })
        .collect()
}

fn placed_plane_surfaces(scan: &ContainerScan) -> BTreeMap<u32, (PlaneEquation, [f64; 3], usize)> {
    plane_candidates(scan)
        .into_iter()
        .filter_map(|(id, candidates)| {
            agreed_plane_surface(&candidates).map(|surface| (id, surface))
        })
        .collect()
}

fn placed_carriers(scan: &ContainerScan, ir: &CadIr) -> BTreeMap<u32, CarrierEquation> {
    let mut carriers = placed_planes(scan)
        .into_iter()
        .map(|(id, plane)| (id, CarrierEquation::Plane(plane)))
        .collect::<BTreeMap<_, _>>();
    for row in crate::surface::uniquely_identified_rows(&scan.surfaces.rows) {
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        let Some(surface) = ir.model.surfaces.iter().find(|surface| surface.id == id) else {
            continue;
        };
        if let SurfaceGeometry::Plane { origin, normal, .. } = &surface.geometry {
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
        } else if let SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } = &surface.geometry
        {
            carriers.insert(
                row.id,
                CarrierEquation::Cylinder(CylinderEquation {
                    origin: [origin.x, origin.y, origin.z],
                    axis: [axis.x, axis.y, axis.z],
                    ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                    radius: *radius,
                }),
            );
        } else if let SurfaceGeometry::Sphere {
            center,
            axis: _,
            ref_direction,
            radius,
        } = &surface.geometry
        {
            carriers.insert(
                row.id,
                CarrierEquation::Sphere(SphereEquation {
                    center: [center.x, center.y, center.z],
                    ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                    radius: *radius,
                }),
            );
        } else if let SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } = &surface.geometry
        {
            if ratio.is_finite() && *ratio > 0.0 {
                carriers.insert(
                    row.id,
                    CarrierEquation::Cone(ConeEquation {
                        origin: [origin.x, origin.y, origin.z],
                        axis: [axis.x, axis.y, axis.z],
                        ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                        radius: *radius,
                        ratio: *ratio,
                        half_angle: *half_angle,
                    }),
                );
            }
        } else if let SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } = &surface.geometry
        {
            carriers.insert(
                row.id,
                CarrierEquation::Torus(TorusEquation {
                    center: [center.x, center.y, center.z],
                    axis: [axis.x, axis.y, axis.z],
                    ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                    major_radius: *major_radius,
                    minor_radius: *minor_radius,
                }),
            );
        }
    }
    carriers
}

fn geometry_section_record(scan: &ContainerScan, offset: usize) -> Option<UnknownId> {
    scan.framing
        .sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY)
        .find(|section| {
            offset >= section.offset && offset < section.offset.saturating_add(section.length)
        })
        .map(|section| UnknownId(format!("creo:{}:section#{}", section.name, section.offset)))
}

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
    let polygon = lp
        .half_edges
        .iter()
        .map(|half_edge| {
            let vertex = incidence.get(half_edge)?.start_vertex_id;
            let point = solved_vertices.get(&vertex)?;
            Some(match dropped_axis {
                0 => [point[1], point[2]],
                1 => [point[0], point[2]],
                _ => [point[0], point[1]],
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let area_twice = (0..polygon.len())
        .map(|index| {
            let first = polygon[index];
            let second = polygon[(index + 1) % polygon.len()];
            first[0].mul_add(second[1], -(first[1] * second[0]))
        })
        .sum::<f64>();
    let scale = polygon
        .iter()
        .flat_map(|point| point.iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (polygon.len() >= 3 && area_twice.abs() > 1e-12 * scale * scale).then_some(polygon)
}

fn polygon_strictly_contains(polygon: &[[f64; 2]], point: [f64; 2]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let edge = [second[0] - first[0], second[1] - first[1]];
        let relative = [point[0] - first[0], point[1] - first[1]];
        let cross = edge[0].mul_add(relative[1], -(edge[1] * relative[0]));
        let scale = edge[0].abs().max(edge[1].abs()).max(1.0);
        if cross.abs() <= 1e-9 * scale
            && point[0] >= first[0].min(second[0]) - 1e-9 * scale
            && point[0] <= first[0].max(second[0]) + 1e-9 * scale
            && point[1] >= first[1].min(second[1]) - 1e-9 * scale
            && point[1] <= first[1].max(second[1]) + 1e-9 * scale
        {
            return false;
        }
        if (first[1] > point[1]) != (second[1] > point[1]) {
            let intersection = edge[0].mul_add((point[1] - first[1]) / edge[1], first[0]);
            if point[0] < intersection {
                inside = !inside;
            }
        }
    }
    inside
}

fn ordered_planar_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    plane: PlaneEquation,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    if loops.len() == 1 {
        return Some(loops);
    }
    let polygons = loops
        .iter()
        .map(|lp| projected_loop_polygon(lp, plane, incidence, solved_vertices))
        .collect::<Option<Vec<_>>>()?;
    let outer = polygons
        .iter()
        .enumerate()
        .filter(|(candidate, polygon)| {
            polygons.iter().enumerate().all(|(index, inner)| {
                index == *candidate
                    || inner
                        .iter()
                        .all(|point| polygon_strictly_contains(polygon, *point))
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [outer] = outer.as_slice() else {
        return None;
    };
    let mut ordered = Vec::with_capacity(loops.len());
    ordered.push(loops[*outer]);
    ordered.extend(
        loops
            .into_iter()
            .enumerate()
            .filter_map(|(index, lp)| (index != *outer).then_some(lp)),
    );
    Some(ordered)
}

fn ordered_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    plane: Option<PlaneEquation>,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    if let Some(plane) = plane {
        ordered_planar_face_loops(loops, plane, incidence, solved_vertices)
    } else {
        let [single] = loops.as_slice() else {
            return None;
        };
        Some(vec![*single])
    }
}

fn rowless_round_face_orientations(
    round_feature_ids: &BTreeSet<u32>,
    tables: &[crate::feature::FeatureEntityTable],
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

fn native_face_orientations(scan: &ContainerScan, ir: &CadIr) -> BTreeMap<u32, bool> {
    let mut orientations = scan
        .surfaces
        .rows
        .iter()
        .map(|row| row.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, id)
                .map(|row| (id, row.reversed))
        })
        .collect::<BTreeMap<_, _>>();
    let round_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(913))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let available_surfaces = ir
        .model
        .surfaces
        .iter()
        .filter_map(|surface| {
            surface
                .id
                .0
                .strip_prefix("creo:visibgeom:surface#")?
                .parse()
                .ok()
        })
        .collect::<BTreeSet<_>>();
    orientations.extend(rowless_round_face_orientations(
        &round_feature_ids,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
        &available_surfaces,
    ));
    orientations
}

fn model_points_agree(first: [f64; 3], second: [f64; 3]) -> bool {
    let scale = first
        .into_iter()
        .chain(second)
        .map(f64::abs)
        .fold(1.0, f64::max);
    first
        .into_iter()
        .zip(second)
        .all(|(first, second)| (first - second).abs() <= 1e-9 * scale)
}

fn line_line_intersection(first: &CurveGeometry, second: &CurveGeometry) -> Option<[f64; 3]> {
    let (
        CurveGeometry::Line {
            origin: first_origin,
            direction: first_direction,
        },
        CurveGeometry::Line {
            origin: second_origin,
            direction: second_direction,
        },
    ) = (first, second)
    else {
        return None;
    };
    let first_origin = [first_origin.x, first_origin.y, first_origin.z];
    let second_origin = [second_origin.x, second_origin.y, second_origin.z];
    let first_direction = [first_direction.x, first_direction.y, first_direction.z];
    let second_direction = [second_direction.x, second_direction.y, second_direction.z];
    let relative = std::array::from_fn(|axis| first_origin[axis] - second_origin[axis]);
    let first_squared = dot(first_direction, first_direction);
    let second_squared = dot(second_direction, second_direction);
    let product = dot(first_direction, second_direction);
    let first_relative = dot(first_direction, relative);
    let second_relative = dot(second_direction, relative);
    let denominator = first_squared.mul_add(second_squared, -(product * product));
    if !denominator.is_finite()
        || denominator <= 1e-12 * first_squared * second_squared
        || first_squared <= 0.0
        || second_squared <= 0.0
    {
        return None;
    }
    let first_parameter =
        product.mul_add(second_relative, -(second_squared * first_relative)) / denominator;
    let second_parameter =
        first_squared.mul_add(second_relative, -(product * first_relative)) / denominator;
    let first_point = std::array::from_fn(|axis| {
        first_direction[axis].mul_add(first_parameter, first_origin[axis])
    });
    let second_point = std::array::from_fn(|axis| {
        second_direction[axis].mul_add(second_parameter, second_origin[axis])
    });
    (first_point
        .iter()
        .chain(second_point.iter())
        .all(|value| value.is_finite())
        && model_points_agree(first_point, second_point))
    .then(|| std::array::from_fn(|axis| f64::midpoint(first_point[axis], second_point[axis])))
}

fn line_conic_intersections(line: &CurveGeometry, conic: &CurveGeometry) -> Vec<[f64; 3]> {
    let CurveGeometry::Line { origin, direction } = line else {
        return Vec::new();
    };
    let Some(PlanarConicEquation {
        origin: conic_origin,
        normal,
        x_axis,
        y_axis,
        quadratic,
        linear,
        constant,
        scale: conic_scale,
    }) = planar_conic_equation(conic)
    else {
        return Vec::new();
    };
    let origin = [origin.x, origin.y, origin.z];
    let Some(direction) = normalized([direction.x, direction.y, direction.z]) else {
        return Vec::new();
    };
    let relative = std::array::from_fn(|coordinate| origin[coordinate] - conic_origin[coordinate]);
    let direction_plane = dot(direction, normal);
    let origin_plane = dot(relative, normal);
    let model_scale = origin
        .into_iter()
        .chain(conic_origin)
        .map(f64::abs)
        .fold(conic_scale.max(1.0), f64::max);
    if direction_plane.abs() > 1e-12 {
        let parameter = -origin_plane / direction_plane;
        let point = std::array::from_fn(|coordinate| {
            direction[coordinate].mul_add(parameter, origin[coordinate])
        });
        return (point.iter().all(|value| value.is_finite())
            && curve_contains_points(conic, [point, point]))
        .then_some(point)
        .into_iter()
        .collect();
    }
    if origin_plane.abs() > 1e-9 * model_scale {
        return Vec::new();
    }
    let local_origin = [dot(relative, x_axis), dot(relative, y_axis)];
    let local_direction = [dot(direction, x_axis), dot(direction, y_axis)];
    let line_quadratic = quadratic[0].mul_add(
        local_direction[0].powi(2),
        quadratic[1] * local_direction[1].powi(2),
    );
    let line_linear =
        2.0 * quadratic[0].mul_add(
            local_origin[0] * local_direction[0],
            quadratic[1] * local_origin[1] * local_direction[1],
        ) + linear[0].mul_add(local_direction[0], linear[1] * local_direction[1]);
    let line_constant = quadratic[0].mul_add(
        local_origin[0].powi(2),
        quadratic[1] * local_origin[1].powi(2),
    ) + linear[0].mul_add(local_origin[0], linear[1] * local_origin[1])
        + constant;
    let coefficient_scale = line_linear
        .abs()
        .max((line_quadratic * line_constant).abs().sqrt())
        .max(1.0);
    let coefficient_tolerance = 1e-14 * coefficient_scale;
    if !line_quadratic.is_finite() || !line_linear.is_finite() || !line_constant.is_finite() {
        return Vec::new();
    }
    if line_quadratic.abs() <= coefficient_tolerance {
        if line_linear.abs() <= coefficient_tolerance {
            return Vec::new();
        }
        let parameter = -line_constant / line_linear;
        let point = std::array::from_fn(|coordinate| {
            direction[coordinate].mul_add(parameter, origin[coordinate])
        });
        return curve_contains_points(conic, [point, point])
            .then_some(point)
            .into_iter()
            .collect();
    }
    let discriminant = line_linear.mul_add(line_linear, -4.0 * line_quadratic * line_constant);
    let tolerance = 1e-12 * coefficient_scale * coefficient_scale;
    if !discriminant.is_finite() || discriminant < -tolerance {
        return Vec::new();
    }
    let root = discriminant.max(0.0).sqrt();
    let first_parameter = -line_linear / (2.0 * line_quadratic);
    let first = std::array::from_fn(|coordinate| {
        direction[coordinate].mul_add(first_parameter, origin[coordinate])
    });
    if root <= 1e-9 * coefficient_scale {
        return curve_contains_points(conic, [first, first])
            .then_some(first)
            .into_iter()
            .collect();
    }
    let root_product = -0.5 * (line_linear + root.copysign(line_linear));
    let first_parameter = root_product / line_quadratic;
    let second_parameter = line_constant / root_product;
    let first = std::array::from_fn(|coordinate| {
        direction[coordinate].mul_add(first_parameter, origin[coordinate])
    });
    let second = std::array::from_fn(|coordinate| {
        direction[coordinate].mul_add(second_parameter, origin[coordinate])
    });
    [first, second]
        .into_iter()
        .filter(|point| curve_contains_points(conic, [*point, *point]))
        .collect()
}

fn restrict_planar_conic_to_chart(
    conic: PlanarConicEquation,
    origin: [f64; 3],
    u_axis: [f64; 3],
    v_axis: [f64; 3],
) -> PlaneConicEquation {
    let offset: [f64; 3] =
        std::array::from_fn(|coordinate| origin[coordinate] - conic.origin[coordinate]);
    let x = [
        dot(offset, conic.x_axis),
        dot(u_axis, conic.x_axis),
        dot(v_axis, conic.x_axis),
    ];
    let y = [
        dot(offset, conic.y_axis),
        dot(u_axis, conic.y_axis),
        dot(v_axis, conic.y_axis),
    ];
    PlaneConicEquation {
        uu: conic.quadratic[0].mul_add(x[1].powi(2), conic.quadratic[1] * y[1].powi(2)),
        uv: 2.0 * conic.quadratic[0].mul_add(x[1] * x[2], conic.quadratic[1] * y[1] * y[2]),
        vv: conic.quadratic[0].mul_add(x[2].powi(2), conic.quadratic[1] * y[2].powi(2)),
        u: 2.0 * conic.quadratic[0].mul_add(x[0] * x[1], conic.quadratic[1] * y[0] * y[1])
            + conic.linear[0].mul_add(x[1], conic.linear[1] * y[1]),
        v: 2.0 * conic.quadratic[0].mul_add(x[0] * x[2], conic.quadratic[1] * y[0] * y[2])
            + conic.linear[0].mul_add(x[2], conic.linear[1] * y[2]),
        constant: conic.quadratic[0].mul_add(x[0].powi(2), conic.quadratic[1] * y[0].powi(2))
            + conic.linear[0].mul_add(x[0], conic.linear[1] * y[0])
            + conic.constant,
    }
}

fn conic_conic_intersections(first: &CurveGeometry, second: &CurveGeometry) -> Vec<[f64; 3]> {
    let Some(first_equation) = planar_conic_equation(first) else {
        return Vec::new();
    };
    let Some(second_equation) = planar_conic_equation(second) else {
        return Vec::new();
    };
    let normal_cross = cross(first_equation.normal, second_equation.normal);
    if dot(normal_cross, normal_cross) > 1e-18 {
        let Some((origin, direction)) = plane_intersection_line(
            PlaneEquation {
                origin: first_equation.origin,
                normal: first_equation.normal,
            },
            PlaneEquation {
                origin: second_equation.origin,
                normal: second_equation.normal,
            },
        ) else {
            return Vec::new();
        };
        let line = CurveGeometry::Line {
            origin: Point3::new(origin[0], origin[1], origin[2]),
            direction: Vector3::new(direction[0], direction[1], direction[2]),
        };
        let mut points = line_conic_intersections(&line, first);
        points.retain(|point| curve_contains_points(second, [*point, *point]));
        return points;
    }
    let delta: [f64; 3] = std::array::from_fn(|coordinate| {
        second_equation.origin[coordinate] - first_equation.origin[coordinate]
    });
    let scale = first_equation
        .origin
        .into_iter()
        .chain(second_equation.origin)
        .map(f64::abs)
        .fold(
            first_equation.scale.max(second_equation.scale).max(1.0),
            f64::max,
        );
    if dot(delta, first_equation.normal).abs() > 1e-9 * scale {
        return Vec::new();
    }
    let first_chart = restrict_planar_conic_to_chart(
        first_equation,
        first_equation.origin,
        first_equation.x_axis,
        first_equation.y_axis,
    );
    let second_chart = restrict_planar_conic_to_chart(
        second_equation,
        first_equation.origin,
        first_equation.x_axis,
        first_equation.y_axis,
    );
    common_plane_conic_parameters(first_chart, second_chart)
        .into_iter()
        .map(|[u, v]| {
            std::array::from_fn(|coordinate| {
                first_equation.origin[coordinate]
                    + u * first_equation.x_axis[coordinate]
                    + v * first_equation.y_axis[coordinate]
            })
        })
        .filter(|point| {
            curve_contains_points(first, [*point, *point])
                && curve_contains_points(second, [*point, *point])
        })
        .collect()
}

fn incident_analytic_vertex_domain(curves: &[&CurveGeometry]) -> Vec<[f64; 3]> {
    let mut candidates = Vec::new();
    for first in 0..curves.len() {
        for second in first + 1..curves.len() {
            candidates.extend(
                line_line_intersection(curves[first], curves[second])
                    .into_iter()
                    .chain(line_conic_intersections(curves[first], curves[second]))
                    .chain(line_conic_intersections(curves[second], curves[first]))
                    .chain(conic_conic_intersections(curves[first], curves[second])),
            );
        }
    }
    candidates.retain(|point| {
        curves
            .iter()
            .all(|curve| curve_contains_points(curve, [*point, *point]))
    });
    candidates
        .into_iter()
        .fold(Vec::new(), |mut unique, point| {
            if !unique
                .iter()
                .any(|candidate| model_points_agree(*candidate, point))
            {
                unique.push(point);
            }
            unique
        })
}

fn mapped_pcurve_endpoints(
    ir: &CadIr,
    faces: [u32; 2],
    endpoint_sets: [[[f64; 2]; 2]; 2],
) -> Option<[[f64; 3]; 2]> {
    let mapped = faces
        .into_iter()
        .zip(endpoint_sets)
        .filter_map(|(face_id, endpoints)| {
            let surface = ir.model.surfaces.iter().find(|surface| {
                surface.id == SurfaceId(format!("creo:visibgeom:surface#{face_id}"))
            })?;
            let [first, second] = endpoints.map(|uv| {
                cadmpeg_ir::eval::surface_point(&surface.geometry, uv[0], uv[1])
                    .map(|point| [point.x, point.y, point.z])
            });
            Some([first?, second?])
        })
        .collect::<Vec<[[f64; 3]; 2]>>();
    let first = *mapped.first()?;
    mapped
        .iter()
        .all(|candidate| {
            model_points_agree(first[0], candidate[0]) && model_points_agree(first[1], candidate[1])
        })
        .then_some(first)
}

fn pcurve_edge_endpoints(scan: &ContainerScan, ir: &CadIr) -> BTreeMap<u32, [[f64; 3]; 2]> {
    let mut candidates = BTreeMap::<u32, Vec<[[f64; 3]; 2]>>::new();
    for (curve_id, faces, first, second) in scan
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
        if let Some(points) = mapped_pcurve_endpoints(ir, faces, [first, second]) {
            candidates.entry(curve_id).or_default().push(points);
        }
    }
    candidates
        .into_iter()
        .filter_map(|(curve_id, candidates)| {
            let first = *candidates.first()?;
            candidates
                .iter()
                .all(|candidate| {
                    model_points_agree(first[0], candidate[0])
                        && model_points_agree(first[1], candidate[1])
                })
                .then_some((curve_id, first))
        })
        .collect()
}

fn linear_pcurve_carrier(
    surface: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
) -> Option<CurveGeometry> {
    let scaled_vector = |vector: Vector3, scale: f64| {
        Vector3::new(vector.x * scale, vector.y * scale, vector.z * scale)
    };
    let offset_point = |point: Point3, vector: Vector3, scale: f64| {
        Point3::new(
            point.x + vector.x * scale,
            point.y + vector.y * scale,
            point.z + vector.z * scale,
        )
    };
    let [start, end] = endpoints;
    if start == end {
        return None;
    }
    match surface {
        SurfaceGeometry::Plane { .. } => {
            let [first, second] = endpoints.map(|uv| {
                cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1])
                    .map(|point| [point.x, point.y, point.z])
            });
            let [first, second] = [first?, second?];
            let direction = normalized(std::array::from_fn(|axis| second[axis] - first[axis]))?;
            Some(CurveGeometry::Line {
                origin: Point3::new(first[0], first[1], first[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } if start[0] == end[0] => {
            let transverse = cross(
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            let reference = [ref_direction.x, ref_direction.y, ref_direction.z];
            let radial: [f64; 3] = std::array::from_fn(|coordinate| {
                start[0].cos() * reference[coordinate] + start[0].sin() * transverse[coordinate]
            });
            let point = [
                origin.x + radius * radial[0] + start[1] * axis.x,
                origin.y + radius * radial[1] + start[1] * axis.y,
                origin.z + radius * radial[2] + start[1] * axis.z,
            ];
            let direction = normalized([axis.x, axis.y, axis.z])?;
            Some(CurveGeometry::Line {
                origin: Point3::new(point[0], point[1], point[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } if start[1] == end[1] && radius.is_finite() && *radius > 0.0 => {
            Some(CurveGeometry::Circle {
                center: offset_point(*origin, *axis, start[1]),
                axis: *axis,
                ref_direction: *ref_direction,
                radius: *radius,
            })
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            ratio,
            half_angle,
            ..
        } if start[0] == end[0] => {
            let [first, second] = endpoints.map(|uv| {
                cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1])
                    .map(|point| [point.x, point.y, point.z])
            });
            let [first, second] = [first?, second?];
            let direction = normalized(std::array::from_fn(|axis| second[axis] - first[axis]))?;
            (ratio.is_finite() && *ratio > 0.0 && half_angle.is_finite()).then_some(())?;
            Some(CurveGeometry::Line {
                origin: Point3::new(first[0], first[1], first[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } if start[1] == end[1] && ratio.is_finite() && *ratio > 0.0 => {
            let local_radius = radius + start[1] * half_angle.tan();
            let first_radius = local_radius.abs();
            let second_radius = (local_radius * ratio).abs();
            if !first_radius.is_finite() || !second_radius.is_finite() {
                return None;
            }
            let center = offset_point(*origin, *axis, start[1]);
            if (first_radius - second_radius).abs()
                <= 1e-12 * first_radius.max(second_radius).max(1.0)
            {
                (first_radius > 0.0).then_some(CurveGeometry::Circle {
                    center,
                    axis: *axis,
                    ref_direction: scaled_vector(*ref_direction, local_radius.signum()),
                    radius: first_radius,
                })
            } else {
                let transverse = cross(
                    [axis.x, axis.y, axis.z],
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                );
                let transverse = Vector3::new(transverse[0], transverse[1], transverse[2]);
                let (major_direction, major_radius, minor_radius) = if first_radius > second_radius
                {
                    (
                        scaled_vector(*ref_direction, local_radius.signum()),
                        first_radius,
                        second_radius,
                    )
                } else {
                    (
                        scaled_vector(transverse, (local_radius * ratio).signum()),
                        second_radius,
                        first_radius,
                    )
                };
                (minor_radius > 0.0).then_some(CurveGeometry::Ellipse {
                    center,
                    axis: *axis,
                    major_direction,
                    major_radius,
                    minor_radius,
                })
            }
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } if start[1] == end[1] && radius.is_finite() && *radius > 0.0 => {
            let ring = radius * start[1].cos();
            (ring.abs() > 0.0).then_some(CurveGeometry::Circle {
                center: offset_point(*center, *axis, radius * start[1].sin()),
                axis: *axis,
                ref_direction: scaled_vector(*ref_direction, ring.signum()),
                radius: ring.abs(),
            })
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } if start[0] == end[0] && radius.is_finite() && *radius > 0.0 => {
            let transverse = cross(
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            let radial = Vector3::new(
                start[0].cos() * ref_direction.x + start[0].sin() * transverse[0],
                start[0].cos() * ref_direction.y + start[0].sin() * transverse[1],
                start[0].cos() * ref_direction.z + start[0].sin() * transverse[2],
            );
            let normal = cross([radial.x, radial.y, radial.z], [axis.x, axis.y, axis.z]);
            Some(CurveGeometry::Circle {
                center: *center,
                axis: Vector3::new(normal[0], normal[1], normal[2]),
                ref_direction: radial,
                radius: *radius,
            })
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if start[1] == end[1]
            && major_radius.is_finite()
            && minor_radius.is_finite()
            && *minor_radius > 0.0 =>
        {
            let ring = major_radius + minor_radius * start[1].cos();
            (ring.abs() > 0.0).then_some(CurveGeometry::Circle {
                center: offset_point(*center, *axis, minor_radius * start[1].sin()),
                axis: *axis,
                ref_direction: scaled_vector(*ref_direction, ring.signum()),
                radius: ring.abs(),
            })
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if start[0] == end[0]
            && major_radius.is_finite()
            && minor_radius.is_finite()
            && *minor_radius > 0.0 =>
        {
            let transverse = cross(
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            let radial = Vector3::new(
                start[0].cos() * ref_direction.x + start[0].sin() * transverse[0],
                start[0].cos() * ref_direction.y + start[0].sin() * transverse[1],
                start[0].cos() * ref_direction.z + start[0].sin() * transverse[2],
            );
            let normal = cross([radial.x, radial.y, radial.z], [axis.x, axis.y, axis.z]);
            Some(CurveGeometry::Circle {
                center: offset_point(*center, radial, *major_radius),
                axis: Vector3::new(normal[0], normal[1], normal[2]),
                ref_direction: radial,
                radius: *minor_radius,
            })
        }
        _ => None,
    }
}

fn transfer_analytic_pcurve_carriers(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> BTreeSet<CurveId> {
    let reconciled_endpoints = pcurve_edge_endpoints(scan, ir);
    let mut candidates = BTreeMap::<u32, Vec<(CurveGeometry, usize)>>::new();
    let mut evaluable_path_counts = BTreeMap::<u32, usize>::new();
    for (curve_id, faces, endpoint_sets, offset) in scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                [pcurve.face_0_endpoints, pcurve.face_1_endpoints],
                pcurve.offset,
            )
        })
        .chain(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                [pcurve.face_0_endpoints, pcurve.face_1_endpoints],
                pcurve.offset,
            )
        }))
    {
        for (face_id, endpoints) in faces.into_iter().zip(endpoint_sets) {
            let Some(surface) = ir.model.surfaces.iter().find(|surface| {
                surface.id == SurfaceId(format!("creo:visibgeom:surface#{face_id}"))
            }) else {
                continue;
            };
            if endpoints.iter().all(|uv| {
                cadmpeg_ir::eval::surface_point(&surface.geometry, uv[0], uv[1]).is_some()
            }) {
                *evaluable_path_counts.entry(curve_id).or_default() += 1;
            }
            if let Some(carrier) = linear_pcurve_carrier(&surface.geometry, endpoints) {
                candidates
                    .entry(curve_id)
                    .or_default()
                    .push((carrier, offset));
            }
        }
    }
    let mut transferred = BTreeSet::new();
    for (curve_id, candidates) in candidates {
        if evaluable_path_counts.get(&curve_id).copied() != Some(candidates.len()) {
            continue;
        }
        let Some(points) = reconciled_endpoints.get(&curve_id).copied() else {
            continue;
        };
        let Some((geometry, offset)) = candidates.first() else {
            continue;
        };
        if !curve_contains_points(geometry, points)
            || !candidates.iter().all(|(candidate, _)| {
                curve_contains_points(candidate, points)
                    && [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().all(|parameter| {
                        let point = cadmpeg_ir::eval::curve_point(candidate, parameter);
                        point.is_some_and(|point| {
                            curve_contains_points(geometry, [[point.x, point.y, point.z]; 2])
                        })
                    })
            })
        {
            continue;
        }
        let offset = candidates
            .iter()
            .map(|(_, offset)| *offset)
            .min()
            .unwrap_or(*offset);
        let id = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            offset as u64,
            "analytic_pcurve_carrier",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: geometry.clone(),
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{curve_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred.insert(id);
    }
    transferred
}

type PcurveVertexConstraint = ([u32; 2], [[f64; 3]; 2]);

fn directed_pcurve_points(directions: [u8; 2], points: [[f64; 3]; 2]) -> Option<[[f64; 3]; 2]> {
    match directions {
        [0x01, 0xf6] => Some(points),
        [0xf6, 0x01] => Some([points[1], points[0]]),
        _ => None,
    }
}

fn solve_pcurve_vertex_domains(
    constraints: &[PcurveVertexConstraint],
    fixed_points: &BTreeMap<u32, Option<[f64; 3]>>,
    analytic_domains: &BTreeMap<u32, Vec<[f64; 3]>>,
    incident_curves: &BTreeMap<u32, Vec<&CurveGeometry>>,
) -> BTreeMap<u32, [f64; 3]> {
    let mut domains = BTreeMap::<u32, Vec<[f64; 3]>>::new();
    for (vertices, points) in constraints {
        if vertices[0] == vertices[1] {
            match domains.entry(vertices[0]) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(if model_points_agree(points[0], points[1]) {
                        vec![points[0]]
                    } else {
                        Vec::new()
                    });
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let domain = entry.get_mut();
                    if model_points_agree(points[0], points[1]) {
                        domain.retain(|candidate| model_points_agree(*candidate, points[0]));
                    } else {
                        domain.clear();
                    }
                }
            }
            continue;
        }
        for vertex in vertices {
            let domain = domains.entry(*vertex).or_insert_with(|| points.to_vec());
            domain.retain(|candidate| {
                points
                    .iter()
                    .any(|point| model_points_agree(*candidate, *point))
            });
        }
    }
    for (vertex, candidates) in analytic_domains {
        match domains.entry(*vertex) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(candidates.clone());
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().retain(|point| {
                    candidates
                        .iter()
                        .any(|candidate| model_points_agree(*point, *candidate))
                });
            }
        }
    }
    for (vertex, point) in fixed_points {
        match domains.entry(*vertex) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(point.iter().copied().collect());
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if let Some(point) = point {
                    entry
                        .get_mut()
                        .retain(|candidate| model_points_agree(*candidate, *point));
                } else {
                    entry.get_mut().clear();
                }
            }
        }
    }
    for (vertex, curves) in incident_curves {
        if let Some(domain) = domains.get_mut(vertex) {
            domain.retain(|candidate| {
                curves
                    .iter()
                    .all(|curve| curve_contains_points(curve, [*candidate, *candidate]))
            });
        }
    }
    let compatible = |first: [f64; 3], second: [f64; 3], points: [[f64; 3]; 2]| {
        (model_points_agree(first, points[0]) && model_points_agree(second, points[1]))
            || (model_points_agree(first, points[1]) && model_points_agree(second, points[0]))
    };
    loop {
        let mut changed = false;
        for (vertices, points) in constraints {
            if vertices[0] == vertices[1] {
                continue;
            }
            let first = domains.get(&vertices[0]).cloned().unwrap_or_default();
            let second = domains.get(&vertices[1]).cloned().unwrap_or_default();
            let retained_first = first
                .iter()
                .copied()
                .filter(|first| {
                    second
                        .iter()
                        .any(|second| compatible(*first, *second, *points))
                })
                .collect::<Vec<_>>();
            let retained_second = second
                .iter()
                .copied()
                .filter(|second| {
                    first
                        .iter()
                        .any(|first| compatible(*first, *second, *points))
                })
                .collect::<Vec<_>>();
            changed |= retained_first.len() != first.len() || retained_second.len() != second.len();
            domains.insert(vertices[0], retained_first);
            domains.insert(vertices[1], retained_second);
        }
        if !changed {
            break;
        }
    }
    domains
        .into_iter()
        .filter_map(|(vertex, mut domain)| {
            domain.dedup_by(|first, second| model_points_agree(*first, *second));
            let [point] = domain.as_slice() else {
                return None;
            };
            Some((vertex, *point))
        })
        .collect()
}

fn solved_topological_vertices(
    scan: &ContainerScan,
    ir: &CadIr,
    carriers: &BTreeMap<u32, CarrierEquation>,
) -> BTreeMap<u32, [f64; 3]> {
    let vertex_faces =
        crate::topology::vertex_incident_faces(&scan.topology.vertices, &scan.topology.half_edges);
    let carrier_points = scan
        .topology
        .vertices
        .iter()
        .filter_map(|vertex| {
            let incident_carriers = vertex_faces
                .get(&vertex.id)?
                .iter()
                .filter_map(|face_id| carriers.get(face_id))
                .copied()
                .collect::<Vec<_>>();
            solve_carriers(&incident_carriers).map(|point| (vertex.id, point))
        })
        .collect::<BTreeMap<_, _>>();
    let edge_endpoints = pcurve_edge_endpoints(scan, ir);
    let edge_vertices =
        crate::topology::edge_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    let mut fixed_points = carrier_points
        .into_iter()
        .map(|(vertex, point)| (vertex, Some(point)))
        .collect::<BTreeMap<_, _>>();
    let mut constraints = Vec::new();
    for row in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        let Some(points) = edge_endpoints.get(&row.id).copied() else {
            continue;
        };
        let Some(vertices) = edge_vertices.get(&row.id).copied() else {
            continue;
        };
        constraints.push((vertices, points));
        if let Some(ordered) = directed_pcurve_points(row.directions, points) {
            for (vertex, point) in vertices.into_iter().zip(ordered) {
                match fixed_points.entry(vertex) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(Some(point));
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        if entry
                            .get()
                            .is_none_or(|known| !model_points_agree(known, point))
                        {
                            entry.insert(None);
                        }
                    }
                }
            }
        }
    }
    let analytic_curves = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
        .into_iter()
        .filter_map(|row| {
            let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
            let geometry = &ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == id)?
                .geometry;
            matches!(
                geometry,
                CurveGeometry::Line { .. }
                    | CurveGeometry::Circle { .. }
                    | CurveGeometry::Ellipse { .. }
                    | CurveGeometry::Parabola { .. }
                    | CurveGeometry::Hyperbola { .. }
            )
            .then_some((row.id, geometry))
        })
        .collect::<BTreeMap<_, _>>();
    let incident_curves = scan
        .topology
        .vertices
        .iter()
        .filter_map(|vertex| {
            let curves = vertex
                .half_edges
                .iter()
                .filter_map(|half_edge| analytic_curves.get(&half_edge.curve_id).copied())
                .collect::<Vec<_>>();
            (!curves.is_empty()).then_some((vertex.id, curves))
        })
        .collect::<BTreeMap<_, _>>();
    let analytic_domains = incident_curves
        .iter()
        .filter_map(|(vertex, curves)| {
            let candidates = incident_analytic_vertex_domain(curves);
            (!candidates.is_empty()).then_some((*vertex, candidates))
        })
        .collect::<BTreeMap<_, _>>();
    solve_pcurve_vertex_domains(
        &constraints,
        &fixed_points,
        &analytic_domains,
        &incident_curves,
    )
}

#[cfg(test)]
mod topological_vertex_tests;

fn orient_line_edge_carrier(
    geometry: &mut CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    if !curve_contains_points(geometry, points) {
        return None;
    }
    let CurveGeometry::Line { origin, direction } = geometry else {
        return None;
    };
    let delta: [f64; 3] = std::array::from_fn(|index| points[1][index] - points[0][index]);
    let length = dot(delta, delta).sqrt();
    let oriented = normalized(delta)?;
    *origin = Point3::new(points[0][0], points[0][1], points[0][2]);
    *direction = Vector3::new(oriented[0], oriented[1], oriented[2]);
    Some([0.0, length])
}

fn exact_line_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    if !curve_contains_points(geometry, points) {
        return None;
    }
    let CurveGeometry::Line { origin, direction } = geometry else {
        return None;
    };
    let direction = [direction.x, direction.y, direction.z];
    let denominator = dot(direction, direction);
    if !denominator.is_finite() || denominator <= 0.0 {
        return None;
    }
    let origin = [origin.x, origin.y, origin.z];
    let parameters = points.map(|point| {
        dot(
            std::array::from_fn(|index| point[index] - origin[index]),
            direction,
        ) / denominator
    });
    parameters
        .into_iter()
        .all(f64::is_finite)
        .then_some(if parameters[0] <= parameters[1] {
            parameters
        } else {
            [parameters[1], parameters[0]]
        })
}

fn point_pair_alignments(mapped: [[f64; 3]; 2], target: [[f64; 3]; 2]) -> [bool; 2] {
    let mismatch = |left: [f64; 3], right: [f64; 3]| {
        dot(
            std::array::from_fn(|index| left[index] - right[index]),
            std::array::from_fn(|index| left[index] - right[index]),
        )
        .sqrt()
    };
    let scale = mapped
        .into_iter()
        .flatten()
        .chain(target.into_iter().flatten())
        .map(f64::abs)
        .fold(1.0, f64::max);
    let tolerance = 1e-9 * scale;
    [
        mismatch(mapped[0], target[0]).max(mismatch(mapped[1], target[1])) <= tolerance,
        mismatch(mapped[0], target[1]).max(mismatch(mapped[1], target[0])) <= tolerance,
    ]
}

fn nurbs_control_extent(nurbs: &NurbsCurve) -> Option<f64> {
    let bounds = nurbs.control_points.iter().try_fold(
        [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]],
        |mut bounds, point| {
            for (index, coordinate) in [point.x, point.y, point.z].into_iter().enumerate() {
                coordinate.is_finite().then_some(())?;
                bounds[0][index] = bounds[0][index].min(coordinate);
                bounds[1][index] = bounds[1][index].max(coordinate);
            }
            Some(bounds)
        },
    )?;
    Some(
        (0..3)
            .map(|index| bounds[1][index] - bounds[0][index])
            .fold(1.0, f64::max),
    )
}

fn nurbs_intrinsic_parameter_range(nurbs: &NurbsCurve) -> Option<[f64; 2]> {
    let degree = usize::try_from(nurbs.degree).ok()?;
    (degree > 0
        && nurbs.control_points.len() > degree
        && nurbs.knots.len() == nurbs.control_points.len().checked_add(degree + 1)?
        && nurbs_control_extent(nurbs).is_some()
        && nurbs.knots.iter().all(|knot| knot.is_finite())
        && nurbs.knots.windows(2).all(|pair| pair[0] <= pair[1])
        && nurbs
            .weights
            .as_ref()
            .is_none_or(|weights| weights.len() == nurbs.control_points.len()))
    .then_some(())?;
    let range = [
        *nurbs.knots.get(degree)?,
        *nurbs.knots.get(nurbs.control_points.len())?,
    ];
    (range[0] < range[1]).then_some(range)
}

fn nonperiodic_nurbs_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    let CurveGeometry::Nurbs(nurbs) = geometry else {
        return None;
    };
    if nurbs.periodic {
        return None;
    }
    let degree = usize::try_from(nurbs.degree).ok()?;
    let range = nurbs_intrinsic_parameter_range(nurbs)?;

    if degree == 1 {
        nurbs
            .weights
            .as_ref()
            .is_none_or(|weights| {
                weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0)
            })
            .then_some(())?;
        let scale = nurbs_control_extent(nurbs)?;
        let tolerance = 1e-9 * scale;
        let first = degree_one_nurbs_point_parameter(geometry, nurbs, points[0], range, tolerance)?;
        let second =
            degree_one_nurbs_point_parameter(geometry, nurbs, points[1], range, tolerance)?;
        let parameters = if first <= second {
            [first, second]
        } else {
            [second, first]
        };
        return (parameters[1] - parameters[0] > 1e-12 * (range[1] - range[0]).max(1.0))
            .then_some(parameters);
    }

    let mapped = range.map(|parameter| {
        cadmpeg_ir::eval::curve_point(geometry, parameter).map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    match point_pair_alignments([first, second], points) {
        [true, false] | [false, true] => Some(range),
        _ => None,
    }
}

fn full_periodic_nurbs_edge_parameter_range(
    geometry: &CurveGeometry,
    point: [f64; 3],
) -> Option<[f64; 2]> {
    let CurveGeometry::Nurbs(nurbs) = geometry else {
        return None;
    };
    nurbs.periodic.then_some(())?;
    nurbs
        .weights
        .as_ref()
        .is_none_or(|weights| {
            weights
                .iter()
                .all(|weight| weight.is_finite() && *weight > 0.0)
        })
        .then_some(())?;
    let range = nurbs_intrinsic_parameter_range(nurbs)?;
    let mapped = range.map(|parameter| {
        cadmpeg_ir::eval::curve_point(geometry, parameter).map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    let tolerance = 1e-9 * nurbs_control_extent(nurbs)?;
    [first, second]
        .into_iter()
        .all(|mapped| {
            let delta: [f64; 3] = std::array::from_fn(|index| mapped[index] - point[index]);
            dot(delta, delta).sqrt() <= tolerance
        })
        .then_some(range)
}

fn degree_one_nurbs_point_parameter(
    geometry: &CurveGeometry,
    nurbs: &NurbsCurve,
    point: [f64; 3],
    range: [f64; 2],
    tolerance: f64,
) -> Option<f64> {
    let parameter_tolerance = 1e-9 * (range[1] - range[0]).max(1.0);
    let mut candidates = Vec::<f64>::new();
    for span in 1..nurbs.control_points.len() {
        let lower = nurbs.knots[span];
        let upper = nurbs.knots[span + 1];
        if !lower.is_finite() || !upper.is_finite() || upper <= lower {
            continue;
        }
        let first = nurbs.control_points[span - 1];
        let second = nurbs.control_points[span];
        let delta = [second.x - first.x, second.y - first.y, second.z - first.z];
        let denominator = dot(delta, delta);
        if !denominator.is_finite() {
            continue;
        }
        let relative = [point[0] - first.x, point[1] - first.y, point[2] - first.z];
        if denominator <= tolerance * tolerance {
            if dot(relative, relative).sqrt() <= tolerance {
                return None;
            }
            continue;
        }
        let fraction = dot(relative, delta) / denominator;
        if !(-1e-9..=1.0 + 1e-9).contains(&fraction) {
            continue;
        }
        let fraction = fraction.clamp(0.0, 1.0);
        let projected = [
            first.x + fraction * delta[0],
            first.y + fraction * delta[1],
            first.z + fraction * delta[2],
        ];
        let mismatch: [f64; 3] = std::array::from_fn(|index| projected[index] - point[index]);
        if dot(mismatch, mismatch).sqrt() > tolerance {
            continue;
        }
        let first_weight = nurbs
            .weights
            .as_ref()
            .map_or(1.0, |weights| weights[span - 1]);
        let second_weight = nurbs.weights.as_ref().map_or(1.0, |weights| weights[span]);
        let rational_denominator = second_weight * (1.0 - fraction) + fraction * first_weight;
        if rational_denominator <= 0.0 || !rational_denominator.is_finite() {
            continue;
        }
        let local = fraction * first_weight / rational_denominator;
        let parameter = lower + local * (upper - lower);
        let Some(mapped) = cadmpeg_ir::eval::curve_point(geometry, parameter) else {
            continue;
        };
        let mismatch = [
            mapped.x - point[0],
            mapped.y - point[1],
            mapped.z - point[2],
        ];
        if dot(mismatch, mismatch).sqrt() <= tolerance
            && !candidates
                .iter()
                .any(|known| (parameter - known).abs() <= parameter_tolerance)
        {
            candidates.push(parameter);
        }
    }
    let [parameter] = candidates.as_slice() else {
        return None;
    };
    Some(*parameter)
}

#[derive(Clone, Copy)]
struct PeriodicConicFrame {
    center: [f64; 3],
    normal: [f64; 3],
    x_axis: [f64; 3],
    y_axis: [f64; 3],
    radii: [f64; 2],
}

#[derive(Clone, Copy)]
struct PlanarConicEquation {
    origin: [f64; 3],
    normal: [f64; 3],
    x_axis: [f64; 3],
    y_axis: [f64; 3],
    quadratic: [f64; 2],
    linear: [f64; 2],
    constant: f64,
    scale: f64,
}

#[derive(Clone, Copy)]
enum NonperiodicConicFamily {
    Parabola,
    Hyperbola,
}

#[derive(Clone, Copy)]
struct NonperiodicConicFrame {
    origin: [f64; 3],
    normal: [f64; 3],
    x_axis: [f64; 3],
    y_axis: [f64; 3],
    x_scale: f64,
    y_scale: f64,
    family: NonperiodicConicFamily,
}

fn planar_conic_equation(geometry: &CurveGeometry) -> Option<PlanarConicEquation> {
    if let Some(frame) = periodic_conic_frame(geometry) {
        return Some(PlanarConicEquation {
            origin: frame.center,
            normal: frame.normal,
            x_axis: frame.x_axis,
            y_axis: frame.y_axis,
            quadratic: [1.0 / frame.radii[0].powi(2), 1.0 / frame.radii[1].powi(2)],
            linear: [0.0, 0.0],
            constant: -1.0,
            scale: frame.radii.into_iter().fold(1.0, f64::max),
        });
    }
    let NonperiodicConicFrame {
        origin,
        normal,
        x_axis,
        y_axis,
        x_scale,
        y_scale,
        family,
    } = nonperiodic_conic_frame(geometry)?;
    let (quadratic, linear, constant) = match family {
        NonperiodicConicFamily::Parabola => ([0.0, -1.0 / (2.0 * y_scale)], [1.0, 0.0], 0.0),
        NonperiodicConicFamily::Hyperbola => (
            [1.0 / x_scale.powi(2), -1.0 / y_scale.powi(2)],
            [0.0, 0.0],
            -1.0,
        ),
    };
    Some(PlanarConicEquation {
        origin,
        normal,
        x_axis,
        y_axis,
        quadratic,
        linear,
        constant,
        scale: x_scale.max(y_scale),
    })
}

fn nonperiodic_conic_frame(geometry: &CurveGeometry) -> Option<NonperiodicConicFrame> {
    let (origin, normal, x_axis, x_scale, y_scale, family) = match geometry {
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => (
            [vertex.x, vertex.y, vertex.z],
            [axis.x, axis.y, axis.z],
            [major_direction.x, major_direction.y, major_direction.z],
            *focal_distance,
            2.0 * *focal_distance,
            NonperiodicConicFamily::Parabola,
        ),
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => (
            [center.x, center.y, center.z],
            [axis.x, axis.y, axis.z],
            [major_direction.x, major_direction.y, major_direction.z],
            *major_radius,
            *minor_radius,
            NonperiodicConicFamily::Hyperbola,
        ),
        _ => return None,
    };
    let normal = normalized(normal)?;
    let x_axis = normalized(x_axis)?;
    (dot(normal, x_axis).abs() <= 1e-9).then_some(())?;
    let y_axis = normalized(cross(normal, x_axis))?;
    (origin.into_iter().all(f64::is_finite)
        && x_scale > 0.0
        && x_scale.is_finite()
        && y_scale > 0.0
        && y_scale.is_finite())
    .then_some(())?;
    Some(NonperiodicConicFrame {
        origin,
        normal,
        x_axis,
        y_axis,
        x_scale,
        y_scale,
        family,
    })
}

fn periodic_conic_frame(geometry: &CurveGeometry) -> Option<PeriodicConicFrame> {
    let (center, axis, x_axis, radii) = match geometry {
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => (
            [center.x, center.y, center.z],
            [axis.x, axis.y, axis.z],
            [ref_direction.x, ref_direction.y, ref_direction.z],
            [*radius, *radius],
        ),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => (
            [center.x, center.y, center.z],
            [axis.x, axis.y, axis.z],
            [major_direction.x, major_direction.y, major_direction.z],
            [*major_radius, *minor_radius],
        ),
        _ => return None,
    };
    let axis = normalized(axis)?;
    let x_axis = normalized(x_axis)?;
    (dot(axis, x_axis).abs() <= 1e-9).then_some(())?;
    let y_axis = normalized(cross(axis, x_axis))?;
    (center.into_iter().all(f64::is_finite)
        && radii
            .into_iter()
            .all(|radius| radius > 0.0 && radius.is_finite()))
    .then_some(PeriodicConicFrame {
        center,
        normal: axis,
        x_axis,
        y_axis,
        radii,
    })
}

fn nonperiodic_conic_parameter(geometry: &CurveGeometry, point: [f64; 3]) -> Option<f64> {
    let NonperiodicConicFrame {
        origin,
        normal,
        x_axis,
        y_axis,
        x_scale,
        y_scale,
        family,
    } = nonperiodic_conic_frame(geometry)?;
    let relative = std::array::from_fn(|index| point[index] - origin[index]);
    let scale = dot(relative, relative)
        .sqrt()
        .max(x_scale)
        .max(y_scale)
        .max(1.0);
    (dot(relative, normal).abs() <= 1e-7 * scale).then_some(())?;
    let x = dot(relative, x_axis);
    let y = dot(relative, y_axis);
    let parameter = match family {
        NonperiodicConicFamily::Parabola => y / y_scale,
        NonperiodicConicFamily::Hyperbola => (y / y_scale).asinh(),
    };
    let expected_x = match family {
        NonperiodicConicFamily::Parabola => x_scale * parameter * parameter,
        NonperiodicConicFamily::Hyperbola => x_scale * parameter.cosh(),
    };
    (parameter.is_finite() && (x - expected_x).abs() <= 1e-7 * scale).then_some(parameter)
}

fn nonperiodic_conic_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    let [Some(first), Some(second)] =
        points.map(|point| nonperiodic_conic_parameter(geometry, point))
    else {
        return None;
    };
    let parameters = if first <= second {
        [first, second]
    } else {
        [second, first]
    };
    (parameters[1] - parameters[0] > 1e-12).then_some(parameters)
}

fn periodic_conic_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
    interior: [f64; 3],
) -> Option<[f64; 2]> {
    if !curve_contains_points(geometry, points)
        || !curve_contains_points(geometry, [interior, interior])
    {
        return None;
    }
    let PeriodicConicFrame {
        center,
        x_axis,
        y_axis,
        radii,
        ..
    } = periodic_conic_frame(geometry)?;
    let parameter = |point: [f64; 3]| {
        let relative = std::array::from_fn(|index| point[index] - center[index]);
        (dot(relative, y_axis) / radii[1])
            .atan2(dot(relative, x_axis) / radii[0])
            .rem_euclid(std::f64::consts::TAU)
    };
    let [first, second] = points.map(parameter);
    let increasing = |start: f64, end: f64| {
        [
            start,
            if end < start {
                end + std::f64::consts::TAU
            } else {
                end
            },
        ]
    };
    let first_arc = increasing(first, second);
    let second_arc = if (first - second).abs() <= 1e-12 {
        [first, first + std::f64::consts::TAU]
    } else {
        increasing(second, first)
    };
    let scale = radii.into_iter().fold(1.0, f64::max);
    let matches_interior = |range: [f64; 2]| {
        cadmpeg_ir::eval::curve_point(geometry, f64::midpoint(range[0], range[1])).is_some_and(
            |point| {
                let point = [point.x, point.y, point.z];
                dot(
                    std::array::from_fn(|index| point[index] - interior[index]),
                    std::array::from_fn(|index| point[index] - interior[index]),
                )
                .sqrt()
                    <= 1e-9 * scale
            },
        )
    };
    let selected = match (matches_interior(first_arc), matches_interior(second_arc)) {
        (true, false) => first_arc,
        (false, true) => second_arc,
        _ => return None,
    };
    (selected[1] - selected[0] > 1e-12).then_some(selected)
}

fn full_periodic_conic_edge_parameter_range(
    geometry: &CurveGeometry,
    point: [f64; 3],
) -> Option<[f64; 2]> {
    curve_contains_points(geometry, [point, point]).then_some(())?;
    let PeriodicConicFrame {
        center,
        x_axis,
        y_axis,
        radii,
        ..
    } = periodic_conic_frame(geometry)?;
    let relative = std::array::from_fn(|index| point[index] - center[index]);
    let start = (dot(relative, y_axis) / radii[1])
        .atan2(dot(relative, x_axis) / radii[0])
        .rem_euclid(std::f64::consts::TAU);
    Some([start, start + std::f64::consts::TAU])
}

fn native_pcurve_midpoint(
    surface: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
    edge_points: [[f64; 3]; 2],
) -> Option<[f64; 3]> {
    let mapped = endpoints.map(|uv| {
        cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1])
            .map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    point_pair_alignments([first, second], edge_points)
        .into_iter()
        .any(|matches| matches)
        .then_some(())?;
    let uv = [
        f64::midpoint(endpoints[0][0], endpoints[1][0]),
        f64::midpoint(endpoints[0][1], endpoints[1][1]),
    ];
    cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1]).map(|point| [point.x, point.y, point.z])
}

type NativePcurveCandidates = BTreeMap<(u32, u32), Vec<([[f64; 2]; 2], usize)>>;

fn pcurve_backed_periodic_conic_parameter_range(
    geometry: &CurveGeometry,
    curve_id: u32,
    faces: [u32; 2],
    candidates: &NativePcurveCandidates,
    surfaces: &[Surface],
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    let mut selected = None;
    for face_id in faces {
        let Some(surface) = surfaces
            .iter()
            .find(|surface| surface.id == SurfaceId(format!("creo:visibgeom:surface#{face_id}")))
            .map(|surface| &surface.geometry)
        else {
            continue;
        };
        for (endpoints, _) in candidates.get(&(curve_id, face_id)).into_iter().flatten() {
            let Some(interior) = native_pcurve_midpoint(surface, *endpoints, points) else {
                continue;
            };
            let candidate = periodic_conic_edge_parameter_range(geometry, points, interior)?;
            if selected.is_some_and(|selected: [f64; 2]| {
                candidate
                    .into_iter()
                    .zip(selected)
                    .any(|(candidate, selected)| (candidate - selected).abs() > 1e-9)
            }) {
                return None;
            }
            selected = Some(candidate);
        }
    }
    selected
}

#[cfg(test)]
mod native_edge_parameter_tests;

fn oriented_native_pcurve_endpoints(
    surface: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
    traversal: [[f64; 3]; 2],
) -> Option<[[f64; 2]; 2]> {
    let mapped = endpoints.map(|uv| {
        cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1])
            .map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    match point_pair_alignments([first, second], traversal) {
        [true, false] => Some(endpoints),
        [false, true] => Some([endpoints[1], endpoints[0]]),
        _ => None,
    }
}

fn unique_oriented_native_pcurve(
    surface: &SurfaceGeometry,
    candidates: &[([[f64; 2]; 2], usize)],
    traversal: [[f64; 3]; 2],
) -> Option<([[f64; 2]; 2], usize)> {
    let mut matching = candidates.iter().filter_map(|(endpoints, offset)| {
        oriented_native_pcurve_endpoints(surface, *endpoints, traversal)
            .map(|oriented| (oriented, *offset))
    });
    let mut selected = matching.next()?;
    for candidate in matching {
        if candidate.0 != selected.0 {
            return None;
        }
        selected.1 = selected.1.min(candidate.1);
    }
    Some(selected)
}

fn planar_curve_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = surface
    else {
        return None;
    };
    let origin = [origin.x, origin.y, origin.z];
    let normal = normalized([normal.x, normal.y, normal.z])?;
    let u_axis = normalized([u_axis.x, u_axis.y, u_axis.z])?;
    (dot(normal, u_axis).abs() <= 1e-10).then_some(())?;
    let v_axis = normalized(cross(normal, u_axis))?;
    let project_point = |point: [f64; 3], tolerance: f64| {
        let relative: [f64; 3] = std::array::from_fn(|index| point[index] - origin[index]);
        (dot(relative, normal).abs() <= tolerance)
            .then_some(Point2::new(dot(relative, u_axis), dot(relative, v_axis)))
    };
    let project_direction = |direction: [f64; 3]| {
        let length = dot(direction, direction).sqrt();
        (length.is_finite() && length > 0.0 && dot(direction, normal).abs() <= 1e-10 * length)
            .then_some(Point2::new(dot(direction, u_axis), dot(direction, v_axis)))
    };
    let conic_frame = |center: [f64; 3], axis: [f64; 3], x_axis: [f64; 3], scale: f64| {
        let axis = normalized(axis)?;
        let x_axis = normalized(x_axis)?;
        ((dot(axis, normal).abs() - 1.0).abs() <= 1e-10 && dot(axis, x_axis).abs() <= 1e-10)
            .then_some(())?;
        let y_axis = normalized(cross(axis, x_axis))?;
        Some((
            project_point(center, 1e-9 * scale.max(1.0))?,
            project_direction(x_axis)?,
            project_direction(y_axis)?,
        ))
    };

    match geometry {
        CurveGeometry::Line { origin, direction } => {
            let direction = [direction.x, direction.y, direction.z];
            Some(PcurveGeometry::Line {
                origin: project_point([origin.x, origin.y, origin.z], 1e-9)?,
                direction: project_direction(direction)?,
            })
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } if radius.is_finite() && *radius > 0.0 => {
            let (center, x_axis, y_axis) = conic_frame(
                [center.x, center.y, center.z],
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
                *radius,
            )?;
            Some(PcurveGeometry::Circle {
                center,
                x_axis,
                y_axis,
                radius: *radius,
            })
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if major_radius.is_finite()
            && minor_radius.is_finite()
            && *major_radius > 0.0
            && *minor_radius > 0.0 =>
        {
            let (center, x_axis, y_axis) = conic_frame(
                [center.x, center.y, center.z],
                [axis.x, axis.y, axis.z],
                [major_direction.x, major_direction.y, major_direction.z],
                major_radius.max(*minor_radius),
            )?;
            Some(PcurveGeometry::Ellipse {
                center,
                x_axis,
                y_axis,
                major_radius: *major_radius,
                minor_radius: *minor_radius,
            })
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } if focal_distance.is_finite() && *focal_distance > 0.0 => {
            let (vertex, x_axis, y_axis) = conic_frame(
                [vertex.x, vertex.y, vertex.z],
                [axis.x, axis.y, axis.z],
                [major_direction.x, major_direction.y, major_direction.z],
                *focal_distance,
            )?;
            Some(PcurveGeometry::Parabola {
                vertex,
                x_axis,
                y_axis,
                focal_distance: *focal_distance,
            })
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if major_radius.is_finite()
            && minor_radius.is_finite()
            && *major_radius > 0.0
            && *minor_radius > 0.0 =>
        {
            let (center, x_axis, y_axis) = conic_frame(
                [center.x, center.y, center.z],
                [axis.x, axis.y, axis.z],
                [major_direction.x, major_direction.y, major_direction.z],
                major_radius.max(*minor_radius),
            )?;
            Some(PcurveGeometry::Hyperbola {
                center,
                x_axis,
                y_axis,
                major_radius: *major_radius,
                minor_radius: *minor_radius,
            })
        }
        CurveGeometry::Nurbs(nurbs) => {
            nurbs_intrinsic_parameter_range(nurbs)?;
            nurbs
                .weights
                .as_ref()
                .is_none_or(|weights| weights.iter().all(|weight| weight.is_finite()))
                .then_some(())?;
            let tolerance = 1e-9 * nurbs_control_extent(nurbs)?;
            let control_points = nurbs
                .control_points
                .iter()
                .map(|point| project_point([point.x, point.y, point.z], tolerance))
                .collect::<Option<Vec<_>>>()?;
            Some(PcurveGeometry::Nurbs {
                degree: nurbs.degree,
                knots: nurbs.knots.clone(),
                control_points,
                weights: nurbs.weights.clone(),
                periodic: nurbs.periodic,
            })
        }
        _ => None,
    }
}

fn stored_unit_vector(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(vector, vector).sqrt();
    (length.is_finite() && (length - 1.0).abs() <= 1e-10).then_some(vector)
}

fn surface_of_revolution_parallel_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let (center, conic_axis, conic_x, conic_radii) = match geometry {
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } if radius.is_finite() && *radius > 0.0 => {
            (*center, *axis, *ref_direction, [*radius, *radius])
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if major_radius.is_finite()
            && minor_radius.is_finite()
            && *major_radius > 0.0
            && *minor_radius > 0.0 =>
        {
            (
                *center,
                *axis,
                *major_direction,
                [*major_radius, *minor_radius],
            )
        }
        _ => return None,
    };
    let (origin, axis, ref_direction) = match surface {
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } if radius.is_finite() && *radius > 0.0 => (*origin, *axis, *ref_direction),
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } if radius.is_finite() && ratio.is_finite() && *ratio > 0.0 && half_angle.is_finite() => {
            half_angle.tan().is_finite().then_some(())?;
            (*origin, *axis, *ref_direction)
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } if radius.is_finite() && *radius > 0.0 => (*center, *axis, *ref_direction),
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if major_radius.is_finite()
            && minor_radius.is_finite()
            && *major_radius > 0.0
            && *minor_radius > 0.0 =>
        {
            (*center, *axis, *ref_direction)
        }
        _ => return None,
    };
    let surface_axis = stored_unit_vector([axis.x, axis.y, axis.z])?;
    let surface_x = stored_unit_vector([ref_direction.x, ref_direction.y, ref_direction.z])?;
    (dot(surface_axis, surface_x).abs() <= 1e-10).then_some(())?;
    let surface_y = cross(surface_axis, surface_x);
    let conic_axis = stored_unit_vector([conic_axis.x, conic_axis.y, conic_axis.z])?;
    let conic_x = stored_unit_vector([conic_x.x, conic_x.y, conic_x.z])?;
    (dot(conic_axis, conic_x).abs() <= 1e-10
        && (dot(conic_axis, surface_axis).abs() - 1.0).abs() <= 1e-10)
        .then_some(())?;
    let conic_y = cross(conic_axis, conic_x);
    let center_relative = [
        center.x - origin.x,
        center.y - origin.y,
        center.z - origin.z,
    ];
    let axial = dot(center_relative, surface_axis);
    let center_radial = std::array::from_fn::<_, 3, _>(|index| {
        center_relative[index] - axial * surface_axis[index]
    });
    let (v, surface_radii) = match surface {
        SurfaceGeometry::Cylinder { radius, .. } => (axial, [*radius, *radius]),
        SurfaceGeometry::Cone {
            radius,
            ratio,
            half_angle,
            ..
        } => {
            let local_radius = radius + axial * half_angle.tan();
            (axial, [local_radius, local_radius * ratio])
        }
        SurfaceGeometry::Sphere { radius, .. } => {
            ((conic_radii[0] - conic_radii[1]).abs()
                <= 1e-9 * conic_radii.into_iter().fold(1.0, f64::max))
            .then_some(())?;
            let scale = radius.abs().max(conic_radii[0]).max(1.0);
            ((axial.mul_add(axial, conic_radii[0] * conic_radii[0]) - radius * radius).abs()
                <= 1e-9 * scale * scale)
                .then_some(())?;
            let polar = axial.atan2(conic_radii[0]);
            let ring = radius * polar.cos();
            (polar, [ring, ring])
        }
        SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ..
        } => {
            ((conic_radii[0] - conic_radii[1]).abs()
                <= 1e-9 * conic_radii.into_iter().fold(1.0, f64::max))
            .then_some(())?;
            let candidates = [conic_radii[0], -conic_radii[0]]
                .into_iter()
                .filter_map(|ring| {
                    let sine = axial / minor_radius;
                    let cosine = (ring - major_radius) / minor_radius;
                    ((sine.mul_add(sine, cosine * cosine) - 1.0).abs() <= 1e-9)
                        .then_some((sine.atan2(cosine), ring))
                })
                .collect::<Vec<_>>();
            let [candidate] = candidates.as_slice() else {
                return None;
            };
            (candidate.0, [candidate.1, candidate.1])
        }
        _ => unreachable!(),
    };
    let scale = surface_radii
        .into_iter()
        .chain(conic_radii)
        .map(f64::abs)
        .fold(1.0, f64::max);
    (dot(center_radial, center_radial).sqrt() <= 1e-9 * scale
        && surface_radii
            .iter()
            .all(|radius| radius.abs() > 1e-12 * scale)
        && surface_radii
            .into_iter()
            .map(f64::abs)
            .zip(conic_radii)
            .all(|(surface_radius, conic_radius)| {
                (surface_radius - conic_radius).abs() <= 1e-9 * scale
            }))
    .then_some(())?;
    let radius_sign = surface_radii[0].signum();
    let phase =
        (radius_sign * dot(conic_x, surface_y)).atan2(radius_sign * dot(conic_x, surface_x));
    let surface_tangent = std::array::from_fn::<_, 3, _>(|index| {
        -phase.sin() * surface_x[index] + phase.cos() * surface_y[index]
    });
    let orientation = radius_sign * dot(conic_y, surface_tangent);
    ((orientation.abs() - 1.0).abs() <= 1e-10).then_some(())?;
    Some(PcurveGeometry::Line {
        origin: Point2::new(phase, v),
        direction: Point2::new(orientation.signum(), 0.0),
    })
}

fn meridian_circle_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let (surface_center, surface_axis, surface_x, major_radius, meridian_radius) = match surface {
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } if radius.is_finite() && *radius > 0.0 => (*center, *axis, *ref_direction, None, *radius),
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if major_radius.is_finite()
            && minor_radius.is_finite()
            && *major_radius > 0.0
            && *minor_radius > 0.0 =>
        {
            (
                *center,
                *axis,
                *ref_direction,
                Some(*major_radius),
                *minor_radius,
            )
        }
        _ => return None,
    };
    let CurveGeometry::Circle {
        center: circle_center,
        axis: circle_axis,
        ref_direction: circle_x,
        radius: circle_radius,
    } = geometry
    else {
        return None;
    };
    (circle_radius.is_finite() && *circle_radius > 0.0).then_some(())?;
    let surface_axis = stored_unit_vector([surface_axis.x, surface_axis.y, surface_axis.z])?;
    let surface_x = stored_unit_vector([surface_x.x, surface_x.y, surface_x.z])?;
    (dot(surface_axis, surface_x).abs() <= 1e-10).then_some(())?;
    let surface_y = cross(surface_axis, surface_x);
    let circle_axis = stored_unit_vector([circle_axis.x, circle_axis.y, circle_axis.z])?;
    let circle_x = stored_unit_vector([circle_x.x, circle_x.y, circle_x.z])?;
    (dot(circle_axis, circle_x).abs() <= 1e-10).then_some(())?;
    let circle_y = cross(circle_axis, circle_x);
    let center_relative = [
        circle_center.x - surface_center.x,
        circle_center.y - surface_center.y,
        circle_center.z - surface_center.z,
    ];
    let scale = major_radius
        .unwrap_or(0.0)
        .abs()
        .max(meridian_radius.abs())
        .max(circle_radius.abs())
        .max(1.0);
    ((circle_radius - meridian_radius).abs() <= 1e-9 * scale).then_some(())?;
    let radial = if let Some(major_radius) = major_radius {
        let axial = dot(center_relative, surface_axis);
        let radial = std::array::from_fn::<_, 3, _>(|index| {
            center_relative[index] - axial * surface_axis[index]
        });
        let radial_length = dot(radial, radial).sqrt();
        (axial.abs() <= 1e-9 * scale && (radial_length - major_radius).abs() <= 1e-9 * scale)
            .then_some(())?;
        radial.map(|coordinate| coordinate / radial_length)
    } else {
        (dot(center_relative, center_relative).sqrt() <= 1e-9 * scale).then_some(())?;
        let radial = cross(circle_axis, surface_axis);
        stored_unit_vector(radial)?
    };
    let meridian_normal = cross(surface_axis, radial);
    ((dot(circle_axis, meridian_normal).abs() - 1.0).abs() <= 1e-10).then_some(())?;
    let u = dot(radial, surface_y).atan2(dot(radial, surface_x));
    let phase = dot(circle_x, surface_axis).atan2(dot(circle_x, radial));
    let surface_tangent = std::array::from_fn::<_, 3, _>(|index| {
        -phase.sin() * radial[index] + phase.cos() * surface_axis[index]
    });
    let orientation = dot(circle_y, surface_tangent);
    ((orientation.abs() - 1.0).abs() <= 1e-10).then_some(())?;
    Some(PcurveGeometry::Line {
        origin: Point2::new(u, phase),
        direction: Point2::new(0.0, orientation.signum()),
    })
}

fn ruled_generator_line_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let CurveGeometry::Line {
        origin: line_origin,
        direction: line_direction,
    } = geometry
    else {
        return None;
    };
    let (surface_origin, surface_axis, surface_x, reference_radius, radius_ratio, radius_slope) =
        match surface {
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            } if radius.is_finite() && *radius > 0.0 => {
                (*origin, *axis, *ref_direction, *radius, 1.0, 0.0)
            }
            SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                ratio,
                half_angle,
            } if radius.is_finite()
                && ratio.is_finite()
                && *ratio > 0.0
                && half_angle.is_finite() =>
            {
                let slope = half_angle.tan();
                slope.is_finite().then_some((
                    *origin,
                    *axis,
                    *ref_direction,
                    *radius,
                    *ratio,
                    slope,
                ))?
            }
            _ => return None,
        };
    let surface_axis = stored_unit_vector([surface_axis.x, surface_axis.y, surface_axis.z])?;
    let surface_x = stored_unit_vector([surface_x.x, surface_x.y, surface_x.z])?;
    (dot(surface_axis, surface_x).abs() <= 1e-10).then_some(())?;
    let surface_y = cross(surface_axis, surface_x);
    let relative = [
        line_origin.x - surface_origin.x,
        line_origin.y - surface_origin.y,
        line_origin.z - surface_origin.z,
    ];
    let v = dot(relative, surface_axis);
    let radial = std::array::from_fn::<_, 3, _>(|index| relative[index] - v * surface_axis[index]);
    let local_radius = reference_radius + v * radius_slope;
    let scale = local_radius
        .abs()
        .max((local_radius * radius_ratio).abs())
        .max(1.0);
    (local_radius.abs() > 1e-12 * scale).then_some(())?;
    let chart_x = dot(radial, surface_x) / local_radius;
    let chart_y = dot(radial, surface_y) / (local_radius * radius_ratio);
    (chart_x.is_finite()
        && chart_y.is_finite()
        && (chart_x.mul_add(chart_x, chart_y * chart_y) - 1.0).abs() <= 1e-9)
        .then_some(())?;
    let u = chart_y.atan2(chart_x);
    let chart_radial = std::array::from_fn::<_, 3, _>(|index| {
        chart_x * surface_x[index] + radius_ratio * chart_y * surface_y[index]
    });
    let surface_derivative = std::array::from_fn::<_, 3, _>(|index| {
        surface_axis[index] + radius_slope * chart_radial[index]
    });
    let line_direction = [line_direction.x, line_direction.y, line_direction.z];
    let direction_length = dot(line_direction, line_direction).sqrt();
    let derivative_norm = dot(surface_derivative, surface_derivative);
    (direction_length.is_finite()
        && direction_length > 0.0
        && derivative_norm.is_finite()
        && derivative_norm > 0.0)
        .then_some(())?;
    let parameter_scale = dot(line_direction, surface_derivative) / derivative_norm;
    let residual = std::array::from_fn::<_, 3, _>(|index| {
        line_direction[index] - parameter_scale * surface_derivative[index]
    });
    (parameter_scale.is_finite()
        && parameter_scale.abs() > 0.0
        && dot(residual, residual).sqrt() <= 1e-10 * direction_length)
        .then_some(())?;
    Some(PcurveGeometry::Line {
        origin: Point2::new(u, v),
        direction: Point2::new(0.0, parameter_scale),
    })
}

#[cfg(test)]
mod native_pcurve_tests;

fn transfer_native_brep(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    derived_intersection_curves: &BTreeSet<CurveId>,
    analytic_pcurve_carriers: &BTreeSet<CurveId>,
) -> (usize, usize) {
    let planes = placed_planes(scan);
    let carriers = placed_carriers(scan, ir);
    let face_orientations = native_face_orientations(scan, ir);
    let half_edges = scan
        .topology
        .half_edges
        .iter()
        .map(|half_edge| (half_edge.id, half_edge))
        .collect::<BTreeMap<_, _>>();
    let incidence = scan
        .topology
        .half_edge_vertex_incidence
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let solved_vertices = solved_topological_vertices(scan, ir, &carriers);
    let mut native_pcurves = NativePcurveCandidates::new();
    for (curve_id, faces, face_0_endpoints, face_1_endpoints, offset) in scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
                pcurve.offset,
            )
        })
        .chain(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
                pcurve.offset,
            )
        }))
    {
        native_pcurves
            .entry((curve_id, faces[0]))
            .or_default()
            .push((face_0_endpoints, offset));
        native_pcurves
            .entry((curve_id, faces[1]))
            .or_default()
            .push((face_1_endpoints, offset));
    }
    let native_edge_vertices =
        crate::topology::edge_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    let edge_vertices = scan
        .curves
        .topology_rows
        .iter()
        .filter_map(|row| {
            let vertices = native_edge_vertices.get(&row.id).copied()?;
            vertices
                .iter()
                .all(|vertex| solved_vertices.contains_key(vertex))
                .then_some((row.id, vertices))
        })
        .collect::<BTreeMap<_, _>>();
    let mut loops_by_face = BTreeMap::<u32, Vec<&crate::topology::Loop>>::new();
    for lp in &scan.topology.loops {
        loops_by_face.entry(lp.face_id).or_default().push(lp);
    }
    let eligible_faces = loops_by_face
        .into_iter()
        .filter_map(|(face_id, loops)| {
            face_orientations.contains_key(&face_id).then_some(())?;
            loops
                .iter()
                .all(|lp| {
                    lp.half_edges
                        .iter()
                        .all(|half_edge| edge_vertices.contains_key(&half_edge.curve_id))
                })
                .then_some(())?;
            let ordered = ordered_face_loops(
                loops,
                planes.get(&face_id).copied(),
                &incidence,
                &solved_vertices,
            )?;
            Some((face_id, ordered))
        })
        .collect::<BTreeMap<_, _>>();
    let eligible_loops = eligible_faces
        .values()
        .flatten()
        .copied()
        .collect::<Vec<_>>();

    let emitted_half_edges = eligible_loops
        .iter()
        .flat_map(|lp| lp.half_edges.iter().copied())
        .collect::<BTreeSet<_>>();
    let face_curves = emitted_half_edges
        .iter()
        .map(|half_edge| half_edge.curve_id)
        .collect::<BTreeSet<_>>();
    let closed_single_edge_curves = face_curves
        .iter()
        .filter(|curve_id| {
            let uses = eligible_loops
                .iter()
                .filter(|lp| {
                    lp.half_edges
                        .iter()
                        .any(|half_edge| half_edge.curve_id == **curve_id)
                })
                .collect::<Vec<_>>();
            !uses.is_empty() && uses.iter().all(|lp| lp.half_edges.len() == 1)
        })
        .copied()
        .collect::<BTreeSet<_>>();
    let emitted_curves = face_curves.clone();
    let used_vertices = emitted_curves
        .iter()
        .filter_map(|curve| edge_vertices.get(curve))
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    let row_offsets = scan
        .curves
        .topology_rows
        .iter()
        .map(|row| (row.id, row.offset))
        .collect::<BTreeMap<_, _>>();
    let curve_faces = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
        .into_iter()
        .map(|row| (row.id, row.faces))
        .collect::<BTreeMap<_, _>>();

    let solved_point_count = used_vertices.len();
    for vertex_id in used_vertices {
        let point_id = PointId(format!("creo:visibgeom:point#{vertex_id}"));
        let vertex = VertexId(format!("creo:visibgeom:vertex#{vertex_id}"));
        if ir.model.vertices.iter().any(|item| item.id == vertex) {
            continue;
        }
        annotate(
            annotations,
            &point_id,
            "VisibGeom",
            0,
            "topological_vertex_point",
            Exactness::Derived,
        );
        let position = solved_vertices[&vertex_id];
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: Point3::new(position[0], position[1], position[2]),
            source_object: None,
        });
        annotate(
            annotations,
            &vertex,
            "VisibGeom",
            0,
            "topological_vertex_orbit",
            Exactness::Derived,
        );
        ir.model.vertices.push(Vertex {
            id: vertex,
            point: point_id,
            tolerance: None,
        });
    }
    for curve_id in &emitted_curves {
        let [start, end] = edge_vertices[curve_id];
        let curve = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
        let points = [solved_vertices[&start], solved_vertices[&end]];
        let unbacked_closed_edge = start == end
            && closed_single_edge_curves.contains(curve_id)
            && curve_faces.get(curve_id).is_some_and(|face_ids| {
                !face_ids
                    .iter()
                    .any(|face_id| native_pcurves.contains_key(&(*curve_id, *face_id)))
            });
        let derived_line = (derived_intersection_curves.contains(&curve)
            || analytic_pcurve_carriers.contains(&curve))
            && ir.model.curves.iter().any(|candidate| {
                candidate.id == curve && matches!(candidate.geometry, CurveGeometry::Line { .. })
            });
        let param_range = if derived_line {
            ir.model
                .curves
                .iter_mut()
                .find(|candidate| candidate.id == curve)
                .and_then(|candidate| orient_line_edge_carrier(&mut candidate.geometry, points))
        } else {
            ir.model
                .curves
                .iter()
                .find(|candidate| candidate.id == curve)
                .and_then(|candidate| {
                    exact_line_edge_parameter_range(&candidate.geometry, points).or_else(|| {
                        nonperiodic_nurbs_edge_parameter_range(&candidate.geometry, points).or_else(
                            || {
                                nonperiodic_conic_edge_parameter_range(&candidate.geometry, points)
                                    .or_else(|| {
                                        pcurve_backed_periodic_conic_parameter_range(
                                            &candidate.geometry,
                                            *curve_id,
                                            *curve_faces.get(curve_id)?,
                                            &native_pcurves,
                                            &ir.model.surfaces,
                                            points,
                                        )
                                    })
                                    .or_else(|| {
                                        unbacked_closed_edge.then_some(()).and_then(|()| {
                                            full_periodic_conic_edge_parameter_range(
                                                &candidate.geometry,
                                                points[0],
                                            )
                                        })
                                    })
                                    .or_else(|| {
                                        unbacked_closed_edge.then_some(()).and_then(|()| {
                                            full_periodic_nurbs_edge_parameter_range(
                                                &candidate.geometry,
                                                points[0],
                                            )
                                        })
                                    })
                            },
                        )
                    })
                })
        };
        let id = EdgeId(format!("creo:visibgeom:edge#{curve_id}"));
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row_offsets.get(curve_id).copied().unwrap_or(0) as u64,
            "curve_topology_edge",
            Exactness::Derived,
        );
        ir.model.edges.push(Edge {
            id,
            curve: Some(curve.clone()),
            start: VertexId(format!("creo:visibgeom:vertex#{start}")),
            end: VertexId(format!("creo:visibgeom:vertex#{end}")),
            param_range,
            tolerance: None,
        });
        if !ir.model.curves.iter().any(|item| item.id == curve) {
            let offset = row_offsets.get(curve_id).copied().unwrap_or(0);
            annotate(
                annotations,
                &curve,
                "VisibGeom",
                offset as u64,
                "opaque_native_curve_carrier",
                Exactness::Unknown,
            );
            ir.model.curves.push(Curve {
                id: curve,
                geometry: CurveGeometry::Unknown {
                    record: geometry_section_record(scan, offset),
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{curve_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
    }

    let mut face_adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    for face_id in eligible_faces.keys() {
        face_adjacency.entry(*face_id).or_default();
    }
    for curve_id in &emitted_curves {
        let faces = emitted_half_edges
            .iter()
            .filter(|half_edge| half_edge.curve_id == *curve_id)
            .filter_map(|half_edge| half_edges.get(half_edge))
            .map(|half_edge| half_edge.face_id)
            .collect::<Vec<_>>();
        if let [first, second] = faces.as_slice() {
            if eligible_faces.contains_key(first) && eligible_faces.contains_key(second) {
                face_adjacency.entry(*first).or_default().insert(*second);
                face_adjacency.entry(*second).or_default().insert(*first);
            }
        }
    }
    let mut remaining = face_adjacency.keys().copied().collect::<BTreeSet<_>>();
    let mut components = Vec::new();
    while let Some(start) = remaining.pop_first() {
        let mut component = BTreeSet::from([start]);
        let mut pending = vec![start];
        while let Some(face) = pending.pop() {
            for neighbour in face_adjacency.get(&face).into_iter().flatten() {
                if remaining.remove(neighbour) {
                    component.insert(*neighbour);
                    pending.push(*neighbour);
                }
            }
        }
        components.push(component);
    }

    for (component_index, faces) in components.iter().enumerate() {
        let body_id = BodyId(format!("creo:visibgeom:body#{}", component_index + 1));
        let region_id = RegionId(format!("creo:visibgeom:region#{}", component_index + 1));
        let shell_id = ShellId(format!("creo:visibgeom:shell#{}", component_index + 1));
        for (id, tag) in [
            (body_id.to_string(), "native_component_body"),
            (region_id.to_string(), "native_component_region"),
            (shell_id.to_string(), "native_component_shell"),
        ] {
            annotate(annotations, id, "VisibGeom", 0, tag, Exactness::Derived);
        }
        let component_curves = eligible_loops
            .iter()
            .filter(|lp| faces.contains(&lp.face_id))
            .flat_map(|lp| lp.half_edges.iter().map(|half_edge| half_edge.curve_id))
            .collect::<BTreeSet<_>>();
        let closed = component_curves.iter().all(|curve_id| {
            let adjacent = emitted_half_edges
                .iter()
                .filter(|half_edge| half_edge.curve_id == *curve_id)
                .filter_map(|half_edge| half_edges.get(half_edge))
                .map(|half_edge| half_edge.face_id)
                .collect::<BTreeSet<_>>();
            adjacent.len() == 2 && adjacent.iter().all(|face| faces.contains(face))
        });
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind: if closed {
                BodyKind::Solid
            } else {
                BodyKind::Sheet
            },
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id,
            shells: vec![shell_id.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id,
            faces: faces
                .iter()
                .map(|face| FaceId(format!("creo:visibgeom:face#{face}")))
                .collect(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        for face_id in faces {
            let native_loops = &eligible_faces[face_id];
            let face = FaceId(format!("creo:visibgeom:face#{face_id}"));
            let loop_ids = (0..native_loops.len())
                .map(|index| {
                    if index == 0 {
                        LoopId(format!("creo:visibgeom:loop#{face_id}"))
                    } else {
                        LoopId(format!("creo:visibgeom:loop#{face_id}:{index}"))
                    }
                })
                .collect::<Vec<_>>();
            let face_offset = crate::surface::unique_surface_row(&scan.surfaces.rows, *face_id)
                .map_or(0, |row| row.offset);
            let surface = SurfaceId(format!("creo:visibgeom:surface#{face_id}"));
            if !ir.model.surfaces.iter().any(|item| item.id == surface) {
                annotate(
                    annotations,
                    &surface,
                    "VisibGeom",
                    face_offset as u64,
                    "opaque_native_surface_carrier",
                    Exactness::Unknown,
                );
                ir.model.surfaces.push(Surface {
                    id: surface.clone(),
                    geometry: SurfaceGeometry::Unknown {
                        record: geometry_section_record(scan, face_offset),
                    },
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!("VisibGeom:{face_id}"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
            let face_sense = if face_orientations[face_id] {
                Sense::Reversed
            } else {
                Sense::Forward
            };
            annotate(
                annotations,
                &face,
                "VisibGeom",
                face_offset as u64,
                "native_face",
                Exactness::Derived,
            );
            for loop_id in &loop_ids {
                annotate(
                    annotations,
                    loop_id,
                    "VisibGeom",
                    face_offset as u64,
                    "native_face_loop",
                    Exactness::Derived,
                );
            }
            ir.model.faces.push(Face {
                id: face.clone(),
                shell: shell_id.clone(),
                surface,
                sense: face_sense,
                loops: loop_ids.clone(),
                name: None,
                color: None,
                tolerance: None,
            });
            for (boundary_index, (native_loop, loop_id)) in
                native_loops.iter().zip(loop_ids).enumerate()
            {
                let coedge_ids = native_loop
                    .half_edges
                    .iter()
                    .map(|half_edge| {
                        CoedgeId(format!(
                            "creo:visibgeom:coedge#{}:{}",
                            half_edge.curve_id, half_edge.side
                        ))
                    })
                    .collect::<Vec<_>>();
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face.clone(),
                    boundary_role: if boundary_index == 0 {
                        cadmpeg_ir::topology::LoopBoundaryRole::Outer
                    } else {
                        cadmpeg_ir::topology::LoopBoundaryRole::Inner
                    },
                    coedges: coedge_ids.clone(),
                    vertex_uses: Vec::new(),
                });
                for (index, half_edge) in native_loop.half_edges.iter().enumerate() {
                    let id = coedge_ids[index].clone();
                    let twin = HalfEdgeId {
                        curve_id: half_edge.curve_id,
                        side: 1 - half_edge.side,
                    };
                    let radial_next = if emitted_half_edges.contains(&twin) {
                        CoedgeId(format!(
                            "creo:visibgeom:coedge#{}:{}",
                            twin.curve_id, twin.side
                        ))
                    } else {
                        id.clone()
                    };
                    annotate(
                        annotations,
                        &id,
                        "VisibGeom",
                        row_offsets.get(&half_edge.curve_id).copied().unwrap_or(0) as u64,
                        "native_half_edge",
                        Exactness::Derived,
                    );
                    let native_candidates = native_pcurves.get(&(half_edge.curve_id, *face_id));
                    let pcurve_geometry = native_candidates
                        .and_then(|candidates| {
                            let incidence = incidence.get(half_edge)?;
                            let end = incidence.end_vertex_id?;
                            let traversal = [
                                solved_vertices[&incidence.start_vertex_id],
                                solved_vertices[&end],
                            ];
                            let surface = ir.model.surfaces.iter().find(|candidate| {
                                candidate.id
                                    == SurfaceId(format!("creo:visibgeom:surface#{face_id}"))
                            })?;
                            unique_oriented_native_pcurve(&surface.geometry, candidates, traversal)
                        })
                        .map(|(endpoints, offset)| {
                            (
                                line_pcurve(endpoints[0], endpoints[1]),
                                Some([0.0, 1.0]),
                                offset,
                                "native_endpoint_pcurve",
                            )
                        })
                        .or_else(|| {
                            native_candidates.is_none().then_some(())?;
                            let surface = ir.model.surfaces.iter().find(|candidate| {
                                candidate.id
                                    == SurfaceId(format!("creo:visibgeom:surface#{face_id}"))
                            })?;
                            let curve = ir.model.curves.iter().find(|candidate| {
                                candidate.id
                                    == CurveId(format!(
                                        "creo:visibgeom:curve#{}",
                                        half_edge.curve_id
                                    ))
                            })?;
                            let edge = ir.model.edges.iter().find(|candidate| {
                                candidate.id
                                    == EdgeId(format!("creo:visibgeom:edge#{}", half_edge.curve_id))
                            })?;
                            let (geometry, tag) =
                                planar_curve_pcurve(&surface.geometry, &curve.geometry)
                                    .map(|geometry| (geometry, "projected_planar_pcurve"))
                                    .or_else(|| {
                                        surface_of_revolution_parallel_pcurve(
                                            &surface.geometry,
                                            &curve.geometry,
                                        )
                                        .map(|geometry| {
                                            (geometry, "projected_parallel_conic_pcurve")
                                        })
                                    })
                                    .or_else(|| {
                                        meridian_circle_pcurve(&surface.geometry, &curve.geometry)
                                            .map(|geometry| (geometry, "projected_meridian_pcurve"))
                                    })
                                    .or_else(|| {
                                        ruled_generator_line_pcurve(
                                            &surface.geometry,
                                            &curve.geometry,
                                        )
                                        .map(|geometry| {
                                            (geometry, "projected_ruled_generator_pcurve")
                                        })
                                    })?;
                            Some((
                                geometry,
                                edge.param_range,
                                row_offsets.get(&half_edge.curve_id).copied().unwrap_or(0),
                                tag,
                            ))
                        });
                    let pcurves = pcurve_geometry
                        .map(|(geometry, parameter_range, offset, tag)| {
                            let pcurve = PcurveId(format!(
                                "creo:visibgeom:pcurve#{}:{face_id}",
                                half_edge.curve_id
                            ));
                            if !ir.model.pcurves.iter().any(|item| item.id == pcurve) {
                                annotate(
                                    annotations,
                                    &pcurve,
                                    "VisibGeom",
                                    offset as u64,
                                    tag,
                                    Exactness::Derived,
                                );
                                ir.model.pcurves.push(Pcurve {
                                    id: pcurve.clone(),
                                    geometry,
                                    wrapper_reversed: None,
                                    native_tail_flags: None,
                                    parameter_range,
                                    fit_tolerance: None,
                                });
                            }
                            PcurveUse {
                                pcurve,
                                isoparametric: None,
                                parameter_range: None,
                            }
                        })
                        .into_iter()
                        .collect();
                    ir.model.coedges.push(Coedge {
                        id,
                        owner_loop: loop_id.clone(),
                        edge: EdgeId(format!("creo:visibgeom:edge#{}", half_edge.curve_id)),
                        next: coedge_ids[(index + 1) % coedge_ids.len()].clone(),
                        previous: coedge_ids[(index + coedge_ids.len() - 1) % coedge_ids.len()]
                            .clone(),
                        radial_next,
                        sense: if half_edge.side == 0 {
                            Sense::Forward
                        } else {
                            Sense::Reversed
                        },
                        pcurves,
                        use_curve: None,
                        use_curve_parameter_range: None,
                    });
                }
            }
        }
    }
    (solved_point_count, emitted_curves.len())
}

fn transfer_cap_pair_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for pair in &scan.curves.fc05_cylinder_cap_pairs {
        let placed_caps = pair
            .cap_plane_ids
            .iter()
            .zip(&pair.curve_cap_ordinates_row_frame)
            .filter_map(|(id, ordinate)| {
                crate::surface::unique_outline_plane(&scan.planes.outlines, *id)
                    .map(|plane| (plane, *ordinate))
            })
            .collect::<Vec<_>>();
        let Some((first_cap, first_ordinate)) = placed_caps.first().copied() else {
            continue;
        };
        let Some(axis_index) = (0..3).find(|axis| first_cap.normal[*axis].abs() > 1.0 - 1e-9)
        else {
            continue;
        };
        if placed_caps
            .iter()
            .any(|(plane, _)| plane.normal != first_cap.normal)
        {
            continue;
        }
        let translations = placed_caps
            .iter()
            .map(|(plane, ordinate)| plane.origin[axis_index] - ordinate)
            .collect::<Vec<_>>();
        if translations
            .iter()
            .any(|translation| (translation - translations[0]).abs() > 1e-9)
        {
            continue;
        }
        let axis_origin = first_ordinate + translations[0];
        let axis_sign = -f64::from(pair.parameter_sign);
        let (origin, axis, ref_direction) = fc05_model_frame(
            axis_index,
            axis_origin,
            pair.center_row_frame,
            pair.reference_direction_row_frame,
            axis_sign,
        );
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", pair.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            pair.offset as u64,
            "fc05_cap_pair_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
                radius: pair.radius_mm,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", pair.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        for ((curve_id, ordinate), cap_plane_id) in pair
            .curve_ids
            .iter()
            .zip(&pair.curve_cap_ordinates_row_frame)
            .zip(&pair.cap_plane_ids)
        {
            let cap_offset =
                crate::surface::unique_outline_plane(&scan.planes.outlines, *cap_plane_id)
                    .map_or_else(
                        || ordinate + translations[0],
                        |plane| plane.origin[axis_index],
                    );
            let (center, _, _) = fc05_model_frame(
                axis_index,
                cap_offset,
                pair.center_row_frame,
                pair.reference_direction_row_frame,
                axis_sign,
            );
            let id = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
            if ir.model.curves.iter().any(|curve| curve.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "VisibGeom",
                scan.curves
                    .fc05_circles
                    .iter()
                    .find(|circle| circle.curve_id == *curve_id)
                    .map_or(pair.offset, |circle| circle.offset) as u64,
                "fc05_cap_circle",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry: CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(
                        ref_direction[0],
                        ref_direction[1],
                        ref_direction[2],
                    ),
                    radius: pair.radius_mm,
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{curve_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
    }
}

fn prototype_scalar(record: &crate::surface::SurfacePrototypeRecord, name: &str) -> Option<f64> {
    match &record.field(name)?.value {
        crate::surface::SurfaceNamedValue::ScalarSequence(values) if values.len() == 1 => {
            Some(values[0])
        }
        _ => None,
    }
}

fn prototype_vector_array(
    record: &crate::surface::SurfacePrototypeRecord,
    name: &str,
) -> Option<Vec<[f64; 3]>> {
    let crate::surface::SurfaceNamedValue::ScalarArray {
        dimensions,
        count: 3,
        values,
        ..
    } = &record.field(name)?.value
    else {
        return None;
    };
    let vector_count = usize::try_from(*dimensions).ok()?;
    (values.len() == vector_count.checked_mul(3)?).then_some(())?;
    values
        .chunks_exact(3)
        .map(|coordinates| Some([coordinates[0]?, coordinates[1]?, coordinates[2]?]))
        .collect()
}

fn prototype_parameter_array(
    record: &crate::surface::SurfacePrototypeRecord,
    name: &str,
) -> Option<Vec<f64>> {
    let crate::surface::SurfaceNamedValue::CountedScalarArray { count, values, .. } =
        &record.field(name)?.value
    else {
        return None;
    };
    (values.len() == usize::try_from(*count).ok()?).then_some(())?;
    values.iter().copied().collect()
}

fn prototype_spline_nurbs(record: &crate::surface::SurfacePrototypeRecord) -> Option<NurbsSurface> {
    interpolation_spline_surface(
        &prototype_vector_array(record, "i_points")?,
        &prototype_parameter_array(record, "u_params")?,
        &prototype_parameter_array(record, "v_params")?,
        &prototype_vector_array(record, "end_u_tangts")?,
        &prototype_vector_array(record, "end_v_tangts")?,
        &prototype_vector_array(record, "end_uv_deriv")?,
    )
}

fn prototype_local_frame(
    record: &crate::surface::SurfacePrototypeRecord,
) -> Option<([f64; 3], [f64; 3], [f64; 3])> {
    let crate::surface::SurfaceNamedValue::ScalarArray {
        dimensions: 4,
        count: 3,
        values,
        ..
    } = &record.field("local_sys")?.value
    else {
        return None;
    };
    let slots = values.iter().copied().collect::<Option<Vec<_>>>()?;
    let slots: [f64; 12] = slots.try_into().ok()?;
    let first: [f64; 3] = slots[0..3].try_into().ok()?;
    let middle: [f64; 3] = slots[3..6].try_into().ok()?;
    let third: [f64; 3] = slots[6..9].try_into().ok()?;
    let first_norm = dot(first, first).sqrt();
    let reference = normalized(first)?;
    let torus = matches!(record.family, crate::surface::SurfacePrototypeFamily::Torus);
    let mut second_candidates =
        [(middle, torus), (third, true)]
            .into_iter()
            .filter_map(|(candidate, eligible)| {
                let candidate_norm = dot(candidate, candidate).sqrt();
                let equal_scale =
                    (first_norm - candidate_norm).abs() <= 1e-10 * first_norm.max(candidate_norm);
                eligible
                    .then_some(())
                    .filter(|()| {
                        equal_scale && dot(reference, candidate).abs() <= 1e-10 * candidate_norm
                    })
                    .and_then(|()| normalized(candidate))
            });
    let second = second_candidates.next()?;
    second_candidates.next().is_none().then_some(())?;
    let axis = normalized(cross(reference, second))?;
    let origin = slots[9..12].try_into().ok()?;
    Some((origin, axis, reference))
}

#[cfg(test)]
mod prototype_local_frame_tests;

fn unique_surface_prototype_associations(
    scan: &ContainerScan,
) -> Vec<(
    &crate::surface::SurfacePrototypeRecord,
    &crate::surface::SurfaceRow,
    &crate::container::Section,
)> {
    let mut associations = Vec::new();
    for record in &scan.surfaces.prototype_records {
        let row_kind = match record.family {
            crate::surface::SurfacePrototypeFamily::Plane => crate::surface::SurfaceKind::Plane,
            crate::surface::SurfacePrototypeFamily::Cylinder => {
                crate::surface::SurfaceKind::Cylinder
            }
            crate::surface::SurfacePrototypeFamily::Torus => {
                crate::surface::SurfaceKind::TorusOrSphere
            }
            crate::surface::SurfacePrototypeFamily::Cone => crate::surface::SurfaceKind::Cone,
            crate::surface::SurfacePrototypeFamily::Spline => crate::surface::SurfaceKind::Spline,
            _ => continue,
        };
        let Some(section) = scan.framing.sections.iter().find(|section| {
            record.offset >= section.offset
                && record.offset < section.offset.saturating_add(section.length)
        }) else {
            continue;
        };
        let adjacent_rows = scan.surfaces.rows.iter().filter(|row| {
            row.offset >= section.offset
                && row.offset < section.offset.saturating_add(section.length)
        });
        let previous = adjacent_rows
            .clone()
            .filter(|row| row.offset < record.offset)
            .max_by_key(|row| row.offset);
        let following = adjacent_rows
            .filter(|row| row.offset > record.offset)
            .min_by_key(|row| row.offset);
        let previous = previous.filter(|row| row.kind == row_kind);
        let following = following.filter(|row| row.kind == row_kind);
        let Some(row) = previous.or(following) else {
            continue;
        };
        if crate::surface::unique_surface_row(&scan.surfaces.rows, row.id)
            .is_none_or(|unique| unique.offset != row.offset)
        {
            continue;
        }
        associations.push((record, row, section));
    }
    let mut association_counts = BTreeMap::<usize, usize>::new();
    for (_, row, _) in &associations {
        *association_counts.entry(row.offset).or_default() += 1;
    }
    associations
        .into_iter()
        .filter(|(_, row, _)| association_counts.get(&row.offset) == Some(&1))
        .collect()
}

fn transfer_first_instance_prototype_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    if scan.framing.layout != crate::container::Layout::Nd {
        return 0;
    }
    let mut transferred = 0;
    for (record, row, section) in unique_surface_prototype_associations(scan) {
        let geometry = match record.family {
            crate::surface::SurfacePrototypeFamily::Plane => {
                let Some((origin, axis, reference)) = prototype_local_frame(record) else {
                    continue;
                };
                SurfaceGeometry::Plane {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    normal: Vector3::new(axis[0], axis[1], axis[2]),
                    u_axis: Vector3::new(reference[0], reference[1], reference[2]),
                }
            }
            crate::surface::SurfacePrototypeFamily::Cylinder => {
                let Some((origin, axis, reference)) = prototype_local_frame(record) else {
                    continue;
                };
                let Some(radius) = prototype_scalar(record, "radius")
                    .filter(|radius| radius.is_finite() && *radius > 0.0)
                else {
                    continue;
                };
                SurfaceGeometry::Cylinder {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                }
            }
            crate::surface::SurfacePrototypeFamily::Torus => {
                let Some((origin, axis, reference)) = prototype_local_frame(record) else {
                    continue;
                };
                let point = Point3::new(origin[0], origin[1], origin[2]);
                let axis = Vector3::new(axis[0], axis[1], axis[2]);
                let reference = Vector3::new(reference[0], reference[1], reference[2]);
                let radii = match (
                    prototype_scalar(record, "radius1")
                        .filter(|radius| radius.is_finite() && *radius >= 0.0),
                    prototype_scalar(record, "radius2")
                        .filter(|radius| radius.is_finite() && *radius > 0.0),
                ) {
                    (Some(radius1), Some(radius2)) => Some([radius1, radius2]),
                    _ => None,
                };
                let Some([radius1, radius2]) = radii else {
                    continue;
                };
                if radius1 == 0.0 {
                    SurfaceGeometry::Sphere {
                        center: point,
                        axis,
                        ref_direction: reference,
                        radius: radius2,
                    }
                } else {
                    SurfaceGeometry::Torus {
                        center: point,
                        axis,
                        ref_direction: reference,
                        major_radius: radius1,
                        minor_radius: radius2,
                    }
                }
            }
            crate::surface::SurfacePrototypeFamily::Cone => {
                let Some(frame) = crate::surface::prototype_cone_frame(record) else {
                    continue;
                };
                SurfaceGeometry::Cone {
                    origin: Point3::new(frame.apex[0], frame.apex[1], frame.apex[2]),
                    axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                    ref_direction: Vector3::new(
                        frame.ref_direction[0],
                        frame.ref_direction[1],
                        frame.ref_direction[2],
                    ),
                    radius: 0.0,
                    ratio: 1.0,
                    half_angle: frame.half_angle,
                }
            }
            crate::surface::SurfacePrototypeFamily::Spline => {
                let Some(nurbs) = prototype_spline_nurbs(record) else {
                    continue;
                };
                SurfaceGeometry::Nurbs(nurbs)
            }
            _ => unreachable!("prototype family was filtered above"),
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            &section.name,
            record.offset as u64,
            "first_instance_surface_prototype",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("{}:{}", section.name, row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

fn transfer_paired_envelope_spheres(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    if scan.framing.layout != crate::container::Layout::Nd {
        return 0;
    }
    let mut transferred = 0;
    for (prototype, associated_row, section) in unique_surface_prototype_associations(scan) {
        if prototype.family != crate::surface::SurfacePrototypeFamily::Torus
            || prototype_scalar(prototype, "radius1") != Some(0.0)
        {
            continue;
        }
        let Some(radius) = prototype_scalar(prototype, "radius2")
            .filter(|radius| radius.is_finite() && *radius > 0.0)
        else {
            continue;
        };
        let rows = scan
            .surfaces
            .rows
            .iter()
            .filter(|row| {
                row.feature_id == associated_row.feature_id
                    && row.kind == crate::surface::SurfaceKind::TorusOrSphere
            })
            .collect::<Vec<_>>();
        let [first_row, second_row] = rows.as_slice() else {
            continue;
        };
        let envelopes = [first_row, second_row].map(|row| {
            unique_surface_parameter_record(scan, row)?
                .type26_five_coordinate_envelope(row.type_byte)
        });
        let [Some(first_envelope), Some(second_envelope)] = envelopes else {
            continue;
        };
        let Some(center) =
            paired_five_coordinate_sphere_center([first_envelope, second_envelope], radius)
        else {
            continue;
        };
        for row in rows {
            let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                &section.name,
                row.offset as u64,
                "paired_type26_sphere_envelope",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: SurfaceGeometry::Sphere {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius,
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("{}:{}", section.name, row.id),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

fn transfer_positional_tori(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
        else {
            continue;
        };
        if row.kind != crate::surface::SurfaceKind::TorusOrSphere
            || crate::surface::unique_surface_parameter(
                &scan.surfaces.parameters,
                record.surface_id,
            )
            .is_none_or(|unique| unique.offset != record.offset)
        {
            continue;
        }
        let Some(frame) = record.positional_torus_frame else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        let Some(section) = scan.framing.sections.iter().find(|section| {
            row.offset >= section.offset
                && row.offset < section.offset.saturating_add(section.length)
        }) else {
            continue;
        };
        annotate(
            annotations,
            &id,
            &section.name,
            row.offset as u64,
            "positional_torus_frame",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Torus {
                center: Point3::new(frame.center[0], frame.center[1], frame.center[2]),
                axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                ref_direction: Vector3::new(
                    frame.ref_direction[0],
                    frame.ref_direction[1],
                    frame.ref_direction[2],
                ),
                major_radius: frame.major_radius,
                minor_radius: frame.minor_radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("{}:{}", section.name, row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

fn transfer_positional_line_extrusion_planes(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let replay_bound_surfaces = scan
        .curves
        .tabulated_cylinder_replays
        .iter()
        .map(|replay| replay.surface_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        if replay_bound_surfaces.contains(&record.surface_id) {
            continue;
        }
        if crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id)
            .is_none_or(|unique| unique.offset != record.offset)
        {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
        else {
            continue;
        };
        if row.kind != crate::surface::SurfaceKind::Extrusion {
            continue;
        }
        let type_byte = row.type_byte;
        let Some(frame) = record.line_extrusion_frame(type_byte) else {
            continue;
        };
        let directrix =
            std::array::from_fn(|axis| frame.directrix[1][axis] - frame.directrix[0][axis]);
        let (Some(_direction), Some(u_axis), Some(normal)) = (
            normalized(frame.direction),
            normalized(directrix),
            normalized(cross(directrix, frame.direction)),
        ) else {
            continue;
        };
        let surface_id = SurfaceId(format!("creo:visibgeom:surface#{}", record.surface_id));
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        let curve_id = CurveId(format!(
            "creo:visibgeom:surface_directrix#{}",
            record.surface_id
        ));
        let procedural_id = ProceduralSurfaceId(format!(
            "creo:visibgeom:surface_extrusion#{}",
            record.surface_id
        ));
        annotate(
            annotations,
            &curve_id,
            "VisibGeom",
            record.body_offset as u64,
            "positional_line_extrusion_directrix",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &surface_id,
            "VisibGeom",
            record.body_offset as u64,
            "positional_line_extrusion_plane",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &procedural_id,
            "VisibGeom",
            record.body_offset as u64,
            "positional_line_extrusion_construction",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Line {
                origin: Point3::new(
                    frame.directrix[0][0],
                    frame.directrix[0][1],
                    frame.directrix[0][2],
                ),
                direction: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:surface_directrix#{}", record.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(
                    frame.directrix[0][0],
                    frame.directrix[0][1],
                    frame.directrix[0][2],
                ),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", record.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Extrusion {
                directrix: curve_id,
                parameter_interval: None,
                direction: Vector3::new(frame.direction[0], frame.direction[1], frame.direction[2]),
                native_position: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        transferred += 1;
    }
    transferred
}

fn transfer_tabulated_cylinder_spline_extrusions(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut replay_counts = BTreeMap::<u32, usize>::new();
    for replay in &scan.curves.tabulated_cylinder_replays {
        *replay_counts.entry(replay.surface_id).or_default() += 1;
    }
    let mut transferred = 0;
    for replay in &scan.curves.tabulated_cylinder_replays {
        if replay_counts.get(&replay.surface_id) != Some(&1) {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, replay.surface_id)
        else {
            continue;
        };
        if row.type_byte != 0x2c || row.offset != replay.surface_row_offset {
            continue;
        }
        let Some(parameters) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, replay.surface_id)
        else {
            continue;
        };
        let Some((directrix, sweep)) = placed_tabulated_cylinder_directrix(replay, parameters)
        else {
            continue;
        };
        let Some(surface) = extruded_nurbs_surface(&directrix, sweep) else {
            continue;
        };
        let curve_id = CurveId(format!(
            "creo:visibgeom:tabulated_directrix#{}",
            replay.surface_id
        ));
        let surface_id = SurfaceId(format!("creo:visibgeom:surface#{}", replay.surface_id));
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        let procedural_id = ProceduralSurfaceId(format!(
            "creo:visibgeom:tabulated_extrusion#{}",
            replay.surface_id
        ));
        annotate(
            annotations,
            &curve_id,
            "VisibGeom",
            replay.offset as u64,
            "tabulated_cylinder_directrix",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &surface_id,
            "VisibGeom",
            replay.surface_row_offset as u64,
            "tabulated_cylinder_surface",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &procedural_id,
            "VisibGeom",
            replay.surface_row_offset as u64,
            "tabulated_cylinder_extrusion",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Nurbs(directrix),
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:curve#{}", replay.curve_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Nurbs(surface),
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", replay.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Extrusion {
                directrix: curve_id,
                parameter_interval: Some([0.0, 1.0]),
                direction: Vector3::new(sweep[0], sweep[1], sweep[2]),
                native_position: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        transferred += 1;
    }
    transferred
}

fn transfer_part_product(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> bool {
    let Some(model_name) = scan.framing.model_name.as_ref() else {
        return false;
    };
    let Some(model_name_offset) = scan.framing.model_name_offset else {
        return false;
    };
    let product_id = ProductId("creo:model:product#root".to_string());
    let occurrence_id = OccurrenceId("creo:model:product_occurrence#root".to_string());
    annotate(
        annotations,
        &product_id,
        "archive_header",
        model_name_offset as u64,
        "part_product",
        Exactness::Derived,
    );
    annotate(
        annotations,
        &occurrence_id,
        "archive_header",
        model_name_offset as u64,
        "part_product_occurrence",
        Exactness::Derived,
    );
    ir.model.products.push(Product {
        id: product_id.clone(),
        product_id: model_name.clone(),
        name: Some(model_name.clone()),
        bodies: ir.model.bodies.iter().map(|body| body.id.clone()).collect(),
    });
    ir.model.product_occurrences.push(ProductOccurrence {
        id: occurrence_id,
        product: product_id,
        parent: OccurrenceParent::Root,
        transform: Transform::identity(),
        name: Some(model_name.clone()),
    });
    true
}

fn fc05_model_frame(
    axis_index: usize,
    axis_ordinate: f64,
    center_row_frame: [f64; 2],
    reference_row_frame: [f64; 2],
    axis_sign: f64,
) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let [first, second] = center_row_frame;
    let [reference_x, reference_z] = reference_row_frame;
    match axis_index {
        0 => (
            [axis_ordinate, second, first],
            [axis_sign, 0.0, 0.0],
            [0.0, reference_z, reference_x],
        ),
        1 => (
            [first, axis_ordinate, second],
            [0.0, axis_sign, 0.0],
            [reference_x, 0.0, reference_z],
        ),
        2 => (
            [second, first, axis_ordinate],
            [0.0, 0.0, axis_sign],
            [reference_z, reference_x, 0.0],
        ),
        _ => unreachable!("model-space axis index is bounded by XYZ"),
    }
}

fn transfer_fc05_cap_circles(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for circle in &scan.curves.fc05_circles {
        let topology = scan
            .curves
            .topology_rows
            .iter()
            .filter(|row| row.id == circle.curve_id)
            .collect::<Vec<_>>();
        let [topology] = topology.as_slice() else {
            continue;
        };
        let cap_planes = topology
            .faces
            .iter()
            .filter_map(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, *face)
                    .filter(|row| row.kind == crate::surface::SurfaceKind::Plane)?;
                crate::surface::unique_outline_plane(&scan.planes.outlines, *face)
            })
            .collect::<Vec<_>>();
        let cylinders = topology
            .faces
            .iter()
            .filter(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, **face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
            })
            .copied()
            .collect::<Vec<_>>();
        let ([cap], [cylinder_id], Some(_)) = (
            cap_planes.as_slice(),
            cylinders.as_slice(),
            circle.cap_ordinate_row_frame,
        ) else {
            continue;
        };
        let Some(axis_index) = (0..3).find(|axis| cap.normal[*axis].abs() > 1.0 - 1e-9) else {
            continue;
        };
        let [first, second] = circle.center_row_frame;
        let (reference, axis_sign) = circle
            .reference_direction_row_frame
            .zip(circle.parameter_sign)
            .map_or(
                (
                    circle.sample_direction_row_frame,
                    cap.normal[axis_index].signum(),
                ),
                |(reference, parameter_sign)| (reference, -f64::from(parameter_sign)),
            );
        let (center, axis, ref_direction) = fc05_model_frame(
            axis_index,
            cap.origin[axis_index],
            [first, second],
            reference,
            axis_sign,
        );
        let id = CurveId(format!("creo:visibgeom:curve#{}", circle.curve_id));
        if !ir.model.curves.iter().any(|curve| curve.id == id) {
            annotate(
                annotations,
                &id,
                "VisibGeom",
                circle.offset as u64,
                "fc05_cap_circle",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry: CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(
                        ref_direction[0],
                        ref_direction[1],
                        ref_direction[2],
                    ),
                    radius: circle.radius_mm,
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{}", circle.curve_id),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        let surface_id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        annotate(
            annotations,
            &surface_id,
            "VisibGeom",
            circle.offset as u64,
            "fc05_axis_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id: surface_id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
                radius: circle.radius_mm,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{cylinder_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
}

fn carrier_intersection_curve(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Option<(CurveGeometry, &'static str)> {
    match (first, second) {
        (CarrierEquation::Plane(first), CarrierEquation::Plane(second)) => {
            let direction = cross(first.normal, second.normal);
            let denominator = dot(direction, direction);
            if denominator <= 1e-18 {
                return None;
            }
            let first_distance = dot(first.normal, first.origin);
            let second_distance = dot(second.normal, second.origin);
            let weighted = [0, 1, 2].map(|axis| {
                first_distance * second.normal[axis] - second_distance * first.normal[axis]
            });
            let point_numerator = cross(weighted, direction);
            let origin = point_numerator.map(|value| value / denominator);
            let direction = normalized(direction)?;
            Some((
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(direction[0], direction[1], direction[2]),
                },
                "plane_intersection_line",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Cylinder(cylinder))
        | (CarrierEquation::Cylinder(cylinder), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let axis = normalized(cylinder.axis)?;
            let cosine = dot(normal, axis);
            if cosine.abs() <= 1e-10 {
                let signed_distance = dot(
                    normal,
                    std::array::from_fn(|index| cylinder.origin[index] - plane.origin[index]),
                );
                let scale = cylinder.radius.max(1.0);
                if (signed_distance.abs() - cylinder.radius).abs() > 1e-9 * scale {
                    return None;
                }
                let origin: [f64; 3] = std::array::from_fn(|index| {
                    cylinder.origin[index] - signed_distance * normal[index]
                });
                return Some((
                    CurveGeometry::Line {
                        origin: Point3::new(origin[0], origin[1], origin[2]),
                        direction: Vector3::new(axis[0], axis[1], axis[2]),
                    },
                    "plane_cylinder_tangent_line",
                ));
            }
            let axis_parameter = dot(
                normal,
                std::array::from_fn(|index| plane.origin[index] - cylinder.origin[index]),
            ) / cosine;
            let center: [f64; 3] =
                std::array::from_fn(|index| cylinder.origin[index] + axis_parameter * axis[index]);
            if (cosine.abs() - 1.0).abs() <= 1e-10 {
                let reference = normalized(cylinder.ref_direction)?;
                return Some((
                    CurveGeometry::Circle {
                        center: Point3::new(center[0], center[1], center[2]),
                        axis: Vector3::new(normal[0], normal[1], normal[2]),
                        ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                        radius: cylinder.radius,
                    },
                    "plane_cylinder_circle",
                ));
            }
            let projected_axis = normalized(std::array::from_fn(|index| {
                axis[index] - cosine * normal[index]
            }))?;
            Some((
                CurveGeometry::Ellipse {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    major_direction: Vector3::new(
                        projected_axis[0],
                        projected_axis[1],
                        projected_axis[2],
                    ),
                    major_radius: cylinder.radius / cosine.abs(),
                    minor_radius: cylinder.radius,
                },
                "plane_cylinder_ellipse",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let signed_distance = dot(
                normal,
                std::array::from_fn(|index| sphere.center[index] - plane.origin[index]),
            );
            let radius_squared = sphere
                .radius
                .mul_add(sphere.radius, -(signed_distance * signed_distance));
            let scale = sphere.radius.max(1.0);
            if radius_squared <= 1e-18 * scale * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - signed_distance * normal[index]);
            let reference = normalized(std::array::from_fn(|index| {
                sphere.ref_direction[index] - dot(sphere.ref_direction, normal) * normal[index]
            }))
            .unwrap_or_else(|| {
                let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                    normal[0], normal[1], normal[2],
                ));
                [reference.x, reference.y, reference.z]
            });
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: radius_squared.sqrt(),
                },
                "plane_sphere_circle",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Cone(cone))
        | (CarrierEquation::Cone(cone), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let axis = normalized(cone.axis)?;
            let alignment = dot(normal, axis);
            let slope = cone.half_angle.tan();
            if circular_cone(cone) && slope.abs() > 1e-12 {
                let apex: [f64; 3] = std::array::from_fn(|index| {
                    cone.origin[index] - (cone.radius / slope) * axis[index]
                });
                let plane_distance = dot(
                    normal,
                    std::array::from_fn(|index| apex[index] - plane.origin[index]),
                );
                let scale = cone.radius.max(1.0);
                if plane_distance.abs() <= 1e-9 * scale
                    && (alignment.abs() - cone.half_angle.sin()).abs() <= 1e-10
                {
                    let direction = normalized(std::array::from_fn(|index| {
                        axis[index] - alignment * normal[index]
                    }))?;
                    return Some((
                        CurveGeometry::Line {
                            origin: Point3::new(apex[0], apex[1], apex[2]),
                            direction: Vector3::new(direction[0], direction[1], direction[2]),
                        },
                        "plane_cone_tangent_line",
                    ));
                }
            }
            let apex_generators = apex_plane_cone_generator_candidates(
                CarrierEquation::Plane(plane),
                CarrierEquation::Cone(cone),
            );
            if apex_generators.len() == 1 {
                return apex_generators.into_iter().next();
            }
            if (alignment.abs() - 1.0).abs() <= 1e-10 {
                let axial = dot(
                    axis,
                    std::array::from_fn(|index| plane.origin[index] - cone.origin[index]),
                );
                let radius = (cone.radius + axial * cone.half_angle.tan()).abs();
                if radius <= 1e-12 {
                    return None;
                }
                let center: [f64; 3] =
                    std::array::from_fn(|index| cone.origin[index] + axial * axis[index]);
                let reference = normalized(cone.ref_direction)?;
                let (geometry, tag) = if circular_cone(cone) {
                    (
                        CurveGeometry::Circle {
                            center: Point3::new(center[0], center[1], center[2]),
                            axis: Vector3::new(normal[0], normal[1], normal[2]),
                            ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                            radius,
                        },
                        "plane_cone_circle",
                    )
                } else {
                    (
                        CurveGeometry::Ellipse {
                            center: Point3::new(center[0], center[1], center[2]),
                            axis: Vector3::new(normal[0], normal[1], normal[2]),
                            major_direction: Vector3::new(reference[0], reference[1], reference[2]),
                            major_radius: radius,
                            minor_radius: radius * cone.ratio,
                        },
                        "plane_cone_parallel_ellipse",
                    )
                };
                return Some((geometry, tag));
            }
            plane_cone_conic(plane, cone)
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let axis = normalized(torus.axis)?;
            if (dot(normal, axis).abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let axial = dot(
                axis,
                std::array::from_fn(|index| plane.origin[index] - torus.center[index]),
            );
            let scale = torus.minor_radius.max(torus.major_radius).max(1.0);
            if (axial.abs() - torus.minor_radius).abs() > 1e-9 * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] + axial * axis[index]);
            let reference = normalized(torus.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: torus.major_radius,
                },
                "plane_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Cylinder(first), CarrierEquation::Cylinder(second)) => {
            let first_axis = normalized(first.axis)?;
            let second_axis = normalized(second.axis)?;
            let alignment = dot(first_axis, second_axis);
            if (alignment.abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let relative = std::array::from_fn(|index| second.origin[index] - first.origin[index]);
            let axial = dot(relative, first_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
            let distance = dot(transverse, transverse).sqrt();
            if distance <= 1e-12 {
                return None;
            }
            let external = first.radius + second.radius;
            let internal = (first.radius - second.radius).abs();
            let scale = external.max(distance).max(1.0);
            let first_fraction = if (distance - external).abs() <= 1e-9 * scale {
                first.radius / distance
            } else if (distance - internal).abs() <= 1e-9 * scale {
                let signed = if first.radius >= second.radius {
                    first.radius
                } else {
                    -first.radius
                };
                signed / distance
            } else {
                return None;
            };
            let origin: [f64; 3] = std::array::from_fn(|index| {
                first.origin[index] + first_fraction * transverse[index]
            });
            Some((
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                },
                "parallel_cylinder_tangent_line",
            ))
        }
        (CarrierEquation::Sphere(first), CarrierEquation::Sphere(second)) => {
            let center_delta: [f64; 3] =
                std::array::from_fn(|index| second.center[index] - first.center[index]);
            let distance = dot(center_delta, center_delta).sqrt();
            if distance <= 1e-12
                || distance >= first.radius + second.radius
                || distance <= (first.radius - second.radius).abs()
            {
                return None;
            }
            let axis = center_delta.map(|value| value / distance);
            let axial = (distance * distance + first.radius * first.radius
                - second.radius * second.radius)
                / (2.0 * distance);
            let radius_squared = first.radius.mul_add(first.radius, -(axial * axial));
            if radius_squared <= 1e-18 {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| first.center[index] + axial * axis[index]);
            let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                axis[0], axis[1], axis[2],
            ));
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: reference,
                    radius: radius_squared.sqrt(),
                },
                "sphere_intersection_circle",
            ))
        }
        (CarrierEquation::Cylinder(cylinder), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Cylinder(cylinder)) => {
            let axis = normalized(cylinder.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - cylinder.origin[index]);
            let axial = dot(relative, axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let scale = sphere.radius.max(cylinder.radius).max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale
                || (sphere.radius - cylinder.radius).abs() > 1e-9 * scale
            {
                return None;
            }
            let reference = normalized(cylinder.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(sphere.center[0], sphere.center[1], sphere.center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_sphere_circle",
            ))
        }
        (CarrierEquation::Cylinder(cylinder), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Cylinder(cylinder)) => {
            let cylinder_axis = normalized(cylinder.axis)?;
            let torus_axis = normalized(torus.axis)?;
            if (dot(cylinder_axis, torus_axis).abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let relative: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] - cylinder.origin[index]);
            let axial = dot(relative, cylinder_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * cylinder_axis[index]);
            let scale = torus
                .major_radius
                .max(torus.minor_radius)
                .max(cylinder.radius)
                .max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale {
                return None;
            }
            let outer_radius = torus.major_radius + torus.minor_radius;
            let inner_radius = (torus.major_radius - torus.minor_radius).abs();
            if (cylinder.radius - outer_radius).abs() > 1e-9 * scale
                && (inner_radius <= 1e-12 || (cylinder.radius - inner_radius).abs() > 1e-9 * scale)
            {
                return None;
            }
            let reference = normalized(cylinder.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(torus.center[0], torus.center[1], torus.center[2]),
                    axis: Vector3::new(cylinder_axis[0], cylinder_axis[1], cylinder_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Cone(cone), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Cone(cone)) => {
            if !circular_cone(cone) {
                return None;
            }
            let cone_axis = normalized(cone.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - cone.origin[index]);
            let axial = dot(relative, cone_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * cone_axis[index]);
            let scale = cone.radius.max(sphere.radius).max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale {
                return None;
            }
            let slope = cone.half_angle.tan();
            if slope.abs() <= 1e-12 {
                return None;
            }
            let quadratic = 1.0 + slope * slope;
            let linear = 2.0 * (cone.radius * slope - axial);
            let constant =
                cone.radius * cone.radius + axial * axial - sphere.radius * sphere.radius;
            let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
            let discriminant_scale = linear
                .abs()
                .max((4.0 * quadratic * constant).abs().sqrt())
                .max(1.0);
            if discriminant.abs() > 1e-9 * discriminant_scale * discriminant_scale {
                return None;
            }
            let cone_parameter = -linear / (2.0 * quadratic);
            let radius = (cone.radius + cone_parameter * slope).abs();
            if radius <= 1e-12 * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + cone_parameter * cone_axis[index]);
            let reference = normalized(cone.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(cone_axis[0], cone_axis[1], cone_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_cone_sphere_tangent_circle",
            ))
        }
        (CarrierEquation::Sphere(sphere), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Sphere(sphere)) => {
            let axis = normalized(torus.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] - sphere.center[index]);
            let axial = dot(relative, axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let scale = torus
                .major_radius
                .max(torus.minor_radius)
                .max(sphere.radius)
                .max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale {
                return None;
            }
            let meridian_distance = torus.major_radius.hypot(axial);
            if meridian_distance <= 1e-12 {
                return None;
            }
            let external = sphere.radius + torus.minor_radius;
            let internal = (sphere.radius - torus.minor_radius).abs();
            if (meridian_distance - external).abs() > 1e-9 * scale
                && (meridian_distance - internal).abs() > 1e-9 * scale
            {
                return None;
            }
            let sphere_parameter = (meridian_distance * meridian_distance
                + sphere.radius * sphere.radius
                - torus.minor_radius * torus.minor_radius)
                / (2.0 * meridian_distance);
            let radius = (sphere_parameter * torus.major_radius / meridian_distance).abs();
            if radius <= 1e-12 * scale {
                return None;
            }
            let center_axial = sphere_parameter * axial / meridian_distance;
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] + center_axial * axis[index]);
            let reference = normalized(torus.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_sphere_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Torus(first), CarrierEquation::Torus(second)) => {
            let first_axis = normalized(first.axis)?;
            let second_axis = normalized(second.axis)?;
            if (dot(first_axis, second_axis).abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let relative: [f64; 3] =
                std::array::from_fn(|index| second.center[index] - first.center[index]);
            let axial = dot(relative, first_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
            let scale = first
                .major_radius
                .max(first.minor_radius)
                .max(second.major_radius)
                .max(second.minor_radius)
                .max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale {
                return None;
            }
            let radial_delta = second.major_radius - first.major_radius;
            let meridian_distance = radial_delta.hypot(axial);
            if meridian_distance <= 1e-12 {
                return None;
            }
            let external = first.minor_radius + second.minor_radius;
            let internal = (first.minor_radius - second.minor_radius).abs();
            if (meridian_distance - external).abs() > 1e-9 * scale
                && (meridian_distance - internal).abs() > 1e-9 * scale
            {
                return None;
            }
            let first_parameter = (meridian_distance * meridian_distance
                + first.minor_radius * first.minor_radius
                - second.minor_radius * second.minor_radius)
                / (2.0 * meridian_distance);
            let radius =
                (first.major_radius + first_parameter * radial_delta / meridian_distance).abs();
            if radius <= 1e-12 * scale {
                return None;
            }
            let center_axial = first_parameter * axial / meridian_distance;
            let center: [f64; 3] =
                std::array::from_fn(|index| first.center[index] + center_axial * first_axis[index]);
            let reference = normalized(first.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_tori_tangent_circle",
            ))
        }
        (
            CarrierEquation::Cone(_),
            CarrierEquation::Cylinder(_) | CarrierEquation::Cone(_) | CarrierEquation::Torus(_),
        )
        | (CarrierEquation::Cylinder(_) | CarrierEquation::Torus(_), CarrierEquation::Cone(_)) => {
            None
        }
    }
}

fn parallel_plane_cylinder_generator_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Plane(plane), CarrierEquation::Cylinder(cylinder))
    | (CarrierEquation::Cylinder(cylinder), CarrierEquation::Plane(plane))) = (first, second)
    else {
        return Vec::new();
    };
    let Some(normal) = normalized(plane.normal) else {
        return Vec::new();
    };
    let Some(axis) = normalized(cylinder.axis) else {
        return Vec::new();
    };
    if dot(normal, axis).abs() > 1e-10 || cylinder.radius <= 0.0 {
        return Vec::new();
    }
    let signed_distance = dot(
        normal,
        std::array::from_fn(|index| cylinder.origin[index] - plane.origin[index]),
    );
    let scale = cylinder.radius.max(1.0);
    let offset_squared = cylinder
        .radius
        .mul_add(cylinder.radius, -(signed_distance * signed_distance));
    if offset_squared <= 1e-18 * scale * scale {
        return Vec::new();
    }
    let closest: [f64; 3] =
        std::array::from_fn(|index| cylinder.origin[index] - signed_distance * normal[index]);
    let Some(transverse) = normalized(cross(axis, normal)) else {
        return Vec::new();
    };
    let offset = offset_squared.sqrt();
    [-1.0, 1.0]
        .into_iter()
        .map(|sense| {
            let origin: [f64; 3] =
                std::array::from_fn(|index| closest[index] + sense * offset * transverse[index]);
            (
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(axis[0], axis[1], axis[2]),
                },
                "plane_cylinder_secant_generator",
            )
        })
        .collect()
}

fn parallel_cylinder_generator_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let (CarrierEquation::Cylinder(first), CarrierEquation::Cylinder(second)) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(first_axis), Some(second_axis)) = (normalized(first.axis), normalized(second.axis))
    else {
        return Vec::new();
    };
    if (dot(first_axis, second_axis).abs() - 1.0).abs() > 1e-10
        || first.radius <= 0.0
        || second.radius <= 0.0
    {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| second.origin[index] - first.origin[index]);
    let axial = dot(relative, first_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
    let distance = dot(transverse, transverse).sqrt();
    let scale = first.radius.max(second.radius).max(distance).max(1.0);
    if distance <= 1e-12 * scale
        || distance >= first.radius + second.radius - 1e-9 * scale
        || distance <= (first.radius - second.radius).abs() + 1e-9 * scale
    {
        return Vec::new();
    }
    let center_direction = transverse.map(|value| value / distance);
    let along = (first.radius * first.radius - second.radius * second.radius + distance * distance)
        / (2.0 * distance);
    let height_squared = first.radius.mul_add(first.radius, -(along * along));
    if height_squared <= 1e-12 * scale * scale {
        return Vec::new();
    }
    let Some(perpendicular) = normalized(cross(first_axis, center_direction)) else {
        return Vec::new();
    };
    let base: [f64; 3] =
        std::array::from_fn(|index| first.origin[index] + along * center_direction[index]);
    let height = height_squared.sqrt();
    [-height, height]
        .into_iter()
        .map(|offset| {
            let origin: [f64; 3] =
                std::array::from_fn(|index| base[index] + offset * perpendicular[index]);
            (
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                },
                "parallel_cylinder_secant_generator",
            )
        })
        .collect()
}

fn coaxial_cylinder_sphere_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cylinder(cylinder), CarrierEquation::Sphere(sphere))
    | (CarrierEquation::Sphere(sphere), CarrierEquation::Cylinder(cylinder))) = (first, second)
    else {
        return Vec::new();
    };
    let Some(axis) = normalized(cylinder.axis) else {
        return Vec::new();
    };
    let relative: [f64; 3] =
        std::array::from_fn(|index| sphere.center[index] - cylinder.origin[index]);
    let axial = dot(relative, axis);
    let transverse: [f64; 3] = std::array::from_fn(|index| relative[index] - axial * axis[index]);
    let scale = sphere.radius.max(cylinder.radius).max(1.0);
    if sphere.radius <= 0.0
        || cylinder.radius <= 0.0
        || dot(transverse, transverse).sqrt() > 1e-9 * scale
    {
        return Vec::new();
    }
    let offset_squared = sphere
        .radius
        .mul_add(sphere.radius, -(cylinder.radius * cylinder.radius));
    if offset_squared <= 1e-9 * scale * scale {
        return Vec::new();
    }
    let Some(reference) = normalized(cylinder.ref_direction) else {
        return Vec::new();
    };
    let offset = offset_squared.sqrt();
    [-offset, offset]
        .into_iter()
        .map(|offset| {
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] + offset * axis[index]);
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_sphere_secant_circle",
            )
        })
        .collect()
}

fn coaxial_cone_cylinder_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cone(cone), CarrierEquation::Cylinder(cylinder))
    | (CarrierEquation::Cylinder(cylinder), CarrierEquation::Cone(cone))) = (first, second)
    else {
        return Vec::new();
    };
    if !circular_cone(cone) {
        return Vec::new();
    }
    let (Some(cone_axis), Some(cylinder_axis), Some(reference)) = (
        normalized(cone.axis),
        normalized(cylinder.axis),
        normalized(cone.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(cone_axis, cylinder_axis).abs() - 1.0).abs() > 1e-10 {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| cylinder.origin[index] - cone.origin[index]);
    let axial = dot(relative, cone_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * cone_axis[index]);
    let scale = cone.radius.max(cylinder.radius).max(1.0);
    let slope = cone.half_angle.tan();
    if dot(transverse, transverse).sqrt() > 1e-9 * scale
        || cylinder.radius <= 1e-12 * scale
        || cone.radius < 0.0
        || slope.abs() <= 1e-12
        || !slope.is_finite()
    {
        return Vec::new();
    }
    [cylinder.radius, -cylinder.radius]
        .into_iter()
        .map(|signed_radius| {
            let parameter = (signed_radius - cone.radius) / slope;
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + parameter * cone_axis[index]);
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(cone_axis[0], cone_axis[1], cone_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cone_cylinder_secant_circle",
            )
        })
        .collect()
}

fn coaxial_cones_section_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let (CarrierEquation::Cone(first), CarrierEquation::Cone(second)) = (first, second) else {
        return Vec::new();
    };
    if first.ratio <= 0.0
        || second.ratio <= 0.0
        || !first.ratio.is_finite()
        || !second.ratio.is_finite()
    {
        return Vec::new();
    }
    let (Some(first_axis), Some(second_axis), Some(reference), Some(second_reference)) = (
        normalized(first.axis),
        normalized(second.axis),
        normalized(first.ref_direction),
        normalized(second.ref_direction),
    ) else {
        return Vec::new();
    };
    let axis_alignment = dot(first_axis, second_axis);
    if (axis_alignment.abs() - 1.0).abs() > 1e-10
        || dot(first_axis, reference).abs() > 1e-10
        || dot(second_axis, second_reference).abs() > 1e-10
    {
        return Vec::new();
    }
    let first_y = cross(first_axis, reference);
    let second_y = cross(second_axis, second_reference);
    let second_metric = |direction: [f64; 3]| {
        let x = dot(direction, second_reference);
        let y = dot(direction, second_y) / second.ratio;
        x.mul_add(x, y * y)
    };
    let metric_xx = second_metric(reference);
    let metric_yy = second_metric(first_y);
    let metric_xy = dot(reference, second_reference).mul_add(
        dot(first_y, second_reference),
        dot(reference, second_y) * dot(first_y, second_y) / (second.ratio * second.ratio),
    );
    let metric_scale_squared = metric_xx;
    let metric_coefficient_scale = metric_xx.abs().max(metric_yy.abs()).max(1.0);
    if metric_scale_squared <= 0.0
        || !metric_scale_squared.is_finite()
        || !metric_yy.is_finite()
        || !metric_xy.is_finite()
        || metric_xy.abs() > 1e-10 * metric_coefficient_scale
        || (metric_yy - metric_scale_squared / (first.ratio * first.ratio)).abs()
            > 1e-10 * metric_coefficient_scale
    {
        return Vec::new();
    }
    let metric_scale = metric_scale_squared.sqrt();
    let relative: [f64; 3] =
        std::array::from_fn(|index| second.origin[index] - first.origin[index]);
    let second_origin_axial = dot(relative, first_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - second_origin_axial * first_axis[index]);
    let scale = first.radius.max(second.radius).max(1.0);
    let first_slope = first.half_angle.tan();
    let second_slope = axis_alignment * second.half_angle.tan();
    let second_intercept = second.radius - second_slope * second_origin_axial;
    if dot(transverse, transverse).sqrt() > 1e-9 * scale
        || first.radius < 0.0
        || second.radius < 0.0
        || first_slope.abs() <= 1e-12
        || second_slope.abs() <= 1e-12
        || !first_slope.is_finite()
        || !second_slope.is_finite()
    {
        return Vec::new();
    }

    let mut parameters = Vec::<f64>::new();
    let scaled_first_slope = metric_scale * first_slope;
    let scaled_first_radius = metric_scale * first.radius;
    let slope_scale = scaled_first_slope.abs().max(second_slope.abs()).max(1.0);
    let intercept_scale = first
        .radius
        .max(scaled_first_radius.abs())
        .max(second_intercept.abs())
        .max(second.radius)
        .max(1.0);
    for radial_sense in [-1.0, 1.0] {
        let denominator = scaled_first_slope - radial_sense * second_slope;
        let numerator = radial_sense * second_intercept - scaled_first_radius;
        if denominator.abs() <= 1e-12 * slope_scale {
            if numerator.abs() <= 1e-9 * intercept_scale {
                return Vec::new();
            }
            continue;
        }
        let parameter = numerator / denominator;
        let radius = (first.radius + parameter * first_slope).abs();
        if radius <= 1e-12 * scale {
            continue;
        }
        if !parameters
            .iter()
            .any(|known| (parameter - known).abs() <= 1e-9 * scale)
        {
            parameters.push(parameter);
        }
    }
    parameters
        .into_iter()
        .map(|parameter| {
            let radius = (first.radius + parameter * first_slope).abs();
            let center: [f64; 3] =
                std::array::from_fn(|index| first.origin[index] + parameter * first_axis[index]);
            let (geometry, tag) = if circular_cone(first) {
                (
                    CurveGeometry::Circle {
                        center: Point3::new(center[0], center[1], center[2]),
                        axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                        ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                        radius,
                    },
                    "coaxial_cones_circle",
                )
            } else {
                (
                    CurveGeometry::Ellipse {
                        center: Point3::new(center[0], center[1], center[2]),
                        axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                        major_direction: Vector3::new(reference[0], reference[1], reference[2]),
                        major_radius: radius,
                        minor_radius: radius * first.ratio,
                    },
                    "coaxial_cones_ellipse",
                )
            };
            (geometry, tag)
        })
        .collect()
}

fn apex_plane_cone_generator_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Plane(plane), CarrierEquation::Cone(cone))
    | (CarrierEquation::Cone(cone), CarrierEquation::Plane(plane))) = (first, second)
    else {
        return Vec::new();
    };
    let Some(normal) = normalized(plane.normal) else {
        return Vec::new();
    };
    let Some(axis) = normalized(cone.axis) else {
        return Vec::new();
    };
    let Some(x_axis) = normalized(cone.ref_direction) else {
        return Vec::new();
    };
    let slope = cone.half_angle.tan();
    if slope <= 1e-12
        || !slope.is_finite()
        || cone.radius < 0.0
        || cone.ratio <= 0.0
        || !cone.ratio.is_finite()
        || dot(axis, x_axis).abs() > 1e-10
    {
        return Vec::new();
    }
    let apex: [f64; 3] =
        std::array::from_fn(|index| cone.origin[index] - cone.radius / slope * axis[index]);
    let plane_distance = dot(
        normal,
        std::array::from_fn(|index| apex[index] - plane.origin[index]),
    );
    let scale = cone.radius.max(1.0);
    if plane_distance.abs() > 1e-9 * scale {
        return Vec::new();
    }
    let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
        normal[0], normal[1], normal[2],
    ));
    let plane_u = [reference.x, reference.y, reference.z];
    let plane_v = cross(normal, plane_u);
    let y_axis = cross(axis, x_axis);
    let cone_coordinates = |direction: [f64; 3]| {
        [
            dot(direction, x_axis),
            dot(direction, y_axis) / cone.ratio,
            dot(direction, axis),
        ]
    };
    let quadratic = |first: [f64; 3], second: [f64; 3]| {
        first[0].mul_add(
            second[0],
            first[1] * second[1] - slope * slope * first[2] * second[2],
        )
    };
    let u_coordinates = cone_coordinates(plane_u);
    let v_coordinates = cone_coordinates(plane_v);
    let quadratic_uu = quadratic(u_coordinates, u_coordinates);
    let quadratic_uv = quadratic(u_coordinates, v_coordinates);
    let quadratic_vv = quadratic(v_coordinates, v_coordinates);
    let coefficient_scale = quadratic_uu
        .abs()
        .max(quadratic_uv.abs())
        .max(quadratic_vv.abs())
        .max(1.0);
    let determinant = quadratic_uu.mul_add(quadratic_vv, -quadratic_uv * quadratic_uv);
    let determinant_tolerance = 1e-12 * coefficient_scale * coefficient_scale;
    if determinant > determinant_tolerance {
        return Vec::new();
    }
    let angle = 0.5 * (2.0 * quadratic_uv).atan2(quadratic_uu - quadratic_vv);
    let (sine, cosine) = angle.sin_cos();
    let first_direction: [f64; 3] =
        std::array::from_fn(|index| cosine * plane_u[index] + sine * plane_v[index]);
    let second_direction: [f64; 3] =
        std::array::from_fn(|index| -sine * plane_u[index] + cosine * plane_v[index]);
    let first_value = quadratic_uu * cosine * cosine
        + 2.0 * quadratic_uv * cosine * sine
        + quadratic_vv * sine * sine;
    let second_value = quadratic_uu * sine * sine - 2.0 * quadratic_uv * cosine * sine
        + quadratic_vv * cosine * cosine;
    let directions = if determinant.abs() <= determinant_tolerance {
        if first_value.abs() <= second_value.abs() {
            vec![first_direction]
        } else {
            vec![second_direction]
        }
    } else {
        let (negative_value, negative_direction, positive_value, positive_direction) =
            if first_value < 0.0 {
                (first_value, first_direction, second_value, second_direction)
            } else {
                (second_value, second_direction, first_value, first_direction)
            };
        let negative_weight = positive_value.sqrt();
        let positive_weight = (-negative_value).sqrt();
        [-1.0, 1.0]
            .into_iter()
            .filter_map(|sense| {
                normalized(std::array::from_fn(|index| {
                    negative_weight * negative_direction[index]
                        + sense * positive_weight * positive_direction[index]
                }))
            })
            .collect()
    };
    let tag = if directions.len() == 1 {
        "plane_cone_tangent_line"
    } else {
        "plane_cone_secant_generator"
    };
    directions
        .into_iter()
        .map(|direction| {
            (
                CurveGeometry::Line {
                    origin: Point3::new(apex[0], apex[1], apex[2]),
                    direction: Vector3::new(direction[0], direction[1], direction[2]),
                },
                tag,
            )
        })
        .collect()
}

fn coaxial_cone_sphere_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cone(cone), CarrierEquation::Sphere(sphere))
    | (CarrierEquation::Sphere(sphere), CarrierEquation::Cone(cone))) = (first, second)
    else {
        return Vec::new();
    };
    if !circular_cone(cone) {
        return Vec::new();
    }
    let Some(axis) = normalized(cone.axis) else {
        return Vec::new();
    };
    let relative: [f64; 3] = std::array::from_fn(|index| sphere.center[index] - cone.origin[index]);
    let sphere_axial = dot(relative, axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - sphere_axial * axis[index]);
    let scale = cone.radius.max(sphere.radius).max(1.0);
    if dot(transverse, transverse).sqrt() > 1e-9 * scale {
        return Vec::new();
    }
    let slope = cone.half_angle.tan();
    if slope.abs() <= 1e-12 || !slope.is_finite() || cone.radius < 0.0 {
        return Vec::new();
    }
    let quadratic = 1.0 + slope * slope;
    let linear = 2.0 * (cone.radius * slope - sphere_axial);
    let constant =
        cone.radius * cone.radius + sphere_axial * sphere_axial - sphere.radius * sphere.radius;
    let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
    let discriminant_scale = linear
        .abs()
        .max((4.0 * quadratic * constant).abs().sqrt())
        .max(1.0);
    if discriminant <= 1e-9 * discriminant_scale * discriminant_scale {
        return Vec::new();
    }
    let Some(reference) = normalized(cone.ref_direction) else {
        return Vec::new();
    };
    let root_delta = discriminant.sqrt();
    [-root_delta, root_delta]
        .into_iter()
        .filter_map(|delta| {
            let parameter = (-linear + delta) / (2.0 * quadratic);
            let radius = (cone.radius + parameter * slope).abs();
            if radius <= 1e-12 * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + parameter * axis[index]);
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_cone_sphere_secant_circle",
            ))
        })
        .collect()
}

fn coaxial_cone_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cone(cone), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Cone(cone))) = (first, second)
    else {
        return Vec::new();
    };
    if !circular_cone(cone) {
        return Vec::new();
    }
    let (Some(cone_axis), Some(torus_axis), Some(reference)) = (
        normalized(cone.axis),
        normalized(torus.axis),
        normalized(cone.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(cone_axis, torus_axis).abs() - 1.0).abs() > 1e-10 {
        return Vec::new();
    }
    let relative: [f64; 3] = std::array::from_fn(|index| torus.center[index] - cone.origin[index]);
    let torus_axial = dot(relative, cone_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - torus_axial * cone_axis[index]);
    let scale = cone
        .radius
        .max(torus.major_radius)
        .max(torus.minor_radius)
        .max(1.0);
    let slope = cone.half_angle.tan();
    if dot(transverse, transverse).sqrt() > 1e-9 * scale
        || cone.radius < 0.0
        || torus.major_radius <= 1e-12 * scale
        || torus.minor_radius <= 1e-12 * scale
        || slope.abs() <= 1e-12
        || !slope.is_finite()
    {
        return Vec::new();
    }

    let quadratic = 1.0 + slope * slope;
    let mut parameters = Vec::<f64>::new();
    for radial_sense in [-1.0, 1.0] {
        let radial_offset = radial_sense * cone.radius - torus.major_radius;
        let radial_slope = radial_sense * slope;
        let linear = 2.0 * (radial_offset * radial_slope - torus_axial);
        let constant = radial_offset * radial_offset + torus_axial * torus_axial
            - torus.minor_radius * torus.minor_radius;
        let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
        let discriminant_scale = linear
            .abs()
            .max((4.0 * quadratic * constant).abs().sqrt())
            .max(1.0);
        let tolerance = 1e-9 * discriminant_scale * discriminant_scale;
        let deltas = if discriminant < -tolerance {
            continue;
        } else if discriminant.abs() <= tolerance {
            vec![0.0]
        } else {
            let root = discriminant.sqrt();
            vec![-root, root]
        };
        for delta in deltas {
            let parameter = (-linear + delta) / (2.0 * quadratic);
            let radius = radial_sense * (cone.radius + parameter * slope);
            if radius <= 1e-12 * scale {
                continue;
            }
            if !parameters
                .iter()
                .any(|known| (parameter - known).abs() <= 1e-9 * scale)
            {
                parameters.push(parameter);
            }
        }
    }
    parameters
        .into_iter()
        .map(|parameter| {
            let radius = (cone.radius + parameter * slope).abs();
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + parameter * cone_axis[index]);
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(cone_axis[0], cone_axis[1], cone_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_cone_torus_circle",
            )
        })
        .collect()
}

fn coaxial_cylinder_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cylinder(cylinder), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Cylinder(cylinder))) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(cylinder_axis), Some(torus_axis), Some(reference)) = (
        normalized(cylinder.axis),
        normalized(torus.axis),
        normalized(cylinder.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(cylinder_axis, torus_axis).abs() - 1.0).abs() > 1e-10 {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| torus.center[index] - cylinder.origin[index]);
    let axial = dot(relative, cylinder_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * cylinder_axis[index]);
    let scale = torus
        .major_radius
        .max(torus.minor_radius)
        .max(cylinder.radius)
        .max(1.0);
    if dot(transverse, transverse).sqrt() > 1e-9 * scale {
        return Vec::new();
    }
    let radial_delta = cylinder.radius - torus.major_radius;
    let height_squared = torus
        .minor_radius
        .mul_add(torus.minor_radius, -(radial_delta * radial_delta));
    if height_squared <= 1e-9 * scale * scale || cylinder.radius <= 1e-12 * scale {
        return Vec::new();
    }
    let height = height_squared.sqrt();
    [-height, height]
        .into_iter()
        .map(|offset| {
            let center: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] + offset * torus_axis[index]);
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(torus_axis[0], torus_axis[1], torus_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_torus_secant_circle",
            )
        })
        .collect()
}

fn axis_normal_plane_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Plane(plane), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Plane(plane))) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(normal), Some(axis), Some(reference)) = (
        normalized(plane.normal),
        normalized(torus.axis),
        normalized(torus.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(normal, axis).abs() - 1.0).abs() > 1e-10 {
        return Vec::new();
    }
    let relative: [f64; 3] = std::array::from_fn(|index| plane.origin[index] - torus.center[index]);
    let axial = dot(relative, axis);
    let scale = torus.major_radius.max(torus.minor_radius).max(1.0);
    let radial_offset_squared = torus
        .minor_radius
        .mul_add(torus.minor_radius, -(axial * axial));
    if radial_offset_squared <= 1e-9 * scale * scale {
        return Vec::new();
    }
    let center: [f64; 3] = std::array::from_fn(|index| torus.center[index] + axial * axis[index]);
    let radial_offset = radial_offset_squared.sqrt();
    [
        torus.major_radius - radial_offset,
        torus.major_radius + radial_offset,
    ]
    .into_iter()
    .filter(|radius| *radius > 1e-12 * scale)
    .map(|radius| {
        (
            CurveGeometry::Circle {
                center: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                radius,
            },
            "plane_torus_secant_circle",
        )
    })
    .collect()
}

fn meridian_circle_intersections(
    first_center: [f64; 2],
    first_radius: f64,
    second_center: [f64; 2],
    second_radius: f64,
    scale: f64,
) -> Vec<[f64; 2]> {
    let delta = [
        second_center[0] - first_center[0],
        second_center[1] - first_center[1],
    ];
    let distance = delta[0].hypot(delta[1]);
    if distance <= 1e-12 * scale
        || distance >= first_radius + second_radius - 1e-9 * scale
        || distance <= (first_radius - second_radius).abs() + 1e-9 * scale
    {
        return Vec::new();
    }
    let along = (distance * distance + first_radius * first_radius - second_radius * second_radius)
        / (2.0 * distance);
    let height_squared = first_radius.mul_add(first_radius, -(along * along));
    if height_squared <= 1e-12 * scale * scale {
        return Vec::new();
    }
    let unit = [delta[0] / distance, delta[1] / distance];
    let height = height_squared.sqrt();
    [-height, height]
        .into_iter()
        .map(|sense| {
            [
                first_center[0] + along * unit[0] - sense * unit[1],
                first_center[1] + along * unit[1] + sense * unit[0],
            ]
        })
        .collect()
}

fn axis_containing_plane_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Plane(plane), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Plane(plane))) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(normal), Some(axis)) = (normalized(plane.normal), normalized(torus.axis)) else {
        return Vec::new();
    };
    let scale = torus.major_radius.max(torus.minor_radius).max(1.0);
    let center_offset: [f64; 3] =
        std::array::from_fn(|index| torus.center[index] - plane.origin[index]);
    if dot(normal, axis).abs() > 1e-10
        || dot(normal, center_offset).abs() > 1e-9 * scale
        || !torus.major_radius.is_finite()
        || !torus.minor_radius.is_finite()
        || torus.major_radius <= 1e-12 * scale
        || torus.minor_radius <= 1e-12 * scale
    {
        return Vec::new();
    }
    let Some(radial) = normalized(cross(normal, axis)) else {
        return Vec::new();
    };
    [-1.0, 1.0]
        .into_iter()
        .map(|sense| {
            let center: [f64; 3] = std::array::from_fn(|index| {
                torus.center[index] + sense * torus.major_radius * radial[index]
            });
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    ref_direction: Vector3::new(axis[0], axis[1], axis[2]),
                    radius: torus.minor_radius,
                },
                "axis_containing_plane_torus_meridian_circle",
            )
        })
        .collect()
}

fn coaxial_sphere_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Sphere(sphere), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Sphere(sphere))) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(axis), Some(reference)) = (normalized(torus.axis), normalized(torus.ref_direction))
    else {
        return Vec::new();
    };
    let relative: [f64; 3] =
        std::array::from_fn(|index| torus.center[index] - sphere.center[index]);
    let axial = dot(relative, axis);
    let transverse: [f64; 3] = std::array::from_fn(|index| relative[index] - axial * axis[index]);
    let scale = torus
        .major_radius
        .max(torus.minor_radius)
        .max(sphere.radius)
        .max(1.0);
    if dot(transverse, transverse).sqrt() > 1e-9 * scale {
        return Vec::new();
    }
    meridian_circle_intersections(
        [0.0, 0.0],
        sphere.radius,
        [torus.major_radius, axial],
        torus.minor_radius,
        scale,
    )
    .into_iter()
    .filter_map(|[radius, center_axial]| {
        let radius = radius.abs();
        if radius <= 1e-12 * scale {
            return None;
        }
        let center: [f64; 3] =
            std::array::from_fn(|index| sphere.center[index] + center_axial * axis[index]);
        Some((
            CurveGeometry::Circle {
                center: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                radius,
            },
            "coaxial_sphere_torus_secant_circle",
        ))
    })
    .collect()
}

fn coaxial_tori_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let (CarrierEquation::Torus(first), CarrierEquation::Torus(second)) = (first, second) else {
        return Vec::new();
    };
    let (Some(first_axis), Some(second_axis), Some(reference)) = (
        normalized(first.axis),
        normalized(second.axis),
        normalized(first.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(first_axis, second_axis).abs() - 1.0).abs() > 1e-10 {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| second.center[index] - first.center[index]);
    let axial = dot(relative, first_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
    let scale = first
        .major_radius
        .max(first.minor_radius)
        .max(second.major_radius)
        .max(second.minor_radius)
        .max(1.0);
    if dot(transverse, transverse).sqrt() > 1e-9 * scale {
        return Vec::new();
    }
    meridian_circle_intersections(
        [first.major_radius, 0.0],
        first.minor_radius,
        [second.major_radius, axial],
        second.minor_radius,
        scale,
    )
    .into_iter()
    .filter_map(|[radius, center_axial]| {
        let radius = radius.abs();
        if radius <= 1e-12 * scale {
            return None;
        }
        let center: [f64; 3] =
            std::array::from_fn(|index| first.center[index] + center_axial * first_axis[index]);
        Some((
            CurveGeometry::Circle {
                center: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                radius,
            },
            "coaxial_tori_secant_circle",
        ))
    })
    .collect()
}

fn multi_component_intersection_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let mut candidates = parallel_plane_cylinder_generator_candidates(first, second);
    candidates.extend(parallel_cylinder_generator_candidates(first, second));
    candidates.extend(coaxial_cylinder_sphere_circle_candidates(first, second));
    candidates.extend(coaxial_cone_cylinder_circle_candidates(first, second));
    candidates.extend(coaxial_cones_section_candidates(first, second));
    candidates.extend(apex_plane_cone_generator_candidates(first, second));
    candidates.extend(coaxial_cone_sphere_circle_candidates(first, second));
    candidates.extend(coaxial_cone_torus_circle_candidates(first, second));
    candidates.extend(coaxial_cylinder_torus_circle_candidates(first, second));
    candidates.extend(coaxial_sphere_torus_circle_candidates(first, second));
    candidates.extend(coaxial_tori_circle_candidates(first, second));
    candidates.extend(axis_normal_plane_torus_circle_candidates(first, second));
    candidates.extend(axis_containing_plane_torus_circle_candidates(first, second));
    candidates
}

fn carrier_intersection_components(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    carrier_intersection_curve(first, second)
        .into_iter()
        .chain(multi_component_intersection_candidates(first, second))
        .collect()
}

fn intersect_plane_with_carrier_components(
    plane: PlaneEquation,
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<[f64; 3]> {
    carrier_intersection_components(first, second)
        .into_iter()
        .filter_map(|(geometry, _)| circle_parameters(&geometry))
        .flat_map(|(center, axis, radius)| intersect_plane_with_circle(plane, center, axis, radius))
        .collect()
}

fn curve_contains_points(geometry: &CurveGeometry, points: [[f64; 3]; 2]) -> bool {
    match geometry {
        CurveGeometry::Line { origin, direction } => {
            let origin = [origin.x, origin.y, origin.z];
            let Some(direction) = normalized([direction.x, direction.y, direction.z]) else {
                return false;
            };
            points.into_iter().all(|point| {
                let relative: [f64; 3] = std::array::from_fn(|index| point[index] - origin[index]);
                let residual = cross(relative, direction);
                let scale = dot(relative, relative).sqrt().max(1.0);
                dot(residual, residual).sqrt() <= 1e-7 * scale
            })
        }
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
            let Some(PeriodicConicFrame {
                center,
                normal,
                x_axis,
                y_axis,
                radii,
            }) = periodic_conic_frame(geometry)
            else {
                return false;
            };
            points.into_iter().all(|point| {
                let relative: [f64; 3] = std::array::from_fn(|index| point[index] - center[index]);
                let scale = radii.into_iter().fold(1.0, f64::max);
                let x = dot(relative, x_axis) / radii[0];
                let y = dot(relative, y_axis) / radii[1];
                dot(relative, normal).abs() <= 1e-7 * scale
                    && x.mul_add(x, y * y).is_finite()
                    && (x.mul_add(x, y * y) - 1.0).abs() <= 1e-7
            })
        }
        CurveGeometry::Parabola { .. } | CurveGeometry::Hyperbola { .. } => points
            .into_iter()
            .all(|point| nonperiodic_conic_parameter(geometry, point).is_some()),
        _ => false,
    }
}

fn select_unique_curve_candidate(
    candidates: Vec<(CurveGeometry, &'static str)>,
    points: [[f64; 3]; 2],
) -> Option<(CurveGeometry, &'static str)> {
    let candidates = candidates
        .into_iter()
        .filter(|(geometry, _)| curve_contains_points(geometry, points))
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn resolve_curve_candidates(
    candidates: Vec<(CurveGeometry, &'static str)>,
    points: Option<[[f64; 3]; 2]>,
) -> Option<(CurveGeometry, &'static str)> {
    if let Some(points) = points {
        return select_unique_curve_candidate(candidates, points);
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn analytic_curve_branches(
    geometry: &CurveGeometry,
    tag: &'static str,
) -> Vec<(CurveGeometry, &'static str)> {
    let mut branches = vec![(geometry.clone(), tag)];
    if let CurveGeometry::Hyperbola {
        center,
        axis,
        major_direction,
        major_radius,
        minor_radius,
    } = geometry
    {
        branches.push((
            CurveGeometry::Hyperbola {
                center: *center,
                axis: *axis,
                major_direction: Vector3::new(
                    -major_direction.x,
                    -major_direction.y,
                    -major_direction.z,
                ),
                major_radius: *major_radius,
                minor_radius: *minor_radius,
            },
            tag,
        ));
    }
    branches
}

fn transfer_carrier_intersection_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> BTreeSet<CurveId> {
    let mut transferred = BTreeSet::new();
    let carriers = placed_carriers(scan, ir);
    let solved_vertices = solved_topological_vertices(scan, ir, &carriers);
    let edge_vertices =
        crate::topology::edge_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    for row in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        let (Some(first), Some(second)) = (
            carriers.get(&row.faces[0]).copied(),
            carriers.get(&row.faces[1]).copied(),
        ) else {
            continue;
        };
        let points = (|| {
            let vertices = edge_vertices.get(&row.id)?;
            let points = [
                *solved_vertices.get(&vertices[0])?,
                *solved_vertices.get(&vertices[1])?,
            ];
            Some(points)
        })();
        let resolved = carrier_intersection_curve(first, second)
            .and_then(|(geometry, tag)| {
                resolve_curve_candidates(analytic_curve_branches(&geometry, tag), points)
            })
            .or_else(|| {
                resolve_curve_candidates(
                    multi_component_intersection_candidates(first, second),
                    points,
                )
            });
        let Some((geometry, tag)) = resolved else {
            continue;
        };
        let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            tag,
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred.insert(id);
    }
    transferred
}

fn rowless_round_cylinder_pairs(
    round_feature_ids: &BTreeSet<u32>,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Vec<(u32, u32, usize)> {
    tables
        .iter()
        .filter_map(|table| {
            let feature_id = table.feature_id?;
            round_feature_ids.contains(&feature_id).then_some(())?;
            let [first, second, rowless, cylinder] = table.entry_ids.as_slice() else {
                return None;
            };
            rows.iter().any(|row| row.id == *first).then_some(())?;
            rows.iter().any(|row| row.id == *second).then_some(())?;
            (!rows.iter().any(|row| row.id == *rowless)).then_some(())?;
            rows.iter()
                .any(|row| {
                    row.id == *cylinder
                        && row.feature_id == feature_id
                        && row.kind == crate::surface::SurfaceKind::Cylinder
                })
                .then_some(())?;
            Some((*rowless, *cylinder, table.offset))
        })
        .collect()
}

fn transfer_constrained_slot_fillet_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let round_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(913))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in round_feature_ids {
        let named = agreed_feature_affected_ids(
            &scan.features.affected_ids,
            feature_id,
            crate::feature::AffectedIdKind::Geometry,
        );
        let named_present = has_feature_affected_ids(
            &scan.features.affected_ids,
            feature_id,
            crate::feature::AffectedIdKind::Geometry,
        );
        let replay =
            agreed_feature_replay_geometry_ids(&scan.features.replay_affected_ids, feature_id);
        let affected = match (named, replay) {
            (Some(ids), _) => ids,
            (None, Some(ids)) if !named_present => ids,
            _ => continue,
        };
        let Some((cap_ids, support_ids)) = affected.split_at_checked(2) else {
            continue;
        };
        if support_ids.len() < 4 {
            continue;
        }
        let planes = affected
            .iter()
            .filter_map(|id| {
                let surface_id = SurfaceId(format!("creo:visibgeom:surface#{id}"));
                let surface = ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == surface_id)?;
                match surface.geometry {
                    SurfaceGeometry::Plane { origin, normal, .. } => Some(PlaneEquation {
                        origin: [origin.x, origin.y, origin.z],
                        normal: [normal.x, normal.y, normal.z],
                    }),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        if planes.len() != affected.len() {
            continue;
        }
        let cap_planes: [PlaneEquation; 2] = planes[..cap_ids.len()].try_into().expect("two caps");
        let Some(cylinder) = slot_fillet_cylinder(cap_planes, &planes[cap_ids.len()..]) else {
            continue;
        };
        let unresolved_rows = scan
            .surfaces
            .rows
            .iter()
            .filter(|row| {
                row.feature_id == feature_id
                    && row.kind == crate::surface::SurfaceKind::Cylinder
                    && !ir.model.surfaces.iter().any(|surface| {
                        surface.id == SurfaceId(format!("creo:visibgeom:surface#{}", row.id))
                    })
            })
            .collect::<Vec<_>>();
        let [row] = unresolved_rows.as_slice() else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        annotate(
            annotations,
            &id,
            "AllFeatur",
            row.offset as u64,
            "constrained_slot_fillet_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(cylinder.origin[0], cylinder.origin[1], cylinder.origin[2]),
                axis: Vector3::new(cylinder.axis[0], cylinder.axis[1], cylinder.axis[2]),
                ref_direction: Vector3::new(
                    cylinder.ref_direction[0],
                    cylinder.ref_direction[1],
                    cylinder.ref_direction[2],
                ),
                radius: cylinder.radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("AllFeatur:{}:{}", feature_id, row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

fn transfer_rowless_round_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let round_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(913))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for (rowless_id, sibling_id, offset) in rowless_round_cylinder_pairs(
        &round_feature_ids,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    ) {
        let sibling = SurfaceId(format!("creo:visibgeom:surface#{sibling_id}"));
        let Some(SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        }) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == sibling)
            .map(|surface| &surface.geometry)
        else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{rowless_id}"));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "AllFeatur",
            offset as u64,
            "round_rowless_sibling_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: *origin,
                axis: *axis,
                ref_direction: *ref_direction,
                radius: *radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("AllFeatur:{rowless_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

fn transfer_hole_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let hole_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(911))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in hole_feature_ids {
        let cylinders = if let Some(hole) = simple_hole_geometry(scan, feature_id) {
            hole.cylinder_ids
                .into_iter()
                .map(|id| (id, hole.geometry.clone()))
                .collect::<Vec<_>>()
        } else {
            counterbore_patch_geometries(scan, ir, feature_id).unwrap_or_default()
        };
        for (cylinder_id, geometry) in cylinders {
            let row = crate::surface::unique_surface_row(&scan.surfaces.rows, cylinder_id)
                .expect("validated cylinder row");
            let id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "AllFeatur",
                row.offset as u64,
                "hole_cap_outline_cylinder",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{cylinder_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

fn transfer_split_outline_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let rows = scan
        .surfaces
        .rows
        .iter()
        .map(|row| (row.id, row))
        .collect::<BTreeMap<_, _>>();
    let mut cylinders_by_plane = BTreeMap::<(u32, u32), BTreeSet<u32>>::new();
    for edge in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        if edge.type_byte != 0 {
            continue;
        }
        let [left, right] = edge.faces;
        let pair = match (rows.get(&left), rows.get(&right)) {
            (Some(plane), Some(cylinder))
                if plane.kind == crate::surface::SurfaceKind::Plane
                    && cylinder.kind == crate::surface::SurfaceKind::Cylinder =>
            {
                Some(((left, cylinder.feature_id), right))
            }
            (Some(cylinder), Some(plane))
                if plane.kind == crate::surface::SurfaceKind::Plane
                    && cylinder.kind == crate::surface::SurfaceKind::Cylinder =>
            {
                Some(((right, cylinder.feature_id), left))
            }
            _ => None,
        };
        if let Some((plane_and_feature, cylinder)) = pair {
            cylinders_by_plane
                .entry(plane_and_feature)
                .or_default()
                .insert(cylinder);
        }
    }

    let mut transferred = 0;
    for ((plane_id, _), cylinder_ids) in cylinders_by_plane {
        let cylinder_ids = cylinder_ids.into_iter().collect::<Vec<_>>();
        let [first_id, second_id] = cylinder_ids.as_slice() else {
            continue;
        };
        let Some(first) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, *first_id)
        else {
            continue;
        };
        let Some(second) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, *second_id)
        else {
            continue;
        };
        let Some(bounds) = first
            .split_cylinder_outline_bounds
            .zip(second.split_cylinder_outline_bounds)
            .map(|(first, second)| [first, second])
        else {
            continue;
        };
        let plane_id = SurfaceId(format!("creo:visibgeom:surface#{plane_id}"));
        let Some(plane) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == plane_id)
        else {
            continue;
        };
        let Some(geometry) = cylinder_from_complementary_outline_bounds(&plane.geometry, bounds)
        else {
            continue;
        };
        for cylinder_id in [*first_id, *second_id] {
            let id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            let row = rows[&cylinder_id];
            annotate(
                annotations,
                &id,
                "VisibGeom",
                row.offset as u64,
                "split_outline_cylinder",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: geometry.clone(),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{cylinder_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

fn transfer_positional_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        if crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id)
            != Some(record)
        {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
            .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        else {
            continue;
        };
        let reference_bound_frame = || {
            let entity_ids = scan
                .features
                .entity_tables
                .iter()
                .filter(|table| table.feature_id == Some(row.feature_id))
                .flat_map(|table| table.entry_ids.iter().copied())
                .collect::<BTreeSet<_>>();
            let circles = scan
                .references
                .circles
                .iter()
                .filter(|circle| entity_ids.contains(&circle.entity_id))
                .collect::<Vec<_>>();
            let generated_cylinder_count = scan
                .surfaces
                .rows
                .iter()
                .filter(|candidate| {
                    candidate.feature_id == row.feature_id
                        && candidate.kind == crate::surface::SurfaceKind::Cylinder
                })
                .count();
            if generated_cylinder_count == 1 {
                if let Some(frame) = reference_circle_pair_cylinder_frame(&circles) {
                    return Some((frame, "reference_circle_pair_cylinder_frame"));
                }
            }
            let envelope = record.type24_scalar_frame_round_envelope(row.type_byte)?;
            reference_cap_bound_round_frame(envelope, &circles)
                .map(|frame| (frame, "round_reference_cap_cylinder_frame"))
        };
        let (frame, mechanism) = match record.positional_cylinder_frame {
            Some(frame) => (frame, "positional_cylinder_frame"),
            None => {
                let Some(frame) = reference_bound_frame() else {
                    continue;
                };
                frame
            }
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", record.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            mechanism,
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(frame.origin[0], frame.origin[1], frame.origin[2]),
                axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                ref_direction: Vector3::new(
                    frame.ref_direction[0],
                    frame.ref_direction[1],
                    frame.ref_direction[2],
                ),
                radius: frame.radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", record.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

fn reference_circle_pair_cylinder_frame(
    circles: &[&crate::reference::ReferenceCircle],
) -> Option<crate::surface::PositionalCylinderFrame> {
    let [first, second] = circles else {
        return None;
    };
    (first.radius.is_finite()
        && first.radius > 0.0
        && first.center_stored
        && second.center_stored
        && second.radius.is_finite())
    .then_some(())?;
    let radius = first.radius;
    let radius_scale = radius.max(second.radius).max(1.0);
    ((second.radius - radius).abs() <= 1e-9 * radius_scale).then_some(())?;
    let scale = first
        .center
        .iter()
        .chain(&second.center)
        .map(|value| value.abs())
        .fold(radius_scale, f64::max);
    let first_axis = normalized(first.axis)?;
    let second_axis = normalized(second.axis)?;
    ((dot(first_axis, second_axis).abs() - 1.0).abs() <= 1e-9).then_some(())?;
    let displacement: [f64; 3] =
        std::array::from_fn(|index| second.center[index] - first.center[index]);
    let length = dot(displacement, displacement).sqrt();
    (length.is_finite() && length > 1e-9 * scale).then_some(())?;
    let center_direction = displacement.map(|value| value / length);
    ((dot(center_direction, first_axis).abs() - 1.0).abs() <= 1e-9
        && (dot(center_direction, second_axis).abs() - 1.0).abs() <= 1e-9)
        .then_some(())?;
    let validated_radial = |circle: &crate::reference::ReferenceCircle, axis| {
        let vector: [f64; 3] =
            std::array::from_fn(|index| circle.start[index] - circle.center[index]);
        let length = dot(vector, vector).sqrt();
        ((length - radius).abs() <= 1e-9 * radius_scale
            && dot(axis, vector).abs() <= 1e-9 * radius_scale)
            .then_some((vector, length))
    };
    let (radial, radial_length) = validated_radial(first, first_axis)?;
    validated_radial(second, second_axis)?;
    Some(crate::surface::PositionalCylinderFrame {
        origin: first.center,
        axis: first_axis,
        ref_direction: radial.map(|value| value / radial_length),
        radius,
        length: Some(length),
    })
}

fn reference_cap_bound_round_frame(
    envelope: crate::surface::Type24RoundEnvelope,
    circles: &[&crate::reference::ReferenceCircle],
) -> Option<crate::surface::PositionalCylinderFrame> {
    let [first, second] = envelope.extent_endpoints;
    let scale = first
        .iter()
        .chain(&second)
        .copied()
        .map(f64::abs)
        .fold(envelope.diameter.max(1.0), f64::max);
    let tolerance = 1.0e-9 * scale;
    let point_matches = |actual: [f64; 3], expected: [f64; 3]| {
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() <= tolerance)
    };
    let mut candidates = Vec::new();
    for axis_index in 0..3 {
        let radial_indices = (0..3)
            .filter(|index| *index != axis_index)
            .collect::<Vec<_>>();
        if radial_indices.iter().any(|index| {
            ((second[*index] - first[*index]).abs() - envelope.diameter).abs() > tolerance
        }) || (second[axis_index] - first[axis_index]).abs() <= tolerance
        {
            continue;
        }
        let cap_pair = |coordinate: f64, crossed: bool| {
            let mut first_corner = first;
            let mut second_corner = second;
            first_corner[axis_index] = coordinate;
            second_corner[axis_index] = coordinate;
            if crossed {
                first_corner[radial_indices[1]] = second[radial_indices[1]];
                second_corner[radial_indices[1]] = first[radial_indices[1]];
            }
            circles.iter().any(|circle| {
                circle.axis.iter().enumerate().all(|(index, component)| {
                    if index == axis_index {
                        (component.abs() - 1.0).abs() <= 1.0e-9
                    } else {
                        component.abs() <= 1.0e-9
                    }
                }) && ((point_matches(circle.start, first_corner)
                    && point_matches(circle.end, second_corner))
                    || (point_matches(circle.end, first_corner)
                        && point_matches(circle.start, second_corner)))
            })
        };
        if ![false, true].into_iter().any(|crossed| {
            cap_pair(first[axis_index], crossed) && cap_pair(second[axis_index], crossed)
        }) {
            continue;
        }
        let mut origin = first;
        for index in &radial_indices {
            origin[*index] = first[*index].midpoint(second[*index]);
        }
        let mut axis = [0.0; 3];
        axis[axis_index] = (second[axis_index] - first[axis_index]).signum();
        let mut ref_direction = [0.0; 3];
        let reference_index = radial_indices[0];
        ref_direction[reference_index] =
            (second[reference_index] - first[reference_index]).signum();
        candidates.push(crate::surface::PositionalCylinderFrame {
            origin,
            axis,
            ref_direction,
            radius: envelope.diameter / 2.0,
            length: Some((second[axis_index] - first[axis_index]).abs()),
        });
    }
    let [frame] = candidates.as_slice() else {
        return None;
    };
    Some(*frame)
}

fn transfer_positional_cones(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        let Some(frame) = record.positional_cone_frame else {
            continue;
        };
        if crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id)
            != Some(record)
        {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
            .filter(|row| row.kind == crate::surface::SurfaceKind::Cone)
        else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", record.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            "positional_cone_frame",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cone {
                origin: Point3::new(frame.apex[0], frame.apex[1], frame.apex[2]),
                axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                ref_direction: Vector3::new(
                    frame.ref_direction[0],
                    frame.ref_direction[1],
                    frame.ref_direction[2],
                ),
                radius: 0.0,
                ratio: 1.0,
                half_angle: frame.half_angle,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", record.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

fn transfer_circular_sweep_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let sweep_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| {
            row.root_schema_class == Some(917)
                && section_sweep_allows_linear_extrusion(917, feature_recipe(scan, row.feature_id))
        })
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in sweep_feature_ids {
        let Some(sweep) = circular_sweep_geometry(scan, feature_id) else {
            continue;
        };
        for cylinder_id in &sweep.cylinder_ids {
            let row = crate::surface::unique_surface_row(&scan.surfaces.rows, *cylinder_id)
                .expect("validated cylinder row");
            let id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "AllFeatur",
                row.offset as u64,
                "circular_sweep_cap_outline_cylinder",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: sweep.geometry.clone(),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{cylinder_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

fn transfer_cross_section_planes(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for frame in &scan.planes.cross_section_local_systems {
        let (Some(origin), Some(normal), Some(u_axis)) = (frame.origin, frame.normal, frame.u_axis)
        else {
            continue;
        };
        if is_axis_aligned(normal) {
            continue;
        }
        let id = SurfaceId(format!(
            "creo:cross_section_geometry:surface#{}",
            frame.surface_id
        ));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "Xsections",
            frame.offset as u64,
            "cross_section_plane_local_system",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("Xsections:{}", frame.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    for plane in &scan.planes.cross_section_outlines {
        let id = SurfaceId(format!(
            "creo:cross_section_geometry:surface#{}",
            plane.surface_id
        ));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "Xsections",
            plane.offset as u64,
            "cross_section_plane_outline_held_coordinate",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                normal: Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]),
                u_axis: Vector3::new(plane.u_axis[0], plane.u_axis[1], plane.u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("Xsections:{}", plane.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

/// Decode a `.prt` stream into an IR document and loss report.
///
/// The stream is read from its beginning. When `options.container_only` is set,
/// the returned IR contains source metadata and preserved geometry sections but
/// no transferred entities.
pub fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<DecodeResult, CodecError> {
    let scan = container::scan_bytes(root.window().to_vec());

    let (mut ir, annotations, unknowns, coverage) = if ctx.container_only() {
        build_container_ir(&scan)?
    } else {
        build_ir(&scan)?
    };
    let report = build_report(&scan, &ir, coverage, ctx.container_only());
    let mut source_fidelity = cadmpeg_ir::SourceFidelity {
        annotations,
        ..cadmpeg_ir::SourceFidelity::default()
    };
    source_fidelity.attach_native_unknown_records(&mut ir, "creo", &unknowns)?;
    Ok(DecodeResult::with_source_fidelity(
        ir,
        report,
        source_fidelity,
    ))
}

fn preserve_passthrough_sections(
    scan: &ContainerScan,
    annotations: &mut AnnotationBuilder,
) -> Vec<UnknownRecord> {
    let mut unknowns = Vec::new();
    for section in scan
        .framing
        .sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY || section.role == role::THUMBNAIL)
    {
        let end = (section.offset + section.length).min(scan.framing.data.len());
        let section_bytes = &scan.framing.data[section.offset..end];
        let payload_start = if section.role == role::THUMBNAIL {
            let Some(offset) = section_bytes
                .windows(3)
                .position(|window| window == [0xff, 0xd8, 0xff])
            else {
                continue;
            };
            offset
        } else {
            0
        };
        let bytes = &section_bytes[payload_start..];
        let offset = section.offset + payload_start;
        let id = UnknownId(format!("creo:{}:section#{}", section.name, offset));
        let (tag, exactness) = if section.role == role::THUMBNAIL {
            ("jpeg_thumbnail", Exactness::ByteExact)
        } else {
            ("psb_geometry_section", Exactness::Unknown)
        };
        annotate(
            annotations,
            &id,
            &section.name,
            offset as u64,
            tag,
            exactness,
        );
        unknowns.push(UnknownRecord {
            id,
            offset: offset as u64,
            byte_len: bytes.len() as u64,
            sha256: sha256_hex(bytes),
            data: Some(bytes.to_vec()),
            links: Vec::new(),
        });
    }
    unknowns
}

/// Decoded IR together with its annotations, preserved unknown records, and
/// decode-coverage counts.
type BuiltIr = (
    CadIr,
    cadmpeg_ir::Annotations,
    Vec<UnknownRecord>,
    BTreeMap<String, usize>,
);

fn build_container_ir(scan: &ContainerScan) -> Result<BuiltIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let (meta, coverage) = source_meta(scan);
    ir.source = Some(meta);
    let unknowns = preserve_passthrough_sections(scan, &mut annotations);
    attach_expanded_sections(scan, &mut ir, &mut annotations)?;
    Ok((ir, annotations.build(), unknowns, coverage))
}

/// Build source metadata, preserved geometry records, and transferred entities.
fn build_ir(scan: &ContainerScan) -> Result<BuiltIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let (meta, mut coverage) = source_meta(scan);
    ir.source = Some(meta);
    let unknowns = preserve_passthrough_sections(scan, &mut annotations);
    if !scan.references.lines.is_empty() {
        let family = |kind: &crate::reference::ReferenceLineKind| match kind {
            crate::reference::ReferenceLineKind::Line => "line",
            crate::reference::ReferenceLineKind::Line3d { .. } => "line3d",
        };
        let records = scan
            .references
            .lines
            .iter()
            .map(|line| CreoReferenceLineRecord {
                id: format!(
                    "creo:mdl_ref_info:{}_record#{}",
                    family(&line.kind),
                    line.offset
                ),
                family: family(&line.kind),
                entity_id: match &line.kind {
                    crate::reference::ReferenceLineKind::Line => None,
                    crate::reference::ReferenceLineKind::Line3d { entity_id, .. } => {
                        Some(*entity_id)
                    }
                },
                start: line.start,
                end: line.end,
                original_length: match &line.kind {
                    crate::reference::ReferenceLineKind::Line => None,
                    crate::reference::ReferenceLineKind::Line3d {
                        original_length, ..
                    } => Some(*original_length),
                },
                offset: line.offset,
            })
            .collect::<Vec<_>>();
        emit_arena(
            &mut ir,
            &mut annotations,
            "reference_lines",
            &records,
            |annotations, record| {
                annotate(
                    annotations,
                    &record.id,
                    "MdlRefInfo",
                    record.offset as u64,
                    "reference_line_record",
                    Exactness::ByteExact,
                );
            },
        )?;
    }
    if !scan.references.circles.is_empty() {
        let records = scan
            .references
            .circles
            .iter()
            .map(|circle| CreoReferenceCircleRecord {
                id: format!("creo:mdl_ref_info:arc_z_record#{}", circle.offset),
                entity_id: circle.entity_id,
                center: circle.center,
                center_source: if circle.center_stored {
                    "stored"
                } else {
                    "endpoint_midpoint"
                },
                radius: circle.radius,
                axis: circle.axis,
                endpoints: [circle.start, circle.end],
                offset: circle.offset,
            })
            .collect::<Vec<_>>();
        emit_arena(
            &mut ir,
            &mut annotations,
            "reference_circles",
            &records,
            |annotations, record| {
                annotate(
                    annotations,
                    &record.id,
                    "MdlRefInfo",
                    record.offset as u64,
                    "reference_circle_record",
                    Exactness::Derived,
                );
            },
        )?;
    }
    if !scan.references.conics.is_empty() {
        let records = scan
            .references
            .conics
            .iter()
            .map(|conic| CreoReferenceConicRecord {
                id: format!("creo:mdl_ref_info:conic_record#{}", conic.offset),
                entity_id: conic.entity_id,
                type_id: conic.type_id,
                flip: conic.flip,
                endpoints: [conic.start, conic.end],
                parameter_interval: [conic.parameter_start, conic.parameter_end],
                coefficients: [conic.coefficient_1, conic.coefficient_2],
                local_system: conic.local_system,
                body: conic.body.clone(),
                offset: conic.offset,
            })
            .collect::<Vec<_>>();
        emit_arena(
            &mut ir,
            &mut annotations,
            "reference_conics",
            &records,
            |annotations, record| {
                annotate(
                    annotations,
                    &record.id,
                    "MdlRefInfo",
                    record.offset as u64,
                    "reference_conic_record",
                    Exactness::ByteExact,
                );
            },
        )?;
    }
    if !scan.references.ellipses.is_empty() {
        let records = scan
            .references
            .ellipses
            .iter()
            .map(|ellipse| CreoReferenceEllipseRecord {
                id: format!("creo:mdl_ref_info:ellipse_carrier#{}", ellipse.offset),
                source_conic_id: format!("creo:mdl_ref_info:conic_record#{}", ellipse.offset),
                source_entity_id: ellipse.source_entity_id,
                center: ellipse.center,
                axis: ellipse.axis,
                major_direction: ellipse.major_direction,
                major_radius: ellipse.major_radius,
                minor_radius: ellipse.minor_radius,
                offset: ellipse.offset,
            })
            .collect::<Vec<_>>();
        emit_arena(
            &mut ir,
            &mut annotations,
            "reference_ellipses",
            &records,
            |annotations, record| {
                annotate(
                    annotations,
                    &record.id,
                    "MdlRefInfo",
                    record.offset as u64,
                    "reference_ellipse_carrier",
                    Exactness::Derived,
                );
            },
        )?;
    }
    let line3d_id_counts =
        scan.references
            .lines
            .iter()
            .fold(BTreeMap::<u32, usize>::new(), |mut counts, line| {
                if let crate::reference::ReferenceLineKind::Line3d { entity_id, .. } = &line.kind {
                    *counts.entry(*entity_id).or_default() += 1;
                }
                counts
            });
    for line in &scan.references.lines {
        let direction = std::array::from_fn(|axis| line.end[axis] - line.start[axis]);
        let Some(direction) = normalized(direction) else {
            continue;
        };
        let (family, native_identity) = match &line.kind {
            crate::reference::ReferenceLineKind::Line => ("line", line.offset.to_string()),
            crate::reference::ReferenceLineKind::Line3d { entity_id, .. } => {
                let identity = if line3d_id_counts.get(entity_id) == Some(&1) {
                    entity_id.to_string()
                } else {
                    format!("{entity_id}@{}", line.offset)
                };
                ("line3d", identity)
            }
        };
        let prefix = format!("creo:mdl_ref_info:{family}#{native_identity}");
        let id = CurveId(prefix);
        annotate(
            &mut annotations,
            &id,
            "MdlRefInfo",
            line.offset as u64,
            "reference_line",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Line {
                origin: Point3::new(line.start[0], line.start[1], line.start[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("MdlRefInfo:{family}:{native_identity}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    let circle_id_counts =
        scan.references
            .circles
            .iter()
            .fold(BTreeMap::<u32, usize>::new(), |mut counts, circle| {
                *counts.entry(circle.entity_id).or_default() += 1;
                counts
            });
    for circle in &scan.references.circles {
        let radial = std::array::from_fn(|axis| circle.start[axis] - circle.center[axis]);
        let Some(reference) = normalized(radial) else {
            continue;
        };
        let native_identity = if circle_id_counts.get(&circle.entity_id) == Some(&1) {
            circle.entity_id.to_string()
        } else {
            format!("{}@{}", circle.entity_id, circle.offset)
        };
        let id = CurveId(format!("creo:mdl_ref_info:arc_z#{native_identity}"));
        annotate(
            &mut annotations,
            &id,
            "MdlRefInfo",
            circle.offset as u64,
            "reference_circle",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Circle {
                center: Point3::new(circle.center[0], circle.center[1], circle.center[2]),
                axis: Vector3::new(circle.axis[0], circle.axis[1], circle.axis[2]),
                ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                radius: circle.radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("MdlRefInfo:arc_z:{native_identity}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    let ellipse_id_counts = scan.references.ellipses.iter().fold(
        BTreeMap::<u32, usize>::new(),
        |mut counts, ellipse| {
            *counts.entry(ellipse.source_entity_id).or_default() += 1;
            counts
        },
    );
    for ellipse in &scan.references.ellipses {
        let native_identity = if ellipse_id_counts.get(&ellipse.source_entity_id) == Some(&1) {
            ellipse.source_entity_id.to_string()
        } else {
            format!("{}@{}", ellipse.source_entity_id, ellipse.offset)
        };
        let id = CurveId(format!("creo:mdl_ref_info:conic#{native_identity}"));
        annotate(
            &mut annotations,
            &id,
            "MdlRefInfo",
            ellipse.offset as u64,
            "reference_ellipse",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Ellipse {
                center: Point3::new(ellipse.center[0], ellipse.center[1], ellipse.center[2]),
                axis: Vector3::new(ellipse.axis[0], ellipse.axis[1], ellipse.axis[2]),
                major_direction: Vector3::new(
                    ellipse.major_direction[0],
                    ellipse.major_direction[1],
                    ellipse.major_direction[2],
                ),
                major_radius: ellipse.major_radius,
                minor_radius: ellipse.minor_radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("MdlRefInfo:conic:{native_identity}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    for strip in &scan.primitives.triangle_strips {
        let id = format!("creo:solid_primdata:tessellation#{}", strip.offset);
        let mut triangles = Vec::new();
        let mut base = 0u32;
        for length in &strip.strip_lengths {
            for index in 0..length.saturating_sub(2) {
                let a = base + index;
                let triangle = if index % 2 == 0 {
                    [a, a + 1, a + 2]
                } else {
                    [a, a + 2, a + 1]
                };
                triangles.push(triangle);
            }
            base += length;
        }
        annotate(
            &mut annotations,
            &id,
            "SolidPrimdata",
            strip.offset as u64,
            "display_triangle_strip",
            Exactness::Derived,
        );
        ir.model.tessellations.push(Tessellation {
            id,
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            vertices: strip
                .positions
                .iter()
                .map(|point| Point3::new(point[0], point[1], point[2]))
                .collect(),
            triangles,
            strip_lengths: strip.strip_lengths.clone(),
            normals: strip
                .normals
                .iter()
                .map(|normal| Vector3::new(normal[0], normal[1], normal[2]))
                .collect(),
            channels: Vec::new(),
        });
    }
    for plane in &scan.planes.datums {
        let id = SurfaceId(format!("creo:actdatums:surface#{}", plane.id));
        annotate(
            &mut annotations,
            &id,
            "ActDatums",
            plane.offset_in_payload as u64,
            "datum_plane_outline",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(
                    plane.normal[0] * plane.offset,
                    plane.normal[1] * plane.offset,
                    plane.normal[2] * plane.offset,
                ),
                normal: Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]),
                u_axis: cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                    plane.normal[0],
                    plane.normal[1],
                    plane.normal[2],
                )),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("ActDatums:{}", plane.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    for (surface_id, (plane, u_axis, offset)) in placed_plane_surfaces(scan) {
        let id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        let tag = if scan
            .planes
            .positional_frames
            .iter()
            .any(|plane| plane.surface_id == surface_id && plane.offset == offset)
        {
            "plane_positional_corner_frame"
        } else if scan
            .planes
            .outlines
            .iter()
            .any(|outline| outline.surface_id == surface_id && outline.offset == offset)
        {
            "plane_outline_held_coordinate"
        } else {
            "plane_local_system"
        };
        annotate(
            &mut annotations,
            &id,
            "VisibGeom",
            offset as u64,
            tag,
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                normal: Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]),
                u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{surface_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    let cross_section_plane_count = transfer_cross_section_planes(scan, &mut ir, &mut annotations);
    let first_instance_prototype_surface_count =
        transfer_first_instance_prototype_surfaces(scan, &mut ir, &mut annotations);
    let paired_envelope_sphere_count =
        transfer_paired_envelope_spheres(scan, &mut ir, &mut annotations);
    let positional_torus_count = transfer_positional_tori(scan, &mut ir, &mut annotations);
    let positional_line_extrusion_plane_count =
        transfer_positional_line_extrusion_planes(scan, &mut ir, &mut annotations);
    let tabulated_cylinder_spline_extrusion_count =
        transfer_tabulated_cylinder_spline_extrusions(scan, &mut ir, &mut annotations);
    transfer_fc05_cap_circles(scan, &mut ir, &mut annotations);
    transfer_cap_pair_cylinders(scan, &mut ir, &mut annotations);
    let saved_spline_curve_count = transfer_saved_spline_curves(scan, &mut ir, &mut annotations);
    transfer_sketches(scan, &mut ir, &mut annotations);
    let feature_revolution_surface_count =
        transfer_resolved_revolution_surfaces(scan, &mut ir, &mut annotations);
    let feature_revolution_vertex_orbit_curve_count =
        transfer_resolved_revolution_vertex_orbit_curves(scan, &mut ir, &mut annotations);
    let feature_extrusion_surface_count =
        transfer_feature_extrusion_surfaces(scan, &mut ir, &mut annotations);
    let feature_extrusion_vertex_orbit_curve_count =
        transfer_resolved_extrusion_vertex_orbit_curves(scan, &mut ir, &mut annotations);
    let circular_sweep_cylinder_count =
        transfer_circular_sweep_cylinders(scan, &mut ir, &mut annotations);
    let positional_cylinder_count = transfer_positional_cylinders(scan, &mut ir, &mut annotations);
    let positional_cone_count = transfer_positional_cones(scan, &mut ir, &mut annotations);
    let split_outline_cylinder_count =
        transfer_split_outline_cylinders(scan, &mut ir, &mut annotations);
    let hole_cylinder_count = transfer_hole_cylinders(scan, &mut ir, &mut annotations);
    let constrained_slot_fillet_cylinder_count =
        transfer_constrained_slot_fillet_cylinders(scan, &mut ir, &mut annotations);
    let rowless_round_cylinder_count =
        transfer_rowless_round_cylinders(scan, &mut ir, &mut annotations);
    let analytic_pcurve_carriers =
        transfer_analytic_pcurve_carriers(scan, &mut ir, &mut annotations);
    let analytic_pcurve_carrier_count = analytic_pcurve_carriers.len();
    let derived_intersection_curves =
        transfer_carrier_intersection_curves(scan, &mut ir, &mut annotations);
    let (topological_point_count, native_topological_edge_count) = transfer_native_brep(
        scan,
        &mut ir,
        &mut annotations,
        &derived_intersection_curves,
        &analytic_pcurve_carriers,
    );
    let feature_revolution_brep_count =
        transfer_resolved_revolution_breps(scan, &mut ir, &mut annotations);
    let feature_circular_extrusion_brep_count =
        transfer_resolved_circular_extrusion_breps(scan, &mut ir, &mut annotations);
    let feature_extrusion_brep_count =
        transfer_resolved_extrusion_breps(scan, &mut ir, &mut annotations);
    let transferred_part_product = transfer_part_product(scan, &mut ir, &mut annotations);
    let decoded_feature_skamp_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(|relations| relations.skamps.len())
        .sum::<usize>();
    let skamp_constraint_coverage =
        design_constraint_transfer_coverage(&ir.model.sketch_constraints, ":skamp:", "creo:skamp:");
    let decoded_feature_relation_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(|relations| relations.rows.len())
        .sum::<usize>();
    let relation_constraint_coverage = design_constraint_transfer_coverage(
        &ir.model.sketch_constraints,
        ":relation:",
        "creo:relation:",
    );
    let surface_coverage = surface_transfer_coverage(
        &scan.surfaces.rows,
        &ir.model.surfaces,
        &ir.model.procedural_surfaces,
    );
    let curve_coverage = curve_transfer_coverage(&scan.curves.topology_rows, &ir.model.curves);
    {
        coverage.insert(
            "unique_visible_surface_row_count".to_string(),
            surface_coverage.unique_rows,
        );
        coverage.insert(
            "transferred_visible_surface_row_count".to_string(),
            surface_coverage.transferred_rows,
        );
        coverage.insert(
            "untransferred_visible_surface_row_count".to_string(),
            surface_coverage
                .unique_rows
                .saturating_sub(surface_coverage.transferred_rows),
        );
        coverage.insert(
            "ambiguous_visible_surface_row_count".to_string(),
            surface_coverage.ambiguous_rows,
        );
        for (family, (rows, transferred)) in &surface_coverage.by_family {
            coverage.insert(format!("visible_{family}_surface_row_count"), *rows);
            coverage.insert(
                format!("transferred_visible_{family}_surface_row_count"),
                *transferred,
            );
        }
        coverage.insert(
            "unique_visible_curve_row_count".to_string(),
            curve_coverage.unique_rows,
        );
        coverage.insert(
            "transferred_visible_curve_row_count".to_string(),
            curve_coverage.transferred_rows,
        );
        coverage.insert(
            "untransferred_visible_curve_row_count".to_string(),
            curve_coverage
                .unique_rows
                .saturating_sub(curve_coverage.transferred_rows),
        );
        coverage.insert(
            "ambiguous_visible_curve_row_count".to_string(),
            curve_coverage.ambiguous_rows,
        );
        for (type_byte, (rows, transferred)) in &curve_coverage.by_type {
            coverage.insert(
                format!("visible_curve_type_{type_byte:02x}_row_count"),
                *rows,
            );
            coverage.insert(
                format!("transferred_visible_curve_type_{type_byte:02x}_row_count"),
                *transferred,
            );
        }
        coverage.insert(
            "transferred_cross_section_plane_count".to_string(),
            cross_section_plane_count,
        );
        coverage.insert(
            "transferred_first_instance_prototype_surface_count".to_string(),
            first_instance_prototype_surface_count,
        );
        coverage.insert(
            "transferred_paired_envelope_sphere_count".to_string(),
            paired_envelope_sphere_count,
        );
        coverage.insert(
            "transferred_positional_torus_count".to_string(),
            positional_torus_count,
        );
        coverage.insert(
            "transferred_positional_line_extrusion_plane_count".to_string(),
            positional_line_extrusion_plane_count,
        );
        coverage.insert(
            "transferred_tabulated_cylinder_spline_extrusion_count".to_string(),
            tabulated_cylinder_spline_extrusion_count,
        );
        coverage.insert(
            "transferred_saved_spline_curve_count".to_string(),
            saved_spline_curve_count,
        );
        coverage.insert(
            "transferred_topological_point_count".to_string(),
            topological_point_count,
        );
        coverage.insert(
            "transferred_native_topological_edge_count".to_string(),
            native_topological_edge_count,
        );
        coverage.insert(
            "transferred_analytic_pcurve_carrier_count".to_string(),
            analytic_pcurve_carrier_count,
        );
        coverage.insert(
            "transferred_feature_revolution_surface_count".to_string(),
            feature_revolution_surface_count,
        );
        coverage.insert(
            "transferred_feature_revolution_vertex_orbit_curve_count".to_string(),
            feature_revolution_vertex_orbit_curve_count,
        );
        coverage.insert(
            "transferred_feature_extrusion_surface_count".to_string(),
            feature_extrusion_surface_count,
        );
        coverage.insert(
            "transferred_feature_extrusion_vertex_orbit_curve_count".to_string(),
            feature_extrusion_vertex_orbit_curve_count,
        );
        coverage.insert(
            "transferred_circular_sweep_cylinder_count".to_string(),
            circular_sweep_cylinder_count,
        );
        coverage.insert(
            "transferred_hole_cylinder_count".to_string(),
            hole_cylinder_count,
        );
        coverage.insert(
            "transferred_positional_cylinder_count".to_string(),
            positional_cylinder_count,
        );
        coverage.insert(
            "transferred_positional_cone_count".to_string(),
            positional_cone_count,
        );
        coverage.insert(
            "transferred_split_outline_cylinder_count".to_string(),
            split_outline_cylinder_count,
        );
        coverage.insert(
            "transferred_constrained_slot_fillet_cylinder_count".to_string(),
            constrained_slot_fillet_cylinder_count,
        );
        coverage.insert(
            "transferred_rowless_round_cylinder_count".to_string(),
            rowless_round_cylinder_count,
        );
        coverage.insert(
            "transferred_feature_revolution_brep_count".to_string(),
            feature_revolution_brep_count,
        );
        coverage.insert(
            "transferred_feature_circular_extrusion_brep_count".to_string(),
            feature_circular_extrusion_brep_count,
        );
        coverage.insert(
            "transferred_feature_extrusion_brep_count".to_string(),
            feature_extrusion_brep_count,
        );
        coverage.insert(
            "transferred_part_product_count".to_string(),
            usize::from(transferred_part_product),
        );
        coverage.insert(
            "decoded_feature_skamp_count".to_string(),
            decoded_feature_skamp_count,
        );
        coverage.insert(
            "transferred_feature_skamp_constraint_count".to_string(),
            skamp_constraint_coverage.transferred,
        );
        coverage.insert(
            "transferred_native_feature_skamp_constraint_count".to_string(),
            skamp_constraint_coverage.native,
        );
        coverage.insert(
            "transferred_typed_feature_skamp_constraint_count".to_string(),
            skamp_constraint_coverage.typed(),
        );
        coverage.insert(
            "active_feature_skamp_constraint_count".to_string(),
            skamp_constraint_coverage.active,
        );
        coverage.insert(
            "active_native_feature_skamp_constraint_count".to_string(),
            skamp_constraint_coverage.active_native,
        );
        coverage.insert(
            "active_typed_feature_skamp_constraint_count".to_string(),
            skamp_constraint_coverage.active_typed(),
        );
        coverage.insert(
            "decoded_feature_relation_count".to_string(),
            decoded_feature_relation_count,
        );
        coverage.insert(
            "transferred_feature_relation_constraint_count".to_string(),
            relation_constraint_coverage.transferred,
        );
        coverage.insert(
            "transferred_native_feature_relation_constraint_count".to_string(),
            relation_constraint_coverage.native,
        );
        coverage.insert(
            "transferred_typed_feature_relation_constraint_count".to_string(),
            relation_constraint_coverage.typed(),
        );
        coverage.insert(
            "active_feature_relation_constraint_count".to_string(),
            relation_constraint_coverage.active,
        );
        coverage.insert(
            "active_native_feature_relation_constraint_count".to_string(),
            relation_constraint_coverage.active_native,
        );
        coverage.insert(
            "active_typed_feature_relation_constraint_count".to_string(),
            relation_constraint_coverage.active_typed(),
        );
    }
    let prototype_feature_dependencies = surface_prototype_feature_dependencies(scan);
    let operation_feature_ids = scan
        .features
        .operations
        .iter()
        .map(|operation| operation.feature_id)
        .collect::<BTreeSet<_>>();
    for datum in &scan.planes.datums {
        if operation_feature_ids.contains(&datum.feature_id) {
            continue;
        }
        let id = IrFeatureId(format!("creo:model:feature#{}", datum.feature_id));
        if ir.model.features.iter().any(|feature| feature.id == id) {
            continue;
        }
        annotate(
            &mut annotations,
            &id,
            "ActDatums",
            datum.offset_in_payload as u64,
            "datum_plane_feature",
            Exactness::Derived,
        );
        ir.model.features.push(Feature {
            id,
            ordinal: ir.model.features.len() as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: if unique_feature_datum_plane(&scan.planes.datums, datum.feature_id)
                .is_some()
            {
                datum_plane_feature_definition(datum)
            } else {
                IrFeatureDefinition::DatumPlaneUnresolved
            },
            native_ref: None,
        });
    }
    let operation_ordinal_base = ir.model.features.len();
    for (operation_index, operation) in scan.features.operations.iter().enumerate() {
        let id = IrFeatureId(format!("creo:model:feature#{}", operation.feature_id));
        let current_operation =
            current_feature_operation(&scan.features.operations, operation.feature_id);
        let outputs = feature_output_bodies(scan, &ir, operation.feature_id);
        let mut source_properties = feature_source_properties(scan, operation.feature_id);
        if let Some(prefix) = current_operation.and_then(|operation| operation.stored_name_prefix) {
            source_properties.insert(
                "mdl_stored_name_prefix".to_string(),
                char::from(prefix).to_string(),
            );
        }
        let parameters = feature_parameters(scan, operation.feature_id);
        let schema_class = feature_schema_class(scan, operation.feature_id);
        let definition = schema_class.map_or_else(
            || {
                current_feature_recipe(&scan.features.operations, operation.feature_id)
                    .map(|_| {
                        schema_feature_definition(
                            scan,
                            &ir,
                            operation.feature_id,
                            0,
                            &operation.kind,
                        )
                    })
                    .or_else(|| {
                        current_operation.and_then(|operation| {
                            named_or_referenced_feature_definition(
                                scan,
                                &ir,
                                operation.feature_id,
                                &operation.kind,
                            )
                        })
                    })
                    .or_else(|| unbounded_feature_plane_definition(scan, &ir, operation.feature_id))
                    .unwrap_or_else(|| IrFeatureDefinition::Native {
                        kind: current_operation
                            .map_or("Native Feature", |operation| operation.kind.as_str())
                            .to_string(),
                        parameters: parameters.clone(),
                        properties: BTreeMap::new(),
                    })
            },
            |schema_class| {
                schema_feature_definition(
                    scan,
                    &ir,
                    operation.feature_id,
                    schema_class,
                    &operation.kind,
                )
            },
        );
        retain_native_feature_parameters(&mut source_properties, &definition, &parameters);
        let dependencies = feature_dependencies(
            scan,
            &ir,
            operation.feature_id,
            &prototype_feature_dependencies,
        );
        let parent = current_feature_recipe_parent(&scan.features.operations, operation.feature_id)
            .and_then(|parent_feature_id| {
                let parent = IrFeatureId(format!("creo:model:feature#{parent_feature_id}"));
                ir.model
                    .features
                    .iter()
                    .any(|feature| feature.id == parent)
                    .then_some(parent)
            });
        let operation_section = scan
            .framing
            .sections
            .iter()
            .find(|section| {
                operation.offset >= section.offset
                    && operation.offset < section.offset.saturating_add(section.length)
            })
            .map_or("MdlStatus", |section| section.name.as_str());
        let name = current_operation.and_then(|operation| {
            operation.display_name_stored.then_some(())?;
            let stored_name = operation.stored_name.as_deref()?;
            Some(
                operation
                    .stored_name_prefix
                    .and_then(|prefix| stored_name.strip_prefix(char::from(prefix)))
                    .unwrap_or(stored_name)
                    .to_string(),
            )
        });
        let source_tag = current_feature_recipe(&scan.features.operations, operation.feature_id)
            .map(|recipe| recipe.name().to_string());
        let native_ref = owning_feature_definition_ref(scan, operation.feature_id);
        if let Some(existing) = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.id == id)
        {
            if name.is_some() {
                existing.name = name;
            }
            if existing.parent.is_none() {
                existing.parent = parent;
            }
            for dependency in dependencies {
                if !existing.dependencies.contains(&dependency) {
                    existing.dependencies.push(dependency);
                }
            }
            existing.source_properties.extend(source_properties);
            if source_tag.is_some() {
                existing.source_tag = source_tag;
            }
            if existing.native_ref.is_none() {
                existing.native_ref = native_ref;
            }
            for output in outputs {
                if !existing.outputs.contains(&output) {
                    existing.outputs.push(output);
                }
            }
            continue;
        }
        annotate(
            &mut annotations,
            &id,
            operation_section,
            operation.offset as u64,
            if operation.display_name_stored {
                "feature_operation_name"
            } else {
                "feature_recipe"
            },
            Exactness::ByteExact,
        );
        ir.model.features.push(Feature {
            id,
            ordinal: (operation_ordinal_base + operation_index) as u64,
            name,
            suppressed: Some(false),
            parent,
            dependencies,
            source_properties,
            source_tag,
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition,
            native_ref,
        });
    }
    let row_feature_ids = scan
        .features
        .rows
        .iter()
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    for feature_id in row_feature_ids {
        let id = IrFeatureId(format!("creo:model:feature#{feature_id}"));
        if ir.model.features.iter().any(|feature| feature.id == id) {
            continue;
        }
        let schema_class = feature_schema_class(scan, feature_id);
        let Some(offset) = scan
            .features
            .rows
            .iter()
            .filter(|row| row.feature_id == feature_id)
            .map(|row| row.offset)
            .min()
        else {
            continue;
        };
        let reference_name = feature_reference_name(scan, feature_id);
        let kind = reference_name.unwrap_or_else(|| {
            schema_class
                .and_then(schema_operation_kind)
                .unwrap_or("Native Feature")
        });
        annotate(
            &mut annotations,
            &id,
            "AllFeatur",
            offset as u64,
            "schema_feature_operation",
            Exactness::ByteExact,
        );
        let parameters = feature_parameters(scan, feature_id);
        let mut source_properties = feature_source_properties(scan, feature_id);
        let definition = schema_class.map_or_else(
            || {
                named_feature_definition(scan, &ir, feature_id, kind)
                    .or_else(|| unbounded_feature_plane_definition(scan, &ir, feature_id))
                    .unwrap_or_else(|| IrFeatureDefinition::Native {
                        kind: kind.to_string(),
                        parameters: parameters.clone(),
                        properties: BTreeMap::new(),
                    })
            },
            |schema_class| schema_feature_definition(scan, &ir, feature_id, schema_class, kind),
        );
        let row_schema_classes = row_feature_schema_classes(&scan.features.rows, feature_id);
        if schema_class.is_none() {
            source_properties.insert(
                "featdefs_schema_state".to_string(),
                if row_schema_classes.is_empty() {
                    "absent"
                } else {
                    "ambiguous"
                }
                .to_string(),
            );
        }
        if !row_schema_classes.is_empty() {
            source_properties.insert(
                "featdefs_row_schema_classes".to_string(),
                row_schema_classes
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        retain_native_feature_parameters(&mut source_properties, &definition, &parameters);
        ir.model.features.push(Feature {
            id,
            ordinal: ir.model.features.len() as u64,
            name: Some(
                reference_name.map_or_else(|| format!("{kind} id {feature_id}"), str::to_string),
            ),
            suppressed: Some(false),
            parent: None,
            dependencies: feature_dependencies(
                scan,
                &ir,
                feature_id,
                &prototype_feature_dependencies,
            ),
            source_properties,
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: feature_output_bodies(scan, &ir, feature_id),
            definition,
            native_ref: owning_feature_definition_ref(scan, feature_id),
        });
    }
    link_feature_sketch_history(scan, &mut ir);
    reconcile_feature_links(scan, &mut ir, &prototype_feature_dependencies);
    let (transferred_feature_dimension_count, dimension_parameters) =
        transfer_feature_dimensions(scan, &mut ir, &mut annotations);
    let transferred_curve_expression_assignment_count =
        transfer_curve_expression_features(scan, &mut ir, &mut annotations, &dimension_parameters);
    {
        let active_expressions = scan
            .curves
            .expressions
            .iter()
            .filter(|record| !record.backup);
        let decoded_curve_expression_assignment_count = active_expressions
            .clone()
            .map(|record| record.assignments.len())
            .sum::<usize>();
        let evaluated_curve_expression_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.assignments)
            .filter(|assignment| assignment.value.is_some())
            .count();
        let prohibited_curve_expression_record_count = active_expressions
            .clone()
            .filter(|record| !record.prohibited_constructs.is_empty())
            .count();
        let prohibited_curve_expression_kind_count = active_expressions
            .clone()
            .map(|record| record.prohibited_constructs.len())
            .sum::<usize>();
        let activation_count = |activation| {
            active_expressions
                .clone()
                .flat_map(|record| &record.assignments)
                .filter(|assignment| assignment.activation == activation)
                .count()
        };
        coverage.insert(
            "decoded_active_curve_expression_assignment_count".to_string(),
            decoded_curve_expression_assignment_count,
        );
        coverage.insert(
            "transferred_curve_expression_parameter_count".to_string(),
            transferred_curve_expression_assignment_count,
        );
        coverage.insert(
            "evaluated_active_curve_expression_assignment_count".to_string(),
            evaluated_curve_expression_assignment_count,
        );
        coverage.insert(
            "prohibited_active_curve_expression_record_count".to_string(),
            prohibited_curve_expression_record_count,
        );
        coverage.insert(
            "prohibited_active_curve_expression_kind_count".to_string(),
            prohibited_curve_expression_kind_count,
        );
        for (name, activation) in [
            ("active", crate::curve::CurveExpressionActivation::Active),
            (
                "inactive",
                crate::curve::CurveExpressionActivation::Inactive,
            ),
            (
                "conditional",
                crate::curve::CurveExpressionActivation::Conditional,
            ),
        ] {
            coverage.insert(
                format!("{name}_curve_expression_assignment_count"),
                activation_count(activation),
            );
        }
        let (decoded_dimension_count, resolved_dimension_count) = scan
            .features
            .definitions
            .iter()
            .filter_map(|definition| definition.dimensions.as_ref())
            .flat_map(|table| &table.rows)
            .fold((0usize, 0usize), |(decoded, resolved), dimension| {
                (
                    decoded + 1,
                    resolved + usize::from(dimension.value.is_some()),
                )
            });
        coverage.insert(
            "decoded_feature_dimension_count".to_string(),
            decoded_dimension_count,
        );
        coverage.insert(
            "transferred_feature_dimension_parameter_count".to_string(),
            transferred_feature_dimension_count,
        );
        coverage.insert(
            "resolved_feature_dimension_value_count".to_string(),
            resolved_dimension_count,
        );
    }
    close_sketch_constraint_parameter_references(&mut ir);
    attach_expanded_sections(scan, &mut ir, &mut annotations)?;
    let surface_rows = surface_row_records(scan, &scan.surfaces.rows, "visibgeom");
    emit_arena(
        &mut ir,
        &mut annotations,
        "surface_rows",
        &surface_rows,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "surface_namespace_row",
                Exactness::ByteExact,
            );
        },
    )?;
    let nonvisible_surface_rows =
        surface_row_records(scan, &scan.surfaces.nonvisible_rows, "novisgeom");
    emit_arena(
        &mut ir,
        &mut annotations,
        "nonvisible_surface_rows",
        &nonvisible_surface_rows,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "nonvisible_surface_namespace_row",
                Exactness::ByteExact,
            );
        },
    )?;
    let cross_section_surface_rows = surface_row_records(
        scan,
        &scan.surfaces.cross_section_rows,
        "cross_section_geometry",
    );
    emit_arena(
        &mut ir,
        &mut annotations,
        "cross_section_surface_rows",
        &cross_section_surface_rows,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "cross_section_surface_namespace_row",
                Exactness::ByteExact,
            );
        },
    )?;
    let surface_prototypes =
        surface_prototype_records(scan, &scan.surfaces.prototype_records, "visibgeom");
    emit_arena(
        &mut ir,
        &mut annotations,
        "surface_prototypes",
        &surface_prototypes,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "surface_prototype_record",
                Exactness::ByteExact,
            );
        },
    )?;
    let nonvisible_surface_prototypes = surface_prototype_records(
        scan,
        &scan.surfaces.nonvisible_prototype_records,
        "novisgeom",
    );
    emit_arena(
        &mut ir,
        &mut annotations,
        "nonvisible_surface_prototypes",
        &nonvisible_surface_prototypes,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "nonvisible_surface_prototype_record",
                Exactness::ByteExact,
            );
        },
    )?;
    let tabulated_cylinder_curve_replays = tabulated_cylinder_curve_replay_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "tabulated_cylinder_curve_replays",
        &tabulated_cylinder_curve_replays,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "tabulated_cylinder_curve_replay",
                Exactness::ByteExact,
            );
        },
    )?;
    let curve_parameters = curve_parameter_records(scan, &scan.curves.parameters, "visibgeom");
    emit_arena(
        &mut ir,
        &mut annotations,
        "curve_parameters",
        &curve_parameters,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "curve_parameter_record",
                Exactness::ByteExact,
            );
        },
    )?;
    let nonvisible_curve_parameters =
        curve_parameter_records(scan, &scan.curves.nonvisible_parameters, "novisgeom");
    emit_arena(
        &mut ir,
        &mut annotations,
        "nonvisible_curve_parameters",
        &nonvisible_curve_parameters,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "nonvisible_curve_parameter_record",
                Exactness::ByteExact,
            );
        },
    )?;
    let fc_curve_coordinates = fc_curve_coordinate_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "fc_curve_coordinates",
        &fc_curve_coordinates,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "fc_curve_coordinates",
                Exactness::ByteExact,
            );
        },
    )?;
    let fc05_circles = fc05_circle_records(scan);
    store_arena(&mut ir, "fc05_circles", &fc05_circles)?;
    let fc05_cylinder_cap_pairs = fc05_cylinder_cap_pair_records(scan);
    store_arena(&mut ir, "fc05_cylinder_cap_pairs", &fc05_cylinder_cap_pairs)?;
    let prototype_pcurves = prototype_pcurve_records(scan);
    store_arena(&mut ir, "prototype_pcurves", &prototype_pcurves)?;
    let curve_prototype_topology = curve_prototype_topology_records(scan);
    store_arena(
        &mut ir,
        "curve_prototype_topology",
        &curve_prototype_topology,
    )?;
    let curve_prototypes =
        curve_prototype_records(scan, &scan.curves.prototypes, "creo:curve:prototype");
    emit_arena(
        &mut ir,
        &mut annotations,
        "curve_prototypes",
        &curve_prototypes,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "curve_prototype",
                Exactness::ByteExact,
            );
        },
    )?;
    let nonvisible_curve_prototypes = curve_prototype_records(
        scan,
        &scan.curves.nonvisible_prototypes,
        "creo:novisgeom:curve_prototype",
    );
    emit_arena(
        &mut ir,
        &mut annotations,
        "nonvisible_curve_prototypes",
        &nonvisible_curve_prototypes,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "nonvisible_curve_prototype",
                Exactness::ByteExact,
            );
        },
    )?;
    let cross_section_curve_prototypes = curve_prototype_records(
        scan,
        &scan.curves.cross_section_prototypes,
        "creo:cross_section_geometry:curve_prototype",
    );
    emit_arena(
        &mut ir,
        &mut annotations,
        "cross_section_curve_prototypes",
        &cross_section_curve_prototypes,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "cross_section_curve_prototype",
                Exactness::ByteExact,
            );
        },
    )?;
    let curve_topology_rows =
        curve_topology_row_records(scan, &scan.curves.topology_rows, "visibgeom");
    emit_arena(
        &mut ir,
        &mut annotations,
        "curve_topology_rows",
        &curve_topology_rows,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "curve_topology_row",
                Exactness::ByteExact,
            );
        },
    )?;
    let nonvisible_curve_topology_rows =
        curve_topology_row_records(scan, &scan.curves.nonvisible_topology_rows, "novisgeom");
    emit_arena(
        &mut ir,
        &mut annotations,
        "nonvisible_curve_topology_rows",
        &nonvisible_curve_topology_rows,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "nonvisible_curve_topology_row",
                Exactness::ByteExact,
            );
        },
    )?;
    let cross_section_curve_rows = cross_section_curve_row_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "cross_section_curve_rows",
        &cross_section_curve_rows,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "cross_section_curve_row",
                Exactness::ByteExact,
            );
        },
    )?;
    let half_edges = half_edge_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "half_edges",
        &half_edges,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "native_half_edge",
                Exactness::Derived,
            );
        },
    )?;
    let native_loops = loop_records(scan);
    store_arena(&mut ir, "loops", &native_loops)?;
    let topological_vertices = topological_vertex_records(scan);
    store_arena(&mut ir, "topological_vertices", &topological_vertices)?;
    let half_edge_vertex_incidence = half_edge_vertex_incidence_records(scan);
    store_arena(
        &mut ir,
        "half_edge_vertex_incidence",
        &half_edge_vertex_incidence,
    )?;
    let face_components = face_component_records(scan);
    store_arena(&mut ir, "face_components", &face_components)?;
    let surface_parameters = surface_parameter_records(
        scan,
        &scan.surfaces.rows,
        &scan.surfaces.parameters,
        "visibgeom",
    );
    emit_arena(
        &mut ir,
        &mut annotations,
        "surface_parameters",
        &surface_parameters,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.body_offset as u64,
                "surface_parameter_frame",
                Exactness::ByteExact,
            );
        },
    )?;
    let nonvisible_surface_parameters = surface_parameter_records(
        scan,
        &scan.surfaces.nonvisible_rows,
        &scan.surfaces.nonvisible_parameters,
        "novisgeom",
    );
    emit_arena(
        &mut ir,
        &mut annotations,
        "nonvisible_surface_parameters",
        &nonvisible_surface_parameters,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.body_offset as u64,
                "nonvisible_surface_parameter_frame",
                Exactness::ByteExact,
            );
        },
    )?;
    let cross_section_surface_parameters = surface_parameter_records(
        scan,
        &scan.surfaces.cross_section_rows,
        &scan.surfaces.cross_section_parameters,
        "cross_section_geometry",
    );
    emit_arena(
        &mut ir,
        &mut annotations,
        "cross_section_surface_parameters",
        &cross_section_surface_parameters,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.body_offset as u64,
                "cross_section_surface_parameter_frame",
                Exactness::ByteExact,
            );
        },
    )?;
    let plane_local_systems = plane_local_system_records(
        scan,
        &scan.planes.local_systems,
        "creo:surface:plane_local_system",
    );
    store_arena(&mut ir, "plane_local_systems", &plane_local_systems)?;
    let cross_section_plane_local_systems = plane_local_system_records(
        scan,
        &scan.planes.cross_section_local_systems,
        "creo:cross_section_geometry:plane_local_system",
    );
    store_arena(
        &mut ir,
        "cross_section_plane_local_systems",
        &cross_section_plane_local_systems,
    )?;
    let plane_envelopes =
        plane_envelope_records(scan, &scan.planes.envelopes, "creo:surface:plane_envelope");
    store_arena(&mut ir, "plane_envelopes", &plane_envelopes)?;
    let cross_section_plane_envelopes = plane_envelope_records(
        scan,
        &scan.planes.cross_section_envelopes,
        "creo:cross_section_geometry:plane_envelope",
    );
    store_arena(
        &mut ir,
        "cross_section_plane_envelopes",
        &cross_section_plane_envelopes,
    )?;
    let outline_planes =
        outline_plane_records(scan, &scan.planes.outlines, "creo:surface:outline_plane");
    store_arena(&mut ir, "outline_planes", &outline_planes)?;
    let positional_frame_planes = outline_plane_records(
        scan,
        &scan.planes.positional_frames,
        "creo:surface:positional_frame_plane",
    );
    store_arena(&mut ir, "positional_frame_planes", &positional_frame_planes)?;
    let cross_section_outline_planes = outline_plane_records(
        scan,
        &scan.planes.cross_section_outlines,
        "creo:cross_section_geometry:outline_plane",
    );
    store_arena(
        &mut ir,
        "cross_section_outline_planes",
        &cross_section_outline_planes,
    )?;
    let datum_planes = datum_plane_records(scan);
    store_arena(&mut ir, "datum_planes", &datum_planes)?;
    let feature_section_transforms = feature_section_transform_records(scan);
    store_arena(
        &mut ir,
        "feature_section_transforms",
        &feature_section_transforms,
    )?;
    let feature_placement_instructions = feature_placement_instruction_records(scan);
    store_arena(
        &mut ir,
        "feature_placement_instructions",
        &feature_placement_instructions,
    )?;
    // Bespoke annotation: the arena payload drops the per-record source offset the
    // annotation needs, so the offset travels alongside each record in a tuple.
    let pcurve_endpoints = pcurve_endpoint_records(scan);
    for (record, offset) in &pcurve_endpoints {
        annotate(
            &mut annotations,
            &record.id,
            "VisibGeom",
            *offset as u64,
            "pcurve_endpoint_frames",
            Exactness::Derived,
        );
    }
    let pcurve_endpoint_payload = pcurve_endpoints
        .iter()
        .map(|(record, _)| record)
        .collect::<Vec<_>>();
    store_arena(&mut ir, "pcurve_endpoints", &pcurve_endpoint_payload)?;
    let feature_definitions = feature_definition_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_definitions",
        &feature_definitions,
        |annotations, definition| {
            annotate(
                annotations,
                &definition.id,
                &definition.source_section,
                definition.offset as u64,
                "feature_definition_record",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_entities = feature_entity_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_entities",
        &feature_entities,
        |annotations, entity| {
            annotate(
                annotations,
                &entity.id,
                "AllFeatur",
                entity.offset as u64,
                "feature_entity",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_entity_references = feature_entity_reference_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_entity_references",
        &feature_entity_references,
        |annotations, reference| {
            annotate(
                annotations,
                &reference.id,
                "AllFeatur",
                reference.offset as u64,
                "feature_entity_reference",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_entity_tables = feature_entity_table_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_entity_tables",
        &feature_entity_tables,
        |annotations, table| {
            annotate(
                annotations,
                &table.id,
                "AllFeatur",
                table.offset as u64,
                "feature_entity_table",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_surface_replays = feature_surface_replay_associations(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_surface_replays",
        &feature_surface_replays,
        |annotations, association| {
            annotate(
                annotations,
                &association.id,
                "AllFeatur",
                association.table_offset as u64,
                "feature_surface_replay_association",
                Exactness::Derived,
            );
        },
    )?;
    let feature_geometry_tables = feature_geometry_table_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_geometry_tables",
        &feature_geometry_tables,
        |annotations, table| {
            annotate(
                annotations,
                &table.id,
                &table.source_section,
                table.offset as u64,
                "feature_geometry_table",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_loop_history_entries = feature_loop_history_entry_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_loop_history_entries",
        &feature_loop_history_entries,
        |annotations, entry| {
            annotate(
                annotations,
                &entry.id,
                &entry.source_section,
                entry.offset as u64,
                "feature_loop_history_entry",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_affected_ids = feature_affected_id_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_affected_ids",
        &feature_affected_ids,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_affected_ids",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_replay_affected_ids = feature_replay_affected_id_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_replay_affected_ids",
        &feature_replay_affected_ids,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_replay_affected_ids",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_loop_restore_directions = feature_loop_restore_direction_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_loop_restore_directions",
        &feature_loop_restore_directions,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_loop_restore_direction",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_revolution_extents = feature_revolution_extent_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_revolution_extents",
        &feature_revolution_extents,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_revolution_extent",
                Exactness::Derived,
            );
        },
    )?;
    let feature_rows = feature_row_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_rows",
        &feature_rows,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_row",
                Exactness::ByteExact,
            );
        },
    )?;
    let depdb_recipe_rows = depdb_recipe_row_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "depdb_recipe_rows",
        &depdb_recipe_rows,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "depdb_recipe_row",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_choices = feature_choice_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_choices",
        &feature_choices,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_choice",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_choice_fields = feature_choice_field_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_choice_fields",
        &feature_choice_fields,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_choice_field",
                Exactness::ByteExact,
            );
        },
    )?;
    let sketches = sketch_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "sketches",
        &sketches,
        |annotations, sketch| {
            annotate(
                annotations,
                &sketch.id,
                &sketch.source_section,
                sketch.offset as u64,
                "feature_sketch",
                Exactness::Derived,
            );
        },
    )?;
    // Bespoke annotation: the source offset comes from the parallel scan rows, not
    // the record, so annotation zips the two before the arena is stored.
    let curve_expressions = curve_expression_records(scan);
    for (expression, source) in curve_expressions.iter().zip(&scan.curves.expressions) {
        annotate(
            &mut annotations,
            &expression.id,
            "DEPDB_DATA",
            source.expression_offset as u64,
            "curve_expression_program",
            Exactness::ByteExact,
        );
    }
    store_arena(&mut ir, "curve_expressions", &curve_expressions)?;
    let feature_operation_states = feature_operation_state_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_operation_states",
        &feature_operation_states,
        |annotations, state| {
            let section = scan
                .framing
                .sections
                .iter()
                .find(|section| {
                    state.state_offset >= section.offset
                        && state.state_offset < section.offset.saturating_add(section.length)
                })
                .map_or("MdlStatus", |section| section.name.as_str());
            annotate(
                annotations,
                &state.id,
                section,
                state.state_offset as u64,
                "feature_operation_state",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_reference_names = feature_reference_name_records(scan);
    emit_arena(
        &mut ir,
        &mut annotations,
        "feature_reference_names",
        &feature_reference_names,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                "MdlRefInfo",
                record.offset as u64,
                "feature_reference_name",
                Exactness::ByteExact,
            );
        },
    )?;
    if let Some(family_table) = family_table_record(scan) {
        annotate(
            &mut annotations,
            family_table.id,
            "FamilyInf",
            family_table.offset as u64,
            "configuration_driver_table_pointer",
            Exactness::ByteExact,
        );
        store_arena(&mut ir, "configuration", &[family_table])?;
    }
    let native_feature_count = ir
        .model
        .features
        .iter()
        .filter(|feature| matches!(feature.definition, IrFeatureDefinition::Native { .. }))
        .count();
    coverage.insert(
        "transferred_feature_count".to_string(),
        ir.model.features.len(),
    );
    coverage.insert(
        "transferred_typed_feature_count".to_string(),
        ir.model.features.len() - native_feature_count,
    );
    coverage.insert(
        "transferred_native_feature_count".to_string(),
        native_feature_count,
    );
    Ok((ir, annotations.build(), unknowns, coverage))
}

#[derive(Default)]
struct TorusParameterCoverage {
    radius_overrides: usize,
    outline_extents: usize,
    five_coordinate_envelopes: usize,
    split_coordinate_envelopes: usize,
}

fn torus_parameter_coverage(scan: &ContainerScan) -> TorusParameterCoverage {
    let rows = scan.surfaces.parameters.iter().filter_map(|record| {
        crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
            .map(|row| (record, row))
    });
    TorusParameterCoverage {
        radius_overrides: rows
            .clone()
            .filter(|(record, row)| record.torus_radius_overrides(row.type_byte).is_some())
            .count(),
        outline_extents: rows
            .clone()
            .filter(|(record, row)| record.torus_outline_frame(row.type_byte).is_some())
            .count(),
        five_coordinate_envelopes: rows
            .clone()
            .filter(|(record, row)| {
                record
                    .type26_five_coordinate_envelope(row.type_byte)
                    .is_some()
            })
            .count(),
        split_coordinate_envelopes: rows
            .filter(|(record, row)| {
                record
                    .type26_split_coordinate_envelope(row.type_byte)
                    .is_some()
            })
            .count(),
    }
}

fn source_meta(scan: &ContainerScan) -> (SourceMeta, BTreeMap<String, usize>) {
    let mut attributes = BTreeMap::new();
    let mut coverage = BTreeMap::new();
    attributes.insert(
        "version_line".to_string(),
        scan.framing.version_line.clone(),
    );
    if let Some(name) = &scan.framing.model_name {
        attributes.insert("model_name".to_string(), name.clone());
    }
    attributes.insert(
        "layout".to_string(),
        scan.framing.layout.token().to_string(),
    );
    attributes.insert("file_size".to_string(), scan.framing.data.len().to_string());
    attributes.insert(
        "section_count".to_string(),
        scan.framing.sections.len().to_string(),
    );
    for (index, section) in scan.framing.sections.iter().enumerate() {
        let prefix = format!("section.{index}");
        attributes.insert(format!("{prefix}.name"), section.name.clone());
        attributes.insert(format!("{prefix}.raw_name"), section.raw_name.clone());
        attributes.insert(format!("{prefix}.role"), section.role.to_string());
        attributes.insert(format!("{prefix}.offset"), section.offset.to_string());
        attributes.insert(format!("{prefix}.length"), section.length.to_string());
    }
    if let Some(c) = scan.framing.census.srf_array_count {
        attributes.insert("srf_array_count".to_string(), c.to_string());
    }
    if let Some(c) = scan.framing.census.crv_array_count {
        attributes.insert("crv_array_count".to_string(), c.to_string());
    }
    if let Some(unit) = &scan.framing.principal_unit {
        attributes.insert("principal_unit".to_string(), unit.clone());
    }
    coverage.insert(
        "decoded_surface_row_count".to_string(),
        scan.surfaces.rows.len(),
    );
    coverage.insert(
        "decoded_cross_section_surface_row_count".to_string(),
        scan.surfaces.cross_section_rows.len(),
    );
    coverage.insert(
        "decoded_surface_parameter_record_count".to_string(),
        scan.surfaces.parameters.len(),
    );
    coverage.insert(
        "decoded_cross_section_surface_parameter_record_count".to_string(),
        scan.surfaces.cross_section_parameters.len(),
    );
    coverage.insert(
        "decoded_positional_extrusion_direction_count".to_string(),
        scan.surfaces
            .parameters
            .iter()
            .filter(|record| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
                    .is_some_and(|row| {
                        row.kind == crate::surface::SurfaceKind::Extrusion
                            && record.extrusion_direction(row.type_byte).is_some()
                    })
            })
            .count(),
    );
    let torus_coverage = torus_parameter_coverage(scan);
    coverage.insert(
        "decoded_torus_radius_override_count".to_string(),
        torus_coverage.radius_overrides,
    );
    coverage.insert(
        "decoded_torus_outline_extent_count".to_string(),
        torus_coverage.outline_extents,
    );
    coverage.insert(
        "decoded_type26_five_coordinate_envelope_count".to_string(),
        torus_coverage.five_coordinate_envelopes,
    );
    coverage.insert(
        "decoded_type26_split_coordinate_envelope_count".to_string(),
        torus_coverage.split_coordinate_envelopes,
    );
    coverage.insert(
        "decoded_plane_local_system_count".to_string(),
        scan.planes.local_systems.len(),
    );
    coverage.insert(
        "decoded_cross_section_plane_local_system_count".to_string(),
        scan.planes.cross_section_local_systems.len(),
    );
    coverage.insert(
        "decoded_plane_envelope_count".to_string(),
        scan.planes.envelopes.len(),
    );
    coverage.insert(
        "decoded_cross_section_plane_envelope_count".to_string(),
        scan.planes.cross_section_envelopes.len(),
    );
    coverage.insert(
        "decoded_outline_plane_count".to_string(),
        scan.planes.outlines.len(),
    );
    coverage.insert(
        "decoded_positional_frame_plane_count".to_string(),
        scan.planes.positional_frames.len(),
    );
    coverage.insert(
        "decoded_cross_section_outline_plane_count".to_string(),
        scan.planes.cross_section_outlines.len(),
    );
    coverage.insert(
        "decoded_surface_prototype_count".to_string(),
        scan.surfaces.prototypes.len(),
    );
    coverage.insert(
        "decoded_named_surface_prototype_count".to_string(),
        scan.surfaces.prototype_records.len(),
    );
    coverage.insert(
        "decoded_reference_line_count".to_string(),
        scan.references.lines.len(),
    );
    coverage.insert(
        "decoded_reference_circle_count".to_string(),
        scan.references.circles.len(),
    );
    coverage.insert(
        "decoded_reference_conic_count".to_string(),
        scan.references.conics.len(),
    );
    coverage.insert(
        "transferred_reference_ellipse_count".to_string(),
        scan.references.ellipses.len(),
    );
    coverage.insert(
        "decoded_tabulated_cylinder_curve_replay_count".to_string(),
        scan.curves.tabulated_cylinder_replays.len(),
    );
    coverage.insert(
        "decoded_tabulated_cylinder_control_point_set_count".to_string(),
        scan.curves
            .tabulated_cylinder_replays
            .iter()
            .filter(|replay| replay.control_points.iter().all(Option::is_some))
            .count(),
    );
    coverage.insert(
        "decoded_curve_prototype_count".to_string(),
        scan.curves.prototypes.len(),
    );
    coverage.insert(
        "decoded_curve_parameter_record_count".to_string(),
        scan.curves.parameters.len(),
    );
    coverage.insert(
        "decoded_curve_expression_record_count".to_string(),
        scan.curves.expressions.len(),
    );
    attributes.insert(
        "expanded_section_count".to_string(),
        scan.framing.expanded_sections.len().to_string(),
    );
    attributes.insert(
        "expanded_section_byte_count".to_string(),
        scan.framing
            .expanded_sections
            .iter()
            .map(|section| section.data.len())
            .sum::<usize>()
            .to_string(),
    );
    if let Some(family_table) = scan.framing.family_table {
        attributes.insert(
            "family_table_pointer".to_string(),
            match family_table.pointer {
                crate::container::FamilyTablePointer::Null => "null".to_string(),
                crate::container::FamilyTablePointer::Entity(id) => format!("entity:{id}"),
            },
        );
        attributes.insert(
            "configuration_state".to_string(),
            match family_table.pointer {
                crate::container::FamilyTablePointer::Null => "none".to_string(),
                crate::container::FamilyTablePointer::Entity(_) => {
                    "driver_table_unresolved".to_string()
                }
            },
        );
    }
    coverage.insert(
        "decoded_pcurve_count".to_string(),
        scan.curves.pcurves.len(),
    );
    coverage.insert(
        "decoded_fc_curve_coordinate_record_count".to_string(),
        scan.curves.fc_coordinates.len(),
    );
    coverage.insert(
        "decoded_fc05_circle_count".to_string(),
        scan.curves.fc05_circles.len(),
    );
    coverage.insert(
        "decoded_fc05_cylinder_cap_pair_count".to_string(),
        scan.curves.fc05_cylinder_cap_pairs.len(),
    );
    coverage.insert(
        "decoded_prototype_pcurve_count".to_string(),
        scan.curves.prototype_pcurves.len(),
    );
    coverage.insert(
        "decoded_curve_prototype_topology_count".to_string(),
        scan.curves.prototype_topology.len(),
    );
    coverage.insert(
        "decoded_bound_prototype_pcurve_count".to_string(),
        scan.curves.bound_prototype_pcurves.len(),
    );
    coverage.insert(
        "decoded_curve_topology_row_count".to_string(),
        scan.curves.topology_rows.len(),
    );
    coverage.insert(
        "decoded_cross_section_curve_row_count".to_string(),
        scan.curves.cross_section_rows.len(),
    );
    coverage.insert(
        "decoded_cross_section_curve_prototype_count".to_string(),
        scan.curves.cross_section_prototypes.len(),
    );
    coverage.insert(
        "decoded_half_edge_count".to_string(),
        scan.topology.half_edges.len(),
    );
    coverage.insert(
        "decoded_topological_vertex_count".to_string(),
        scan.topology.vertices.len(),
    );
    coverage.insert("decoded_loop_count".to_string(), scan.topology.loops.len());
    coverage.insert(
        "decoded_face_component_count".to_string(),
        scan.topology.face_components.len(),
    );
    coverage.insert(
        "decoded_datum_plane_count".to_string(),
        scan.planes.datums.len(),
    );
    coverage.insert("decoded_feature_count".to_string(), scan.features.ids.len());
    coverage.insert(
        "decoded_feature_row_count".to_string(),
        scan.features.rows.len(),
    );
    coverage.insert(
        "decoded_feature_choice_count".to_string(),
        scan.features.choices.len(),
    );
    coverage.insert(
        "decoded_feature_choice_field_count".to_string(),
        scan.features.choice_fields.len(),
    );
    coverage.insert(
        "decoded_feature_geometry_table_count".to_string(),
        scan.features.geometry_tables.len(),
    );
    coverage.insert(
        "decoded_feature_loop_history_entry_count".to_string(),
        scan.features.loop_history_entries.len(),
    );
    coverage.insert(
        "decoded_feature_affected_id_array_count".to_string(),
        scan.features.affected_ids.len(),
    );
    coverage.insert(
        "decoded_feature_replay_affected_id_count".to_string(),
        scan.features.replay_affected_ids.len(),
    );
    coverage.insert(
        "decoded_feature_loop_restore_direction_count".to_string(),
        scan.features.loop_restore_directions.len(),
    );
    coverage.insert(
        "decoded_feature_revolution_extent_count".to_string(),
        scan.features.revolution_extents.len(),
    );
    coverage.insert(
        "decoded_feature_definition_count".to_string(),
        scan.features.definitions.len(),
    );
    coverage.insert(
        "decoded_feature_section_transform_count".to_string(),
        scan.features.section_transforms.len(),
    );
    coverage.insert(
        "decoded_feature_placement_instruction_count".to_string(),
        scan.features
            .definitions
            .iter()
            .map(|definition| crate::feature::placement_instructions(definition).len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_operation_state_count".to_string(),
        scan.features.operation_states.len(),
    );
    coverage.insert(
        "decoded_feature_operation_count".to_string(),
        scan.features.operations.len(),
    );
    coverage.insert(
        "decoded_feature_outline_count".to_string(),
        scan.features
            .definitions
            .iter()
            .map(|definition| definition.outlines.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_section_point_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .map(|variables| {
                let (points, ambiguous) = variables.reconciled_points();
                points.len() + ambiguous.len()
            })
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_solver_variable_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .map(|variables| variables.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_dimension_driven_variable_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .flat_map(|variables| &variables.rows)
            .filter(|row| row.dimension_driven)
            .count(),
    );
    coverage.insert(
        "resolved_feature_dimension_driven_variable_count".to_string(),
        scan.features
            .definitions
            .iter()
            .map(|definition| {
                let resolved = resolved_section_coordinates(definition);
                definition
                    .variables
                    .iter()
                    .flat_map(|variables| &variables.rows)
                    .filter(|row| {
                        row.dimension_driven
                            && matches!(row.variable_type, 1 | 2)
                            && resolved.get(&row.key).is_some_and(|coordinates| {
                                coordinates[usize::from(row.variable_type == 2)].is_some()
                            })
                    })
                    .count()
            })
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_opaque_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.opaque_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_trim_entity_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.trim_entities.as_ref())
            .map(|entities| entities.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_trim_vertex_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.trim_vertices.as_ref())
            .map(|vertices| vertices.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_order_entry_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.order_table.as_ref())
            .map(|order| order.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_dimension_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.dimensions.as_ref())
            .map(|dimensions| dimensions.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_relation_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.relations.as_ref())
            .map(|relations| relations.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_saved_entity_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.saved_section.as_ref())
            .map(|saved| saved.entities.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_entity_count".to_string(),
        scan.features.entities.len(),
    );
    coverage.insert(
        "decoded_feature_entity_reference_count".to_string(),
        scan.features.entity_references.len(),
    );
    coverage.insert(
        "decoded_feature_entity_table_count".to_string(),
        scan.features.entity_tables.len(),
    );
    coverage.insert(
        "decoded_feature_surface_replay_association_count".to_string(),
        feature_surface_replay_associations(scan).len(),
    );
    if let Some(count) = scan.framing.declared_body_count {
        attributes.insert("declared_body_count".to_string(), count.to_string());
    }
    if let Some(value) = scan.framing.first_quilt_ptr {
        attributes.insert("first_quilt_ptr".to_string(), value.to_string());
    }
    (
        SourceMeta {
            format: "creo".to_string(),
            attributes,
        },
        coverage,
    )
}

fn has_transferred_geometry(ir: &CadIr) -> bool {
    let model = &ir.model;
    !model.points.is_empty()
        || !model.vertices.is_empty()
        || !model.edges.is_empty()
        || !model.coedges.is_empty()
        || !model.loops.is_empty()
        || !model.faces.is_empty()
        || !model.shells.is_empty()
        || !model.regions.is_empty()
        || !model.bodies.is_empty()
        || model
            .surfaces
            .iter()
            .any(|surface| !matches!(&surface.geometry, SurfaceGeometry::Unknown { .. }))
        || model
            .curves
            .iter()
            .any(|curve| !matches!(&curve.geometry, CurveGeometry::Unknown { .. }))
        || !model.subds.is_empty()
        || !model.pcurves.is_empty()
        || model.procedural_surfaces.iter().any(|surface| {
            !matches!(
                &surface.definition,
                ProceduralSurfaceDefinition::Unknown { .. }
            )
        })
        || model
            .procedural_curves
            .iter()
            .any(|curve| !matches!(&curve.definition, ProceduralCurveDefinition::Unknown { .. }))
        || model
            .sketch_entities
            .iter()
            .any(|entity| !matches!(&entity.geometry, SketchGeometry::Native { .. }))
        || !model.tessellations.is_empty()
}

/// Build diagnostics for data that cannot be represented in the emitted IR.
fn build_report(
    scan: &ContainerScan,
    ir: &CadIr,
    coverage: BTreeMap<String, usize>,
    container_only: bool,
) -> DecodeReport {
    let count = |key: &str| coverage.get(key).copied().unwrap_or(0);
    let summary = container::summarize(scan);
    let geom_sections = scan
        .framing
        .sections
        .iter()
        .filter(|s| s.role == role::GEOMETRY)
        .count();
    let mut placed_plane_ids = scan
        .planes
        .local_systems
        .iter()
        .filter(|frame| {
            frame.origin.is_some()
                && frame.u_axis.is_some()
                && frame.normal.is_some_and(|normal| !is_axis_aligned(normal))
        })
        .map(|frame| frame.surface_id)
        .collect::<BTreeSet<_>>();
    placed_plane_ids.extend(scan.planes.outlines.iter().map(|plane| plane.surface_id));
    placed_plane_ids.extend(
        scan.planes
            .positional_frames
            .iter()
            .map(|plane| plane.surface_id),
    );
    let placed_plane_count = placed_plane_ids.len();
    let first_instance_prototype_surface_count =
        count("transferred_first_instance_prototype_surface_count");
    let positional_line_extrusion_plane_count =
        count("transferred_positional_line_extrusion_plane_count");
    let tabulated_cylinder_spline_extrusion_count =
        count("transferred_tabulated_cylinder_spline_extrusion_count");
    let positional_cone_count = count("transferred_positional_cone_count");
    let positional_cylinder_count = count("transferred_positional_cylinder_count");
    let paired_envelope_sphere_count = count("transferred_paired_envelope_sphere_count");
    let positional_torus_count = count("transferred_positional_torus_count");
    let mut losses = Vec::new();

    if container_only {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::ContainerOnly,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: "Container-only decode requested; entity transfer was skipped.".to_string(),
            provenance: None,
        });
    }

    // The namespace census: what is byte-backed and readable.
    let srf = scan
        .framing
        .census
        .srf_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    let crv = scan
        .framing
        .census
        .crv_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossCode::CarrierSummary,
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
            "PSB container decoded structurally: {} section(s), {} layout, VisibGeom namespace \
             census srf_array={srf} / crv_array={crv}; {} typed surface rows, {} labeled curve \
             prototypes, {} canonical curve-topology rows, and {} closed native loops were decoded. \
             Outline-backed planes, guarded non-axis support frames, complete ND first-instance \
             plane, cylinder, cone, torus, and interpolation-spline prototypes, unbound straight positional \
             surface-of-extrusion planes, \
             topology-bound `fc 05` \
             cylinders with a resolved axis-normal cap plane, four-entry two-cap and blind \
             circular-sweep cylinders, \
             four-entry simple-hole cylinders with complete cap outlines, radius-anchored \
             class-911 counterbore and bore patches, and compact simple-hole cylinders with \
             complete positional carriers, complementary split-outline cylinders \
             bound to an axis-normal plane, complete positional cylinder bodies, \
             complete support-apex and planar-envelope positional cones, and complete \
             local-system positional tori transfer as carriers; \
             other parameter bodies remain structural records.",
            scan.framing.sections.len(),
            scan.framing.layout.token(),
            scan.surfaces.rows.len(),
            scan.curves.prototypes.len(),
            scan.curves.topology_rows.len(),
            scan.topology.loops.len(),
        ),
        provenance: None,
    });

    // The core prototype-vs-instance limitation.
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: format!(
            "General model B-rep transfer remains incomplete. Native face components transfer \
             when every boundary edge has solved vertex orbits, face orientation is unique, and \
             every loop is complete; a multi-loop planar face additionally requires one strict \
             containment outer boundary. Selected \
             cylinders transfer when an exact `fc 05` record and placed cap outline binds a row, \
             a four-entry class-917 circular-sweep or class-911 simple-hole table with a complete \
             square cap outline establishes the complete axis placement and radius, or a compact \
             class-911 table owns a complete positional cylinder carrier, a class-911 \
             counterbore dimension replay agrees with its generated larger-cylinder carrier, or two same-feature \
             patches have complementary square outline bounds on one axis-normal plane. Later positional \
             instances do not inherit prototype placement or scalar \
             defaults; they require their per-instance parameter bodies \
             ([spec §4.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#32-surface-prototypes)). {geom_sections} PSB geometry section(s) were preserved verbatim as unknown \
             records."
        ),
        provenance: None,
    });

    if !container_only && placed_plane_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {placed_plane_count} model-space plane carrier(s) from complete \
                 VisibGeom local-system support frames."
            ),
            provenance: None,
        });
    }

    if !container_only && first_instance_prototype_surface_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {first_instance_prototype_surface_count} first-instance ND plane, \
                 cylinder, cone, torus, or interpolation-spline carrier(s) from complete named \
                 parameters."
            ),
            provenance: None,
        });
    }

    if !container_only && paired_envelope_sphere_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {paired_envelope_sphere_count} sphere carrier(s) from complementary \
                 five-coordinate type-26 hemisphere envelopes and their shared zero-major-radius \
                 prototype."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_torus_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {positional_torus_count} exact positional torus carrier(s) from \
                 complete local-system, radius, and five-coordinate envelope bodies."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_cylinder_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {positional_cylinder_count} exact positional cylinder carrier(s) \
                 from complete per-instance parameter bodies."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_cone_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {positional_cone_count} exact positional cone carrier(s) from \
                 complete support-apex or planar-envelope bodies."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_line_extrusion_plane_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {positional_line_extrusion_plane_count} unbound straight positional \
                 surface-of-extrusion carrier(s) from complete sweep-direction and directrix \
                 frames."
            ),
            provenance: None,
        });
    }

    if !container_only && tabulated_cylinder_spline_extrusion_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {tabulated_cylinder_spline_extrusion_count} tabulated-cylinder \
                 cubic spline extrusion carrier(s) from uniquely matched directrix and frame spans."
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.planes.datums.is_empty() {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {} exact model-space construction datum plane carrier(s) from ActDatums; \
                 these are unbounded reference planes, not model B-rep faces.",
                scan.planes.datums.len()
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.references.lines.is_empty() {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {} finite model-space reference line carrier(s) from MdlRefInfo; \
                 their byte-exact endpoints remain attached as native line records.",
                scan.references.lines.len()
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.references.circles.is_empty() {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {} circular reference carrier(s) from MdlRefInfo rows whose stored center, radius, and endpoints satisfy the circle equation; byte-exact endpoints remain attached as native circle records.",
                scan.references.circles.len()
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.references.ellipses.is_empty() {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {} elliptical reference carrier(s) from MdlRefInfo conic rows whose frame, coefficient radii, and antipodal endpoints satisfy one ellipse equation; the source conic records remain byte-exact native records.",
                scan.references.ellipses.len()
            ),
            provenance: None,
        });
    }

    let topological_point_count = count("transferred_topological_point_count");
    if !container_only && topological_point_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {topological_point_count} exact model-space point(s) for native topological vertex orbits from unique placed-carrier intersections or pcurve endpoint domains constrained by agreeing face maps and incident analytic edge carriers."
            ),
            provenance: None,
        });
    }

    let native_topological_edge_count = count("transferred_native_topological_edge_count");
    if !container_only && native_topological_edge_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Topology,
            severity: Severity::Info,
            message: format!(
                "Transferred {native_topological_edge_count} native topological edge(s) whose endpoint vertex orbits have exact model-space points."
            ),
            provenance: None,
        });
    }

    let analytic_pcurve_carrier_count = count("transferred_analytic_pcurve_carrier_count");
    if !container_only && analytic_pcurve_carrier_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {analytic_pcurve_carrier_count} exact analytic carrier(s) by mapping native linear pcurves through placed planar, cylindrical, conical, spherical, or toroidal face charts."
            ),
            provenance: None,
        });
    }

    let torus_coverage = torus_parameter_coverage(scan);
    if torus_coverage.radius_overrides != 0
        || torus_coverage.outline_extents != 0
        || torus_coverage.five_coordinate_envelopes != 0
        || torus_coverage.split_coordinate_envelopes != 0
    {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Retained {} tagged type-26 radius override(s), {} terminal outline extent(s), \
                 {} five-coordinate envelope(s), and {} split-coordinate envelope(s). These \
                 row-local fields remain byte-exact native data. Placement-complete paired sphere \
                 envelopes additionally transfer as analytic carriers.",
                torus_coverage.radius_overrides,
                torus_coverage.outline_extents,
                torus_coverage.five_coordinate_envelopes,
                torus_coverage.split_coordinate_envelopes,
            ),
            provenance: None,
        });
    }

    // The specific undecoded PSB layers that gate per-instance geometry.
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: "Additional model-space carriers are gated by unresolved lane-specific scalar \
                  prefixes, feature-local transform bindings, placement-incomplete or untagged \
                  `0x26` torus/sphere variants, and the round/fillet feature evaluator. These gaps \
                  prevent transfer of the remaining non-plane per-instance surfaces, curves, and \
                  vertices."
            .to_string(),
        provenance: None,
    });

    // Topology.
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossCode::TopologyNotTransferred,
        category: LossCategory::Topology,
        severity: Severity::Blocking,
        message: "Native curve half-edges and closed loops were decoded. Components with complete \
                  solved boundaries and unique face orientations transfer as \
                  body/region/shell/face/loop/coedge/edge/vertex graphs; remaining components \
                  require face-instance partitioning, surface parameter bindings, curve geometry, \
                  or vertex coordinates."
            .to_string(),
        provenance: None,
    });

    let configuration_gap = match scan.framing.family_table.map(|record| record.pointer) {
        Some(crate::container::FamilyTablePointer::Null) => "",
        Some(crate::container::FamilyTablePointer::Entity(_)) => {
            ", configuration driver-table rows"
        }
        None => ", configuration presence",
    };
    let prohibited_curve_expression_record_count = scan
        .curves
        .expressions
        .iter()
        .filter(|record| !record.backup && !record.prohibited_constructs.is_empty())
        .count();
    let curve_expression_transfer = if prohibited_curve_expression_record_count == 0 {
        "Curve-equation assignments transfer with their source, dependencies, and closed numeric \
         and string operator and deterministic function values."
            .to_string()
    } else {
        format!(
            "Admitted curve-equation assignments transfer with their source, dependencies, and \
             closed numeric and string operator and deterministic function values. \
             {prohibited_curve_expression_record_count} active curve-equation record(s) containing \
             prohibited datum-curve constructs retain source and dependencies without values or \
             derived curves."
        )
    };

    // Features, history, materials.
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossCode::FeatureHistoryRetained,
        category: LossCategory::Attribute,
        severity: Severity::Warning,
        message: format!(
            "Named feature operations and their decoded dependency/input tables transfer as typed \
             or native design records. {curve_expression_transfer} \
             Full neutral operation semantics\
             {configuration_gap}, graph, case-study, cabling, and cross-model relation functions, \
             materials, and display data \
             remain untransferred."
        ),
        provenance: None,
    });

    // Coverage drops: VisibGeom rows and curve-equation records that decoded
    // but could not be transferred, resolved, or evaluated.
    let untransferred_surface_rows = count("untransferred_visible_surface_row_count");
    if untransferred_surface_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{untransferred_surface_rows} unique VisibGeom surface row(s) were not \
                 transferred as carriers and remain structural namespace records."
            ),
            provenance: None,
        });
    }
    let untransferred_curve_rows = count("untransferred_visible_curve_row_count");
    if untransferred_curve_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{untransferred_curve_rows} unique VisibGeom curve-topology row(s) were not \
                 transferred as carriers and remain structural namespace records."
            ),
            provenance: None,
        });
    }
    let ambiguous_surface_rows = count("ambiguous_visible_surface_row_count");
    if ambiguous_surface_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "{ambiguous_surface_rows} VisibGeom surface row(s) share a non-unique identity \
                 and were not resolved to a single carrier."
            ),
            provenance: None,
        });
    }
    let ambiguous_curve_rows = count("ambiguous_visible_curve_row_count");
    if ambiguous_curve_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "{ambiguous_curve_rows} VisibGeom curve-topology row(s) share a non-unique \
                 identity and were not resolved to a single carrier."
            ),
            provenance: None,
        });
    }
    let active_native_skamps = count("active_native_feature_skamp_constraint_count");
    if active_native_skamps != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::FeatureHistoryRetained,
            category: LossCategory::Attribute,
            severity: Severity::Warning,
            message: format!(
                "{active_native_skamps} active section incidence constraint(s) retain native \
                 operands because their neutral semantics or referenced geometry remain unresolved."
            ),
            provenance: None,
        });
    }
    let active_native_relations = count("active_native_feature_relation_constraint_count");
    if active_native_relations != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::FeatureHistoryRetained,
            category: LossCategory::Attribute,
            severity: Severity::Warning,
            message: format!(
                "{active_native_relations} active section dimension relation(s) retain native \
                 operands because their neutral semantics, incidence join, or referenced geometry \
                 remain unresolved."
            ),
            provenance: None,
        });
    }
    let prohibited_records = count("prohibited_active_curve_expression_record_count");
    if prohibited_records != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::FeatureHistoryRetained,
            category: LossCategory::Attribute,
            severity: Severity::Warning,
            message: format!(
                "{prohibited_records} active curve-equation record(s) containing prohibited \
                 datum-curve constructs were not evaluated; source and dependencies were \
                 retained without values or derived curves."
            ),
            provenance: None,
        });
    }
    let prohibited_kinds = count("prohibited_active_curve_expression_kind_count");
    if prohibited_kinds != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::FeatureHistoryRetained,
            category: LossCategory::Attribute,
            severity: Severity::Warning,
            message: format!(
                "{prohibited_kinds} prohibited datum-curve construct(s) across active \
                 curve-equation records were not evaluated."
            ),
            provenance: None,
        });
    }

    DecodeReport {
        format: "creo".to_string(),
        container_only,
        geometry_transferred: has_transferred_geometry(ir),
        coverage,
        losses,
        notes: summary.notes,
    }
}
