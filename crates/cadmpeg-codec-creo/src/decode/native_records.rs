// SPDX-License-Identifier: Apache-2.0
//! Native-arena nested record types moved from `decode.rs`.

use serde::Serialize;

use crate::feature::definitions::{DecodedField, DimensionValue, ReferencePlanes, ScalarLane};

#[derive(Serialize)]
pub(super) struct CreoSketchSectionPoint {
    pub(super) point_id: u32,
    #[serde(flatten, serialize_with = "serialize_section_point_state")]
    pub(super) state: CreoSketchPointState,
}

/// Reconciled section-point coordinate state.
pub(super) enum CreoSketchPointState {
    Conflicting,
    Resolved([f64; 2]),
    PartialU(f64),
    PartialV(f64),
    Unresolved,
}

fn serialize_section_point_state<S: serde::Serializer>(
    state: &CreoSketchPointState,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let (state, u, v) = match state {
        CreoSketchPointState::Conflicting => ("conflicting", None, None),
        CreoSketchPointState::Resolved([u, v]) => ("resolved", Some(*u), Some(*v)),
        CreoSketchPointState::PartialU(u) => ("partial", Some(*u), None),
        CreoSketchPointState::PartialV(v) => ("partial", None, Some(*v)),
        CreoSketchPointState::Unresolved => ("unresolved", None, None),
    };
    let mut map = serializer.serialize_map(Some(3))?;
    map.serialize_entry("state", state)?;
    map.serialize_entry("u", &u)?;
    map.serialize_entry("v", &v)?;
    map.end()
}

#[derive(Serialize)]
pub(super) struct CreoSketchTableHeader {
    #[serde(flatten, serialize_with = "serialize_sketch_table_kind")]
    pub(super) kind: CreoSketchTableKind,
    pub(super) row_count: usize,
    pub(super) offset: usize,
}

/// Sketch table kind and its header fields.
pub(super) enum CreoSketchTableKind {
    Variables {
        declared_count: u32,
        entity_ref: Option<u32>,
    },
    Equations {
        declared_count: u32,
        entity_ref: Option<u32>,
    },
    Segments {
        declared_count: u32,
        entity_ref: Option<u32>,
    },
    Order {
        declared_count: u32,
        entity_ref: Option<u32>,
    },
    Dimensions {
        declared_count: u32,
        entity_ref: Option<u32>,
    },
    Relations {
        declared_count: u32,
        entity_ref: Option<u32>,
    },
    SolverIncidences {
        declared_count: u32,
        entity_ref: u32,
    },
    RelationTriples {
        declared_count: u32,
        entity_ref: u32,
    },
    TrimEntities {
        declared_count: Option<u32>,
        entity_ref: Option<u32>,
        entry_ref: Option<u32>,
        buckets: Vec<CreoSketchBucketHeader>,
    },
    TrimVertices {
        declared_count: Option<u32>,
        entity_ref: Option<u32>,
        entry_ref: Option<u32>,
        buckets: Vec<CreoSketchBucketHeader>,
    },
    SavedEntities,
}

