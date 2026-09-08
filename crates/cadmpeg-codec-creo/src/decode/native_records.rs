// SPDX-License-Identifier: Apache-2.0
//! Native-arena nested record types moved from `decode.rs`.

use serde::Serialize;

use crate::feature::definitions::{DecodedField, DimensionValue, ReferencePlanes, ScalarLane};

#[derive(Serialize)]
pub(crate) struct CreoSketchSectionPoint {
    pub(crate) point_id: u32,
    #[serde(flatten, serialize_with = "serialize_section_point_state")]
    pub(crate) state: CreoSketchPointState,
}

/// Reconciled section-point coordinate state.
pub(crate) enum CreoSketchPointState {
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
pub(crate) struct CreoSketchTableHeader {
    #[serde(flatten, serialize_with = "serialize_sketch_table_kind")]
    pub(crate) kind: CreoSketchTableKind,
    pub(crate) row_count: usize,
    pub(crate) offset: usize,
}

/// Sketch table kind and its header fields.
pub(crate) enum CreoSketchTableKind {
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
pub(crate) struct CreoSketchBucketHeader {
    pub(crate) index: u32,
    pub(crate) declared_entry_count: u32,
    pub(crate) decoded_entry_count: u32,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchSection3d {
    pub(crate) sketch_plane_entity_id: Option<u32>,
    pub(crate) sketch_plane_flip: Option<bool>,
    #[serde(flatten, serialize_with = "serialize_reference_planes")]
    pub(crate) reference_planes: ReferencePlanes,
    pub(crate) reference_plane_datum_geometry_id: Option<u32>,
    pub(crate) orientation: CreoSketchSectionOrientation,
    pub(crate) dimension_ids: Vec<u32>,
    pub(crate) offset: usize,
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
pub(crate) struct CreoSketchReferencePlane {
    pub(crate) plane_entity_id: u32,
    pub(crate) reference_type: Option<u32>,
    pub(crate) external_reference_id: Option<u32>,
    pub(crate) segment_id: Option<u32>,
    pub(crate) sub_index: Option<u32>,
    pub(crate) reference_flip: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchSectionOrientation {
    pub(crate) section_flip: Option<bool>,
    pub(crate) reference_type: Option<u32>,
    pub(crate) segment_id: Option<u32>,
    pub(crate) reference_flip: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct CreoFeatureParameterFrame {
    pub(crate) kind: &'static str,
    pub(crate) body: Vec<u8>,
    pub(crate) decoded_values: Option<[f64; 12]>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoFeatureOutline {
    pub(crate) phase: &'static str,
    pub(crate) local_values: Vec<Option<f64>>,
    pub(crate) local_value_bodies: Vec<Vec<u8>>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchTrimEntity {
    pub(crate) external_id: u32,
    pub(crate) mode: Option<u32>,
    pub(crate) vertices: [u32; 2],
    pub(crate) center_vertex: Option<u32>,
    pub(crate) kind: &'static str,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchTrimVertex {
    pub(crate) vertex_id: u32,
    pub(crate) entities: Vec<u32>,
    pub(crate) section_coordinates: Option<[f64; 2]>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchOrderRow {
    pub(crate) external_id: u32,
    pub(crate) internal_id: u32,
    pub(crate) bitmask: u32,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum CreoSketchSavedEntity {
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
pub(crate) struct SplineTangents(pub(crate) Option<DecodedField<[[f64; 3]; 2]>>);

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
pub(crate) struct SplineParameters(pub(crate) Option<DecodedField<Vec<f64>>>);

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
pub(crate) struct CreoSketchVariable {
    pub(crate) variable_type: u32,
    pub(crate) key: u32,
    #[serde(flatten, serialize_with = "serialize_variable_value")]
    pub(crate) value: ScalarLane,
    pub(crate) value_body: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_value: Option<f64>,
    #[serde(flatten, serialize_with = "serialize_variable_guess")]
    pub(crate) guess: ScalarLane,
    pub(crate) guess_body: Vec<u8>,
    pub(crate) known: Option<u32>,
    pub(crate) homogeneity: Option<u32>,
    pub(crate) uvar_id: Option<u32>,
    pub(crate) offset: usize,
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
pub(crate) struct CreoSketchEquation {
    pub(crate) equation_id: u32,
    pub(crate) function_id: u32,
    pub(crate) explicit_argument_count: Option<u32>,
    pub(crate) arguments: Vec<Option<u32>>,
    pub(crate) arguments_body: Vec<u8>,
    pub(crate) auxiliary_body: Vec<u8>,
    pub(crate) body: Vec<u8>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchSegment {
    pub(crate) external_id: u32,
    pub(crate) kind: &'static str,
    pub(crate) point_ids: [u32; 2],
    pub(crate) center_id: Option<u32>,
    pub(crate) directions: [Option<u32>; 3],
    pub(crate) arc_orientation: Option<u32>,
    pub(crate) vertical_horizontal_constraint: Option<u32>,
    pub(crate) radius_dimension_id: Option<u32>,
    pub(crate) secondary_radius_dimension_id: Option<u32>,
    pub(crate) body: Vec<u8>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchCircleSegment {
    pub(crate) external_id: u32,
    pub(crate) center_id: u32,
    pub(crate) radius_dimension_id: u32,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchPointSegment {
    pub(crate) external_id: u32,
    pub(crate) point_id: u32,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchCenteredLineSegment {
    pub(crate) external_id: u32,
    pub(crate) center_id: u32,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchReferenceLineSegment {
    pub(crate) external_id: u32,
    pub(crate) point_ids: [Option<u32>; 2],
    pub(crate) directions: [Option<u32>; 3],
    pub(crate) vertical_horizontal_constraint: Option<u32>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchBoundedCurveSegment {
    pub(crate) external_id: u32,
    pub(crate) point_ids: [u32; 2],
    pub(crate) center_id: Option<u32>,
    pub(crate) directions: [Option<u32>; 3],
    pub(crate) arc_orientation: Option<u32>,
    pub(crate) vertical_horizontal_constraint: Option<u32>,
    pub(crate) radius_dimension_id: Option<u32>,
    pub(crate) secondary_radius_dimension_id: Option<u32>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchConicSegment {
    pub(crate) external_id: u32,
    pub(crate) center_id: u32,
    pub(crate) first_coefficient_ref: u32,
    pub(crate) second_coefficient_ref: u32,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchOpaqueSegment {
    pub(crate) external_id: u32,
    pub(crate) kind: u32,
    pub(crate) point_ids: [Option<u32>; 2],
    pub(crate) center_id: Option<u32>,
    pub(crate) directions: [Option<u32>; 3],
    pub(crate) arc_orientation: Option<u32>,
    pub(crate) vertical_horizontal_constraint: Option<u32>,
    pub(crate) radius_dimension_id: Option<u32>,
    pub(crate) secondary_radius_dimension_id: Option<u32>,
    pub(crate) body: Vec<u8>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchDimension {
    pub(crate) external_id: u32,
    pub(crate) dimension_type: u32,
    #[serde(flatten, serialize_with = "serialize_dimension_value")]
    pub(crate) value: DimensionValue,
    pub(crate) value_body: Vec<u8>,
    pub(crate) unit: &'static str,
    pub(crate) direction_byte: u8,
    pub(crate) auxiliary_value: Option<f64>,
    pub(crate) auxiliary_body: Vec<u8>,
    pub(crate) references: Option<CreoSketchDimensionReferenceTable>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchDimensionReferenceTable {
    pub(crate) declared_count: u32,
    pub(crate) entity_ref: Option<u32>,
    pub(crate) rows: Vec<CreoSketchDimensionReference>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchDimensionReference {
    pub(crate) item_id: Option<u32>,
    pub(crate) sense: Option<u32>,
    pub(crate) point: [Option<u32>; 2],
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchRelation {
    pub(crate) relation_id: u32,
    pub(crate) used: u32,
    pub(crate) operands: Vec<u8>,
    pub(crate) operand_vectors: Option<[[Option<u32>; 4]; 3]>,
    pub(crate) sign: u32,
    pub(crate) dimension_id: u32,
    pub(crate) relation_type: u32,
    pub(crate) body: Vec<u8>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchSkamp {
    pub(crate) id: u32,
    pub(crate) kind: u32,
    pub(crate) flags: u32,
    pub(crate) status: u32,
    pub(crate) items: Vec<CreoSketchSkampItem>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchSkampItem {
    pub(crate) entity_id: u32,
    pub(crate) sense: u32,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchRelationTriple {
    #[serde(rename = "relation_id")]
    pub(crate) relation: Option<u32>,
    #[serde(rename = "equation_id")]
    pub(crate) equation: Option<u32>,
    #[serde(rename = "skamp_id")]
    pub(crate) skamp: Option<u32>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoCurveExpressionLocalSystem {
    pub(crate) dimensions: u32,
    pub(crate) count: u32,
    pub(crate) body: Vec<u8>,
    pub(crate) explicit_slots: Option<[f64; 12]>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoCurveExpressionLine {
    pub(crate) text: String,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoCurveExpressionAssignment {
    pub(crate) target: crate::curve::CurveExpressionTarget,
    pub(crate) expression: String,
    pub(crate) dependencies: Vec<String>,
    pub(crate) value: Option<crate::curve::CurveExpressionValue>,
    pub(crate) activation: &'static str,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoCurveExpressionSolveBlock {
    pub(crate) equations: Vec<CreoCurveExpressionEquation>,
    pub(crate) assignments: Vec<CreoCurveExpressionAssignment>,
    pub(crate) variables: Vec<String>,
    pub(crate) solutions: Vec<Option<crate::curve::CurveExpressionValue>>,
    pub(crate) offset: usize,
    pub(crate) for_offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoCurveExpressionEquation {
    pub(crate) left: String,
    pub(crate) right: String,
    pub(crate) dependencies: Vec<String>,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoFeatureOperationState {
    pub(crate) id: String,
    pub(crate) feature_id: u32,
    pub(crate) state_ordinal: usize,
    pub(crate) current: bool,
    pub(crate) family: String,
    #[serde(flatten, serialize_with = "serialize_operation_name")]
    pub(crate) name: crate::feature::operations::OperationName,
    pub(crate) recipe: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recipe_conflict: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) display_state_conflict: Option<bool>,
    pub(crate) root_schema_class: Option<u32>,
    pub(crate) parent_feature_id: Option<u32>,
    pub(crate) offset: usize,
    pub(crate) state_offset: usize,
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
pub(crate) struct CreoFeatureSurfaceReplayAssociation {
    pub(crate) id: String,
    pub(crate) owner_feature_id: u32,
    pub(crate) visible_surface_id: u32,
    pub(crate) replay_surface_id: u32,
    pub(crate) replay_ordinal: usize,
    pub(crate) surface_family: String,
    pub(crate) table_offset: usize,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum CreoFeatureFieldValue {
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
pub(crate) struct CreoHalfEdgeRef {
    pub(crate) curve_id: u32,
    pub(crate) side: crate::topology::Side,
}

#[derive(Serialize)]
pub(crate) struct CreoFc05CircleRecord {
    pub(crate) id: String,
    pub(crate) curve_id: u32,
    pub(crate) center_row_frame: [f64; 2],
    pub(crate) radius_mm: f64,
    pub(crate) sample_direction_row_frame: [f64; 2],
    #[serde(flatten, serialize_with = "serialize_angle_parameter")]
    pub(crate) angle_parameter: crate::curve::Fc05AngleParameterRelation,
    pub(crate) cap_ordinate_row_frame: Option<f64>,
    pub(crate) point_count: usize,
    pub(crate) max_residual: f64,
    pub(crate) offset: usize,
    pub(crate) source_section: String,
}

#[derive(Serialize)]
pub(crate) struct CreoFc05CylinderCapPairRecord {
    pub(crate) id: String,
    pub(crate) surface_id: u32,
    #[serde(flatten, serialize_with = "serialize_cap_edges")]
    pub(crate) cap_edges: Vec<crate::curve::Fc05CapEdge>,
    pub(crate) center_row_frame: [f64; 2],
    pub(crate) radius_mm: f64,
    pub(crate) reference_direction_row_frame: [f64; 2],
    pub(crate) parameter_sign: i8,
    pub(crate) cap_ordinates_row_frame: Vec<f64>,
    pub(crate) offset: usize,
    pub(crate) source_section: String,
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
pub(crate) enum CreoPlaneEnvelope {
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
pub(crate) struct CreoTabulatedCylinderFrame {
    pub(crate) values: [f64; 6],
    pub(crate) prefixes: [u8; 6],
}

#[derive(Serialize)]
pub(crate) struct CreoPositionalCylinderFrame {
    pub(crate) origin: [f64; 3],
    pub(crate) axis: [f64; 3],
    pub(crate) ref_direction: [f64; 3],
    pub(crate) radius: f64,
    pub(crate) length: Option<f64>,
}

#[derive(Serialize)]
pub(crate) struct CreoPositionalConeFrame {
    pub(crate) apex: [f64; 3],
    pub(crate) axis: [f64; 3],
    pub(crate) ref_direction: [f64; 3],
    pub(crate) half_angle: f64,
}

#[derive(Serialize)]
pub(crate) struct CreoPositionalTorusFrame {
    pub(crate) center: [f64; 3],
    pub(crate) axis: [f64; 3],
    pub(crate) ref_direction: [f64; 3],
    pub(crate) major_radius: f64,
    pub(crate) minor_radius: f64,
}

#[derive(Serialize)]
pub(crate) struct CreoTorusOutlineFrame {
    pub(crate) values: [f64; 6],
    pub(crate) selector: u32,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoType26FiveCoordinateEnvelope {
    pub(crate) values: [f64; 5],
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoType26SplitCoordinateEnvelope {
    pub(crate) values: [f64; 4],
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoTorusRadiusOverrides {
    pub(crate) radius1: f64,
    pub(crate) radius2: f64,
    pub(crate) radius2_encoding: &'static str,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoConeHalfAngleOverride {
    pub(crate) radians: f64,
    pub(crate) offset: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoCurveParameterScalar {
    pub(crate) value: f64,
    pub(crate) raw: Vec<u8>,
    pub(crate) offset: usize,
    pub(crate) length: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoCurveParameterReference {
    pub(crate) entity_id: u32,
    pub(crate) offset: usize,
    pub(crate) length: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoCurveParameterOpaqueSpan {
    pub(crate) raw: Vec<u8>,
    pub(crate) offset: usize,
    pub(crate) length: usize,
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
