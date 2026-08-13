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

use cadmpeg_core::decode::{alloc_filled, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::features::{
    Angle, BodySelection, BooleanOp, ChamferSpec, DesignParameter, DimensionDisplay, EdgeSelection,
    ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection, Feature,
    FeatureDefinition as IrFeatureDefinition, FeatureId as IrFeatureId, FeatureResultTopology,
    FeatureSourceContent, FeatureTreeNodeRole, GeneratedEdgeRef, GeneratedFaceRef, HoleBottom,
    HoleForm, HoleKind, Length, ParameterId, ParameterValue, PathRef, PatternForm, PatternKind,
    ProfileRef, RadiusForm, RadiusSpec, RevolutionAxis, RevolutionConstruction, RevolveExtent,
    SurfaceBoundary, SurfaceContinuity, Termination, ThickenSide, VertexSelection,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, FeatureResultTopologyId, LoopId, OccurrenceId,
    PcurveId, PointId, ProceduralCurveId, ProceduralSurfaceId, ProductDefinitionId, RegionId,
    ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::report::{DecodeReport, LossNote, LossTaxonomy, Severity};
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

mod analytic;
mod build;
mod feature_history;
mod holes;
mod native;
mod records;
mod sketch;
mod sketch_transfer;
mod surfaces;
mod sweep;
#[allow(clippy::wildcard_imports)]
use analytic::*;
#[allow(clippy::wildcard_imports)]
use build::*;
#[allow(clippy::wildcard_imports)]
use feature_history::*;
#[allow(clippy::wildcard_imports)]
use holes::*;
use native::{annotate, emit_arena, emit_uniform, store_arena};
#[allow(clippy::wildcard_imports)]
use records::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use sketch::*;
#[allow(clippy::wildcard_imports)]
use sketch_transfer::*;
#[allow(clippy::wildcard_imports)]
use surfaces::*;
#[allow(clippy::wildcard_imports)]
use sweep::*;

/// The sole item of `iter`, or `None` when `iter` is empty or ambiguous.
fn exactly_one<T>(mut iter: impl Iterator<Item = T>) -> Option<T> {
    let first = iter.next()?;
    iter.next().is_none().then_some(first)
}

fn unique_owned_feature_definition(
    definitions: &[crate::feature::FeatureDefinition],
    feature_id: u32,
) -> Option<&crate::feature::FeatureDefinition> {
    exactly_one(
        definitions
            .iter()
            .filter(|definition| definition.owner_feature_id == Some(feature_id)),
    )
}

fn unique_feature_section_transform(
    transforms: &[crate::placement::FeatureSectionTransform],
    definition_id: u32,
    section_offset: usize,
) -> Option<&crate::placement::FeatureSectionTransform> {
    let transform = exactly_one(transforms.iter().filter(|transform| {
        transform.definition_id == definition_id && transform.offset == section_offset
    }))?;
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
    exactly_one(definitions.iter().filter(|definition| {
        definition.id == transform.definition_id
            && definition
                .section_3d
                .as_ref()
                .is_some_and(|section| section.offset == transform.offset)
    }))
}

fn unique_feature_profile_definition<'a>(
    definitions: &'a [crate::feature::FeatureDefinition],
    transforms: &[crate::placement::FeatureSectionTransform],
    feature_id: u32,
) -> Option<&'a crate::feature::FeatureDefinition> {
    let feature_transforms = transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    match feature_transforms.as_slice() {
        [transform] => unique_feature_definition_for_transform(definitions, transform),
        [] => unique_owned_feature_definition(definitions, feature_id),
        _ => None,
    }
}