fn serialize_sketch_table_kind<S: serde::Serializer>(
    header: &CreoSketchTableKind,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let (kind, declared_count, entity_ref, entry_ref, buckets) = match header {
        CreoSketchTableKind::Variables {
            declared_count,
            entity_ref,
        } => (
            "variables",
            Some(*declared_count),
            *entity_ref,
            None,
            &[][..],
        ),
        CreoSketchTableKind::Equations {
            declared_count,
            entity_ref,
        } => (
            "equations",
            Some(*declared_count),
            *entity_ref,
            None,
            &[][..],
        ),
        CreoSketchTableKind::Segments {
            declared_count,
            entity_ref,
        } => (
            "segments",
            Some(*declared_count),
            *entity_ref,
            None,
            &[][..],
        ),
        CreoSketchTableKind::Order {
            declared_count,
            entity_ref,
        } => ("order", Some(*declared_count), *entity_ref, None, &[][..]),
        CreoSketchTableKind::Dimensions {
            declared_count,
            entity_ref,
        } => (
            "dimensions",
            Some(*declared_count),
            *entity_ref,
            None,
            &[][..],
        ),
        CreoSketchTableKind::Relations {
            declared_count,
            entity_ref,
        } => (
            "relations",
            Some(*declared_count),
            *entity_ref,
            None,
            &[][..],
        ),
        CreoSketchTableKind::SolverIncidences {
            declared_count,
            entity_ref,
        } => (
            "solver_incidences",
            Some(*declared_count),
            Some(*entity_ref),
            None,
            &[][..],
        ),
        CreoSketchTableKind::RelationTriples {
            declared_count,
            entity_ref,
        } => (
            "relation_triples",
            Some(*declared_count),
            Some(*entity_ref),
            None,
            &[][..],
        ),
        CreoSketchTableKind::TrimEntities {
            declared_count,
            entity_ref,
            entry_ref,
            buckets,
        } => (
            "trim_entities",
            *declared_count,
            *entity_ref,
            *entry_ref,
            buckets.as_slice(),
        ),
        CreoSketchTableKind::TrimVertices {
            declared_count,
            entity_ref,
            entry_ref,
            buckets,
        } => (
            "trim_vertices",
            *declared_count,
            *entity_ref,
            *entry_ref,
            buckets.as_slice(),
        ),
        CreoSketchTableKind::SavedEntities => ("saved_entities", None, None, None, &[][..]),
    };
    let mut map = serializer.serialize_map(Some(5))?;
    map.serialize_entry("kind", &kind)?;
    map.serialize_entry("declared_count", &declared_count)?;
    map.serialize_entry("entity_ref", &entity_ref)?;
    map.serialize_entry("entry_ref", &entry_ref)?;
    map.serialize_entry("buckets", &buckets)?;
    map.end()
}

