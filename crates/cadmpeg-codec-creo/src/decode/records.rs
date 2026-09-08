// SPDX-License-Identifier: Apache-2.0
//! Record shadow-layer structs and their `ContainerScan` mappers, moved
//! verbatim from `decode.rs`.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::hash::sha256_hex;
use serde::Serialize;

use crate::curve::{FcCurveCoordinateToken, FcCurveOpaqueSpan};
use crate::surface::{
    SurfaceParameterOpaqueSpan, SurfaceParameterScalar, SurfaceParameterScalarFrame,
};

pub(super) mod double_xar;

use crate::container::ContainerScan;
use crate::feature::definitions::{FeatureRelationTable, VariableType};
use crate::feature::schema::SchemaClass;

use super::coverage::{
    source_section, surface_family, surface_named_parameter_record, surface_prototype_family_name,
    surface_variant,
};
use super::curve_expressions::curve_expression_record_id;
use super::expanded::{affected_kind, extent_source, half_edge_ref};
use super::feature_history::replayed_torus_minor_radius;
use super::native_records::{
    CreoConeHalfAngleOverride, CreoCurveExpressionAssignment, CreoCurveExpressionEquation,
    CreoCurveExpressionLine, CreoCurveExpressionLocalSystem, CreoCurveExpressionSolveBlock,
    CreoCurveParameterOpaqueSpan, CreoCurveParameterReference, CreoCurveParameterScalar,
    CreoFeatureFieldValue, CreoFeatureOperationState, CreoFeatureOutline,
    CreoFeatureParameterFrame, CreoHalfEdgeRef, CreoPlaneEnvelope, CreoPositionalConeFrame,
    CreoPositionalCylinderFrame, CreoPositionalTorusFrame, CreoSketchBoundedCurveSegment,
    CreoSketchCenteredLineSegment, CreoSketchCircleSegment, CreoSketchConicSegment,
    CreoSketchDimension, CreoSketchDimensionReference, CreoSketchDimensionReferenceTable,
    CreoSketchEquation, CreoSketchOpaqueSegment, CreoSketchOrderRow, CreoSketchPointSegment,
    CreoSketchPointState, CreoSketchReferenceLineSegment, CreoSketchRelation,
    CreoSketchRelationTriple, CreoSketchSavedEntity, CreoSketchSection3d,
    CreoSketchSectionOrientation, CreoSketchSectionPoint, CreoSketchSegment, CreoSketchSkamp,
    CreoSketchSkampItem, CreoSketchTableHeader, CreoSketchTrimEntity, CreoSketchTrimVertex,
    CreoSketchVariable, CreoTabulatedCylinderFrame, CreoTorusOutlineFrame,
    CreoTorusRadiusOverrides, CreoType26FiveCoordinateEnvelope, CreoType26SplitCoordinateEnvelope,
};
use super::sketch::{
    resolved_section_coordinates, resolved_section_radii, resolved_section_scalar_values,
};
use super::sketch_ids::{
    binary_flag_value, feature_definition_has_sketch_design, feature_definition_record_id,
    feature_sketch_record_id_in_scan, sketch_table_headers,
};

#[derive(Serialize)]
pub(super) struct CreoSketchRecord {
    pub(super) id: String,
    pub(super) definition_id: u32,
    pub(super) owner_feature_id: Option<u32>,
    pub(super) source_section: String,
    pub(super) offset: usize,
    pub(super) section_3d: Option<CreoSketchSection3d>,
    pub(super) table_headers: Vec<CreoSketchTableHeader>,
    pub(super) section_points: Vec<CreoSketchSectionPoint>,
    pub(super) solved_external_ids: Vec<u32>,
    pub(super) variables: Vec<CreoSketchVariable>,
    pub(super) equations: Vec<CreoSketchEquation>,
    pub(super) segments: Vec<CreoSketchSegment>,
    pub(super) circle_segments: Vec<CreoSketchCircleSegment>,
    pub(super) point_segments: Vec<CreoSketchPointSegment>,
    pub(super) centered_line_segments: Vec<CreoSketchCenteredLineSegment>,
    pub(super) reference_line_segments: Vec<CreoSketchReferenceLineSegment>,
    pub(super) bounded_curve_segments: Vec<CreoSketchBoundedCurveSegment>,
    pub(super) conic_segments: Vec<CreoSketchConicSegment>,
    pub(super) opaque_segments: Vec<CreoSketchOpaqueSegment>,
    pub(super) trim_entities: Vec<CreoSketchTrimEntity>,
    pub(super) trim_vertices: Vec<CreoSketchTrimVertex>,
    pub(super) order_rows: Vec<CreoSketchOrderRow>,
    pub(super) saved_entities: Vec<CreoSketchSavedEntity>,
    pub(super) dimensions: Vec<CreoSketchDimension>,
    pub(super) relations: Vec<CreoSketchRelation>,
    pub(super) skamps: Vec<CreoSketchSkamp>,
    pub(super) relation_triples: Vec<CreoSketchRelationTriple>,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureDefinitionRecord {
    pub(super) id: String,
    pub(super) definition_id: u32,
    pub(super) owner_feature_id: Option<u32>,
    pub(super) source_section: String,
    pub(super) body: Vec<u8>,
    pub(super) parameter_frames: Vec<CreoFeatureParameterFrame>,
    pub(super) outlines: Vec<CreoFeatureOutline>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveExpressionRecord {
    pub(super) id: String,
    pub(super) entity_id: u32,
    pub(super) backup: bool,
    pub(super) local_system: Option<CreoCurveExpressionLocalSystem>,
    pub(super) lines: Vec<CreoCurveExpressionLine>,
    pub(super) assignments: Vec<CreoCurveExpressionAssignment>,
    pub(super) solve_blocks: Vec<CreoCurveExpressionSolveBlock>,
    pub(super) unresolved_solve_control: bool,
    pub(super) prohibited_constructs: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureReferenceNameRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) name: String,
    pub(super) name_bytes: Vec<u8>,
    pub(super) own_reference_id: u32,
    pub(super) reference_type: u32,
    pub(super) offset: usize,
}

pub(super) struct CreoFamilyTableRecord {
    pointer: crate::container::FamilyTablePointer,
    pub(super) offset: usize,
}

impl CreoFamilyTableRecord {
    /// The fixed driver-table record identity.
    pub(super) const ID: &'static str = "creo:family_info:driver_table#root";
}

impl Serialize for CreoFamilyTableRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut record = serializer.serialize_struct("CreoFamilyTableRecord", 4)?;
        record.serialize_field("id", Self::ID)?;
        let (kind, entity_id) = match self.pointer {
            crate::container::FamilyTablePointer::Null => ("null", None),
            crate::container::FamilyTablePointer::Entity(id) => ("entity_reference", Some(id)),
        };
        record.serialize_field("pointer_kind", kind)?;
        record.serialize_field("table_entity_id", &entity_id)?;
        record.serialize_field("offset", &self.offset)?;
        record.end()
    }
}

#[derive(Serialize)]
pub(super) struct CreoFeatureEntityRecord {
    pub(super) id: String,
    pub(super) entity_id: u32,
    pub(super) type_byte: u8,
    pub(super) name: String,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureEntityReferenceRecord {
    pub(super) id: String,
    pub(super) source_entity_id: Option<u32>,
    pub(super) target_entity_id: u32,
    pub(super) target_resolved: bool,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureEntityTableRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) table_class_id: u32,
    pub(super) entry_ids: Vec<u32>,
    pub(super) entries: Vec<CreoFeatureEntityTableEntryRecord>,
    pub(super) surface_ids: Vec<u32>,
    pub(super) non_surface_entity_ids: Vec<u32>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureEntityTableEntryRecord {
    pub(super) entity_id: u32,
    pub(super) class_id: u32,
    pub(super) source_entity_id: Option<u32>,
    pub(super) related_entity_id: Option<u32>,
    pub(super) related_entity_state: Option<u8>,
    pub(super) prefixed: bool,
    pub(super) offset: usize,
    pub(super) end_offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureGeometryTableRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    #[serde(flatten, serialize_with = "serialize_geometry_table_kind")]
    pub(super) kind: crate::feature::FeatureGeometryTableKind,
    pub(super) declared_count: u32,
    pub(super) entity_class_id: u32,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

fn serialize_geometry_table_kind<S: serde::Serializer>(
    kind: &crate::feature::FeatureGeometryTableKind,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use crate::feature::FeatureGeometryTableKind;
    use serde::ser::SerializeMap;
    let name = match kind {
        FeatureGeometryTableKind::EdgeIds => "edge_ids",
        FeatureGeometryTableKind::LoopIds => "loop_ids",
        FeatureGeometryTableKind::Boundaries => "boundaries",
        FeatureGeometryTableKind::UsedBodies => "used_bodies",
        FeatureGeometryTableKind::GeometryLists => "geometry_lists",
        FeatureGeometryTableKind::DatumIds(_) => "datum_ids",
    };
    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry("kind", name)?;
    map.serialize_entry("entry_ids", &kind.datum_ids())?;
    map.end()
}

#[derive(Serialize)]
pub(super) struct CreoFeatureLoopHistoryEntryRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) ordinal: u32,
    pub(super) loop_id: u32,
    pub(super) field_bytes: Vec<Vec<u8>>,
    #[serde(flatten, serialize_with = "serialize_loop_history_boundary")]
    pub(super) boundary: crate::feature::FeatureLoopHistoryBoundary,
    pub(super) offset: usize,
    pub(super) end_offset: usize,
    pub(super) source_section: String,
}

