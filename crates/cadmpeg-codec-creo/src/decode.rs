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

use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::features::{
    Angle, BooleanOp, ChamferSpec, DesignParameter, DimensionDisplay, EdgeSelection, Extent,
    FaceSelection, Feature, FeatureDefinition as IrFeatureDefinition, FeatureId as IrFeatureId,
    FeatureSourceContent, FeatureTreeNodeRole, HoleKind, Length, ParameterId, ParameterValue,
    PatternForm, PatternKind, ProfileRef, RadiusSpec, RevolutionAxis, RevolutionConstruction,
    SketchSpace,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchCoordinateAxis,
    SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus,
    SketchNativeOperand,
};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::{Exactness, SourceObjectAssociation};
use serde::Serialize;

use crate::container::{self, role, ContainerScan};
use crate::topology::HalfEdgeId;

fn unique_feature_definition(
    definitions: &[crate::feature::FeatureDefinition],
    definition_id: u32,
) -> Option<&crate::feature::FeatureDefinition> {
    let mut matches = definitions
        .iter()
        .filter(|definition| definition.id == definition_id);
    let definition = matches.next()?;
    matches.next().is_none().then_some(definition)
}

fn unique_feature_section_transform(
    transforms: &[crate::placement::FeatureSectionTransform],
    definition_id: u32,
) -> Option<&crate::placement::FeatureSectionTransform> {
    let mut matches = transforms
        .iter()
        .filter(|transform| transform.definition_id == definition_id);
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

#[derive(Serialize)]
struct CreoSketchRecord {
    id: String,
    definition_id: u32,
    owner_feature_id: Option<u32>,
    source_section: String,
    offset: usize,
    section_3d: Option<CreoSketchSection3d>,
    table_headers: Vec<CreoSketchTableHeader>,
    section_points: Vec<CreoSketchSectionPoint>,
    solved_external_ids: Vec<u32>,
    variables: Vec<CreoSketchVariable>,
    segments: Vec<CreoSketchSegment>,
    trim_entities: Vec<CreoSketchTrimEntity>,
    trim_vertices: Vec<CreoSketchTrimVertex>,
    order_rows: Vec<CreoSketchOrderRow>,
    saved_entities: Vec<CreoSketchSavedEntity>,
    dimensions: Vec<CreoSketchDimension>,
    relations: Vec<CreoSketchRelation>,
    skamps: Vec<CreoSketchSkamp>,
    relation_triples: Vec<CreoSketchRelationTriple>,
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
    row_count: usize,
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
struct CreoFeatureDefinitionRecord {
    id: String,
    definition_id: u32,
    owner_feature_id: Option<u32>,
    source_section: String,
    body: Vec<u8>,
    parameter_frames: Vec<CreoFeatureParameterFrame>,
    outlines: Vec<CreoFeatureOutline>,
    offset: usize,
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
    entities: [u32; 2],
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
struct CreoSketchDimension {
    external_id: u32,
    dimension_type: u32,
    value: Option<f64>,
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
struct CreoCurveExpressionRecord {
    id: String,
    entity_id: u32,
    backup: bool,
    local_system: Option<CreoCurveExpressionLocalSystem>,
    lines: Vec<CreoCurveExpressionLine>,
    assignments: Vec<CreoCurveExpressionAssignment>,
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
    expression: String,
    dependencies: Vec<String>,
    value: Option<f64>,
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
struct CreoFamilyTableRecord {
    id: &'static str,
    pointer_kind: &'static str,
    table_entity_id: Option<u32>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoFeatureEntityRecord {
    id: String,
    entity_id: u32,
    type_byte: u8,
    name: String,
    offset: usize,
}

#[derive(Serialize)]
struct CreoFeatureEntityReferenceRecord {
    id: String,
    source_entity_id: Option<u32>,
    target_entity_id: u32,
    target_resolved: bool,
    offset: usize,
}

#[derive(Serialize)]
struct CreoFeatureEntityTableRecord {
    id: String,
    owner_feature_id: Option<u32>,
    table_class_id: u32,
    entry_ids: Vec<u32>,
    entries: Vec<CreoFeatureEntityTableEntryRecord>,
    surface_ids: Vec<u32>,
    non_surface_entity_ids: Vec<u32>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoFeatureEntityTableEntryRecord {
    entity_id: u32,
    class_id: u32,
    source_entity_id: Option<u32>,
    prefixed: bool,
    offset: usize,
    end_offset: usize,
}

#[derive(Serialize)]
struct CreoFeatureGeometryTableRecord {
    id: String,
    owner_feature_id: u32,
    kind: &'static str,
    declared_count: u32,
    entity_class_id: u32,
    entry_ids: Option<Vec<u32>>,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoFeatureAffectedIdsRecord {
    id: String,
    owner_feature_id: u32,
    kind: &'static str,
    ids: Vec<u32>,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoFeatureReplayAffectedIdsRecord {
    id: String,
    owner_feature_id: u32,
    geometry_ids: Vec<u32>,
    edge_ids: Vec<u32>,
    geometry_extent: &'static str,
    edge_extent: &'static str,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoFeatureLoopRestoreDirectionRecord {
    id: String,
    owner_feature_id: u32,
    lane: &'static str,
    value: u32,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoFeatureRevolutionExtentRecord {
    id: String,
    owner_feature_id: u32,
    kind: &'static str,
    angle_radians: f64,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoFeatureChoiceRecord {
    id: String,
    owner_feature_id: u32,
    label: String,
    type_byte: Option<u8>,
    payload: Vec<u8>,
    payload_offset: usize,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoFeatureRowRecord {
    id: String,
    owner_feature_id: u32,
    header: [u8; 2],
    root_schema_class: Option<u32>,
    stream_offset: usize,
    body: Vec<u8>,
    body_offset: usize,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoFeatureChoiceFieldRecord {
    id: String,
    owner_feature_id: u32,
    choice_label: String,
    name: String,
    type_byte: u8,
    value: CreoFeatureFieldValue,
    offset: usize,
    source_section: String,
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

#[derive(Serialize)]
struct CreoHalfEdgeRecord {
    id: String,
    curve_id: u32,
    side: u8,
    face_id: u32,
    next: Option<CreoHalfEdgeRef>,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoLoopRecord {
    id: String,
    face_id: u32,
    half_edges: Vec<CreoHalfEdgeRef>,
}

#[derive(Serialize)]
struct CreoTopologicalVertexRecord {
    id: String,
    vertex_id: u32,
    half_edges: Vec<CreoHalfEdgeRef>,
}

#[derive(Serialize)]
struct CreoHalfEdgeVertexIncidenceRecord {
    id: String,
    half_edge: CreoHalfEdgeRef,
    start_vertex_id: u32,
    end_vertex_id: Option<u32>,
}

#[derive(Serialize)]
struct CreoFaceComponentRecord {
    id: String,
    face_ids: Vec<u32>,
    curve_ids: Vec<u32>,
}

#[derive(Serialize)]
struct CreoExpandedSectionRecord {
    id: String,
    name: String,
    source_offset: usize,
    compressed_length: usize,
    expanded_length: usize,
    sha256: String,
}

#[derive(Serialize)]
struct CreoDoubleXarTableRecord {
    id: String,
    section_name: String,
    section_source_offset: usize,
    expanded_offset: usize,
    count: u32,
    entries: Vec<CreoDoubleXarEntryRecord>,
}

#[derive(Serialize)]
struct CreoDoubleXarEntryRecord {
    index: u32,
    raw: Vec<u8>,
    value: Option<f64>,
    kind: &'static str,
}

#[derive(Serialize)]
struct CreoPrimitiveScalarArrayRecord {
    id: String,
    field: String,
    expanded_offset: usize,
    count: u32,
    values: Vec<f64>,
}

#[derive(Debug, Serialize)]
struct CreoReferenceLineRecord {
    id: String,
    family: &'static str,
    start: [f64; 3],
    end: [f64; 3],
    offset: usize,
}

#[derive(Serialize)]
struct CreoReferenceCircleRecord {
    id: String,
    center: [f64; 3],
    center_source: &'static str,
    radius: f64,
    axis: [f64; 3],
    endpoints: [[f64; 3]; 2],
    offset: usize,
}

#[derive(Serialize)]
struct CreoReferenceConicRecord {
    id: String,
    entity_id: u32,
    type_id: u32,
    flip: u32,
    endpoints: [[f64; 3]; 2],
    parameter_interval: [Option<f64>; 2],
    coefficients: [f64; 2],
    local_system: Option<[f64; 12]>,
    body: Vec<u8>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoReferenceEllipseRecord {
    id: String,
    source_conic_id: String,
    center: [f64; 3],
    axis: [f64; 3],
    major_direction: [f64; 3],
    major_radius: f64,
    minor_radius: f64,
    offset: usize,
}

fn expanded_section_records(scan: &ContainerScan) -> Vec<CreoExpandedSectionRecord> {
    scan.expanded_sections
        .iter()
        .map(|section| CreoExpandedSectionRecord {
            id: format!(
                "creo:container:expanded_section#{}:{}",
                section.name, section.source_offset
            ),
            name: section.name.clone(),
            source_offset: section.source_offset,
            compressed_length: section.compressed_length,
            expanded_length: section.data.len(),
            sha256: sha256_hex(&section.data),
        })
        .collect()
}

fn attach_expanded_sections(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let records = expanded_section_records(scan);
    if records.is_empty() {
        return Ok(());
    }
    for record in &records {
        annotate(
            annotations,
            &record.id,
            &record.name,
            record.source_offset as u64,
            "unix_compress_expanded_section",
            Exactness::Derived,
        );
    }
    let namespace = ir.native.namespace_mut("creo");
    namespace.version = 1;
    namespace.set_arena("expanded_sections", &records)?;
    if !scan.double_xar_tables.is_empty() {
        let tables = scan
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
        for table in &tables {
            annotate(
                annotations,
                &table.id,
                &table.section_name,
                table.section_source_offset as u64,
                "model_scalar_dictionary",
                Exactness::ByteExact,
            );
        }
        namespace.set_arena("double_xar_tables", &tables)?;
    }
    let primitive_arrays = scan
        .primitive_scalar_arrays
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
    if !primitive_arrays.is_empty() {
        namespace.set_arena("primitive_scalar_arrays", &primitive_arrays)?;
    }
    Ok(())
}

#[derive(Serialize)]
struct CreoFcCurveCoordinateRecord {
    id: String,
    curve_id: u32,
    subtype: u8,
    body: Vec<u8>,
    values_mm: Vec<f64>,
    tokens: Vec<CreoFcCurveCoordinateToken>,
    opaque_spans: Vec<CreoFcCurveOpaqueSpan>,
    offset: usize,
    source_section: String,
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
struct CreoPrototypePcurveRecord {
    id: String,
    curve_id: u32,
    face_0_endpoints: [[f64; 2]; 2],
    face_1_endpoints: [[f64; 2]; 2],
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoCurvePrototypeTopologyRecord {
    id: String,
    curve_id: u32,
    faces: [u32; 2],
    next_edges: [u32; 2],
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoCurvePrototypeRecord {
    id: String,
    curve_id: u32,
    type_byte: u8,
    generating_feature_id: Option<u32>,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoPlaneLocalSystemRecord {
    id: String,
    surface_id: u32,
    body: Vec<u8>,
    slots: Vec<Option<f64>>,
    origin: Option<[f64; 3]>,
    u_axis: Option<[f64; 3]>,
    normal: Option<[f64; 3]>,
    classification: &'static str,
    row_offset: usize,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoPlaneEnvelopeRecord {
    id: String,
    surface_id: u32,
    body: Vec<u8>,
    envelope: CreoPlaneEnvelope,
    corner_coordinate_equal: [Option<bool>; 3],
    scalar_tokens: Vec<Vec<u8>>,
    row_offset: usize,
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

#[derive(Serialize)]
struct CreoOutlinePlaneRecord {
    id: String,
    surface_id: u32,
    origin: [f64; 3],
    normal: [f64; 3],
    u_axis: [f64; 3],
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoDatumPlaneRecord {
    id: String,
    datum_id: u32,
    owner_feature_id: u32,
    normal: [f64; 3],
    plane_offset: f64,
    corners: [[Option<f64>; 3]; 2],
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoFeatureSectionTransformRecord {
    id: String,
    definition_id: u32,
    owner_feature_id: Option<u32>,
    origin: [f64; 3],
    u_axis: [f64; 3],
    v_axis: [f64; 3],
    normal: [f64; 3],
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoFeaturePlacementInstructionRecord {
    id: String,
    definition_id: u32,
    owner_feature_id: Option<u32>,
    instruction_type: u32,
    zero_offset: bool,
    dimension_id: Option<u32>,
    reference_id: Option<u32>,
    geometry1_id: Option<u32>,
    geometry2_id: Option<u32>,
    member1: u32,
    member2: u32,
    offset: usize,
    source_section: String,
}

fn feature_entity_records(scan: &ContainerScan) -> Vec<CreoFeatureEntityRecord> {
    scan.feature_entities
        .iter()
        .map(|entity| CreoFeatureEntityRecord {
            id: format!("creo:allfeatur:entity#{}", entity.entity_id),
            entity_id: entity.entity_id,
            type_byte: entity.type_byte,
            name: entity.name.clone(),
            offset: entity.offset,
        })
        .collect()
}

fn feature_entity_reference_records(scan: &ContainerScan) -> Vec<CreoFeatureEntityReferenceRecord> {
    scan.feature_entity_references
        .iter()
        .map(|reference| CreoFeatureEntityReferenceRecord {
            id: format!("creo:allfeatur:entity_reference#{}", reference.offset),
            source_entity_id: reference.source_entity_id,
            target_entity_id: reference.target_entity_id,
            target_resolved: reference.target_resolved,
            offset: reference.offset,
        })
        .collect()
}

fn feature_entity_table_records(scan: &ContainerScan) -> Vec<CreoFeatureEntityTableRecord> {
    scan.feature_entity_tables
        .iter()
        .map(|table| CreoFeatureEntityTableRecord {
            id: format!("creo:allfeatur:entity_table#{}", table.offset),
            owner_feature_id: table.feature_id,
            table_class_id: table.table_class_id,
            entry_ids: table.entry_ids.clone(),
            entries: table
                .entries
                .iter()
                .map(|entry| CreoFeatureEntityTableEntryRecord {
                    entity_id: entry.entity_id,
                    class_id: entry.class_id,
                    source_entity_id: entry.source_entity_id,
                    prefixed: entry.prefixed,
                    offset: entry.offset,
                    end_offset: entry.end_offset,
                })
                .collect(),
            surface_ids: table.surface_ids.clone(),
            non_surface_entity_ids: table.non_surface_entity_ids.clone(),
            offset: table.offset,
        })
        .collect()
}

fn feature_geometry_table_records(scan: &ContainerScan) -> Vec<CreoFeatureGeometryTableRecord> {
    scan.feature_geometry_tables
        .iter()
        .map(|table| CreoFeatureGeometryTableRecord {
            id: format!("creo:feature:geometry_table#{}", table.offset),
            owner_feature_id: table.feature_id,
            kind: match table.kind {
                crate::feature::FeatureGeometryTableKind::EdgeIds => "edge_ids",
                crate::feature::FeatureGeometryTableKind::LoopIds => "loop_ids",
                crate::feature::FeatureGeometryTableKind::Boundaries => "boundaries",
                crate::feature::FeatureGeometryTableKind::UsedBodies => "used_bodies",
                crate::feature::FeatureGeometryTableKind::GeometryLists => "geometry_lists",
                crate::feature::FeatureGeometryTableKind::DatumIds => "datum_ids",
            },
            declared_count: table.count,
            entity_class_id: table.entity_class,
            entry_ids: table.entry_ids.clone(),
            offset: table.offset,
            source_section: source_section(scan, table.offset),
        })
        .collect()
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

fn feature_affected_id_records(scan: &ContainerScan) -> Vec<CreoFeatureAffectedIdsRecord> {
    scan.feature_affected_ids
        .iter()
        .map(|record| CreoFeatureAffectedIdsRecord {
            id: format!("creo:feature:affected_ids#{}", record.offset),
            owner_feature_id: record.feature_id,
            kind: affected_kind(record.kind),
            ids: record.ids.clone(),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn extent_source(source: crate::feature::ReplayExtentSource) -> &'static str {
    match source {
        crate::feature::ReplayExtentSource::Explicit => "explicit",
        crate::feature::ReplayExtentSource::Inherited => "inherited",
    }
}

fn feature_replay_affected_id_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureReplayAffectedIdsRecord> {
    scan.feature_replay_affected_ids
        .iter()
        .map(|record| CreoFeatureReplayAffectedIdsRecord {
            id: format!("creo:feature:replay_affected_ids#{}", record.offset),
            owner_feature_id: record.feature_id,
            geometry_ids: record.geometry_ids.clone(),
            edge_ids: record.edge_ids.clone(),
            geometry_extent: extent_source(record.geometry_extent),
            edge_extent: extent_source(record.edge_extent),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn feature_loop_restore_direction_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureLoopRestoreDirectionRecord> {
    scan.feature_loop_restore_directions
        .iter()
        .map(|record| CreoFeatureLoopRestoreDirectionRecord {
            id: format!("creo:feature:loop_restore_direction#{}", record.offset),
            owner_feature_id: record.feature_id,
            lane: match record.lane {
                crate::feature::LoopRestoreDirectionLane::Primary => "primary",
                crate::feature::LoopRestoreDirectionLane::Secondary => "secondary",
            },
            value: record.value,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn feature_revolution_extent_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureRevolutionExtentRecord> {
    scan.feature_revolution_extents
        .iter()
        .map(|record| CreoFeatureRevolutionExtentRecord {
            id: format!("creo:feature:revolution_extent#{}", record.offset),
            owner_feature_id: record.feature_id,
            kind: match record.kind {
                crate::feature::FeatureRevolutionExtentKind::FullTurn => "full_turn",
            },
            angle_radians: std::f64::consts::TAU,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn feature_choice_records(scan: &ContainerScan) -> Vec<CreoFeatureChoiceRecord> {
    scan.feature_choices
        .iter()
        .map(|choice| CreoFeatureChoiceRecord {
            id: format!("creo:feature:choice#{}", choice.offset),
            owner_feature_id: choice.feature_id,
            label: choice.label.clone(),
            type_byte: choice.type_byte,
            payload: choice.payload.clone(),
            payload_offset: choice.payload_offset,
            offset: choice.offset,
            source_section: source_section(scan, choice.offset),
        })
        .collect()
}

fn feature_row_records(scan: &ContainerScan) -> Vec<CreoFeatureRowRecord> {
    scan.feature_rows
        .iter()
        .map(|row| CreoFeatureRowRecord {
            id: format!("creo:allfeatur:feature_row#{}", row.offset),
            owner_feature_id: row.feature_id,
            header: row.header,
            root_schema_class: row.root_schema_class,
            stream_offset: row.stream_offset,
            body: row.body.clone(),
            body_offset: row.body_offset,
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

fn depdb_recipe_row_records(scan: &ContainerScan) -> Vec<CreoFeatureRowRecord> {
    scan.depdb_recipe_rows
        .iter()
        .map(|row| CreoFeatureRowRecord {
            id: format!("creo:depdb:recipe_row#{}", row.offset),
            owner_feature_id: row.feature_id,
            header: row.header,
            root_schema_class: row.root_schema_class,
            stream_offset: row.stream_offset,
            body: row.body.clone(),
            body_offset: row.body_offset,
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

fn feature_choice_field_records(scan: &ContainerScan) -> Vec<CreoFeatureChoiceFieldRecord> {
    scan.feature_choice_fields
        .iter()
        .map(|field| CreoFeatureChoiceFieldRecord {
            id: format!("creo:feature:choice_field#{}", field.offset),
            owner_feature_id: field.feature_id,
            choice_label: field.choice_label.clone(),
            name: field.name.clone(),
            type_byte: field.type_byte,
            value: match &field.value {
                crate::feature::FeatureFieldValue::Empty => CreoFeatureFieldValue::Empty,
                crate::feature::FeatureFieldValue::CompactInt(value) => {
                    CreoFeatureFieldValue::CompactInt { value: *value }
                }
                crate::feature::FeatureFieldValue::CompactIntArray(values) => {
                    CreoFeatureFieldValue::CompactIntArray {
                        values: values.clone(),
                    }
                }
                crate::feature::FeatureFieldValue::EntityReference {
                    entity_id,
                    terminated,
                } => CreoFeatureFieldValue::EntityReference {
                    entity_id: *entity_id,
                    terminated: *terminated,
                },
                crate::feature::FeatureFieldValue::ScalarArray {
                    dimensions,
                    count,
                    body,
                    decoded_values,
                } => CreoFeatureFieldValue::ScalarArray {
                    dimensions: *dimensions,
                    count: *count,
                    body: body.clone(),
                    decoded_values: decoded_values.clone(),
                },
                crate::feature::FeatureFieldValue::Raw(bytes) => CreoFeatureFieldValue::Raw {
                    bytes: bytes.clone(),
                },
            },
            offset: field.offset,
            source_section: source_section(scan, field.offset),
        })
        .collect()
}

fn half_edge_ref(id: crate::topology::HalfEdgeId) -> CreoHalfEdgeRef {
    CreoHalfEdgeRef {
        curve_id: id.curve_id,
        side: id.side,
    }
}

fn half_edge_records(scan: &ContainerScan) -> Vec<CreoHalfEdgeRecord> {
    let topology_rows = scan
        .curve_topology_rows
        .iter()
        .map(|row| (row.id, row))
        .collect::<BTreeMap<_, _>>();
    scan.half_edges
        .iter()
        .filter_map(|edge| {
            let row = topology_rows.get(&edge.id.curve_id)?;
            Some(CreoHalfEdgeRecord {
                id: format!(
                    "creo:topology:half_edge#{}:{}",
                    edge.id.curve_id, edge.id.side
                ),
                curve_id: edge.id.curve_id,
                side: edge.id.side,
                face_id: edge.face_id,
                next: edge.next.map(half_edge_ref),
                offset: row.offset,
                source_section: source_section(scan, row.offset),
            })
        })
        .collect()
}

fn loop_records(scan: &ContainerScan) -> Vec<CreoLoopRecord> {
    scan.loops
        .iter()
        .enumerate()
        .map(|(index, record)| CreoLoopRecord {
            id: format!("creo:topology:loop#{}", index + 1),
            face_id: record.face_id,
            half_edges: record
                .half_edges
                .iter()
                .copied()
                .map(half_edge_ref)
                .collect(),
        })
        .collect()
}

fn topological_vertex_records(scan: &ContainerScan) -> Vec<CreoTopologicalVertexRecord> {
    scan.topological_vertices
        .iter()
        .map(|record| CreoTopologicalVertexRecord {
            id: format!("creo:topology:vertex#{}", record.id),
            vertex_id: record.id,
            half_edges: record
                .half_edges
                .iter()
                .copied()
                .map(half_edge_ref)
                .collect(),
        })
        .collect()
}

fn half_edge_vertex_incidence_records(
    scan: &ContainerScan,
) -> Vec<CreoHalfEdgeVertexIncidenceRecord> {
    scan.half_edge_vertex_incidence
        .iter()
        .map(|record| CreoHalfEdgeVertexIncidenceRecord {
            id: format!(
                "creo:topology:half_edge_vertex_incidence#{}:{}",
                record.half_edge.curve_id, record.half_edge.side
            ),
            half_edge: half_edge_ref(record.half_edge),
            start_vertex_id: record.start_vertex_id,
            end_vertex_id: record.end_vertex_id,
        })
        .collect()
}

fn face_component_records(scan: &ContainerScan) -> Vec<CreoFaceComponentRecord> {
    scan.face_components
        .iter()
        .enumerate()
        .map(|(index, record)| CreoFaceComponentRecord {
            id: format!("creo:topology:face_component#{}", index + 1),
            face_ids: record.face_ids.clone(),
            curve_ids: record.curve_ids.clone(),
        })
        .collect()
}

fn fc_curve_coordinate_records(scan: &ContainerScan) -> Vec<CreoFcCurveCoordinateRecord> {
    scan.fc_curve_coordinates
        .iter()
        .map(|record| CreoFcCurveCoordinateRecord {
            id: format!("creo:curve:fc_coordinates#{}", record.curve_id),
            curve_id: record.curve_id,
            subtype: record.subtype,
            body: record.body.clone(),
            values_mm: record.values_mm.clone(),
            tokens: record
                .tokens
                .iter()
                .map(|token| CreoFcCurveCoordinateToken {
                    value_mm: token.value_mm,
                    raw: token.raw.clone(),
                    offset: token.offset,
                    length: token.length,
                })
                .collect(),
            opaque_spans: record
                .opaque_spans
                .iter()
                .map(|span| CreoFcCurveOpaqueSpan {
                    raw: span.raw.clone(),
                    offset: span.offset,
                    length: span.length,
                })
                .collect(),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn fc05_circle_records(scan: &ContainerScan) -> Vec<CreoFc05CircleRecord> {
    scan.fc05_circles
        .iter()
        .map(|record| CreoFc05CircleRecord {
            id: format!("creo:curve:fc05_circle#{}", record.curve_id),
            curve_id: record.curve_id,
            center_row_frame: record.center_row_frame,
            radius_mm: record.radius_mm,
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
    scan.fc05_cylinder_cap_pairs
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

fn prototype_pcurve_records(scan: &ContainerScan) -> Vec<CreoPrototypePcurveRecord> {
    scan.prototype_pcurves
        .iter()
        .map(|record| CreoPrototypePcurveRecord {
            id: format!("creo:curve:prototype_pcurve#{}", record.curve_id),
            curve_id: record.curve_id,
            face_0_endpoints: record.face_0_endpoints,
            face_1_endpoints: record.face_1_endpoints,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn curve_prototype_topology_records(scan: &ContainerScan) -> Vec<CreoCurvePrototypeTopologyRecord> {
    scan.curve_prototype_topology
        .iter()
        .map(|record| CreoCurvePrototypeTopologyRecord {
            id: format!("creo:curve:prototype_topology#{}", record.curve_id),
            curve_id: record.curve_id,
            faces: record.faces,
            next_edges: record.next_edges,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn curve_prototype_records(
    scan: &ContainerScan,
    prototypes: &[crate::curve::CurvePrototype],
    id_prefix: &str,
) -> Vec<CreoCurvePrototypeRecord> {
    prototypes
        .iter()
        .map(|record| CreoCurvePrototypeRecord {
            id: format!("{id_prefix}#{}:{}", record.offset, record.id),
            curve_id: record.id,
            type_byte: record.type_byte,
            generating_feature_id: record.feature_id,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn plane_local_system_records(
    scan: &ContainerScan,
    systems: &[crate::surface::PlaneLocalSystem],
    id_prefix: &str,
) -> Vec<CreoPlaneLocalSystemRecord> {
    systems
        .iter()
        .map(|record| CreoPlaneLocalSystemRecord {
            id: format!("{id_prefix}#{}:{}", record.offset, record.surface_id),
            surface_id: record.surface_id,
            body: record.body.clone(),
            slots: record.slots.clone(),
            origin: record.origin,
            u_axis: record.u_axis,
            normal: record.normal,
            classification: match record.classification {
                crate::surface::LocalSystemClassification::Simple => "simple",
                crate::surface::LocalSystemClassification::Unclassified => "unclassified",
            },
            row_offset: record.row_offset,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn plane_envelope_records(
    scan: &ContainerScan,
    envelopes: &[crate::surface::PlaneEnvelopeRecord],
    id_prefix: &str,
) -> Vec<CreoPlaneEnvelopeRecord> {
    envelopes
        .iter()
        .map(|record| CreoPlaneEnvelopeRecord {
            id: format!("{id_prefix}#{}:{}", record.offset, record.surface_id),
            surface_id: record.surface_id,
            body: record.body.clone(),
            envelope: match &record.envelope {
                crate::surface::PlaneEnvelope::Standard {
                    bounds_2d,
                    corners_3d,
                } => CreoPlaneEnvelope::Standard {
                    bounds_2d: *bounds_2d,
                    corners_3d: *corners_3d,
                },
                crate::surface::PlaneEnvelope::Compact { prefix, corners_3d } => {
                    CreoPlaneEnvelope::Compact {
                        prefix: *prefix,
                        corners_3d: *corners_3d,
                    }
                }
            },
            corner_coordinate_equal: record.corner_coordinate_equal,
            scalar_tokens: record.scalar_tokens.clone(),
            row_offset: record.row_offset,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn outline_plane_records(
    scan: &ContainerScan,
    planes: &[crate::surface::OutlinePlane],
    id_prefix: &str,
) -> Vec<CreoOutlinePlaneRecord> {
    planes
        .iter()
        .map(|record| CreoOutlinePlaneRecord {
            id: format!("{id_prefix}#{}:{}", record.offset, record.surface_id),
            surface_id: record.surface_id,
            origin: record.origin,
            normal: record.normal,
            u_axis: record.u_axis,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn datum_plane_records(scan: &ContainerScan) -> Vec<CreoDatumPlaneRecord> {
    scan.datum_planes
        .iter()
        .map(|record| CreoDatumPlaneRecord {
            id: format!(
                "creo:datum:plane#{}:{}",
                record.offset_in_payload, record.id
            ),
            datum_id: record.id,
            owner_feature_id: record.feature_id,
            normal: record.normal,
            plane_offset: record.offset,
            corners: record.corners,
            offset: record.offset_in_payload,
            source_section: source_section(scan, record.offset_in_payload),
        })
        .collect()
}

fn feature_section_transform_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureSectionTransformRecord> {
    let mut records = scan
        .feature_section_transforms
        .iter()
        .map(|record| CreoFeatureSectionTransformRecord {
            id: format!(
                "creo:feature:section_transform#{}:{}",
                record.definition_id, record.offset
            ),
            definition_id: record.definition_id,
            owner_feature_id: record.feature_id,
            origin: record.origin,
            u_axis: record.u_axis,
            v_axis: record.v_axis,
            normal: record.normal,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records.dedup_by(|left, right| left.id == right.id);
    records
}

fn feature_placement_instruction_records(
    scan: &ContainerScan,
) -> Vec<CreoFeaturePlacementInstructionRecord> {
    scan.feature_definitions
        .iter()
        .flat_map(|definition| {
            crate::feature::placement_instructions(definition)
                .into_iter()
                .map(|instruction| CreoFeaturePlacementInstructionRecord {
                    id: format!(
                        "creo:featdefs:placement_instruction#{}:{}",
                        definition.id, instruction.offset
                    ),
                    definition_id: definition.id,
                    owner_feature_id: definition.owner_feature_id,
                    instruction_type: instruction.kind,
                    zero_offset: instruction.zero_offset,
                    dimension_id: instruction.dimension_id,
                    reference_id: instruction.reference_id,
                    geometry1_id: instruction.geometry1_id,
                    geometry2_id: instruction.geometry2_id,
                    member1: instruction.member1,
                    member2: instruction.member2,
                    offset: instruction.offset,
                    source_section: source_section(scan, instruction.offset),
                })
        })
        .collect()
}

#[derive(Serialize)]
struct CreoSurfaceParameterRecord {
    id: String,
    surface_id: u32,
    surface_type_byte: u8,
    surface_family: &'static str,
    boundary: &'static str,
    body: Vec<u8>,
    slots: Vec<CreoSurfaceParameterSlot>,
    opaque_spans: Vec<CreoSurfaceParameterOpaqueSpan>,
    scalar_frames: Vec<CreoSurfaceParameterScalarFrame>,
    terminal_scalar_frame: Option<CreoSurfaceParameterScalarFrame>,
    tabulated_cylinder_frame: Option<CreoTabulatedCylinderFrame>,
    extrusion_direction: Option<[f64; 3]>,
    row_offset: usize,
    body_offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoTabulatedCylinderFrame {
    values: [f64; 6],
    prefixes: [u8; 6],
}

#[derive(Serialize)]
struct CreoSurfaceRowRecord {
    id: String,
    surface_id: u32,
    type_byte: u8,
    surface_family: &'static str,
    surface_variant: Option<&'static str>,
    feature_id: u32,
    reversed: bool,
    boundary_type: u8,
    next_surface: u32,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoSurfacePrototypeRecord {
    id: String,
    declared_family: String,
    family: String,
    parameters: Vec<CreoSurfaceNamedParameterRecord>,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoSurfaceNamedParameterRecord {
    name: String,
    value_kind: &'static str,
    compact_values: Vec<u32>,
    scalar_dimensions: Option<u32>,
    scalar_count: Option<u32>,
    scalar_values: Vec<Option<f64>>,
    scalar_tokens: Vec<Vec<u8>>,
    opaque: Vec<u8>,
    body: Vec<u8>,
    offset: usize,
    value_offset: usize,
}

#[derive(Serialize)]
struct CreoCurveParameterRecord {
    id: String,
    curve_id: u32,
    type_byte: u8,
    body: Vec<u8>,
    scalar_values: Vec<f64>,
    scalar_tokens: Vec<CreoCurveParameterScalar>,
    skipped_references: Vec<u32>,
    references: Vec<CreoCurveParameterReference>,
    opaque_spans: Vec<CreoCurveParameterOpaqueSpan>,
    suffix: &'static str,
    suffix_candidate_count: Option<usize>,
    offset: usize,
    body_offset: usize,
    suffix_offset: usize,
    source_section: String,
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
struct CreoCurveTopologyRowRecord {
    id: String,
    curve_id: u32,
    type_byte: u8,
    feature_id: u32,
    directions: [u8; 2],
    faces: [u32; 2],
    next_edges: [u32; 2],
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoCrossSectionCurveRowRecord {
    id: String,
    curve_id: u32,
    type_byte: u8,
    feature_id: u32,
    directions: [u8; 2],
    suffix: [u32; 4],
    body: Vec<u8>,
    scalar_values: Vec<f64>,
    scalar_tokens: Vec<CreoCurveParameterScalar>,
    references: Vec<CreoCurveParameterReference>,
    opaque_spans: Vec<CreoCurveParameterOpaqueSpan>,
    offset: usize,
    source_section: String,
}

#[derive(Serialize)]
struct CreoTabulatedCylinderCurveReplayRecord {
    id: String,
    surface_id: u32,
    curve_id: u32,
    curve_type: u8,
    flip: u8,
    tangent_condition: u8,
    degree: u8,
    parameter_body: Vec<u8>,
    control_point_ids: [u32; 4],
    successor_reference: u32,
    control_point_bodies: [Vec<u8>; 4],
    control_points: [Option<[f64; 2]>; 4],
    terminal_reference: u32,
    offset: usize,
    surface_row_offset: usize,
    source_section: String,
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
    scan.sections
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

fn surface_variant(type_byte: u8) -> Option<&'static str> {
    match type_byte {
        0x2a => Some("ruled_surface"),
        0x2c => Some("tabulated_cylinder"),
        _ => None,
    }
}

fn surface_row_records(
    scan: &ContainerScan,
    rows: &[crate::surface::SurfaceRow],
    namespace: &str,
) -> Vec<CreoSurfaceRowRecord> {
    rows.iter()
        .map(|row| CreoSurfaceRowRecord {
            id: format!("creo:{namespace}:surface_row#{}", row.id),
            surface_id: row.id,
            type_byte: row.type_byte,
            surface_family: surface_family(row.kind),
            surface_variant: surface_variant(row.type_byte),
            feature_id: row.feature_id,
            reversed: row.reversed,
            boundary_type: row.boundary_type,
            next_surface: row.next_surface,
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
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

fn surface_prototype_records(scan: &ContainerScan) -> Vec<CreoSurfacePrototypeRecord> {
    scan.surface_prototype_records
        .iter()
        .map(|record| CreoSurfacePrototypeRecord {
            id: format!("creo:visibgeom:surface_prototype#{}", record.offset),
            declared_family: record.declared_family.clone(),
            family: surface_prototype_family_name(&record.family),
            parameters: record
                .parameters
                .iter()
                .map(surface_named_parameter_record)
                .collect(),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn curve_parameter_records(scan: &ContainerScan) -> Vec<CreoCurveParameterRecord> {
    scan.curve_parameters
        .iter()
        .map(|record| {
            let (suffix, suffix_candidate_count) = match record.suffix {
                crate::curve::CurveSuffixStatus::Unique => ("unique", None),
                crate::curve::CurveSuffixStatus::Ambiguous { candidate_count } => {
                    ("ambiguous", Some(candidate_count))
                }
            };
            CreoCurveParameterRecord {
                id: format!("creo:visibgeom:curve_parameter#{}", record.curve_id),
                curve_id: record.curve_id,
                type_byte: record.type_byte,
                body: record.body.clone(),
                scalar_values: record.scalar_values.clone(),
                scalar_tokens: record
                    .scalar_tokens
                    .iter()
                    .map(|token| CreoCurveParameterScalar {
                        value: token.value,
                        raw: token.raw.clone(),
                        offset: token.offset,
                        length: token.length,
                    })
                    .collect(),
                skipped_references: record.skipped_references.clone(),
                references: record
                    .references
                    .iter()
                    .map(|reference| CreoCurveParameterReference {
                        entity_id: reference.entity_id,
                        offset: reference.offset,
                        length: reference.length,
                    })
                    .collect(),
                opaque_spans: record
                    .opaque_spans
                    .iter()
                    .map(|span| CreoCurveParameterOpaqueSpan {
                        raw: span.raw.clone(),
                        offset: span.offset,
                        length: span.length,
                    })
                    .collect(),
                suffix,
                suffix_candidate_count,
                offset: record.offset,
                body_offset: record.body_offset,
                suffix_offset: record.suffix_offset,
                source_section: source_section(scan, record.offset),
            }
        })
        .collect()
}

fn cross_section_curve_row_records(scan: &ContainerScan) -> Vec<CreoCrossSectionCurveRowRecord> {
    scan.cross_section_curve_rows
        .iter()
        .map(|row| CreoCrossSectionCurveRowRecord {
            id: format!("creo:cross_section_geometry:curve_row#{}", row.id),
            curve_id: row.id,
            type_byte: row.type_byte,
            feature_id: row.feature_id,
            directions: row.directions,
            suffix: row.suffix,
            body: row.body.clone(),
            scalar_values: row.scalar_tokens.iter().map(|token| token.value).collect(),
            scalar_tokens: row
                .scalar_tokens
                .iter()
                .map(|token| CreoCurveParameterScalar {
                    value: token.value,
                    raw: token.raw.clone(),
                    offset: token.offset,
                    length: token.length,
                })
                .collect(),
            references: row
                .references
                .iter()
                .map(|reference| CreoCurveParameterReference {
                    entity_id: reference.entity_id,
                    offset: reference.offset,
                    length: reference.length,
                })
                .collect(),
            opaque_spans: row
                .opaque_spans
                .iter()
                .map(|span| CreoCurveParameterOpaqueSpan {
                    raw: span.raw.clone(),
                    offset: span.offset,
                    length: span.length,
                })
                .collect(),
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

fn curve_topology_row_records(scan: &ContainerScan) -> Vec<CreoCurveTopologyRowRecord> {
    scan.curve_topology_rows
        .iter()
        .map(|row| CreoCurveTopologyRowRecord {
            id: format!("creo:visibgeom:curve_topology#{}", row.id),
            curve_id: row.id,
            type_byte: row.type_byte,
            feature_id: row.feature_id,
            directions: row.directions,
            faces: row.faces,
            next_edges: row.next_edges,
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

fn tabulated_cylinder_curve_replay_records(
    scan: &ContainerScan,
) -> Vec<CreoTabulatedCylinderCurveReplayRecord> {
    scan.tabulated_cylinder_curve_replays
        .iter()
        .map(|record| CreoTabulatedCylinderCurveReplayRecord {
            id: format!(
                "creo:visibgeom:tabulated_cylinder_curve_replay#{}",
                record.surface_id
            ),
            surface_id: record.surface_id,
            curve_id: record.curve_id,
            curve_type: record.curve_type,
            flip: record.flip,
            tangent_condition: record.tangent_condition,
            degree: record.degree,
            parameter_body: record.parameter_body.clone(),
            control_point_ids: record.control_point_ids,
            successor_reference: record.successor_reference,
            control_point_bodies: record.control_point_bodies.clone(),
            control_points: record.control_points,
            terminal_reference: record.terminal_reference,
            offset: record.offset,
            surface_row_offset: record.surface_row_offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn surface_parameter_records(
    scan: &ContainerScan,
    rows: &[crate::surface::SurfaceRow],
    parameters: &[crate::surface::SurfaceParameterRecord],
    namespace: &str,
) -> Vec<CreoSurfaceParameterRecord> {
    parameters
        .iter()
        .filter_map(|record| {
            let row = crate::surface::unique_surface_row(rows, record.surface_id)?;
            let surface_family = surface_family(row.kind);
            let boundary = match record.boundary {
                crate::surface::SurfaceBodyBoundary::CompoundClose => "compound_close",
                crate::surface::SurfaceBodyBoundary::NextRow => "next_row",
                crate::surface::SurfaceBodyBoundary::NamedRecord => "named_record",
                crate::surface::SurfaceBodyBoundary::SectionEnd => "section_end",
            };
            let source_section = source_section(scan, record.body_offset);
            Some(CreoSurfaceParameterRecord {
                id: format!("creo:{namespace}:surface_parameter#{}", record.surface_id),
                surface_id: record.surface_id,
                surface_type_byte: row.type_byte,
                surface_family,
                boundary,
                body: record.body.clone(),
                slots: record
                    .scalar_tokens
                    .iter()
                    .map(|slot| CreoSurfaceParameterSlot {
                        value: slot.value,
                        raw: slot.raw.clone(),
                        offset: slot.offset,
                        length: slot.length,
                    })
                    .collect(),
                opaque_spans: record
                    .opaque_spans
                    .iter()
                    .map(|span| CreoSurfaceParameterOpaqueSpan {
                        raw: span.raw.clone(),
                        offset: span.offset,
                        length: span.length,
                    })
                    .collect(),
                scalar_frames: record
                    .scalar_frames
                    .iter()
                    .map(|frame| CreoSurfaceParameterScalarFrame {
                        offset: frame.offset,
                        slots: frame
                            .slots
                            .iter()
                            .map(|slot| CreoSurfaceParameterSlot {
                                value: slot.value,
                                raw: slot.raw.clone(),
                                offset: slot.offset,
                                length: slot.length,
                            })
                            .collect(),
                    })
                    .collect(),
                terminal_scalar_frame: record.terminal_scalar_frame.as_ref().map(|frame| {
                    CreoSurfaceParameterScalarFrame {
                        offset: frame.offset,
                        slots: frame
                            .slots
                            .iter()
                            .map(|slot| CreoSurfaceParameterSlot {
                                value: slot.value,
                                raw: slot.raw.clone(),
                                offset: slot.offset,
                                length: slot.length,
                            })
                            .collect(),
                    }
                }),
                tabulated_cylinder_frame: record.tabulated_cylinder_frame.map(|frame| {
                    CreoTabulatedCylinderFrame {
                        values: frame.values,
                        prefixes: frame.prefixes,
                    }
                }),
                extrusion_direction: (row.kind == crate::surface::SurfaceKind::Extrusion)
                    .then(|| record.extrusion_direction(row.type_byte))
                    .flatten(),
                row_offset: record.offset,
                body_offset: record.body_offset,
                source_section,
            })
        })
        .collect()
}

fn family_table_record(scan: &ContainerScan) -> Option<CreoFamilyTableRecord> {
    let record = scan.family_table?;
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

fn feature_operation_state_records(scan: &ContainerScan) -> Vec<CreoFeatureOperationState> {
    let current_offsets = scan
        .feature_operations
        .iter()
        .map(|state| (state.feature_id, state.offset))
        .collect::<BTreeMap<_, _>>();
    let mut ordinals = BTreeMap::<u32, usize>::new();
    scan.feature_operation_states
        .iter()
        .map(|state| {
            let state_ordinal = *ordinals.entry(state.feature_id).or_default();
            ordinals.insert(state.feature_id, state_ordinal + 1);
            CreoFeatureOperationState {
                id: format!(
                    "creo:mdlstatus:feature_state#{}:{state_ordinal}",
                    state.feature_id
                ),
                feature_id: state.feature_id,
                state_ordinal,
                current: current_offsets.get(&state.feature_id) == Some(&state.offset),
                family: state.kind.clone(),
                display_name_stored: state.display_name_stored,
                stored_name: state.stored_name.clone(),
                stored_name_bytes: state.stored_name_bytes.clone(),
                identifier_keyword: state.identifier_keyword.clone(),
                stored_name_prefix: state
                    .stored_name_prefix
                    .map(|prefix| char::from(prefix).to_string()),
                recipe: state.recipe.map(crate::feature::FeatureRecipe::name),
                root_schema_class: state.root_schema_class,
                parent_feature_id: state.parent_feature_id,
                offset: state.offset,
                state_offset: state.state_offset,
            }
        })
        .collect()
}

#[derive(Serialize)]
struct CreoPcurveEndpointRecord {
    id: String,
    curve_id: u32,
    faces: [u32; 2],
    face_0_endpoints: [[f64; 2]; 2],
    face_1_endpoints: [[f64; 2]; 2],
    source_form: &'static str,
}

fn pcurve_endpoint_records(scan: &ContainerScan) -> Vec<(CreoPcurveEndpointRecord, usize)> {
    let mut records = scan
        .pcurves
        .iter()
        .map(|pcurve| {
            (
                CreoPcurveEndpointRecord {
                    id: format!("creo:visibgeom:pcurve_endpoints#{}", pcurve.curve_id),
                    curve_id: pcurve.curve_id,
                    faces: pcurve.faces,
                    face_0_endpoints: pcurve.face_0_endpoints,
                    face_1_endpoints: pcurve.face_1_endpoints,
                    source_form: "positional",
                },
                pcurve.offset,
            )
        })
        .collect::<Vec<_>>();
    records.extend(scan.bound_prototype_pcurves.iter().map(|pcurve| {
        (
            CreoPcurveEndpointRecord {
                id: format!(
                    "creo:visibgeom:prototype_pcurve_endpoints#{}",
                    pcurve.curve_id
                ),
                curve_id: pcurve.curve_id,
                faces: pcurve.faces,
                face_0_endpoints: pcurve.face_0_endpoints,
                face_1_endpoints: pcurve.face_1_endpoints,
                source_form: "prototype",
            },
            pcurve.offset,
        )
    }));
    records.sort_by_key(|(_, offset)| *offset);
    records
}

fn curve_expression_records(scan: &ContainerScan) -> Vec<CreoCurveExpressionRecord> {
    scan.curve_expressions
        .iter()
        .map(|record| CreoCurveExpressionRecord {
            id: curve_expression_record_id(record),
            entity_id: record.entity_id,
            backup: record.backup,
            local_system: record.local_system.as_ref().map(|frame| {
                CreoCurveExpressionLocalSystem {
                    dimensions: frame.dimensions,
                    count: frame.count,
                    body: frame.body.clone(),
                    explicit_slots: frame.explicit_slots,
                    offset: frame.offset,
                }
            }),
            lines: record
                .lines
                .iter()
                .map(|line| CreoCurveExpressionLine {
                    text: line.text.clone(),
                    offset: line.offset,
                })
                .collect(),
            assignments: record
                .assignments
                .iter()
                .map(|assignment| CreoCurveExpressionAssignment {
                    name: assignment.name.clone(),
                    expression: assignment.expression.clone(),
                    dependencies: assignment.dependencies.clone(),
                    value: assignment.value,
                    offset: assignment.offset,
                })
                .collect(),
        })
        .collect()
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
                .filter_map(|name| unique_assignment_indices.get(name).copied())
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
            *counts.entry(assignment.name.as_str()).or_insert(0usize) += 1;
            counts
        });
    let mut occurrences = BTreeMap::new();
    assignments
        .iter()
        .map(|assignment| {
            if counts[assignment.name.as_str()] == 1 {
                return assignment.name.clone();
            }
            let occurrence = occurrences
                .entry(assignment.name.as_str())
                .or_insert(0usize);
            *occurrence += 1;
            format!("{}#{occurrence}", assignment.name)
        })
        .collect()
}

fn transfer_curve_expression_features(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    let ordinal_base = ir
        .model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
        .map_or(0, |value| value + 1);
    for (expression_ordinal, record) in scan
        .curve_expressions
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
            assignment_indices_by_name
                .entry(assignment.name.clone())
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
            let dependencies = assignment
                .dependencies
                .iter()
                .filter_map(|name| unique_assignment_indices.get(name).copied())
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
            let external_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| {
                    name.as_str() != "t" && !assignment_indices_by_name.contains_key(*name)
                })
                .cloned()
                .collect::<Vec<_>>();
            let ambiguous_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| matches!(assignment_indices_by_name.get(*name), Some(None)))
                .cloned()
                .collect::<Vec<_>>();
            let intrinsic_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| name.as_str() == "t")
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
                    unique_assignment_indices
                        .get(name)
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
                owner: feature_id.clone(),
                ordinal,
                name: parameter_names[assignment_ordinal].clone(),
                expression: assignment.expression.clone(),
                display: None,
                value: assignment.value.map(ParameterValue::Real),
                dependencies,
                properties,
                pmi: None,
                native_ref: Some(curve_expression_record_id(record)),
            });
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
                radius: Length(helix.radius),
                height: Length(helix.height),
                revolutions: helix.revolutions,
                start_angle: Angle(helix.start_angle),
                clockwise: helix.clockwise,
            },
        );
        ir.model.features.push(Feature {
            id: feature_id,
            ordinal,
            name: Some(format!("Curve Equation {}", record.entity_id)),
            suppressed: false,
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
}

fn sketch_records(scan: &ContainerScan) -> Vec<CreoSketchRecord> {
    scan.feature_definitions
        .iter()
        .filter(|definition| {
            definition.variables.is_some()
                || definition.segments.is_some()
                || definition.trim_entities.is_some()
                || definition.trim_vertices.is_some()
                || definition.order_table.is_some()
                || definition.section_3d.is_some()
                || definition.saved_section.is_some()
                || definition.dimensions.is_some()
                || definition.relations.is_some()
        })
        .map(|definition| CreoSketchRecord {
            id: feature_sketch_record_id_in_scan(scan, definition),
            definition_id: definition.id,
            owner_feature_id: definition.owner_feature_id,
            source_section: source_section(scan, definition.offset),
            offset: definition.offset,
            section_3d: definition
                .section_3d
                .as_ref()
                .map(|section| CreoSketchSection3d {
                    sketch_plane_entity_id: section.sketch_plane_entity_id,
                    sketch_plane_flip: section.sketch_plane_flip.map(binary_flag_value),
                    reference_plane_entity_ids: section.reference_plane_entity_ids.clone(),
                    reference_plane_datum_geometry_id: section.reference_plane_datum_geometry_id,
                    orientation: CreoSketchSectionOrientation {
                        section_flip: section.orientation.section_flip.map(binary_flag_value),
                        reference_type: section.orientation.reference_type,
                        segment_id: section.orientation.segment_id,
                        reference_flip: section.orientation.reference_flip.map(binary_flag_value),
                    },
                    dimension_ids: section.dimension_ids.clone(),
                    offset: section.offset,
                }),
            table_headers: sketch_table_headers(definition),
            section_points: sketch_section_point_records(definition),
            solved_external_ids: definition
                .trim_entities
                .as_ref()
                .map_or_else(Vec::new, |table| table.solved_external_ids.clone()),
            variables: definition
                .variables
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| CreoSketchVariable {
                    variable_type: row.variable_type,
                    key: row.key,
                    value: row.value,
                    guess: row.guess,
                    known: row.known,
                    homogeneity: row.homogeneity,
                    uvar_id: row.uvar_id,
                    dimension_driven: row.dimension_driven,
                    offset: row.offset,
                })
                .collect(),
            segments: definition
                .segments
                .iter()
                .flat_map(|table| &table.rows)
                .map(|segment| CreoSketchSegment {
                    external_id: segment.external_id,
                    kind: match segment.kind {
                        crate::feature::FeatureSegmentKind::Line => "line",
                        crate::feature::FeatureSegmentKind::Arc => "arc",
                        crate::feature::FeatureSegmentKind::Point => "point",
                    },
                    point_ids: segment.point_ids,
                    center_id: segment.center_id,
                    directions: segment.directions,
                    arc_orientation: segment.arc_orientation,
                    vertical_horizontal_constraint: segment.vertical_horizontal,
                    radius_dimension_id: segment.radius_ref,
                    secondary_radius_dimension_id: segment.radius2_ref,
                    offset: segment.offset,
                })
                .collect(),
            trim_entities: definition
                .trim_entities
                .iter()
                .flat_map(|table| &table.rows)
                .map(|entity| CreoSketchTrimEntity {
                    external_id: entity.external_id,
                    mode: entity.mode,
                    vertices: entity.vertices,
                    center_vertex: entity.center_vertex,
                    kind: match entity.kind {
                        crate::feature::TrimEntityKind::Line => "line",
                        crate::feature::TrimEntityKind::Arc => "arc",
                    },
                    offset: entity.offset,
                })
                .collect(),
            trim_vertices: definition
                .trim_vertices
                .iter()
                .flat_map(|table| &table.rows)
                .map(|vertex| CreoSketchTrimVertex {
                    vertex_id: vertex.vertex_id,
                    entities: vertex.entities,
                    section_coordinates: vertex.section_coordinates,
                    offset: vertex.offset,
                })
                .collect(),
            order_rows: definition
                .order_table
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| CreoSketchOrderRow {
                    external_id: row.external_id,
                    internal_id: row.internal_id,
                    bitmask: row.bitmask,
                    offset: row.offset,
                })
                .collect(),
            saved_entities: definition
                .saved_section
                .iter()
                .flat_map(|section| &section.entities)
                .map(|entity| match entity {
                    crate::feature::FeatureSavedEntity::Line(line) => CreoSketchSavedEntity::Line {
                        entity_id: line.entity_id,
                        references: line.references.clone(),
                        attributes: line.attributes.clone(),
                        endpoints: line.endpoints,
                        offset: line.offset,
                    },
                    crate::feature::FeatureSavedEntity::Arc(arc) => CreoSketchSavedEntity::Arc {
                        entity_id: arc.entity_id,
                        center: arc.center,
                        radius: arc.radius,
                        endpoints: arc.endpoints,
                        parameters: arc.parameters,
                        offset: arc.offset,
                    },
                    crate::feature::FeatureSavedEntity::Circle(circle) => {
                        CreoSketchSavedEntity::Circle {
                            entity_id: circle.entity_id,
                            center: circle.center,
                            radius: circle.radius,
                            offset: circle.offset,
                        }
                    }
                    crate::feature::FeatureSavedEntity::Spline(spline) => {
                        CreoSketchSavedEntity::Spline {
                            entity_id: spline.entity_id,
                            interpolation_points: spline.interpolation_points.clone(),
                            endpoint_tangents: spline.endpoint_tangents,
                            parameters: spline.parameters.clone(),
                            offset: spline.offset,
                        }
                    }
                    crate::feature::FeatureSavedEntity::Dummy(dummy) => {
                        CreoSketchSavedEntity::Dummy {
                            entity_id: dummy.entity_id,
                            offset: dummy.offset,
                        }
                    }
                })
                .collect(),
            dimensions: definition
                .dimensions
                .iter()
                .flat_map(|table| &table.rows)
                .map(|dimension| CreoSketchDimension {
                    external_id: dimension.external_id,
                    dimension_type: dimension.dimension_type,
                    value: dimension.value,
                    unit: match dimension.value_unit {
                        crate::feature::DimensionUnit::Radians => "radians",
                        crate::feature::DimensionUnit::Millimeters => "millimeters",
                        crate::feature::DimensionUnit::SchemaDefined => "schema_defined",
                    },
                    direction_byte: dimension.direction_byte,
                    auxiliary_value: dimension.auxiliary_value,
                    offset: dimension.offset,
                })
                .collect(),
            relations: definition
                .relations
                .iter()
                .flat_map(|table| &table.rows)
                .map(|relation| CreoSketchRelation {
                    relation_id: relation.relation_id,
                    used: relation.used,
                    operands: relation.operands.clone(),
                    operand_vectors: relation.operand_vectors,
                    sign: relation.sign,
                    dimension_id: relation.dimension_id,
                    relation_type: relation.relation_type,
                    body: relation.body.clone(),
                    offset: relation.offset,
                })
                .collect(),
            skamps: definition
                .relations
                .iter()
                .flat_map(|table| &table.skamps)
                .map(|skamp| CreoSketchSkamp {
                    id: skamp.id,
                    kind: skamp.kind,
                    flags: skamp.flags,
                    status: skamp.status,
                    items: skamp
                        .items
                        .iter()
                        .map(|item| CreoSketchSkampItem {
                            entity_id: item.entity_id,
                            sense: item.sense,
                        })
                        .collect(),
                    offset: skamp.offset,
                })
                .collect(),
            relation_triples: definition
                .relations
                .iter()
                .flat_map(|table| &table.triples)
                .map(|triple| CreoSketchRelationTriple {
                    relation: triple.relation_id,
                    equation: triple.equation_id,
                    skamp: triple.skamp_id,
                    offset: triple.offset,
                })
                .collect(),
        })
        .collect()
}

fn sketch_section_point_records(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<CreoSketchSectionPoint> {
    let Some(variables) = &definition.variables else {
        return Vec::new();
    };
    let (points, ambiguous) = variables.reconciled_points();
    points
        .keys()
        .copied()
        .chain(ambiguous.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|point_id| {
            let [u, v] = points.get(&point_id).copied().unwrap_or([None; 2]);
            let state = if ambiguous.contains(&point_id) {
                "conflicting"
            } else {
                match (u.is_some(), v.is_some()) {
                    (true, true) => "resolved",
                    (true, false) | (false, true) => "partial",
                    (false, false) => "unresolved",
                }
            };
            CreoSketchSectionPoint {
                point_id,
                u,
                v,
                state,
            }
        })
        .collect()
}

fn sketch_table_headers(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<CreoSketchTableHeader> {
    let mut headers = Vec::new();
    let mut push = |kind, declared_count, entity_ref, entry_ref, row_count, offset| {
        headers.push(CreoSketchTableHeader {
            kind,
            declared_count,
            entity_ref,
            entry_ref,
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
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.trim_entities {
        push(
            "trim_entities",
            table.declared_count,
            table.entity_ref,
            table.entry_ref,
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
            table.rows.len(),
            table.offset,
        );
        if let Some(header) = &table.skamp_header {
            push(
                "solver_incidences",
                Some(header.declared_count),
                Some(header.entity_ref),
                None,
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
        .feature_definitions
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
        .feature_definitions
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

fn feature_sketch_record_id(definition: &crate::feature::FeatureDefinition) -> String {
    if definition.id == 0 && definition.owner_feature_id.is_none() {
        format!("creo:featdefs:sketch#offset:{}", definition.offset)
    } else {
        format!("creo:featdefs:sketch#{}", definition.id)
    }
}

fn feature_definition_records(scan: &ContainerScan) -> Vec<CreoFeatureDefinitionRecord> {
    scan.feature_definitions
        .iter()
        .map(|definition| CreoFeatureDefinitionRecord {
            id: feature_definition_record_id(scan, definition),
            definition_id: definition.id,
            owner_feature_id: definition.owner_feature_id,
            source_section: source_section(scan, definition.offset),
            body: definition.body.clone(),
            parameter_frames: definition
                .parameter_frames
                .iter()
                .map(|frame| CreoFeatureParameterFrame {
                    kind: match frame.kind {
                        crate::feature::FeatureParameterFrameKind::LocalSystem => "local_system",
                        crate::feature::FeatureParameterFrameKind::Transform => "transform",
                    },
                    body: frame.body.clone(),
                    decoded_values: frame.decoded_values.clone(),
                    offset: frame.offset,
                })
                .collect(),
            outlines: definition
                .outlines
                .iter()
                .map(|outline| CreoFeatureOutline {
                    phase: match outline.phase {
                        crate::feature::OutlinePhase::PreRollback => "pre_rollback",
                        crate::feature::OutlinePhase::PostRollback => "post_rollback",
                        crate::feature::OutlinePhase::PostRegen => "post_regen",
                    },
                    local_values: outline.local_values.clone(),
                    offset: outline.offset,
                })
                .collect(),
            offset: definition.offset,
        })
        .collect()
}

fn owning_feature_definition_ref(scan: &ContainerScan, feature_id: u32) -> Option<String> {
    let definitions = scan
        .feature_definitions
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
            let segments = &definition.segments.as_ref()?.rows;
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
            let trimmed = definition.trim_entities.as_ref()?;
            let segment_ids = definition
                .segments
                .as_ref()?
                .rows
                .iter()
                .filter(|candidate| {
                    candidate.kind == crate::feature::FeatureSegmentKind::Line
                        && trimmed
                            .rows
                            .iter()
                            .any(|row| row.external_id == candidate.external_id)
                        && !order_table
                            .rows
                            .iter()
                            .any(|row| row.external_id == candidate.external_id)
                })
                .map(|candidate| candidate.external_id)
                .collect::<Vec<_>>();
            let saved_ids = saved_section
                .entities
                .iter()
                .filter_map(|entity| match entity {
                    crate::feature::FeatureSavedEntity::Line(line)
                        if !order_table
                            .rows
                            .iter()
                            .any(|row| row.internal_id == line.entity_id) =>
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
        })?;
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
    definition_id: u32,
    geometries: &[(u32, SketchGeometry)],
) -> Vec<Vec<SketchEntityUse>> {
    let mut profiles = geometries
        .iter()
        .filter(|(_, geometry)| is_full_circle_geometry(geometry))
        .map(|(external_id, _)| {
            vec![SketchEntityUse {
                entity: SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{definition_id}:{external_id}"
                )),
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
                entity: SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{definition_id}:{}",
                    rows[row].0
                )),
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
    section_segment_geometry(points, segment)
        .or_else(|| saved_section_line_geometry(definition, segment))
        .or_else(|| saved_section_arc_geometry(definition, segment))
}

pub(crate) fn resolved_section_points(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, [f64; 2]> {
    let Some(variables) = &definition.variables else {
        return BTreeMap::new();
    };
    let (mut points, ambiguous_point_ids) = variables.reconciled_points();
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
    let coincident_points = definition
        .relations
        .iter()
        .flat_map(|table| &table.skamps)
        .filter_map(|skamp| {
            let [first, second] = skamp.items.as_slice() else {
                return None;
            };
            let pair = match skamp.kind {
                0 => Some([
                    section_skamp_endpoint_point(definition, first)?,
                    section_skamp_endpoint_point(definition, second)?,
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
    let point_on_line_coordinates = definition
        .relations
        .iter()
        .flat_map(|table| &table.skamps)
        .filter_map(|skamp| section_skamp_point_on_line(definition, skamp))
        .filter(|(first, second, _)| {
            !ambiguous_point_ids.contains(first) && !ambiguous_point_ids.contains(second)
        })
        .collect::<Vec<_>>();
    let saved_point_on_line_coordinates = definition
        .relations
        .iter()
        .flat_map(|table| &table.skamps)
        .filter_map(|skamp| section_skamp_saved_point_on_line(definition, skamp))
        .filter(|(point_id, _, _)| !ambiguous_point_ids.contains(point_id))
        .collect::<Vec<_>>();
    let symmetric_point_constraints = definition
        .relations
        .iter()
        .flat_map(|table| &table.skamps)
        .filter_map(|skamp| section_skamp_axis_symmetry(definition, skamp))
        .filter(|(axis, first, second, _)| {
            !ambiguous_point_ids.contains(first)
                && !ambiguous_point_ids.contains(second)
                && match axis {
                    SectionSymmetryAxis::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionSymmetryAxis::Value(_) => true,
                }
        })
        .collect::<Vec<_>>();
    let signed_dimension_candidates = definition
        .relations
        .iter()
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
            let magnitude = definition
                .dimensions
                .as_ref()?
                .rows
                .get(usize::try_from(relation.dimension_id).ok()?)?
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
    loop {
        let mut changed = false;
        for segment in &segments {
            let Some(coordinate) = section_line_fixed_coordinate(definition, segment) else {
                continue;
            };
            let [first_id, second_id] = segment.point_ids;
            let [first, second] =
                [first_id, second_id].map(|id| points.get(&id).copied().unwrap_or([None, None]));
            match [first[coordinate], second[coordinate]] {
                [Some(value), None] => {
                    points.entry(second_id).or_insert([None, None])[coordinate] = Some(value);
                    changed = true;
                }
                [None, Some(value)] => {
                    points.entry(first_id).or_insert([None, None])[coordinate] = Some(value);
                    changed = true;
                }
                _ => {}
            }
        }
        for &(first_id, second_id, coordinate, delta) in &signed_dimensions {
            let [first, second] =
                [first_id, second_id].map(|id| points.get(&id).copied().unwrap_or([None, None]));
            match [first[coordinate], second[coordinate]] {
                [Some(value), None] => {
                    points.entry(second_id).or_insert([None, None])[coordinate] =
                        Some(value + delta);
                    changed = true;
                }
                [None, Some(value)] => {
                    points.entry(first_id).or_insert([None, None])[coordinate] =
                        Some(value - delta);
                    changed = true;
                }
                _ => {}
            }
        }
        for &[first_source, second_source] in &coincident_points {
            let [first, second] = [first_source, second_source].map(|source| match source {
                SectionPointSource::Point(id) => points.get(&id).copied().unwrap_or([None, None]),
                SectionPointSource::Value(value) => [Some(value[0]), Some(value[1])],
            });
            for coordinate in 0..2 {
                match [first[coordinate], second[coordinate]] {
                    [Some(value), None] => {
                        if let SectionPointSource::Point(second_id) = second_source {
                            points.entry(second_id).or_insert([None, None])[coordinate] =
                                Some(value);
                            changed = true;
                        }
                    }
                    [None, Some(value)] => {
                        if let SectionPointSource::Point(first_id) = first_source {
                            points.entry(first_id).or_insert([None, None])[coordinate] =
                                Some(value);
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        for &(line_point_id, point_id, coordinate) in &point_on_line_coordinates {
            let [line_point, point] = [line_point_id, point_id]
                .map(|id| points.get(&id).copied().unwrap_or([None, None]));
            match [line_point[coordinate], point[coordinate]] {
                [Some(value), None] => {
                    points.entry(point_id).or_insert([None, None])[coordinate] = Some(value);
                    changed = true;
                }
                [None, Some(value)] => {
                    points.entry(line_point_id).or_insert([None, None])[coordinate] = Some(value);
                    changed = true;
                }
                _ => {}
            }
        }
        for &(point_id, coordinate, value) in &saved_point_on_line_coordinates {
            let point = points.get(&point_id).copied().unwrap_or([None, None]);
            if point[coordinate].is_none() {
                points.entry(point_id).or_insert([None, None])[coordinate] = Some(value);
                changed = true;
            }
        }
        for &(axis, first_id, second_id, fixed_coordinate) in &symmetric_point_constraints {
            let [first, second] =
                [first_id, second_id].map(|id| points.get(&id).copied().unwrap_or([None, None]));
            let parallel_coordinate = 1usize.saturating_sub(fixed_coordinate);
            match [first[parallel_coordinate], second[parallel_coordinate]] {
                [Some(value), None] => {
                    points.entry(second_id).or_insert([None, None])[parallel_coordinate] =
                        Some(value);
                    changed = true;
                }
                [None, Some(value)] => {
                    points.entry(first_id).or_insert([None, None])[parallel_coordinate] =
                        Some(value);
                    changed = true;
                }
                _ => {}
            }
            let axis_value = match axis {
                SectionSymmetryAxis::Point(point_id) => points
                    .get(&point_id)
                    .and_then(|point| point[fixed_coordinate]),
                SectionSymmetryAxis::Value(value) => Some(value),
            };
            let Some(axis_value) = axis_value else {
                continue;
            };
            match [first[fixed_coordinate], second[fixed_coordinate]] {
                [Some(value), None] => {
                    points.entry(second_id).or_insert([None, None])[fixed_coordinate] =
                        Some(2.0 * axis_value - value);
                    changed = true;
                }
                [None, Some(value)] => {
                    points.entry(first_id).or_insert([None, None])[fixed_coordinate] =
                        Some(2.0 * axis_value - value);
                    changed = true;
                }
                _ => {}
            }
        }
        if !changed {
            break;
        }
    }
    points
        .into_iter()
        .filter_map(|(id, [u, v])| Some((id, [u?, v?])))
        .collect()
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
    for skamp in definition.relations.iter().flat_map(|table| &table.skamps) {
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
        definition
            .relations
            .iter()
            .flat_map(|table| &table.skamps)
            .filter_map(|skamp| match (skamp.kind, skamp.items.as_slice()) {
                (1, [item]) if item.sense == 0 && item.entity_id == entity_id => Some(1),
                (2, [item]) if item.sense == 0 && item.entity_id == entity_id => Some(0),
                _ => None,
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
) -> Option<(SectionSymmetryAxis, u32, u32, usize)> {
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
        section_skamp_selected_point_id(definition, first_item)?,
        section_skamp_selected_point_id(definition, second_item)?,
        coordinate,
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

fn section_skamp_endpoint_point(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SectionPointSource> {
    if let Some(segment) = unique_section_skamp_segment(definition, item.entity_id) {
        return match item.sense {
            2 => Some(SectionPointSource::Point(segment.point_ids[0])),
            3 => Some(SectionPointSource::Point(segment.point_ids[1])),
            _ => None,
        };
    }
    saved_section_point(definition, item).map(SectionPointSource::Value)
}

fn unique_section_skamp_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureSegment> {
    let segments = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .filter(|segment| segment.external_id == external_id)
        .collect::<Vec<_>>();
    let [segment] = segments.as_slice() else {
        return None;
    };
    Some(segment)
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
    for row in definition.variables.iter().flat_map(|table| &table.rows) {
        if row.variable_type == 3 {
            if let Some(value) = row.value.filter(|value| value.is_finite() && *value > 0.0) {
                candidates.entry(row.key).or_default().push(value);
            }
        }
    }
    for relation in definition.relations.iter().flat_map(|table| &table.rows) {
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
        let Some(value) = definition
            .dimensions
            .as_ref()
            .and_then(|table| table.rows.get(usize::try_from(relation.dimension_id).ok()?))
            .and_then(|dimension| dimension.value)
            .filter(|value| value.is_finite() && *value > 0.0)
        else {
            continue;
        };
        candidates.entry(radius_id).or_default().push(value);
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
    for skamp in definition.relations.iter().flat_map(|table| &table.skamps) {
        let [first, second] = skamp.items.as_slice() else {
            continue;
        };
        if skamp.kind != 6 || first.sense != 0 || second.sense != 0 {
            continue;
        }
        let Some(first_radius) = unique_section_skamp_segment(definition, first.entity_id)
            .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
            .and_then(|segment| segment.radius_ref)
        else {
            continue;
        };
        let Some(second_radius) = unique_section_skamp_segment(definition, second.entity_id)
            .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
            .and_then(|segment| segment.radius_ref)
        else {
            continue;
        };
        adjacency
            .entry(first_radius)
            .or_default()
            .insert(second_radius);
        adjacency
            .entry(second_radius)
            .or_default()
            .insert(first_radius);
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

struct SectionIntersectionCarrier {
    geometry: SketchGeometry,
    line_is_bounded: bool,
}

fn section_axis_line_carrier(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    let variables = definition.variables.as_ref()?;
    let (points, _) = variables.reconciled_points();
    let endpoint = |id| points.get(&id);
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

fn section_segment_intersection_carrier(
    definition: &crate::feature::FeatureDefinition,
    radii: &BTreeMap<u32, f64>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SectionIntersectionCarrier> {
    if let Some(geometry) = resolved_section_segment_geometry(definition, points, segment) {
        return Some(SectionIntersectionCarrier {
            line_is_bounded: matches!(geometry, SketchGeometry::Line { .. }),
            geometry,
        });
    }
    if let Some(geometry) = section_axis_line_carrier(definition, segment) {
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
    let Some(segment_table) = &definition.segments else {
        return Some(row.external_id);
    };
    let segments = &segment_table.rows;
    let trim_rows = &definition.trim_entities.as_ref()?.rows;
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

fn resolved_trim_vertex_coordinates(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
) -> BTreeMap<u32, [f64; 2]> {
    let Some(segments) = &definition.segments else {
        return BTreeMap::new();
    };
    let radii = resolved_section_radii(definition);
    let mut coordinate_candidates = definition
        .trim_vertices
        .iter()
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
    for (vertex, entities) in incident {
        let [first_id, second_id] = entities.as_slice() else {
            continue;
        };
        let geometry = |external_id| {
            let segment = segments.segment(external_id)?;
            section_segment_intersection_carrier(definition, &radii, points, segment)
        };
        let (Some(first), Some(second)) = (geometry(*first_id), geometry(*second_id)) else {
            continue;
        };
        let line_arc_is_bounded = match (&first.geometry, &second.geometry) {
            (SketchGeometry::Line { .. }, SketchGeometry::Arc { .. }) => first.line_is_bounded,
            (SketchGeometry::Arc { .. }, SketchGeometry::Line { .. }) => second.line_is_bounded,
            _ => false,
        };
        if let Some(coordinate) = intersect_section_lines(&first.geometry, &second.geometry)
            .or_else(|| {
                line_arc_is_bounded
                    .then(|| intersect_section_line_arc(&first.geometry, &second.geometry))
                    .flatten()
            })
            .or_else(|| intersect_tangent_section_arcs(&first.geometry, &second.geometry))
        {
            coordinate_candidates.push((vertex, coordinate));
        }
    }
    let (mut coordinates, mut ambiguous_vertices) =
        reconciled_section_coordinates(coordinate_candidates);
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
                resolved_section_segment_geometry(definition, points, segment)
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

fn trimmed_section_segment_geometry(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    trim_vertices: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    let trim = definition
        .trim_entities
        .as_ref()?
        .rows
        .iter()
        .find(|row| trim_segment_id(definition, row) == Some(segment.external_id))?;
    let start = trim_vertices.get(&trim.vertices[0])?;
    let end = trim_vertices.get(&trim.vertices[1])?;
    if let Some(geometry) = resolved_section_segment_geometry(definition, points, segment) {
        if !matches!(geometry, SketchGeometry::Line { .. }) {
            return Some(geometry);
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
        let orientation_matches = match segment.vertical_horizontal {
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
        .surface_rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .map(|row| row.id)
        .collect::<BTreeSet<_>>();
    ids.into_iter()
        .filter_map(|id| {
            crate::surface::unique_surface_row(&scan.surface_rows, id)?;
            let outlines = scan
                .outline_planes
                .iter()
                .filter(|plane| plane.surface_id == id)
                .collect::<Vec<_>>();
            match outlines.as_slice() {
                [plane] => Some((plane.origin, plane.normal)),
                [] => {
                    let frames = scan
                        .plane_local_systems
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
    scan.surface_rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .map(|row| row.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|id| {
            crate::surface::unique_surface_row(&scan.surface_rows, id)?;
            let outlines = scan
                .outline_planes
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

#[cfg(test)]
fn extruded_segment_surface(
    transform: &crate::placement::FeatureSectionTransform,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SurfaceGeometry> {
    extruded_geometry_surface(transform, &section_segment_geometry(points, segment)?)
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

#[cfg(test)]
fn placed_section_curve_geometry(
    transform: &crate::placement::FeatureSectionTransform,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<CurveGeometry> {
    placed_section_geometry_curve(transform, &section_segment_geometry(points, segment)?)
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
    for sign in [-1.0, 1.0] {
        let frame = [sign * frame[0], sign * frame[1]];
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
    let [mapping] = matches.as_slice() else {
        return None;
    };
    Some(*mapping)
}

fn is_zero_offset_signed_planar_frame(heads: &[u8]) -> bool {
    matches!(heads, [_, 0x42, z0, _, 0x18, z1]
        if matches!(z0, 0x7f..=0x86) && matches!(z1, 0x7f..=0x86))
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
        let frame = parameters.tabulated_cylinder_frame.or_else(|| {
            let cache = crate::scalar::ScalarCache::default();
            let marker = parameters
                .body
                .windows(3)
                .position(|window| window == [0x00, 0x0c, 0x9a])?;
            let mut cursor = marker + 3;
            let mut values = Vec::with_capacity(6);
            let mut prefixes = Vec::with_capacity(6);
            for slot in 0..6 {
                prefixes.push(*parameters.body.get(cursor)?);
                let (value, next) = if matches!(slot, 0 | 3)
                    || (matches!(slot, 1 | 4) && parameters.body.get(cursor) == Some(&0x2d))
                {
                    crate::scalar::decode_tabulated_cylinder_first_frame_coordinate(
                        &parameters.body,
                        cursor,
                        &cache,
                    )?
                } else {
                    crate::scalar::decode_tabulated_cylinder_frame_coordinate(
                        &parameters.body,
                        cursor,
                        &cache,
                    )?
                };
                values.push(value);
                cursor = next;
            }
            Some(crate::surface::TabulatedCylinderFrame {
                values: values.try_into().ok()?,
                prefixes: prefixes.try_into().ok()?,
            })
        })?;
        let values = frame.values.to_vec();
        let heads = frame.prefixes;
        let resistor_layout = matches!(heads.as_slice(), [_, 0x46, 0x2f, _, 0x46, 0x2e]);
        let zero_offset_layout = is_zero_offset_signed_planar_frame(&heads);
        if resistor_layout {
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
    let local_min = [
        points
            .iter()
            .map(|point| point[0])
            .fold(f64::INFINITY, f64::min),
        points
            .iter()
            .map(|point| point[1])
            .fold(f64::INFINITY, f64::min),
    ];
    let local_max = [
        points
            .iter()
            .map(|point| point[0])
            .fold(f64::NEG_INFINITY, f64::max),
        points
            .iter()
            .map(|point| point[1])
            .fold(f64::NEG_INFINITY, f64::max),
    ];
    let local_span = [local_max[0] - local_min[0], local_max[1] - local_min[1]];
    if local_span
        .iter()
        .any(|span| !span.is_finite() || *span <= 0.0)
    {
        return None;
    }
    let frame_span = std::array::from_fn::<_, 3, _>(|axis| (second[axis] - first[axis]).abs());
    let close = |left: f64, right: f64| {
        (left - right).abs() <= 1.0e-9 * left.abs().max(right.abs()).max(1.0)
    };
    let assignments = (0..3)
        .flat_map(|first_axis| {
            (0..3)
                .filter(move |&second_axis| {
                    first_axis != second_axis
                        && close(frame_span[first_axis], local_span[0])
                        && close(frame_span[second_axis], local_span[1])
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
                    [local_min[0], local_max[0]],
                    [first[*first_axis], second[*first_axis]],
                    first_offset,
                )?,
                signed_unit_chart(
                    [local_min[1], local_max[1]],
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
                                [local_min[0], local_max[0]],
                                [first[*first_axis], second[*first_axis]],
                                first_offset,
                            )?,
                            signed_unit_chart(
                                [local_min[1], local_max[1]],
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
                        first[*first_axis].max(second[*first_axis]) - (point[0] - local_min[0]);
                    let chart_second =
                        first[*second_axis].min(second[*second_axis]) + (point[1] - local_min[1]);
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
    for transform in &scan.feature_section_transforms {
        if unique_feature_section_transform(
            &scan.feature_section_transforms,
            transform.definition_id,
        )
        .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition(&scan.feature_definitions, transform.definition_id)
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
    for transform in &scan.feature_section_transforms {
        if unique_feature_section_transform(
            &scan.feature_section_transforms,
            transform.definition_id,
        )
        .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition(&scan.feature_definitions, transform.definition_id)
        else {
            continue;
        };
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Extrude) {
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
        for segment in definition
            .segments
            .iter()
            .flat_map(|segments| &segments.rows)
            .filter(|segment| solved.contains(&segment.external_id))
        {
            let surface_id = match segment.kind {
                crate::feature::FeatureSegmentKind::Line
                | crate::feature::FeatureSegmentKind::Arc => {
                    order_table.internal_id(segment.external_id).and_then(|_| {
                        generated_surface_id_for_feature(
                            &scan.feature_entity_tables,
                            feature_id,
                            segment.external_id,
                        )
                    })
                }
                crate::feature::FeatureSegmentKind::Point => None,
            };
            let Some(surface_id) = surface_id else {
                continue;
            };
            let id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            let Some(section_geometry) =
                resolved_section_segment_geometry(definition, &points, segment)
            else {
                continue;
            };
            let Some(geometry) = extruded_geometry_surface(transform, &section_geometry) else {
                continue;
            };
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
                &scan.feature_entity_tables,
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
            if !scan.surface_rows.iter().any(|row| {
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
                    &scan.feature_entity_tables,
                    feature_id,
                    external_id,
                )?;
                scan.surface_rows
                    .iter()
                    .any(|row| {
                        row.id == surface_id
                            && row.feature_id == feature_id
                            && row.kind == crate::surface::SurfaceKind::Extrusion
                    })
                    .then_some((surface_id, spline))
            })
            .collect::<Vec<_>>();
        let Some(span) = extrusion_span(
            transform.origin,
            transform.normal,
            feature_plane_equations(scan, feature_id),
        ) else {
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

fn closed_sketch_profile_vertices(ir: &CadIr, sketch_id: &SketchId) -> Vec<(usize, Vec<[f64; 2]>)> {
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
            (profile.len() >= 2).then_some(())?;
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
            uses.iter()
                .enumerate()
                .all(|(index, (_, end))| {
                    let next = uses[(index + 1) % uses.len()].0;
                    (end[0] - next[0]).hypot(end[1] - next[1]) <= 1e-9 * scale
                })
                .then(|| {
                    (
                        profile_index,
                        uses.into_iter().map(|(start, _)| start).collect(),
                    )
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
        SurfaceGeometry::Nurbs(_) | SurfaceGeometry::Unknown { .. } => None,
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
    for (transform_index, transform) in scan.feature_section_transforms.iter().enumerate() {
        if unique_feature_section_transform(
            &scan.feature_section_transforms,
            transform.definition_id,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Revolve)
            || scan.feature_section_transforms[..transform_index]
                .iter()
                .filter_map(|preceding| preceding.feature_id)
                .any(|preceding| feature_recipe(scan, preceding).is_some())
            || unique_feature_revolution_extent_kind(&scan.feature_revolution_extents, feature_id)
                != Some(crate::feature::FeatureRevolutionExtentKind::FullTurn)
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition(&scan.feature_definitions, transform.definition_id)
        else {
            continue;
        };
        let Some(axis) = resolved_revolution_axis(definition, transform) else {
            continue;
        };
        let sketch_id = SketchId(format!("creo:model:sketch#{}", transform.definition_id));
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
                    coedges: vec![coedge_id.clone()],
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
                    pcurve: Some(pcurve),
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
    for (transform_index, transform) in scan.feature_section_transforms.iter().enumerate() {
        if unique_feature_section_transform(
            &scan.feature_section_transforms,
            transform.definition_id,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Extrude)
            || scan.feature_section_transforms[..transform_index]
                .iter()
                .filter_map(|preceding| preceding.feature_id)
                .any(|preceding| feature_recipe(scan, preceding).is_some())
        {
            continue;
        }
        let sketch_id = SketchId(format!("creo:model:sketch#{}", transform.definition_id));
        let Some(sketch) = ir
            .model
            .sketches
            .iter()
            .find(|sketch| sketch.id == sketch_id)
        else {
            continue;
        };
        let [profile] = sketch.profiles.as_slice() else {
            continue;
        };
        let [entity_use] = profile.as_slice() else {
            continue;
        };
        let Some(SketchGeometry::Circle { center, radius }) = ir
            .model
            .sketch_entities
            .iter()
            .find(|entity| entity.id == entity_use.entity && entity.sketch == sketch_id)
            .map(|entity| entity.geometry.clone())
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
        let section_center = [center.u, center.v];
        let center = section_point_in_model(transform, section_center);
        let seam =
            std::array::from_fn::<_, 3, _>(|axis| center[axis] + radius.0 * transform.u_axis[axis]);
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
                circular_pcurve(section_center, radius.0, 0.0, std::f64::consts::TAU),
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
                    radius: radius.0,
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
                coedges: vec![cap_coedge.clone()],
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
                pcurve: Some(cap_pcurve),
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
                radius: radius.0,
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
                coedges: vec![coedge.clone()],
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
                pcurve: Some(pcurve),
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

fn transfer_resolved_extrusion_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for (transform_index, transform) in scan.feature_section_transforms.iter().enumerate() {
        if unique_feature_section_transform(
            &scan.feature_section_transforms,
            transform.definition_id,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Extrude) {
            continue;
        }
        if scan.feature_section_transforms[..transform_index]
            .iter()
            .filter_map(|preceding| preceding.feature_id)
            .any(|preceding| feature_recipe(scan, preceding).is_some())
        {
            continue;
        }
        let sketch_id = SketchId(format!("creo:model:sketch#{}", transform.definition_id));
        let cap_origins = feature_plane_equations(scan, feature_id);
        let Some(span) = extrusion_span(transform.origin, transform.normal, cap_origins) else {
            continue;
        };
        let length = span.upper - span.lower;
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
                coedges: bottom_coedges.clone(),
            });
            ir.model.loops.push(IrLoop {
                id: top_loop.clone(),
                face: top_face.clone(),
                coedges: top_coedges.clone(),
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
                    pcurve: Some(bottom_pcurve),
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
                    pcurve: Some(top_pcurve),
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
                    coedges: coedges.to_vec(),
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
                        pcurve: Some(pcurve),
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
    agreed_feature_recipe(&scan.feature_operation_states, feature_id)
        .map(crate::feature::FeatureRecipe::kind)
}

fn feature_recipe_effect(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipeEffect> {
    agreed_feature_recipe(&scan.feature_operation_states, feature_id)
        .map(crate::feature::FeatureRecipe::effect)
}

fn agreed_feature_recipe(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipe> {
    let mut recipes = operations
        .iter()
        .filter(|operation| operation.feature_id == feature_id)
        .filter_map(|operation| operation.recipe);
    let recipe = recipes.next()?;
    recipes
        .all(|candidate| candidate == recipe)
        .then_some(recipe)
}

fn agreed_feature_recipe_parent(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<u32> {
    let mut parents = operations
        .iter()
        .filter(|operation| operation.feature_id == feature_id && operation.recipe.is_some())
        .map(|operation| operation.parent_feature_id);
    let parent = parents.next()?;
    parents
        .all(|candidate| candidate == parent)
        .then_some(parent)
        .flatten()
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
        &scan.feature_operations,
        feature_row_schema_classes(scan, feature_id),
        feature_id,
    )
}

fn resolved_feature_schema_class_from_classes(
    operations: &[crate::feature::FeatureOperation],
    classes: BTreeSet<u32>,
    feature_id: u32,
) -> Option<u32> {
    if !classes.is_empty() {
        let mut classes = classes.into_iter();
        let schema_class = classes.next()?;
        return classes.next().is_none().then_some(schema_class);
    }
    current_feature_operation(operations, feature_id)
        .and_then(|operation| operation.root_schema_class)
}

fn feature_row_schema_classes(scan: &ContainerScan, feature_id: u32) -> BTreeSet<u32> {
    row_feature_schema_classes(&scan.feature_rows, feature_id)
        .into_iter()
        .chain(row_feature_schema_classes(
            &scan.depdb_recipe_rows,
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

fn feature_revolution_extent(scan: &ContainerScan, feature_id: u32) -> Option<Extent> {
    unique_feature_revolution_extent_kind(&scan.feature_revolution_extents, feature_id).map(
        |kind| match kind {
            crate::feature::FeatureRevolutionExtentKind::FullTurn => Extent::Angle {
                angle: Angle(std::f64::consts::TAU),
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
        SketchConstraintDefinition::Symmetric {
            first,
            second,
            axis,
        } => locus_emitted(first) && locus_emitted(second) && emitted.contains(axis),
        SketchConstraintDefinition::Concentric { first, second }
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
        SketchConstraintDefinition::Coincident { .. }
        | SketchConstraintDefinition::CoincidentLoci { .. }
        | SketchConstraintDefinition::SameCoordinate { .. }
        | SketchConstraintDefinition::Midpoint { .. }
        | SketchConstraintDefinition::Concentric { .. }
        | SketchConstraintDefinition::Collinear { .. }
        | SketchConstraintDefinition::Symmetric { .. }
        | SketchConstraintDefinition::Horizontal { .. }
        | SketchConstraintDefinition::Vertical { .. }
        | SketchConstraintDefinition::Parallel { .. }
        | SketchConstraintDefinition::Perpendicular { .. }
        | SketchConstraintDefinition::Tangent { .. }
        | SketchConstraintDefinition::TangentLoci { .. }
        | SketchConstraintDefinition::Equal { .. }
        | SketchConstraintDefinition::Fixed { .. } => true,
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

fn relation_incidence(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Option<&crate::feature::FeatureSkamp> {
    let Some(relations) = &definition.relations else {
        return None;
    };
    let incidence_ids = relations
        .triples
        .iter()
        .filter(|triple| triple.relation_id == Some(relation_id))
        .filter_map(|triple| triple.skamp_id)
        .collect::<BTreeSet<_>>();
    if incidence_ids.len() != 1 {
        return None;
    }
    let incidence_id = *incidence_ids
        .first()
        .expect("single relation incidence id exists");
    let incidences = relations
        .skamps
        .iter()
        .filter(|skamp| skamp.id == incidence_id)
        .collect::<Vec<_>>();
    let [incidence] = incidences.as_slice() else {
        return None;
    };
    Some(*incidence)
}

fn relation_incidence_entities(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Vec<SketchEntityId> {
    let Some(incidence) = relation_incidence(definition, relation_id) else {
        return Vec::new();
    };
    let known = section_entity_external_ids(definition);
    incidence
        .items
        .iter()
        .map(|item| {
            known.contains(&item.entity_id).then(|| {
                SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id, item.entity_id
                ))
            })
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

fn relation_incidence_known_entities(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Vec<SketchEntityId> {
    let Some(incidence) = relation_incidence(definition, relation_id) else {
        return Vec::new();
    };
    let known = section_entity_external_ids(definition);
    incidence
        .items
        .iter()
        .filter(|item| known.contains(&item.entity_id))
        .map(|item| {
            SketchEntityId(format!(
                "creo:featdefs:sketch_entity#{}:{}",
                definition.id, item.entity_id
            ))
        })
        .collect()
}

fn relation_incidence_loci(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Option<[SketchLocus; 2]> {
    let incidence = relation_incidence(definition, relation_id)?;
    let [first, second] = incidence.items.as_slice() else {
        return None;
    };
    Some([
        section_skamp_locus(definition, first)?,
        section_skamp_locus(definition, second)?,
    ])
}

fn section_dimension_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let Some(relations) = &definition.relations else {
        return Vec::new();
    };
    let segments = definition
        .segments
        .as_ref()
        .map_or(&[][..], |segments| segments.rows.as_slice());
    let known_entities = section_entity_external_ids(definition);
    relations
        .rows
        .iter()
        .map(|relation| {
            let unique_relation_id = relations
                .rows
                .iter()
                .filter(|candidate| candidate.relation_id == relation.relation_id)
                .count()
                == 1;
            let dimension = definition
                .owner_feature_id
                .zip(definition.dimensions.as_ref())
                .and_then(|(owner, dimensions)| {
                    resolved_feature_dimension_parameter(
                        definition.id,
                        owner,
                        dimensions,
                        usize::try_from(relation.dimension_id).ok()?,
                    )
                });
            let parameter = dimension.as_ref().map(|(_, parameter)| parameter.clone());
            let typed = (|| {
                unique_relation_id.then_some(())?;
                let (dimension, _) = dimension.as_ref()?;
                if dimension.value_unit != crate::feature::DimensionUnit::Millimeters {
                    return None;
                }
                let parameter = parameter.clone()?;
                if relation.relation_type == 14
                    && relation.sign == 1
                    && relation.operand_vectors?[1] == [Some(0); 4]
                    && relation.operand_vectors?[2] == [Some(15), Some(0), Some(0), Some(0)]
                {
                    let vectors = relation.operand_vectors?;
                    let [Some(radius_id), Some(0), Some(0), Some(0)] = vectors[0] else {
                        return None;
                    };
                    let matching = segments
                        .iter()
                        .filter(|segment| {
                            segment.kind == crate::feature::FeatureSegmentKind::Arc
                                && segment.radius_ref == Some(radius_id)
                        })
                        .collect::<Vec<_>>();
                    let [segment] = matching.as_slice() else {
                        return None;
                    };
                    known_entities
                        .contains(&segment.external_id)
                        .then_some(())?;
                    return Some(SketchConstraintDefinition::Radius {
                        entity: SketchEntityId(format!(
                            "creo:featdefs:sketch_entity#{}:{}",
                            definition.id, segment.external_id
                        )),
                        parameter,
                    });
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
                                known_entities
                                    .contains(&measured.external_id)
                                    .then_some(())?;
                                let entity = SketchEntityId(format!(
                                    "creo:featdefs:sketch_entity#{}:{}",
                                    definition.id, measured.external_id
                                ));
                                let [first, second] = if measured.point_ids == [first_id, second_id]
                                {
                                    [SketchLocus::Start(entity.clone()), SketchLocus::End(entity)]
                                } else {
                                    [SketchLocus::End(entity.clone()), SketchLocus::Start(entity)]
                                };
                                match measured.vertical_horizontal {
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
                    }
                }
                if let Some([first, second]) =
                    relation_incidence_loci(definition, relation.relation_id)
                {
                    return Some(SketchConstraintDefinition::DistanceLoci {
                        first,
                        second,
                        parameter,
                    });
                }
                let entities = relation_incidence_entities(definition, relation.relation_id);
                (!entities.is_empty()).then_some(SketchConstraintDefinition::Distance {
                    entities,
                    parameter,
                })
            })();
            let incidence_entities = if unique_relation_id {
                relation_incidence_known_entities(definition, relation.relation_id)
            } else {
                Vec::new()
            };
            let constraint_definition =
                typed.unwrap_or_else(|| SketchConstraintDefinition::Native {
                    native_kind: format!("creo:relation:{}", relation.relation_type),
                    entities: incidence_entities,
                    parameter,
                    operands: vec![SketchNativeOperand {
                        native_kind: "relat_ptr".to_string(),
                        object_index: relation.relation_id,
                        native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
                    }],
                });
            (
                SketchConstraint {
                    id: SketchConstraintId(if unique_relation_id {
                        format!(
                            "creo:featdefs:sketch_constraint#{}:relation:{}",
                            definition.id, relation.relation_id
                        )
                    } else {
                        format!(
                            "creo:featdefs:sketch_constraint#{}:relation:offset:{}",
                            definition.id, relation.offset
                        )
                    }),
                    sketch: sketch.clone(),
                    definition: constraint_definition,
                    native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
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

fn section_skamp_locus(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    let entity = SketchEntityId(format!(
        "creo:featdefs:sketch_entity#{}:{}",
        definition.id, item.entity_id
    ));
    if let Some(segment) = unique_section_skamp_segment(definition, item.entity_id) {
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
    if definition
        .segments
        .iter()
        .flat_map(|segments| &segments.rows)
        .any(|segment| segment.external_id == item.entity_id)
    {
        return None;
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

fn section_skamp_endpoint(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    matches!(item.sense, 2 | 3)
        .then(|| section_skamp_locus(definition, item))
        .flatten()
}

fn section_skamp_point_locus(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    matches!(item.sense, 2..=4)
        .then(|| section_skamp_locus(definition, item))
        .flatten()
}

fn section_skamp_line_pair(
    definition: &crate::feature::FeatureDefinition,
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
    Some([first, second].map(|item| {
        SketchEntityId(format!(
            "creo:featdefs:sketch_entity#{}:{}",
            definition.id, item.entity_id
        ))
    }))
}

fn section_skamp_same_coordinate(
    definition: &crate::feature::FeatureDefinition,
    first: &crate::feature::FeatureSkampItem,
    second: &crate::feature::FeatureSkampItem,
) -> Option<(SketchLocus, SketchLocus, SketchCoordinateAxis)> {
    let first_locus = section_skamp_point_locus(definition, first)?;
    let second_locus = section_skamp_point_locus(definition, second)?;
    let points = resolved_section_points(definition);
    let first_point = points.get(&section_skamp_selected_point_id(definition, first)?)?;
    let second_point = points.get(&section_skamp_selected_point_id(definition, second)?)?;
    let scale = first_point
        .iter()
        .chain(second_point)
        .map(|coordinate| coordinate.abs())
        .fold(1.0, f64::max);
    let tolerance = 1e-9 * scale;
    let equal = [
        (first_point[0] - second_point[0]).abs() <= tolerance,
        (first_point[1] - second_point[1]).abs() <= tolerance,
    ];
    let axis = match equal {
        [true, false] => SketchCoordinateAxis::U,
        [false, true] => SketchCoordinateAxis::V,
        _ => return None,
    };
    Some((first_locus, second_locus, axis))
}

fn section_skamp_is_line(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    let has_segment = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == item.entity_id);
    if has_segment {
        return unique_section_skamp_segment(definition, item.entity_id)
            .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line);
    }
    section_saved_entity(definition, item.entity_id)
        .is_some_and(|entity| matches!(entity, crate::feature::FeatureSavedEntity::Line(_)))
}

fn section_skamp_is_point(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    unique_section_skamp_segment(definition, item.entity_id)
        .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Point)
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
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchEntityId> {
    if item.sense != 0 {
        return None;
    }
    let has_segment = definition
        .segments
        .iter()
        .flat_map(|segments| &segments.rows)
        .any(|segment| segment.external_id == item.entity_id);
    let circular = if has_segment {
        unique_section_skamp_segment(definition, item.entity_id)
            .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
    } else {
        section_saved_entity(definition, item.entity_id).is_some_and(|entity| {
            matches!(
                entity,
                crate::feature::FeatureSavedEntity::Arc(_)
                    | crate::feature::FeatureSavedEntity::Circle(_)
            )
        })
    };
    circular.then(|| {
        SketchEntityId(format!(
            "creo:featdefs:sketch_entity#{}:{}",
            definition.id, item.entity_id
        ))
    })
}

fn section_skamp_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let Some(relations) = &definition.relations else {
        return Vec::new();
    };
    let section_entities = section_entity_external_ids(definition);
    relations
        .skamps
        .iter()
        .filter_map(|skamp| {
            let unique_skamp_id = relations
                .skamps
                .iter()
                .filter(|candidate| candidate.id == skamp.id)
                .count()
                == 1;
            let native_constraint = || {
                let entities = skamp
                    .items
                    .iter()
                    .filter(|item| section_entities.contains(&item.entity_id))
                    .map(|item| {
                        SketchEntityId(format!(
                            "creo:featdefs:sketch_entity#{}:{}",
                            definition.id, item.entity_id
                        ))
                    })
                    .collect::<Vec<_>>();
                Some(SketchConstraintDefinition::Native {
                    native_kind: format!("creo:skamp:{}", skamp.kind),
                    entities,
                    parameter: None,
                    operands: skamp
                        .items
                        .iter()
                        .map(|item| SketchNativeOperand {
                            native_kind: format!("sense:{}", item.sense),
                            object_index: item.entity_id,
                            native_ref: None,
                        })
                        .collect(),
                })
            };
            let constraint_definition = if unique_skamp_id {
                match (skamp.kind, skamp.items.as_slice()) {
                    (0, [first, second])
                        if section_skamp_endpoint(definition, first).is_some()
                            && section_skamp_endpoint(definition, second).is_some() =>
                    {
                        SketchConstraintDefinition::CoincidentLoci {
                            loci: vec![
                                section_skamp_endpoint(definition, first)?,
                                section_skamp_endpoint(definition, second)?,
                            ],
                        }
                    }
                    (3, [first, second])
                        if (first.sense == 0
                            && section_skamp_locus(definition, first).is_some()
                            && section_skamp_point_locus(definition, second).is_some())
                            || (second.sense == 0
                                && section_skamp_locus(definition, second).is_some()
                                && section_skamp_point_locus(definition, first).is_some()) =>
                    {
                        SketchConstraintDefinition::CoincidentLoci {
                            loci: vec![
                                section_skamp_locus(definition, first)?,
                                section_skamp_locus(definition, second)?,
                            ],
                        }
                    }
                    (1, [item]) if item.sense == 0 && section_skamp_is_line(definition, item) => {
                        SketchConstraintDefinition::Horizontal {
                            entity: SketchEntityId(format!(
                                "creo:featdefs:sketch_entity#{}:{}",
                                definition.id, item.entity_id
                            )),
                        }
                    }
                    (2, [item]) if item.sense == 0 && section_skamp_is_line(definition, item) => {
                        SketchConstraintDefinition::Vertical {
                            entity: SketchEntityId(format!(
                                "creo:featdefs:sketch_entity#{}:{}",
                                definition.id, item.entity_id
                            )),
                        }
                    }
                    (4, [first, second])
                        if section_skamp_endpoint(definition, first).is_some()
                            && section_skamp_endpoint(definition, second).is_some() =>
                    {
                        SketchConstraintDefinition::TangentLoci {
                            first: section_skamp_endpoint(definition, first)?,
                            second: section_skamp_endpoint(definition, second)?,
                        }
                    }
                    (5, [first, second])
                        if section_skamp_line_pair(definition, first, second).is_some() =>
                    {
                        let [first, second] = section_skamp_line_pair(definition, first, second)?;
                        SketchConstraintDefinition::Perpendicular { first, second }
                    }
                    (6, [first, second])
                        if section_skamp_circular_entity(definition, first).is_some()
                            && section_skamp_circular_entity(definition, second).is_some() =>
                    {
                        SketchConstraintDefinition::Equal {
                            first: section_skamp_circular_entity(definition, first)?,
                            second: section_skamp_circular_entity(definition, second)?,
                        }
                    }
                    (7, [first, second])
                        if section_skamp_line_pair(definition, first, second).is_some() =>
                    {
                        let [first, second] = section_skamp_line_pair(definition, first, second)?;
                        SketchConstraintDefinition::Parallel { first, second }
                    }
                    (8, [first, second])
                        if section_skamp_line_pair(definition, first, second).is_some() =>
                    {
                        let [first, second] = section_skamp_line_pair(definition, first, second)?;
                        SketchConstraintDefinition::Equal { first, second }
                    }
                    (9, [first, second])
                        if first.sense == 0
                            && second.sense == 0
                            && ((section_skamp_is_line(definition, first)
                                && section_skamp_is_point(definition, second))
                                || (section_skamp_is_point(definition, first)
                                    && section_skamp_is_line(definition, second))) =>
                    {
                        SketchConstraintDefinition::CoincidentLoci {
                            loci: vec![
                                section_skamp_locus(definition, first)?,
                                section_skamp_locus(definition, second)?,
                            ],
                        }
                    }
                    (14, [axis, first, second])
                        if axis.sense == 0
                            && section_skamp_is_line(definition, axis)
                            && section_skamp_point_locus(definition, first).is_some()
                            && section_skamp_point_locus(definition, second).is_some() =>
                    {
                        SketchConstraintDefinition::Symmetric {
                            first: section_skamp_point_locus(definition, first)?,
                            second: section_skamp_point_locus(definition, second)?,
                            axis: SketchEntityId(format!(
                                "creo:featdefs:sketch_entity#{}:{}",
                                definition.id, axis.entity_id
                            )),
                        }
                    }
                    (17, [first, second])
                        if section_skamp_same_coordinate(definition, first, second).is_some() =>
                    {
                        let (first, second, axis) =
                            section_skamp_same_coordinate(definition, first, second)?;
                        SketchConstraintDefinition::SameCoordinate {
                            first,
                            second,
                            axis,
                        }
                    }
                    _ => native_constraint()?,
                }
            } else {
                native_constraint()?
            };
            Some((
                SketchConstraint {
                    id: SketchConstraintId(if unique_skamp_id {
                        format!(
                            "creo:featdefs:sketch_constraint#{}:skamp:{}",
                            definition.id, skamp.id
                        )
                    } else {
                        format!(
                            "creo:featdefs:sketch_constraint#{}:skamp:offset:{}",
                            definition.id, skamp.offset
                        )
                    }),
                    sketch: sketch.clone(),
                    definition: constraint_definition,
                    native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
                },
                skamp.offset,
            ))
        })
        .collect()
}

fn section_entity_external_ids(definition: &crate::feature::FeatureDefinition) -> BTreeSet<u32> {
    let mut ids = unique_section_segment_external_ids(
        definition
            .segments
            .iter()
            .flat_map(|segments| &segments.rows),
    );
    let Some(order) = &definition.order_table else {
        return ids;
    };
    let ambiguous_segment_ids = ambiguous_section_segment_external_ids(
        definition
            .segments
            .iter()
            .flat_map(|segments| &segments.rows),
    );
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
    let id = SketchEntityId(external_id.map_or_else(
        || match kind {
            SavedSectionEntityKind::Spline => {
                format!("creo:featdefs:saved_spline#{}:{suffix}", definition.id)
            }
            SavedSectionEntityKind::Dummy => {
                format!("creo:featdefs:saved_dummy#{}:{suffix}", definition.id)
            }
            _ => format!("creo:featdefs:sketch_entity#{}:{suffix}", definition.id),
        },
        |external_id| {
            format!(
                "creo:featdefs:sketch_entity#{}:{external_id}",
                definition.id
            )
        },
    ));
    (
        SketchEntity {
            id,
            sketch: sketch.clone(),
            construction: true,
            native_ref: Some(feature_sketch_record_id(definition)),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Native {
                native_kind: format!("saved_{}", kind.name()),
            },
        },
        offset,
    )
}

fn unique_section_segment_external_ids<'a>(
    segments: impl IntoIterator<Item = &'a crate::feature::FeatureSegment>,
) -> BTreeSet<u32> {
    segments
        .into_iter()
        .fold(BTreeMap::new(), |mut counts, segment| {
            *counts.entry(segment.external_id).or_insert(0usize) += 1;
            counts
        })
        .into_iter()
        .filter_map(|(external_id, count)| (count == 1).then_some(external_id))
        .collect()
}

fn ambiguous_section_segment_external_ids<'a>(
    segments: impl IntoIterator<Item = &'a crate::feature::FeatureSegment>,
) -> BTreeSet<u32> {
    segments
        .into_iter()
        .fold(BTreeMap::new(), |mut counts, segment| {
            *counts.entry(segment.external_id).or_insert(0usize) += 1;
            counts
        })
        .into_iter()
        .filter_map(|(external_id, count)| (count > 1).then_some(external_id))
        .collect()
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
    emitted: &BTreeSet<u32>,
) -> Vec<Vec<SketchEntityUse>> {
    let Some(table) = &definition.trim_entities else {
        return resolved_segment_profile_chains(definition, emitted);
    };
    let rows = table
        .rows
        .iter()
        .filter_map(|row| Some((row, trim_segment_id(definition, row)?)))
        .collect::<Vec<_>>();
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
                entity: SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id, external_id
                )),
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
    emitted: &BTreeSet<u32>,
) -> Vec<Vec<SketchEntityUse>> {
    let Some(table) = &definition.segments else {
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
                entity: SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id, segment.external_id
                )),
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

fn transfer_resolved_sketches(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for transform in &scan.feature_section_transforms {
        if unique_feature_section_transform(
            &scan.feature_section_transforms,
            transform.definition_id,
        )
        .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition(&scan.feature_definitions, transform.definition_id)
        else {
            continue;
        };
        let segments = definition
            .segments
            .as_ref()
            .map_or(&[][..], |segments| segments.rows.as_slice());
        let unique_segment_ids = unique_section_segment_external_ids(segments);
        let ambiguous_segment_ids = ambiguous_section_segment_external_ids(segments);
        let points = resolved_section_points(definition);
        let solved = definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        let trim_vertex_coordinates = resolved_trim_vertex_coordinates(definition, &points);
        let emitted = segments
            .iter()
            .filter(|segment| {
                if !unique_segment_ids.contains(&segment.external_id) {
                    return false;
                }
                if solved.contains(&segment.external_id) {
                    trimmed_section_segment_geometry(
                        definition,
                        &points,
                        &trim_vertex_coordinates,
                        segment,
                    )
                    .is_some()
                } else {
                    resolved_section_segment_geometry(definition, &points, segment).is_some()
                }
            })
            .map(|segment| segment.external_id)
            .collect::<BTreeSet<_>>();
        let resolved_segment_offsets = segments
            .iter()
            .filter(|segment| {
                if unique_segment_ids.contains(&segment.external_id)
                    && solved.contains(&segment.external_id)
                {
                    trimmed_section_segment_geometry(
                        definition,
                        &points,
                        &trim_vertex_coordinates,
                        segment,
                    )
                    .is_some()
                } else {
                    resolved_section_segment_geometry(definition, &points, segment).is_some()
                }
            })
            .map(|segment| segment.offset)
            .collect::<BTreeSet<_>>();
        let mut profiles = resolved_profile_chains(definition, &emitted);
        let profile_segments = segments
            .iter()
            .filter(|segment| {
                let id = SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id, segment.external_id
                ));
                profiles
                    .iter()
                    .flatten()
                    .any(|entity_use| entity_use.entity == id)
            })
            .map(|segment| segment.external_id)
            .collect::<BTreeSet<_>>();
        let sketch_id = SketchId(format!("creo:model:sketch#{}", definition.id));
        let mut entities = segments
            .iter()
            .filter_map(|segment| {
                let geometry = if unique_segment_ids.contains(&segment.external_id)
                    && solved.contains(&segment.external_id)
                {
                    trimmed_section_segment_geometry(
                        definition,
                        &points,
                        &trim_vertex_coordinates,
                        segment,
                    )?
                } else {
                    resolved_section_segment_geometry(definition, &points, segment)?
                };
                let id = SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id,
                    section_segment_identity_suffix(&unique_segment_ids, segment)
                ));
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
                Some(SketchEntity {
                    id,
                    sketch: sketch_id.clone(),
                    construction: !unique_segment_ids.contains(&segment.external_id)
                        || (!solved.contains(&segment.external_id)
                            && !profile_segments.contains(&segment.external_id)),
                    native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
                    geometry_ref: Some(format!(
                        "creo:featdefs:section_curve#{}:{}",
                        definition.id,
                        section_segment_identity_suffix(&unique_segment_ids, segment)
                    )),
                    endpoint_refs: match segment.kind {
                        crate::feature::FeatureSegmentKind::Arc => {
                            vec![segment.point_ids[1], segment.point_ids[0]]
                        }
                        crate::feature::FeatureSegmentKind::Line => segment.point_ids.to_vec(),
                        crate::feature::FeatureSegmentKind::Point => vec![segment.point_ids[0]],
                    }
                    .into_iter()
                    .map(|point| format!("creo:featdefs:sketch#{}:point#{point}", definition.id))
                    .collect(),
                    geometry,
                })
            })
            .collect::<Vec<_>>();
        for segment in segments
            .iter()
            .filter(|segment| !resolved_segment_offsets.contains(&segment.offset))
        {
            let id = SketchEntityId(format!(
                "creo:featdefs:sketch_entity#{}:{}",
                definition.id,
                section_segment_identity_suffix(&unique_segment_ids, segment)
            ));
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
                native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
                geometry_ref: None,
                endpoint_refs: match segment.kind {
                    crate::feature::FeatureSegmentKind::Arc => {
                        vec![segment.point_ids[1], segment.point_ids[0]]
                    }
                    crate::feature::FeatureSegmentKind::Line => segment.point_ids.to_vec(),
                    crate::feature::FeatureSegmentKind::Point => vec![segment.point_ids[0]],
                }
                .into_iter()
                .map(|point| format!("creo:featdefs:sketch#{}:point#{point}", definition.id))
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
        let mut saved_section_geometries = Vec::new();
        let mut generated_saved_geometries = Vec::new();
        let unique_saved_ids = unique_saved_section_internal_ids(definition);
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
            let entity_id = SketchEntityId(format!(
                "creo:featdefs:sketch_entity#{}:{suffix}",
                definition.id
            ));
            if entities.iter().any(|entity| entity.id == entity_id) {
                continue;
            }
            let generated_kind = match &geometry {
                SketchGeometry::Line { .. } => crate::surface::SurfaceKind::Plane,
                SketchGeometry::Arc { .. } | SketchGeometry::Circle { .. } => {
                    crate::surface::SurfaceKind::Cylinder
                }
                _ => continue,
            };
            let generated = external_id.is_some_and(|external_id| {
                saved_entity_is_generated_profile(
                    definition.owner_feature_id,
                    external_id,
                    generated_kind,
                    &scan.feature_entity_tables,
                    &scan.surface_rows,
                )
            });
            let curve_id = CurveId(format!(
                "creo:featdefs:section_curve#{}:{suffix}",
                definition.id
            ));
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
                native_ref: Some(format!("creo:featdefs:saved_entity#{internal_id}")),
                geometry_ref: Some(curve_id.0.clone()),
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
            let Some(nurbs) = saved_spline_nurbs(spline) else {
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
                saved_entity_is_generated_profile(
                    definition.owner_feature_id,
                    external_id,
                    crate::surface::SurfaceKind::Spline,
                    &scan.feature_entity_tables,
                    &scan.surface_rows,
                )
            });
            let entity_id = SketchEntityId(external_id.map_or_else(
                || format!("creo:featdefs:saved_spline#{}:{suffix}", definition.id),
                |external_id| {
                    format!(
                        "creo:featdefs:sketch_entity#{}:{external_id}",
                        definition.id
                    )
                },
            ));
            let curve_id = CurveId(format!(
                "creo:featdefs:saved_spline_curve#{}:{suffix}",
                definition.id
            ));
            if entities.iter().any(|entity| entity.id == entity_id) {
                continue;
            }
            if nurbs
                .control_points
                .iter()
                .any(|point| point.z.abs() > 1e-12)
            {
                continue;
            }
            let geometry = SketchGeometry::Nurbs {
                degree: nurbs.degree,
                knots: nurbs.knots.clone(),
                control_points: nurbs
                    .control_points
                    .iter()
                    .map(|point| cadmpeg_ir::math::Point2::new(point.x, point.y))
                    .collect(),
                weights: None,
                periodic: false,
            };
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
                native_ref: Some(format!("creo:featdefs:saved_spline#{suffix}")),
                geometry_ref: Some(curve_id.0.clone()),
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
            definition.id,
            &generated_saved_geometries,
        ));
        if entities.is_empty() {
            continue;
        }
        for segment in segments {
            let Some(section_geometry) =
                resolved_section_segment_geometry(definition, &points, segment)
            else {
                continue;
            };
            let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry) else {
                continue;
            };
            let id = CurveId(format!(
                "creo:featdefs:section_curve#{}:{}",
                definition.id,
                section_segment_identity_suffix(&unique_segment_ids, segment)
            ));
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
                        "FeatDefs:section#{}:{}",
                        definition.id,
                        section_segment_identity_suffix(&unique_segment_ids, segment)
                    ),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        for (internal_id, external_id, section_geometry, offset, id) in saved_section_geometries {
            if ir.model.curves.iter().any(|existing| existing.id == id) {
                continue;
            }
            let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry) else {
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
                        |external_id| format!("FeatDefs:section#{}:{external_id}", definition.id),
                    ),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        let emitted_entity_ids = entities
            .iter()
            .map(|entity| entity.id.clone())
            .collect::<BTreeSet<_>>();
        let mut constraints = segments
            .iter()
            .filter_map(|segment| {
                let entity = SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id,
                    section_segment_identity_suffix(&unique_segment_ids, segment)
                ));
                let mut constraint_definition = line_orientation_definition(segment, entity)?;
                reconcile_constraint_entity_references(
                    &mut constraint_definition,
                    &emitted_entity_ids,
                )
                .then_some(())?;
                let id = SketchConstraintId(format!(
                    "creo:featdefs:sketch_constraint#{}:verhor:{}",
                    definition.id,
                    section_segment_identity_suffix(&unique_segment_ids, segment)
                ));
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
                    native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
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
        for (mut constraint, offset) in section_skamp_constraints(definition, &sketch_id) {
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
        annotate(
            annotations,
            &sketch_id.0,
            "FeatDefs",
            transform.offset as u64,
            "datum_placed_section",
            Exactness::Derived,
        );
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
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
            profiles,
            native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
        });
        if owned_section_feature_id(scan, definition.id).is_some() {
            continue;
        }
        let feature_id = IrFeatureId(format!("creo:model:sketch_feature#{}", definition.id));
        annotate(
            annotations,
            &feature_id.0,
            "FeatDefs",
            transform.offset as u64,
            "section_sketch_feature",
            Exactness::Derived,
        );
        ir.model.features.push(Feature {
            id: feature_id,
            ordinal: ir.model.features.len() as u64,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("section".to_string()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: IrFeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: Some(sketch_id),
            },
            native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
        });
    }
}

fn link_feature_sketch_history(scan: &ContainerScan, ir: &mut CadIr) {
    let links = scan
        .feature_section_transforms
        .iter()
        .filter(|transform| {
            unique_feature_section_transform(
                &scan.feature_section_transforms,
                transform.definition_id,
            )
            .is_some()
        })
        .filter_map(|transform| {
            let owner = IrFeatureId(format!("creo:model:feature#{}", transform.feature_id?));
            let sketch_feature = owned_section_feature_id(scan, transform.definition_id)
                .map_or_else(
                    || {
                        IrFeatureId(format!(
                            "creo:model:sketch_feature#{}",
                            transform.definition_id
                        ))
                    },
                    |feature_id| IrFeatureId(format!("creo:model:feature#{feature_id}")),
                );
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
        SurfaceGeometry::Unknown { .. } => None,
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

fn saved_entity_is_generated_profile(
    feature_id: Option<u32>,
    source_entity_id: u32,
    expected_kind: crate::surface::SurfaceKind,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> bool {
    let Some(feature_id) = feature_id else {
        return false;
    };
    let direct = generated_surface_id_for_feature(tables, feature_id, source_entity_id)
        .is_some_and(|surface_id| {
            crate::surface::unique_surface_row(rows, surface_id)
                .is_some_and(|row| row.feature_id == feature_id && row.kind == expected_kind)
        });
    if direct {
        return true;
    }
    if expected_kind != crate::surface::SurfaceKind::Cylinder {
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

fn ordered_analytic_surface_id_for_feature(
    surface_rows: &[crate::surface::SurfaceRow],
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
    order: &crate::feature::FeatureOrderTable,
    external_id: u32,
    geometry: &SurfaceGeometry,
) -> Option<u32> {
    order.internal_id(external_id)?;
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
    for transform in &scan.feature_section_transforms {
        if unique_feature_section_transform(
            &scan.feature_section_transforms,
            transform.definition_id,
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
        if unique_feature_revolution_extent_kind(&scan.feature_revolution_extents, feature_id)
            .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition(&scan.feature_definitions, transform.definition_id)
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
            let segments = definition
                .segments
                .iter()
                .flat_map(|table| &table.rows)
                .cloned()
                .collect::<Vec<_>>();
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
                    &scan.surface_rows,
                    feature_id,
                    &scan.feature_entity_tables,
                    order,
                    definition
                        .segments
                        .iter()
                        .flat_map(|segments| &segments.rows)
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
                    &scan.surface_rows,
                    feature_id,
                    &scan.feature_entity_tables,
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
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| &table.rows)
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
                            &scan.surface_rows,
                            &scan.feature_entity_tables,
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
                    &scan.surface_rows,
                    &scan.feature_entity_tables,
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
                    ],
                    transposed: false,
                },
                cache_fit_tolerance: None,
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
    for transform in &scan.feature_section_transforms {
        if unique_feature_section_transform(
            &scan.feature_section_transforms,
            transform.definition_id,
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
            unique_feature_definition(&scan.feature_definitions, transform.definition_id)
        else {
            continue;
        };
        let Some(axis) = resolved_revolution_axis(definition, transform) else {
            continue;
        };
        let sketch_id = SketchId(format!("creo:model:sketch#{}", definition.id));
        for (profile_index, vertices) in closed_sketch_profile_vertices(ir, &sketch_id) {
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
    for transform in &scan.feature_section_transforms {
        if unique_feature_section_transform(
            &scan.feature_section_transforms,
            transform.definition_id,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Extrude) {
            continue;
        }
        let sketch_id = SketchId(format!("creo:model:sketch#{}", transform.definition_id));
        for (profile_index, vertices) in closed_sketch_profile_vertices(ir, &sketch_id) {
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

fn feature_dimension_parameter_id(
    definition_id: u32,
    owner_feature_id: u32,
    external_id: u32,
) -> ParameterId {
    ParameterId(format!(
        "creo:featdefs:parameter#{definition_id}:{owner_feature_id}:{external_id}"
    ))
}

fn feature_dimension_parameter_row_id(
    definition_id: u32,
    owner_feature_id: u32,
    external_id: u32,
    occurrence: Option<usize>,
) -> ParameterId {
    occurrence.map_or_else(
        || feature_dimension_parameter_id(definition_id, owner_feature_id, external_id),
        |occurrence| {
            ParameterId(format!(
                "creo:featdefs:parameter#{definition_id}:{owner_feature_id}:{external_id}:{}",
                occurrence + 1
            ))
        },
    )
}

fn resolved_feature_dimension_parameter(
    definition_id: u32,
    owner_feature_id: u32,
    table: &crate::feature::FeatureDimensionTable,
    ordinal: usize,
) -> Option<(&crate::feature::FeatureDimension, ParameterId)> {
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
                feature_dimension_parameter_id(
                    definition_id,
                    owner_feature_id,
                    dimension.external_id,
                ),
            )
        })
}

fn feature_dimension_parameter_layout(
    keys: &[(u32, u32, u32)],
) -> Option<Vec<(u32, String, Option<usize>)>> {
    let mut name_counts = BTreeMap::new();
    let mut local_counts = BTreeMap::new();
    for &(owner_feature_id, _, external_id) in keys {
        *name_counts
            .entry((owner_feature_id, external_id))
            .or_insert(0usize) += 1;
    }
    for &key in keys {
        *local_counts.entry(key).or_insert(0usize) += 1;
    }
    let mut next_ordinals = BTreeMap::<u32, u32>::new();
    let mut local_occurrences = BTreeMap::new();
    keys.iter()
        .map(|&(owner_feature_id, definition_id, external_id)| {
            let ordinal = next_ordinals.entry(owner_feature_id).or_default();
            let assigned = *ordinal;
            *ordinal = ordinal.checked_add(1)?;
            let key = (owner_feature_id, definition_id, external_id);
            let occurrence = (local_counts[&key] > 1).then(|| {
                let occurrence = local_occurrences.entry(key).or_insert(0usize);
                let assigned = *occurrence;
                *occurrence += 1;
                assigned
            });
            let name = if name_counts[&(owner_feature_id, external_id)] == 1 {
                format!("d{external_id}")
            } else if let Some(occurrence) = occurrence {
                format!("d{definition_id}_{external_id}_{}", occurrence + 1)
            } else {
                format!("d{definition_id}_{external_id}")
            };
            Some((assigned, name, occurrence))
        })
        .collect()
}

fn transfer_feature_dimensions(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    let feature_ids = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<BTreeSet<_>>();
    let mut candidates = Vec::new();
    for definition in &scan.feature_definitions {
        let Some(owner_feature_id) = definition.owner_feature_id else {
            continue;
        };
        let owner = IrFeatureId(format!("creo:model:feature#{owner_feature_id}"));
        if !feature_ids.contains(&owner) {
            continue;
        }
        let Some(table) = &definition.dimensions else {
            continue;
        };
        for (source_ordinal, dimension) in table.rows.iter().enumerate() {
            candidates.push((owner_feature_id, definition, source_ordinal, dimension));
        }
    }
    candidates.sort_by_key(|(owner, definition, source_ordinal, _)| {
        (*owner, definition.offset, definition.id, *source_ordinal)
    });
    let keys = candidates
        .iter()
        .map(|(owner, definition, _, dimension)| (*owner, definition.id, dimension.external_id))
        .collect::<Vec<_>>();
    let Some(layout) = feature_dimension_parameter_layout(&keys) else {
        return;
    };
    for ((owner_feature_id, definition, source_ordinal, dimension), (ordinal, name, occurrence)) in
        candidates.into_iter().zip(layout)
    {
        let owner = IrFeatureId(format!("creo:model:feature#{owner_feature_id}"));
        let id = feature_dimension_parameter_row_id(
            definition.id,
            owner_feature_id,
            dimension.external_id,
            occurrence,
        );
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
            owner: owner.clone(),
            ordinal,
            name,
            expression,
            display: (dimension.dimension_type == 0x03).then_some(DimensionDisplay::Radius),
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
            .find(|feature| feature.id == owner)
        {
            feature
                .source_content
                .push(FeatureSourceContent::Parameter(id));
        }
    }
}

fn feature_output_bodies(scan: &ContainerScan, ir: &CadIr, feature_id: u32) -> Vec<BodyId> {
    let affected_geometry = agreed_feature_affected_ids(
        &scan.feature_affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Geometry,
    );
    let generated_surfaces = scan
        .surface_rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .map(|row| SurfaceId(format!("creo:visibgeom:surface#{}", row.id)))
        .chain(
            scan.feature_entity_tables
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
    for surface in generated_surfaces {
        for face in ir.model.faces.iter().filter(|face| face.surface == surface) {
            let Some(shell) = ir.model.shells.iter().find(|shell| shell.id == face.shell) else {
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
    outputs
}

fn evaluated_sweep_output_bodies(ir: &CadIr, feature_id: u32) -> Vec<BodyId> {
    ["extrusion", "revolution"]
        .into_iter()
        .map(|family| BodyId(format!("creo:feature:{family}#{feature_id}:body")))
        .filter(|id| ir.model.bodies.iter().any(|body| body.id == *id))
        .collect()
}

fn has_evaluated_sweep_body(ir: &CadIr, family: &str, feature_id: u32) -> bool {
    let id = BodyId(format!("creo:feature:{family}#{feature_id}:body"));
    ir.model.bodies.iter().any(|body| body.id == id)
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
        .feature_choice_fields
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
        .feature_affected_ids
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
        .feature_replay_affected_ids
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
        .feature_loop_restore_directions
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
        unique_feature_revolution_extent_kind(&scan.feature_revolution_extents, feature_id)
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
        .feature_entity_tables
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
        .feature_definitions
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
        .feature_section_transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
    {
        insert_feature_parameter(
            &mut parameters,
            "profile_sketch",
            format!("creo:model:sketch#{}", transform.definition_id),
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
        916 | 917 => Some("Protrusion"),
        923 => Some("Datum Plane"),
        926 => Some("Section"),
        _ => None,
    }
}

fn owned_section_feature_id(scan: &ContainerScan, definition_id: u32) -> Option<u32> {
    let definitions = scan
        .feature_definitions
        .iter()
        .filter(|definition| definition.id == definition_id)
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return None;
    };
    let rows = scan
        .feature_rows
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
        .feature_rows
        .iter()
        .filter(|row| row.feature_id == feature_id && row.root_schema_class == Some(926))
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return None;
    };
    let definitions = scan
        .feature_definitions
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
    if let Some(recipe) = agreed_feature_recipe(&scan.feature_operation_states, feature_id) {
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

fn feature_dependencies(scan: &ContainerScan, ir: &CadIr, feature_id: u32) -> Vec<IrFeatureId> {
    agreed_feature_parent_ids(&scan.feature_affected_ids, feature_id)
        .into_iter()
        .chain(agreed_feature_recipe_parent(
            &scan.feature_operation_states,
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

fn reconcile_feature_links(scan: &ContainerScan, ir: &mut CadIr) {
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
        let native_dependencies = agreed_feature_parent_ids(&scan.feature_affected_ids, feature_id)
            .into_iter()
            .chain(agreed_feature_recipe_parent(
                &scan.feature_operation_states,
                feature_id,
            ))
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
            feature.parent =
                agreed_feature_recipe_parent(&scan.feature_operation_states, feature_id)
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

fn feature_edge_selection(scan: &ContainerScan, feature_id: u32) -> Option<EdgeSelection> {
    if let Some(ids) = agreed_feature_affected_ids(
        &scan.feature_affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Edges,
    ) {
        if ids.is_empty() {
            return None;
        }
        return Some(EdgeSelection::Native(format!(
            "creo:allfeatur:edgs_affected#{feature_id}:{}",
            ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
        )));
    }
    if has_feature_affected_ids(
        &scan.feature_affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Edges,
    ) {
        return None;
    }

    let ids = agreed_feature_replay_edge_ids(&scan.feature_replay_affected_ids, feature_id)?;
    if ids.is_empty() {
        return None;
    }
    Some(EdgeSelection::Native(format!(
        "creo:allfeatur:replay_edgs_affected#{feature_id}:{}",
        ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
    )))
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

fn round_constant_radius(scan: &ContainerScan, ir: &CadIr, feature_id: u32) -> Option<f64> {
    let cylinder_rows = scan
        .surface_rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
        .collect::<Vec<_>>();
    if cylinder_rows.is_empty() {
        return None;
    }
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
    if cylinder_radii.len() == cylinder_rows.len() {
        return unique_positive_length(&cylinder_radii);
    }
    let named_ids = agreed_feature_affected_ids(
        &scan.feature_affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Geometry,
    );
    let named_present = has_feature_affected_ids(
        &scan.feature_affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Geometry,
    );
    let replay_ids =
        agreed_feature_replay_geometry_ids(&scan.feature_replay_affected_ids, feature_id);
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
    if schema_class == 926 {
        let transforms = section_definition_for_history_feature(scan, feature_id)
            .into_iter()
            .flat_map(|definition| {
                scan.feature_section_transforms
                    .iter()
                    .filter(move |transform| transform.definition_id == definition.id)
            })
            .collect::<Vec<_>>();
        let sketch = if let [transform] = transforms.as_slice() {
            let sketch = SketchId(format!("creo:model:sketch#{}", transform.definition_id));
            ir.model
                .sketches
                .iter()
                .any(|candidate| candidate.id == sketch)
                .then_some(sketch)
        } else {
            None
        };
        return IrFeatureDefinition::Sketch {
            space: SketchSpace::Planar,
            sketch,
        };
    }
    if schema_class == 911 {
        let placement = hole_placement(feature_outline_planes(scan, feature_id));
        let solved = simple_hole_geometry(scan, feature_id);
        let (face, position, direction, diameter, extent) = solved.map_or_else(
            || {
                placement.map_or(
                    (None, None, None, None, None),
                    |(entry_surface_id, direction, extent)| {
                        (
                            Some(FaceSelection::Native(format!(
                                "creo:visibgeom:surface#{entry_surface_id}"
                            ))),
                            None,
                            Some(Vector3::new(direction[0], direction[1], direction[2])),
                            None,
                            Some(extent),
                        )
                    },
                )
            },
            |hole| {
                let SurfaceGeometry::Cylinder { origin, radius, .. } = hole.geometry else {
                    unreachable!("simple hole helper returns a cylinder")
                };
                (
                    Some(FaceSelection::Native(format!(
                        "creo:visibgeom:surface#{}",
                        hole.entry_surface_id
                    ))),
                    Some(origin),
                    Some(Vector3::new(
                        hole.direction[0],
                        hole.direction[1],
                        hole.direction[2],
                    )),
                    Some(Length(2.0 * radius)),
                    Some(hole.extent),
                )
            },
        );
        return IrFeatureDefinition::Hole {
            face,
            position,
            direction,
            kind: if diameter.is_some() {
                HoleKind::Simple
            } else {
                HoleKind::Unresolved {
                    form: None,
                    counterbore_diameter: None,
                    counterbore_depth: None,
                    countersink_diameter: None,
                    countersink_angle: None,
                }
            },
            diameter,
            extent,
        };
    }
    if schema_class == 913 {
        return IrFeatureDefinition::Fillet {
            edges: feature_edge_selection(scan, feature_id).unwrap_or(EdgeSelection::Unresolved),
            radius: round_constant_radius(scan, ir, feature_id).map_or(
                RadiusSpec::Unresolved { form: None },
                |radius| RadiusSpec::Constant {
                    radius: Length(radius),
                },
            ),
        };
    }
    if schema_class == 914 {
        return IrFeatureDefinition::Chamfer {
            edges: feature_edge_selection(scan, feature_id).unwrap_or(EdgeSelection::Unresolved),
            spec: ChamferSpec::Unresolved { form: None },
        };
    }
    if schema_class == 917
        && feature_recipe(scan, feature_id) == Some(crate::feature::FeatureRecipeKind::Extrude)
    {
        let definitions = scan
            .feature_definitions
            .iter()
            .filter(|definition| definition.owner_feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        if let ([definition], Some(sweep)) = (
            definitions.as_slice(),
            circular_sweep_geometry(scan, feature_id),
        ) {
            return circular_sweep_feature_definition(
                definition.id,
                &sweep,
                section_sweep_boolean_operation(
                    feature_recipe_effect(scan, feature_id),
                    kind,
                    false,
                    preceding_features_establish_body(ir),
                ),
            );
        }
    }
    if feature_recipe(scan, feature_id) == Some(crate::feature::FeatureRecipeKind::Revolve) {
        let extent = feature_revolution_extent(scan, feature_id);
        let transforms = scan
            .feature_section_transforms
            .iter()
            .filter(|transform| transform.feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        let (profile, axis) = if let [transform] = transforms.as_slice() {
            let sketch_id = SketchId(format!("creo:model:sketch#{}", transform.definition_id));
            let profile = ir
                .model
                .sketches
                .iter()
                .find(|sketch| sketch.id == sketch_id)
                .map(|sketch| {
                    if sketch.profiles.is_empty() {
                        ProfileRef::Native(format!(
                            "creo:featdefs:sketch#{}",
                            transform.definition_id
                        ))
                    } else {
                        ProfileRef::Sketch(sketch_id)
                    }
                });
            let axis =
                unique_feature_definition(&scan.feature_definitions, transform.definition_id)
                    .and_then(|definition| resolved_revolution_axis(definition, transform));
            (profile, axis)
        } else {
            (None, None)
        };
        if profile.is_some() || axis.is_some() || extent.is_some() {
            return IrFeatureDefinition::Revolve {
                construction: RevolutionConstruction {
                    profile,
                    axis,
                    extent,
                },
                op: section_sweep_boolean_operation(
                    feature_recipe_effect(scan, feature_id),
                    kind,
                    has_evaluated_sweep_body(ir, "revolution", feature_id),
                    preceding_features_establish_body(ir),
                ),
            };
        }
    }
    if feature_recipe(scan, feature_id) == Some(crate::feature::FeatureRecipeKind::Extrude) {
        let transforms = scan
            .feature_section_transforms
            .iter()
            .filter(|transform| transform.feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        if let [transform] = transforms.as_slice() {
            let sketch_id = SketchId(format!("creo:model:sketch#{}", transform.definition_id));
            let profile = ir
                .model
                .sketches
                .iter()
                .find(|sketch| sketch.id == sketch_id)
                .map(|sketch| {
                    if sketch.profiles.is_empty() {
                        ProfileRef::Native(format!(
                            "creo:featdefs:sketch#{}",
                            transform.definition_id
                        ))
                    } else {
                        ProfileRef::Sketch(sketch_id.clone())
                    }
                });
            if let Some(profile) = profile {
                let cap_origins = feature_plane_equations(scan, feature_id);
                if let Some((extent, direction)) =
                    extrusion_extent_and_direction(transform.origin, transform.normal, cap_origins)
                {
                    return IrFeatureDefinition::Extrude {
                        profile,
                        direction: Some(Vector3::new(direction[0], direction[1], direction[2])),
                        extent,
                        op: section_sweep_boolean_operation(
                            feature_recipe_effect(scan, feature_id),
                            kind,
                            has_evaluated_sweep_body(ir, "extrusion", feature_id),
                            preceding_features_establish_body(ir),
                        ),
                        draft: None,
                    };
                }
            }
        }
    }
    if schema_class == 923 {
        let planes = scan
            .surface_rows
            .iter()
            .filter(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
            })
            .filter_map(|row| {
                let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
                ir.model.surfaces.iter().find(|surface| surface.id == id)
            })
            .collect::<Vec<_>>();
        if let [plane] = planes.as_slice() {
            if let SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } = &plane.geometry
            {
                return IrFeatureDefinition::DatumPlane {
                    origin: *origin,
                    normal: *normal,
                    u_axis: *u_axis,
                };
            }
        }
    }
    IrFeatureDefinition::Native {
        kind: kind.to_string(),
        parameters: feature_parameters(scan, feature_id),
        properties: BTreeMap::new(),
    }
}

fn preceding_features_establish_body(ir: &CadIr) -> bool {
    ir.model.features.iter().any(|feature| {
        matches!(
            feature.source_tag.as_deref(),
            Some("protextrude" | "protrevolve")
        ) || matches!(
            feature.definition,
            IrFeatureDefinition::Extrude { .. }
                | IrFeatureDefinition::Revolve { .. }
                | IrFeatureDefinition::Hole { .. }
                | IrFeatureDefinition::Fillet { .. }
                | IrFeatureDefinition::Chamfer { .. }
        )
    })
}

fn section_sweep_boolean_operation(
    recipe_effect: Option<crate::feature::FeatureRecipeEffect>,
    kind: &str,
    creates_body: bool,
    prior_body: bool,
) -> BooleanOp {
    if creates_body {
        return BooleanOp::NewBody;
    }
    match recipe_effect {
        Some(crate::feature::FeatureRecipeEffect::Protrude) if prior_body => BooleanOp::Join,
        Some(crate::feature::FeatureRecipeEffect::Protrude) => BooleanOp::NewBody,
        Some(crate::feature::FeatureRecipeEffect::Cut) => BooleanOp::Cut,
        None if kind == "Protrusion" && prior_body => BooleanOp::Join,
        None if kind == "Cut" => BooleanOp::Cut,
        _ => BooleanOp::Unresolved,
    }
}

fn named_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    kind: &str,
) -> Option<IrFeatureDefinition> {
    if let Some(role) = match kind {
        "Annotation Feature" => Some(FeatureTreeNodeRole::Annotations),
        "Cross Section" | "Querschnitt" => Some(FeatureTreeNodeRole::CrossSections),
        _ => None,
    } {
        return Some(IrFeatureDefinition::TreeNode { role });
    }
    if kind == "Mirror" {
        return Some(IrFeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Mirror),
            },
        });
    }
    let schema_class = match kind {
        "Datum Plane" | "Bezugsebene" => 923,
        "Hole" => 911,
        "Round" | "Rundung" => 913,
        "Chamfer" => 914,
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
) -> Option<([f64; 3], Extent)> {
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
        Extent::Blind {
            length: Length(signed_length.abs()),
        },
    ))
}

fn hole_placement(
    planes: impl IntoIterator<Item = (u32, [f64; 3], [f64; 3])>,
) -> Option<(u32, [f64; 3], Extent)> {
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

#[derive(Debug, Clone, PartialEq)]
struct SimpleHoleGeometry {
    entry_surface_id: u32,
    cylinder_ids: [u32; 2],
    direction: [f64; 3],
    extent: Extent,
    geometry: SurfaceGeometry,
}

fn simple_hole_geometry(scan: &ContainerScan, feature_id: u32) -> Option<SimpleHoleGeometry> {
    let cap_rows = feature_outline_planes(scan, feature_id)
        .into_iter()
        .map(|(id, origin, normal)| {
            let envelopes = scan
                .plane_envelopes
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
        .feature_entity_tables
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
        !crate::surface::unique_surface_row(&scan.surface_rows, *id).is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    }) {
        return None;
    }
    let (_, direction, extent) =
        hole_placement([*first, *second].map(|(id, origin, normal, _)| (id, origin, normal)))?;
    Some(SimpleHoleGeometry {
        entry_surface_id: *entry_plane,
        cylinder_ids,
        direction,
        extent,
        geometry: hole_cylinder_from_cap_outlines([*first, *second])?,
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
    cylinder_ids: [u32; 2],
    direction: [f64; 3],
    extent: Extent,
    geometry: SurfaceGeometry,
}

fn single_cap_circular_sweep_geometry(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<(u32, SurfaceGeometry)> {
    let tables = scan
        .feature_entity_tables
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
    crate::surface::unique_surface_row(&scan.surface_rows, cap_id.entity_id)
        .is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .then_some(())?;
    crate::surface::unique_surface_row(&scan.surface_rows, cylinder_id.entity_id)
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
        .plane_envelopes
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
    Some((
        cylinder_id.entity_id,
        cylinder_from_single_cap_outline(cap)?,
    ))
}

fn circular_sweep_feature_definition(
    definition_id: u32,
    sweep: &CircularSweepGeometry,
    op: BooleanOp,
) -> IrFeatureDefinition {
    IrFeatureDefinition::Extrude {
        profile: ProfileRef::Native(format!("creo:featdefs:sketch#{definition_id}")),
        direction: Some(Vector3::new(
            sweep.direction[0],
            sweep.direction[1],
            sweep.direction[2],
        )),
        extent: sweep.extent.clone(),
        op,
        draft: None,
    }
}

fn circular_sweep_geometry(scan: &ContainerScan, feature_id: u32) -> Option<CircularSweepGeometry> {
    let tables = scan
        .feature_entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && !table.surface_ids.is_empty())
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
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
            .plane_envelopes
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
        !crate::surface::unique_surface_row(&scan.surface_rows, *id).is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    }) {
        return None;
    }
    let (_, direction, extent) = hole_placement([*first, *second])?;
    Some(CircularSweepGeometry {
        cylinder_ids,
        direction,
        extent,
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
) -> Option<(Extent, [f64; 3])> {
    let span = extrusion_span(profile_origin, direction, planes)?;
    let direction = normalized(direction)?;
    if span.lower == 0.0 || span.upper == 0.0 {
        let signed_length = if span.upper == 0.0 {
            span.lower
        } else {
            span.upper
        };
        return Some((
            Extent::Blind {
                length: Length(signed_length.abs()),
            },
            direction.map(|value| value * signed_length.signum()),
        ));
    }
    let first = span.upper;
    let second = -span.lower;
    let scale = first.max(second).max(1.0);
    let extent = if (first - second).abs() <= 1e-9 * scale {
        Extent::Symmetric {
            length: Length(first + second),
        }
    } else {
        Extent::TwoSided {
            first: Length(first),
            second: Length(second),
        }
    };
    Some((extent, direction))
}

#[cfg(test)]
mod resolved_sketch_tests {
    use super::*;

    #[test]
    fn normalization_rejects_overflowed_finite_vectors() {
        assert_eq!(normalized([f64::MAX, f64::MAX, 0.0]), None);
        assert_eq!(normalized([3.0, 4.0, 0.0]), Some([0.6, 0.8, 0.0]));
    }

    #[test]
    fn dependency_reconciliation_preserves_typed_history_edges() {
        let owner = IrFeatureId("creo:model:feature#40".to_string());
        let sketch = IrFeatureId("creo:model:sketch_feature#917".to_string());
        let parent = IrFeatureId("creo:model:feature#3".to_string());
        let missing = IrFeatureId("creo:model:feature#999".to_string());
        let emitted = [owner.clone(), sketch.clone(), parent.clone()]
            .into_iter()
            .collect();

        assert_eq!(
            reconciled_dependencies(
                &owner,
                &[sketch.clone(), missing],
                [parent.clone(), sketch.clone(), owner.clone()],
                &emitted,
            ),
            vec![sketch, parent]
        );
    }

    #[test]
    fn closed_fallback_profile_selects_revolution_segments() {
        let segment = |external_id| crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            offset: 0,
        };
        let segments = [segment(9), segment(10), segment(11)];
        let profiles = vec![vec![
            SketchEntityUse {
                entity: SketchEntityId("creo:featdefs:sketch_entity#2:9".to_string()),
                reversed: false,
            },
            SketchEntityUse {
                entity: SketchEntityId("creo:featdefs:sketch_entity#2:11".to_string()),
                reversed: true,
            },
        ]];

        assert_eq!(
            profile_segment_ids(2, &segments, &profiles),
            BTreeSet::from([9, 11])
        );
    }

    fn parameter_slot(value: f64) -> crate::surface::SurfaceParameterScalar {
        crate::surface::SurfaceParameterScalar {
            value: Some(value),
            raw: vec![],
            offset: 0,
            length: 1,
        }
    }

    #[test]
    fn tabulated_cylinder_frame_places_a_unique_cubic_chart() {
        let replay = crate::surface::TabulatedCylinderCurveReplay {
            surface_id: 7,
            curve_id: 9,
            curve_type: 0x13,
            flip: 1,
            tangent_condition: 0,
            degree: 3,
            parameter_body: vec![],
            control_point_ids: [1, 2, 3, 4],
            successor_reference: 5,
            control_point_bodies: std::array::from_fn(|_| vec![]),
            control_points: [
                Some([1.0, 2.0]),
                Some([2.0, 2.5]),
                Some([3.0, 3.5]),
                Some([4.0, 4.0]),
            ],
            terminal_reference: 6,
            offset: 0,
            surface_row_offset: 0,
        };
        let parameters = crate::surface::SurfaceParameterRecord {
            surface_id: 7,
            body: vec![],
            scalar_values: vec![],
            scalar_tokens: vec![],
            opaque_spans: vec![crate::surface::SurfaceParameterOpaqueSpan {
                raw: vec![0x00, 0x0c, 0x9a],
                offset: 3,
                length: 3,
            }],
            scalar_frames: vec![
                crate::surface::SurfaceParameterScalarFrame {
                    offset: 0,
                    slots: [0.0, 0.0, 1.0].into_iter().map(parameter_slot).collect(),
                },
                crate::surface::SurfaceParameterScalarFrame {
                    offset: 6,
                    slots: [13.0, 22.0, 5.0, 10.0, 20.0, 10.0]
                        .into_iter()
                        .map(parameter_slot)
                        .collect(),
                },
            ],
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 0,
            body_offset: 0,
        };

        let (curve, sweep) =
            placed_tabulated_cylinder_directrix(&replay, &parameters).expect("placement");
        assert_eq!(curve.control_points[0], Point3::new(-13.0, -20.0, 5.0));
        assert_eq!(curve.control_points[3], Point3::new(-10.0, -22.0, 5.0));
        assert_eq!(sweep, [0.0, 0.0, 5.0]);
    }

    #[test]
    fn tabulated_cylinder_offset_chart_resolves_signed_unit_axes() {
        assert_eq!(
            signed_unit_chart(
                [33.480_874_469_5, 34.047_445_706_6],
                [3.480_874_469_5, 4.047_445_706_6],
                30.0,
            ),
            Some((1.0, -30.0))
        );
        assert_eq!(
            signed_unit_chart(
                [0.576_336_341_1, 0.746_308_064_9],
                [-0.746_308_064_9, -0.576_336_341_1],
                0.0,
            ),
            Some((-1.0, 0.0))
        );
        assert_eq!(signed_unit_chart([1.0, 2.0], [4.0, 5.0], 30.0), None);
        assert!(is_zero_offset_signed_planar_frame(&[
            0x68, 0x42, 0x84, 0x71, 0x18, 0x86,
        ]));
        assert!(!is_zero_offset_signed_planar_frame(&[
            0x68, 0x42, 0x84, 0x71, 0x19, 0x86,
        ]));
    }

    #[test]
    fn zero_offset_2d_tabulated_frame_retains_the_stored_span() {
        let replay = crate::surface::TabulatedCylinderCurveReplay {
            surface_id: 815,
            curve_id: 1,
            curve_type: 0x13,
            flip: 1,
            tangent_condition: 0,
            degree: 3,
            parameter_body: Vec::new(),
            control_point_ids: [1, 2, 3, 4],
            successor_reference: 0,
            control_point_bodies: std::array::from_fn(|_| Vec::new()),
            control_points: [
                Some([2.603_530_729_189_511_6, -6.634_758_301_120_719]),
                Some([2.486_761_892_214_414, -6.583_162_851_673_087]),
                Some([2.403_937_662_020_322, -6.519_347_555_976_829]),
                Some([2.355_057_866_495_792, -6.440_596_814_034_794]),
            ],
            terminal_reference: 0,
            offset: 0,
            surface_row_offset: 0,
        };
        let body = vec![
            0x18, 0xe4, 0x0f, 0x00, 0x0c, 0x9a, 0x8d, 0xd7, 0x28, 0x94, 0x26, 0x4b, 0xb2, 0x2d,
            0x19, 0xc3, 0x2b, 0xcf, 0xac, 0x01, 0x44, 0x9e, 0x1e, 0xb8, 0x51, 0xeb, 0x85, 0x1f,
            0x8f, 0xd4, 0x07, 0xeb, 0x3f, 0xff, 0xf8, 0x2d, 0x1a, 0x89, 0xfe, 0x14, 0x80, 0xb6,
            0x48, 0x9e, 0x85, 0x1e, 0xb8, 0x51, 0xeb, 0x85,
        ];
        let parameters = crate::surface::SurfaceParameterRecord {
            surface_id: 815,
            body,
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: vec![crate::surface::SurfaceParameterOpaqueSpan {
                raw: vec![0, 0x0c, 0x9a],
                offset: 3,
                length: 3,
            }],
            scalar_frames: vec![crate::surface::SurfaceParameterScalarFrame {
                offset: 0,
                slots: vec![
                    parameter_slot(0.0),
                    parameter_slot(1.0),
                    parameter_slot(0.0),
                ],
            }],
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 0,
            body_offset: 0,
        };
        let (curve, sweep) = placed_tabulated_cylinder_directrix(&replay, &parameters)
            .expect("zero-offset directrix placement");
        assert_eq!(
            curve.control_points[0],
            Point3::new(2.603_530_729_189_511_6, 6.634_758_301_120_719, 4.78)
        );
        assert_eq!(
            curve.control_points[3],
            Point3::new(2.355_057_866_495_792, 6.440_596_814_034_794, 4.78)
        );
        assert_eq!(sweep, [0.0, 0.0, 0.099_999_999_999_999_64]);
    }

    #[test]
    fn geometry_signal_excludes_opaque_carriers() {
        let mut ir = CadIr::empty(Units::default());
        let surface_id = SurfaceId("surface".to_string());
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });
        ir.model.curves.push(Curve {
            id: CurveId("curve".to_string()),
            geometry: CurveGeometry::Unknown { record: None },
            source_object: None,
        });

        assert!(!has_transferred_geometry(&ir));

        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId("procedural".to_string()),
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Exact {
                parameter_ranges: [[0.0, 1.0], [0.0, 1.0]],
                extension: 0,
            },
            cache_fit_tolerance: None,
        });

        assert!(has_transferred_geometry(&ir));
    }

    #[test]
    fn fc05_row_frame_maps_cyclically_onto_each_model_axis() {
        let center = [11.0, 13.0];
        let reference = [0.6, 0.8];
        assert_eq!(
            fc05_model_frame(0, 17.0, center, reference, -1.0),
            ([17.0, 13.0, 11.0], [-1.0, 0.0, 0.0], [0.0, 0.8, 0.6])
        );
        assert_eq!(
            fc05_model_frame(1, 17.0, center, reference, -1.0),
            ([11.0, 17.0, 13.0], [0.0, -1.0, 0.0], [0.6, 0.0, 0.8])
        );
        assert_eq!(
            fc05_model_frame(2, 17.0, center, reference, -1.0),
            ([13.0, 11.0, 17.0], [0.0, 0.0, -1.0], [0.8, 0.6, 0.0])
        );
    }

    #[test]
    fn full_turn_section_carriers_classify_analytic_revolution_surfaces() {
        let transform = crate::placement::FeatureSectionTransform {
            definition_id: 1,
            feature_id: Some(2),
            origin: [0.0, 0.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 1.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            offset: 0,
        };
        let axis = RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 1.0, 0.0),
        };
        let line = |start: [f64; 2], end: [f64; 2]| SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
            end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
        };

        assert!(matches!(
            revolved_section_circle(&transform, [2.0, 3.0], axis),
            Some(CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            }) if center == Point3::new(0.0, 3.0, 0.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 2.0
        ));
        assert!(revolved_section_circle(&transform, [0.0, 3.0], axis).is_none());
        assert!(matches!(
            extruded_section_line(&transform, [2.0, 3.0]),
            Some(CurveGeometry::Line { origin, direction })
                if origin == Point3::new(2.0, 3.0, 0.0)
                    && direction == Vector3::new(0.0, 0.0, 1.0)
        ));

        assert!(matches!(
            revolved_section_surface(&transform, &line([2.0, 0.0], [2.0, 4.0]), axis),
            Some(SurfaceGeometry::Cylinder { radius, .. }) if radius == 2.0
        ));
        assert!(matches!(
            revolved_section_surface(&transform, &line([0.0, 3.0], [4.0, 3.0]), axis),
            Some(SurfaceGeometry::Plane { origin, .. }) if origin.y == 3.0
        ));
        assert!(matches!(
            revolved_section_surface(&transform, &line([2.0, 0.0], [4.0, 2.0]), axis),
            Some(SurfaceGeometry::Cone { radius, half_angle, .. })
                if radius == 2.0 && (half_angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12
        ));
        assert!(matches!(
            revolved_section_surface(&transform, &line([4.0, 0.0], [2.0, 2.0]), axis),
            Some(SurfaceGeometry::Cone { axis, radius, half_angle, .. })
                if axis.y == -1.0
                    && radius == 4.0
                    && (half_angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12
        ));
        let centered_arc = SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 3.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        };
        assert!(matches!(
            revolved_section_surface(&transform, &centered_arc, axis),
            Some(SurfaceGeometry::Sphere { radius, .. }) if radius == 2.0
        ));
        let offset_arc = SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(5.0, 3.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        };
        assert!(matches!(
            revolved_section_surface(&transform, &offset_arc, axis),
            Some(SurfaceGeometry::Torus { major_radius, minor_radius, .. })
                if major_radius == 5.0 && minor_radius == 2.0
        ));
        let offset_circle = SketchGeometry::Circle {
            center: Point2::new(5.0, 3.0),
            radius: Length(2.0),
        };
        assert!(matches!(
            revolved_section_surface(&transform, &offset_circle, axis),
            Some(SurfaceGeometry::Torus { major_radius, minor_radius, .. })
                if major_radius == 5.0 && minor_radius == 2.0
        ));
    }

    #[test]
    fn spindle_torus_boundary_pcurve_retains_the_signed_ring_branch() {
        let surface = SurfaceGeometry::Torus {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: 2.0,
            minor_radius: 5.0,
        };
        let axis = RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        };
        let pcurve =
            revolution_boundary_pcurve(&surface, [-3.0, 0.0, 0.0], axis).expect("spindle boundary");
        for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let uv = cadmpeg_ir::eval::pcurve_uv(&pcurve, parameter).expect("pcurve point");
            let point =
                cadmpeg_ir::eval::surface_point(&surface, uv.u, uv.v).expect("surface point");
            assert!((point.x.hypot(point.y) - 3.0).abs() < 1e-12);
            assert!(point.z.abs() < 1e-12);
        }
    }

    #[test]
    fn generated_source_ids_bind_carriers_independently_of_table_position() {
        let table = crate::feature::FeatureEntityTable {
            feature_id: Some(17),
            table_class_id: 80,
            entry_ids: vec![42, 41, 43],
            entries: vec![
                crate::feature::FeatureEntityTableEntry {
                    entity_id: 42,
                    class_id: 200,
                    source_entity_id: Some(10),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::FeatureEntityTableEntry {
                    entity_id: 41,
                    class_id: 200,
                    source_entity_id: Some(8),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::FeatureEntityTableEntry {
                    entity_id: 43,
                    class_id: 200,
                    source_entity_id: Some(9),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
            ],
            surface_ids: vec![41, 42, 43],
            non_surface_entity_ids: Vec::new(),
            offset: 0,
        };
        let order = crate::feature::FeatureOrderTable {
            declared_count: 2,
            entity_ref: Some(3),
            rows: vec![
                crate::feature::FeatureOrderRow {
                    external_id: 8,
                    internal_id: 1,
                    bitmask: 0,
                    offset: 0,
                },
                crate::feature::FeatureOrderRow {
                    external_id: 9,
                    internal_id: 2,
                    bitmask: 0,
                    offset: 0,
                },
            ],
            offset: 0,
        };
        let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
            id,
            type_byte: kind.canonical_type_byte(),
            kind,
            feature_id: 17,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 0,
        };
        let rows = vec![
            row(41, crate::surface::SurfaceKind::Cylinder),
            row(42, crate::surface::SurfaceKind::Cone),
            row(43, crate::surface::SurfaceKind::TorusOrSphere),
        ];
        let cylinder = SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        assert_eq!(
            ordered_analytic_surface_id_for_feature(
                &rows,
                std::slice::from_ref(&table),
                17,
                &order,
                8,
                &cylinder,
            ),
            Some(41)
        );
        assert_eq!(
            ordered_analytic_surface_id_for_feature(
                &rows,
                std::slice::from_ref(&table),
                17,
                &order,
                9,
                &cylinder,
            ),
            None
        );
        let mut first_table = table.clone();
        first_table.entry_ids = vec![41];
        first_table.entries = vec![table.entries[1].clone()];
        first_table.surface_ids = vec![41];
        let mut second_table = table.clone();
        second_table.entry_ids = vec![43];
        second_table.entries = vec![table.entries[2].clone()];
        second_table.surface_ids = vec![43];
        assert_eq!(
            generated_surface_id_for_feature(&[first_table.clone(), second_table], 17, 9),
            Some(43)
        );
        first_table.entries[0].source_entity_id = Some(9);
        assert_eq!(
            generated_surface_id_for_feature(&[first_table, table.clone()], 17, 9),
            None
        );
        let torus = SurfaceGeometry::Torus {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 1.0, 0.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: 4.0,
            minor_radius: 1.0,
        };
        assert_eq!(
            ordered_analytic_surface_id_for_feature(
                &rows,
                std::slice::from_ref(&table),
                17,
                &order,
                9,
                &torus,
            ),
            Some(43)
        );
        assert_eq!(
            ordered_family_surface_bindings_for_feature(
                &rows,
                17,
                std::slice::from_ref(&table),
                &order,
                [9],
                crate::surface::SurfaceKind::TorusOrSphere,
            ),
            BTreeMap::from([(9, 43)])
        );
        assert!(saved_entity_is_generated_profile(
            Some(17),
            8,
            crate::surface::SurfaceKind::Cylinder,
            std::slice::from_ref(&table),
            &rows,
        ));
        assert!(!saved_entity_is_generated_profile(
            Some(17),
            10,
            crate::surface::SurfaceKind::Cylinder,
            &[table],
            &rows,
        ));
    }

    #[test]
    fn rowless_round_cylinder_requires_the_four_entry_sibling_layout() {
        let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
            id,
            type_byte: kind.canonical_type_byte(),
            kind,
            feature_id: 23,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 0,
        };
        let mut rows = vec![
            row(10, crate::surface::SurfaceKind::Plane),
            row(11, crate::surface::SurfaceKind::Plane),
            row(13, crate::surface::SurfaceKind::Cylinder),
        ];
        let table = crate::feature::FeatureEntityTable {
            feature_id: Some(23),
            table_class_id: 80,
            entry_ids: vec![10, 11, 12, 13],
            entries: Vec::new(),
            surface_ids: vec![10, 11, 13],
            non_surface_entity_ids: vec![12],
            offset: 47,
        };
        assert_eq!(
            rowless_round_cylinder_pairs(
                &BTreeSet::from([23]),
                std::slice::from_ref(&table),
                &rows,
            ),
            vec![(12, 13, 47)]
        );
        assert!(rowless_round_cylinder_pairs(
            &BTreeSet::new(),
            std::slice::from_ref(&table),
            &rows,
        )
        .is_empty());
        rows[2].reversed = true;
        assert_eq!(
            rowless_round_face_orientations(
                &BTreeSet::from([23]),
                std::slice::from_ref(&table),
                &rows,
                &BTreeSet::from([12]),
            ),
            BTreeMap::from([(12, true)])
        );
        assert!(rowless_round_face_orientations(
            &BTreeSet::from([23]),
            std::slice::from_ref(&table),
            &rows,
            &BTreeSet::new(),
        )
        .is_empty());
        let mut materialized_rowless = rows;
        materialized_rowless.push(row(12, crate::surface::SurfaceKind::Cylinder));
        assert!(rowless_round_cylinder_pairs(
            &BTreeSet::from([23]),
            &[table],
            &materialized_rowless,
        )
        .is_empty());
    }

    #[test]
    fn spline_extrusion_preserves_directrix_basis_and_weights() {
        let directrix = NurbsCurve {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point3::new(1.0, 2.0, 3.0),
                Point3::new(4.0, 5.0, 6.0),
                Point3::new(7.0, 8.0, 9.0),
            ],
            weights: Some(vec![1.0, 0.5, 1.0]),
            periodic: false,
        };
        let surface =
            extruded_nurbs_surface(&directrix, [0.0, 0.0, 4.0]).expect("valid extrusion surface");

        assert_eq!((surface.u_degree, surface.v_degree), (2, 1));
        assert_eq!((surface.u_count, surface.v_count), (3, 2));
        assert_eq!(surface.u_knots, directrix.knots);
        assert_eq!(surface.v_knots, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            surface.control_points,
            [
                Point3::new(1.0, 2.0, 3.0),
                Point3::new(1.0, 2.0, 7.0),
                Point3::new(4.0, 5.0, 6.0),
                Point3::new(4.0, 5.0, 10.0),
                Point3::new(7.0, 8.0, 9.0),
                Point3::new(7.0, 8.0, 13.0),
            ]
        );
        assert_eq!(surface.weights, Some(vec![1.0, 1.0, 0.5, 0.5, 1.0, 1.0]));
    }

    #[test]
    fn reversed_arc_uses_opposite_axis_and_canonical_increasing_domain() {
        let (axis_sign, range) = oriented_arc_parameterization(
            true,
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
        );

        assert_eq!(axis_sign, -1.0);
        assert_eq!(
            range,
            [
                3.0 * std::f64::consts::FRAC_PI_2,
                5.0 * std::f64::consts::FRAC_PI_2
            ]
        );
    }

    #[test]
    fn extrusion_arc_pcurve_is_exact_in_both_directions() {
        for (start, end, expected_middle) in [
            (0.0, std::f64::consts::PI, Point2::new(2.0, 5.0)),
            (std::f64::consts::PI, 0.0, Point2::new(2.0, 5.0)),
        ] {
            let pcurve = circular_pcurve([2.0, 2.0], 3.0, start, end);
            let first = cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.0).expect("first endpoint");
            let middle = cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.5).expect("arc midpoint");
            let last = cadmpeg_ir::eval::pcurve_uv(&pcurve, 1.0).expect("last endpoint");
            assert!((first.u - (2.0 + 3.0 * start.cos())).abs() < 1e-12);
            assert!((first.v - (2.0 + 3.0 * start.sin())).abs() < 1e-12);
            assert!((middle.u - expected_middle.u).abs() < 1e-12);
            assert!((middle.v - expected_middle.v).abs() < 1e-12);
            assert!((last.u - (2.0 + 3.0 * end.cos())).abs() < 1e-12);
            assert!((last.v - (2.0 + 3.0 * end.sin())).abs() < 1e-12);
        }
    }

    #[test]
    fn extrusion_profile_area_includes_oriented_arc_sector() {
        let arc = SketchGeometry::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length(1.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        };
        let line = SketchGeometry::Line {
            start: Point2::new(-1.0, 0.0),
            end: Point2::new(1.0, 0.0),
        };
        let counterclockwise = vec![
            (arc.clone(), false, [1.0, 0.0], [-1.0, 0.0]),
            (line.clone(), false, [-1.0, 0.0], [1.0, 0.0]),
        ];
        let clockwise = vec![
            (arc, true, [-1.0, 0.0], [1.0, 0.0]),
            (line, true, [1.0, 0.0], [-1.0, 0.0]),
        ];
        assert!(
            (extrusion_profile_signed_area(&counterclockwise).expect("positive area")
                - std::f64::consts::FRAC_PI_2)
                .abs()
                < 1e-12
        );
        assert!(
            (extrusion_profile_signed_area(&clockwise).expect("negative area")
                + std::f64::consts::FRAC_PI_2)
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn extrusion_profiles_require_one_oppositely_oriented_hole() {
        let rectangle = |minimum: [f64; 2], maximum: [f64; 2], clockwise: bool| {
            let mut points = [
                minimum,
                [maximum[0], minimum[1]],
                maximum,
                [minimum[0], maximum[1]],
            ];
            if clockwise {
                points.reverse();
            }
            (0..4)
                .map(|index| {
                    let start = points[index];
                    let end = points[(index + 1) % 4];
                    (
                        SketchGeometry::Line {
                            start: Point2::new(start[0], start[1]),
                            end: Point2::new(end[0], end[1]),
                        },
                        false,
                        start,
                        end,
                    )
                })
                .collect::<ExtrusionProfile>()
        };
        let outer = rectangle([-2.0, -2.0], [2.0, 2.0], false);
        let hole = rectangle([-1.0, -1.0], [1.0, 1.0], true);
        let (profiles, outer_area) = ordered_extrusion_profiles(vec![hole.clone(), outer.clone()])
            .expect("strict outer and hole");
        assert_eq!(profiles[0], outer);
        assert!(outer_area > 0.0);
        assert!(extrusion_profile_signed_area(&profiles[1]).expect("hole area") < 0.0);

        assert!(ordered_extrusion_profiles(vec![
            rectangle([-2.0, -2.0], [2.0, 2.0], false),
            rectangle([-1.0, -1.0], [1.0, 1.0], false),
        ])
        .is_none());
        assert!(ordered_extrusion_profiles(vec![
            rectangle([-2.0, -2.0], [2.0, 2.0], false),
            rectangle([1.0, -1.0], [3.0, 1.0], true),
        ])
        .is_none());

        let circular_hole = [
            (std::f64::consts::PI, 0.0, [-0.5, 0.0], [0.5, 0.0]),
            (
                std::f64::consts::TAU,
                std::f64::consts::PI,
                [0.5, 0.0],
                [-0.5, 0.0],
            ),
        ]
        .into_iter()
        .map(|(end_angle, start_angle, start, end)| {
            (
                SketchGeometry::Arc {
                    center: Point2::new(0.0, 0.0),
                    radius: Length(0.5),
                    start_angle: Angle(start_angle),
                    end_angle: Angle(end_angle),
                },
                true,
                start,
                end,
            )
        })
        .collect::<ExtrusionProfile>();
        let (profiles, _) = ordered_extrusion_profiles(vec![
            circular_hole,
            rectangle([-2.0, -2.0], [2.0, 2.0], false),
        ])
        .expect("arc-bounded hole");
        assert!(matches!(profiles[1][0].0, SketchGeometry::Arc { .. }));
    }

    #[test]
    fn extrusion_profile_intersections_include_analytic_tangency() {
        let full_upper_circle = ([0.0, 0.0], 1.0, 0.0, std::f64::consts::PI);
        assert!(line_arc_intersect(
            [[-2.0, 1.0], [2.0, 1.0]],
            full_upper_circle,
            1e-9,
        ));
        assert!(!line_arc_intersect(
            [[-2.0, 1.1], [2.0, 1.1]],
            full_upper_circle,
            1e-9,
        ));
        assert!(arcs_intersect(
            full_upper_circle,
            ([2.0, 0.0], 1.0, std::f64::consts::PI, std::f64::consts::PI),
            1e-9,
        ));
        assert!(!arcs_intersect(
            full_upper_circle,
            ([3.0, 0.0], 1.0, std::f64::consts::PI, std::f64::consts::PI),
            1e-9,
        ));
    }

    #[test]
    fn equal_opposite_cap_planes_define_symmetric_extent() {
        let extent = extrusion_extent_and_direction(
            [0.0, 0.0, 0.0],
            [0.0, -1.0, 0.0],
            [
                ([0.0, 4.0, 0.0], [0.0, 1.0, 0.0]),
                ([0.0, -4.0, 0.0], [0.0, 1.0, 0.0]),
                ([3.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ],
        );

        assert_eq!(
            extent,
            Some((
                Extent::Symmetric {
                    length: Length(8.0)
                },
                [0.0, -1.0, 0.0]
            ))
        );
    }

    #[test]
    fn stored_section_sweep_family_defines_boolean_operation() {
        use crate::feature::FeatureRecipeEffect::{Cut, Protrude};

        assert_eq!(
            section_sweep_boolean_operation(Some(Protrude), "Körper", false, true),
            BooleanOp::Join
        );
        assert_eq!(
            section_sweep_boolean_operation(Some(Cut), "Ausschnitt", false, false),
            BooleanOp::Cut
        );
        assert_eq!(
            section_sweep_boolean_operation(Some(Protrude), "Protrusion", true, false),
            BooleanOp::NewBody
        );
        assert_eq!(
            section_sweep_boolean_operation(Some(Protrude), "Körper", false, false),
            BooleanOp::NewBody
        );
        assert_eq!(
            section_sweep_boolean_operation(None, "Körper", false, true),
            BooleanOp::Unresolved
        );
    }

    #[test]
    fn circular_sweep_projects_profile_direction_and_extent() {
        let sweep = CircularSweepGeometry {
            cylinder_ids: [12, 13],
            direction: [0.0, 0.0, -1.0],
            extent: Extent::Blind {
                length: Length(6.5),
            },
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(2.0, 3.0, 4.0),
                axis: Vector3::new(0.0, 0.0, -1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 1.5,
            },
        };

        assert_eq!(
            circular_sweep_feature_definition(917, &sweep, BooleanOp::Join),
            IrFeatureDefinition::Extrude {
                profile: ProfileRef::Native("creo:featdefs:sketch#917".to_string()),
                direction: Some(Vector3::new(0.0, 0.0, -1.0)),
                extent: Extent::Blind {
                    length: Length(6.5),
                },
                op: BooleanOp::Join,
                draft: None,
            }
        );
    }

    #[test]
    fn ordered_hole_cap_planes_define_blind_direction_and_depth() {
        assert_eq!(
            hole_extent_and_direction([
                ([2.0, -21.0, -0.75], [1.0, 0.0, 0.0]),
                ([5.0, -22.5, 0.75], [-1.0, 0.0, 0.0]),
            ]),
            Some((
                [1.0, 0.0, 0.0],
                Extent::Blind {
                    length: Length(3.0),
                },
            ))
        );
        assert_eq!(
            hole_extent_and_direction([
                ([0.0, 0.5, 0.0], [0.0, 1.0, 0.0]),
                ([0.0, -0.5, 0.0], [0.0, 1.0, 0.0]),
            ]),
            Some((
                [-0.0, -1.0, -0.0],
                Extent::Blind {
                    length: Length(1.0),
                },
            ))
        );
        assert_eq!(
            hole_extent_and_direction([
                ([0.0; 3], [1.0, 0.0, 0.0]),
                ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ]),
            None
        );

        assert_eq!(
            hole_placement([
                (902, [0.0, 0.0, 0.85], [0.0, 0.0, 1.0]),
                (905, [0.0, 0.0, 7.35], [0.0, 0.0, -1.0]),
            ]),
            Some((
                902,
                [0.0, 0.0, 1.0],
                Extent::Blind {
                    length: Length(6.5),
                },
            ))
        );
        assert_eq!(
            hole_placement([
                (902, [0.0; 3], [0.0, 0.0, 1.0]),
                (905, [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]),
                (908, [0.0, 0.0, 2.0], [0.0, 0.0, -1.0]),
            ]),
            None
        );
        assert!(matches!(
            hole_cylinder_from_cap_outlines([
                (
                    902,
                    [0.0, 0.0, 0.85],
                    [0.0, 0.0, 1.0],
                    [[-1.5, 17.5, 0.85], [1.5, 20.5, 0.85]],
                ),
                (
                    905,
                    [0.0, 0.0, 7.35],
                    [0.0, 0.0, -1.0],
                    [[-1.5, 17.5, 7.35], [1.5, 20.5, 7.35]],
                ),
            ]),
            Some(SurfaceGeometry::Cylinder { origin, axis, radius, .. })
                if origin == Point3::new(0.0, 19.0, 0.85)
                    && axis == Vector3::new(0.0, 0.0, 1.0)
                    && radius == 1.5
        ));
        assert!(hole_cylinder_from_cap_outlines([
            (
                902,
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [[-1.0, -2.0, 0.0], [1.0, 2.0, 0.0]],
            ),
            (
                905,
                [0.0, 0.0, 1.0],
                [0.0, 0.0, -1.0],
                [[-1.0, -2.0, 1.0], [1.0, 2.0, 1.0]],
            ),
        ])
        .is_none());
        assert!(circular_sweep_cylinder_from_cap_outlines([
            (
                828,
                [0.0, 4.0, 0.0],
                [0.0, 1.0, 0.0],
                Some([[-13.25, 4.0, -0.75], [-11.75, 4.0, 0.75]]),
            ),
            (831, [0.0, -4.0, 0.0], [0.0, 1.0, 0.0], None,),
        ])
        .is_none());
        assert!(matches!(
            cylinder_from_single_cap_outline((
                46,
                [0.0, 16.0, 0.0],
                [0.0, 1.0, 0.0],
                Some([[-4.45, 16.0, -4.45], [4.45, 16.0, 4.45]]),
            )),
            Some(SurfaceGeometry::Cylinder { origin, axis, radius, .. })
                if origin == Point3::new(0.0, 16.0, 0.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && radius == 4.45
        ));
    }

    #[test]
    fn unique_parallel_round_supports_define_constant_radius() {
        assert_eq!(unique_positive_length(&[0.5, 0.5 + 1e-12]), Some(0.5));
        assert_eq!(unique_positive_length(&[0.5, 0.6]), None);
        assert_eq!(unique_positive_length(&[0.0]), None);
        assert_eq!(
            parallel_support_radius([
                ([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ([0.0, 0.0, -6.1], [0.0, 0.0, 1.0]),
                ([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ]),
            Some(0.5)
        );
        assert_eq!(
            parallel_support_radius([
                ([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ([0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
                ([0.0, 0.0, -8.0], [0.0, 0.0, 1.0]),
            ]),
            None
        );
        assert_eq!(
            parallel_support_radius([
                ([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ([0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
                ([0.0, 0.0, -7.0], [0.0, 0.0, 1.0]),
            ]),
            Some(0.5)
        );
        let cylinder = slot_fillet_cylinder(
            [
                PlaneEquation {
                    origin: [0.0, -2.0, 0.0],
                    normal: [0.0, 1.0, 0.0],
                },
                PlaneEquation {
                    origin: [0.0, 3.0, 0.0],
                    normal: [0.0, 1.0, 0.0],
                },
            ],
            &[
                PlaneEquation {
                    origin: [-9.0, 0.0, 0.0],
                    normal: [1.0, 0.0, 0.0],
                },
                PlaneEquation {
                    origin: [-8.0, 0.0, 0.0],
                    normal: [1.0, 0.0, 0.0],
                },
                PlaneEquation {
                    origin: [0.0, 0.0, -7.0],
                    normal: [0.0, 0.0, 1.0],
                },
                PlaneEquation {
                    origin: [0.0, 0.0, -6.0],
                    normal: [0.0, 0.0, 1.0],
                },
            ],
        )
        .expect("fully constrained slot fillet");
        assert_eq!(cylinder.origin, [-8.5, -2.0, -6.5]);
        assert_eq!(cylinder.axis, [0.0, 1.0, 0.0]);
        assert_eq!(cylinder.radius, 0.5);
        assert!(slot_fillet_cylinder(
            [
                PlaneEquation {
                    origin: [0.0, -2.0, 0.0],
                    normal: [0.0, 1.0, 0.0],
                },
                PlaneEquation {
                    origin: [0.0, 3.0, 0.0],
                    normal: [0.0, 1.0, 0.0],
                },
            ],
            &[
                PlaneEquation {
                    origin: [-9.0, 0.0, 0.0],
                    normal: [1.0, 0.0, 0.0],
                },
                PlaneEquation {
                    origin: [-8.0, 0.0, 0.0],
                    normal: [1.0, 0.0, 0.0],
                },
            ],
        )
        .is_none());
    }

    #[test]
    fn asymmetric_cap_planes_define_two_sided_extent() {
        assert_eq!(
            extrusion_extent_and_direction(
                [0.0; 3],
                [0.0, 0.0, 1.0],
                [
                    ([0.0, 0.0, -2.0], [0.0, 0.0, 1.0]),
                    ([0.0, 0.0, 3.0], [0.0, 0.0, 1.0]),
                ],
            ),
            Some((
                Extent::TwoSided {
                    first: Length(3.0),
                    second: Length(2.0),
                },
                [0.0, 0.0, 1.0],
            ))
        );
    }

    #[test]
    fn one_negative_cap_offset_reverses_blind_direction() {
        assert_eq!(
            extrusion_extent_and_direction(
                [0.0; 3],
                [0.0, -1.0, 0.0],
                [([0.0, 48.0, 0.0], [0.0, 1.0, 0.0])],
            ),
            Some((
                Extent::Blind {
                    length: Length(48.0),
                },
                [-0.0, 1.0, -0.0],
            ))
        );
    }

    #[test]
    fn zero_offset_support_plane_does_not_obscure_blind_cap() {
        assert_eq!(
            extrusion_extent_and_direction(
                [0.0; 3],
                [0.0, 1.0, 0.0],
                [
                    ([20.0, 0.0, 6.0], [0.0, 1.0, 0.0]),
                    ([0.0, 48.0, 0.0], [0.0, 1.0, 0.0]),
                ],
            ),
            Some((
                Extent::Blind {
                    length: Length(48.0),
                },
                [0.0, 1.0, 0.0],
            ))
        );
    }

    #[test]
    fn interior_axis_normal_planes_do_not_shorten_blind_extent() {
        assert_eq!(
            extrusion_extent_and_direction(
                [0.0; 3],
                [0.0, -1.0, 0.0],
                [
                    ([0.0, 38.0, 0.0], [0.0, 1.0, 0.0]),
                    ([3.0, 2.5, 7.0], [0.0, -1.0, 0.0]),
                    ([-4.0, 5.75, 1.0], [0.0, 1.0, 0.0]),
                ],
            ),
            Some((
                Extent::Blind {
                    length: Length(38.0),
                },
                [-0.0, 1.0, -0.0],
            ))
        );
    }

    #[test]
    fn section_line_requires_two_solved_points() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [7, 9],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 12,
            offset: 40,
        };
        let mut points = BTreeMap::from([(7, [2.0, 3.0])]);
        assert!(section_line_geometry(&points, &segment).is_none());
        points.insert(9, [5.0, 8.0]);
        assert_eq!(
            section_line_geometry(&points, &segment),
            Some(SketchGeometry::Line {
                start: cadmpeg_ir::math::Point2::new(2.0, 3.0),
                end: cadmpeg_ir::math::Point2::new(5.0, 8.0),
            })
        );
    }

    #[test]
    fn sketch_constraints_require_every_neutral_reference_to_be_emitted() {
        let first = SketchEntityId("first".to_string());
        let second = SketchEntityId("second".to_string());
        let emitted = BTreeSet::from([first.clone()]);

        let mut horizontal = SketchConstraintDefinition::Horizontal {
            entity: first.clone(),
        };
        assert!(reconcile_constraint_entity_references(
            &mut horizontal,
            &emitted
        ));
        let mut parallel = SketchConstraintDefinition::Parallel {
            first: first.clone(),
            second: second.clone(),
        };
        assert!(!reconcile_constraint_entity_references(
            &mut parallel,
            &emitted
        ));
        let mut distance = SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Start(first.clone()),
            second: SketchLocus::Center(second.clone()),
            parameter: ParameterId("distance".to_string()),
        };
        assert!(!reconcile_constraint_entity_references(
            &mut distance,
            &emitted
        ));
        let mut native = SketchConstraintDefinition::Native {
            native_kind: "creo:test".to_string(),
            entities: vec![first.clone(), second],
            parameter: None,
            operands: Vec::new(),
        };
        assert!(reconcile_constraint_entity_references(
            &mut native,
            &emitted
        ));
        assert!(matches!(
            native,
            SketchConstraintDefinition::Native { entities, .. }
                if entities == vec![first]
        ));

        let parameter = ParameterId("distance".to_string());
        let parameters = BTreeSet::from([parameter.clone()]);
        let mut radius = SketchConstraintDefinition::Radius {
            entity: SketchEntityId("first".to_string()),
            parameter: parameter.clone(),
        };
        assert!(reconcile_constraint_parameter_reference(
            &mut radius,
            &parameters
        ));
        let mut missing_distance = SketchConstraintDefinition::Distance {
            entities: Vec::new(),
            parameter: ParameterId("missing".to_string()),
        };
        assert!(!reconcile_constraint_parameter_reference(
            &mut missing_distance,
            &parameters
        ));
        let mut native_parameter = SketchConstraintDefinition::Native {
            native_kind: "creo:test".to_string(),
            entities: Vec::new(),
            parameter: Some(ParameterId("missing".to_string())),
            operands: Vec::new(),
        };
        assert!(reconcile_constraint_parameter_reference(
            &mut native_parameter,
            &parameters
        ));
        assert!(matches!(
            native_parameter,
            SketchConstraintDefinition::Native {
                parameter: None,
                ..
            }
        ));
    }

    #[test]
    fn section_point_uses_its_single_solved_position() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Point,
            directions: [None; 3],
            point_ids: [7, 7],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 4,
            offset: 40,
        };
        let points = BTreeMap::from([(7, [2.0, 3.0])]);

        assert_eq!(
            section_point_geometry(&points, &segment),
            Some(SketchGeometry::Point {
                position: cadmpeg_ir::math::Point2::new(2.0, 3.0),
            })
        );
    }

    #[test]
    fn section_axis_line_carrier_uses_equal_decoded_ordinates() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [Some(0), None, Some(0)],
            point_ids: [7, 9],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 12,
            offset: 40,
        };
        let definition = crate::feature::FeatureDefinition {
            id: 5,
            owner_feature_id: Some(6),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                points: vec![
                    crate::feature::FeatureSectionPoint {
                        point_id: 7,
                        u: Some(2.0),
                        v: None,
                    },
                    crate::feature::FeatureSectionPoint {
                        point_id: 9,
                        u: Some(2.0),
                        v: Some(8.0),
                    },
                ],
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };
        assert_eq!(
            section_axis_line_carrier(&definition, &segment),
            Some(SketchGeometry::Line {
                start: cadmpeg_ir::math::Point2::new(2.0, -8.0),
                end: cadmpeg_ir::math::Point2::new(2.0, 8.0),
            })
        );
        assert_eq!(
            unique_feature_definition(std::slice::from_ref(&definition), definition.id)
                .map(|matched| matched.offset),
            Some(0)
        );
        assert!(unique_feature_definition(&[definition.clone(), definition], 5).is_none());
        let operation = crate::feature::FeatureOperation {
            feature_id: 6,
            kind: "Extrude".to_string(),
            display_name_stored: true,
            stored_name: Some("Extrude id 6".to_string()),
            stored_name_bytes: Some(b"Extrude id 6".to_vec()),
            identifier_keyword: Some("id".to_string()),
            stored_name_prefix: None,
            recipe: Some(crate::feature::FeatureRecipe::ProtrudeExtrude),
            root_schema_class: Some(917),
            parent_feature_id: None,
            offset: 10,
            state_offset: 10,
        };
        assert_eq!(
            current_feature_operation(std::slice::from_ref(&operation), 6)
                .and_then(|current| current.root_schema_class),
            Some(917)
        );
        assert!(current_feature_operation(&[operation.clone(), operation.clone()], 6).is_none());
        assert_eq!(
            agreed_feature_recipe(&[operation.clone(), operation.clone()], 6),
            Some(crate::feature::FeatureRecipe::ProtrudeExtrude)
        );
        let mut conflicting_recipe = operation.clone();
        conflicting_recipe.recipe = Some(crate::feature::FeatureRecipe::ProtrudeRevolve);
        assert_eq!(
            agreed_feature_recipe(&[operation.clone(), conflicting_recipe], 6),
            None
        );
        let mut parented_operation = operation.clone();
        parented_operation.parent_feature_id = Some(5);
        assert_eq!(
            agreed_feature_recipe_parent(
                &[parented_operation.clone(), parented_operation.clone()],
                6,
            ),
            Some(5)
        );
        let mut conflicting_parent = parented_operation.clone();
        conflicting_parent.parent_feature_id = Some(4);
        assert_eq!(
            agreed_feature_recipe_parent(&[parented_operation, conflicting_parent], 6),
            None
        );
        let row = |schema_class, offset| crate::feature::FeatureRow {
            feature_id: 6,
            header: [0xeb, 0x04],
            root_schema_class: Some(schema_class),
            stream_offset: 0,
            body: Vec::new(),
            body_offset: offset + 1,
            offset,
        };
        assert_eq!(
            resolved_feature_schema_class_from_classes(
                &[],
                row_feature_schema_classes(&[row(917, 20), row(917, 30)], 6),
                6,
            ),
            Some(917)
        );
        assert_eq!(
            resolved_feature_schema_class_from_classes(
                &[],
                row_feature_schema_classes(&[row(913, 20), row(914, 30)], 6),
                6,
            ),
            None
        );
        assert_eq!(
            resolved_feature_schema_class_from_classes(
                std::slice::from_ref(&operation),
                row_feature_schema_classes(&[row(913, 20), row(914, 30)], 6),
                6,
            ),
            None
        );
        assert_eq!(
            row_feature_schema_classes(&[row(913, 20), row(914, 30)], 6),
            BTreeSet::from([913, 914])
        );
        let extent = |feature_id, offset| crate::feature::FeatureRevolutionExtent {
            feature_id,
            kind: crate::feature::FeatureRevolutionExtentKind::FullTurn,
            offset,
        };
        assert_eq!(
            unique_feature_revolution_extent_kind(&[extent(6, 40), extent(6, 50)], 6),
            Some(crate::feature::FeatureRevolutionExtentKind::FullTurn)
        );
        assert_eq!(
            unique_feature_revolution_extent_kind(&[extent(7, 40)], 6),
            None
        );
        let transform = crate::placement::FeatureSectionTransform {
            definition_id: 5,
            feature_id: Some(6),
            origin: [0.0; 3],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 1.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            offset: 40,
        };
        assert_eq!(
            unique_feature_section_transform(std::slice::from_ref(&transform), 5)
                .map(|placed| placed.offset),
            Some(40)
        );
        assert!(
            unique_feature_section_transform(&[transform.clone(), transform.clone()], 5).is_none()
        );
        let competing_definition = crate::placement::FeatureSectionTransform {
            definition_id: 7,
            offset: 50,
            ..transform.clone()
        };
        assert!(unique_feature_section_transform(&[transform, competing_definition], 5).is_none());
        let affected = |ids: &[u32], offset| crate::feature::FeatureAffectedIds {
            feature_id: 6,
            kind: crate::feature::AffectedIdKind::Edges,
            ids: ids.to_vec(),
            offset,
        };
        assert_eq!(
            agreed_feature_affected_ids(
                &[affected(&[7, 8], 60), affected(&[7, 8], 70)],
                6,
                crate::feature::AffectedIdKind::Edges,
            ),
            Some(&[7, 8][..])
        );
        assert_eq!(
            agreed_feature_affected_ids(
                &[affected(&[7, 8], 60), affected(&[8, 7], 70)],
                6,
                crate::feature::AffectedIdKind::Edges,
            ),
            None
        );
        let replay = |geometry_ids: &[u32], edge_ids: &[u32], offset| {
            crate::feature::FeatureReplayAffectedIds {
                feature_id: 6,
                geometry_ids: geometry_ids.to_vec(),
                edge_ids: edge_ids.to_vec(),
                geometry_extent: crate::feature::ReplayExtentSource::Explicit,
                edge_extent: crate::feature::ReplayExtentSource::Inherited,
                offset,
            }
        };
        assert_eq!(
            agreed_feature_replay_geometry_ids(
                &[replay(&[1, 2], &[7], 80), replay(&[1, 2], &[7], 90)],
                6,
            ),
            Some(&[1, 2][..])
        );
        assert_eq!(
            agreed_feature_replay_edge_ids(&[replay(&[1], &[7], 80), replay(&[1], &[], 90)], 6,),
            None
        );
    }

    #[test]
    fn intersects_evaluated_section_carriers() {
        let horizontal = SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-2.0, 1.0),
            end: cadmpeg_ir::math::Point2::new(2.0, 1.0),
        };
        let vertical = SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(0.5, -3.0),
            end: cadmpeg_ir::math::Point2::new(0.5, 3.0),
        };
        assert_eq!(
            intersect_section_lines(&horizontal, &vertical),
            Some([0.5, 1.0])
        );

        let circle_half = SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        };
        let endpoint_line = SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(2.0, 0.0),
            end: cadmpeg_ir::math::Point2::new(3.0, 1.0),
        };
        let intersection = intersect_section_line_arc(&endpoint_line, &circle_half)
            .expect("line has one endpoint on the arc");
        assert!((intersection[0] - 2.0).abs() <= 1e-12);
        assert!(intersection[1].abs() <= 1e-12);
        let one_crossing = SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            end: cadmpeg_ir::math::Point2::new(3.0, 0.0),
        };
        assert_eq!(
            intersect_section_line_arc(&one_crossing, &circle_half),
            Some([2.0, 0.0])
        );
        let two_crossings = SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-3.0, 0.0),
            end: cadmpeg_ir::math::Point2::new(3.0, 0.0),
        };
        assert_eq!(
            intersect_section_line_arc(&two_crossings, &circle_half),
            None
        );
        let no_crossing = SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(3.0, 0.0),
            end: cadmpeg_ir::math::Point2::new(4.0, 0.0),
        };
        assert_eq!(intersect_section_line_arc(&no_crossing, &circle_half), None);

        let circle = |center, radius| SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(center, 0.0),
            radius: Length(radius),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::TAU),
        };
        assert_eq!(
            intersect_tangent_section_arcs(&circle(0.0, 2.0), &circle(3.0, 1.0)),
            Some([2.0, 0.0])
        );
        assert_eq!(
            intersect_tangent_section_arcs(&circle(0.0, 3.0), &circle(2.0, 1.0)),
            Some([3.0, 0.0])
        );
        assert_eq!(
            intersect_tangent_section_arcs(&circle(0.0, 2.0), &circle(2.0, 2.0)),
            None
        );
    }

    #[test]
    fn saved_line_joins_through_order_table() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [7, 9],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            offset: 40,
        };
        let definition = crate::feature::FeatureDefinition {
            id: 5,
            owner_feature_id: Some(6),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: Some(crate::feature::FeatureOrderTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::FeatureOrderRow {
                    external_id: 42,
                    internal_id: 3,
                    bitmask: 0,
                    offset: 10,
                }],
                offset: 8,
            }),
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: Some(crate::feature::FeatureSavedSection {
                entities: vec![crate::feature::FeatureSavedEntity::Line(
                    crate::feature::FeatureSavedLine {
                        entity_id: 3,
                        references: Vec::new(),
                        attributes: Vec::new(),
                        endpoints: [
                            [Some(-8.0), Some(-0.85), Some(0.0)],
                            [Some(8.0), Some(-0.85), None],
                        ],
                        offset: 20,
                    },
                )],
                offset: 18,
            }),
            offset: 0,
        };

        assert_eq!(
            saved_section_line_geometry(&definition, &segment),
            Some(SketchGeometry::Line {
                start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
                end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
            })
        );
        assert_eq!(
            section_entity_external_ids(&definition),
            BTreeSet::from([42])
        );
        let mut incomplete = definition.clone();
        let crate::feature::FeatureSavedEntity::Line(incomplete_line) = &mut incomplete
            .saved_section
            .as_mut()
            .expect("saved section")
            .entities[0]
        else {
            panic!("saved line");
        };
        incomplete_line.endpoints[1][1] = None;
        assert!(saved_section_entity_geometry(
            &incomplete
                .saved_section
                .as_ref()
                .expect("saved section")
                .entities[0]
        )
        .is_none());
        assert_eq!(
            section_entity_external_ids(&incomplete),
            BTreeSet::from([42])
        );
        let (native_entity, offset) = unresolved_saved_section_entity(
            &incomplete,
            &SketchId("sketch".into()),
            &incomplete
                .saved_section
                .as_ref()
                .expect("saved section")
                .entities[0],
            &unique_saved_section_internal_ids(&incomplete),
            &BTreeSet::new(),
        );
        assert_eq!(offset, 20);
        assert_eq!(native_entity.id.0, "creo:featdefs:sketch_entity#5:42");
        assert!(matches!(
            native_entity.geometry,
            SketchGeometry::Native { ref native_kind } if native_kind == "saved_line"
        ));
        let mut duplicate_order_row = definition.clone();
        duplicate_order_row
            .order_table
            .as_mut()
            .expect("order table")
            .rows
            .push(crate::feature::FeatureOrderRow {
                external_id: 42,
                internal_id: 4,
                bitmask: 0,
                offset: 11,
            });
        assert_eq!(
            saved_section_line_geometry(&duplicate_order_row, &segment),
            None
        );
        let mut duplicate_saved_line = definition.clone();
        let duplicate = duplicate_saved_line
            .saved_section
            .as_ref()
            .expect("saved section")
            .entities[0]
            .clone();
        duplicate_saved_line
            .saved_section
            .as_mut()
            .expect("saved section")
            .entities
            .push(duplicate);
        assert_eq!(
            saved_section_line_geometry(&duplicate_saved_line, &segment),
            None
        );
        assert_eq!(
            saved_section_external_id(
                definition.order_table.as_ref().expect("order table"),
                &unique_saved_section_internal_ids(&definition),
                &ambiguous_section_segment_external_ids(
                    definition
                        .segments
                        .iter()
                        .flat_map(|segments| &segments.rows)
                ),
                3,
            ),
            Some(42)
        );
        let mut constrained = definition.clone();
        constrained.segments = Some(crate::feature::FeatureSegmentTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 0,
        });
        constrained.relations = Some(crate::feature::FeatureRelationTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            skamps: vec![crate::feature::FeatureSkamp {
                id: 5,
                kind: 99,
                flags: 0,
                status: 0,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 42,
                    sense: 4,
                }],
                offset: 30,
            }],
            skamp_header: None,
            triples: vec![crate::feature::FeatureRelationTriple {
                relation_id: Some(7),
                equation_id: None,
                skamp_id: Some(5),
                offset: 31,
            }],
            triples_header: None,
            offset: 28,
        });
        let constraints =
            section_skamp_constraints(&constrained, &SketchId("creo:model:sketch#5".to_string()));
        assert!(matches!(
            &constraints[0].0.definition,
            SketchConstraintDefinition::Native { entities, .. }
                if entities == &[SketchEntityId(
                    "creo:featdefs:sketch_entity#5:42".to_string()
                )]
        ));
        assert_eq!(
            relation_incidence_entities(&constrained, 7),
            vec![SketchEntityId(
                "creo:featdefs:sketch_entity#5:42".to_string()
            )]
        );
        constrained.segments = None;
        let constraints =
            section_skamp_constraints(&constrained, &SketchId("creo:model:sketch#5".to_string()));
        assert!(matches!(
            &constraints[0].0.definition,
            SketchConstraintDefinition::Native { entities, .. }
                if entities == &[SketchEntityId(
                    "creo:featdefs:sketch_entity#5:42".to_string()
                )]
        ));

        let mut completed = definition;
        completed
            .order_table
            .as_mut()
            .expect("test definition has an order table")
            .rows
            .clear();
        completed.segments = Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![segment.clone()],
            offset: 4,
        });
        completed.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            rows: vec![crate::feature::FeatureTrimEntity {
                external_id: 42,
                mode: Some(0),
                vertices: [1, 2],
                center_vertex: None,
                kind: crate::feature::TrimEntityKind::Line,
                offset: 6,
            }],
            solved_external_ids: vec![42],
            offset: 5,
        });
        assert_eq!(
            saved_section_line_geometry(&completed, &segment),
            Some(SketchGeometry::Line {
                start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
                end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
            })
        );
        let trim = completed.trim_entities.as_ref().expect("trim table").rows[0].clone();
        assert_eq!(trim_segment_id(&completed, &trim), Some(42));
        let mut duplicate_segment = completed.clone();
        duplicate_segment
            .segments
            .as_mut()
            .expect("segment table")
            .rows
            .push(segment);
        assert_eq!(trim_segment_id(&duplicate_segment, &trim), None);
        let mut duplicate_trim = completed;
        duplicate_trim
            .trim_entities
            .as_mut()
            .expect("trim table")
            .rows
            .push(trim.clone());
        assert_eq!(trim_segment_id(&duplicate_trim, &trim), None);
    }

    #[test]
    fn complete_saved_circle_defines_full_section_geometry() {
        let entity =
            crate::feature::FeatureSavedEntity::Circle(crate::feature::FeatureSavedCircle {
                entity_id: 7,
                center: [Some(2.0), Some(-3.0), Some(0.0)],
                radius: Some(4.5),
                offset: 19,
            });

        assert_eq!(
            saved_section_entity_geometry(&entity),
            Some((
                7,
                SketchGeometry::Circle {
                    center: Point2::new(2.0, -3.0),
                    radius: Length(4.5),
                },
                19,
            ))
        );
        let (_, geometry, _) =
            saved_section_entity_geometry(&entity).expect("complete saved circle");
        assert!(is_full_circle_geometry(&geometry));
    }

    #[test]
    fn generated_saved_geometry_forms_closed_profiles() {
        let line = |external_id: u32, start: (f64, f64), end: (f64, f64)| {
            (
                external_id,
                SketchGeometry::Line {
                    start: Point2::new(start.0, start.1),
                    end: Point2::new(end.0, end.1),
                },
            )
        };
        let geometries = vec![
            line(12, (0.0, 1.0), (1.0, 1.0)),
            (
                10,
                SketchGeometry::Nurbs {
                    degree: 1,
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                    weights: None,
                    periodic: false,
                },
            ),
            line(13, (0.0, 0.0), (0.0, 1.0)),
            line(11, (1.0, 1.0), (1.0, 0.0)),
            line(20, (5.0, 5.0), (6.0, 5.0)),
            (
                30,
                SketchGeometry::Arc {
                    center: Point2::new(8.0, 8.0),
                    radius: Length(2.0),
                    start_angle: Angle(0.0),
                    end_angle: Angle(std::f64::consts::TAU),
                },
            ),
        ];

        let profiles = saved_profile_chains(917, &geometries);

        assert_eq!(profiles.len(), 2);
        assert_eq!(
            profiles[0][0].entity.0,
            "creo:featdefs:sketch_entity#917:30"
        );
        assert_eq!(profiles[1].len(), 4);
        assert_eq!(
            profiles[1][0].entity.0,
            "creo:featdefs:sketch_entity#917:10"
        );
        assert!(!profiles[1][0].reversed);
        assert!(profiles[1][1..].iter().all(|entity| entity.reversed));
        assert!(profiles
            .iter()
            .flatten()
            .all(|entity| !entity.entity.0.ends_with(":20")));
    }

    #[test]
    fn saved_arc_joins_through_order_table() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [7, 9],
            center_id: Some(8),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            offset: 40,
        };
        let definition = crate::feature::FeatureDefinition {
            id: 5,
            owner_feature_id: Some(6),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: Some(crate::feature::FeatureOrderTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::FeatureOrderRow {
                    external_id: 42,
                    internal_id: 3,
                    bitmask: 0,
                    offset: 10,
                }],
                offset: 8,
            }),
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: Some(crate::feature::FeatureSavedSection {
                entities: vec![crate::feature::FeatureSavedEntity::Arc(
                    crate::feature::FeatureSavedArc {
                        entity_id: 3,
                        center: [Some(0.0), Some(0.0), Some(0.0)],
                        radius: Some(2.0),
                        endpoints: [
                            [Some(0.0), Some(-2.0), Some(0.0)],
                            [Some(-2.0), Some(0.0), Some(0.0)],
                        ],
                        parameters: [None; 2],
                        offset: 20,
                    },
                )],
                offset: 18,
            }),
            offset: 0,
        };

        assert_eq!(
            saved_section_arc_geometry(&definition, &segment),
            Some(SketchGeometry::Arc {
                center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                radius: Length(2.0),
                start_angle: Angle(std::f64::consts::PI),
                end_angle: Angle(3.0 * std::f64::consts::FRAC_PI_2),
            })
        );
        let mut duplicate_order_row = definition.clone();
        duplicate_order_row
            .order_table
            .as_mut()
            .expect("order table")
            .rows
            .push(crate::feature::FeatureOrderRow {
                external_id: 42,
                internal_id: 4,
                bitmask: 0,
                offset: 11,
            });
        assert_eq!(
            saved_section_arc_geometry(&duplicate_order_row, &segment),
            None
        );
        let mut duplicate_saved_arc = definition.clone();
        let duplicate = duplicate_saved_arc
            .saved_section
            .as_ref()
            .expect("saved section")
            .entities[0]
            .clone();
        duplicate_saved_arc
            .saved_section
            .as_mut()
            .expect("saved section")
            .entities
            .push(duplicate);
        assert_eq!(
            saved_section_arc_geometry(&duplicate_saved_arc, &segment),
            None
        );

        let mut trimmed = definition;
        trimmed.segments = Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![segment],
            offset: 38,
        });
        trimmed.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            rows: vec![crate::feature::FeatureTrimEntity {
                external_id: 42,
                mode: Some(0),
                vertices: [1, 2],
                center_vertex: None,
                kind: crate::feature::TrimEntityKind::Arc,
                offset: 30,
            }],
            solved_external_ids: vec![42],
            offset: 28,
        });
        assert_eq!(
            resolved_trim_vertex_coordinates(&trimmed, &BTreeMap::new()),
            BTreeMap::from([(1, [0.0, -2.0]), (2, [-2.0, 0.0])])
        );
        let mut conflicting_vertex = trimmed.clone();
        conflicting_vertex.trim_vertices = Some(crate::feature::FeatureTrimVertexTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            rows: vec![
                crate::feature::FeatureTrimVertex {
                    vertex_id: 1,
                    entities: [42, 43],
                    section_coordinates: Some([0.0, -2.0]),
                    offset: 31,
                },
                crate::feature::FeatureTrimVertex {
                    vertex_id: 1,
                    entities: [42, 44],
                    section_coordinates: Some([9.0, 9.0]),
                    offset: 32,
                },
            ],
            offset: 30,
        });
        assert_eq!(
            resolved_trim_vertex_coordinates(&conflicting_vertex, &BTreeMap::new()),
            BTreeMap::from([(2, [-2.0, 0.0])])
        );
        if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
            .saved_section
            .as_mut()
            .expect("test definition has a saved section")
            .entities[0]
        {
            arc.center[1] = None;
            arc.radius = None;
        }
        let segment = &trimmed
            .segments
            .as_ref()
            .expect("test definition has a segment table")
            .rows[0];
        assert_eq!(
            saved_section_arc_carrier(&trimmed, segment),
            Some(([0.0, 0.0], 2.0))
        );
        if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
            .saved_section
            .as_mut()
            .expect("test definition has a saved section")
            .entities[0]
        {
            arc.center[1] = Some(0.0);
            arc.radius = Some(2.0);
        }
        if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
            .saved_section
            .as_mut()
            .expect("test definition has a saved section")
            .entities[0]
        {
            arc.endpoints[0] = [None; 3];
        } else {
            panic!("test entity is an arc");
        }
        assert_eq!(
            resolved_trim_vertex_coordinates(&trimmed, &BTreeMap::new()),
            BTreeMap::from([(2, [-2.0, 0.0])])
        );
        if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
            .saved_section
            .as_mut()
            .expect("test definition has a saved section")
            .entities[0]
        {
            arc.endpoints[1] = [None; 3];
        }
        let segment = &trimmed
            .segments
            .as_ref()
            .expect("test definition has a segment table")
            .rows[0];
        assert!(saved_section_arc_geometry(&trimmed, segment).is_none());
        assert_eq!(
            section_segment_intersection_carrier(
                &trimmed,
                &resolved_section_radii(&trimmed),
                &BTreeMap::new(),
                segment,
            )
            .map(|carrier| carrier.geometry),
            Some(SketchGeometry::Arc {
                center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                radius: Length(2.0),
                start_angle: Angle(0.0),
                end_angle: Angle(std::f64::consts::TAU),
            })
        );
    }

    #[test]
    fn saved_arc_carrier_combines_with_trim_vertices() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [7, 9],
            center_id: Some(8),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            offset: 40,
        };
        let definition = crate::feature::FeatureDefinition {
            id: 5,
            owner_feature_id: Some(6),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: Some(crate::feature::FeatureTrimEntityTable {
                declared_count: None,
                entity_ref: None,
                entry_ref: None,
                rows: vec![crate::feature::FeatureTrimEntity {
                    external_id: 42,
                    mode: Some(0),
                    vertices: [1, 2],
                    center_vertex: None,
                    kind: crate::feature::TrimEntityKind::Arc,
                    offset: 30,
                }],
                solved_external_ids: vec![42],
                offset: 28,
            }),
            trim_vertices: None,
            order_table: Some(crate::feature::FeatureOrderTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::FeatureOrderRow {
                    external_id: 42,
                    internal_id: 3,
                    bitmask: 0,
                    offset: 10,
                }],
                offset: 8,
            }),
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: Some(crate::feature::FeatureSavedSection {
                entities: vec![crate::feature::FeatureSavedEntity::Arc(
                    crate::feature::FeatureSavedArc {
                        entity_id: 3,
                        center: [Some(0.0), Some(0.0), Some(0.0)],
                        radius: Some(2.0),
                        endpoints: [[None; 3]; 2],
                        parameters: [None; 2],
                        offset: 20,
                    },
                )],
                offset: 18,
            }),
            offset: 0,
        };
        let trim_vertices = BTreeMap::from([(1, [-2.0, 0.0]), (2, [0.0, -2.0])]);

        assert_eq!(
            trimmed_section_segment_geometry(
                &definition,
                &BTreeMap::new(),
                &trim_vertices,
                &segment,
            ),
            Some(SketchGeometry::Arc {
                center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                radius: Length(2.0),
                start_angle: Angle(-std::f64::consts::FRAC_PI_2),
                end_angle: Angle(std::f64::consts::PI),
            })
        );
    }

    #[test]
    fn placed_extrusion_line_defines_plane() {
        let transform = crate::placement::FeatureSectionTransform {
            definition_id: 5,
            feature_id: Some(5),
            origin: [10.0, 20.0, 30.0],
            u_axis: [0.0, 1.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
            normal: [1.0, 0.0, 0.0],
            offset: 7,
        };
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 3,
            offset: 9,
        };
        let points = BTreeMap::from([(1, [2.0, 3.0]), (2, [6.0, 3.0])]);
        assert_eq!(
            extruded_segment_surface(&transform, &points, &segment),
            Some(SurfaceGeometry::Plane {
                origin: Point3::new(10.0, 22.0, 33.0),
                normal: Vector3::new(0.0, 0.0, -1.0),
                u_axis: Vector3::new(0.0, 1.0, 0.0),
            })
        );
        assert_eq!(
            placed_section_curve_geometry(&transform, &points, &segment),
            Some(CurveGeometry::Line {
                origin: Point3::new(10.0, 22.0, 33.0),
                direction: Vector3::new(0.0, 1.0, 0.0),
            })
        );
    }

    #[test]
    fn placed_extrusion_arc_defines_cylinder() {
        let transform = crate::placement::FeatureSectionTransform {
            definition_id: 5,
            feature_id: Some(5),
            origin: [10.0, 20.0, 30.0],
            u_axis: [0.0, 1.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
            normal: [1.0, 0.0, 0.0],
            offset: 7,
        };
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: Some(3),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 4,
            offset: 9,
        };
        let points = BTreeMap::from([(1, [2.0, 0.0]), (2, [-2.0, 0.0]), (3, [0.0, 0.0])]);
        assert_eq!(
            extruded_segment_surface(&transform, &points, &segment),
            Some(SurfaceGeometry::Cylinder {
                origin: Point3::new(10.0, 20.0, 30.0),
                axis: Vector3::new(1.0, 0.0, 0.0),
                ref_direction: Vector3::new(0.0, 1.0, 0.0),
                radius: 2.0,
            })
        );
        assert_eq!(
            placed_section_curve_geometry(&transform, &points, &segment),
            Some(CurveGeometry::Circle {
                center: Point3::new(10.0, 20.0, 30.0),
                axis: Vector3::new(1.0, 0.0, 0.0),
                ref_direction: Vector3::new(0.0, 1.0, 0.0),
                radius: 2.0,
            })
        );
        assert_eq!(
            placed_section_geometry_curve(
                &transform,
                &SketchGeometry::Circle {
                    center: Point2::new(3.0, -4.0),
                    radius: Length(2.0),
                },
            ),
            Some(CurveGeometry::Circle {
                center: Point3::new(10.0, 23.0, 26.0),
                axis: Vector3::new(1.0, 0.0, 0.0),
                ref_direction: Vector3::new(0.0, 1.0, 0.0),
                radius: 2.0,
            })
        );
    }

    #[test]
    fn line_orientation_selectors_are_closed() {
        let mut segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [7, 9],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: Some(0),
            radius_ref: None,
            radius2_ref: None,
            external_id: 12,
            offset: 40,
        };
        let entity = SketchEntityId("entity".into());
        assert_eq!(
            line_orientation_definition(&segment, entity.clone()),
            Some(SketchConstraintDefinition::Vertical {
                entity: entity.clone()
            })
        );
        segment.vertical_horizontal = Some(1);
        assert_eq!(
            line_orientation_definition(&segment, entity.clone()),
            Some(SketchConstraintDefinition::Horizontal {
                entity: entity.clone()
            })
        );
        segment.vertical_horizontal = Some(2);
        assert_eq!(line_orientation_definition(&segment, entity.clone()), None);
        segment.kind = crate::feature::FeatureSegmentKind::Arc;
        segment.vertical_horizontal = Some(0);
        assert_eq!(line_orientation_definition(&segment, entity), None);
    }

    #[test]
    fn dimension_identity_includes_its_feature_definition() {
        assert_ne!(
            feature_dimension_parameter_id(917, 40, 3),
            feature_dimension_parameter_id(1104, 40, 3)
        );
        assert_eq!(
            feature_dimension_parameter_id(917, 40, 3).0,
            "creo:featdefs:parameter#917:40:3"
        );
        assert_eq!(
            feature_dimension_parameter_layout(&[
                (40, 917, 3),
                (40, 1104, 3),
                (40, 1104, 4),
                (41, 1200, 3),
            ]),
            Some(vec![
                (0, "d917_3".to_string(), None),
                (1, "d1104_3".to_string(), None),
                (2, "d4".to_string(), None),
                (0, "d3".to_string(), None),
            ])
        );
        assert_eq!(
            feature_dimension_parameter_layout(&[(40, 917, 3), (40, 917, 3)]),
            Some(vec![
                (0, "d917_3_1".to_string(), Some(0)),
                (1, "d917_3_2".to_string(), Some(1)),
            ])
        );
        assert_ne!(
            feature_dimension_parameter_row_id(917, 40, 3, Some(0)),
            feature_dimension_parameter_row_id(917, 40, 3, Some(1))
        );
        let dimension = crate::feature::FeatureDimension {
            dimension_type: 2,
            value: Some(5.0),
            value_unit: crate::feature::DimensionUnit::Millimeters,
            direction_byte: 0,
            auxiliary_value: None,
            external_id: 3,
            offset: 10,
        };
        let mut table = crate::feature::FeatureDimensionTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![dimension.clone()],
            offset: 9,
        };
        let mut definition = crate::feature::FeatureDefinition {
            id: 917,
            owner_feature_id: Some(40),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: Some(table.clone()),
            relations: None,
            saved_section: None,
            offset: 8,
        };
        assert_eq!(
            resolved_feature_dimension_parameter(
                definition.id,
                definition.owner_feature_id.expect("dimension owner"),
                definition.dimensions.as_ref().expect("dimension table"),
                0,
            ),
            Some((
                &dimension,
                ParameterId("creo:featdefs:parameter#917:40:3".to_string())
            ))
        );
        let unresolved_dimension = crate::feature::FeatureDimension {
            value: None,
            external_id: 4,
            ..dimension.clone()
        };
        let unresolved_table = crate::feature::FeatureDimensionTable {
            rows: vec![unresolved_dimension.clone()],
            ..table.clone()
        };
        assert_eq!(
            resolved_feature_dimension_parameter(917, 40, &unresolved_table, 0),
            Some((
                &unresolved_dimension,
                ParameterId("creo:featdefs:parameter#917:40:4".to_string())
            ))
        );
        table.rows.push(dimension);
        definition.dimensions = Some(table);
        assert_eq!(
            resolved_feature_dimension_parameter(
                definition.id,
                definition.owner_feature_id.expect("dimension owner"),
                definition.dimensions.as_ref().expect("dimension table"),
                0,
            ),
            None
        );
        assert_eq!(
            resolved_feature_dimension_parameter(
                definition.id,
                definition.owner_feature_id.expect("dimension owner"),
                definition.dimensions.as_ref().expect("dimension table"),
                1,
            ),
            None
        );
        assert!(
            unique_feature_definition(&[definition.clone(), definition.clone()], 917).is_none()
        );
    }

    #[test]
    fn evaluated_sweep_bodies_are_feature_outputs() {
        let mut ir = CadIr::empty(Units::default());
        for id in [
            "creo:feature:extrusion#40:body",
            "creo:feature:revolution#40:body",
            "creo:feature:revolution#41:body",
        ] {
            ir.model.bodies.push(Body {
                id: BodyId(id.to_string()),
                kind: BodyKind::Solid,
                regions: Vec::new(),
                transform: None,
                name: None,
                color: None,
                visible: None,
            });
        }
        assert_eq!(
            evaluated_sweep_output_bodies(&ir, 40),
            vec![
                BodyId("creo:feature:extrusion#40:body".to_string()),
                BodyId("creo:feature:revolution#40:body".to_string()),
            ]
        );
        assert!(has_evaluated_sweep_body(&ir, "extrusion", 40));
        assert!(has_evaluated_sweep_body(&ir, "revolution", 40));
        assert!(!has_evaluated_sweep_body(&ir, "revolution", 42));
    }

    #[test]
    fn unary_solver_incidences_define_line_orientation() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 12,
            offset: 40,
        };
        let arc = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [2, 3],
            center_id: Some(4),
            arc_orientation: Some(1),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 13,
            offset: 41,
        };
        let point = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Point,
            directions: [None; 3],
            point_ids: [4, 4],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 14,
            offset: 42,
        };
        let other_line = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [5, 6],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 15,
            offset: 43,
        };
        let other_arc = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [5, 6],
            center_id: Some(7),
            arc_orientation: Some(1),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 16,
            offset: 44,
        };
        let relations = crate::feature::FeatureRelationTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![crate::feature::FeatureRelation {
                relation_id: 8,
                used: 1,
                operands: vec![12, 4],
                operand_vectors: None,
                sign: 1,
                dimension_id: 0,
                relation_type: 99,
                body: Vec::new(),
                offset: 80,
            }],
            skamps: vec![
                crate::feature::FeatureSkamp {
                    id: 3,
                    kind: 1,
                    flags: 0,
                    status: 0,
                    items: vec![crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    }],
                    offset: 50,
                },
                crate::feature::FeatureSkamp {
                    id: 4,
                    kind: 2,
                    flags: 0,
                    status: 0,
                    items: vec![crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    }],
                    offset: 60,
                },
                crate::feature::FeatureSkamp {
                    id: 5,
                    kind: 7,
                    flags: 0,
                    status: 0,
                    items: vec![crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 4,
                    }],
                    offset: 70,
                },
                crate::feature::FeatureSkamp {
                    id: 6,
                    kind: 1,
                    flags: 0,
                    status: 0,
                    items: vec![crate::feature::FeatureSkampItem {
                        entity_id: 13,
                        sense: 0,
                    }],
                    offset: 71,
                },
                crate::feature::FeatureSkamp {
                    id: 7,
                    kind: 0,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 2,
                        },
                    ],
                    offset: 72,
                },
                crate::feature::FeatureSkamp {
                    id: 8,
                    kind: 4,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 3,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 2,
                        },
                    ],
                    offset: 73,
                },
                crate::feature::FeatureSkamp {
                    id: 9,
                    kind: 14,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 2,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 3,
                        },
                    ],
                    offset: 74,
                },
                crate::feature::FeatureSkamp {
                    id: 10,
                    kind: 14,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 4,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 4,
                        },
                    ],
                    offset: 75,
                },
                crate::feature::FeatureSkamp {
                    id: 11,
                    kind: 3,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 14,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 4,
                        },
                    ],
                    offset: 76,
                },
                crate::feature::FeatureSkamp {
                    id: 12,
                    kind: 9,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 14,
                            sense: 0,
                        },
                    ],
                    offset: 77,
                },
                crate::feature::FeatureSkamp {
                    id: 13,
                    kind: 5,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 15,
                            sense: 0,
                        },
                    ],
                    offset: 78,
                },
                crate::feature::FeatureSkamp {
                    id: 14,
                    kind: 7,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 15,
                            sense: 0,
                        },
                    ],
                    offset: 79,
                },
                crate::feature::FeatureSkamp {
                    id: 15,
                    kind: 8,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 15,
                            sense: 0,
                        },
                    ],
                    offset: 80,
                },
                crate::feature::FeatureSkamp {
                    id: 16,
                    kind: 6,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 16,
                            sense: 0,
                        },
                    ],
                    offset: 81,
                },
                crate::feature::FeatureSkamp {
                    id: 17,
                    kind: 17,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 2,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 15,
                            sense: 2,
                        },
                    ],
                    offset: 82,
                },
            ],
            skamp_header: None,
            triples: Vec::new(),
            triples_header: None,
            offset: 45,
        };
        let definition = crate::feature::FeatureDefinition {
            id: 917,
            owner_feature_id: Some(40),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                points: vec![
                    crate::feature::FeatureSectionPoint {
                        point_id: 1,
                        u: Some(0.0),
                        v: Some(2.0),
                    },
                    crate::feature::FeatureSectionPoint {
                        point_id: 5,
                        u: Some(3.0),
                        v: Some(2.0),
                    },
                ],
                offset: 89,
            }),
            segments: Some(crate::feature::FeatureSegmentTable {
                declared_count: 5,
                entity_ref: None,
                rows: vec![segment, arc, point, other_line, other_arc],
                offset: 30,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: Some(crate::feature::FeatureDimensionTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::FeatureDimension {
                    dimension_type: 2,
                    value: Some(3.0),
                    value_unit: crate::feature::DimensionUnit::Millimeters,
                    direction_byte: 0,
                    auxiliary_value: None,
                    external_id: 42,
                    offset: 75,
                }],
                offset: 74,
            }),
            relations: Some(relations),
            saved_section: None,
            offset: 0,
        };
        let mut equal_radius_definition = definition.clone();
        let equal_radius_segments = &mut equal_radius_definition
            .segments
            .as_mut()
            .expect("segments")
            .rows;
        equal_radius_segments[1].radius_ref = Some(101);
        equal_radius_segments[4].radius_ref = Some(102);
        equal_radius_definition.variables = Some(crate::feature::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![
                crate::feature::FeatureSectionPoint {
                    point_id: 2,
                    u: Some(3.0),
                    v: Some(0.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 4,
                    u: Some(0.0),
                    v: Some(0.0),
                },
            ],
            offset: 89,
        });
        assert_eq!(
            resolved_section_radii(&equal_radius_definition),
            BTreeMap::from([(101, 3.0), (102, 3.0)])
        );
        equal_radius_definition
            .variables
            .as_mut()
            .expect("variables")
            .rows
            .push(crate::feature::FeatureVariableRow {
                variable_type: 3,
                key: 102,
                value: Some(4.0),
                guess: None,
                known: None,
                homogeneity: None,
                uvar_id: None,
                dimension_driven: false,
                offset: 91,
            });
        assert!(resolved_section_radii(&equal_radius_definition).is_empty());
        let constraints = section_skamp_constraints(&definition, &SketchId("sketch".into()));

        assert!(matches!(
            constraints[0].0.definition,
            SketchConstraintDefinition::Horizontal { .. }
        ));
        assert!(matches!(
            constraints[1].0.definition,
            SketchConstraintDefinition::Vertical { .. }
        ));
        let mut locus_orientation = definition.clone();
        locus_orientation
            .relations
            .as_mut()
            .expect("relations")
            .skamps[0]
            .items[0]
            .sense = 2;
        assert!(matches!(
            section_skamp_constraints(&locus_orientation, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Native {
                ref native_kind,
                ..
            } if native_kind == "creo:skamp:1"
        ));
        let mut duplicate_entity = definition.clone();
        let mut duplicate_line =
            duplicate_entity.segments.as_ref().expect("segments").rows[0].clone();
        duplicate_line.offset = 500;
        duplicate_entity
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(duplicate_line);
        let unique_ids = unique_section_segment_external_ids(
            duplicate_entity
                .segments
                .iter()
                .flat_map(|segments| &segments.rows),
        );
        assert!(!unique_ids.contains(&12));
        assert_eq!(
            section_segment_identity_suffix(
                &unique_ids,
                duplicate_entity
                    .segments
                    .as_ref()
                    .expect("segments")
                    .rows
                    .last()
                    .expect("duplicate segment")
            ),
            "offset:500"
        );
        assert!(matches!(
            section_skamp_constraints(&duplicate_entity, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Native {
                ref native_kind,
                ..
            } if native_kind == "creo:skamp:1"
        ));
        let mut equivalent_skamp = definition.clone();
        let mut redundant = equivalent_skamp
            .relations
            .as_ref()
            .expect("relations")
            .skamps[0]
            .clone();
        redundant.id = 100;
        redundant.offset = 500;
        equivalent_skamp
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .push(redundant);
        let equivalent_constraints =
            section_skamp_constraints(&equivalent_skamp, &SketchId("sketch".into()));
        assert_eq!(
            equivalent_constraints[0].0.definition,
            equivalent_constraints
                .last()
                .expect("redundant")
                .0
                .definition
        );
        assert_ne!(
            equivalent_constraints[0].0.id,
            equivalent_constraints.last().expect("redundant").0.id
        );
        let mut duplicate_skamp_id = definition.clone();
        let mut duplicate = duplicate_skamp_id
            .relations
            .as_ref()
            .expect("relations")
            .skamps[0]
            .clone();
        duplicate.offset = 500;
        duplicate_skamp_id
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .push(duplicate);
        let duplicate_constraints =
            section_skamp_constraints(&duplicate_skamp_id, &SketchId("sketch".into()));
        assert!(matches!(
            duplicate_constraints[0].0.definition,
            SketchConstraintDefinition::Native { .. }
        ));
        assert!(matches!(
            duplicate_constraints
                .last()
                .expect("duplicate")
                .0
                .definition,
            SketchConstraintDefinition::Native { .. }
        ));
        assert_eq!(
            duplicate_constraints[0].0.id.0,
            "creo:featdefs:sketch_constraint#917:skamp:offset:50"
        );
        assert_eq!(
            duplicate_constraints.last().expect("duplicate").0.id.0,
            "creo:featdefs:sketch_constraint#917:skamp:offset:500"
        );
        assert_eq!(
            constraints[2].0.definition,
            SketchConstraintDefinition::Native {
                native_kind: "creo:skamp:7".to_string(),
                entities: vec![SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )],
                parameter: None,
                operands: vec![SketchNativeOperand {
                    native_kind: "sense:4".to_string(),
                    object_index: 12,
                    native_ref: None,
                }],
            }
        );
        assert!(matches!(
            constraints[3].0.definition,
            SketchConstraintDefinition::Native {
                ref native_kind,
                ..
            } if native_kind == "creo:skamp:1"
        ));
        assert!(matches!(
            constraints[4].0.definition,
            SketchConstraintDefinition::Native {
                ref native_kind,
                ..
            } if native_kind == "creo:skamp:0"
        ));
        assert_eq!(
            constraints[5].0.definition,
            SketchConstraintDefinition::TangentLoci {
                first: SketchLocus::End(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )),
                second: SketchLocus::End(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string()
                )),
            }
        );
        assert_eq!(
            constraints[6].0.definition,
            SketchConstraintDefinition::Symmetric {
                first: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )),
                second: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string()
                )),
                axis: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
            }
        );
        assert_eq!(
            constraints[7].0.definition,
            SketchConstraintDefinition::Symmetric {
                first: SketchLocus::Center(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string()
                )),
                second: SketchLocus::Center(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string()
                )),
                axis: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
            }
        );
        assert_eq!(
            constraints[8].0.definition,
            SketchConstraintDefinition::CoincidentLoci {
                loci: vec![
                    SketchLocus::Entity(SketchEntityId(
                        "creo:featdefs:sketch_entity#917:14".to_string()
                    )),
                    SketchLocus::Center(SketchEntityId(
                        "creo:featdefs:sketch_entity#917:13".to_string()
                    )),
                ],
            }
        );
        assert_eq!(
            constraints[9].0.definition,
            SketchConstraintDefinition::CoincidentLoci {
                loci: vec![
                    SketchLocus::Entity(SketchEntityId(
                        "creo:featdefs:sketch_entity#917:12".to_string()
                    )),
                    SketchLocus::Entity(SketchEntityId(
                        "creo:featdefs:sketch_entity#917:14".to_string()
                    )),
                ],
            }
        );
        let first = SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string());
        let second = SketchEntityId("creo:featdefs:sketch_entity#917:15".to_string());
        assert_eq!(
            constraints[10].0.definition,
            SketchConstraintDefinition::Perpendicular {
                first: first.clone(),
                second: second.clone(),
            }
        );
        assert_eq!(
            constraints[11].0.definition,
            SketchConstraintDefinition::Parallel {
                first: first.clone(),
                second: second.clone(),
            }
        );
        assert_eq!(
            constraints[12].0.definition,
            SketchConstraintDefinition::Equal { first, second }
        );
        assert_eq!(
            constraints[13].0.definition,
            SketchConstraintDefinition::Equal {
                first: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
                second: SketchEntityId("creo:featdefs:sketch_entity#917:16".to_string()),
            }
        );
        assert_eq!(
            constraints[14].0.definition,
            SketchConstraintDefinition::SameCoordinate {
                first: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )),
                second: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:15".to_string()
                )),
                axis: SketchCoordinateAxis::V,
            }
        );
        let mut distance_definition = definition.clone();
        let distance_segment = &mut distance_definition
            .segments
            .as_mut()
            .expect("segments")
            .rows[0];
        distance_segment.vertical_horizontal = Some(0);
        let distance_relation = &mut distance_definition
            .relations
            .as_mut()
            .expect("relations")
            .rows[0];
        distance_relation.relation_type = 0;
        distance_relation.sign = 1;
        distance_relation.dimension_id = 0;
        distance_relation.operand_vectors = Some([
            [Some(1), Some(2), None, Some(1)],
            [Some(0), Some(0), Some(0), Some(0)],
            [Some(15), Some(16), Some(15), Some(1)],
        ]);
        assert_eq!(
            section_dimension_constraints(&distance_definition, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::VerticalDistance {
                first: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )),
                second: SketchLocus::End(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )),
                parameter: ParameterId("creo:featdefs:parameter#917:40:42".to_string()),
            }
        );
        let mut solver_definition = distance_definition.clone();
        solver_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .clear();
        assert_eq!(
            resolved_section_points(&solver_definition).get(&2),
            Some(&[0.0, 5.0])
        );
        let mut equivalent_relation = solver_definition.clone();
        let duplicate = equivalent_relation
            .relations
            .as_ref()
            .expect("relations")
            .rows[0]
            .clone();
        equivalent_relation
            .relations
            .as_mut()
            .expect("relations")
            .rows
            .push(duplicate);
        assert_eq!(
            resolved_section_points(&equivalent_relation).get(&2),
            Some(&[0.0, 5.0])
        );
        let conflicting_relation = equivalent_relation
            .relations
            .as_mut()
            .expect("relations")
            .rows
            .last_mut()
            .expect("duplicate relation");
        conflicting_relation.sign = 0xf6;
        assert!(!resolved_section_points(&equivalent_relation).contains_key(&2));
        let mut duplicate_identity = solver_definition.clone();
        let mut duplicate = duplicate_identity.segments.as_ref().expect("segments").rows[0].clone();
        duplicate.offset = 500;
        duplicate_identity
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(duplicate);
        assert!(!resolved_section_points(&duplicate_identity).contains_key(&2));
        let mut duplicate_endpoint_segment = solver_definition;
        let mut duplicate = duplicate_endpoint_segment
            .segments
            .as_ref()
            .expect("segments")
            .rows[0]
            .clone();
        duplicate.external_id = 99;
        duplicate.offset = 501;
        duplicate_endpoint_segment
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(duplicate);
        assert!(!resolved_section_points(&duplicate_endpoint_segment).contains_key(&2));
        let mut shared_vertex_definition = distance_definition.clone();
        let mut incident = shared_vertex_definition
            .segments
            .as_ref()
            .expect("segments")
            .rows[1]
            .clone();
        incident.external_id = 2;
        incident.point_ids = [9, 1];
        shared_vertex_definition
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(incident);
        assert_eq!(
            section_dimension_constraints(&shared_vertex_definition, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::VerticalDistance {
                first: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )),
                second: SketchLocus::End(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )),
                parameter: ParameterId("creo:featdefs:parameter#917:40:42".to_string()),
            }
        );
        let mut duplicate_relation_id = distance_definition.clone();
        let mut duplicate = duplicate_relation_id
            .relations
            .as_ref()
            .expect("relations")
            .rows[0]
            .clone();
        duplicate.offset = 500;
        duplicate_relation_id
            .relations
            .as_mut()
            .expect("relations")
            .rows
            .push(duplicate);
        let duplicate_constraints =
            section_dimension_constraints(&duplicate_relation_id, &SketchId("sketch".into()));
        assert!(duplicate_constraints.iter().all(|(constraint, _)| matches!(
            constraint.definition,
            SketchConstraintDefinition::Native { .. }
        )));
        assert_eq!(
            duplicate_constraints[0].0.id.0,
            "creo:featdefs:sketch_constraint#917:relation:offset:80"
        );
        assert_eq!(
            duplicate_constraints[1].0.id.0,
            "creo:featdefs:sketch_constraint#917:relation:offset:500"
        );
        let mut duplicate_measured_segment = distance_definition.clone();
        let duplicate = duplicate_measured_segment
            .segments
            .as_ref()
            .expect("segments")
            .rows[0]
            .clone();
        duplicate_measured_segment
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(duplicate);
        assert!(matches!(
            section_dimension_constraints(
                &duplicate_measured_segment,
                &SketchId("sketch".into())
            )[0]
                .0
                .definition,
            SketchConstraintDefinition::Native {
                ref native_kind,
                ..
            } if native_kind == "creo:relation:0"
        ));
        duplicate_measured_segment
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .last_mut()
            .expect("duplicate")
            .point_ids = [8, 9];
        assert!(matches!(
            section_dimension_constraints(
                &duplicate_measured_segment,
                &SketchId("sketch".into())
            )[0]
                .0
                .definition,
            SketchConstraintDefinition::Native {
                ref native_kind,
                ..
            } if native_kind == "creo:relation:0"
        ));
        let mut angular_distance = distance_definition.clone();
        angular_distance
            .dimensions
            .as_mut()
            .expect("dimensions")
            .rows[0]
            .value_unit = crate::feature::DimensionUnit::Radians;
        assert!(matches!(
            section_dimension_constraints(&angular_distance, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Native {
                ref native_kind,
                ..
            } if native_kind == "creo:relation:0"
        ));
        let mut duplicate_dimension = distance_definition.clone();
        let duplicate = duplicate_dimension
            .dimensions
            .as_ref()
            .expect("dimensions")
            .rows[0]
            .clone();
        duplicate_dimension
            .dimensions
            .as_mut()
            .expect("dimensions")
            .rows
            .push(duplicate);
        assert_eq!(
            section_dimension_constraints(&duplicate_dimension, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Native {
                native_kind: "creo:relation:0".to_string(),
                entities: Vec::new(),
                parameter: None,
                operands: vec![SketchNativeOperand {
                    native_kind: "relat_ptr".to_string(),
                    object_index: 8,
                    native_ref: Some("creo:featdefs:sketch#917".to_string()),
                }],
            }
        );
        let mut radius_definition = definition.clone();
        radius_definition.segments.as_mut().expect("segments").rows[1].radius_ref = Some(101);
        let radius_relation = &mut radius_definition
            .relations
            .as_mut()
            .expect("relations")
            .rows[0];
        radius_relation.relation_type = 14;
        radius_relation.sign = 1;
        radius_relation.dimension_id = 0;
        radius_relation.operand_vectors = Some([
            [Some(101), Some(0), Some(0), Some(0)],
            [Some(0), Some(0), Some(0), Some(0)],
            [Some(15), Some(0), Some(0), Some(0)],
        ]);
        assert_eq!(
            section_dimension_constraints(&radius_definition, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Radius {
                entity: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
                parameter: ParameterId("creo:featdefs:parameter#917:40:42".to_string()),
            }
        );
        let duplicate = radius_definition.segments.as_ref().expect("segments").rows[1].clone();
        radius_definition
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(duplicate);
        assert!(matches!(
            section_dimension_constraints(&radius_definition, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Native {
                ref native_kind,
                ..
            } if native_kind == "creo:relation:14"
        ));
        radius_definition
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .pop();
        radius_definition
            .dimensions
            .as_mut()
            .expect("dimensions")
            .rows[0]
            .value_unit = crate::feature::DimensionUnit::Radians;
        assert!(matches!(
            section_dimension_constraints(&radius_definition, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Native {
                ref native_kind,
                ..
            } if native_kind == "creo:relation:14"
        ));
        let mut incidence_distance = distance_definition.clone();
        let incidence_relations = incidence_distance.relations.as_mut().expect("relations");
        incidence_relations.rows[0].operand_vectors = None;
        incidence_relations.skamps = vec![crate::feature::FeatureSkamp {
            id: 81,
            kind: 0,
            flags: 0,
            status: 0,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 13,
                    sense: 0,
                },
            ],
            offset: 81,
        }];
        incidence_relations.triples = vec![crate::feature::FeatureRelationTriple {
            relation_id: Some(8),
            equation_id: None,
            skamp_id: Some(81),
            offset: 82,
        }];
        assert_eq!(
            section_dimension_constraints(&incidence_distance, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::DistanceLoci {
                first: SketchLocus::Entity(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string(),
                )),
                second: SketchLocus::Entity(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string(),
                )),
                parameter: ParameterId("creo:featdefs:parameter#917:40:42".to_string()),
            }
        );
        incidence_distance
            .relations
            .as_mut()
            .expect("relations")
            .skamps[0]
            .items[0]
            .sense = 2;
        assert_eq!(
            section_dimension_constraints(&incidence_distance, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::DistanceLoci {
                first: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string(),
                )),
                second: SketchLocus::Entity(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string(),
                )),
                parameter: ParameterId("creo:featdefs:parameter#917:40:42".to_string()),
            }
        );
        let mut partially_resolved_incidence = incidence_distance.clone();
        partially_resolved_incidence
            .relations
            .as_mut()
            .expect("relations")
            .skamps[0]
            .items[1]
            .entity_id = 999;
        assert_eq!(
            section_dimension_constraints(
                &partially_resolved_incidence,
                &SketchId("sketch".into())
            )[0]
            .0
            .definition,
            SketchConstraintDefinition::Native {
                native_kind: "creo:relation:0".to_string(),
                entities: vec![SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )],
                parameter: Some(ParameterId("creo:featdefs:parameter#917:40:42".to_string())),
                operands: vec![SketchNativeOperand {
                    native_kind: "relat_ptr".to_string(),
                    object_index: 8,
                    native_ref: Some("creo:featdefs:sketch#917".to_string()),
                }],
            }
        );
        let mut unresolved_angle = definition.clone();
        let angle_dimension = &mut unresolved_angle
            .dimensions
            .as_mut()
            .expect("dimensions")
            .rows[0];
        angle_dimension.dimension_type = 10;
        angle_dimension.value_unit = crate::feature::DimensionUnit::Radians;
        let angle_relation = &mut unresolved_angle.relations.as_mut().expect("relations").rows[0];
        angle_relation.relation_type = 1;
        angle_relation.operand_vectors = Some([
            [Some(4), Some(5), None, Some(1)],
            [Some(1), None, Some(1), Some(1)],
            [Some(15), Some(16), Some(15), Some(24)],
        ]);
        unresolved_angle.order_table = Some(crate::feature::FeatureOrderTable {
            declared_count: 2,
            entity_ref: None,
            rows: vec![
                crate::feature::FeatureOrderRow {
                    external_id: 12,
                    internal_id: 4,
                    bitmask: 1,
                    offset: 90,
                },
                crate::feature::FeatureOrderRow {
                    external_id: 15,
                    internal_id: 5,
                    bitmask: 1,
                    offset: 91,
                },
            ],
            offset: 89,
        });
        assert_eq!(
            section_dimension_constraints(&unresolved_angle, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Native {
                native_kind: "creo:relation:1".to_string(),
                entities: Vec::new(),
                parameter: Some(ParameterId("creo:featdefs:parameter#917:40:42".to_string(),)),
                operands: vec![SketchNativeOperand {
                    native_kind: "relat_ptr".to_string(),
                    object_index: 8,
                    native_ref: Some("creo:featdefs:sketch#917".to_string()),
                }],
            }
        );
        let relations = section_dimension_constraints(&definition, &SketchId("sketch".into()));
        assert_eq!(
            relations[0].0.definition,
            SketchConstraintDefinition::Native {
                native_kind: "creo:relation:99".to_string(),
                entities: Vec::new(),
                parameter: Some(ParameterId("creo:featdefs:parameter#917:40:42".to_string(),)),
                operands: vec![SketchNativeOperand {
                    native_kind: "relat_ptr".to_string(),
                    object_index: 8,
                    native_ref: Some("creo:featdefs:sketch#917".to_string()),
                }],
            }
        );
        let mut coincident_definition = definition.clone();
        coincident_definition
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [7, 8],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 17,
                offset: 87,
            });
        coincident_definition.variables = Some(crate::feature::FeatureVariableTable {
            declared_count: 2,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![
                crate::feature::FeatureSectionPoint {
                    point_id: 2,
                    u: Some(3.0),
                    v: Some(4.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 5,
                    u: None,
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 4,
                    u: None,
                    v: Some(9.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 6,
                    u: None,
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 7,
                    u: Some(2.0),
                    v: Some(6.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 8,
                    u: None,
                    v: None,
                },
            ],
            offset: 0,
        });
        coincident_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps = vec![
            crate::feature::FeatureSkamp {
                id: 17,
                kind: 0,
                flags: 0,
                status: 0,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 3,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 15,
                        sense: 2,
                    },
                ],
                offset: 83,
            },
            crate::feature::FeatureSkamp {
                id: 18,
                kind: 3,
                flags: 0,
                status: 0,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 14,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 15,
                        sense: 3,
                    },
                ],
                offset: 84,
            },
            crate::feature::FeatureSkamp {
                id: 19,
                kind: 2,
                flags: 0,
                status: 0,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 15,
                    sense: 0,
                }],
                offset: 85,
            },
            crate::feature::FeatureSkamp {
                id: 20,
                kind: 9,
                flags: 0,
                status: 0,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 15,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 14,
                        sense: 0,
                    },
                ],
                offset: 86,
            },
            crate::feature::FeatureSkamp {
                id: 21,
                kind: 1,
                flags: 0,
                status: 0,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                }],
                offset: 88,
            },
            crate::feature::FeatureSkamp {
                id: 22,
                kind: 14,
                flags: 0,
                status: 0,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 17,
                        sense: 2,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 17,
                        sense: 3,
                    },
                ],
                offset: 89,
            },
            crate::feature::FeatureSkamp {
                id: 23,
                kind: 5,
                flags: 0,
                status: 0,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 17,
                        sense: 0,
                    },
                ],
                offset: 90,
            },
        ];
        let related_line = unique_section_skamp_segment(&coincident_definition, 17).expect("line");
        assert_eq!(
            section_line_fixed_coordinate(&coincident_definition, related_line),
            Some(0)
        );
        let coincident_points = resolved_section_points(&coincident_definition);
        assert_eq!(coincident_points.get(&5), Some(&[3.0, 4.0]));
        assert_eq!(coincident_points.get(&4), Some(&[3.0, 9.0]));
        assert_eq!(coincident_points.get(&6), Some(&[3.0, 9.0]));
        assert_eq!(coincident_points.get(&8), Some(&[2.0, 2.0]));
        let mut ambiguous_definition = coincident_definition.clone();
        let duplicate = ambiguous_definition
            .variables
            .as_ref()
            .and_then(|table| table.points.iter().find(|point| point.point_id == 5))
            .cloned()
            .expect("point 5");
        ambiguous_definition
            .variables
            .as_mut()
            .expect("variables")
            .points
            .push(duplicate);
        assert_eq!(
            resolved_section_points(&ambiguous_definition).get(&5),
            Some(&[3.0, 4.0])
        );
        let complementary = ambiguous_definition
            .variables
            .as_mut()
            .expect("variables")
            .points
            .last_mut()
            .expect("duplicate point");
        complementary.u = Some(3.0);
        assert_eq!(
            resolved_section_points(&ambiguous_definition).get(&5),
            Some(&[3.0, 4.0])
        );
        let mut conflicting_definition = coincident_definition.clone();
        let mut conflicting = conflicting_definition
            .variables
            .as_ref()
            .and_then(|table| table.points.iter().find(|point| point.point_id == 2))
            .cloned()
            .expect("point 2");
        conflicting.u = Some(30.0);
        conflicting_definition
            .variables
            .as_mut()
            .expect("variables")
            .points
            .push(conflicting);
        assert!(!resolved_section_points(&conflicting_definition).contains_key(&2));
        let conflicting_record = sketch_section_point_records(&conflicting_definition)
            .into_iter()
            .find(|point| point.point_id == 2)
            .expect("conflicting point record");
        assert_eq!(conflicting_record.state, "conflicting");
        assert_eq!([conflicting_record.u, conflicting_record.v], [None; 2]);
        let mut saved_definition = definition;
        saved_definition.order_table = Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
                external_id: 14,
                internal_id: 20,
                bitmask: 1,
                offset: 81,
            }],
            offset: 80,
        });
        saved_definition.saved_section = Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Line(
                crate::feature::FeatureSavedLine {
                    entity_id: 20,
                    references: Vec::new(),
                    attributes: Vec::new(),
                    endpoints: [
                        [Some(0.0), Some(0.0), Some(0.0)],
                        [Some(1.0), Some(0.0), Some(0.0)],
                    ],
                    offset: 82,
                },
            )],
            offset: 82,
        });
        assert_eq!(
            section_skamp_endpoint(
                &saved_definition,
                &crate::feature::FeatureSkampItem {
                    entity_id: 14,
                    sense: 3,
                },
            ),
            Some(SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:14".to_string()
            )))
        );
        saved_definition
            .order_table
            .as_mut()
            .expect("order table")
            .rows
            .push(crate::feature::FeatureOrderRow {
                external_id: 99,
                internal_id: 21,
                bitmask: 1,
                offset: 83,
            });
        saved_definition
            .saved_section
            .as_mut()
            .expect("saved section")
            .entities
            .push(crate::feature::FeatureSavedEntity::Line(
                crate::feature::FeatureSavedLine {
                    entity_id: 21,
                    references: Vec::new(),
                    attributes: Vec::new(),
                    endpoints: [
                        [Some(0.0), Some(1.0), Some(0.0)],
                        [Some(1.0), Some(1.0), Some(0.0)],
                    ],
                    offset: 84,
                },
            ));
        saved_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps = vec![crate::feature::FeatureSkamp {
            id: 30,
            kind: 1,
            flags: 0,
            status: 0,
            items: vec![crate::feature::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            }],
            offset: 85,
        }];
        assert_eq!(
            section_skamp_constraints(&saved_definition, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Horizontal {
                entity: SketchEntityId("creo:featdefs:sketch_entity#917:99".to_string()),
            }
        );
        saved_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps = vec![crate::feature::FeatureSkamp {
            id: 31,
            kind: 7,
            flags: 0,
            status: 0,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 99,
                    sense: 0,
                },
            ],
            offset: 86,
        }];
        let segment = unique_section_skamp_segment(&saved_definition, 12).expect("segment line");
        assert_eq!(
            section_line_fixed_coordinate(&saved_definition, segment),
            Some(1)
        );
        saved_definition
            .variables
            .as_mut()
            .expect("variables")
            .points
            .push(crate::feature::FeatureSectionPoint {
                point_id: 4,
                u: Some(3.0),
                v: None,
            });
        saved_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps = vec![crate::feature::FeatureSkamp {
            id: 32,
            kind: 9,
            flags: 0,
            status: 0,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 99,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 14,
                    sense: 0,
                },
            ],
            offset: 87,
        }];
        assert_eq!(
            resolved_section_points(&saved_definition).get(&4),
            Some(&[3.0, 1.0])
        );
        let mut saved_coincident = saved_definition.clone();
        for point in &mut saved_coincident
            .variables
            .as_mut()
            .expect("variables")
            .points
        {
            if matches!(point.point_id, 4 | 5) {
                point.u = None;
                point.v = None;
            }
        }
        saved_coincident
            .relations
            .as_mut()
            .expect("relations")
            .skamps = vec![
            crate::feature::FeatureSkamp {
                id: 33,
                kind: 0,
                flags: 0,
                status: 0,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 99,
                        sense: 2,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 15,
                        sense: 2,
                    },
                ],
                offset: 88,
            },
            crate::feature::FeatureSkamp {
                id: 34,
                kind: 3,
                flags: 0,
                status: 0,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 14,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 99,
                        sense: 3,
                    },
                ],
                offset: 89,
            },
        ];
        assert_eq!(
            resolved_section_points(&saved_coincident),
            BTreeMap::from([(1, [0.0, 2.0]), (4, [1.0, 1.0]), (5, [0.0, 1.0])])
        );
        saved_definition
            .variables
            .as_mut()
            .expect("variables")
            .points
            .iter_mut()
            .find(|point| point.point_id == 5)
            .expect("point 5")
            .v = None;
        saved_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps = vec![crate::feature::FeatureSkamp {
            id: 33,
            kind: 14,
            flags: 0,
            status: 0,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 99,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 2,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 15,
                    sense: 2,
                },
            ],
            offset: 88,
        }];
        assert_eq!(
            resolved_section_points(&saved_definition).get(&5),
            Some(&[3.0, 0.0])
        );
        let mut duplicate_saved = saved_definition
            .saved_section
            .as_ref()
            .expect("saved section")
            .entities[1]
            .clone();
        if let crate::feature::FeatureSavedEntity::Line(line) = &mut duplicate_saved {
            line.offset = 86;
        }
        saved_definition
            .saved_section
            .as_mut()
            .expect("saved section")
            .entities
            .push(duplicate_saved);
        assert!(matches!(
            section_skamp_constraints(&saved_definition, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Native { .. }
        ));
    }

    #[test]
    fn zero_orientation_arc_runs_clockwise_from_first_endpoint() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: Some(3),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: Some(4),
            radius2_ref: None,
            external_id: 12,
            offset: 40,
        };
        let points = BTreeMap::from([(1, [0.0, -2.0]), (2, [0.0, 2.0]), (3, [0.0, 0.0])]);
        let Some(SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        }) = section_arc_geometry(&points, &segment)
        else {
            panic!("complete arc");
        };
        assert_eq!(center, cadmpeg_ir::math::Point2::new(0.0, 0.0));
        assert_eq!(radius, Length(2.0));
        assert!((start_angle.0 - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((end_angle.0 - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn profile_chain_follows_trim_vertex_incidence() {
        let definition = crate::feature::FeatureDefinition {
            id: 40,
            owner_feature_id: Some(40),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: Some(crate::feature::FeatureTrimEntityTable {
                declared_count: None,
                entity_ref: None,
                entry_ref: None,
                rows: [(10, [1, 2]), (11, [3, 2]), (12, [3, 4]), (13, [4, 1])]
                    .into_iter()
                    .map(
                        |(external_id, vertices)| crate::feature::FeatureTrimEntity {
                            external_id,
                            mode: None,
                            vertices,
                            center_vertex: None,
                            kind: crate::feature::TrimEntityKind::Line,
                            offset: external_id as usize,
                        },
                    )
                    .collect(),
                solved_external_ids: vec![10, 11, 12, 13],
                offset: 5,
            }),
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 1,
        };
        let profiles = resolved_profile_chains(
            &definition,
            &BTreeSet::from([10_u32, 11_u32, 12_u32, 13_u32]),
        );
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].len(), 4);
        assert_eq!(profiles[0][0].entity.0, "creo:featdefs:sketch_entity#40:10");
        assert!(!profiles[0][0].reversed);
        assert!(profiles[0][1].reversed);

        assert!(
            resolved_profile_chains(&definition, &BTreeSet::from([10_u32, 11_u32, 12_u32]))
                .is_empty()
        );

        let mut arcs = definition.clone();
        arcs.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            rows: [(10, [1, 2]), (11, [2, 1])]
                .into_iter()
                .map(
                    |(external_id, vertices)| crate::feature::FeatureTrimEntity {
                        external_id,
                        mode: None,
                        vertices,
                        center_vertex: Some(3),
                        kind: crate::feature::TrimEntityKind::Arc,
                        offset: external_id as usize,
                    },
                )
                .collect(),
            solved_external_ids: vec![10, 11],
            offset: 5,
        });
        arcs.segments = Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            entity_ref: None,
            rows: [10, 11]
                .into_iter()
                .map(|external_id| crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Arc,
                    directions: [None; 3],
                    point_ids: [1, 2],
                    center_id: Some(3),
                    arc_orientation: Some(0),
                    vertical_horizontal: None,
                    radius_ref: None,
                    radius2_ref: None,
                    external_id,
                    offset: external_id as usize,
                })
                .collect(),
            offset: 4,
        });
        let arc_profile = resolved_profile_chains(&arcs, &BTreeSet::from([10, 11]));
        assert_eq!(arc_profile.len(), 1);
        assert!(arc_profile[0].iter().all(|entity| entity.reversed));

        let mut segment_graph = definition;
        segment_graph.trim_entities = None;
        segment_graph.segments = Some(crate::feature::FeatureSegmentTable {
            declared_count: 5,
            entity_ref: None,
            rows: [
                (10, [1, 2]),
                (11, [3, 2]),
                (12, [3, 4]),
                (13, [4, 1]),
                (20, [8, 9]),
            ]
            .into_iter()
            .map(|(external_id, point_ids)| crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids,
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id,
                offset: external_id as usize,
            })
            .collect(),
            offset: 4,
        });
        let segment_profile =
            resolved_profile_chains(&segment_graph, &BTreeSet::from([10, 11, 12, 13, 20]));
        assert_eq!(segment_profile.len(), 1);
        assert_eq!(segment_profile[0].len(), 4);
        assert!(!segment_profile[0][0].reversed);
        assert!(segment_profile[0][1].reversed);
    }

    #[test]
    fn revolution_axis_uses_the_unique_complete_section_centerline() {
        let definition = crate::feature::FeatureDefinition {
            id: 40,
            owner_feature_id: Some(40),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                points: vec![
                    crate::feature::FeatureSectionPoint {
                        point_id: 1,
                        u: Some(0.0),
                        v: Some(-2.0),
                    },
                    crate::feature::FeatureSectionPoint {
                        point_id: 2,
                        u: Some(0.0),
                        v: Some(3.0),
                    },
                ],
                offset: 1,
            }),
            segments: Some(crate::feature::FeatureSegmentTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Line,
                    directions: [None; 3],
                    point_ids: [1, 2],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: Some(0),
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 1,
                    offset: 2,
                }],
                offset: 2,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 1,
        };
        let transform = crate::placement::FeatureSectionTransform {
            definition_id: 40,
            feature_id: Some(40),
            origin: [5.0, 7.0, 11.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
            normal: [0.0, -1.0, 0.0],
            offset: 3,
        };

        let axis = resolved_revolution_axis(&definition, &transform).expect("axis");
        assert_eq!(axis.origin, Point3::new(5.0, 7.0, 9.0));
        assert_eq!(axis.direction, Vector3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn saved_spline_collocation_interpolates_points_and_endpoint_derivatives() {
        let spline = crate::feature::FeatureSavedSpline {
            entity_id: Some(7),
            interpolation_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            endpoint_tangents: Some([[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
            parameters: Some(vec![0.0, 1.0, 2.0]),
            offset: 10,
        };
        let nurbs = saved_spline_nurbs(&spline).expect("clamped interpolation spline");
        for (parameter, expected) in [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)] {
            let point = nurbs.control_points.iter().enumerate().fold(
                [0.0; 3],
                |mut point, (index, control)| {
                    let basis = bspline_basis(
                        index,
                        nurbs.degree as usize,
                        parameter,
                        &nurbs.knots,
                        nurbs.control_points.len(),
                    );
                    point[0] += basis * control.x;
                    point[1] += basis * control.y;
                    point[2] += basis * control.z;
                    point
                },
            );
            assert!((point[0] - expected).abs() < 1e-12);
            assert!(point[1].abs() < 1e-12 && point[2].abs() < 1e-12);
        }
        for parameter in [0.0, 2.0] {
            let derivative = nurbs.control_points.iter().enumerate().fold(
                [0.0; 3],
                |mut derivative, (index, control)| {
                    let basis = bspline_basis_derivative(
                        index,
                        nurbs.degree as usize,
                        parameter,
                        &nurbs.knots,
                        nurbs.control_points.len(),
                    );
                    derivative[0] += basis * control.x;
                    derivative[1] += basis * control.y;
                    derivative[2] += basis * control.z;
                    derivative
                },
            );
            assert!((derivative[0] - 1.0).abs() < 1e-12);
            assert!(derivative[1].abs() < 1e-12 && derivative[2].abs() < 1e-12);
        }
    }

    #[test]
    fn tensor_product_collocation_preserves_position_and_derivative_order() {
        let points = [
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 2.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 3.0],
        ];
        let du = [1.0, 0.0, 1.0];
        let dv = [0.0, 1.0, 2.0];
        let zero = [0.0; 3];
        let nurbs = interpolation_spline_surface(
            &points,
            &[0.0, 1.0],
            &[0.0, 1.0],
            &[du, du, du, du],
            &[dv, dv, dv, dv],
            &[zero, zero, zero, zero],
        )
        .expect("bicubic tensor-product surface");

        assert_eq!((nurbs.u_count, nurbs.v_count), (4, 4));
        assert_eq!(nurbs.u_knots, [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(nurbs.v_knots, nurbs.u_knots);
        for u in 0..4 {
            for v in 0..4 {
                let point = &nurbs.control_points[u * 4 + v];
                let expected_u = u as f64 / 3.0;
                let expected_v = v as f64 / 3.0;
                assert!((point.x - expected_u).abs() < 1e-12);
                assert!((point.y - expected_v).abs() < 1e-12);
                assert!((point.z - expected_u - 2.0 * expected_v).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn nonplanar_saved_spline_places_as_model_curve() {
        let transform = crate::placement::FeatureSectionTransform {
            definition_id: 917,
            feature_id: Some(40),
            origin: [10.0, 20.0, 30.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
            normal: [0.0, -1.0, 0.0],
            offset: 5,
        };
        let local = NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
            weights: None,
            periodic: false,
        };

        let placed = placed_section_nurbs(&transform, &local);

        assert_eq!(placed.control_points[0], Point3::new(11.0, 17.0, 32.0));
        assert_eq!(placed.control_points[1], Point3::new(14.0, 14.0, 35.0));
    }

    #[test]
    fn transferred_geometry_is_derived_from_ir_arenas() {
        let mut ir = CadIr::empty(Units::default());
        assert!(!has_transferred_geometry(&ir));

        ir.model.points.push(Point {
            id: PointId("point".to_string()),
            position: Point3::new(1.0, 2.0, 3.0),
        });
        assert!(has_transferred_geometry(&ir));
    }

    #[test]
    fn full_revolution_uses_exact_quadratic_circle_poles() {
        let directrix = NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
            weights: None,
            periodic: false,
        };
        let surface = revolved_nurbs_surface(
            &directrix,
            RevolutionAxis {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(0.0, 0.0, 1.0),
            },
        )
        .expect("revolution surface");

        assert_eq!((surface.u_count, surface.v_count), (2, 9));
        assert_eq!(surface.control_points[0], Point3::new(2.0, 0.0, 0.0));
        assert_eq!(surface.control_points[1], Point3::new(2.0, 2.0, 0.0));
        assert_eq!(surface.control_points[2], Point3::new(0.0, 2.0, 0.0));
        assert_eq!(surface.control_points[8], surface.control_points[0]);
        assert_eq!(
            surface.weights.as_ref().expect("rational weights")[1],
            std::f64::consts::FRAC_1_SQRT_2
        );
    }

    #[test]
    fn planar_loop_containment_selects_one_outer_boundary() {
        let make_loop = |face_id: u32, first_curve: u32| crate::topology::Loop {
            face_id,
            half_edges: (0_u32..4)
                .map(|index| HalfEdgeId {
                    curve_id: first_curve + index,
                    side: 0,
                })
                .collect(),
        };
        let outer = make_loop(9, 1);
        let inner = make_loop(9, 5);
        let incidences = (1..=8)
            .map(|vertex| crate::topology::HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: vertex,
                    side: 0,
                },
                start_vertex_id: vertex,
                end_vertex_id: Some(if vertex % 4 == 0 {
                    vertex - 3
                } else {
                    vertex + 1
                }),
            })
            .collect::<Vec<_>>();
        let incidence = incidences
            .iter()
            .map(|binding| (binding.half_edge, binding))
            .collect::<BTreeMap<_, _>>();
        let points = BTreeMap::from([
            (1, [-2.0, -2.0, 0.0]),
            (2, [2.0, -2.0, 0.0]),
            (3, [2.0, 2.0, 0.0]),
            (4, [-2.0, 2.0, 0.0]),
            (5, [-1.0, -1.0, 0.0]),
            (6, [1.0, -1.0, 0.0]),
            (7, [1.0, 1.0, 0.0]),
            (8, [-1.0, 1.0, 0.0]),
        ]);
        let plane = PlaneEquation {
            origin: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
        };

        let ordered = ordered_planar_face_loops(vec![&inner, &outer], plane, &incidence, &points)
            .expect("unique outer loop");
        assert_eq!(ordered[0].half_edges[0].curve_id, 1);
        assert_eq!(ordered[1].half_edges[0].curve_id, 5);

        let disjoint_points = points
            .into_iter()
            .map(|(id, mut point)| {
                if id >= 5 {
                    point[0] += 10.0;
                }
                (id, point)
            })
            .collect::<BTreeMap<_, _>>();
        assert!(ordered_planar_face_loops(
            vec![&outer, &inner],
            plane,
            &incidence,
            &disjoint_points,
        )
        .is_none());
        assert_eq!(
            ordered_face_loops(vec![&outer], None, &incidence, &disjoint_points),
            Some(vec![&outer])
        );
        assert!(
            ordered_face_loops(vec![&outer, &inner], None, &incidence, &disjoint_points,).is_none()
        );
    }

    #[test]
    fn carrier_solver_accepts_two_carrier_tangent_vertices() {
        let plane = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 2.0],
            normal: [0.0, 0.0, 1.0],
        });
        let sphere = CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 2.0,
        });
        assert_eq!(solve_carriers(&[plane, sphere]), Some([0.0, 0.0, 2.0]));

        let second_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [5.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 3.0,
        });
        assert_eq!(
            solve_carriers(&[sphere, second_sphere]),
            Some([2.0, 0.0, 0.0])
        );

        let secant = CarrierEquation::Sphere(SphereEquation {
            center: [3.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 2.0,
        });
        assert_eq!(solve_carriers(&[sphere, secant]), None);
    }

    #[test]
    fn carrier_solver_accepts_unique_plane_plane_cylinder_vertex() {
        let cylinder = CarrierEquation::Cylinder(CylinderEquation {
            origin: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 2.0,
        });
        let cap = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 3.0],
            normal: [0.0, 0.0, 1.0],
        });
        let tangent = CarrierEquation::Plane(PlaneEquation {
            origin: [2.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(
            solve_carriers(&[cylinder, cap, tangent]),
            Some([2.0, 0.0, 3.0])
        );

        let secant = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(solve_carriers(&[cylinder, cap, secant]), None);

        assert!(matches!(
            carrier_intersection_curve(cap, cylinder),
            Some((CurveGeometry::Circle { center, radius, .. }, "plane_cylinder_circle"))
                if center.z == 3.0 && radius == 2.0
        ));
        let oblique = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 1.0],
        });
        assert!(matches!(
            carrier_intersection_curve(oblique, cylinder),
            Some((CurveGeometry::Ellipse { major_radius, minor_radius, .. }, "plane_cylinder_ellipse"))
                if (major_radius - 2.0 * 2.0_f64.sqrt()).abs() < 1e-12
                    && minor_radius == 2.0
        ));
        assert!(matches!(
            carrier_intersection_curve(tangent, cylinder),
            Some((CurveGeometry::Line { origin, direction }, "plane_cylinder_tangent_line"))
                if origin.x == 2.0 && direction.z == 1.0
        ));
        assert!(carrier_intersection_curve(secant, cylinder).is_none());
        let generators = parallel_plane_cylinder_generator_candidates(secant, cylinder);
        assert_eq!(generators.len(), 2);
        assert!(matches!(
            select_parallel_plane_cylinder_generator(
                secant,
                cylinder,
                [[0.0, 2.0, -1.0], [0.0, 2.0, 4.0]],
            ),
            Some((CurveGeometry::Line { origin, direction }, "plane_cylinder_secant_generator"))
                if (origin.y - 2.0).abs() < 1e-12 && direction.z == 1.0
        ));
        assert!(select_parallel_plane_cylinder_generator(
            secant,
            cylinder,
            [[0.0, 0.0, -1.0], [0.0, 0.0, 4.0]],
        )
        .is_none());

        let parallel_cylinder = |origin: [f64; 3], radius| {
            CarrierEquation::Cylinder(CylinderEquation {
                origin,
                axis: [0.0, 0.0, 1.0],
                ref_direction: [1.0, 0.0, 0.0],
                radius,
            })
        };
        assert!(matches!(
            carrier_intersection_curve(
                parallel_cylinder([0.0, 0.0, 0.0], 2.0),
                parallel_cylinder([5.0, 0.0, 0.0], 3.0),
            ),
            Some((CurveGeometry::Line { origin, direction }, "parallel_cylinder_tangent_line"))
                if origin.x == 2.0 && direction.z == 1.0
        ));
        assert_eq!(
            solve_carriers(&[
                cap,
                parallel_cylinder([0.0, 0.0, 0.0], 2.0),
                parallel_cylinder([5.0, 0.0, 0.0], 3.0),
            ]),
            Some([2.0, 0.0, 3.0])
        );
        assert!(matches!(
            carrier_intersection_curve(
                parallel_cylinder([0.0, 0.0, 0.0], 5.0),
                parallel_cylinder([3.0, 0.0, 0.0], 2.0),
            ),
            Some((CurveGeometry::Line { origin, .. }, "parallel_cylinder_tangent_line"))
                if origin.x == 5.0
        ));
        assert!(carrier_intersection_curve(
            parallel_cylinder([0.0, 0.0, 0.0], 3.0),
            parallel_cylinder([4.0, 0.0, 0.0], 3.0),
        )
        .is_none());

        let sphere = CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 2.0,
        });
        let equator = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        });
        assert!(matches!(
            carrier_intersection_curve(equator, sphere),
            Some((CurveGeometry::Circle { center, radius, .. }, "plane_sphere_circle"))
                if center == Point3::new(0.0, 0.0, 0.0) && radius == 2.0
        ));
        assert_eq!(solve_carriers(&[equator, secant, sphere]), None);
        assert_eq!(
            solve_carriers(&[equator, tangent, sphere]),
            Some([2.0, 0.0, 0.0])
        );
        let second_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [4.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 3.0,
        });
        let first_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 3.0,
        });
        assert!(matches!(
            carrier_intersection_curve(first_sphere, second_sphere),
            Some((CurveGeometry::Circle { center, radius, .. }, "sphere_intersection_circle"))
                if center.x == 2.0 && (radius - 5.0_f64.sqrt()).abs() < 1e-12
        ));
        let sphere_circle_tangent = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 5.0_f64.sqrt(), 0.0],
            normal: [0.0, 1.0, 0.0],
        });
        assert_eq!(
            solve_carriers(&[first_sphere, second_sphere, sphere_circle_tangent]),
            Some([2.0, 5.0_f64.sqrt(), 0.0])
        );
        let external_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [5.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 3.0,
        });
        assert_eq!(
            solve_carriers(&[sphere, external_tangent_sphere, equator]),
            Some([2.0, 0.0, 0.0])
        );
        let noncoaxial_cylinder = CarrierEquation::Cylinder(CylinderEquation {
            origin: [1.0, 3.0_f64.sqrt(), 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 2.0,
        });
        assert_eq!(
            solve_carriers(&[sphere, tangent, noncoaxial_cylinder]),
            Some([2.0, 0.0, 0.0])
        );
        let enclosing_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 5.0,
        });
        let internally_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [3.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 2.0,
        });
        assert_eq!(
            solve_carriers(&[enclosing_sphere, internally_tangent_sphere, equator]),
            Some([5.0, 0.0, 0.0])
        );
        assert!(matches!(
            carrier_intersection_curve(cylinder, sphere),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_sphere_circle"))
                if center == Point3::new(0.0, 0.0, 0.0) && radius == 2.0
        ));
        assert_eq!(
            solve_carriers(&[cylinder, sphere, tangent]),
            Some([2.0, 0.0, 0.0])
        );
        assert!(
            carrier_intersection_curve(parallel_cylinder([0.0, 0.0, 0.0], 1.0), sphere,).is_none()
        );

        let cone = CarrierEquation::Cone(ConeEquation {
            origin: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 2.0,
            half_angle: std::f64::consts::FRAC_PI_4,
        });
        assert!(matches!(
            carrier_intersection_curve(cap, cone),
            Some((CurveGeometry::Circle { center, radius, .. }, "plane_cone_circle"))
                if center == Point3::new(0.0, 0.0, 3.0) && (radius - 5.0).abs() < 1e-12
        ));
        let inverse_sqrt_two = 1.0 / 2.0_f64.sqrt();
        let cone_tangent_plane = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, -2.0],
            normal: [inverse_sqrt_two, 0.0, inverse_sqrt_two],
        });
        assert!(matches!(
            carrier_intersection_curve(cone_tangent_plane, cone),
            Some((CurveGeometry::Line { origin, direction }, "plane_cone_tangent_line"))
                if origin.x.abs() < 1e-12
                    && origin.y.abs() < 1e-12
                    && (origin.z + 2.0).abs() < 1e-12
                    && (direction.x + inverse_sqrt_two).abs() < 1e-12
                    && (direction.z - inverse_sqrt_two).abs() < 1e-12
        ));
        let cone_ellipse_plane = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 2.0],
            normal: [-0.2, 0.0, 1.0],
        });
        assert!(matches!(
            carrier_intersection_curve(cone_ellipse_plane, cone),
            Some((
                CurveGeometry::Ellipse {
                    major_radius,
                    minor_radius,
                    ..
                },
                "plane_cone_ellipse"
            )) if major_radius > minor_radius && minor_radius > 0.0
        ));
        let cone_parabola_plane = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 2.0],
            normal: [inverse_sqrt_two, 0.0, inverse_sqrt_two],
        });
        assert!(matches!(
            carrier_intersection_curve(cone_parabola_plane, cone),
            Some((
                CurveGeometry::Parabola { focal_distance, .. },
                "plane_cone_parabola"
            )) if focal_distance > 0.0
        ));
        let cone_hyperbola_plane = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 2.0],
            normal: [1.0, 0.0, 0.2],
        });
        assert!(matches!(
            carrier_intersection_curve(cone_hyperbola_plane, cone),
            Some((
                CurveGeometry::Hyperbola {
                    major_radius,
                    minor_radius,
                    ..
                },
                "plane_cone_hyperbola"
            )) if major_radius > 0.0 && minor_radius > 0.0
        ));
        let cone_degenerate_plane = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, -2.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert!(carrier_intersection_curve(cone_degenerate_plane, cone).is_none());
        let cone_generators = apex_plane_cone_generator_candidates(cone_degenerate_plane, cone);
        assert_eq!(cone_generators.len(), 2);
        assert!(matches!(
            select_unique_curve_candidate(
                cone_generators,
                [[0.0, 1.0, -1.0], [0.0, 2.0, 0.0]],
            ),
            Some((CurveGeometry::Line { origin, .. }, "plane_cone_secant_generator"))
                if (origin.z + 2.0).abs() < 1e-12
        ));
        assert_eq!(solve_carriers(&[cone, cap, tangent]), None);
        let cone_tangent = CarrierEquation::Plane(PlaneEquation {
            origin: [5.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(
            solve_carriers(&[cone, cap, cone_tangent]),
            Some([5.0, 0.0, 3.0])
        );
        let cone_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 2.0_f64.sqrt(),
        });
        assert!(matches!(
            carrier_intersection_curve(cone_tangent_sphere, cone),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_sphere_tangent_circle"))
                if (center.z + 1.0).abs() < 1e-12 && (radius - 1.0).abs() < 1e-12
        ));
        let cone_sphere_plane = CarrierEquation::Plane(PlaneEquation {
            origin: [1.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        let cone_sphere_vertex =
            solve_carriers(&[cone_tangent_sphere, cone, cone_sphere_plane]).expect("unique vertex");
        assert!((cone_sphere_vertex[0] - 1.0).abs() < 1e-12);
        assert!(cone_sphere_vertex[1].abs() < 1e-12);
        assert!((cone_sphere_vertex[2] + 1.0).abs() < 1e-12);
        assert!(carrier_intersection_curve(sphere, cone).is_none());
        let cone_secant_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 5.0,
        });
        let cone_sphere_candidates =
            coaxial_cone_sphere_circle_candidates(cone, cone_secant_sphere);
        assert_eq!(cone_sphere_candidates.len(), 2);
        let upper_parameter = (-4.0 + 184.0_f64.sqrt()) / 4.0;
        let upper_radius = 2.0 + upper_parameter;
        assert!(matches!(
            select_unique_curve_candidate(
                cone_sphere_candidates,
                [
                    [upper_radius, 0.0, upper_parameter],
                    [0.0, upper_radius, upper_parameter],
                ],
            ),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_sphere_secant_circle"))
                if (center.z - upper_parameter).abs() < 1e-12
                    && (radius - upper_radius).abs() < 1e-12
        ));

        let torus = CarrierEquation::Torus(TorusEquation {
            center: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            major_radius: 5.0,
            minor_radius: 2.0,
        });
        let torus_tangent = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 2.0],
            normal: [0.0, 0.0, 1.0],
        });
        assert!(matches!(
            carrier_intersection_curve(torus_tangent, torus),
            Some((CurveGeometry::Circle { center, radius, .. }, "plane_torus_tangent_circle"))
                if center == Point3::new(0.0, 0.0, 2.0) && radius == 5.0
        ));
        assert!(carrier_intersection_curve(equator, torus).is_none());
        let plane_torus_candidates = axis_normal_plane_torus_circle_candidates(equator, torus);
        assert_eq!(plane_torus_candidates.len(), 2);
        assert!(matches!(
            select_unique_curve_candidate(
                plane_torus_candidates,
                [[7.0, 0.0, 0.0], [0.0, 7.0, 0.0]],
            ),
            Some((CurveGeometry::Circle { center, radius, .. }, "plane_torus_secant_circle"))
                if center == Point3::new(0.0, 0.0, 0.0) && radius == 7.0
        ));
        let outer_tangent_cylinder = parallel_cylinder([0.0, 0.0, 0.0], 7.0);
        assert!(matches!(
            carrier_intersection_curve(outer_tangent_cylinder, torus),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_torus_tangent_circle"))
                if center == Point3::new(0.0, 0.0, 0.0) && radius == 7.0
        ));
        let secant_cylinder = parallel_cylinder([0.0, 0.0, 0.0], 6.0);
        let cylinder_torus_candidates =
            coaxial_cylinder_torus_circle_candidates(secant_cylinder, torus);
        assert_eq!(cylinder_torus_candidates.len(), 2);
        let section_height = 3.0_f64.sqrt();
        assert!(matches!(
            select_unique_curve_candidate(
                cylinder_torus_candidates,
                [
                    [6.0, 0.0, section_height],
                    [0.0, 6.0, section_height],
                ],
            ),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_torus_secant_circle"))
                if (center.z - section_height).abs() < 1e-12 && radius == 6.0
        ));
        let outer_circle_tangent = CarrierEquation::Plane(PlaneEquation {
            origin: [7.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(
            solve_carriers(&[outer_tangent_cylinder, torus, outer_circle_tangent]),
            Some([7.0, 0.0, 0.0])
        );
        assert!(
            carrier_intersection_curve(parallel_cylinder([0.0, 0.0, 0.0], 6.0), torus).is_none()
        );
        let torus_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 3.0,
        });
        assert!(matches!(
            carrier_intersection_curve(torus_tangent_sphere, torus),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_sphere_torus_tangent_circle"))
                if center == Point3::new(0.0, 0.0, 0.0) && (radius - 3.0).abs() < 1e-12
        ));
        let torus_secant_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 5.0,
        });
        let sphere_torus_candidates =
            coaxial_sphere_torus_circle_candidates(torus_secant_sphere, torus);
        assert_eq!(sphere_torus_candidates.len(), 2);
        let sphere_torus_height = 3.84_f64.sqrt();
        assert!(matches!(
            select_unique_curve_candidate(
                sphere_torus_candidates,
                [
                    [4.6, 0.0, sphere_torus_height],
                    [0.0, 4.6, sphere_torus_height],
                ],
            ),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_sphere_torus_secant_circle"))
                if (center.z - sphere_torus_height).abs() < 1e-12
                    && (radius - 4.6).abs() < 1e-12
        ));
        let torus_sphere_plane = CarrierEquation::Plane(PlaneEquation {
            origin: [3.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(
            solve_carriers(&[torus_tangent_sphere, torus, torus_sphere_plane]),
            Some([3.0, 0.0, 0.0])
        );
        assert!(carrier_intersection_curve(sphere, torus).is_none());
        let second_torus = CarrierEquation::Torus(TorusEquation {
            center: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            major_radius: 9.0,
            minor_radius: 2.0,
        });
        assert!(matches!(
            carrier_intersection_curve(torus, second_torus),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_tori_tangent_circle"))
                if center == Point3::new(0.0, 0.0, 0.0) && (radius - 7.0).abs() < 1e-12
        ));
        let secant_torus = CarrierEquation::Torus(TorusEquation {
            center: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            major_radius: 6.0,
            minor_radius: 2.0,
        });
        let tori_candidates = coaxial_tori_circle_candidates(torus, secant_torus);
        assert_eq!(tori_candidates.len(), 2);
        let tori_height = 3.75_f64.sqrt();
        assert!(matches!(
            select_unique_curve_candidate(
                tori_candidates,
                [[5.5, 0.0, tori_height], [0.0, 5.5, tori_height]],
            ),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_tori_secant_circle"))
                if (center.z - tori_height).abs() < 1e-12
                    && (radius - 5.5).abs() < 1e-12
        ));
        let tori_plane = CarrierEquation::Plane(PlaneEquation {
            origin: [7.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(
            solve_carriers(&[torus, second_torus, tori_plane]),
            Some([7.0, 0.0, 0.0])
        );
        assert!(point_on_carrier([5.0, 0.0, 2.0], torus));
        assert!(!point_on_carrier([5.0, 0.0, 0.0], torus));
        assert_eq!(
            solve_carriers(&[torus, torus_tangent, cone_tangent]),
            Some([5.0, 0.0, 2.0])
        );
    }
}

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
    half_angle: f64,
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

fn intersect_two_planes_with_cylinder(
    first: PlaneEquation,
    second: PlaneEquation,
    cylinder: CylinderEquation,
) -> Vec<[f64; 3]> {
    let direction = cross(first.normal, second.normal);
    let denominator = dot(direction, direction);
    if denominator <= 1e-18 {
        return Vec::new();
    }
    let first_distance = dot(first.normal, first.origin);
    let second_distance = dot(second.normal, second.origin);
    let second_cross_direction = cross(second.normal, direction);
    let direction_cross_first = cross(direction, first.normal);
    let line_origin: [f64; 3] = std::array::from_fn(|index| {
        (first_distance * second_cross_direction[index]
            + second_distance * direction_cross_first[index])
            / denominator
    });
    let Some(axis) = normalized(cylinder.axis) else {
        return Vec::new();
    };
    let relative = std::array::from_fn(|index| line_origin[index] - cylinder.origin[index]);
    let relative_axial = dot(relative, axis);
    let direction_axial = dot(direction, axis);
    let radial = std::array::from_fn(|index| relative[index] - relative_axial * axis[index]);
    let radial_direction =
        std::array::from_fn(|index| direction[index] - direction_axial * axis[index]);
    let quadratic = dot(radial_direction, radial_direction);
    if quadratic <= 1e-18 {
        return Vec::new();
    }
    let linear = 2.0 * dot(radial, radial_direction);
    let constant = dot(radial, radial) - cylinder.radius * cylinder.radius;
    let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
    let scale = linear
        .abs()
        .max((4.0 * quadratic * constant).abs().sqrt())
        .max(1.0);
    if discriminant < -1e-12 * scale * scale {
        return Vec::new();
    }
    let root = discriminant.max(0.0).sqrt();
    let mut parameters = vec![(-linear - root) / (2.0 * quadratic)];
    if root > 1e-12 * scale {
        parameters.push((-linear + root) / (2.0 * quadratic));
    }
    parameters
        .into_iter()
        .map(|parameter| {
            std::array::from_fn(|index| line_origin[index] + parameter * direction[index])
        })
        .filter(|point: &[f64; 3]| point.iter().all(|value| value.is_finite()))
        .collect()
}

fn intersect_two_planes_with_sphere(
    first: PlaneEquation,
    second: PlaneEquation,
    sphere: SphereEquation,
) -> Vec<[f64; 3]> {
    let direction = cross(first.normal, second.normal);
    let denominator = dot(direction, direction);
    if denominator <= 1e-18 {
        return Vec::new();
    }
    let first_distance = dot(first.normal, first.origin);
    let second_distance = dot(second.normal, second.origin);
    let second_cross_direction = cross(second.normal, direction);
    let direction_cross_first = cross(direction, first.normal);
    let line_origin: [f64; 3] = std::array::from_fn(|index| {
        (first_distance * second_cross_direction[index]
            + second_distance * direction_cross_first[index])
            / denominator
    });
    let relative: [f64; 3] = std::array::from_fn(|index| line_origin[index] - sphere.center[index]);
    let quadratic = denominator;
    let linear = 2.0 * dot(relative, direction);
    let constant = dot(relative, relative) - sphere.radius * sphere.radius;
    let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
    let scale = linear
        .abs()
        .max((4.0 * quadratic * constant).abs().sqrt())
        .max(1.0);
    if discriminant < -1e-12 * scale * scale {
        return Vec::new();
    }
    let root = discriminant.max(0.0).sqrt();
    let mut parameters = vec![(-linear - root) / (2.0 * quadratic)];
    if root > 1e-12 * scale {
        parameters.push((-linear + root) / (2.0 * quadratic));
    }
    parameters
        .into_iter()
        .map(|parameter| {
            std::array::from_fn(|index| line_origin[index] + parameter * direction[index])
        })
        .filter(|point: &[f64; 3]| point.iter().all(|value| value.is_finite()))
        .collect()
}

fn intersect_two_planes_with_cone(
    first: PlaneEquation,
    second: PlaneEquation,
    cone: ConeEquation,
) -> Vec<[f64; 3]> {
    let direction = cross(first.normal, second.normal);
    let denominator = dot(direction, direction);
    let Some(axis) = normalized(cone.axis) else {
        return Vec::new();
    };
    if denominator <= 1e-18 || !(0.0..std::f64::consts::FRAC_PI_2).contains(&cone.half_angle) {
        return Vec::new();
    }
    let first_distance = dot(first.normal, first.origin);
    let second_distance = dot(second.normal, second.origin);
    let second_cross_direction = cross(second.normal, direction);
    let direction_cross_first = cross(direction, first.normal);
    let line_origin: [f64; 3] = std::array::from_fn(|index| {
        (first_distance * second_cross_direction[index]
            + second_distance * direction_cross_first[index])
            / denominator
    });
    let relative = std::array::from_fn(|index| line_origin[index] - cone.origin[index]);
    let axial = dot(relative, axis);
    let axial_direction = dot(direction, axis);
    let radial = std::array::from_fn(|index| relative[index] - axial * axis[index]);
    let radial_direction =
        std::array::from_fn(|index| direction[index] - axial_direction * axis[index]);
    let slope = cone.half_angle.tan();
    let local_radius = cone.radius + axial * slope;
    let radius_rate = axial_direction * slope;
    let quadratic = dot(radial_direction, radial_direction) - radius_rate * radius_rate;
    let linear = 2.0 * (dot(radial, radial_direction) - local_radius * radius_rate);
    let constant = dot(radial, radial) - local_radius * local_radius;
    let scale = quadratic
        .abs()
        .max(linear.abs())
        .max(constant.abs())
        .max(1.0);
    let mut parameters = Vec::<f64>::new();
    if quadratic.abs() <= 1e-14 * scale {
        if linear.abs() <= 1e-14 * scale {
            return Vec::new();
        }
        parameters.push(-constant / linear);
    } else {
        let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
        if discriminant < -1e-12 * scale * scale {
            return Vec::new();
        }
        let root = discriminant.max(0.0).sqrt();
        parameters.push((-linear - root) / (2.0 * quadratic));
        if root > 1e-12 * scale {
            parameters.push((-linear + root) / (2.0 * quadratic));
        }
    }
    parameters
        .into_iter()
        .map(|parameter| {
            std::array::from_fn(|index| line_origin[index] + parameter * direction[index])
        })
        .filter(|point: &[f64; 3]| point.iter().all(|value| value.is_finite()))
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
    let parameter_delta = remaining.max(0.0).sqrt() / denominator.sqrt();
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
    let slope = cone.half_angle.tan();
    if slope <= 1e-12 || !slope.is_finite() || cone.radius < 0.0 {
        return None;
    }
    let alignment = dot(normal, axis);
    let u = normalized(std::array::from_fn(|index| {
        axis[index] - alignment * normal[index]
    }))?;
    let v = normalized(cross(normal, u))?;
    let relative: [f64; 3] = std::array::from_fn(|index| plane.origin[index] - cone.origin[index]);
    let axial = dot(relative, axis);
    let axis_u = dot(axis, u);
    let cone_factor = 1.0 + slope * slope;
    let quadratic_u = 1.0 - cone_factor * axis_u * axis_u;
    let quadratic_v = 1.0;
    let linear_u =
        2.0 * (dot(relative, u) - cone_factor * axial * axis_u - cone.radius * slope * axis_u);
    let linear_v = 2.0 * dot(relative, v);
    let constant = dot(relative, relative)
        - cone_factor * axial * axial
        - 2.0 * cone.radius * slope * axial
        - cone.radius * cone.radius;
    let coefficient_scale = quadratic_u
        .abs()
        .max(linear_u.abs())
        .max(linear_v.abs())
        .max(constant.abs())
        .max(1.0);
    let point = |u_parameter: f64, v_parameter: f64| {
        Point3::new(
            plane.origin[0] + u_parameter * u[0] + v_parameter * v[0],
            plane.origin[1] + u_parameter * u[1] + v_parameter * v[1],
            plane.origin[2] + u_parameter * u[2] + v_parameter * v[2],
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
        let direction = u.map(|value| value * opening.signum());
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
            (u, u_radius, v_radius)
        } else {
            (v, v_radius, u_radius)
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
            u,
            (shifted_constant / -quadratic_u).sqrt(),
            (shifted_constant / quadratic_v).sqrt(),
        )
    } else {
        (
            v,
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
            let Some(axis) = normalized(cone.axis) else {
                return false;
            };
            let relative = std::array::from_fn(|index| point[index] - cone.origin[index]);
            let axial = dot(relative, axis);
            let radial = std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let radius = cone.radius + axial * cone.half_angle.tan();
            (dot(radial, radial).sqrt() - radius.abs()).abs() <= 1e-7 * radius.abs().max(1.0)
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
                } else if let ([first, second], [cylinder]) =
                    (planes.as_slice(), cylinders.as_slice())
                {
                    candidates.extend(intersect_two_planes_with_cylinder(
                        *first, *second, *cylinder,
                    ));
                } else if let ([plane], [first, second]) = (planes.as_slice(), cylinders.as_slice())
                {
                    let Some((CurveGeometry::Line { origin, direction }, _)) =
                        carrier_intersection_curve(
                            CarrierEquation::Cylinder(*first),
                            CarrierEquation::Cylinder(*second),
                        )
                    else {
                        continue;
                    };
                    let line_origin = [origin.x, origin.y, origin.z];
                    let line_direction = [direction.x, direction.y, direction.z];
                    let denominator = dot(plane.normal, line_direction);
                    if denominator.abs() <= 1e-12 {
                        continue;
                    }
                    let parameter = (dot(plane.normal, plane.origin)
                        - dot(plane.normal, line_origin))
                        / denominator;
                    candidates.push(std::array::from_fn(|index| {
                        line_origin[index] + parameter * line_direction[index]
                    }));
                } else if let ([first, second], [], [sphere]) =
                    (planes.as_slice(), cylinders.as_slice(), spheres.as_slice())
                {
                    if cones.is_empty() && tori.is_empty() {
                        candidates
                            .extend(intersect_two_planes_with_sphere(*first, *second, *sphere));
                    }
                } else if let ([first, second], [cone]) = (planes.as_slice(), cones.as_slice()) {
                    if cylinders.is_empty() && spheres.is_empty() && tori.is_empty() {
                        candidates.extend(intersect_two_planes_with_cone(*first, *second, *cone));
                    }
                } else if let ([plane], [cylinder], [sphere]) =
                    (planes.as_slice(), cylinders.as_slice(), spheres.as_slice())
                {
                    if cones.is_empty() && tori.is_empty() {
                        if let Some((geometry, _)) = carrier_intersection_curve(
                            CarrierEquation::Cylinder(*cylinder),
                            CarrierEquation::Sphere(*sphere),
                        ) {
                            if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                candidates.extend(intersect_plane_with_circle(
                                    *plane, center, axis, radius,
                                ));
                            }
                        }
                    }
                } else if let ([plane], [first, second]) = (planes.as_slice(), spheres.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && tori.is_empty() {
                        if let Some(point) = tangent_sphere_point(*first, *second) {
                            candidates.push(point);
                        } else if let Some((geometry, _)) = carrier_intersection_curve(
                            CarrierEquation::Sphere(*first),
                            CarrierEquation::Sphere(*second),
                        ) {
                            if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                candidates.extend(intersect_plane_with_circle(
                                    *plane, center, axis, radius,
                                ));
                            }
                        }
                    }
                } else if let ([first, second], [torus]) = (planes.as_slice(), tori.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && spheres.is_empty() {
                        for (section_plane, cutting_plane) in [(*first, *second), (*second, *first)]
                        {
                            if let Some((geometry, _)) = carrier_intersection_curve(
                                CarrierEquation::Plane(section_plane),
                                CarrierEquation::Torus(*torus),
                            ) {
                                if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                    candidates.extend(intersect_plane_with_circle(
                                        cutting_plane,
                                        center,
                                        axis,
                                        radius,
                                    ));
                                }
                            }
                        }
                    }
                } else if let ([plane], [cylinder], [torus]) =
                    (planes.as_slice(), cylinders.as_slice(), tori.as_slice())
                {
                    if cones.is_empty() && spheres.is_empty() {
                        if let Some((geometry, _)) = carrier_intersection_curve(
                            CarrierEquation::Cylinder(*cylinder),
                            CarrierEquation::Torus(*torus),
                        ) {
                            if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                candidates.extend(intersect_plane_with_circle(
                                    *plane, center, axis, radius,
                                ));
                            }
                        }
                    }
                } else if let ([plane], [cone], [sphere]) =
                    (planes.as_slice(), cones.as_slice(), spheres.as_slice())
                {
                    if cylinders.is_empty() && tori.is_empty() {
                        if let Some((geometry, _)) = carrier_intersection_curve(
                            CarrierEquation::Cone(*cone),
                            CarrierEquation::Sphere(*sphere),
                        ) {
                            if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                candidates.extend(intersect_plane_with_circle(
                                    *plane, center, axis, radius,
                                ));
                            }
                        }
                    }
                } else if let ([plane], [sphere], [torus]) =
                    (planes.as_slice(), spheres.as_slice(), tori.as_slice())
                {
                    if cylinders.is_empty() && cones.is_empty() {
                        if let Some((geometry, _)) = carrier_intersection_curve(
                            CarrierEquation::Sphere(*sphere),
                            CarrierEquation::Torus(*torus),
                        ) {
                            if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                candidates.extend(intersect_plane_with_circle(
                                    *plane, center, axis, radius,
                                ));
                            }
                        }
                    }
                } else if let ([plane], [first, second]) = (planes.as_slice(), tori.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && spheres.is_empty() {
                        if let Some((geometry, _)) = carrier_intersection_curve(
                            CarrierEquation::Torus(*first),
                            CarrierEquation::Torus(*second),
                        ) {
                            if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                candidates.extend(intersect_plane_with_circle(
                                    *plane, center, axis, radius,
                                ));
                            }
                        }
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
    u_axis: Option<[f64; 3]>,
    offset: usize,
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
        .map(|candidate| {
            let normal = normalized(candidate.equation.normal)?;
            let u_axis = normalized(candidate.u_axis?)?;
            (dot(normal, u_axis).abs() <= 1e-9).then_some((
                normal,
                u_axis,
                candidate.offset,
                candidate.equation.origin,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let representative = charts.iter().min_by_key(|(_, _, offset, _)| *offset)?;
    charts
        .iter()
        .all(|(normal, u_axis, _, _)| {
            representative
                .0
                .iter()
                .zip(normal)
                .all(|(left, right)| (left - right).abs() <= 1e-9)
                && representative
                    .1
                    .iter()
                    .zip(u_axis)
                    .all(|(left, right)| (left - right).abs() <= 1e-9)
        })
        .then_some((
            PlaneEquation {
                origin: representative.3,
                normal: representative.0,
            },
            representative.1,
            representative.2,
        ))
}

#[cfg(test)]
mod plane_reconciliation_tests {
    use super::{agreed_plane, agreed_plane_surface, dot, PlaneCandidate, PlaneEquation};

    #[test]
    fn reconciles_equivalent_plane_frames_and_rejects_conflicts() {
        let first = PlaneEquation {
            origin: [1.0, 2.0, 3.0],
            normal: [0.0, 0.0, 2.0],
        };
        let equivalent = PlaneEquation {
            origin: [-4.0, 9.0, 3.0],
            normal: [0.0, 0.0, -1.0],
        };
        let agreed = agreed_plane(&[first, equivalent]).expect("equivalent planes agree");
        assert_eq!(agreed.normal, [0.0, 0.0, 1.0]);
        assert_eq!(dot(agreed.normal, agreed.origin), 3.0);

        let conflicting = PlaneEquation {
            origin: [0.0, 0.0, 4.0],
            normal: [0.0, 0.0, 1.0],
        };
        assert!(agreed_plane(&[first, conflicting]).is_none());
    }

    #[test]
    fn plane_surface_reconciliation_requires_one_chart_direction() {
        let plane = PlaneEquation {
            origin: [0.0, 0.0, 3.0],
            normal: [0.0, 0.0, 1.0],
        };
        let candidate = |u_axis, offset| PlaneCandidate {
            equation: plane,
            u_axis: Some(u_axis),
            offset,
        };
        assert!(agreed_plane_surface(&[
            candidate([1.0, 0.0, 0.0], 20),
            candidate([2.0, 0.0, 0.0], 10),
        ])
        .is_some_and(|(_, u_axis, offset)| u_axis == [1.0, 0.0, 0.0] && offset == 10));
        assert!(agreed_plane_surface(&[
            candidate([1.0, 0.0, 0.0], 10),
            candidate([0.0, 1.0, 0.0], 20),
        ])
        .is_none());
    }
}

fn plane_candidates(scan: &ContainerScan) -> BTreeMap<u32, Vec<PlaneCandidate>> {
    let mut candidates = BTreeMap::<u32, Vec<PlaneCandidate>>::new();
    for frame in &scan.plane_local_systems {
        let (Some(origin), Some(normal)) = (frame.origin, frame.normal) else {
            continue;
        };
        if !is_axis_aligned(normal) {
            candidates
                .entry(frame.surface_id)
                .or_default()
                .push(PlaneCandidate {
                    equation: PlaneEquation { origin, normal },
                    u_axis: frame.u_axis,
                    offset: frame.offset,
                });
        }
    }
    for outline in &scan.outline_planes {
        candidates
            .entry(outline.surface_id)
            .or_default()
            .push(PlaneCandidate {
                equation: PlaneEquation {
                    origin: outline.origin,
                    normal: outline.normal,
                },
                u_axis: Some(outline.u_axis),
                offset: outline.offset,
            });
    }
    candidates
        .into_iter()
        .filter(|(id, _)| {
            scan.surface_rows
                .iter()
                .filter(|row| row.id == *id)
                .take(2)
                .count()
                < 2
        })
        .collect()
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
    for row in crate::surface::uniquely_identified_rows(&scan.surface_rows) {
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        let Some(surface) = ir.model.surfaces.iter().find(|surface| surface.id == id) else {
            continue;
        };
        if let SurfaceGeometry::Cylinder {
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
            if (*ratio - 1.0).abs() <= 1e-12 {
                carriers.insert(
                    row.id,
                    CarrierEquation::Cone(ConeEquation {
                        origin: [origin.x, origin.y, origin.z],
                        axis: [axis.x, axis.y, axis.z],
                        ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                        radius: *radius,
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
    scan.sections
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
        .surface_rows
        .iter()
        .map(|row| row.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|id| {
            crate::surface::unique_surface_row(&scan.surface_rows, id).map(|row| (id, row.reversed))
        })
        .collect::<BTreeMap<_, _>>();
    let round_feature_ids = scan
        .feature_rows
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
        &scan.feature_entity_tables,
        &scan.surface_rows,
        &available_surfaces,
    ));
    orientations
}

fn solved_topological_vertices(
    scan: &ContainerScan,
    carriers: &BTreeMap<u32, CarrierEquation>,
) -> BTreeMap<u32, [f64; 3]> {
    let half_edges = scan
        .half_edges
        .iter()
        .map(|half_edge| (half_edge.id, half_edge))
        .collect::<BTreeMap<_, _>>();
    scan.topological_vertices
        .iter()
        .filter_map(|vertex| {
            let incident_carriers = vertex
                .half_edges
                .iter()
                .filter_map(|half_edge| half_edges.get(half_edge))
                .filter_map(|half_edge| carriers.get(&half_edge.face_id))
                .copied()
                .collect::<Vec<_>>();
            solve_carriers(&incident_carriers).map(|point| (vertex.id, point))
        })
        .collect()
}

fn transfer_plane_brep(scan: &ContainerScan, ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let planes = placed_planes(scan);
    let carriers = placed_carriers(scan, ir);
    let face_orientations = native_face_orientations(scan, ir);
    let half_edges = scan
        .half_edges
        .iter()
        .map(|half_edge| (half_edge.id, half_edge))
        .collect::<BTreeMap<_, _>>();
    let incidence = scan
        .half_edge_vertex_incidence
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let solved_vertices = solved_topological_vertices(scan, &carriers);
    let edge_vertices = scan
        .curve_topology_rows
        .iter()
        .filter_map(|row| {
            let forward = incidence.get(&HalfEdgeId {
                curve_id: row.id,
                side: 0,
            })?;
            let reverse = incidence.get(&HalfEdgeId {
                curve_id: row.id,
                side: 1,
            })?;
            let end = forward.end_vertex_id?;
            (reverse.start_vertex_id == end
                && reverse.end_vertex_id == Some(forward.start_vertex_id)
                && solved_vertices.contains_key(&forward.start_vertex_id)
                && solved_vertices.contains_key(&end))
            .then_some((row.id, [forward.start_vertex_id, end]))
        })
        .collect::<BTreeMap<_, _>>();
    let mut loops_by_face = BTreeMap::<u32, Vec<&crate::topology::Loop>>::new();
    for lp in &scan.loops {
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
    if eligible_faces.is_empty() {
        return;
    }
    let eligible_loops = eligible_faces
        .values()
        .flatten()
        .copied()
        .collect::<Vec<_>>();

    let emitted_half_edges = eligible_loops
        .iter()
        .flat_map(|lp| lp.half_edges.iter().copied())
        .collect::<BTreeSet<_>>();
    let emitted_curves = emitted_half_edges
        .iter()
        .map(|half_edge| half_edge.curve_id)
        .collect::<BTreeSet<_>>();
    let used_vertices = emitted_curves
        .iter()
        .filter_map(|curve| edge_vertices.get(curve))
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    let row_offsets = scan
        .curve_topology_rows
        .iter()
        .map(|row| (row.id, row.offset))
        .collect::<BTreeMap<_, _>>();

    for vertex_id in used_vertices {
        let point_id = PointId(format!("creo:visibgeom:point#{vertex_id}"));
        let vertex = VertexId(format!("creo:visibgeom:vertex#{vertex_id}"));
        if ir.model.vertices.iter().any(|item| item.id == vertex) {
            continue;
        }
        let position = solved_vertices[&vertex_id];
        annotate(
            annotations,
            &point_id,
            "VisibGeom",
            0,
            "plane_intersection_point",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &vertex,
            "VisibGeom",
            0,
            "topological_vertex_orbit",
            Exactness::Derived,
        );
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: Point3::new(position[0], position[1], position[2]),
        });
        ir.model.vertices.push(Vertex {
            id: vertex,
            point: point_id,
            tolerance: None,
        });
    }
    for curve_id in &emitted_curves {
        let [start, end] = edge_vertices[curve_id];
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
            curve: Some(CurveId(format!("creo:visibgeom:curve#{curve_id}"))),
            start: VertexId(format!("creo:visibgeom:vertex#{start}")),
            end: VertexId(format!("creo:visibgeom:vertex#{end}")),
            param_range: None,
            tolerance: None,
        });
        let curve = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
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
            let face_offset = crate::surface::unique_surface_row(&scan.surface_rows, *face_id)
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
            for (native_loop, loop_id) in native_loops.iter().zip(loop_ids) {
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
                    coedges: coedge_ids.clone(),
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
                        pcurve: None,
                    });
                }
            }
        }
    }
}

fn transfer_cap_pair_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for pair in &scan.fc05_cylinder_cap_pairs {
        let placed_caps = pair
            .cap_plane_ids
            .iter()
            .zip(&pair.curve_cap_ordinates_row_frame)
            .filter_map(|(id, ordinate)| {
                crate::surface::unique_outline_plane(&scan.outline_planes, *id)
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
                crate::surface::unique_outline_plane(&scan.outline_planes, *cap_plane_id)
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
                scan.fc05_circles
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
    let middle_norm = dot(middle, middle).sqrt();
    let third_norm = dot(third, third).sqrt();
    let second = if middle_norm <= 1e-10
        && third_norm > 1e-10
        && (first_norm - third_norm).abs() <= 1e-10 * first_norm.max(third_norm)
    {
        normalized(third)?
    } else if matches!(record.family, crate::surface::SurfacePrototypeFamily::Torus)
        && middle_norm > 1e-10
        && (first_norm - middle_norm).abs() <= 1e-10 * first_norm.max(middle_norm)
    {
        normalized(middle)?
    } else {
        return None;
    };
    let reference = normalized(first)?;
    if dot(reference, second).abs() > 1e-10 {
        return None;
    }
    let axis = normalized(cross(reference, second))?;
    let origin = slots[9..12].try_into().ok()?;
    Some((origin, axis, reference))
}

fn transfer_first_instance_prototype_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    if scan.layout != crate::container::Layout::Nd {
        return 0;
    }
    let mut associations = Vec::new();
    for record in &scan.surface_prototype_records {
        let row_kind = match record.family {
            crate::surface::SurfacePrototypeFamily::Plane => crate::surface::SurfaceKind::Plane,
            crate::surface::SurfacePrototypeFamily::Torus => {
                crate::surface::SurfaceKind::TorusOrSphere
            }
            crate::surface::SurfacePrototypeFamily::Spline => crate::surface::SurfaceKind::Spline,
            _ => continue,
        };
        let Some(section) = scan.sections.iter().find(|section| {
            record.offset >= section.offset
                && record.offset < section.offset.saturating_add(section.length)
        }) else {
            continue;
        };
        let adjacent_rows = scan.surface_rows.iter().filter(|row| {
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
        let candidates = [previous, following]
            .into_iter()
            .flatten()
            .filter(|row| row.kind == row_kind)
            .filter(|row| {
                crate::surface::unique_surface_row(&scan.surface_rows, row.id)
                    .is_some_and(|unique| unique.offset == row.offset)
            })
            .collect::<Vec<_>>();
        let [row] = candidates.as_slice() else {
            continue;
        };
        associations.push((record, *row, section));
    }
    let mut association_counts = BTreeMap::<usize, usize>::new();
    for (_, row, _) in &associations {
        *association_counts.entry(row.offset).or_default() += 1;
    }

    let mut transferred = 0;
    for (record, row, section) in associations {
        if association_counts.get(&row.offset) != Some(&1) {
            continue;
        }
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

fn transfer_positional_line_extrusion_planes(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let replay_bound_surfaces = scan
        .tabulated_cylinder_curve_replays
        .iter()
        .map(|replay| replay.surface_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for record in &scan.surface_parameters {
        if replay_bound_surfaces.contains(&record.surface_id) {
            continue;
        }
        if crate::surface::unique_surface_parameter(&scan.surface_parameters, record.surface_id)
            .is_none_or(|unique| unique.offset != record.offset)
        {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surface_rows, record.surface_id)
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
    for replay in &scan.tabulated_cylinder_curve_replays {
        *replay_counts.entry(replay.surface_id).or_default() += 1;
    }
    let mut transferred = 0;
    for replay in &scan.tabulated_cylinder_curve_replays {
        if replay_counts.get(&replay.surface_id) != Some(&1) {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surface_rows, replay.surface_id)
        else {
            continue;
        };
        if row.type_byte != 0x2c || row.offset != replay.surface_row_offset {
            continue;
        }
        let Some(parameters) =
            crate::surface::unique_surface_parameter(&scan.surface_parameters, replay.surface_id)
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
        });
        transferred += 1;
    }
    transferred
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
    for circle in &scan.fc05_circles {
        let topology = scan
            .curve_topology_rows
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
                crate::surface::unique_surface_row(&scan.surface_rows, *face)
                    .filter(|row| row.kind == crate::surface::SurfaceKind::Plane)?;
                crate::surface::unique_outline_plane(&scan.outline_planes, *face)
            })
            .collect::<Vec<_>>();
        let cylinders = topology
            .faces
            .iter()
            .filter(|face| {
                crate::surface::unique_surface_row(&scan.surface_rows, **face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
            })
            .copied()
            .collect::<Vec<_>>();
        let ([cap], [cylinder_id], Some(reference), Some(parameter_sign), Some(_)) = (
            cap_planes.as_slice(),
            cylinders.as_slice(),
            circle.reference_direction_row_frame,
            circle.parameter_sign,
            circle.cap_ordinate_row_frame,
        ) else {
            continue;
        };
        let Some(axis_index) = (0..3).find(|axis| cap.normal[*axis].abs() > 1.0 - 1e-9) else {
            continue;
        };
        let [first, second] = circle.center_row_frame;
        let axis_sign = -f64::from(parameter_sign);
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
            if slope.abs() > 1e-12 {
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
                return Some((
                    CurveGeometry::Circle {
                        center: Point3::new(center[0], center[1], center[2]),
                        axis: Vector3::new(normal[0], normal[1], normal[2]),
                        ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                        radius,
                    },
                    "plane_cone_circle",
                ));
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
    let slope = cone.half_angle.tan();
    if slope <= 1e-12 || cone.radius < 0.0 {
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
    let axial_normal = dot(axis, normal);
    let projected_length_squared = 1.0 - axial_normal * axial_normal;
    let cosine = cone.half_angle.cos();
    if projected_length_squared <= cosine * cosine + 1e-12 {
        return Vec::new();
    }
    let Some(projected_axis) = normalized(std::array::from_fn(|index| {
        axis[index] - axial_normal * normal[index]
    })) else {
        return Vec::new();
    };
    let Some(transverse) = normalized(cross(normal, projected_axis)) else {
        return Vec::new();
    };
    let along = cosine / projected_length_squared.sqrt();
    let across = (1.0 - along * along).sqrt();
    [-1.0, 1.0]
        .into_iter()
        .map(|sense| {
            let direction: [f64; 3] = std::array::from_fn(|index| {
                along * projected_axis[index] + sense * across * transverse[index]
            });
            (
                CurveGeometry::Line {
                    origin: Point3::new(apex[0], apex[1], apex[2]),
                    direction: Vector3::new(direction[0], direction[1], direction[2]),
                },
                "plane_cone_secant_generator",
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
        CurveGeometry::Circle {
            center,
            axis,
            radius,
            ..
        } => {
            let center = [center.x, center.y, center.z];
            let Some(axis) = normalized([axis.x, axis.y, axis.z]) else {
                return false;
            };
            points.into_iter().all(|point| {
                let relative: [f64; 3] = std::array::from_fn(|index| point[index] - center[index]);
                let scale = radius.abs().max(1.0);
                dot(relative, axis).abs() <= 1e-7 * scale
                    && (dot(relative, relative).sqrt() - radius).abs() <= 1e-7 * scale
            })
        }
        _ => false,
    }
}

fn select_parallel_plane_cylinder_generator(
    first: CarrierEquation,
    second: CarrierEquation,
    points: [[f64; 3]; 2],
) -> Option<(CurveGeometry, &'static str)> {
    select_unique_curve_candidate(
        parallel_plane_cylinder_generator_candidates(first, second),
        points,
    )
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

fn transfer_carrier_intersection_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    let carriers = placed_carriers(scan, ir);
    let solved_vertices = solved_topological_vertices(scan, &carriers);
    let incidence = scan
        .half_edge_vertex_incidence
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    for row in crate::topology::uniquely_identified_rows(&scan.curve_topology_rows) {
        let (Some(first), Some(second)) = (
            carriers.get(&row.faces[0]).copied(),
            carriers.get(&row.faces[1]).copied(),
        ) else {
            continue;
        };
        let resolved = carrier_intersection_curve(first, second).or_else(|| {
            let forward = incidence.get(&HalfEdgeId {
                curve_id: row.id,
                side: 0,
            })?;
            let reverse = incidence.get(&HalfEdgeId {
                curve_id: row.id,
                side: 1,
            })?;
            let end = forward.end_vertex_id?;
            if reverse.start_vertex_id != end
                || reverse.end_vertex_id != Some(forward.start_vertex_id)
            {
                return None;
            }
            let points = [
                *solved_vertices.get(&forward.start_vertex_id)?,
                *solved_vertices.get(&end)?,
            ];
            select_parallel_plane_cylinder_generator(first, second, points).or_else(|| {
                select_unique_curve_candidate(
                    apex_plane_cone_generator_candidates(first, second),
                    points,
                )
                .or_else(|| {
                    select_unique_curve_candidate(
                        coaxial_cone_sphere_circle_candidates(first, second),
                        points,
                    )
                })
                .or_else(|| {
                    select_unique_curve_candidate(
                        coaxial_cylinder_torus_circle_candidates(first, second),
                        points,
                    )
                })
                .or_else(|| {
                    select_unique_curve_candidate(
                        coaxial_sphere_torus_circle_candidates(first, second),
                        points,
                    )
                })
                .or_else(|| {
                    select_unique_curve_candidate(
                        coaxial_tori_circle_candidates(first, second),
                        points,
                    )
                })
                .or_else(|| {
                    select_unique_curve_candidate(
                        axis_normal_plane_torus_circle_candidates(first, second),
                        points,
                    )
                })
            })
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
            id,
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
    }
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
        .feature_rows
        .iter()
        .filter(|row| row.root_schema_class == Some(913))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in round_feature_ids {
        let named = agreed_feature_affected_ids(
            &scan.feature_affected_ids,
            feature_id,
            crate::feature::AffectedIdKind::Geometry,
        );
        let named_present = has_feature_affected_ids(
            &scan.feature_affected_ids,
            feature_id,
            crate::feature::AffectedIdKind::Geometry,
        );
        let replay =
            agreed_feature_replay_geometry_ids(&scan.feature_replay_affected_ids, feature_id);
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
            .surface_rows
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
        .feature_rows
        .iter()
        .filter(|row| row.root_schema_class == Some(913))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for (rowless_id, sibling_id, offset) in rowless_round_cylinder_pairs(
        &round_feature_ids,
        &scan.feature_entity_tables,
        &scan.surface_rows,
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
        .feature_rows
        .iter()
        .filter(|row| row.root_schema_class == Some(911))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in hole_feature_ids {
        let Some(hole) = simple_hole_geometry(scan, feature_id) else {
            continue;
        };
        for cylinder_id in hole.cylinder_ids {
            let row = crate::surface::unique_surface_row(&scan.surface_rows, cylinder_id)
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
                geometry: hole.geometry.clone(),
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

fn transfer_circular_sweep_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let sweep_feature_ids = scan
        .feature_rows
        .iter()
        .filter(|row| row.root_schema_class == Some(917))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in sweep_feature_ids {
        let Some(sweep) = circular_sweep_geometry(scan, feature_id) else {
            continue;
        };
        for cylinder_id in sweep.cylinder_ids {
            let row = crate::surface::unique_surface_row(&scan.surface_rows, cylinder_id)
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

fn transfer_single_cap_circular_sweep_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let feature_ids = scan
        .feature_rows
        .iter()
        .filter(|row| {
            feature_recipe(scan, row.feature_id) == Some(crate::feature::FeatureRecipeKind::Extrude)
        })
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in feature_ids {
        let Some((cylinder_id, geometry)) = single_cap_circular_sweep_geometry(scan, feature_id)
        else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        let row = crate::surface::unique_surface_row(&scan.surface_rows, cylinder_id)
            .expect("validated single-cap cylinder row");
        annotate(
            annotations,
            &id,
            "AllFeatur",
            row.offset as u64,
            "single_cap_circular_sweep_cylinder",
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
    transferred
}

fn transfer_cross_section_planes(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for frame in &scan.cross_section_plane_local_systems {
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
    for plane in &scan.cross_section_outline_planes {
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
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    if options.container_only {
        let ir = build_container_ir(&scan)?;
        let report = build_report(&scan, &ir, true);
        return Ok(DecodeResult::new(ir, report));
    }

    let ir = build_ir(&scan)?;
    let report = build_report(&scan, &ir, false);
    Ok(DecodeResult::new(ir, report))
}

fn preserve_passthrough_sections(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    for section in scan
        .sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY || section.role == role::THUMBNAIL)
    {
        let end = (section.offset + section.length).min(scan.data.len());
        let section_bytes = &scan.data[section.offset..end];
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
        ir.push_native_unknown(
            "creo",
            UnknownRecord {
                id,
                offset: offset as u64,
                byte_len: bytes.len() as u64,
                sha256: sha256_hex(bytes),
                data: Some(bytes.to_vec()),
                links: Vec::new(),
            },
        )?;
    }
    Ok(())
}

fn build_container_ir(scan: &ContainerScan) -> Result<CadIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    preserve_passthrough_sections(scan, &mut ir, &mut annotations)?;
    attach_expanded_sections(scan, &mut ir, &mut annotations)?;
    ir.annotations = annotations.build();
    Ok(ir)
}

/// Build source metadata, preserved geometry records, and transferred entities.
fn build_ir(scan: &ContainerScan) -> Result<CadIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    preserve_passthrough_sections(scan, &mut ir, &mut annotations)?;
    if !scan.reference_lines.is_empty() {
        let family = |kind| match kind {
            crate::reference::ReferenceLineKind::Line => "line",
            crate::reference::ReferenceLineKind::Line3d => "line3d",
        };
        let records = scan
            .reference_lines
            .iter()
            .map(|line| CreoReferenceLineRecord {
                id: format!(
                    "creo:mdl_ref_info:{}_record#{}",
                    family(line.kind),
                    line.offset
                ),
                family: family(line.kind),
                start: line.start,
                end: line.end,
                offset: line.offset,
            })
            .collect::<Vec<_>>();
        for record in &records {
            annotate(
                &mut annotations,
                &record.id,
                "MdlRefInfo",
                record.offset as u64,
                "reference_line_record",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("reference_lines", &records)?;
    }
    if !scan.reference_circles.is_empty() {
        let records = scan
            .reference_circles
            .iter()
            .map(|circle| CreoReferenceCircleRecord {
                id: format!("creo:mdl_ref_info:arc_z_record#{}", circle.offset),
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
        for record in &records {
            annotate(
                &mut annotations,
                &record.id,
                "MdlRefInfo",
                record.offset as u64,
                "reference_circle_record",
                Exactness::Derived,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("reference_circles", &records)?;
    }
    if !scan.reference_conics.is_empty() {
        let records = scan
            .reference_conics
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
        for record in &records {
            annotate(
                &mut annotations,
                &record.id,
                "MdlRefInfo",
                record.offset as u64,
                "reference_conic_record",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("reference_conics", &records)?;
    }
    if !scan.reference_ellipses.is_empty() {
        let records = scan
            .reference_ellipses
            .iter()
            .map(|ellipse| CreoReferenceEllipseRecord {
                id: format!("creo:mdl_ref_info:ellipse_carrier#{}", ellipse.offset),
                source_conic_id: format!("creo:mdl_ref_info:conic_record#{}", ellipse.offset),
                center: ellipse.center,
                axis: ellipse.axis,
                major_direction: ellipse.major_direction,
                major_radius: ellipse.major_radius,
                minor_radius: ellipse.minor_radius,
                offset: ellipse.offset,
            })
            .collect::<Vec<_>>();
        for record in &records {
            annotate(
                &mut annotations,
                &record.id,
                "MdlRefInfo",
                record.offset as u64,
                "reference_ellipse_carrier",
                Exactness::Derived,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("reference_ellipses", &records)?;
    }
    for line in &scan.reference_lines {
        let direction = std::array::from_fn(|axis| line.end[axis] - line.start[axis]);
        let Some(direction) = normalized(direction) else {
            continue;
        };
        let family = match line.kind {
            crate::reference::ReferenceLineKind::Line => "line",
            crate::reference::ReferenceLineKind::Line3d => "line3d",
        };
        let prefix = format!("creo:mdl_ref_info:{family}#{}", line.offset);
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
                object_id: format!("MdlRefInfo:{family}:{}", line.offset),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    for circle in &scan.reference_circles {
        let radial = std::array::from_fn(|axis| circle.start[axis] - circle.center[axis]);
        let Some(reference) = normalized(radial) else {
            continue;
        };
        let id = CurveId(format!("creo:mdl_ref_info:arc_z#{}", circle.offset));
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
                object_id: format!("MdlRefInfo:arc_z:{}", circle.offset),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    for ellipse in &scan.reference_ellipses {
        let id = CurveId(format!("creo:mdl_ref_info:conic#{}", ellipse.offset));
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
                object_id: format!("MdlRefInfo:conic:{}", ellipse.offset),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    for strip in &scan.primitive_triangle_strips {
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
    for plane in &scan.datum_planes {
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
            .outline_planes
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
    let positional_line_extrusion_plane_count =
        transfer_positional_line_extrusion_planes(scan, &mut ir, &mut annotations);
    let tabulated_cylinder_spline_extrusion_count =
        transfer_tabulated_cylinder_spline_extrusions(scan, &mut ir, &mut annotations);
    transfer_fc05_cap_circles(scan, &mut ir, &mut annotations);
    transfer_cap_pair_cylinders(scan, &mut ir, &mut annotations);
    let saved_spline_curve_count = transfer_saved_spline_curves(scan, &mut ir, &mut annotations);
    transfer_resolved_sketches(scan, &mut ir, &mut annotations);
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
    let single_cap_circular_sweep_cylinder_count =
        transfer_single_cap_circular_sweep_cylinders(scan, &mut ir, &mut annotations);
    let hole_cylinder_count = transfer_hole_cylinders(scan, &mut ir, &mut annotations);
    let constrained_slot_fillet_cylinder_count =
        transfer_constrained_slot_fillet_cylinders(scan, &mut ir, &mut annotations);
    let rowless_round_cylinder_count =
        transfer_rowless_round_cylinders(scan, &mut ir, &mut annotations);
    transfer_carrier_intersection_curves(scan, &mut ir, &mut annotations);
    transfer_plane_brep(scan, &mut ir, &mut annotations);
    let feature_revolution_brep_count =
        transfer_resolved_revolution_breps(scan, &mut ir, &mut annotations);
    let feature_circular_extrusion_brep_count =
        transfer_resolved_circular_extrusion_breps(scan, &mut ir, &mut annotations);
    let feature_extrusion_brep_count =
        transfer_resolved_extrusion_breps(scan, &mut ir, &mut annotations);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "transferred_cross_section_plane_count".to_string(),
            cross_section_plane_count.to_string(),
        );
        source.attributes.insert(
            "transferred_first_instance_prototype_surface_count".to_string(),
            first_instance_prototype_surface_count.to_string(),
        );
        source.attributes.insert(
            "transferred_positional_line_extrusion_plane_count".to_string(),
            positional_line_extrusion_plane_count.to_string(),
        );
        source.attributes.insert(
            "transferred_tabulated_cylinder_spline_extrusion_count".to_string(),
            tabulated_cylinder_spline_extrusion_count.to_string(),
        );
        source.attributes.insert(
            "transferred_saved_spline_curve_count".to_string(),
            saved_spline_curve_count.to_string(),
        );
        source.attributes.insert(
            "transferred_feature_revolution_surface_count".to_string(),
            feature_revolution_surface_count.to_string(),
        );
        source.attributes.insert(
            "transferred_feature_revolution_vertex_orbit_curve_count".to_string(),
            feature_revolution_vertex_orbit_curve_count.to_string(),
        );
        source.attributes.insert(
            "transferred_feature_extrusion_surface_count".to_string(),
            feature_extrusion_surface_count.to_string(),
        );
        source.attributes.insert(
            "transferred_feature_extrusion_vertex_orbit_curve_count".to_string(),
            feature_extrusion_vertex_orbit_curve_count.to_string(),
        );
        source.attributes.insert(
            "transferred_circular_sweep_cylinder_count".to_string(),
            circular_sweep_cylinder_count.to_string(),
        );
        source.attributes.insert(
            "transferred_single_cap_circular_sweep_cylinder_count".to_string(),
            single_cap_circular_sweep_cylinder_count.to_string(),
        );
        source.attributes.insert(
            "transferred_hole_cylinder_count".to_string(),
            hole_cylinder_count.to_string(),
        );
        source.attributes.insert(
            "transferred_constrained_slot_fillet_cylinder_count".to_string(),
            constrained_slot_fillet_cylinder_count.to_string(),
        );
        source.attributes.insert(
            "transferred_rowless_round_cylinder_count".to_string(),
            rowless_round_cylinder_count.to_string(),
        );
        source.attributes.insert(
            "transferred_feature_revolution_brep_count".to_string(),
            feature_revolution_brep_count.to_string(),
        );
        source.attributes.insert(
            "transferred_feature_circular_extrusion_brep_count".to_string(),
            feature_circular_extrusion_brep_count.to_string(),
        );
        source.attributes.insert(
            "transferred_feature_extrusion_brep_count".to_string(),
            feature_extrusion_brep_count.to_string(),
        );
    }
    for datum in &scan.datum_planes {
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
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: IrFeatureDefinition::DatumPlane {
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
            },
            native_ref: None,
        });
    }
    let operation_ordinal_base = ir.model.features.len();
    for (operation_index, operation) in scan.feature_operations.iter().enumerate() {
        let id = IrFeatureId(format!("creo:model:feature#{}", operation.feature_id));
        let current_operation =
            current_feature_operation(&scan.feature_operations, operation.feature_id);
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
                current_operation
                    .and_then(|operation| {
                        named_feature_definition(scan, &ir, operation.feature_id, &operation.kind)
                    })
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
        let dependencies = feature_dependencies(scan, &ir, operation.feature_id);
        let parent =
            agreed_feature_recipe_parent(&scan.feature_operation_states, operation.feature_id)
                .and_then(|parent_feature_id| {
                    let parent = IrFeatureId(format!("creo:model:feature#{parent_feature_id}"));
                    ir.model
                        .features
                        .iter()
                        .any(|feature| feature.id == parent)
                        .then_some(parent)
                });
        let operation_section = scan
            .sections
            .iter()
            .find(|section| {
                operation.offset >= section.offset
                    && operation.offset < section.offset.saturating_add(section.length)
            })
            .map_or("MdlStatus", |section| section.name.as_str());
        let name = current_operation.and_then(|operation| {
            operation
                .display_name_stored
                .then(|| format!("{} id {}", operation.kind, operation.feature_id))
        });
        let source_tag =
            agreed_feature_recipe(&scan.feature_operation_states, operation.feature_id)
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
            suppressed: false,
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
        .feature_rows
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
            .feature_rows
            .iter()
            .filter(|row| row.feature_id == feature_id)
            .map(|row| row.offset)
            .min()
        else {
            continue;
        };
        let kind = schema_class
            .and_then(schema_operation_kind)
            .unwrap_or("Native Feature");
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
            || IrFeatureDefinition::Native {
                kind: kind.to_string(),
                parameters: parameters.clone(),
                properties: BTreeMap::new(),
            },
            |schema_class| schema_feature_definition(scan, &ir, feature_id, schema_class, kind),
        );
        let row_schema_classes = row_feature_schema_classes(&scan.feature_rows, feature_id);
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
            name: Some(format!("{kind} id {feature_id}")),
            suppressed: false,
            parent: None,
            dependencies: feature_dependencies(scan, &ir, feature_id),
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
    reconcile_feature_links(scan, &mut ir);
    transfer_curve_expression_features(scan, &mut ir, &mut annotations);
    transfer_feature_dimensions(scan, &mut ir, &mut annotations);
    close_sketch_constraint_parameter_references(&mut ir);
    attach_expanded_sections(scan, &mut ir, &mut annotations)?;
    let surface_rows = surface_row_records(scan, &scan.surface_rows, "visibgeom");
    if !surface_rows.is_empty() {
        for record in &surface_rows {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "surface_namespace_row",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("surface_rows", &surface_rows)?;
    }
    let cross_section_surface_rows = surface_row_records(
        scan,
        &scan.cross_section_surface_rows,
        "cross_section_geometry",
    );
    if !cross_section_surface_rows.is_empty() {
        for record in &cross_section_surface_rows {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "cross_section_surface_namespace_row",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("cross_section_surface_rows", &cross_section_surface_rows)?;
    }
    let surface_prototypes = surface_prototype_records(scan);
    if !surface_prototypes.is_empty() {
        for record in &surface_prototypes {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "surface_prototype_record",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("surface_prototypes", &surface_prototypes)?;
    }
    let tabulated_cylinder_curve_replays = tabulated_cylinder_curve_replay_records(scan);
    if !tabulated_cylinder_curve_replays.is_empty() {
        for record in &tabulated_cylinder_curve_replays {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "tabulated_cylinder_curve_replay",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena(
            "tabulated_cylinder_curve_replays",
            &tabulated_cylinder_curve_replays,
        )?;
    }
    let curve_parameters = curve_parameter_records(scan);
    if !curve_parameters.is_empty() {
        for record in &curve_parameters {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "curve_parameter_record",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("curve_parameters", &curve_parameters)?;
    }
    let fc_curve_coordinates = fc_curve_coordinate_records(scan);
    if !fc_curve_coordinates.is_empty() {
        for record in &fc_curve_coordinates {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "fc_curve_coordinates",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("fc_curve_coordinates", &fc_curve_coordinates)?;
    }
    let fc05_circles = fc05_circle_records(scan);
    if !fc05_circles.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("fc05_circles", &fc05_circles)?;
    }
    let fc05_cylinder_cap_pairs = fc05_cylinder_cap_pair_records(scan);
    if !fc05_cylinder_cap_pairs.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("fc05_cylinder_cap_pairs", &fc05_cylinder_cap_pairs)?;
    }
    let prototype_pcurves = prototype_pcurve_records(scan);
    if !prototype_pcurves.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("prototype_pcurves", &prototype_pcurves)?;
    }
    let curve_prototype_topology = curve_prototype_topology_records(scan);
    if !curve_prototype_topology.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("curve_prototype_topology", &curve_prototype_topology)?;
    }
    let curve_prototypes =
        curve_prototype_records(scan, &scan.curve_prototypes, "creo:curve:prototype");
    if !curve_prototypes.is_empty() {
        for record in &curve_prototypes {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "curve_prototype",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("curve_prototypes", &curve_prototypes)?;
    }
    let cross_section_curve_prototypes = curve_prototype_records(
        scan,
        &scan.cross_section_curve_prototypes,
        "creo:cross_section_geometry:curve_prototype",
    );
    if !cross_section_curve_prototypes.is_empty() {
        for record in &cross_section_curve_prototypes {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "cross_section_curve_prototype",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena(
            "cross_section_curve_prototypes",
            &cross_section_curve_prototypes,
        )?;
    }
    let curve_topology_rows = curve_topology_row_records(scan);
    if !curve_topology_rows.is_empty() {
        for record in &curve_topology_rows {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "curve_topology_row",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("curve_topology_rows", &curve_topology_rows)?;
    }
    let cross_section_curve_rows = cross_section_curve_row_records(scan);
    if !cross_section_curve_rows.is_empty() {
        for record in &cross_section_curve_rows {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "cross_section_curve_row",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("cross_section_curve_rows", &cross_section_curve_rows)?;
    }
    let half_edges = half_edge_records(scan);
    if !half_edges.is_empty() {
        for record in &half_edges {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "native_half_edge",
                Exactness::Derived,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("half_edges", &half_edges)?;
    }
    let native_loops = loop_records(scan);
    if !native_loops.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("loops", &native_loops)?;
    }
    let topological_vertices = topological_vertex_records(scan);
    if !topological_vertices.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("topological_vertices", &topological_vertices)?;
    }
    let half_edge_vertex_incidence = half_edge_vertex_incidence_records(scan);
    if !half_edge_vertex_incidence.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("half_edge_vertex_incidence", &half_edge_vertex_incidence)?;
    }
    let face_components = face_component_records(scan);
    if !face_components.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("face_components", &face_components)?;
    }
    let surface_parameters = surface_parameter_records(
        scan,
        &scan.surface_rows,
        &scan.surface_parameters,
        "visibgeom",
    );
    if !surface_parameters.is_empty() {
        for record in &surface_parameters {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.body_offset as u64,
                "surface_parameter_frame",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("surface_parameters", &surface_parameters)?;
    }
    let cross_section_surface_parameters = surface_parameter_records(
        scan,
        &scan.cross_section_surface_rows,
        &scan.cross_section_surface_parameters,
        "cross_section_geometry",
    );
    if !cross_section_surface_parameters.is_empty() {
        for record in &cross_section_surface_parameters {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.body_offset as u64,
                "cross_section_surface_parameter_frame",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena(
            "cross_section_surface_parameters",
            &cross_section_surface_parameters,
        )?;
    }
    let plane_local_systems = plane_local_system_records(
        scan,
        &scan.plane_local_systems,
        "creo:surface:plane_local_system",
    );
    if !plane_local_systems.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("plane_local_systems", &plane_local_systems)?;
    }
    let cross_section_plane_local_systems = plane_local_system_records(
        scan,
        &scan.cross_section_plane_local_systems,
        "creo:cross_section_geometry:plane_local_system",
    );
    if !cross_section_plane_local_systems.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena(
            "cross_section_plane_local_systems",
            &cross_section_plane_local_systems,
        )?;
    }
    let plane_envelopes =
        plane_envelope_records(scan, &scan.plane_envelopes, "creo:surface:plane_envelope");
    if !plane_envelopes.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("plane_envelopes", &plane_envelopes)?;
    }
    let cross_section_plane_envelopes = plane_envelope_records(
        scan,
        &scan.cross_section_plane_envelopes,
        "creo:cross_section_geometry:plane_envelope",
    );
    if !cross_section_plane_envelopes.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena(
            "cross_section_plane_envelopes",
            &cross_section_plane_envelopes,
        )?;
    }
    let outline_planes =
        outline_plane_records(scan, &scan.outline_planes, "creo:surface:outline_plane");
    if !outline_planes.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("outline_planes", &outline_planes)?;
    }
    let cross_section_outline_planes = outline_plane_records(
        scan,
        &scan.cross_section_outline_planes,
        "creo:cross_section_geometry:outline_plane",
    );
    if !cross_section_outline_planes.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena(
            "cross_section_outline_planes",
            &cross_section_outline_planes,
        )?;
    }
    let datum_planes = datum_plane_records(scan);
    if !datum_planes.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("datum_planes", &datum_planes)?;
    }
    let feature_section_transforms = feature_section_transform_records(scan);
    if !feature_section_transforms.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_section_transforms", &feature_section_transforms)?;
    }
    let feature_placement_instructions = feature_placement_instruction_records(scan);
    if !feature_placement_instructions.is_empty() {
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena(
            "feature_placement_instructions",
            &feature_placement_instructions,
        )?;
    }
    let pcurve_endpoints = pcurve_endpoint_records(scan);
    if !pcurve_endpoints.is_empty() {
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
        let records = pcurve_endpoints
            .iter()
            .map(|(record, _)| record)
            .collect::<Vec<_>>();
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("pcurve_endpoints", &records)?;
    }
    let feature_definitions = feature_definition_records(scan);
    if !feature_definitions.is_empty() {
        for definition in &feature_definitions {
            annotate(
                &mut annotations,
                &definition.id,
                &definition.source_section,
                definition.offset as u64,
                "feature_definition_record",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_definitions", &feature_definitions)?;
    }
    let feature_entities = feature_entity_records(scan);
    if !feature_entities.is_empty() {
        for entity in &feature_entities {
            annotate(
                &mut annotations,
                &entity.id,
                "AllFeatur",
                entity.offset as u64,
                "feature_entity",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_entities", &feature_entities)?;
    }
    let feature_entity_references = feature_entity_reference_records(scan);
    if !feature_entity_references.is_empty() {
        for reference in &feature_entity_references {
            annotate(
                &mut annotations,
                &reference.id,
                "AllFeatur",
                reference.offset as u64,
                "feature_entity_reference",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_entity_references", &feature_entity_references)?;
    }
    let feature_entity_tables = feature_entity_table_records(scan);
    if !feature_entity_tables.is_empty() {
        for table in &feature_entity_tables {
            annotate(
                &mut annotations,
                &table.id,
                "AllFeatur",
                table.offset as u64,
                "feature_entity_table",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_entity_tables", &feature_entity_tables)?;
    }
    let feature_geometry_tables = feature_geometry_table_records(scan);
    if !feature_geometry_tables.is_empty() {
        for table in &feature_geometry_tables {
            annotate(
                &mut annotations,
                &table.id,
                &table.source_section,
                table.offset as u64,
                "feature_geometry_table",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_geometry_tables", &feature_geometry_tables)?;
    }
    let feature_affected_ids = feature_affected_id_records(scan);
    if !feature_affected_ids.is_empty() {
        for record in &feature_affected_ids {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_affected_ids",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_affected_ids", &feature_affected_ids)?;
    }
    let feature_replay_affected_ids = feature_replay_affected_id_records(scan);
    if !feature_replay_affected_ids.is_empty() {
        for record in &feature_replay_affected_ids {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_replay_affected_ids",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_replay_affected_ids", &feature_replay_affected_ids)?;
    }
    let feature_loop_restore_directions = feature_loop_restore_direction_records(scan);
    if !feature_loop_restore_directions.is_empty() {
        for record in &feature_loop_restore_directions {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_loop_restore_direction",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena(
            "feature_loop_restore_directions",
            &feature_loop_restore_directions,
        )?;
    }
    let feature_revolution_extents = feature_revolution_extent_records(scan);
    if !feature_revolution_extents.is_empty() {
        for record in &feature_revolution_extents {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_revolution_extent",
                Exactness::Derived,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_revolution_extents", &feature_revolution_extents)?;
    }
    let feature_rows = feature_row_records(scan);
    if !feature_rows.is_empty() {
        for record in &feature_rows {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_row",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_rows", &feature_rows)?;
    }
    let depdb_recipe_rows = depdb_recipe_row_records(scan);
    if !depdb_recipe_rows.is_empty() {
        for record in &depdb_recipe_rows {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "depdb_recipe_row",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("depdb_recipe_rows", &depdb_recipe_rows)?;
    }
    let feature_choices = feature_choice_records(scan);
    if !feature_choices.is_empty() {
        for record in &feature_choices {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_choice",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_choices", &feature_choices)?;
    }
    let feature_choice_fields = feature_choice_field_records(scan);
    if !feature_choice_fields.is_empty() {
        for record in &feature_choice_fields {
            annotate(
                &mut annotations,
                &record.id,
                &record.source_section,
                record.offset as u64,
                "feature_choice_field",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_choice_fields", &feature_choice_fields)?;
    }
    let sketches = sketch_records(scan);
    if !sketches.is_empty() {
        for sketch in &sketches {
            annotate(
                &mut annotations,
                &sketch.id,
                &sketch.source_section,
                sketch.offset as u64,
                "feature_sketch",
                Exactness::Derived,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("sketches", &sketches)?;
    }
    let curve_expressions = curve_expression_records(scan);
    if !curve_expressions.is_empty() {
        for (expression, source) in curve_expressions.iter().zip(&scan.curve_expressions) {
            annotate(
                &mut annotations,
                &expression.id,
                "DEPDB_DATA",
                source.expression_offset as u64,
                "curve_expression_program",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("curve_expressions", &curve_expressions)?;
    }
    let feature_operation_states = feature_operation_state_records(scan);
    if !feature_operation_states.is_empty() {
        for state in &feature_operation_states {
            let section = scan
                .sections
                .iter()
                .find(|section| {
                    state.state_offset >= section.offset
                        && state.state_offset < section.offset.saturating_add(section.length)
                })
                .map_or("MdlStatus", |section| section.name.as_str());
            annotate(
                &mut annotations,
                &state.id,
                section,
                state.state_offset as u64,
                "feature_operation_state",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("feature_operation_states", &feature_operation_states)?;
    }
    if let Some(family_table) = family_table_record(scan) {
        annotate(
            &mut annotations,
            family_table.id,
            "FamilyInf",
            family_table.offset as u64,
            "configuration_driver_table_pointer",
            Exactness::ByteExact,
        );
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("configuration", &[family_table])?;
    }
    ir.annotations = annotations.build();
    Ok(ir)
}

fn annotate(
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    source_stream: &str,
    offset: u64,
    tag: &str,
    exactness: Exactness,
) {
    let stream = annotations.stream(format!("creo:{source_stream}"));
    annotations.note(id.to_string(), stream, offset).tag(tag);
    annotations.exactness(id, exactness);
}

fn source_meta(scan: &ContainerScan) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert("version_line".to_string(), scan.version_line.clone());
    if let Some(name) = &scan.model_name {
        attributes.insert("model_name".to_string(), name.clone());
    }
    attributes.insert("layout".to_string(), scan.layout.token().to_string());
    attributes.insert("file_size".to_string(), scan.data.len().to_string());
    attributes.insert("section_count".to_string(), scan.sections.len().to_string());
    for (index, section) in scan.sections.iter().enumerate() {
        let prefix = format!("section.{index}");
        attributes.insert(format!("{prefix}.name"), section.name.clone());
        attributes.insert(format!("{prefix}.raw_name"), section.raw_name.clone());
        attributes.insert(format!("{prefix}.role"), section.role.to_string());
        attributes.insert(format!("{prefix}.offset"), section.offset.to_string());
        attributes.insert(format!("{prefix}.length"), section.length.to_string());
    }
    if let Some(c) = scan.census.srf_array_count {
        attributes.insert("srf_array_count".to_string(), c.to_string());
    }
    if let Some(c) = scan.census.crv_array_count {
        attributes.insert("crv_array_count".to_string(), c.to_string());
    }
    if let Some(unit) = &scan.principal_unit {
        attributes.insert("principal_unit".to_string(), unit.clone());
    }
    attributes.insert(
        "decoded_surface_row_count".to_string(),
        scan.surface_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_cross_section_surface_row_count".to_string(),
        scan.cross_section_surface_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_surface_parameter_record_count".to_string(),
        scan.surface_parameters.len().to_string(),
    );
    attributes.insert(
        "decoded_cross_section_surface_parameter_record_count".to_string(),
        scan.cross_section_surface_parameters.len().to_string(),
    );
    attributes.insert(
        "decoded_positional_extrusion_direction_count".to_string(),
        scan.surface_parameters
            .iter()
            .filter(|record| {
                crate::surface::unique_surface_row(&scan.surface_rows, record.surface_id)
                    .is_some_and(|row| {
                        row.kind == crate::surface::SurfaceKind::Extrusion
                            && record.extrusion_direction(row.type_byte).is_some()
                    })
            })
            .count()
            .to_string(),
    );
    attributes.insert(
        "decoded_plane_local_system_count".to_string(),
        scan.plane_local_systems.len().to_string(),
    );
    attributes.insert(
        "decoded_cross_section_plane_local_system_count".to_string(),
        scan.cross_section_plane_local_systems.len().to_string(),
    );
    attributes.insert(
        "decoded_plane_envelope_count".to_string(),
        scan.plane_envelopes.len().to_string(),
    );
    attributes.insert(
        "decoded_cross_section_plane_envelope_count".to_string(),
        scan.cross_section_plane_envelopes.len().to_string(),
    );
    attributes.insert(
        "decoded_outline_plane_count".to_string(),
        scan.outline_planes.len().to_string(),
    );
    attributes.insert(
        "decoded_cross_section_outline_plane_count".to_string(),
        scan.cross_section_outline_planes.len().to_string(),
    );
    attributes.insert(
        "decoded_surface_prototype_count".to_string(),
        scan.surface_prototypes.len().to_string(),
    );
    attributes.insert(
        "decoded_named_surface_prototype_count".to_string(),
        scan.surface_prototype_records.len().to_string(),
    );
    attributes.insert(
        "decoded_reference_line_count".to_string(),
        scan.reference_lines.len().to_string(),
    );
    attributes.insert(
        "decoded_reference_circle_count".to_string(),
        scan.reference_circles.len().to_string(),
    );
    attributes.insert(
        "decoded_reference_conic_count".to_string(),
        scan.reference_conics.len().to_string(),
    );
    attributes.insert(
        "transferred_reference_ellipse_count".to_string(),
        scan.reference_ellipses.len().to_string(),
    );
    attributes.insert(
        "decoded_tabulated_cylinder_curve_replay_count".to_string(),
        scan.tabulated_cylinder_curve_replays.len().to_string(),
    );
    attributes.insert(
        "decoded_tabulated_cylinder_control_point_set_count".to_string(),
        scan.tabulated_cylinder_curve_replays
            .iter()
            .filter(|replay| replay.control_points.iter().all(Option::is_some))
            .count()
            .to_string(),
    );
    attributes.insert(
        "decoded_curve_prototype_count".to_string(),
        scan.curve_prototypes.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_parameter_record_count".to_string(),
        scan.curve_parameters.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_expression_record_count".to_string(),
        scan.curve_expressions.len().to_string(),
    );
    attributes.insert(
        "expanded_section_count".to_string(),
        scan.expanded_sections.len().to_string(),
    );
    attributes.insert(
        "expanded_section_byte_count".to_string(),
        scan.expanded_sections
            .iter()
            .map(|section| section.data.len())
            .sum::<usize>()
            .to_string(),
    );
    if let Some(family_table) = scan.family_table {
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
    attributes.insert(
        "decoded_pcurve_count".to_string(),
        scan.pcurves.len().to_string(),
    );
    attributes.insert(
        "decoded_fc_curve_coordinate_record_count".to_string(),
        scan.fc_curve_coordinates.len().to_string(),
    );
    attributes.insert(
        "decoded_fc05_circle_count".to_string(),
        scan.fc05_circles.len().to_string(),
    );
    attributes.insert(
        "decoded_fc05_cylinder_cap_pair_count".to_string(),
        scan.fc05_cylinder_cap_pairs.len().to_string(),
    );
    attributes.insert(
        "decoded_prototype_pcurve_count".to_string(),
        scan.prototype_pcurves.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_prototype_topology_count".to_string(),
        scan.curve_prototype_topology.len().to_string(),
    );
    attributes.insert(
        "decoded_bound_prototype_pcurve_count".to_string(),
        scan.bound_prototype_pcurves.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_topology_row_count".to_string(),
        scan.curve_topology_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_cross_section_curve_row_count".to_string(),
        scan.cross_section_curve_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_cross_section_curve_prototype_count".to_string(),
        scan.cross_section_curve_prototypes.len().to_string(),
    );
    attributes.insert(
        "decoded_half_edge_count".to_string(),
        scan.half_edges.len().to_string(),
    );
    attributes.insert(
        "decoded_topological_vertex_count".to_string(),
        scan.topological_vertices.len().to_string(),
    );
    attributes.insert(
        "decoded_loop_count".to_string(),
        scan.loops.len().to_string(),
    );
    attributes.insert(
        "decoded_face_component_count".to_string(),
        scan.face_components.len().to_string(),
    );
    attributes.insert(
        "decoded_datum_plane_count".to_string(),
        scan.datum_planes.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_count".to_string(),
        scan.feature_ids.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_row_count".to_string(),
        scan.feature_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_choice_count".to_string(),
        scan.feature_choices.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_choice_field_count".to_string(),
        scan.feature_choice_fields.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_geometry_table_count".to_string(),
        scan.feature_geometry_tables.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_affected_id_array_count".to_string(),
        scan.feature_affected_ids.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_replay_affected_id_count".to_string(),
        scan.feature_replay_affected_ids.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_loop_restore_direction_count".to_string(),
        scan.feature_loop_restore_directions.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_revolution_extent_count".to_string(),
        scan.feature_revolution_extents.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_definition_count".to_string(),
        scan.feature_definitions.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_section_transform_count".to_string(),
        scan.feature_section_transforms.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_placement_instruction_count".to_string(),
        scan.feature_definitions
            .iter()
            .map(|definition| crate::feature::placement_instructions(definition).len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_operation_state_count".to_string(),
        scan.feature_operation_states.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_operation_count".to_string(),
        scan.feature_operations.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_outline_count".to_string(),
        scan.feature_definitions
            .iter()
            .map(|definition| definition.outlines.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_section_point_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .map(|variables| {
                let (points, ambiguous) = variables.reconciled_points();
                points.len() + ambiguous.len()
            })
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_segment_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_trim_entity_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.trim_entities.as_ref())
            .map(|entities| entities.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_trim_vertex_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.trim_vertices.as_ref())
            .map(|vertices| vertices.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_order_entry_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.order_table.as_ref())
            .map(|order| order.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_dimension_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.dimensions.as_ref())
            .map(|dimensions| dimensions.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_relation_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.relations.as_ref())
            .map(|relations| relations.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_saved_entity_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.saved_section.as_ref())
            .map(|saved| saved.entities.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_entity_count".to_string(),
        scan.feature_entities.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_entity_reference_count".to_string(),
        scan.feature_entity_references.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_entity_table_count".to_string(),
        scan.feature_entity_tables.len().to_string(),
    );
    if let Some(count) = scan.declared_body_count {
        attributes.insert("declared_body_count".to_string(), count.to_string());
    }
    if let Some(value) = scan.first_quilt_ptr {
        attributes.insert("first_quilt_ptr".to_string(), value.to_string());
    }
    SourceMeta {
        format: "creo".to_string(),
        attributes,
    }
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
fn build_report(scan: &ContainerScan, ir: &CadIr, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let geom_sections = scan
        .sections
        .iter()
        .filter(|s| s.role == role::GEOMETRY)
        .count();
    let mut placed_plane_ids = scan
        .plane_local_systems
        .iter()
        .filter(|frame| {
            frame.origin.is_some()
                && frame.u_axis.is_some()
                && frame.normal.is_some_and(|normal| !is_axis_aligned(normal))
        })
        .map(|frame| frame.surface_id)
        .collect::<BTreeSet<_>>();
    placed_plane_ids.extend(scan.outline_planes.iter().map(|plane| plane.surface_id));
    let placed_plane_count = placed_plane_ids.len();
    let first_instance_prototype_surface_count = ir
        .source
        .as_ref()
        .and_then(|source| {
            source
                .attributes
                .get("transferred_first_instance_prototype_surface_count")
        })
        .and_then(|count| count.parse::<usize>().ok())
        .unwrap_or(0);
    let positional_line_extrusion_plane_count = ir
        .source
        .as_ref()
        .and_then(|source| {
            source
                .attributes
                .get("transferred_positional_line_extrusion_plane_count")
        })
        .and_then(|count| count.parse::<usize>().ok())
        .unwrap_or(0);
    let tabulated_cylinder_spline_extrusion_count = ir
        .source
        .as_ref()
        .and_then(|source| {
            source
                .attributes
                .get("transferred_tabulated_cylinder_spline_extrusion_count")
        })
        .and_then(|count| count.parse::<usize>().ok())
        .unwrap_or(0);
    let mut losses = Vec::new();

    if container_only {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: "Container-only decode requested; entity transfer was skipped.".to_string(),
            provenance: None,
        });
    }

    // The namespace census: what is byte-backed and readable.
    let srf = scan
        .census
        .srf_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    let crv = scan
        .census
        .crv_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
            "PSB container decoded structurally: {} section(s), {} layout, VisibGeom namespace \
             census srf_array={srf} / crv_array={crv}; {} typed surface rows, {} labeled curve \
             prototypes, {} canonical curve-topology rows, and {} closed native loops were decoded. \
             Outline-backed planes, guarded non-axis support frames, complete ND first-instance \
             plane, torus, and interpolation-spline prototypes, unbound straight positional \
             surface-of-extrusion planes, \
             topology-bound `fc 05` \
             cylinders with a resolved axis-normal cap plane, four-entry two-cap and blind \
             circular-sweep cylinders, \
             and four-entry simple-hole cylinders with complete cap outlines transfer as carriers; \
             other parameter bodies remain structural records.",
            scan.sections.len(),
            scan.layout.token(),
            scan.surface_rows.len(),
            scan.curve_prototypes.len(),
            scan.curve_topology_rows.len(),
            scan.loops.len(),
        ),
        provenance: None,
    });

    // The core prototype-vs-instance limitation.
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: format!(
            "General model B-rep transfer remains incomplete. Exact planar components transfer \
             when every loop is solved from placed-plane intersections and one strict containment \
             outer boundary exists per face. Selected \
             cylinders transfer when an exact `fc 05` record and placed cap outline binds a row, \
             or a four-entry class-917 circular-sweep or class-911 simple-hole table with a complete \
             square cap outline establishes the complete axis placement, parameterization, and \
             radius. Later positional instances do not inherit prototype placement or scalar \
             defaults; they require their per-instance parameter bodies \
             ([spec §4.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#32-surface-prototypes)). {geom_sections} PSB geometry section(s) were preserved verbatim as unknown \
             records."
        ),
        provenance: None,
    });

    if !container_only && placed_plane_count != 0 {
        losses.push(LossNote {
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
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {first_instance_prototype_surface_count} first-instance ND plane, \
                 torus, or interpolation-spline carrier(s) from complete named parameters."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_line_extrusion_plane_count != 0 {
        losses.push(LossNote {
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
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {tabulated_cylinder_spline_extrusion_count} tabulated-cylinder \
                 cubic spline extrusion carrier(s) from uniquely matched directrix and frame spans."
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.datum_planes.is_empty() {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {} exact model-space construction datum plane carrier(s) from ActDatums; \
                 these are unbounded reference planes, not model B-rep faces.",
                scan.datum_planes.len()
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.reference_lines.is_empty() {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {} finite model-space reference line carrier(s) from MdlRefInfo; \
                 their byte-exact endpoints remain attached as native line records.",
                scan.reference_lines.len()
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.reference_circles.is_empty() {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {} circular reference carrier(s) from MdlRefInfo rows whose stored center, radius, and endpoints satisfy the circle equation; byte-exact endpoints remain attached as native circle records.",
                scan.reference_circles.len()
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.reference_ellipses.is_empty() {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {} elliptical reference carrier(s) from MdlRefInfo conic rows whose frame, coefficient radii, and antipodal endpoints satisfy one ellipse equation; the source conic records remain byte-exact native records.",
                scan.reference_ellipses.len()
            ),
            provenance: None,
        });
    }

    // The specific undecoded PSB layers that gate per-instance geometry.
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: "Additional model-space carriers are gated by unresolved lane-specific scalar \
                  prefixes, feature-local transform bindings, the `0x26` per-instance torus/sphere \
                  override region, and the round/fillet feature evaluator. These gaps prevent \
                  transfer of the remaining non-plane per-instance surfaces, curves, and vertices."
            .to_string(),
        provenance: None,
    });

    // Topology.
    losses.push(LossNote {
        category: LossCategory::Topology,
        severity: Severity::Blocking,
        message: "Native curve half-edges and closed loops were decoded. Exact plane-intersection \
                  components transfer as body/region/shell/face/loop/coedge/edge/vertex graphs; \
                  remaining components require face-instance partitioning, surface parameter \
                  bindings, curve geometry, or vertex coordinates."
            .to_string(),
        provenance: None,
    });

    if scan
        .family_table
        .is_some_and(|record| record.pointer == crate::container::FamilyTablePointer::Null)
    {
        losses.push(LossNote {
            category: LossCategory::Attribute,
            severity: Severity::Info,
            message: "FamilyInf declares a null configuration driver-table pointer; the part has \
                      no family-table configurations."
                .to_string(),
            provenance: None,
        });
    }

    let configuration_gap = match scan.family_table.map(|record| record.pointer) {
        Some(crate::container::FamilyTablePointer::Null) => "",
        Some(crate::container::FamilyTablePointer::Entity(_)) => {
            ", configuration driver-table rows"
        }
        None => ", configuration presence",
    };

    // Features, history, materials.
    losses.push(LossNote {
        category: LossCategory::Attribute,
        severity: Severity::Warning,
        message: format!(
            "Named feature operations and their decoded dependency/input tables transfer as typed \
             or native design records. Curve-equation assignments transfer with their source, \
             dependencies, and closed arithmetic values. Full neutral operation semantics\
             {configuration_gap}, remaining expression families, materials, and display data \
             remain untransferred."
        ),
        provenance: None,
    });

    DecodeReport {
        format: "creo".to_string(),
        container_only,
        geometry_transferred: has_transferred_geometry(ir),
        losses,
        notes: summary.notes,
    }
}
