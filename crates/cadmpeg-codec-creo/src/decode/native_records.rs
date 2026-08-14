// SPDX-License-Identifier: Apache-2.0
//! Native-arena nested record types moved from `decode.rs`.

use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct CreoSketchSectionPoint {
    pub(crate) point_id: u32,
    pub(crate) u: Option<f64>,
    pub(crate) v: Option<f64>,
    pub(crate) state: &'static str,
}

#[derive(Serialize)]
pub(crate) struct CreoSketchTableHeader {
    pub(crate) kind: &'static str,
    pub(crate) declared_count: Option<u32>,
    pub(crate) entity_ref: Option<u32>,
    pub(crate) entry_ref: Option<u32>,
    pub(crate) buckets: Vec<CreoSketchBucketHeader>,
    pub(crate) row_count: usize,
    pub(crate) offset: usize,
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
    pub(crate) reference_plane_entity_ids: Vec<u32>,
    pub(crate) reference_plane_rows: Vec<CreoSketchReferencePlane>,
    pub(crate) reference_plane_datum_geometry_id: Option<u32>,
    pub(crate) orientation: CreoSketchSectionOrientation,
    pub(crate) dimension_ids: Vec<u32>,
    pub(crate) offset: usize,
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
    pub(crate) decoded_values: Option<Vec<f64>>,
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
pub(crate) struct CreoSketchVariable {
    pub(crate) variable_type: u32,
    pub(crate) key: u32,
    pub(crate) value: Option<f64>,
    pub(crate) value_body: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_value: Option<f64>,
    pub(crate) guess: Option<f64>,
    pub(crate) guess_body: Vec<u8>,
    pub(crate) guess_dimension_driven: bool,
    pub(crate) known: Option<u32>,
    pub(crate) homogeneity: Option<u32>,
    pub(crate) uvar_id: Option<u32>,
    pub(crate) dimension_driven: bool,
    pub(crate) offset: usize,
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
    pub(crate) value: Option<f64>,
    pub(crate) value_body: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) unresolved_value_token: Option<Vec<u8>>,
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
    pub(crate) display_name_stored: bool,
    pub(crate) stored_name: Option<String>,
    pub(crate) stored_name_bytes: Option<Vec<u8>>,
    pub(crate) identifier_keyword: Option<String>,
    pub(crate) stored_name_prefix: Option<String>,
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
    pub(crate) side: u8,
}

#[derive(Serialize)]
pub(crate) struct CreoFcCurveCoordinateToken {
    pub(crate) value_mm: f64,
    pub(crate) raw: Vec<u8>,
    pub(crate) offset: usize,
    pub(crate) length: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoFcCurveOpaqueSpan {
    pub(crate) raw: Vec<u8>,
    pub(crate) offset: usize,
    pub(crate) length: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoFc05CircleRecord {
    pub(crate) id: String,
    pub(crate) curve_id: u32,
    pub(crate) center_row_frame: [f64; 2],
    pub(crate) radius_mm: f64,
    pub(crate) sample_direction_row_frame: [f64; 2],
    pub(crate) reference_direction_row_frame: Option<[f64; 2]>,
    pub(crate) parameter_sign: Option<i8>,
    pub(crate) cap_ordinate_row_frame: Option<f64>,
    pub(crate) point_count: usize,
    pub(crate) max_residual: f64,
    pub(crate) angle_parameter_consistent: bool,
    pub(crate) offset: usize,
    pub(crate) source_section: String,
}

#[derive(Serialize)]
pub(crate) struct CreoFc05CylinderCapPairRecord {
    pub(crate) id: String,
    pub(crate) surface_id: u32,
    pub(crate) curve_ids: Vec<u32>,
    pub(crate) cap_plane_ids: Vec<u32>,
    pub(crate) curve_cap_ordinates_row_frame: Vec<f64>,
    pub(crate) center_row_frame: [f64; 2],
    pub(crate) radius_mm: f64,
    pub(crate) reference_direction_row_frame: [f64; 2],
    pub(crate) parameter_sign: i8,
    pub(crate) cap_ordinates_row_frame: Vec<f64>,
    pub(crate) offset: usize,
    pub(crate) source_section: String,
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

#[derive(Serialize)]
pub(crate) struct CreoSurfaceParameterScalarFrame {
    pub(crate) offset: usize,
    pub(crate) slots: Vec<CreoSurfaceParameterSlot>,
}

#[derive(Serialize)]
pub(crate) struct CreoSurfaceParameterOpaqueSpan {
    pub(crate) raw: Vec<u8>,
    pub(crate) offset: usize,
    pub(crate) length: usize,
}

#[derive(Serialize)]
pub(crate) struct CreoSurfaceParameterSlot {
    pub(crate) value: Option<f64>,
    pub(crate) raw: Vec<u8>,
    pub(crate) offset: usize,
    pub(crate) length: usize,
}