fn serialize_loop_history_boundary<S: serde::Serializer>(
    boundary: &crate::feature::FeatureLoopHistoryBoundary,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use crate::feature::FeatureLoopHistoryBoundary;
    use serde::ser::SerializeMap;
    let (boundary, reference) = match boundary {
        FeatureLoopHistoryBoundary::CompoundClose => ("compound_close", None),
        FeatureLoopHistoryBoundary::ReferenceContinue(reference) => {
            ("reference_continue", Some(*reference))
        }
        FeatureLoopHistoryBoundary::ReferenceFinal(reference) => {
            ("reference_final", Some(*reference))
        }
        FeatureLoopHistoryBoundary::NamedRecord { .. } => ("named_record", None),
    };
    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry("boundary", &boundary)?;
    map.serialize_entry("boundary_reference", &reference)?;
    map.end()
}

#[derive(Serialize)]
pub(super) struct CreoFeatureAffectedIdsRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) kind: &'static str,
    pub(super) ids: Vec<u32>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureReplayAffectedIdsRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) geometry_ids: Vec<u32>,
    pub(super) edge_ids: Vec<u32>,
    pub(super) geometry_extent: &'static str,
    pub(super) edge_extent: &'static str,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceMergeReplayAffectedIdsRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) geometry_ids: Vec<u32>,
    pub(super) edge_ids: Vec<u32>,
    pub(super) quilt_ids: Vec<u32>,
    pub(super) geometry_extent: &'static str,
    pub(super) edge_extent: &'static str,
    pub(super) quilt_extent: &'static str,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureLoopRestoreDirectionRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) lane: &'static str,
    pub(super) value: u32,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureRevolutionExtentRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) kind: &'static str,
    pub(super) angle_radians: f64,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureChoiceRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) label: String,
    pub(super) type_byte: Option<u8>,
    pub(super) payload: Vec<u8>,
    pub(super) payload_offset: usize,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureRowRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) header: [u8; 2],
    pub(super) root_schema_class: Option<u32>,
    pub(super) stream_offset: usize,
    pub(super) body: Vec<u8>,
    pub(super) body_offset: usize,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureChoiceFieldRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) choice_label: String,
    pub(super) name: String,
    pub(super) type_byte: u8,
    pub(super) value: CreoFeatureFieldValue,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoHalfEdgeRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) side: crate::topology::Side,
    pub(super) face_id: u32,
    pub(super) next: Option<CreoHalfEdgeRef>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoLoopRecord {
    pub(super) id: String,
    pub(super) face_id: u32,
    pub(super) half_edges: Vec<CreoHalfEdgeRef>,
}

#[derive(Serialize)]
pub(super) struct CreoLoopArrayFrameRecord {
    pub(super) id: String,
    pub(super) variant: Option<crate::loop_array::LayoutMarker>,
    pub(super) declared_count: u32,
    pub(super) class_id: u32,
    #[serde(flatten, serialize_with = "serialize_loop_array_rows")]
    pub(super) rows: crate::loop_array::LoopArrayFrameRows,
    pub(super) offset: usize,
    pub(super) prototype_end: usize,
    pub(super) end: usize,
    pub(super) source_section: String,
}

fn serialize_loop_array_rows<S: serde::Serializer>(
    rows: &crate::loop_array::LoopArrayFrameRows,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use crate::loop_array::LoopArrayFrameRows;
    use serde::ser::SerializeMap;
    let (count, overfull) = match rows {
        LoopArrayFrameRows::Materialized(count) => (*count, false),
        LoopArrayFrameRows::Overfull => (0, true),
    };
    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry("materialized_count", &count)?;
    map.serialize_entry("overfull", &overfull)?;
    map.end()
}

#[derive(Serialize)]
pub(super) struct CreoLoopArrayRecord {
    pub(super) id: String,
    pub(super) frame_offset: usize,
    pub(super) lo_id: u32,
    pub(super) lo_type: u32,
    pub(super) lo_subtype: u32,
    pub(super) feature_id: u32,
    pub(super) attributes: u8,
    pub(super) direction: u32,
    pub(super) next_lo_ptr: u32,
    pub(super) body: Vec<u8>,
    pub(super) offset: usize,
    pub(super) body_offset: usize,
    pub(super) end: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoTopologicalVertexRecord {
    pub(super) id: String,
    pub(super) vertex_id: u32,
    pub(super) half_edges: Vec<CreoHalfEdgeRef>,
}

#[derive(Serialize)]
pub(super) struct CreoHalfEdgeVertexIncidenceRecord {
    pub(super) id: String,
    pub(super) half_edge: CreoHalfEdgeRef,
    pub(super) start_vertex_id: u32,
    pub(super) end_vertex_id: Option<u32>,
}

#[derive(Serialize)]
pub(super) struct CreoFaceComponentRecord {
    pub(super) id: String,
    pub(super) face_ids: Vec<u32>,
    pub(super) curve_ids: Vec<u32>,
}

#[derive(Serialize)]
pub(super) struct CreoFaceAdmissionRejectionRecord {
    pub(super) id: String,
    pub(super) face_id: u32,
    pub(super) reason: &'static str,
    pub(super) boundary_half_edges: Vec<CreoHalfEdgeRef>,
    pub(super) vertex_ids: Vec<u32>,
}

#[derive(Serialize)]
pub(super) struct CreoExpandedSectionRecord {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) source_offset: usize,
    pub(super) compressed_length: usize,
    pub(super) expanded_length: usize,
    pub(super) sha256: String,
}

#[derive(Serialize)]
pub(super) struct CreoPrimitiveScalarArrayRecord {
    pub(super) id: String,
    pub(super) field: String,
    pub(super) expanded_offset: usize,
    pub(super) count: u32,
    pub(super) values: Vec<f64>,
}

#[derive(Debug, Serialize)]
pub(super) struct CreoReferenceLineRecord {
    pub(super) id: String,
    #[serde(flatten, serialize_with = "serialize_reference_line_kind")]
    pub(super) kind: crate::reference::ReferenceLineKind,
    pub(super) start: [f64; 3],
    pub(super) end: [f64; 3],
    pub(super) offset: usize,
}

fn serialize_reference_line_kind<S: serde::Serializer>(
    kind: &crate::reference::ReferenceLineKind,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use crate::reference::ReferenceLineKind;
    use serde::ser::SerializeMap;
    let (family, entity_id, original_length) = match kind {
        ReferenceLineKind::Line => ("line", None, None),
        ReferenceLineKind::Line3d {
            entity_id,
            original_length,
        } => ("line3d", Some(*entity_id), Some(*original_length)),
    };
    let mut map = serializer.serialize_map(Some(3))?;
    map.serialize_entry("family", &family)?;
    map.serialize_entry("entity_id", &entity_id)?;
    map.serialize_entry("original_length", &original_length)?;
    map.end()
}