#[derive(Serialize)]
pub(super) struct CreoSketchBucketHeader {
    /// Zero-based bucket index.
    pub(super) index: u32,
    /// Number of entries the bucket array opener declares.
    pub(super) declared_entry_count: u32,
    /// Number of structurally complete entries decoded within the bucket
    /// frame.
    ///
    /// `None` states that the scan decoded more entries than the `u32` the
    /// declared count is stored in can name, so the two counts cannot be
    /// compared and the bucket states no completeness. It is copied from
    /// `crate::feature::definitions::FeatureTrimBucket::decoded_entry_count`,
    /// whose one producer is `trim_bucket_entry_count`
    /// (`feature/definitions.rs:2745`): it counts decoded rows over the bucket
    /// frame and answers `None` from `u32::try_from` when that count passes
    /// the stored width. `FeatureTrimBucket::is_complete` is the reader that
    /// acts on it, and `None` is not complete.
    pub(super) decoded_entry_count: Option<u32>,
    /// Byte offset of the stored bucket index.
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchSection3d {
    pub(super) sketch_plane_entity_id: Option<u32>,
    pub(super) sketch_plane_flip: Option<bool>,
    #[serde(flatten, serialize_with = "serialize_reference_planes")]
    pub(super) reference_planes: ReferencePlanes,
    pub(super) reference_plane_datum_geometry_id: Option<u32>,
    pub(super) orientation: CreoSketchSectionOrientation,
    pub(super) dimension_ids: Vec<u32>,
    pub(super) offset: usize,
}

fn serialize_reference_planes<S: serde::Serializer>(
    planes: &ReferencePlanes,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let rows = match planes {
        ReferencePlanes::Named(_) => &[][..],
        ReferencePlanes::Positional(rows) => rows.as_slice(),
    }
    .iter()
    .map(|row| CreoSketchReferencePlane {
        plane_entity_id: row.plane_entity_id,
        reference_type: row.reference_type,
        external_reference_id: row.external_reference_id,
        segment_id: row.segment_id,
        sub_index: row.sub_index,
        reference_flip: row.reference_flip.map(super::sketch_ids::binary_flag_value),
    })
    .collect::<Vec<_>>();
    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry(
        "reference_plane_entity_ids",
        &planes.entity_ids().collect::<Vec<_>>(),
    )?;
    map.serialize_entry("reference_plane_rows", &rows)?;
    map.end()
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
pub(super) struct CreoSketchSectionOrientation {
    pub(super) section_flip: Option<bool>,
    pub(super) reference_type: Option<u32>,
    pub(super) segment_id: Option<u32>,
    pub(super) reference_flip: Option<bool>,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureParameterFrame {
    pub(super) kind: &'static str,
    pub(super) body: Vec<u8>,
    pub(super) decoded_values: Option<[f64; 12]>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureOutline {
    pub(super) phase: &'static str,
    pub(super) local_values: Vec<Option<f64>>,
    pub(super) local_value_bodies: Vec<Vec<u8>>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchTrimEntity {
    pub(super) external_id: u32,
    pub(super) mode: Option<u32>,
    pub(super) vertices: [u32; 2],
    pub(super) center_vertex: Option<u32>,
    pub(super) kind: &'static str,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchTrimVertex {
    pub(super) vertex_id: u32,
    pub(super) entities: Vec<u32>,
    pub(super) section_coordinates: Option<[f64; 2]>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchOrderRow {
    pub(super) external_id: u32,
    pub(super) internal_id: u32,
    pub(super) bitmask: u32,
    pub(super) offset: usize,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum CreoSketchSavedEntity {
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
        #[serde(flatten)]
        endpoint_tangents: SplineTangents,
        #[serde(flatten)]
        parameters: SplineParameters,
        offset: usize,
    },
    Dummy {
        entity_id: Option<u32>,
        body: Vec<u8>,
        offset: usize,
    },
}

fn serialize_spline_field<T: Serialize, S: serde::Serializer>(
    field: Option<&DecodedField<T>>,
    serializer: S,
    value_key: &'static str,
    body_key: &'static str,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry(value_key, &field.map(|field| &field.value))?;
    map.serialize_entry(body_key, &field.map(|field| &field.body))?;
    map.end()
}

/// Optional spline endpoint tangents flattened as their value and body keys.
pub(super) struct SplineTangents(pub(crate) Option<DecodedField<[[f64; 3]; 2]>>);

impl Serialize for SplineTangents {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_spline_field(
            self.0.as_ref(),
            serializer,
            "endpoint_tangents",
            "endpoint_tangents_body",
        )
    }
}

/// Optional spline parameters flattened as their value and body keys.
pub(super) struct SplineParameters(pub(crate) Option<DecodedField<Vec<f64>>>);

impl Serialize for SplineParameters {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_spline_field(self.0.as_ref(), serializer, "parameters", "parameters_body")
    }
}

fn serialize_dimension_value<S: serde::Serializer>(
    value: &DimensionValue,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("value", &value.resolved())?;
    if let Some(token) = value.unresolved_token() {
        map.serialize_entry("unresolved_value_token", token)?;
    }
    map.end()
}

#[derive(Serialize)]
pub(super) struct CreoSketchVariable {
    pub(super) variable_type: u32,
    pub(super) key: u32,
    #[serde(flatten, serialize_with = "serialize_variable_value")]
    pub(super) value: ScalarLane,
    pub(super) value_body: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) resolved_value: Option<f64>,
    #[serde(flatten, serialize_with = "serialize_variable_guess")]
    pub(super) guess: ScalarLane,
    pub(super) guess_body: Vec<u8>,
    pub(super) known: Option<u32>,
    pub(super) homogeneity: Option<u32>,
    pub(super) uvar_id: Option<u32>,
    pub(super) offset: usize,
}

fn serialize_variable_value<S: serde::Serializer>(
    value: &ScalarLane,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry("value", &value.value())?;
    map.serialize_entry("dimension_driven", &(value == &ScalarLane::DimensionDriven))?;
    map.end()
}

fn serialize_variable_guess<S: serde::Serializer>(
    value: &ScalarLane,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry("guess", &value.value())?;
    map.serialize_entry(
        "guess_dimension_driven",
        &(value == &ScalarLane::DimensionDriven),
    )?;
    map.end()
}

#[derive(Serialize)]
pub(super) struct CreoSketchEquation {
    pub(super) equation_id: u32,
    pub(super) function_id: u32,
    pub(super) explicit_argument_count: Option<u32>,
    pub(super) arguments: Vec<Option<u32>>,
    pub(super) arguments_body: Vec<u8>,
    pub(super) auxiliary_body: Vec<u8>,
    pub(super) body: Vec<u8>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchSegment {
    pub(super) external_id: u32,
    pub(super) kind: &'static str,
    pub(super) point_ids: [u32; 2],
    pub(super) center_id: Option<u32>,
    pub(super) directions: [Option<u32>; 3],
    pub(super) arc_orientation: Option<u32>,
    pub(super) vertical_horizontal_constraint: Option<u32>,
    pub(super) radius_dimension_id: Option<u32>,
    pub(super) secondary_radius_dimension_id: Option<u32>,
    pub(super) body: Vec<u8>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchCircleSegment {
    pub(super) external_id: u32,
    pub(super) center_id: u32,
    pub(super) radius_dimension_id: u32,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchPointSegment {
    pub(super) external_id: u32,
    pub(super) point_id: u32,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchCenteredLineSegment {
    pub(super) external_id: u32,
    pub(super) center_id: u32,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchReferenceLineSegment {
    pub(super) external_id: u32,
    pub(super) point_ids: [Option<u32>; 2],
    pub(super) directions: [Option<u32>; 3],
    pub(super) vertical_horizontal_constraint: Option<u32>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchBoundedCurveSegment {
    pub(super) external_id: u32,
    pub(super) point_ids: [u32; 2],
    pub(super) center_id: Option<u32>,
    pub(super) directions: [Option<u32>; 3],
    pub(super) arc_orientation: Option<u32>,
    pub(super) vertical_horizontal_constraint: Option<u32>,
    pub(super) radius_dimension_id: Option<u32>,
    pub(super) secondary_radius_dimension_id: Option<u32>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchConicSegment {
    pub(super) external_id: u32,
    pub(super) center_id: u32,
    pub(super) first_coefficient_ref: u32,
    pub(super) second_coefficient_ref: u32,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchOpaqueSegment {
    pub(super) external_id: u32,
    pub(super) kind: u32,
    pub(super) point_ids: [Option<u32>; 2],
    pub(super) center_id: Option<u32>,
    pub(super) directions: [Option<u32>; 3],
    pub(super) arc_orientation: Option<u32>,
    pub(super) vertical_horizontal_constraint: Option<u32>,
    pub(super) radius_dimension_id: Option<u32>,
    pub(super) secondary_radius_dimension_id: Option<u32>,
    pub(super) body: Vec<u8>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchDimension {
    pub(super) external_id: u32,
    pub(super) dimension_type: u32,
    #[serde(flatten, serialize_with = "serialize_dimension_value")]
    pub(super) value: DimensionValue,
    pub(super) value_body: Vec<u8>,
    pub(super) unit: &'static str,
    pub(super) direction_byte: u8,
    pub(super) auxiliary_value: Option<f64>,
    pub(super) auxiliary_body: Vec<u8>,
    pub(super) references: Option<CreoSketchDimensionReferenceTable>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchDimensionReferenceTable {
    pub(super) declared_count: u32,
    pub(super) entity_ref: Option<u32>,
    pub(super) rows: Vec<CreoSketchDimensionReference>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchDimensionReference {
    pub(super) item_id: Option<u32>,
    pub(super) sense: Option<u32>,
    pub(super) point: [Option<u32>; 2],
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchRelation {
    pub(super) relation_id: u32,
    pub(super) used: u32,
    pub(super) operands: Vec<u8>,
    pub(super) operand_vectors: Option<[[Option<u32>; 4]; 3]>,
    pub(super) sign: u32,
    pub(super) dimension_id: u32,
    pub(super) relation_type: u32,
    pub(super) body: Vec<u8>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchSkamp {
    pub(super) id: u32,
    pub(super) kind: u32,
    pub(super) flags: u32,
    pub(super) status: u32,
    pub(super) items: Vec<CreoSketchSkampItem>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoSketchSkampItem {
    pub(super) entity_id: u32,
    pub(super) sense: u32,
}

#[derive(Serialize)]
pub(super) struct CreoSketchRelationTriple {
    #[serde(rename = "relation_id")]
    pub(super) relation: Option<u32>,
    #[serde(rename = "equation_id")]
    pub(super) equation: Option<u32>,
    #[serde(rename = "skamp_id")]
    pub(super) skamp: Option<u32>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveExpressionLocalSystem {
    pub(super) dimensions: u32,
    pub(super) count: u32,
    pub(super) body: Vec<u8>,
    pub(super) explicit_slots: Option<[f64; 12]>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveExpressionLine {
    pub(super) text: String,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveExpressionAssignment {
    pub(super) target: crate::curve::CurveExpressionTarget,
    pub(super) expression: String,
    pub(super) dependencies: Vec<String>,
    pub(super) value: Option<crate::curve::CurveExpressionValue>,
    pub(super) activation: &'static str,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveExpressionSolveBlock {
    pub(super) equations: Vec<CreoCurveExpressionEquation>,
    pub(super) assignments: Vec<CreoCurveExpressionAssignment>,
    pub(super) variables: Vec<String>,
    pub(super) solutions: Vec<Option<crate::curve::CurveExpressionValue>>,
    pub(super) offset: usize,
    pub(super) for_offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveExpressionEquation {
    pub(super) left: String,
    pub(super) right: String,
    pub(super) dependencies: Vec<String>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureOperationState {
    pub(super) id: String,
    pub(super) feature_id: u32,
    pub(super) state_ordinal: usize,
    pub(super) current: bool,
    pub(super) family: String,
    #[serde(flatten, serialize_with = "serialize_operation_name")]
    pub(super) name: crate::feature::operations::OperationName,
    pub(super) recipe: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) recipe_conflict: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) display_state_conflict: Option<bool>,
    pub(super) root_schema_class: Option<u32>,
    pub(super) parent_feature_id: Option<u32>,
    pub(super) offset: usize,
    pub(super) state_offset: usize,
}

fn serialize_operation_name<S: serde::Serializer>(
    name: &crate::feature::operations::OperationName,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(5))?;
    map.serialize_entry("display_name_stored", &name.display_name_stored())?;
    map.serialize_entry("stored_name", &name.stored_name())?;
    map.serialize_entry("stored_name_bytes", &name.stored_name_bytes())?;
    map.serialize_entry("identifier_keyword", &name.identifier_keyword())?;
    map.serialize_entry(
        "stored_name_prefix",
        &name
            .stored_name_prefix()
            .map(|prefix| char::from(prefix).to_string()),
    )?;
    map.end()
}

#[derive(Serialize)]
pub(super) struct CreoFeatureSurfaceReplayAssociation {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) visible_surface_id: u32,
    pub(super) replay_surface_id: u32,
    pub(super) replay_ordinal: usize,
    pub(super) surface_family: String,
    pub(super) table_offset: usize,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum CreoFeatureFieldValue {
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
pub(super) struct CreoHalfEdgeRef {
    pub(super) curve_id: u32,
    pub(super) side: crate::topology::Side,
}

#[derive(Serialize)]
pub(super) struct CreoFc05CircleRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) center_row_frame: [f64; 2],
    pub(super) radius_mm: f64,
    pub(super) sample_direction_row_frame: [f64; 2],
    #[serde(flatten, serialize_with = "serialize_angle_parameter")]
    pub(super) angle_parameter: crate::curve::Fc05AngleParameterRelation,
    pub(super) cap_ordinate_row_frame: Option<f64>,
    pub(super) point_count: usize,
    pub(super) max_residual: f64,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFc05CylinderCapPairRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    #[serde(flatten, serialize_with = "serialize_cap_edges")]
    pub(super) cap_edges: Vec<crate::curve::Fc05CapEdge>,
    pub(super) center_row_frame: [f64; 2],
    pub(super) radius_mm: f64,
    pub(super) reference_direction_row_frame: [f64; 2],
    pub(super) parameter_sign: i8,
    pub(super) cap_ordinates_row_frame: Vec<f64>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

fn serialize_cap_edges<S: serde::Serializer>(
    edges: &[crate::curve::Fc05CapEdge],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(3))?;
    map.serialize_entry(
        "curve_ids",
        &edges.iter().map(|edge| edge.curve_id).collect::<Vec<_>>(),
    )?;
    map.serialize_entry(
        "cap_plane_ids",
        &edges
            .iter()
            .map(|edge| edge.cap_plane_id)
            .collect::<Vec<_>>(),
    )?;
    map.serialize_entry(
        "curve_cap_ordinates_row_frame",
        &edges
            .iter()
            .map(|edge| edge.cap_ordinate_row_frame)
            .collect::<Vec<_>>(),
    )?;
    map.end()
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum CreoPlaneEnvelope {
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
pub(super) struct CreoTabulatedCylinderFrame {
    pub(super) values: [f64; 6],
    pub(super) prefixes: [u8; 6],
}

#[derive(Serialize)]
pub(super) struct CreoPositionalCylinderFrame {
    pub(super) origin: [f64; 3],
    pub(super) axis: [f64; 3],
    pub(super) ref_direction: [f64; 3],
    pub(super) radius: f64,
    pub(super) length: Option<f64>,
}

#[derive(Serialize)]
pub(super) struct CreoPositionalConeFrame {
    pub(super) apex: [f64; 3],
    pub(super) axis: [f64; 3],
    pub(super) ref_direction: [f64; 3],
    pub(super) half_angle: f64,
}

#[derive(Serialize)]
pub(super) struct CreoPositionalTorusFrame {
    pub(super) center: [f64; 3],
    pub(super) axis: [f64; 3],
    pub(super) ref_direction: [f64; 3],
    pub(super) major_radius: f64,
    pub(super) minor_radius: f64,
}

#[derive(Serialize)]
pub(super) struct CreoTorusOutlineFrame {
    pub(super) values: [f64; 6],
    pub(super) selector: u32,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoType26FiveCoordinateEnvelope {
    pub(super) values: [f64; 5],
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoType26SplitCoordinateEnvelope {
    pub(super) values: [f64; 4],
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoTorusRadiusOverrides {
    pub(super) radius1: f64,
    pub(super) radius2: f64,
    pub(super) radius2_encoding: &'static str,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoConeHalfAngleOverride {
    pub(super) radians: f64,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveParameterScalar {
    pub(super) value: f64,
    pub(super) raw: Vec<u8>,
    pub(super) offset: usize,
    pub(super) length: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveParameterReference {
    pub(super) entity_id: u32,
    pub(super) offset: usize,
    pub(super) length: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveParameterOpaqueSpan {
    pub(super) raw: Vec<u8>,
    pub(super) offset: usize,
    pub(super) length: usize,
}

fn serialize_angle_parameter<S: serde::Serializer>(
    relation: &crate::curve::Fc05AngleParameterRelation,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use crate::curve::Fc05AngleParameterRelation;
    use serde::ser::SerializeMap;
    let (direction, sign) = match relation {
        Fc05AngleParameterRelation::Inconsistent => (None, None),
        Fc05AngleParameterRelation::Consistent {
            sense,
            reference_direction_row_frame,
        } => (Some(reference_direction_row_frame), Some(sense.as_i8())),
    };
    let mut map = serializer.serialize_map(Some(3))?;
    map.serialize_entry("reference_direction_row_frame", &direction)?;
    map.serialize_entry("parameter_sign", &sign)?;
    map.serialize_entry("angle_parameter_consistent", &sign.is_some())?;
    map.end()
}