fn unique_feature_profile_ref(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<ProfileRef> {
    unique_feature_profile_definition(
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    )
    .map(|definition| section_profile_ref(ir, feature_sketch_record_id_in_scan(scan, definition)))
}

fn unique_feature_datum_plane(
    datums: &[crate::datum::DatumPlane],
    feature_id: u32,
) -> Option<&crate::datum::DatumPlane> {
    exactly_one(datums.iter().filter(|datum| datum.feature_id == feature_id))
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
    reference_plane_rows: Vec<CreoSketchReferencePlane>,
    reference_plane_datum_geometry_id: Option<u32>,
    orientation: CreoSketchSectionOrientation,
    dimension_ids: Vec<u32>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchReferencePlane {
    plane_entity_id: u32,
    reference_type: Option<u32>,
    external_reference_id: Option<u32>,
    segment_id: Option<u32>,
    sub_index: Option<u32>,
    reference_flip: Option<bool>,
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
    local_value_bodies: Vec<Vec<u8>>,
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
        body: Vec<u8>,
        offset: usize,
    },
    Arc {
        entity_id: u32,
        center: [Option<f64>; 3],
        radius: Option<f64>,
        endpoints: [[Option<f64>; 3]; 2],
        parameters: [Option<f64>; 2],
        body: Vec<u8>,
        offset: usize,
    },
    Circle {
        entity_id: u32,
        center: [Option<f64>; 3],
        radius: Option<f64>,
        body: Vec<u8>,
        offset: usize,
    },
    Conic {
        entity_id: u32,
        endpoints: [[Option<f64>; 3]; 2],
        parameters: [Option<f64>; 2],
        coefficients: [Option<f64>; 2],
        local_system: Option<[f64; 12]>,
        body: Vec<u8>,
        offset: usize,
    },
    Spline {
        entity_id: Option<u32>,
        declared_point_count: Option<u32>,
        interpolation_points: Vec<[f64; 3]>,
        interpolation_points_body: Vec<u8>,
        endpoint_tangents: Option<[[f64; 3]; 2]>,
        endpoint_tangents_body: Option<Vec<u8>>,
        parameters: Option<Vec<f64>>,
        parameters_body: Option<Vec<u8>>,
        offset: usize,
    },
    Dummy {
        entity_id: Option<u32>,
        body: Vec<u8>,
        offset: usize,
    },
}

#[derive(Serialize)]
struct CreoSketchVariable {
    variable_type: u32,
    key: u32,
    value: Option<f64>,
    value_body: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_value: Option<f64>,
    guess: Option<f64>,
    guess_body: Vec<u8>,
    guess_dimension_driven: bool,
    known: Option<u32>,
    homogeneity: Option<u32>,
    uvar_id: Option<u32>,
    dimension_driven: bool,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchEquation {
    equation_id: u32,
    function_id: u32,
    explicit_argument_count: Option<u32>,
    arguments: Vec<Option<u32>>,
    arguments_body: Vec<u8>,
    auxiliary_body: Vec<u8>,
    body: Vec<u8>,
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
    body: Vec<u8>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchCircleSegment {
    external_id: u32,
    center_id: u32,
    radius_dimension_id: u32,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchPointSegment {
    external_id: u32,
    point_id: u32,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchCenteredLineSegment {
    external_id: u32,
    center_id: u32,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchReferenceLineSegment {
    external_id: u32,
    point_ids: [Option<u32>; 2],
    directions: [Option<u32>; 3],
    vertical_horizontal_constraint: Option<u32>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchBoundedCurveSegment {
    external_id: u32,
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
struct CreoSketchConicSegment {
    external_id: u32,
    center_id: u32,
    first_coefficient_ref: u32,
    second_coefficient_ref: u32,
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
    body: Vec<u8>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchDimension {
    external_id: u32,
    dimension_type: u32,
    value: Option<f64>,
    value_body: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unresolved_value_token: Option<Vec<u8>>,
    unit: &'static str,
    direction_byte: u8,
    auxiliary_value: Option<f64>,
    auxiliary_body: Vec<u8>,
    references: Option<CreoSketchDimensionReferenceTable>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchDimensionReferenceTable {
    declared_count: u32,
    entity_ref: Option<u32>,
    rows: Vec<CreoSketchDimensionReference>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoSketchDimensionReference {
    item_id: Option<u32>,
    sense: Option<u32>,
    point: [Option<u32>; 2],
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
    target: crate::curve::CurveExpressionTarget,
    expression: String,
    dependencies: Vec<String>,
    value: Option<crate::curve::CurveExpressionValue>,
    activation: &'static str,
    offset: usize,
}

#[derive(Serialize)]
struct CreoCurveExpressionSolveBlock {
    equations: Vec<CreoCurveExpressionEquation>,
    assignments: Vec<CreoCurveExpressionAssignment>,
    variables: Vec<String>,
    solutions: Vec<Option<crate::curve::CurveExpressionValue>>,
    offset: usize,
    for_offset: usize,
}

#[derive(Serialize)]
struct CreoCurveExpressionEquation {
    left: String,
    right: String,
    dependencies: Vec<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    recipe_conflict: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_state_conflict: Option<bool>,
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
    emit_uniform(
        ir,
        annotations,
        "expanded_sections",
        &records,
        |record| &record.id,
        |record| &record.name,
        |record| record.source_offset as u64,
        "unix_compress_expanded_section",
        Exactness::Derived,
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
    emit_uniform(
        ir,
        annotations,
        "double_xar_tables",
        &tables,
        |table| &table.id,
        |table| &table.section_name,
        |table| table.section_source_offset as u64,
        "model_scalar_dictionary",
        Exactness::ByteExact,
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
        crate::feature::AffectedIdKind::Quilts => "quilts",
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

const SURFACE_KINDS: [crate::surface::SurfaceKind; 7] = [
    crate::surface::SurfaceKind::Plane,
    crate::surface::SurfaceKind::Cylinder,
    crate::surface::SurfaceKind::Cone,
    crate::surface::SurfaceKind::TorusOrSphere,
    crate::surface::SurfaceKind::Spline,
    crate::surface::SurfaceKind::Fillet,
    crate::surface::SurfaceKind::Extrusion,
];

#[derive(Default)]
struct SurfaceTransferCoverage {
    unique_rows: usize,
    transferred_rows: usize,
    retained_unknown_rows: usize,
    ambiguous_rows: usize,
    by_family: BTreeMap<&'static str, (usize, usize)>,
    unknown_by_family: BTreeMap<&'static str, usize>,
}

#[derive(Default)]
struct CurveTransferCoverage {
    unique_rows: usize,
    transferred_rows: usize,
    retained_unknown_rows: usize,
    ambiguous_rows: usize,
    by_type: BTreeMap<u8, (usize, usize)>,
    unknown_by_type: BTreeMap<u8, usize>,
}

#[derive(Default)]
struct SketchSegmentTransferCoverage {
    decoded_rows: usize,
    resolved_geometry: usize,
    missing_rows: usize,
    by_family: BTreeMap<&'static str, (usize, usize)>,
}

#[derive(Default)]
struct DesignConstraintTransferCoverage {
    transferred: usize,
    native: usize,
    active: usize,
    active_native: usize,
    native_by_kind: BTreeMap<u32, usize>,
    active_native_by_kind: BTreeMap<u32, usize>,
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
                let native_kind_text = match &constraint.definition {
                    SketchConstraintDefinition::Native { native_kind, .. }
                        if native_kind.starts_with(native_kind_prefix) =>
                    {
                        Some(native_kind.as_str())
                    }
                    _ => None,
                };
                let native_kind = native_kind_text
                    .and_then(|kind| kind.strip_prefix(native_kind_prefix))
                    .and_then(|kind| kind.parse().ok());
                if native_kind_text.is_some() {
                    coverage.native += 1;
                }
                if let Some(native_kind) = native_kind {
                    *coverage.native_by_kind.entry(native_kind).or_default() += 1;
                    if constraint.active == Some(true) {
                        *coverage
                            .active_native_by_kind
                            .entry(native_kind)
                            .or_default() += 1;
                    }
                }
                if constraint.active == Some(true) {
                    coverage.active += 1;
                    if native_kind_text.is_some() {
                        coverage.active_native += 1;
                    }
                }
                coverage
            },
        )
}

fn constraint_kind_breakdown(coverage: &BTreeMap<String, usize>, prefix: &str) -> String {
    coverage
        .iter()
        .filter_map(|(key, count)| {
            let kind = key
                .strip_prefix(prefix)?
                .strip_suffix("_constraint_count")?;
            (*count != 0).then_some(format!("type {kind}={count}"))
        })
        .collect::<Vec<_>>()
        .join(", ")
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
    let unknown_ids = curves
        .iter()
        .filter(|curve| matches!(curve.geometry, CurveGeometry::Unknown { .. }))
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
        let retained_unknown = usize::from(unknown_ids.contains(&row.id));
        coverage.transferred_rows += transferred;
        coverage.retained_unknown_rows += retained_unknown;
        let type_coverage = coverage.by_type.entry(row.type_byte).or_default();
        type_coverage.0 += 1;
        type_coverage.1 += transferred;
        *coverage.unknown_by_type.entry(row.type_byte).or_default() += retained_unknown;
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
    let unknown_ids = surfaces
        .iter()
        .filter(|surface| matches!(surface.geometry, SurfaceGeometry::Unknown { .. }))
        .filter_map(|surface| {
            surface
                .source_object
                .as_ref()
                .filter(|source| source.format == "creo")?
                .object_id
                .strip_prefix("VisibGeom:")?
                .parse::<u32>()
                .ok()
        })
        .collect::<BTreeSet<_>>();
    let mut coverage = SurfaceTransferCoverage {
        unique_rows: unique_rows.len(),
        ambiguous_rows: rows.len().saturating_sub(unique_rows.len()),
        ..SurfaceTransferCoverage::default()
    };
    for kind in SURFACE_KINDS {
        coverage.by_family.insert(surface_family(kind), (0, 0));
        coverage.unknown_by_family.insert(surface_family(kind), 0);
    }
    for row in unique_rows {
        let is_transferred = transferred
            .iter()
            .any(|(id, kinds)| *id == row.id && kinds.contains(&row.kind));
        let retained_unknown = unknown_ids.contains(&row.id);
        coverage.transferred_rows += usize::from(is_transferred);
        coverage.retained_unknown_rows += usize::from(retained_unknown);
        let family = surface_family(row.kind);
        let family_coverage = coverage.by_family.entry(family).or_default();
        family_coverage.0 += 1;
        family_coverage.1 += usize::from(is_transferred);
        *coverage.unknown_by_family.entry(family).or_default() += usize::from(retained_unknown);
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
        crate::surface::SurfaceNamedValue::Empty => (
            "empty",
            Vec::new(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
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
) -> Vec<Option<String>> {
    let counts = assignments
        .iter()
        .fold(BTreeMap::new(), |mut counts, assignment| {
            if let Some((name, _)) = assignment.parameter_target() {
                *counts
                    .entry(crate::curve::expression_identifier_key(name))
                    .or_insert(0usize) += 1;
            }
            counts
        });
    let mut occurrences = BTreeMap::new();
    assignments
        .iter()
        .map(|assignment| {
            let (name, _) = assignment.parameter_target()?;
            let key = crate::curve::expression_identifier_key(name);
            if counts[&key] == 1 {
                return Some(name.to_owned());
            }
            let occurrence = occurrences.entry(key).or_insert(0usize);
            *occurrence += 1;
            Some(format!("{name}#{occurrence}"))
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
    let mut transferred_parameter_count = 0;
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
            let Some((name, _)) = assignment.parameter_target() else {
                continue;
            };
            assignment_indices_by_name
                .entry(crate::curve::expression_identifier_key(name))
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
        let mut emitted_assignment_indices = record
            .assignments
            .iter()
            .enumerate()
            .filter_map(|(index, assignment)| assignment.parameter_target().map(|_| index))
            .collect::<Vec<_>>();
        emitted_assignment_indices.sort_by_key(|index| parameter_ordinals[*index]);
        let emitted_ordinals = emitted_assignment_indices
            .into_iter()
            .enumerate()
            .map(|(ordinal, index)| (index, ordinal as u32))
            .collect::<BTreeMap<_, _>>();
        let mut source_content = Vec::with_capacity(emitted_ordinals.len());
        for (assignment_ordinal, assignment) in record.assignments.iter().enumerate() {
            let Some((assignment_name, declared_unit)) = assignment.parameter_target() else {
                continue;
            };
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
            if let Some(unit) = declared_unit {
                properties.insert("declared_unit".to_string(), unit.to_owned());
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
            let parameter_name = parameter_names[assignment_ordinal]
                .as_ref()
                .expect("emitted parameter assignment has a parameter name");
            if parameter_name != assignment_name {
                properties.insert("source_name".to_string(), assignment_name.to_owned());
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
                name: parameter_name.clone(),
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
            transferred_parameter_count += 1;
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
    transferred_parameter_count
}

fn feature_definition_has_sketch_design(definition: &crate::feature::FeatureDefinition) -> bool {
    definition.variables.is_some()
        || crate::feature::equation_table(&definition.body, 0, definition.body.len()).is_some()
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
    if let Some(table) = crate::feature::equation_table(&definition.body, 0, definition.body.len())
    {
        push(
            "equations",
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
            table.retained_row_count(),
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

#[cfg(test)]
mod tests;

#[cfg(test)]
mod plane_reconciliation_tests;

#[cfg(test)]
mod topological_vertex_tests;

#[cfg(test)]
mod native_edge_parameter_tests;

#[cfg(test)]
mod native_pcurve_tests;

#[cfg(test)]
mod prototype_local_frame_tests;

#[cfg(test)]
mod prototype_association_tests;

/// Decode a `.prt` stream into an IR document and loss report.
///
/// The stream is read from its beginning. When `options.container_only` is set,
/// the returned IR contains source metadata and preserved geometry sections but
/// no transferred entities.
pub fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<DecodeResult, CodecError> {
    let scan = container::scan_bytes(root.window());
    // Charge section cardinality before IR construction so max_entities can
    // refuse the build rather than only the finalizer.
    let mut admitted_entities = 0_u64;
    ctx.admit_entities(
        scan.framing.sections.len() as u64,
        &mut admitted_entities,
        "admit Creo sections",
    )?;

    let BuiltIr {
        mut ir,
        annotations,
        unknowns,
        coverage,
    } = if ctx.container_only() {
        build_container_ir(&scan)?
    } else {
        build_ir(ctx, &scan)?
    };
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        &mut admitted_entities,
        "admit Creo entities",
    )?;
    let report = build_report(&scan, &ir, coverage, ctx.container_only());
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations);
    source_fidelity.attach_native_unknown_records(&mut ir, "creo", unknowns)?;
    Ok(DecodeResult::new(ir, report, source_fidelity))
}