#[derive(Serialize)]
pub(super) struct CreoReferenceCircleRecord {
    pub(super) id: String,
    pub(super) entity_id: u32,
    pub(super) center: [f64; 3],
    pub(super) center_source: &'static str,
    pub(super) radius: f64,
    pub(super) axis: [f64; 3],
    pub(super) endpoints: [[f64; 3]; 2],
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoReferenceConicRecord {
    pub(super) id: String,
    pub(super) entity_id: u32,
    pub(super) type_id: crate::reference::ConicType,
    pub(super) flip: u32,
    pub(super) endpoints: [[f64; 3]; 2],
    pub(super) parameter_interval: [Option<f64>; 2],
    pub(super) coefficients: [f64; 2],
    pub(super) local_system: Option<[f64; 12]>,
    pub(super) body: Vec<u8>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoReferenceEllipseRecord {
    pub(super) id: String,
    pub(super) source_conic_id: String,
    pub(super) source_entity_id: u32,
    pub(super) center: [f64; 3],
    pub(super) axis: [f64; 3],
    pub(super) major_direction: [f64; 3],
    pub(super) major_radius: f64,
    pub(super) minor_radius: f64,
    pub(super) offset: usize,
}

pub(super) fn reference_line_records(scan: &ContainerScan) -> Vec<CreoReferenceLineRecord> {
    let family = |kind: &crate::reference::ReferenceLineKind| match kind {
        crate::reference::ReferenceLineKind::Line => "line",
        crate::reference::ReferenceLineKind::Line3d { .. } => "line3d",
    };
    scan.references
        .lines
        .iter()
        .map(|line| CreoReferenceLineRecord {
            id: format!(
                "creo:mdl_ref_info:{}_record#{}",
                family(&line.kind),
                line.offset
            ),
            kind: line.kind.clone(),
            start: line.start,
            end: line.end,
            offset: line.offset,
        })
        .collect()
}

pub(super) fn reference_circle_records(scan: &ContainerScan) -> Vec<CreoReferenceCircleRecord> {
    scan.references
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
        .collect()
}

pub(super) fn reference_conic_records(scan: &ContainerScan) -> Vec<CreoReferenceConicRecord> {
    scan.references
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
        .collect()
}

pub(super) fn reference_ellipse_records(scan: &ContainerScan) -> Vec<CreoReferenceEllipseRecord> {
    scan.references
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
        .collect()
}

pub(super) fn expanded_section_records(scan: &ContainerScan) -> Vec<CreoExpandedSectionRecord> {
    scan.framing
        .expanded_sections
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

#[derive(Serialize)]
pub(super) struct CreoFcCurveCoordinateRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) subtype: u8,
    pub(super) body: Vec<u8>,
    pub(super) values_mm: Vec<f64>,
    pub(super) tokens: Vec<FcCurveCoordinateToken>,
    pub(super) opaque_spans: Vec<FcCurveOpaqueSpan>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoPrototypePcurveRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) face_0_endpoints: [[f64; 2]; 2],
    pub(super) face_1_endpoints: [[f64; 2]; 2],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoCurvePrototypeTopologyRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) faces: [u32; 2],
    pub(super) next_edges: [u32; 2],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoCurvePrototypeRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) type_byte: u8,
    pub(super) generating_feature_id: Option<u32>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoPlaneLocalSystemRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    pub(super) body: Vec<u8>,
    pub(super) slots: Vec<Option<f64>>,
    pub(super) origin: Option<[f64; 3]>,
    pub(super) u_axis: Option<[f64; 3]>,
    pub(super) normal: Option<[f64; 3]>,
    pub(super) classification: &'static str,
    pub(super) row_offset: usize,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoPlaneEnvelopeRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    pub(super) body: Vec<u8>,
    pub(super) envelope: CreoPlaneEnvelope,
    pub(super) corner_coordinate_equal: [Option<bool>; 3],
    pub(super) scalar_tokens: Vec<Vec<u8>>,
    pub(super) row_offset: usize,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoOutlinePlaneRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    pub(super) origin: [f64; 3],
    pub(super) normal: [f64; 3],
    pub(super) u_axis: [f64; 3],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoDatumPlaneRecord {
    pub(super) id: String,
    pub(super) datum_id: u32,
    pub(super) owner_feature_id: u32,
    pub(super) normal: [f64; 3],
    pub(super) plane_offset: f64,
    pub(super) corners: [[Option<f64>; 3]; 2],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoDatumCylinderRecord {
    pub(super) id: String,
    pub(super) datum_id: u32,
    pub(super) owner_feature_id: u32,
    pub(super) reversed: bool,
    pub(super) origin: [f64; 3],
    pub(super) axis: [f64; 3],
    pub(super) ref_direction: [f64; 3],
    pub(super) radius: f64,
    pub(super) length: Option<f64>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureSectionTransformRecord {
    pub(super) id: String,
    pub(super) definition_id: u32,
    pub(super) owner_feature_id: Option<u32>,
    pub(super) origin: [f64; 3],
    pub(super) u_axis: [f64; 3],
    pub(super) v_axis: [f64; 3],
    pub(super) normal: [f64; 3],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeaturePlacementInstructionRecord {
    pub(super) id: String,
    pub(super) definition_id: u32,
    pub(super) owner_feature_id: Option<u32>,
    pub(super) instruction_type: u32,
    pub(super) zero_offset: bool,
    pub(super) dimension_id: Option<u32>,
    pub(super) reference_id: Option<u32>,
    pub(super) geometry1_id: Option<u32>,
    pub(super) geometry2_id: Option<u32>,
    pub(super) member1: u32,
    pub(super) member2: u32,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

pub(super) fn feature_entity_records(scan: &ContainerScan) -> Vec<CreoFeatureEntityRecord> {
    scan.features
        .entities
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

pub(super) fn feature_entity_reference_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureEntityReferenceRecord> {
    scan.features
        .entity_references
        .iter()
        .map(|reference| CreoFeatureEntityReferenceRecord {
            id: format!("creo:allfeatur:entity_reference#{}", reference.offset),
            source_entity_id: reference.source_entity_id,
            target_entity_id: reference.target_entity_id,
            target_resolved: (reference.target_entity_id as usize) < scan.features.entities.len(),
            offset: reference.offset,
        })
        .collect()
}

pub(super) fn feature_entity_table_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureEntityTableRecord> {
    scan.features
        .entity_tables
        .iter()
        .map(|table| CreoFeatureEntityTableRecord {
            id: format!("creo:allfeatur:entity_table#{}", table.offset),
            owner_feature_id: table.feature_id,
            table_class_id: table.table_class_id,
            entry_ids: table.entry_ids(),
            entries: table
                .entries
                .iter()
                .map(|entry| CreoFeatureEntityTableEntryRecord {
                    entity_id: entry.entity_id,
                    class_id: entry.class_id,
                    source_entity_id: entry.source_entity_id(),
                    related_entity_id: entry.related_entity_id(),
                    related_entity_state: entry.related_entity_state(),
                    prefixed: entry.prefixed,
                    offset: entry.offset,
                    end_offset: entry.end_offset,
                })
                .collect(),
            surface_ids: table.surface_ids(),
            non_surface_entity_ids: table.non_surface_entity_ids(),
            offset: table.offset,
        })
        .collect()
}

pub(super) fn feature_geometry_table_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureGeometryTableRecord> {
    scan.features
        .geometry_tables
        .iter()
        .map(|table| CreoFeatureGeometryTableRecord {
            id: format!("creo:feature:geometry_table#{}", table.offset),
            owner_feature_id: table.feature_id,
            kind: table.kind.clone(),
            declared_count: table.count,
            entity_class_id: table.entity_class,
            offset: table.offset,
            source_section: source_section(scan, table.offset),
        })
        .collect()
}

pub(super) fn feature_loop_history_entry_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureLoopHistoryEntryRecord> {
    scan.features
        .loop_history_entries
        .iter()
        .map(|entry| CreoFeatureLoopHistoryEntryRecord {
            id: format!("creo:feature:loop_history_entry#{}", entry.offset),
            owner_feature_id: entry.feature_id,
            ordinal: entry.ordinal,
            loop_id: entry.loop_id,
            field_bytes: entry.fields().map(<[u8]>::to_vec).collect(),
            boundary: entry.boundary.clone(),
            offset: entry.offset,
            end_offset: entry.end_offset,
            source_section: source_section(scan, entry.offset),
        })
        .collect()
}

pub(super) fn feature_affected_id_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureAffectedIdsRecord> {
    scan.features
        .affected_ids
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

pub(super) fn feature_replay_affected_id_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureReplayAffectedIdsRecord> {
    scan.features
        .replay_affected_ids
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

pub(super) fn surface_merge_replay_affected_id_records(
    scan: &ContainerScan,
) -> Vec<CreoSurfaceMergeReplayAffectedIdsRecord> {
    scan.features
        .surface_merge_replay_affected_ids
        .iter()
        .map(|record| CreoSurfaceMergeReplayAffectedIdsRecord {
            id: format!(
                "creo:feature:surface_merge_replay_affected_ids#{}",
                record.offset
            ),
            owner_feature_id: record.feature_id,
            geometry_ids: record.geometry_ids.clone(),
            edge_ids: record.edge_ids.clone(),
            quilt_ids: record.quilt_ids.clone(),
            geometry_extent: extent_source(record.geometry_extent),
            edge_extent: extent_source(record.edge_extent),
            quilt_extent: extent_source(record.quilt_extent),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn feature_loop_restore_direction_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureLoopRestoreDirectionRecord> {
    scan.features
        .loop_restore_directions
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

pub(super) fn feature_revolution_extent_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureRevolutionExtentRecord> {
    scan.features
        .revolution_extents
        .iter()
        .map(|record| CreoFeatureRevolutionExtentRecord {
            id: format!("creo:feature:revolution_extent#{}", record.offset),
            owner_feature_id: record.feature_id,
            kind: "full_turn",
            angle_radians: std::f64::consts::TAU,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn feature_choice_records(scan: &ContainerScan) -> Vec<CreoFeatureChoiceRecord> {
    scan.features
        .choices
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

pub(super) fn feature_row_records(scan: &ContainerScan) -> Vec<CreoFeatureRowRecord> {
    scan.features
        .rows
        .iter()
        .map(|row| CreoFeatureRowRecord {
            id: format!("creo:allfeatur:feature_row#{}", row.offset),
            owner_feature_id: row.feature_id,
            header: row.body.header(),
            root_schema_class: row.root_schema_class.map(SchemaClass::code),
            stream_offset: row.stream_offset,
            body: row.body.to_vec(),
            body_offset: row.body_offset,
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

pub(super) fn depdb_recipe_row_records(scan: &ContainerScan) -> Vec<CreoFeatureRowRecord> {
    scan.features
        .depdb_recipe_rows
        .iter()
        .map(|row| CreoFeatureRowRecord {
            id: format!("creo:depdb:recipe_row#{}", row.offset),
            owner_feature_id: row.feature_id,
            header: [0; 2],
            root_schema_class: row.root_schema_class.map(SchemaClass::code),
            stream_offset: row.stream_offset,
            body: row.body.to_vec(),
            body_offset: row.body_offset,
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

pub(super) fn feature_choice_field_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureChoiceFieldRecord> {
    scan.features
        .choice_fields
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

pub(super) fn half_edge_records(scan: &ContainerScan) -> Vec<CreoHalfEdgeRecord> {
    let topology_rows = scan
        .curves
        .topology_rows
        .iter()
        .map(|row| (row.id, row))
        .collect::<BTreeMap<_, _>>();
    scan.topology
        .half_edges
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
                face_id: edge.face_id.map_or(0, std::num::NonZeroU32::get),
                next: edge.next.map(half_edge_ref),
                offset: row.offset,
                source_section: source_section(scan, row.offset),
            })
        })
        .collect()
}

pub(super) fn loop_records(scan: &ContainerScan) -> Vec<CreoLoopRecord> {
    scan.topology
        .loops
        .iter()
        .enumerate()
        .map(|(index, record)| CreoLoopRecord {
            id: format!("creo:topology:loop#{}", index + 1),
            face_id: record.face_id.map_or(0, std::num::NonZeroU32::get),
            half_edges: record
                .half_edges
                .iter()
                .copied()
                .map(half_edge_ref)
                .collect(),
        })
        .collect()
}

pub(super) fn loop_array_frame_records(scan: &ContainerScan) -> Vec<CreoLoopArrayFrameRecord> {
    scan.loop_arrays
        .frames
        .iter()
        .map(|frame| CreoLoopArrayFrameRecord {
            id: format!("creo:loop_array:frame#{}", frame.offset),
            variant: frame.variant,
            declared_count: frame.declared_count,
            class_id: frame.class_id,
            rows: frame.rows,
            offset: frame.offset,
            prototype_end: frame.prototype_end,
            end: frame.end,
            source_section: source_section(scan, frame.offset),
        })
        .collect()
}

pub(super) fn loop_array_record_records(scan: &ContainerScan) -> Vec<CreoLoopArrayRecord> {
    scan.loop_arrays
        .records
        .iter()
        .map(|record| CreoLoopArrayRecord {
            id: format!("creo:loop_array:record#{}", record.offset),
            frame_offset: record.frame_offset,
            lo_id: record.lo_id,
            lo_type: record.lo_type,
            lo_subtype: record.lo_subtype,
            feature_id: record.feature_id,
            attributes: record.attributes,
            direction: record.direction,
            next_lo_ptr: record.next_lo_ptr,
            body: record.body.clone(),
            offset: record.offset,
            body_offset: record.body_offset,
            end: record.end,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn topological_vertex_records(scan: &ContainerScan) -> Vec<CreoTopologicalVertexRecord> {
    scan.topology
        .vertices
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

pub(super) fn half_edge_vertex_incidence_records(
    scan: &ContainerScan,
) -> Vec<CreoHalfEdgeVertexIncidenceRecord> {
    scan.topology
        .half_edge_vertex_incidence
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

pub(super) fn face_component_records(scan: &ContainerScan) -> Vec<CreoFaceComponentRecord> {
    scan.topology
        .face_components
        .iter()
        .enumerate()
        .map(|(index, record)| CreoFaceComponentRecord {
            id: format!("creo:topology:face_component#{}", index + 1),
            face_ids: record.face_ids.clone(),
            curve_ids: record.curve_ids.clone(),
        })
        .collect()
}

pub(super) fn fc_curve_coordinate_records(
    scan: &ContainerScan,
) -> Vec<CreoFcCurveCoordinateRecord> {
    scan.curves
        .fc_coordinates
        .iter()
        .map(|record| CreoFcCurveCoordinateRecord {
            id: format!("creo:curve:fc_coordinates#{}", record.curve_id),
            curve_id: record.curve_id,
            subtype: record.subtype,
            body: record.body.clone(),
            values_mm: record.values_mm.clone(),
            tokens: record.tokens.clone(),
            opaque_spans: record.opaque_spans.clone(),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn prototype_pcurve_records(scan: &ContainerScan) -> Vec<CreoPrototypePcurveRecord> {
    scan.curves
        .prototype_pcurves
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

pub(super) fn curve_prototype_topology_records(
    scan: &ContainerScan,
) -> Vec<CreoCurvePrototypeTopologyRecord> {
    scan.curves
        .prototype_topology
        .iter()
        .map(|record| CreoCurvePrototypeTopologyRecord {
            id: format!("creo:curve:prototype_topology#{}", record.curve_id),
            curve_id: record.curve_id,
            faces: record.stored_face_ids(),
            next_edges: record.next_edges,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn curve_prototype_records(
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

pub(super) fn plane_local_system_records(
    scan: &ContainerScan,
    systems: &[crate::surface::PlaneLocalSystem],
    id_prefix: &str,
) -> Vec<CreoPlaneLocalSystemRecord> {
    systems
        .iter()
        .map(|record| {
            let frame = record.frame();
            CreoPlaneLocalSystemRecord {
                id: format!("{id_prefix}#{}:{}", record.offset, record.surface_id),
                surface_id: record.surface_id,
                body: record.body.clone(),
                slots: record.slots.to_vec(),
                origin: frame.origin,
                u_axis: frame.u_axis,
                normal: frame.normal,
                classification: match record.classification {
                    crate::surface::LocalSystemClassification::Simple => "simple",
                    crate::surface::LocalSystemClassification::Unclassified => "unclassified",
                },
                row_offset: record.row_offset,
                offset: record.offset,
                source_section: source_section(scan, record.offset),
            }
        })
        .collect()
}

pub(super) fn plane_envelope_records(
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

pub(super) fn outline_plane_records(
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

pub(super) fn datum_plane_records(scan: &ContainerScan) -> Vec<CreoDatumPlaneRecord> {
    scan.planes
        .datums
        .iter()
        .map(|record| CreoDatumPlaneRecord {
            id: format!(
                "creo:datum:plane#{}:{}",
                record.offset_in_payload, record.id
            ),
            datum_id: record.id,
            owner_feature_id: record.feature_id,
            normal: record.plane.normal(),
            plane_offset: record.plane.offset,
            corners: record.corners(),
            offset: record.offset_in_payload,
            source_section: source_section(scan, record.offset_in_payload),
        })
        .collect()
}

pub(super) fn datum_cylinder_records(scan: &ContainerScan) -> Vec<CreoDatumCylinderRecord> {
    scan.planes
        .datum_cylinders
        .iter()
        .map(|record| CreoDatumCylinderRecord {
            id: format!(
                "creo:datum:cylinder#{}:{}",
                record.offset_in_payload, record.id
            ),
            datum_id: record.id,
            owner_feature_id: record.feature_id,
            reversed: record.reversed,
            origin: record.frame.origin(),
            axis: record.frame.axis(),
            ref_direction: record.frame.ref_direction(),
            radius: record.frame.radius(),
            length: record.frame.length(),
            offset: record.offset_in_payload,
            source_section: source_section(scan, record.offset_in_payload),
        })
        .collect()
}

pub(super) fn feature_section_transform_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureSectionTransformRecord> {
    let mut records = scan
        .features
        .section_transforms
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

pub(super) fn feature_placement_instruction_records(
    scan: &ContainerScan,
) -> Vec<CreoFeaturePlacementInstructionRecord> {
    scan.features
        .definitions
        .iter()
        .flat_map(|definition| {
            crate::feature::placement_instructions(definition)
                .into_iter()
                .map(|instruction| CreoFeaturePlacementInstructionRecord {
                    id: format!(
                        "creo:featdefs:placement_instruction#{}:{}",
                        definition.identity.id(),
                        instruction.offset
                    ),
                    definition_id: definition.identity.id(),
                    owner_feature_id: definition.identity.owner_feature_id(),
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
pub(super) struct CreoSurfaceParameterRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    pub(super) surface_type_byte: u8,
    pub(super) surface_family: &'static str,
    pub(super) boundary: &'static str,
    pub(super) body: Vec<u8>,
    pub(super) slots: Vec<SurfaceParameterScalar>,
    pub(super) opaque_spans: Vec<SurfaceParameterOpaqueSpan>,
    pub(super) scalar_frames: Vec<SurfaceParameterScalarFrame>,
    pub(super) terminal_scalar_frame: Option<SurfaceParameterScalarFrame>,
    pub(super) tabulated_cylinder_frame: Option<CreoTabulatedCylinderFrame>,
    pub(super) positional_cylinder_frame: Option<CreoPositionalCylinderFrame>,
    pub(super) split_cylinder_outline_bounds: Option<[[f64; 2]; 2]>,
    pub(super) positional_cone_frame: Option<CreoPositionalConeFrame>,
    pub(super) positional_torus_frame: Option<CreoPositionalTorusFrame>,
    pub(super) torus_outline_frame: Option<CreoTorusOutlineFrame>,
    pub(super) type26_five_coordinate_envelope: Option<CreoType26FiveCoordinateEnvelope>,
    pub(super) type26_split_coordinate_envelope: Option<CreoType26SplitCoordinateEnvelope>,
    pub(super) torus_radius_overrides: Option<CreoTorusRadiusOverrides>,
    pub(super) replayed_torus_minor_radius: Option<f64>,
    pub(super) cone_half_angle_override: Option<CreoConeHalfAngleOverride>,
    pub(super) extrusion_direction: Option<[f64; 3]>,
    pub(super) row_offset: usize,
    pub(super) body_offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceRowRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    pub(super) type_byte: u8,
    pub(super) surface_family: &'static str,
    pub(super) surface_variant: Option<&'static str>,
    pub(super) feature_id: u32,
    pub(super) reversed: bool,
    pub(super) boundary_type: u8,
    pub(super) next_surface: u32,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceContourRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    pub(super) chain_index: usize,
    pub(super) curve_header_id: u32,
    pub(super) trv: u8,
    pub(super) parameter_envelope: [Option<f64>; 4],
    pub(super) separator_reference: Option<u32>,
    pub(super) body: Vec<u8>,
    pub(super) offset: usize,
    pub(super) envelope_offset: usize,
    pub(super) surface_row_offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoSurfacePrototypeRecord {
    pub(super) id: String,
    pub(super) declared_family: String,
    pub(super) family: String,
    pub(super) parameters: Vec<CreoSurfaceNamedParameterRecord>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceNamedParameterRecord {
    pub(super) name: String,
    #[serde(flatten, serialize_with = "serialize_surface_named_value")]
    pub(super) value: crate::surface::SurfaceNamedValue,
    pub(super) body: Vec<u8>,
    pub(super) offset: usize,
    pub(super) value_offset: usize,
}

fn serialize_surface_named_value<S: serde::Serializer>(
    value: &crate::surface::SurfaceNamedValue,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let (
        value_kind,
        compact_values,
        scalar_dimensions,
        scalar_count,
        scalar_values,
        scalar_tokens,
        opaque,
    ) = match value {
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
        crate::surface::SurfaceNamedValue::ContiguousEntityReferences(entity_ids) => (
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
            tokens.clone().unwrap_or_default(),
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
    let mut map = serializer.serialize_map(Some(7))?;
    map.serialize_entry("value_kind", &value_kind)?;
    map.serialize_entry("compact_values", &compact_values)?;
    map.serialize_entry("scalar_dimensions", &scalar_dimensions)?;
    map.serialize_entry("scalar_count", &scalar_count)?;
    map.serialize_entry("scalar_values", &scalar_values)?;
    map.serialize_entry("scalar_tokens", &scalar_tokens)?;
    map.serialize_entry("opaque", &opaque)?;
    map.end()
}

#[derive(Serialize)]
pub(super) struct CreoCurveParameterRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) type_byte: u8,
    pub(super) body: Vec<u8>,
    pub(super) scalar_values: Vec<f64>,
    pub(super) scalar_tokens: Vec<CreoCurveParameterScalar>,
    pub(super) skipped_references: Vec<u32>,
    pub(super) references: Vec<CreoCurveParameterReference>,
    pub(super) opaque_spans: Vec<CreoCurveParameterOpaqueSpan>,
    pub(super) reference_geometry: [u32; 2],
    pub(super) suffix: &'static str,
    pub(super) suffix_candidate_count: Option<usize>,
    pub(super) offset: usize,
    pub(super) body_offset: usize,
    pub(super) suffix_offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoCurveTopologyRowRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) type_byte: u8,
    pub(super) feature_id: u32,
    pub(super) directions: [u8; 2],
    pub(super) faces: [u32; 2],
    pub(super) next_edges: [u32; 2],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoCrossSectionCurveRowRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) type_byte: u8,
    pub(super) feature_id: u32,
    pub(super) directions: [u8; 2],
    pub(super) suffix: crate::curve::DepdbCurveSuffix,
    pub(super) body: Vec<u8>,
    pub(super) scalar_values: Vec<f64>,
    pub(super) scalar_tokens: Vec<CreoCurveParameterScalar>,
    pub(super) references: Vec<CreoCurveParameterReference>,
    pub(super) opaque_spans: Vec<CreoCurveParameterOpaqueSpan>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoTabulatedCylinderCurveReplayRecord {
    pub(super) id: String,
    pub(super) body: Vec<u8>,
    pub(super) surface_id: u32,
    pub(super) curve_id: u32,
    pub(super) curve_type: u8,
    pub(super) flip: u8,
    pub(super) tangent_condition: u8,
    pub(super) degree: u8,
    pub(super) parameter_body: Vec<u8>,
    pub(super) control_point_ids: [u32; 4],
    pub(super) successor_reference: u32,
    pub(super) control_point_bodies: [Vec<u8>; 4],
    pub(super) control_points: [Option<[f64; 2]>; 4],
    pub(super) terminal_reference: u32,
    pub(super) offset: usize,
    pub(super) surface_row_offset: usize,
    pub(super) source_section: String,
}

pub(super) fn surface_row_records(
    scan: &ContainerScan,
    rows: &[crate::surface::SurfaceRow],
    namespace: &str,
) -> Vec<CreoSurfaceRowRecord> {
    rows.iter()
        .map(|row| CreoSurfaceRowRecord {
            id: format!("creo:{namespace}:surface_row#{}", row.id),
            surface_id: row.id,
            type_byte: row.kind.canonical_type_byte(),
            surface_family: surface_family(row.kind),
            surface_variant: surface_variant(row.kind),
            feature_id: row.feature_id,
            reversed: row.reversed,
            boundary_type: row.boundary_type.code(),
            next_surface: row.next_surface,
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

pub(super) fn surface_prototype_records(
    scan: &ContainerScan,
    records: &[crate::surface::SurfacePrototypeRecord],
    id_namespace: &str,
) -> Vec<CreoSurfacePrototypeRecord> {
    records
        .iter()
        .map(|record| CreoSurfacePrototypeRecord {
            id: format!("creo:{id_namespace}:surface_prototype#{}", record.offset),
            declared_family: record.family.name().to_owned(),
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

pub(super) fn surface_contour_records(
    scan: &ContainerScan,
    records: &[crate::surface::SurfaceContourRecord],
    namespace: &str,
) -> Vec<CreoSurfaceContourRecord> {
    records
        .iter()
        .map(|record| CreoSurfaceContourRecord {
            id: format!(
                "creo:{namespace}:surface_contour#{}-{}",
                record.surface_id, record.offset
            ),
            surface_id: record.surface_id,
            chain_index: record.chain_index,
            curve_header_id: record.curve_header_id,
            trv: record.trv,
            parameter_envelope: record.parameter_envelope,
            separator_reference: record.separator_reference,
            body: record.body.clone(),
            offset: record.offset,
            envelope_offset: record.envelope_offset,
            surface_row_offset: record.surface_row_offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

fn curve_occurrence_identity(curve_id: u32, offset: usize, occurrence_count: usize) -> String {
    if occurrence_count == 1 {
        curve_id.to_string()
    } else {
        // The separator sorts before a decimal digit.  This keeps source-order
        // emission lexicographically ordered when a repeated id is followed by
        // an id with the repeated id as a decimal prefix, such as `1` and `10`.
        format!("{curve_id}-{offset:020}")
    }
}

pub(super) fn curve_parameter_records(
    scan: &ContainerScan,
    records: &[crate::curve::CurveParameterRecord],
    id_namespace: &str,
) -> Vec<CreoCurveParameterRecord> {
    let curve_id_counts =
        records
            .iter()
            .fold(BTreeMap::<u32, usize>::new(), |mut counts, record| {
                *counts.entry(record.curve_id).or_default() += 1;
                counts
            });
    records
        .iter()
        .map(|record| CreoCurveParameterRecord {
            id: format!(
                "creo:{id_namespace}:curve_parameter#{}",
                curve_occurrence_identity(
                    record.curve_id,
                    record.offset,
                    curve_id_counts[&record.curve_id],
                )
            ),
            curve_id: record.curve_id,
            type_byte: record.type_byte,
            body: record.body.clone(),
            scalar_values: record.scalar_values(),
            scalar_tokens: record
                .scalar_tokens
                .iter()
                .map(|token| CreoCurveParameterScalar {
                    value: token.value,
                    raw: token.raw.clone(),
                    offset: token.offset,
                    length: token.raw.len(),
                })
                .collect(),
            skipped_references: record.skipped_references(),
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
                    length: span.raw.len(),
                })
                .collect(),
            reference_geometry: record.reference_geometry,
            suffix: "unique",
            suffix_candidate_count: None,
            offset: record.offset,
            body_offset: record.body_offset,
            suffix_offset: record.suffix_offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn cross_section_curve_row_records(
    scan: &ContainerScan,
) -> Vec<CreoCrossSectionCurveRowRecord> {
    let curve_id_counts = scan.curves.cross_section_rows.iter().fold(
        BTreeMap::<u32, usize>::new(),
        |mut counts, row| {
            *counts.entry(row.id).or_default() += 1;
            counts
        },
    );
    scan.curves
        .cross_section_rows
        .iter()
        .map(|row| CreoCrossSectionCurveRowRecord {
            id: format!(
                "creo:cross_section_geometry:curve_row#{}",
                curve_occurrence_identity(row.id, row.offset, curve_id_counts[&row.id])
            ),
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
                    length: token.raw.len(),
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
                    length: span.raw.len(),
                })
                .collect(),
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

pub(super) fn curve_topology_row_records(
    scan: &ContainerScan,
    rows: &[crate::curve::CurveTopologyRow],
    id_namespace: &str,
) -> Vec<CreoCurveTopologyRowRecord> {
    let curve_id_counts = rows
        .iter()
        .fold(BTreeMap::<u32, usize>::new(), |mut counts, row| {
            *counts.entry(row.id).or_default() += 1;
            counts
        });
    rows.iter()
        .map(|row| CreoCurveTopologyRowRecord {
            id: format!(
                "creo:{id_namespace}:curve_topology#{}",
                curve_occurrence_identity(row.id, row.offset, curve_id_counts[&row.id])
            ),
            curve_id: row.id,
            type_byte: row.type_byte,
            feature_id: row.feature_id,
            directions: row.directions,
            faces: row.stored_face_ids(),
            next_edges: row.next_edges,
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

pub(super) fn tabulated_cylinder_curve_replay_records(
    scan: &ContainerScan,
) -> Vec<CreoTabulatedCylinderCurveReplayRecord> {
    scan.curves
        .tabulated_cylinder_replays
        .iter()
        .map(|record| CreoTabulatedCylinderCurveReplayRecord {
            id: format!(
                "creo:visibgeom:tabulated_cylinder_curve_replay#{}",
                record.surface_id
            ),
            body: record.body.clone(),
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

pub(super) fn surface_parameter_records(
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
                surface_type_byte: row.kind.canonical_type_byte(),
                surface_family,
                boundary,
                body: record.body.clone(),
                slots: record.scalar_tokens.clone(),
                opaque_spans: record.opaque_spans.clone(),
                scalar_frames: record.scalar_frames.clone(),
                terminal_scalar_frame: record.terminal_scalar_frame.clone(),
                tabulated_cylinder_frame: record.tabulated_cylinder_frame().map(|frame| {
                    CreoTabulatedCylinderFrame {
                        values: frame.values,
                        prefixes: frame.prefixes,
                    }
                }),
                positional_cylinder_frame: record.positional_cylinder_frame().map(|frame| {
                    CreoPositionalCylinderFrame {
                        origin: frame.origin(),
                        axis: frame.axis(),
                        ref_direction: frame.ref_direction(),
                        radius: frame.radius(),
                        length: frame.length(),
                    }
                }),
                split_cylinder_outline_bounds: record.split_cylinder_outline_bounds(),
                positional_cone_frame: record.positional_cone_frame().map(|frame| {
                    CreoPositionalConeFrame {
                        apex: frame.apex(),
                        axis: frame.axis(),
                        ref_direction: frame.ref_direction(),
                        half_angle: frame.half_angle(),
                    }
                }),
                positional_torus_frame: record.positional_torus_frame().map(|frame| {
                    CreoPositionalTorusFrame {
                        center: frame.center(),
                        axis: frame.axis(),
                        ref_direction: frame.ref_direction(),
                        major_radius: frame.major_radius(),
                        minor_radius: frame.minor_radius(),
                    }
                }),
                torus_outline_frame: record.torus_outline_frame().map(|frame| {
                    CreoTorusOutlineFrame {
                        values: frame.values,
                        selector: frame.selector,
                        offset: frame.offset,
                    }
                }),
                type26_five_coordinate_envelope: record.type26_five_coordinate_envelope().map(
                    |envelope| CreoType26FiveCoordinateEnvelope {
                        values: envelope.values,
                        offset: envelope.offset,
                    },
                ),
                type26_split_coordinate_envelope: record.type26_split_coordinate_envelope().map(
                    |envelope| CreoType26SplitCoordinateEnvelope {
                        values: envelope.values,
                        offset: envelope.offset,
                    },
                ),
                torus_radius_overrides: record.torus_radius_overrides().map(|overrides| {
                    CreoTorusRadiusOverrides {
                        radius1: overrides.radius1,
                        radius2: overrides.radius2,
                        radius2_encoding: match overrides.radius2_encoding {
                            crate::surface::TorusRadius2Encoding::Direct => "direct",
                            crate::surface::TorusRadius2Encoding::OuterRingDifference => {
                                "outer_ring_difference"
                            }
                        },
                        offset: overrides.offset,
                    }
                }),
                replayed_torus_minor_radius: replayed_torus_minor_radius(scan, row, record),
                cone_half_angle_override: record.cone_half_angle_override().map(|half_angle| {
                    CreoConeHalfAngleOverride {
                        radians: half_angle.radians,
                        offset: half_angle.offset,
                    }
                }),
                extrusion_direction: record.extrusion_direction(),
                row_offset: record.offset,
                body_offset: record.body_offset,
                source_section,
            })
        })
        .collect()
}

pub(super) fn feature_operation_state_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureOperationState> {
    let current_offsets = scan
        .features
        .operations
        .iter()
        .map(|state| (state.feature_id, state.offset))
        .collect::<BTreeMap<_, _>>();
    let mut ordinals = BTreeMap::<u32, usize>::new();
    scan.features
        .operation_states
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
                current: !state.display_state_conflict
                    && current_offsets.get(&state.feature_id) == Some(&state.offset),
                family: state.kind.as_str().to_string(),
                name: state.name.clone(),
                recipe: state
                    .recipe
                    .candidate()
                    .map(crate::feature::FeatureRecipe::name),
                recipe_conflict: state.recipe.is_conflicting().then_some(true),
                display_state_conflict: state.display_state_conflict.then_some(true),
                root_schema_class: state.root_schema_class().map(SchemaClass::code),
                parent_feature_id: state.parent_feature_id(),
                offset: state.offset,
                state_offset: state.state_offset,
            }
        })
        .collect()
}

pub(super) fn feature_reference_name_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureReferenceNameRecord> {
    scan.features
        .reference_names
        .iter()
        .map(|record| CreoFeatureReferenceNameRecord {
            id: format!("creo:mdlrefinfo:feature_name#{}", record.offset),
            owner_feature_id: record.feature_id,
            name: record.name().into_owned(),
            name_bytes: record.name_bytes.clone(),
            own_reference_id: record.own_reference_id,
            reference_type: record.reference_type,
            offset: record.offset,
        })
        .collect()
}

#[derive(Serialize)]
pub(super) struct CreoPcurveEndpointRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) faces: [u32; 2],
    pub(super) face_0_endpoints: [[f64; 2]; 2],
    pub(super) face_1_endpoints: [[f64; 2]; 2],
    pub(super) source_form: &'static str,
}

pub(super) fn pcurve_endpoint_records(
    scan: &ContainerScan,
) -> Vec<(CreoPcurveEndpointRecord, usize)> {
    let mut records = scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            (
                CreoPcurveEndpointRecord {
                    id: format!("creo:visibgeom:pcurve_endpoints#{}", pcurve.curve_id),
                    curve_id: pcurve.curve_id,
                    faces: pcurve.stored_face_ids(),
                    face_0_endpoints: pcurve.face_0_endpoints,
                    face_1_endpoints: pcurve.face_1_endpoints,
                    source_form: "positional",
                },
                pcurve.offset,
            )
        })
        .collect::<Vec<_>>();
    records.extend(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
        (
            CreoPcurveEndpointRecord {
                id: format!(
                    "creo:visibgeom:prototype_pcurve_endpoints#{}",
                    pcurve.curve_id
                ),
                curve_id: pcurve.curve_id,
                faces: pcurve.stored_face_ids(),
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

pub(super) fn curve_expression_records(scan: &ContainerScan) -> Vec<CreoCurveExpressionRecord> {
    scan.curves
        .expressions
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
                    target: assignment.target.clone(),
                    expression: assignment.expression.clone(),
                    dependencies: assignment.dependencies.clone(),
                    value: assignment.value.clone(),
                    activation: assignment.activation.token(),
                    offset: assignment.offset,
                })
                .collect(),
            solve_blocks: record
                .solve_blocks
                .iter()
                .map(|block| CreoCurveExpressionSolveBlock {
                    equations: block
                        .equations
                        .iter()
                        .map(|equation| CreoCurveExpressionEquation {
                            left: equation.left.clone(),
                            right: equation.right.clone(),
                            dependencies: equation.dependencies.clone(),
                            offset: equation.offset,
                        })
                        .collect(),
                    assignments: block
                        .assignments
                        .iter()
                        .map(|assignment| CreoCurveExpressionAssignment {
                            target: assignment.target.clone(),
                            expression: assignment.expression.clone(),
                            dependencies: assignment.dependencies.clone(),
                            value: assignment.value.clone(),
                            activation: assignment.activation.token(),
                            offset: assignment.offset,
                        })
                        .collect(),
                    variables: block
                        .unknowns
                        .iter()
                        .map(|unknown| unknown.name.clone())
                        .collect(),
                    solutions: block
                        .unknowns
                        .iter()
                        .map(|unknown| unknown.solution.clone())
                        .collect(),
                    offset: block.offset,
                    for_offset: block.for_offset,
                })
                .collect(),
            unresolved_solve_control: record.unresolved_solve_control,
            prohibited_constructs: record.prohibited_constructs.clone(),
        })
        .collect()
}

pub(super) fn sketch_records(scan: &ContainerScan) -> Vec<CreoSketchRecord> {
    scan.features
        .definitions
        .iter()
        .filter(|definition| feature_definition_has_sketch_design(definition))
        .map(|definition| CreoSketchRecord {
            id: feature_sketch_record_id_in_scan(scan, definition),
            definition_id: definition.identity.id(),
            owner_feature_id: definition.identity.owner_feature_id(),
            source_section: source_section(scan, definition.offset),
            offset: definition.offset,
            section_3d: definition
                .section_3d
                .as_ref()
                .map(|section| CreoSketchSection3d {
                    sketch_plane_entity_id: section.sketch_plane_entity_id,
                    sketch_plane_flip: section.sketch_plane_flip.map(binary_flag_value),
                    reference_planes: section.reference_planes.clone(),
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
            variables: {
                let resolved_coordinates = resolved_section_coordinates(definition);
                let resolved_radii = resolved_section_radii(definition);
                let resolved_scalars = resolved_section_scalar_values(definition);
                definition
                    .variables
                    .iter()
                    .flat_map(|table| &table.rows)
                    .map(|row| CreoSketchVariable {
                        variable_type: row.variable_type.code(),
                        key: row.key,
                        value: row.value,
                        value_body: row.value_body.clone(),
                        guess: row.guess,
                        guess_body: row.guess_body.clone(),
                        known: row.known,
                        homogeneity: row.homogeneity,
                        uvar_id: row.uvar_id,
                        resolved_value: match row.variable_type {
                            VariableType::U => resolved_coordinates
                                .get(&row.key)
                                .and_then(|point| point[0]),
                            VariableType::V => resolved_coordinates
                                .get(&row.key)
                                .and_then(|point| point[1]),
                            VariableType::Radius => resolved_radii.get(&row.key).copied(),
                            _ => resolved_scalars.get(&(row.variable_type, row.key)).copied(),
                        },
                        offset: row.offset,
                    })
                    .collect()
            },
            equations: crate::feature::equation_table(&definition.body, 0, definition.body.len())
                .into_iter()
                .flat_map(|table| table.rows)
                .map(|equation| CreoSketchEquation {
                    equation_id: equation.equation_id,
                    function_id: equation.function_id,
                    explicit_argument_count: equation.explicit_argument_count,
                    arguments: equation.arguments,
                    arguments_body: equation.arguments_body,
                    auxiliary_body: equation.auxiliary_body,
                    body: equation.body,
                    offset: equation.offset,
                })
                .collect(),
            segments: definition
                .segments
                .iter()
                .flat_map(|table| table.rows.ordinary())
                .map(|segment| CreoSketchSegment {
                    external_id: segment.external_id,
                    kind: match segment.kind {
                        crate::feature::FeatureSegmentKind::Line(_) => "line",
                        crate::feature::FeatureSegmentKind::Arc(_) => "arc",
                        crate::feature::FeatureSegmentKind::Point(_) => "point",
                    },
                    point_ids: segment.point_ids(),
                    center_id: segment.center_id,
                    directions: segment.directions,
                    arc_orientation: segment.arc_orientation,
                    vertical_horizontal_constraint: segment.vertical_horizontal,
                    radius_dimension_id: segment.radius_ref,
                    secondary_radius_dimension_id: segment.radius2_ref,
                    body: segment.body.clone(),
                    offset: segment.offset,
                })
                .collect(),
            circle_segments: definition
                .segments
                .iter()
                .flat_map(|table| table.rows.circles())
                .map(|segment| CreoSketchCircleSegment {
                    external_id: segment.external_id,
                    center_id: segment.center_id,
                    radius_dimension_id: segment.radius_ref,
                    offset: segment.offset,
                })
                .collect(),
            point_segments: definition
                .segments
                .iter()
                .flat_map(|table| table.rows.points())
                .map(|segment| CreoSketchPointSegment {
                    external_id: segment.external_id,
                    point_id: segment.point_id,
                    offset: segment.offset,
                })
                .collect(),
            centered_line_segments: definition
                .segments
                .iter()
                .flat_map(|table| table.rows.centered_lines())
                .map(|segment| CreoSketchCenteredLineSegment {
                    external_id: segment.external_id,
                    center_id: segment.center_id,
                    offset: segment.offset,
                })
                .collect(),
            reference_line_segments: definition
                .segments
                .iter()
                .flat_map(|table| table.rows.reference_lines())
                .map(|segment| CreoSketchReferenceLineSegment {
                    external_id: segment.external_id,
                    point_ids: segment.point_ids,
                    directions: segment.directions,
                    vertical_horizontal_constraint: segment.vertical_horizontal,
                    offset: segment.offset,
                })
                .collect(),
            bounded_curve_segments: definition
                .segments
                .iter()
                .flat_map(|table| table.rows.bounded_curves())
                .map(|segment| CreoSketchBoundedCurveSegment {
                    external_id: segment.external_id,
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
            conic_segments: definition
                .segments
                .iter()
                .flat_map(|table| table.rows.conics())
                .map(|segment| CreoSketchConicSegment {
                    external_id: segment.external_id,
                    center_id: segment.center_id,
                    first_coefficient_ref: segment.first_coefficient_ref,
                    second_coefficient_ref: segment.second_coefficient_ref,
                    offset: segment.offset,
                })
                .collect(),
            opaque_segments: definition
                .segments
                .iter()
                .flat_map(|table| table.rows.opaque())
                .map(|segment| CreoSketchOpaqueSegment {
                    external_id: segment.external_id,
                    kind: segment.kind,
                    point_ids: segment.point_ids,
                    center_id: segment.center_id,
                    directions: segment.directions,
                    arc_orientation: segment.arc_orientation,
                    vertical_horizontal_constraint: segment.vertical_horizontal,
                    radius_dimension_id: segment.radius_ref,
                    secondary_radius_dimension_id: segment.radius2_ref,
                    body: segment.body.clone(),
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
                    center_vertex: entity.center_vertex(),
                    kind: match entity.kind {
                        crate::feature::TrimEntityKind::Line => "line",
                        crate::feature::TrimEntityKind::Arc { .. } => "arc",
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
                    entities: vertex.entities.clone(),
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
                        body: line.body.clone(),
                        offset: line.offset,
                    },
                    crate::feature::FeatureSavedEntity::Arc(arc) => CreoSketchSavedEntity::Arc {
                        entity_id: arc.entity_id,
                        center: arc.center,
                        radius: arc.radius,
                        endpoints: arc.endpoints,
                        parameters: arc.parameters,
                        body: arc.body.clone(),
                        offset: arc.offset,
                    },
                    crate::feature::FeatureSavedEntity::Circle(circle) => {
                        CreoSketchSavedEntity::Circle {
                            entity_id: circle.entity_id,
                            center: circle.center,
                            radius: circle.radius,
                            body: circle.body.clone(),
                            offset: circle.offset,
                        }
                    }
                    crate::feature::FeatureSavedEntity::Conic(conic) => {
                        CreoSketchSavedEntity::Conic {
                            entity_id: conic.entity_id,
                            endpoints: conic.endpoints,
                            parameters: conic.parameters,
                            coefficients: conic.coefficients,
                            local_system: conic.local_system,
                            body: conic.body.clone(),
                            offset: conic.offset,
                        }
                    }
                    crate::feature::FeatureSavedEntity::Spline(spline) => {
                        CreoSketchSavedEntity::Spline {
                            entity_id: spline.entity_id,
                            declared_point_count: spline.declared_point_count,
                            interpolation_points: spline.interpolation_points.clone(),
                            interpolation_points_body: spline.interpolation_points_body.clone(),
                            endpoint_tangents: crate::decode::native_records::SplineTangents(
                                spline.endpoint_tangents.clone(),
                            ),
                            parameters: crate::decode::native_records::SplineParameters(
                                spline.parameters.clone(),
                            ),
                            offset: spline.offset,
                        }
                    }
                    crate::feature::FeatureSavedEntity::Dummy(dummy) => {
                        CreoSketchSavedEntity::Dummy {
                            entity_id: dummy.entity_id,
                            body: dummy.body.clone(),
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
                    value: dimension.value.clone(),
                    value_body: dimension.value_body.clone(),
                    unit: match dimension.unit() {
                        crate::feature::DimensionUnit::Radians => "radians",
                        crate::feature::DimensionUnit::Millimeters => "millimeters",
                        crate::feature::DimensionUnit::SchemaDefined => "schema_defined",
                    },
                    direction_byte: dimension.direction_byte,
                    auxiliary_value: dimension.auxiliary_value,
                    auxiliary_body: dimension.auxiliary_body.clone(),
                    references: dimension.references.as_ref().map(|table| {
                        CreoSketchDimensionReferenceTable {
                            declared_count: table.declared_count,
                            entity_ref: table.entity_ref,
                            rows: table
                                .rows
                                .iter()
                                .map(|reference| CreoSketchDimensionReference {
                                    item_id: reference.item_id,
                                    sense: reference.sense,
                                    point: reference.point,
                                    offset: reference.offset,
                                })
                                .collect(),
                            offset: table.offset,
                        }
                    }),
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
                .flat_map(FeatureRelationTable::skamps)
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
                .flat_map(FeatureRelationTable::triples)
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

pub(super) fn sketch_section_point_records(
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
                CreoSketchPointState::Conflicting
            } else {
                match (u, v) {
                    (Some(u), Some(v)) => CreoSketchPointState::Resolved([u, v]),
                    (Some(u), None) => CreoSketchPointState::PartialU(u),
                    (None, Some(v)) => CreoSketchPointState::PartialV(v),
                    (None, None) => CreoSketchPointState::Unresolved,
                }
            };
            CreoSketchSectionPoint { point_id, state }
        })
        .collect()
}

pub(super) fn feature_definition_records(scan: &ContainerScan) -> Vec<CreoFeatureDefinitionRecord> {
    scan.features
        .definitions
        .iter()
        .map(|definition| CreoFeatureDefinitionRecord {
            id: feature_definition_record_id(scan, definition),
            definition_id: definition.identity.id(),
            owner_feature_id: definition.identity.owner_feature_id(),
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
                    decoded_values: frame.decoded_values,
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
                    local_values: outline
                        .local_scalars
                        .iter()
                        .map(|field| field.value)
                        .collect(),
                    local_value_bodies: outline
                        .local_scalars
                        .iter()
                        .map(|field| field.body.clone())
                        .collect(),
                    offset: outline.offset,
                })
                .collect(),
            offset: definition.offset,
        })
        .collect()
}

pub(super) fn family_table_record(scan: &ContainerScan) -> Option<CreoFamilyTableRecord> {
    let record = scan.framing.family_table?;
    Some(CreoFamilyTableRecord {
        pointer: record.pointer,
        offset: record.offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_feature_candidates_do_not_expose_short_headers() {
        let payload = [1, 0xe3, 2, 0, 0, 0xe3, 0xf6, 0x83, 0x8f, 0xe1];
        let mut scan = crate::container::scan_bytes(Vec::new());
        scan.features.rows = crate::feature::rows(&payload, &BTreeSet::from([1, 2]), 0);
        let records = feature_row_records(&scan);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].owner_feature_id, 2);
        assert_eq!(records[0].header, [0, 0]);
        assert_eq!(records[0].body, payload[3..]);
    }
}
