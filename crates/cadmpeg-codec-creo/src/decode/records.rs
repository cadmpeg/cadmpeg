// SPDX-License-Identifier: Apache-2.0
//! Record shadow-layer structs and their `ContainerScan` mappers, moved
//! verbatim from `decode.rs`.

use cadmpeg_ir::native::bytes::NativeBytes;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use cadmpeg_core::CodecError;
use cadmpeg_ir::hash::sha256;
use serde::Serialize;

use crate::curve::{FcCurveCoordinateToken, FcCurveOpaqueSpan};
use crate::surface::{
    SurfaceParameterOpaqueSpan, SurfaceParameterScalar, SurfaceParameterScalarFrame,
};

pub(super) mod double_xar;

use crate::container::ContainerScan;
use crate::feature::definitions::{FeatureRelationTable, VariableType};
use crate::feature::schema::SchemaClass;

use super::coverage::{source_section_ref, surface_family, surface_variant};
use super::curve_expressions::curve_expression_record_id;
use super::expanded::{affected_kind, extent_source, half_edge_ref};
use super::feature_history::round::replayed_torus_minor_radius;
use super::native_records::{
    CreoConeHalfAngleOverride, CreoCurveExpressionAssignment, CreoCurveExpressionEquation,
    CreoCurveExpressionLine, CreoCurveExpressionLocalSystem, CreoCurveExpressionSolveBlock,
    CreoFeatureFieldValue, CreoFeatureOperationState, CreoFeatureOutline,
    CreoFeatureParameterFrame, CreoHalfEdgeRef, CreoOperationNameRecord, CreoPlaneEnvelope,
    CreoPositionalConeFrame, CreoPositionalCylinderFrame, CreoPositionalTorusFrame,
    CreoSketchBoundedCurveSegment, CreoSketchCenteredLineSegment, CreoSketchCircleSegment,
    CreoSketchConicSegment, CreoSketchDimension, CreoSketchDimensionReference,
    CreoSketchDimensionReferenceTable, CreoSketchEquation, CreoSketchOpaqueSegment,
    CreoSketchOrderRow, CreoSketchPointSegment, CreoSketchPointState,
    CreoSketchReferenceLineSegment, CreoSketchRelation, CreoSketchRelationTriple,
    CreoSketchSavedEntity, CreoSketchSection3d, CreoSketchSectionOrientation,
    CreoSketchSectionPoint, CreoSketchSegment, CreoSketchSkamp, CreoSketchSkampItem,
    CreoSketchTableHeader, CreoSketchTrimEntity, CreoSketchTrimVertex, CreoSketchVariable,
    CreoTabulatedCylinderFrame, CreoTorusOutlineFrame, CreoTorusRadiusOverrides,
    CreoType26FiveCoordinateEnvelope, CreoType26SplitCoordinateEnvelope,
};
use super::sketch::coordinates::resolved_section_coordinates;
use super::sketch::equations_scalar::resolved_section_scalar_values;
use super::sketch::radii::resolved_section_radii;
use super::sketch_ids::{
    binary_flag_value, feature_definition_has_sketch_design, feature_definition_record_id,
    sketch_table_headers,
};

#[derive(Serialize)]
pub(super) struct CreoSketchRecord<'a> {
    pub(super) id: String,
    definition_id: u32,
    owner_feature_id: Option<u32>,
    pub(super) source_section: String,
    pub(super) offset: usize,
    section_3d: Option<CreoSketchSection3d<'a>>,
    table_headers: Vec<CreoSketchTableHeader>,
    section_points: Vec<CreoSketchSectionPoint>,
    solved_external_ids: Vec<u32>,
    variables: Vec<CreoSketchVariable>,
    equations: Vec<CreoSketchEquation>,
    segments: Vec<CreoSketchSegment>,
    circle_segments: Vec<CreoSketchCircleSegment>,
    point_segments: Vec<CreoSketchPointSegment>,
    centered_line_segments: Vec<CreoSketchCenteredLineSegment>,
    reference_line_segments: Vec<CreoSketchReferenceLineSegment>,
    bounded_curve_segments: Vec<CreoSketchBoundedCurveSegment>,
    conic_segments: Vec<CreoSketchConicSegment>,
    opaque_segments: Vec<CreoSketchOpaqueSegment>,
    trim_entities: Vec<CreoSketchTrimEntity>,
    trim_vertices: Vec<CreoSketchTrimVertex>,
    order_rows: Vec<CreoSketchOrderRow>,
    saved_entities: Vec<CreoSketchSavedEntity<'a>>,
    dimensions: Vec<CreoSketchDimension>,
    relations: Vec<CreoSketchRelation>,
    skamps: Vec<CreoSketchSkamp>,
    relation_triples: Vec<CreoSketchRelationTriple>,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureDefinitionRecord<'a> {
    pub(super) id: String,
    definition_id: u32,
    owner_feature_id: Option<u32>,
    pub(super) source_section: &'a str,
    body: NativeBytes<&'a [u8]>,
    parameter_frames: Vec<CreoFeatureParameterFrame<'a>>,
    outlines: Vec<CreoFeatureOutline<'a>>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveExpressionRecord<'a> {
    pub(super) id: String,
    entity_id: u32,
    backup: bool,
    local_system: Option<CreoCurveExpressionLocalSystem<'a>>,
    lines: Vec<CreoCurveExpressionLine<'a>>,
    assignments: Vec<CreoCurveExpressionAssignment<'a>>,
    solve_blocks: Vec<CreoCurveExpressionSolveBlock<'a>>,
    unresolved_solve_control: bool,
    prohibited_constructs: &'a [String],
}

#[derive(Debug, Serialize)]
pub(super) struct CreoFeatureReferenceNameRecord {
    pub(super) id: String,
    owner_feature_id: u32,
    name: String,
    name_bytes: NativeBytes<Vec<u8>>,
    own_reference_id: u32,
    reference_type: u32,
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
pub(super) struct CreoFeatureEntityRecord<'a> {
    pub(super) id: String,
    entity_id: u32,
    type_byte: u8,
    name: &'a str,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureEntityReferenceRecord {
    pub(super) id: String,
    source_entity_id: Option<u32>,
    target_entity_id: u32,
    target_resolved: bool,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureEntityTableRecord {
    pub(super) id: String,
    owner_feature_id: u32,
    table_class_id: u32,
    entry_ids: Vec<u32>,
    entries: Vec<CreoFeatureEntityTableEntryRecord>,
    surface_ids: Vec<u32>,
    non_surface_entity_ids: Vec<u32>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
struct CreoFeatureEntityTableEntryRecord {
    entity_id: u32,
    class_id: u32,
    source_entity_id: Option<u32>,
    related_entity_id: Option<u32>,
    related_entity_state: Option<u8>,
    prefixed: bool,
    offset: usize,
    end_offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureGeometryTableRecord<'a> {
    pub(super) id: String,
    owner_feature_id: u32,
    #[serde(flatten, serialize_with = "serialize_geometry_table_kind")]
    kind: &'a crate::feature::rows::FeatureGeometryTableKind,
    declared_count: u32,
    entity_class_id: u32,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

fn serialize_geometry_table_kind<S: serde::Serializer>(
    kind: &crate::feature::rows::FeatureGeometryTableKind,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use crate::feature::rows::FeatureGeometryTableKind;
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
pub(super) struct CreoFeatureLoopHistoryEntryRecord<'a> {
    pub(super) id: String,
    owner_feature_id: u32,
    ordinal: u32,
    loop_id: u32,
    #[serde(serialize_with = "serialize_loop_history_fields")]
    field_bytes: &'a crate::feature::rows::FeatureLoopHistoryEntry,
    #[serde(flatten, serialize_with = "serialize_loop_history_boundary")]
    boundary: &'a crate::feature::rows::FeatureLoopHistoryBoundary,
    pub(super) offset: usize,
    end_offset: usize,
    pub(super) source_section: &'a str,
}

fn serialize_loop_history_fields<S: serde::Serializer>(
    entry: &crate::feature::rows::FeatureLoopHistoryEntry,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(entry.fields().map(NativeBytes::from))
}

fn serialize_loop_history_boundary<S: serde::Serializer>(
    boundary: &crate::feature::rows::FeatureLoopHistoryBoundary,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use crate::feature::rows::FeatureLoopHistoryBoundary;
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
pub(super) struct CreoFeatureAffectedIdsRecord<'a> {
    pub(super) id: String,
    owner_feature_id: u32,
    kind: &'static str,
    ids: &'a [u32],
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureReplayAffectedIdsRecord<'a> {
    pub(super) id: String,
    owner_feature_id: u32,
    geometry_ids: &'a [u32],
    edge_ids: &'a [u32],
    geometry_extent: &'static str,
    edge_extent: &'static str,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceMergeReplayAffectedIdsRecord<'a> {
    pub(super) id: String,
    owner_feature_id: u32,
    geometry_ids: &'a [u32],
    edge_ids: &'a [u32],
    quilt_ids: &'a [u32],
    geometry_extent: &'static str,
    edge_extent: &'static str,
    quilt_extent: &'static str,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureLoopRestoreDirectionRecord<'a> {
    pub(super) id: String,
    owner_feature_id: u32,
    lane: &'static str,
    value: u32,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureRevolutionExtentRecord<'a> {
    pub(super) id: String,
    owner_feature_id: u32,
    kind: &'static str,
    angle_radians: f64,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureChoiceRecord<'a> {
    pub(super) id: String,
    owner_feature_id: u32,
    label: &'a str,
    type_byte: Option<u8>,
    payload: NativeBytes<&'a [u8]>,
    payload_offset: usize,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureRowRecord<'a> {
    pub(super) id: String,
    owner_feature_id: u32,
    header: NativeBytes<[u8; 2]>,
    root_schema_class: Option<u32>,
    stream_offset: usize,
    body: NativeBytes<&'a [u8]>,
    body_offset: usize,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureChoiceFieldRecord<'a> {
    pub(super) id: String,
    owner_feature_id: u32,
    choice_label: &'a str,
    name: &'a str,
    type_byte: u8,
    value: CreoFeatureFieldValue<'a>,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoHalfEdgeRecord<'a> {
    pub(super) id: String,
    curve_id: u32,
    side: crate::topology::Side,
    face_id: u32,
    next: Option<CreoHalfEdgeRef>,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoLoopRecord<'a> {
    id: String,
    face_id: u32,
    #[serde(serialize_with = "serialize_half_edge_refs")]
    half_edges: &'a [crate::topology::HalfEdgeId],
}

fn serialize_half_edge_refs<S: serde::Serializer>(
    half_edges: &[crate::topology::HalfEdgeId],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(half_edges.iter().copied().map(half_edge_ref))
}

#[derive(Serialize)]
pub(super) struct CreoLoopArrayFrameRecord<'a> {
    id: String,
    variant: Option<crate::loop_array::LayoutMarker>,
    declared_count: u32,
    class_id: u32,
    materialized_count: usize,
    overfull: bool,
    offset: usize,
    prototype_end: usize,
    end: usize,
    source_section: &'a str,
}

pub(super) struct CreoLoopArrayRecord<'a> {
    pub(super) id: String,
    frame_offset: usize,
    lo_id: u32,
    lo_type: u32,
    lo_subtype: u32,
    feature_id: u32,
    attributes: u8,
    direction: u32,
    next_lo_ptr: u32,
    body: cadmpeg_ir::native::bytes::NativeBytes<&'a [u8]>,
    pub(super) offset: usize,
    body_offset: usize,
    pub(super) source_section: &'a str,
}

impl Serialize for CreoLoopArrayRecord<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut record = serializer.serialize_struct("CreoLoopArrayRecord", 14)?;
        record.serialize_field("id", &self.id)?;
        record.serialize_field("frame_offset", &self.frame_offset)?;
        record.serialize_field("lo_id", &self.lo_id)?;
        record.serialize_field("lo_type", &self.lo_type)?;
        record.serialize_field("lo_subtype", &self.lo_subtype)?;
        record.serialize_field("feature_id", &self.feature_id)?;
        record.serialize_field("attributes", &self.attributes)?;
        record.serialize_field("direction", &self.direction)?;
        record.serialize_field("next_lo_ptr", &self.next_lo_ptr)?;
        record.serialize_field("body", &self.body)?;
        record.serialize_field("offset", &self.offset)?;
        record.serialize_field("body_offset", &self.body_offset)?;
        record.serialize_field("end", &(self.body_offset + self.body.len()))?;
        record.serialize_field("source_section", &self.source_section)?;
        record.end()
    }
}

#[derive(Serialize)]
pub(super) struct CreoTopologicalVertexRecord<'a> {
    id: String,
    vertex_id: u32,
    #[serde(serialize_with = "serialize_half_edge_refs")]
    half_edges: &'a [crate::topology::HalfEdgeId],
}

#[derive(Serialize)]
pub(super) struct CreoHalfEdgeVertexIncidenceRecord {
    id: String,
    half_edge: CreoHalfEdgeRef,
    start_vertex_id: u32,
    end_vertex_id: Option<u32>,
}

#[derive(Serialize)]
pub(super) struct CreoFaceComponentRecord<'a> {
    id: String,
    face_ids: &'a [u32],
    curve_ids: &'a [u32],
}

#[derive(Serialize)]
pub(super) struct CreoFaceAdmissionRejectionRecord {
    pub(super) id: String,
    pub(super) face_id: u32,
    pub(super) reason: &'static str,
    pub(super) boundary_half_edges: Vec<CreoHalfEdgeRef>,
    pub(super) vertex_ids: Vec<u32>,
}

#[derive(Debug, Serialize)]
pub(super) struct CreoExpandedSectionRecord {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) source_offset: usize,
    compressed_length: usize,
    expanded_length: usize,
    sha256: String,
}

#[derive(Serialize)]
pub(super) struct CreoPrimitiveScalarArrayRecord<'a> {
    pub(super) id: String,
    pub(super) field: &'static str,
    pub(super) expanded_offset: usize,
    pub(super) count: usize,
    pub(super) values: &'a [cadmpeg_ir::scalar::FiniteReal],
}

#[derive(Debug, Serialize)]
pub(super) struct CreoReferenceLineRecord {
    pub(super) id: String,
    #[serde(flatten, serialize_with = "serialize_reference_line_kind")]
    kind: crate::reference::ReferenceLineKind,
    start: [f64; 3],
    end: [f64; 3],
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
        } => ("line3d", Some(*entity_id), Some(original_length.get())),
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
    entity_id: u32,
    center: [f64; 3],
    center_source: &'static str,
    radius: f64,
    axis: [f64; 3],
    endpoints: [[f64; 3]; 2],
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoReferenceConicRecord<'a> {
    pub(super) id: String,
    entity_id: u32,
    type_id: crate::reference::ConicType,
    flip: u32,
    endpoints: [[f64; 3]; 2],
    parameter_interval: [Option<f64>; 2],
    coefficients: [f64; 2],
    local_system: Option<[f64; 12]>,
    body: NativeBytes<&'a [u8]>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoReferenceEllipseRecord {
    pub(super) id: String,
    source_conic_id: String,
    source_entity_id: u32,
    center: [f64; 3],
    axis: [f64; 3],
    major_direction: [f64; 3],
    major_radius: f64,
    minor_radius: f64,
    pub(super) offset: usize,
}

pub(super) fn reference_line_records(scan: &ContainerScan) -> Vec<CreoReferenceLineRecord> {
    let family = |kind: &crate::reference::ReferenceLineKind| match kind {
        crate::reference::ReferenceLineKind::Line => "line",
        crate::reference::ReferenceLineKind::Line3d { .. } => "line3d",
    };
    let mut records = Vec::new();
    for line in &scan.references.lines {
        let id = format!(
            "creo:mdl_ref_info:{}_record#{}",
            family(line.kind()),
            line.offset
        );
        records.push(CreoReferenceLineRecord {
            id,
            kind: line.kind().clone(),
            start: line.start().get().into(),
            end: line.end().get().into(),
            offset: line.offset,
        });
    }
    records
}

pub(super) fn reference_circle_records(scan: &ContainerScan) -> Vec<CreoReferenceCircleRecord> {
    let mut records = Vec::new();
    for circle in &scan.references.circles {
        let id = format!("creo:mdl_ref_info:arc_z_record#{}", circle.offset);
        records.push(CreoReferenceCircleRecord {
            id,
            entity_id: circle.entity_id,
            center: circle.center().get().into(),
            center_source: if circle.center_stored() {
                "stored"
            } else {
                "endpoint_midpoint"
            },
            radius: circle.radius().get(),
            axis: (*circle.axis().as_raw()).into(),
            endpoints: [circle.start().get().into(), circle.end().get().into()],
            offset: circle.offset,
        });
    }
    records
}

pub(super) fn reference_conic_records<'a>(
    scan: &'a ContainerScan<'_>,
) -> Vec<CreoReferenceConicRecord<'a>> {
    let mut records = Vec::new();
    for conic in &scan.references.conics {
        let id = format!("creo:mdl_ref_info:conic_record#{}", conic.offset);
        records.push(CreoReferenceConicRecord {
            id,
            entity_id: conic.entity_id,
            type_id: conic.type_id,
            flip: conic.flip,
            endpoints: [conic.start.get().into(), conic.end.get().into()],
            parameter_interval: [
                conic
                    .parameter_start
                    .map(cadmpeg_ir::scalar::FiniteReal::get),
                conic.parameter_end.map(cadmpeg_ir::scalar::FiniteReal::get),
            ],
            coefficients: [conic.coefficient_1.get(), conic.coefficient_2.get()],
            local_system: conic.local_system.map(cadmpeg_ir::units::FiniteVector::get),
            body: (&conic.body).into(),
            offset: conic.offset,
        });
    }
    records
}

pub(super) fn reference_ellipse_records(scan: &ContainerScan) -> Vec<CreoReferenceEllipseRecord> {
    let mut records = Vec::new();
    for ellipse in &scan.references.ellipses {
        let id = format!("creo:mdl_ref_info:ellipse_carrier#{}", ellipse.offset);
        let source_conic_id = format!("creo:mdl_ref_info:conic_record#{}", ellipse.offset);
        records.push(CreoReferenceEllipseRecord {
            id,
            source_conic_id,
            source_entity_id: ellipse.source_entity_id,
            center: ellipse.center().get().into(),
            axis: (*ellipse.axis().as_raw()).into(),
            major_direction: (*ellipse.major_direction().as_raw()).into(),
            major_radius: ellipse.major_radius().get(),
            minor_radius: ellipse.minor_radius().get(),
            offset: ellipse.offset,
        });
    }
    records
}

struct HexDigest([u8; 32]);

impl std::fmt::Display for HexDigest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const DIGITS: [char; 16] = [
            '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
        ];
        for byte in self.0 {
            formatter.write_char(DIGITS[usize::from(byte >> 4)])?;
            formatter.write_char(DIGITS[usize::from(byte & 0x0f)])?;
        }
        Ok(())
    }
}

pub(super) fn expanded_section_records(scan: &ContainerScan) -> Vec<CreoExpandedSectionRecord> {
    let mut records = Vec::new();
    for section in &scan.framing.expanded_sections {
        let id = format!(
            "creo:container:expanded_section#{}:{}",
            section.name, section.source_offset
        );
        let name = section.name.clone();
        let sha256 = format!("{}", HexDigest(sha256(&section.data)));
        records.push(CreoExpandedSectionRecord {
            id,
            name,
            source_offset: section.source_offset,
            compressed_length: section.compressed_length,
            expanded_length: section.data.len(),
            sha256,
        });
    }
    records
}

#[derive(Serialize)]
pub(super) struct CreoFcCurveCoordinateRecord<'a> {
    pub(super) id: String,
    curve_id: u32,
    subtype: u8,
    body: NativeBytes<&'a [u8]>,
    values_mm: &'a [f64],
    tokens: &'a [FcCurveCoordinateToken],
    opaque_spans: &'a [FcCurveOpaqueSpan],
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoPrototypePcurveRecord<'a> {
    id: String,
    curve_id: u32,
    face_0_endpoints: [[f64; 2]; 2],
    face_1_endpoints: [[f64; 2]; 2],
    offset: usize,
    source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoCurvePrototypeTopologyRecord<'a> {
    id: String,
    curve_id: u32,
    faces: [u32; 2],
    next_edges: [u32; 2],
    offset: usize,
    source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoCurvePrototypeRecord<'a> {
    pub(super) id: String,
    curve_id: u32,
    type_byte: u8,
    generating_feature_id: Option<u32>,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoPlaneLocalSystemRecord<'a> {
    id: String,
    surface_id: u32,
    body: NativeBytes<&'a [u8]>,
    slots: &'a [Option<f64>],
    origin: Option<[f64; 3]>,
    u_axis: Option<[f64; 3]>,
    normal: Option<[f64; 3]>,
    classification: &'static str,
    row_offset: usize,
    offset: usize,
    source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoPlaneEnvelopeRecord<'a> {
    id: String,
    surface_id: u32,
    body: NativeBytes<&'a [u8]>,
    envelope: CreoPlaneEnvelope,
    corner_coordinate_equal: [Option<bool>; 3],
    scalar_tokens: &'a [NativeBytes<Vec<u8>>],
    row_offset: usize,
    offset: usize,
    source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoOutlinePlaneRecord<'a> {
    id: String,
    surface_id: u32,
    origin: [f64; 3],
    normal: [f64; 3],
    u_axis: [f64; 3],
    offset: usize,
    source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoDatumPlaneRecord<'a> {
    id: String,
    datum_id: u32,
    owner_feature_id: u32,
    normal: [f64; 3],
    plane_offset: f64,
    corners: [[Option<f64>; 3]; 2],
    offset: usize,
    source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoDatumCylinderRecord<'a> {
    id: String,
    datum_id: u32,
    owner_feature_id: u32,
    reversed: bool,
    origin: [f64; 3],
    axis: [f64; 3],
    ref_direction: [f64; 3],
    radius: f64,
    length: Option<f64>,
    offset: usize,
    source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureSectionTransformRecord<'a> {
    id: String,
    definition_id: u32,
    owner_feature_id: Option<u32>,
    origin: [f64; 3],
    u_axis: [f64; 3],
    v_axis: [f64; 3],
    normal: [f64; 3],
    offset: usize,
    source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoFeaturePlacementInstructionRecord<'a> {
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
    source_section: &'a str,
}

pub(super) fn feature_entity_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeatureEntityRecord<'a>> {
    let mut records = Vec::new();
    for entity in &scan.features.entities {
        let id = format!("creo:allfeatur:entity#{}", entity.entity_id);
        records.push(CreoFeatureEntityRecord {
            id,
            entity_id: entity.entity_id,
            type_byte: entity.type_byte,
            name: &entity.name,
            offset: entity.offset,
        });
    }
    records
}

pub(super) fn feature_entity_reference_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureEntityReferenceRecord> {
    let mut records = Vec::new();
    for reference in &scan.features.entity_references {
        let id = format!("creo:allfeatur:entity_reference#{}", reference.offset);
        records.push(CreoFeatureEntityReferenceRecord {
            id,
            source_entity_id: reference.source_entity_id,
            target_entity_id: reference.target_entity_id,
            target_resolved: cadmpeg_core::decode::index_from_u32(reference.target_entity_id)
                < scan.features.entities.len(),
            offset: reference.offset,
        });
    }
    records
}

pub(super) fn feature_entity_table_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureEntityTableRecord> {
    let mut records = Vec::new();
    for table in &scan.features.entity_tables {
        let mut entry_ids = Vec::new();
        let mut entries = Vec::new();
        let mut surface_ids = Vec::new();
        let mut non_surface_entity_ids = Vec::new();
        for entry in &table.entries {
            entry_ids.push(entry.entity_id);
            entries.push(CreoFeatureEntityTableEntryRecord {
                entity_id: entry.entity_id,
                class_id: entry.class_id(),
                source_entity_id: entry.source_entity_id(),
                related_entity_id: entry.related_entity_id(),
                related_entity_state: entry.related_entity_state(),
                prefixed: entry.prefixed,
                offset: entry.offset,
                end_offset: entry.end_offset,
            });
            if table.contains_surface_id(entry.entity_id) {
                surface_ids.push(entry.entity_id);
            } else {
                non_surface_entity_ids.push(entry.entity_id);
            }
        }
        let id = format!("creo:allfeatur:entity_table#{}", table.offset);
        records.push(CreoFeatureEntityTableRecord {
            id,
            owner_feature_id: table.feature_id,
            table_class_id: table.table_class_id,
            entry_ids,
            entries,
            surface_ids,
            non_surface_entity_ids,
            offset: table.offset,
        });
    }
    records
}

#[cfg(test)]
mod feature_entity_table_record_tests {
    use super::feature_entity_table_records;
    use crate::feature::entity::{dummy_table_entry, FeatureEntityTable};
    use std::collections::BTreeSet;

    fn scan_with_table() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.features.entity_tables.push(FeatureEntityTable::new(
            4,
            29,
            vec![dummy_table_entry(7), dummy_table_entry(9)],
            &BTreeSet::from([7]),
            12,
        ));
        scan
    }

    #[test]
    fn entity_table_record_preserves_source_order_and_partition() {
        let scan = scan_with_table();
        let records = feature_entity_table_records(&scan);
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.id, "creo:allfeatur:entity_table#12");
        assert_eq!(record.entry_ids, [7, 9]);
        assert_eq!(record.surface_ids, [7]);
        assert_eq!(record.non_surface_entity_ids, [9]);
    }
}

pub(super) fn feature_geometry_table_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeatureGeometryTableRecord<'a>> {
    let mut records = Vec::new();
    for table in &scan.features.geometry_tables {
        let id = format!("creo:feature:geometry_table#{}", table.offset);
        records.push(CreoFeatureGeometryTableRecord {
            id,
            owner_feature_id: table.feature_id,
            kind: &table.kind,
            declared_count: table.count,
            entity_class_id: table.entity_class,
            offset: table.offset,
            source_section: source_section_ref(scan, table.offset),
        });
    }
    records
}

pub(super) fn feature_loop_history_entry_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeatureLoopHistoryEntryRecord<'a>> {
    let mut records = Vec::new();
    for entry in &scan.features.loop_history_entries {
        let id = format!("creo:feature:loop_history_entry#{}", entry.offset);
        records.push(CreoFeatureLoopHistoryEntryRecord {
            id,
            owner_feature_id: entry.feature_id,
            ordinal: entry.ordinal,
            loop_id: entry.loop_id,
            field_bytes: entry,
            boundary: &entry.boundary,
            offset: entry.offset,
            end_offset: entry.end_offset,
            source_section: source_section_ref(scan, entry.offset),
        });
    }
    records
}

pub(super) fn feature_affected_id_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeatureAffectedIdsRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.features.affected_ids {
        let id = format!("creo:feature:affected_ids#{}", record.offset);
        records.push(CreoFeatureAffectedIdsRecord {
            id,
            owner_feature_id: record.feature_id,
            kind: affected_kind(record.kind),
            ids: &record.ids,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn feature_replay_affected_id_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeatureReplayAffectedIdsRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.features.replay_affected_ids {
        let id = format!("creo:feature:replay_affected_ids#{}", record.offset);
        records.push(CreoFeatureReplayAffectedIdsRecord {
            id,
            owner_feature_id: record.feature_id,
            geometry_ids: &record.geometry_ids,
            edge_ids: &record.edge_ids,
            geometry_extent: extent_source(record.geometry_extent),
            edge_extent: extent_source(record.edge_extent),
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn surface_merge_replay_affected_id_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoSurfaceMergeReplayAffectedIdsRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.features.surface_merge_replay_affected_ids {
        let id = format!(
            "creo:feature:surface_merge_replay_affected_ids#{}",
            record.offset
        );
        records.push(CreoSurfaceMergeReplayAffectedIdsRecord {
            id,
            owner_feature_id: record.feature_id,
            geometry_ids: &record.geometry_ids,
            edge_ids: &record.edge_ids,
            quilt_ids: &record.quilt_ids,
            geometry_extent: extent_source(record.geometry_extent),
            edge_extent: extent_source(record.edge_extent),
            quilt_extent: extent_source(record.quilt_extent),
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn feature_loop_restore_direction_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeatureLoopRestoreDirectionRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.features.loop_restore_directions {
        let id = format!("creo:feature:loop_restore_direction#{}", record.offset);
        records.push(CreoFeatureLoopRestoreDirectionRecord {
            id,
            owner_feature_id: record.feature_id,
            lane: match record.lane {
                crate::feature::rows::LoopRestoreDirectionLane::Primary => "primary",
                crate::feature::rows::LoopRestoreDirectionLane::Secondary => "secondary",
            },
            value: record.value,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn feature_revolution_extent_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeatureRevolutionExtentRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.features.revolution_extents {
        let id = format!("creo:feature:revolution_extent#{}", record.offset);
        records.push(CreoFeatureRevolutionExtentRecord {
            id,
            owner_feature_id: record.feature_id,
            kind: "full_turn",
            angle_radians: std::f64::consts::TAU,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn feature_choice_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeatureChoiceRecord<'a>> {
    let mut records = Vec::new();
    for choice in &scan.features.choices {
        let id = format!("creo:feature:choice#{}", choice.offset);
        records.push(CreoFeatureChoiceRecord {
            id,
            owner_feature_id: choice.feature_id,
            label: &choice.label,
            type_byte: choice.type_byte,
            payload: (&choice.payload).into(),
            payload_offset: choice.payload_offset,
            offset: choice.offset,
            source_section: source_section_ref(scan, choice.offset),
        });
    }
    records
}

pub(super) fn feature_row_records<'a>(scan: &'a ContainerScan) -> Vec<CreoFeatureRowRecord<'a>> {
    let mut records = Vec::new();
    for row in &scan.features.rows {
        let id = format!("creo:allfeatur:feature_row#{}", row.offset);
        records.push(CreoFeatureRowRecord {
            id,
            owner_feature_id: row.feature_id,
            header: (row.body.header()).into(),
            root_schema_class: row.root_schema_class.map(SchemaClass::code),
            stream_offset: row.stream_offset,
            body: (&(*row.body)).into(),
            body_offset: row.body_offset,
            offset: row.offset,
            source_section: source_section_ref(scan, row.offset),
        });
    }
    records
}

pub(super) fn depdb_recipe_row_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeatureRowRecord<'a>> {
    let mut records = Vec::new();
    for row in &scan.features.depdb_recipe_rows {
        let id = format!("creo:depdb:recipe_row#{}", row.offset);
        records.push(CreoFeatureRowRecord {
            id,
            owner_feature_id: row.feature_id,
            header: ([0; 2]).into(),
            root_schema_class: row.root_schema_class.map(SchemaClass::code),
            stream_offset: row.stream_offset,
            body: (&(*row.body)).into(),
            body_offset: row.body_offset,
            offset: row.offset,
            source_section: source_section_ref(scan, row.offset),
        });
    }
    records
}

#[cfg(test)]
mod feature_projection_tests {
    use super::{feature_geometry_table_records, feature_loop_history_entry_records};
    use crate::feature::rows::{
        dummy_loop_history_entry, FeatureGeometryTable, FeatureGeometryTableKind,
    };

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.features.geometry_tables.push(FeatureGeometryTable {
            feature_id: 7,
            kind: FeatureGeometryTableKind::DatumIds(Some(vec![4, 5])),
            count: 2,
            entity_class: 200,
            offset: 13,
        });
        scan.features
            .loop_history_entries
            .push(dummy_loop_history_entry());
        scan
    }

    #[test]
    fn borrowed_feature_projection_preserves_json() {
        let scan = scan();
        let geometry = feature_geometry_table_records(&scan);
        let history = feature_loop_history_entry_records(&scan);
        let geometry = serde_json::to_value(&geometry[0]).expect("record serializes");
        let history = serde_json::to_value(&history[0]).expect("record serializes");
        assert_eq!(geometry["entry_ids"], serde_json::json!([4, 5]));
        assert_eq!(
            history["field_bytes"],
            serde_json::json!(["01", "02", "03", "04"])
        );
    }
}

pub(super) fn feature_choice_field_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeatureChoiceFieldRecord<'a>> {
    let mut records = Vec::new();
    for field in &scan.features.choice_fields {
        let id = format!("creo:feature:choice_field#{}", field.offset);
        records.push(CreoFeatureChoiceFieldRecord {
            id,
            owner_feature_id: field.feature_id,
            choice_label: &field.choice_label,
            name: &field.name,
            type_byte: field.type_byte,
            value: match &field.value {
                crate::feature::rows::FeatureFieldValue::Empty => CreoFeatureFieldValue::Empty,
                crate::feature::rows::FeatureFieldValue::CompactInt(value) => {
                    CreoFeatureFieldValue::CompactInt { value: *value }
                }
                crate::feature::rows::FeatureFieldValue::CompactIntArray(values) => {
                    CreoFeatureFieldValue::CompactIntArray { values }
                }
                crate::feature::rows::FeatureFieldValue::EntityReference {
                    entity_id,
                    terminated,
                } => CreoFeatureFieldValue::EntityReference {
                    entity_id: *entity_id,
                    terminated: *terminated,
                },
                crate::feature::rows::FeatureFieldValue::ScalarArray {
                    dimensions,
                    count,
                    body,
                    decoded_values,
                } => CreoFeatureFieldValue::ScalarArray {
                    dimensions: *dimensions,
                    count: *count,
                    body: body.into(),
                    decoded_values: decoded_values.as_deref(),
                },
                crate::feature::rows::FeatureFieldValue::Raw(bytes) => CreoFeatureFieldValue::Raw {
                    bytes: bytes.into(),
                },
            },
            offset: field.offset,
            source_section: source_section_ref(scan, field.offset),
        });
    }
    records
}

#[cfg(test)]
mod feature_choice_field_record_tests {
    use super::feature_choice_field_records;
    use crate::feature::rows::{FeatureChoiceField, FeatureFieldValue};

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        let values = [
            FeatureFieldValue::Empty,
            FeatureFieldValue::CompactInt(7),
            FeatureFieldValue::CompactIntArray(vec![7, 8]),
            FeatureFieldValue::EntityReference {
                entity_id: 9,
                terminated: true,
            },
            FeatureFieldValue::ScalarArray {
                dimensions: 1,
                count: 2,
                body: vec![0xf9, 2],
                decoded_values: Some(vec![1.0, 2.0]),
            },
            FeatureFieldValue::Raw(vec![0xe3]),
        ];
        for (offset, value) in values.into_iter().enumerate() {
            scan.features.choice_fields.push(FeatureChoiceField {
                feature_id: 4,
                choice_label: "depth_choice".into(),
                name: "value".into(),
                type_byte: 1,
                value,
                offset,
            });
        }
        scan
    }

    #[test]
    fn borrowed_feature_choice_field_values_preserve_json() {
        let scan = scan();
        let records = feature_choice_field_records(&scan);
        let values = records
            .iter()
            .map(|record| serde_json::to_value(&record.value).expect("value serializes"))
            .collect::<Vec<_>>();
        assert_eq!(
            serde_json::Value::Array(values),
            serde_json::json!([
                {"kind":"empty"},
                {"kind":"compact_int","value":7},
                {"kind":"compact_int_array","values":[7,8]},
                {"kind":"entity_reference","entity_id":9,"terminated":true},
                {"kind":"scalar_array","dimensions":1,"count":2,"body":"f902","decoded_values":[1.0,2.0]},
                {"kind":"raw","bytes":"e3"}
            ])
        );
    }
}

pub(super) fn half_edge_records<'a>(scan: &'a ContainerScan) -> Vec<CreoHalfEdgeRecord<'a>> {
    let mut topology_rows = BTreeMap::new();
    for row in &scan.curves.topology_rows {
        topology_rows.insert(row.id, row);
    }
    let mut records = Vec::new();
    for edge in &scan.topology.half_edges {
        let Some(row) = topology_rows.get(&edge.id.curve_id) else {
            continue;
        };
        let id = format!(
            "creo:topology:half_edge#{}:{}",
            edge.id.curve_id, edge.id.side
        );
        records.push(CreoHalfEdgeRecord {
            id,
            curve_id: edge.id.curve_id,
            side: edge.id.side,
            face_id: edge.face_id.map_or(0, std::num::NonZeroU32::get),
            next: edge.next.map(half_edge_ref),
            offset: row.offset,
            source_section: source_section_ref(scan, row.offset),
        });
    }
    records
}

pub(super) fn loop_records<'a>(scan: &'a ContainerScan) -> Vec<CreoLoopRecord<'a>> {
    let mut records = Vec::new();
    for (index, record) in scan.topology.loops.iter().enumerate() {
        let id = format!("creo:topology:loop#{}", index + 1);
        records.push(CreoLoopRecord {
            id,
            face_id: record.face_id().map_or(0, std::num::NonZeroU32::get),
            half_edges: record.half_edges(),
        });
    }
    records
}

pub(super) fn loop_array_frame_records<'a>(
    scan: &'a ContainerScan,
) -> Result<Vec<CreoLoopArrayFrameRecord<'a>>, CodecError> {
    let mut counts = BTreeMap::<usize, usize>::new();
    for record in &scan.loop_arrays.records {
        let count = match counts.entry(record.frame_offset) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => entry.insert(0),
        };
        *count = count.checked_add(1).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("Creo native loop array frame counts overflows")
        })?;
    }
    let mut records = Vec::new();
    for frame in &scan.loop_arrays.frames {
        let id = format!("creo:loop_array:frame#{}", frame.offset);
        records.push(CreoLoopArrayFrameRecord {
            id,
            variant: frame.variant,
            declared_count: frame.declared_count,
            class_id: frame.class_id,
            materialized_count: counts.get(&frame.offset).copied().unwrap_or_default(),
            overfull: frame.overfull,
            offset: frame.offset,
            prototype_end: frame.prototype_end,
            end: frame.end,
            source_section: source_section_ref(scan, frame.offset),
        });
    }
    Ok(records)
}

pub(super) fn loop_array_record_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoLoopArrayRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.loop_arrays.records {
        let id = format!("creo:loop_array:record#{}", record.offset);
        records.push(CreoLoopArrayRecord {
            id,
            frame_offset: record.frame_offset,
            lo_id: record.lo_id,
            lo_type: record.lo_type,
            lo_subtype: record.lo_subtype,
            feature_id: record.feature_id,
            attributes: record.attributes,
            direction: record.direction,
            next_lo_ptr: record.next_lo_ptr,
            body: (&record.body).into(),
            offset: record.offset,
            body_offset: record.body_offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn topological_vertex_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoTopologicalVertexRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.topology.vertices {
        let id = format!("creo:topology:vertex#{}", record.id);
        records.push(CreoTopologicalVertexRecord {
            id,
            vertex_id: record.id.get(),
            half_edges: record.half_edges(),
        });
    }
    records
}

pub(super) fn half_edge_vertex_incidence_records(
    scan: &ContainerScan,
) -> Vec<CreoHalfEdgeVertexIncidenceRecord> {
    let mut records = Vec::new();
    for record in &scan.topology.half_edge_vertex_incidence {
        let id = format!(
            "creo:topology:half_edge_vertex_incidence#{}:{}",
            record.half_edge.curve_id, record.half_edge.side
        );
        records.push(CreoHalfEdgeVertexIncidenceRecord {
            id,
            half_edge: half_edge_ref(record.half_edge),
            start_vertex_id: record.start_vertex_id.get(),
            end_vertex_id: record.end_vertex_id.map(std::num::NonZeroU32::get),
        });
    }
    records
}

pub(super) fn face_component_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFaceComponentRecord<'a>> {
    let mut records = Vec::new();
    for (index, record) in scan.topology.face_components.iter().enumerate() {
        let id = format!("creo:topology:face_component#{}", index + 1);
        records.push(CreoFaceComponentRecord {
            id,
            face_ids: record.face_ids(),
            curve_ids: record.curve_ids(),
        });
    }
    records
}

#[cfg(test)]
mod topology_projection_tests {
    use super::{loop_array_record_records, loop_records};
    use crate::loop_array::LoopArrayRecord;
    use crate::topology::{HalfEdgeId, Side};
    use std::num::NonZeroU32;

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        let half_edge = HalfEdgeId {
            curve_id: 8,
            side: Side::Zero,
        };
        scan.topology.loops.push(crate::test_support::closed_loop(
            NonZeroU32::new(1),
            vec![half_edge],
        ));
        scan.loop_arrays.records.push(LoopArrayRecord {
            frame_offset: 17,
            lo_id: 3,
            lo_type: 0,
            lo_subtype: 0,
            feature_id: 2,
            attributes: 0,
            direction: 0,
            next_lo_ptr: 0,
            body: vec![0xe3],
            offset: 19,
            body_offset: 22,
        });
        scan
    }

    #[test]
    fn borrowed_topology_projection_preserves_half_edges_and_body() {
        let scan = scan();
        let loops = loop_records(&scan);
        let rows = loop_array_record_records(&scan);
        let loop_value = serde_json::to_value(&loops[0]).expect("loop serializes");
        let row_value = serde_json::to_value(&rows[0]).expect("row serializes");
        assert_eq!(
            loop_value["half_edges"],
            serde_json::json!([{"curve_id":8,"side":0}])
        );
        assert_eq!(
            crate::test_support::native_bytes(&row_value["body"]),
            [0xe3]
        );
        assert_eq!(row_value["end"], serde_json::json!(23));
    }
}

pub(super) fn fc_curve_coordinate_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFcCurveCoordinateRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.curves.fc_coordinates {
        let id = format!("creo:curve:fc_coordinates#{}", record.curve_id);
        records.push(CreoFcCurveCoordinateRecord {
            id,
            curve_id: record.curve_id,
            subtype: record.subtype,
            body: (&record.body).into(),
            values_mm: &record.values_mm,
            tokens: &record.tokens,
            opaque_spans: &record.opaque_spans,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn prototype_pcurve_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoPrototypePcurveRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.curves.prototype_pcurves {
        let id = format!("creo:curve:prototype_pcurve#{}", record.curve_id);
        records.push(CreoPrototypePcurveRecord {
            id,
            curve_id: record.curve_id,
            face_0_endpoints: record.face_0_endpoints,
            face_1_endpoints: record.face_1_endpoints,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn curve_prototype_topology_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoCurvePrototypeTopologyRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.curves.prototype_topology {
        let id = format!("creo:curve:prototype_topology#{}", record.curve_id);
        records.push(CreoCurvePrototypeTopologyRecord {
            id,
            curve_id: record.curve_id,
            faces: record.stored_face_ids(),
            next_edges: record.next_edges,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn curve_prototype_records<'a>(
    scan: &'a ContainerScan,
    prototypes: &'a [crate::curve::CurvePrototype],
    id_prefix: &str,
) -> Vec<CreoCurvePrototypeRecord<'a>> {
    let mut records = Vec::new();
    for record in prototypes {
        let id = format!("{id_prefix}#{}:{}", record.offset, record.id);
        records.push(CreoCurvePrototypeRecord {
            id,
            curve_id: record.id,
            type_byte: record.type_byte,
            generating_feature_id: record.feature_id,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn plane_local_system_records<'a>(
    scan: &'a ContainerScan,
    systems: &'a [crate::surface::PlaneLocalSystem],
    id_prefix: &str,
) -> Vec<CreoPlaneLocalSystemRecord<'a>> {
    let mut records = Vec::new();
    for record in systems {
        let frame = record.frame();
        let id = format!("{id_prefix}#{}:{}", record.offset, record.surface_id);
        records.push(CreoPlaneLocalSystemRecord {
            id,
            surface_id: record.surface_id,
            body: (&record.body).into(),
            slots: &record.slots,
            origin: frame.origin,
            u_axis: frame.u_axis(),
            normal: frame.normal(),
            classification: match record.classification {
                crate::surface::LocalSystemClassification::Simple => "simple",
                crate::surface::LocalSystemClassification::Unclassified => "unclassified",
            },
            row_offset: record.row_offset,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn plane_envelope_records<'a>(
    scan: &'a ContainerScan,
    envelopes: &'a [crate::surface::PlaneEnvelopeRecord],
    id_prefix: &str,
) -> Vec<CreoPlaneEnvelopeRecord<'a>> {
    let mut records = Vec::new();
    for record in envelopes {
        let id = format!("{id_prefix}#{}:{}", record.offset, record.surface_id);
        records.push(CreoPlaneEnvelopeRecord {
            id,
            surface_id: record.surface_id,
            body: (&record.body).into(),
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
            scalar_tokens: &record.scalar_tokens,
            row_offset: record.row_offset,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn outline_plane_records<'a>(
    scan: &'a ContainerScan,
    planes: &'a [crate::surface::OutlinePlane],
    id_prefix: &str,
) -> Vec<CreoOutlinePlaneRecord<'a>> {
    let mut records = Vec::new();
    for record in planes {
        let id = format!("{id_prefix}#{}:{}", record.offset, record.surface_id);
        records.push(CreoOutlinePlaneRecord {
            id,
            surface_id: record.surface_id,
            origin: record.origin,
            normal: record.normal(),
            u_axis: record.u_axis(),
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn datum_plane_records<'a>(scan: &'a ContainerScan) -> Vec<CreoDatumPlaneRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.planes.datums {
        let id = format!(
            "creo:datum:plane#{}:{}",
            record.offset_in_payload, record.id
        );
        records.push(CreoDatumPlaneRecord {
            id,
            datum_id: record.id,
            owner_feature_id: record.feature_id,
            normal: record.plane().normal(),
            plane_offset: record.plane().offset(),
            corners: record.corners(),
            offset: record.offset_in_payload,
            source_section: source_section_ref(scan, record.offset_in_payload),
        });
    }
    records
}

pub(super) fn datum_cylinder_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoDatumCylinderRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.planes.datum_cylinders {
        let id = format!(
            "creo:datum:cylinder#{}:{}",
            record.offset_in_payload, record.id
        );
        records.push(CreoDatumCylinderRecord {
            id,
            datum_id: record.id,
            owner_feature_id: record.feature_id,
            reversed: record.reversed,
            origin: record.frame.frame().origin(),
            axis: record.frame.frame().axis(),
            ref_direction: record.frame.frame().ref_direction(),
            radius: record.frame.radius().get(),
            length: record
                .frame
                .length()
                .map(cadmpeg_ir::scalar::PositiveLength::get),
            offset: record.offset_in_payload,
            source_section: source_section_ref(scan, record.offset_in_payload),
        });
    }
    records
}

pub(super) fn feature_section_transform_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeatureSectionTransformRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.features.section_transforms {
        let id = format!(
            "creo:feature:section_transform#{}:{}",
            record.definition_id, record.offset
        );
        records.push(CreoFeatureSectionTransformRecord {
            id,
            definition_id: record.definition_id,
            owner_feature_id: record.feature_id,
            origin: record.origin(),
            u_axis: record.u_axis(),
            v_axis: record.v_axis(),
            normal: record.normal(),
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
        .as_mut_slice()
        .sort_by(|left, right| left.id.cmp(&right.id));
    records.dedup_by(|left, right| left.id == right.id);
    records
}

pub(super) fn feature_placement_instruction_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeaturePlacementInstructionRecord<'a>> {
    let mut records = Vec::new();
    for definition in &scan.features.definitions {
        for instruction in crate::feature::definitions::placement_instructions(definition) {
            let id = format!(
                "creo:featdefs:placement_instruction#{}:{}",
                definition.identity.id(),
                instruction.offset
            );
            records.push(CreoFeaturePlacementInstructionRecord {
                id,
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
                source_section: source_section_ref(scan, instruction.offset),
            });
        }
    }
    records
}

#[cfg(test)]
mod curve_plane_projection_tests {
    use super::{fc_curve_coordinate_records, plane_envelope_records};
    use crate::curve::FcCurveCoordinates;
    use crate::surface::{PlaneEnvelope, PlaneEnvelopeRecord};

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.curves.fc_coordinates.push(FcCurveCoordinates {
            curve_id: 8,
            subtype: 1,
            body: vec![0xfc, 1],
            values_mm: vec![2.0],
            tokens: Vec::new(),
            opaque_spans: Vec::new(),
            offset: 3,
        });
        scan.planes.envelopes.push(PlaneEnvelopeRecord {
            surface_id: 3,
            body: vec![0xe3],
            envelope: PlaneEnvelope::Standard {
                bounds_2d: [[None; 2]; 2],
                corners_3d: [[None; 3]; 2],
            },
            corner_coordinate_equal: [None; 3],
            scalar_tokens: vec![vec![0xf9].into()],
            row_offset: 13,
            offset: 17,
        });
        scan
    }

    #[test]
    fn borrowed_curve_and_plane_projection_preserves_json() {
        let scan = scan();
        let fc = fc_curve_coordinate_records(&scan);
        let plane =
            plane_envelope_records(&scan, &scan.planes.envelopes, "creo:surface:plane_envelope");
        let fc = serde_json::to_value(&fc[0]).expect("record serializes");
        let plane = serde_json::to_value(&plane[0]).expect("record serializes");
        assert_eq!(crate::test_support::native_bytes(&fc["body"]), [0xfc, 1]);
        assert_eq!(fc["values_mm"], serde_json::json!([2.0]));
        assert_eq!(plane["scalar_tokens"], serde_json::json!(["f9"]));
    }
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceParameterRecord<'a> {
    pub(super) id: String,
    surface_id: u32,
    surface_type_byte: u8,
    surface_family: &'static str,
    boundary: &'static str,
    body: NativeBytes<&'a [u8]>,
    slots: &'a [SurfaceParameterScalar],
    opaque_spans: &'a [SurfaceParameterOpaqueSpan],
    scalar_frames: &'a [SurfaceParameterScalarFrame],
    terminal_scalar_frame: Option<&'a SurfaceParameterScalarFrame>,
    tabulated_cylinder_frame: Option<CreoTabulatedCylinderFrame>,
    positional_cylinder_frame: Option<CreoPositionalCylinderFrame>,
    split_cylinder_outline_bounds: Option<[[f64; 2]; 2]>,
    positional_cone_frame: Option<CreoPositionalConeFrame>,
    positional_torus_frame: Option<CreoPositionalTorusFrame>,
    torus_outline_frame: Option<CreoTorusOutlineFrame>,
    type26_five_coordinate_envelope: Option<CreoType26FiveCoordinateEnvelope>,
    type26_split_coordinate_envelope: Option<CreoType26SplitCoordinateEnvelope>,
    torus_radius_overrides: Option<CreoTorusRadiusOverrides>,
    replayed_torus_minor_radius: Option<f64>,
    cone_half_angle_override: Option<CreoConeHalfAngleOverride>,
    extrusion_direction: Option<[f64; 3]>,
    row_offset: usize,
    pub(super) body_offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceRowRecord<'a> {
    pub(super) id: String,
    surface_id: u32,
    type_byte: u8,
    surface_family: &'static str,
    surface_variant: Option<&'static str>,
    feature_id: u32,
    reversed: bool,
    boundary_type: u8,
    next_surface: u32,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceContourRecord<'a> {
    pub(super) id: String,
    surface_id: u32,
    chain_index: usize,
    curve_header_id: u32,
    trv: u8,
    parameter_envelope: [Option<f64>; 4],
    separator_reference: Option<u32>,
    body: NativeBytes<&'a [u8]>,
    pub(super) offset: usize,
    envelope_offset: usize,
    surface_row_offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoSurfacePrototypeRecord<'a> {
    pub(super) id: String,
    declared_family: &'a str,
    family: std::borrow::Cow<'a, str>,
    parameters: Vec<CreoSurfaceNamedParameterRecord<'a>>,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceNamedParameterRecord<'a> {
    pub(super) name: &'a str,
    #[serde(flatten, serialize_with = "serialize_surface_named_value")]
    pub(super) value: &'a crate::surface::SurfaceNamedValue,
    pub(super) body: NativeBytes<&'a [u8]>,
    pub(super) offset: usize,
    pub(super) value_offset: usize,
}

enum CompactValues<'a> {
    Empty,
    One(u32),
    Many(&'a [u32]),
}

impl Serialize for CompactValues<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Empty => serializer.collect_seq(std::iter::empty::<u32>()),
            Self::One(value) => serializer.collect_seq(std::iter::once(value)),
            Self::Many(values) => serializer.collect_seq(values.iter()),
        }
    }
}

enum ScalarValues<'a> {
    Empty,
    Optional(&'a [Option<f64>]),
    Sequence(&'a [f64]),
}

impl Serialize for ScalarValues<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Empty => serializer.collect_seq(std::iter::empty::<Option<f64>>()),
            Self::Optional(values) => serializer.collect_seq(values.iter()),
            Self::Sequence(values) => serializer.collect_seq(values.iter().copied().map(Some)),
        }
    }
}

enum ScalarTokens<'a> {
    Empty,
    Present(&'a [NativeBytes]),
    Missing(usize),
}

impl Serialize for ScalarTokens<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Empty => serializer.collect_seq(std::iter::empty::<NativeBytes<&[u8]>>()),
            Self::Present(tokens) => serializer.collect_seq(
                tokens
                    .iter()
                    .map(|token| NativeBytes::from(token.as_slice())),
            ),
            Self::Missing(count) => {
                let empty: &[u8] = &[];
                serializer.collect_seq(std::iter::repeat_n(NativeBytes::from(empty), *count))
            }
        }
    }
}

fn serialize_surface_named_value<S: serde::Serializer>(
    value: &crate::surface::SurfaceNamedValue,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use crate::surface::SurfaceNamedValue;
    use serde::ser::SerializeMap;
    let (kind, compact, dimensions, count, scalars, tokens, opaque) = match value {
        SurfaceNamedValue::Empty => (
            "empty",
            CompactValues::Empty,
            None,
            None,
            ScalarValues::Empty,
            ScalarTokens::Empty,
            &[][..],
        ),
        SurfaceNamedValue::CompactInt(value) => (
            "compact_int",
            CompactValues::One(*value),
            None,
            None,
            ScalarValues::Empty,
            ScalarTokens::Empty,
            &[][..],
        ),
        SurfaceNamedValue::CompactIntArray(values) => (
            "compact_int_array",
            CompactValues::Many(values),
            None,
            None,
            ScalarValues::Empty,
            ScalarTokens::Empty,
            &[][..],
        ),
        SurfaceNamedValue::ContiguousEntityReferences(ids) => (
            "contiguous_entity_references",
            CompactValues::Many(ids),
            None,
            None,
            ScalarValues::Empty,
            ScalarTokens::Empty,
            &[][..],
        ),
        SurfaceNamedValue::ScalarArray(array) => (
            "scalar_array",
            CompactValues::Empty,
            Some(array.dimensions()),
            Some(array.count()),
            ScalarValues::Optional(array.values()),
            array
                .tokens()
                .map_or(ScalarTokens::Empty, ScalarTokens::Present),
            &[][..],
        ),
        SurfaceNamedValue::CountedScalarArray(array) => (
            "counted_scalar_array",
            CompactValues::Empty,
            None,
            Some(array.count()),
            ScalarValues::Optional(array.values()),
            array.tokens().map_or(
                ScalarTokens::Missing(array.values().len()),
                ScalarTokens::Present,
            ),
            &[][..],
        ),
        SurfaceNamedValue::ScalarSequence(values) => (
            "scalar_sequence",
            CompactValues::Empty,
            None,
            None,
            ScalarValues::Sequence(values),
            ScalarTokens::Empty,
            &[][..],
        ),
        SurfaceNamedValue::Opaque(bytes) => (
            "opaque",
            CompactValues::Empty,
            None,
            None,
            ScalarValues::Empty,
            ScalarTokens::Empty,
            bytes.as_slice(),
        ),
    };
    let mut map = serializer.serialize_map(Some(7))?;
    map.serialize_entry("value_kind", kind)?;
    map.serialize_entry("compact_values", &compact)?;
    map.serialize_entry("scalar_dimensions", &dimensions)?;
    map.serialize_entry("scalar_count", &count)?;
    map.serialize_entry("scalar_values", &scalars)?;
    map.serialize_entry("scalar_tokens", &tokens)?;
    map.serialize_entry("opaque", &NativeBytes::from(opaque))?;
    map.end()
}

#[derive(Serialize)]
pub(super) struct CreoCurveParameterRecord<'a> {
    pub(super) id: String,
    curve_id: u32,
    type_byte: u8,
    body: NativeBytes<&'a [u8]>,
    #[serde(serialize_with = "serialize_curve_scalar_values")]
    scalar_values: &'a [crate::curve::CurveParameterScalar],
    #[serde(serialize_with = "serialize_curve_scalar_tokens")]
    scalar_tokens: &'a [crate::curve::CurveParameterScalar],
    #[serde(serialize_with = "serialize_curve_reference_ids")]
    skipped_references: &'a [crate::curve::CurveParameterReference],
    #[serde(serialize_with = "serialize_curve_references")]
    references: &'a [crate::curve::CurveParameterReference],
    #[serde(serialize_with = "serialize_curve_opaque_spans")]
    opaque_spans: &'a [crate::curve::CurveParameterOpaqueSpan],
    reference_geometry: [u32; 2],
    suffix: &'static str,
    suffix_candidate_count: Option<usize>,
    pub(super) offset: usize,
    body_offset: usize,
    suffix_offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoCurveTopologyRowRecord<'a> {
    pub(super) id: String,
    curve_id: u32,
    type_byte: u8,
    feature_id: u32,
    directions: NativeBytes<[u8; 2]>,
    faces: [u32; 2],
    next_edges: [u32; 2],
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoCrossSectionCurveRowRecord<'a> {
    pub(super) id: String,
    curve_id: u32,
    type_byte: u8,
    feature_id: u32,
    directions: NativeBytes<[u8; 2]>,
    suffix: crate::curve::DepdbCurveSuffix,
    body: NativeBytes<&'a [u8]>,
    #[serde(serialize_with = "serialize_curve_scalar_values")]
    scalar_values: &'a [crate::curve::CurveParameterScalar],
    #[serde(serialize_with = "serialize_curve_scalar_tokens")]
    scalar_tokens: &'a [crate::curve::CurveParameterScalar],
    #[serde(serialize_with = "serialize_curve_references")]
    references: &'a [crate::curve::CurveParameterReference],
    #[serde(serialize_with = "serialize_curve_opaque_spans")]
    opaque_spans: &'a [crate::curve::CurveParameterOpaqueSpan],
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoTabulatedCylinderCurveReplayRecord<'a> {
    pub(super) id: String,
    body: NativeBytes<&'a [u8]>,
    surface_id: u32,
    curve_id: u32,
    curve_type: u8,
    flip: u8,
    tangent_condition: u8,
    degree: u8,
    parameter_body: NativeBytes<&'a [u8]>,
    control_point_ids: [u32; 4],
    successor_reference: u32,
    control_point_bodies: &'a [NativeBytes<Vec<u8>>; 4],
    control_points: [Option<[f64; 2]>; 4],
    terminal_reference: u32,
    pub(super) offset: usize,
    surface_row_offset: usize,
    pub(super) source_section: &'a str,
}

fn serialize_curve_scalar_values<S: serde::Serializer>(
    tokens: &[crate::curve::CurveParameterScalar],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(tokens.iter().map(|token| token.value))
}

struct CurveScalarToken<'a>(&'a crate::curve::CurveParameterScalar);

impl Serialize for CurveScalarToken<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut record = serializer.serialize_struct("CreoCurveParameterScalar", 4)?;
        record.serialize_field("value", &self.0.value)?;
        record.serialize_field("raw", &NativeBytes::from(self.0.raw.as_slice()))?;
        record.serialize_field("offset", &self.0.offset)?;
        record.serialize_field("length", &self.0.raw.len())?;
        record.end()
    }
}

fn serialize_curve_scalar_tokens<S: serde::Serializer>(
    tokens: &[crate::curve::CurveParameterScalar],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(tokens.iter().map(CurveScalarToken))
}

fn serialize_curve_reference_ids<S: serde::Serializer>(
    references: &[crate::curve::CurveParameterReference],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(references.iter().map(|reference| reference.entity_id))
}

struct CurveReference<'a>(&'a crate::curve::CurveParameterReference);

impl Serialize for CurveReference<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut record = serializer.serialize_struct("CreoCurveParameterReference", 3)?;
        record.serialize_field("entity_id", &self.0.entity_id)?;
        record.serialize_field("offset", &self.0.offset)?;
        record.serialize_field("length", &self.0.length)?;
        record.end()
    }
}

fn serialize_curve_references<S: serde::Serializer>(
    references: &[crate::curve::CurveParameterReference],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(references.iter().map(CurveReference))
}

struct CurveOpaqueSpan<'a>(&'a crate::curve::CurveParameterOpaqueSpan);

impl Serialize for CurveOpaqueSpan<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut record = serializer.serialize_struct("CreoCurveParameterOpaqueSpan", 3)?;
        record.serialize_field("raw", &NativeBytes::from(self.0.raw.as_slice()))?;
        record.serialize_field("offset", &self.0.offset)?;
        record.serialize_field("length", &self.0.raw.len())?;
        record.end()
    }
}

fn serialize_curve_opaque_spans<S: serde::Serializer>(
    spans: &[crate::curve::CurveParameterOpaqueSpan],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(spans.iter().map(CurveOpaqueSpan))
}

pub(super) fn surface_row_records<'a>(
    scan: &'a ContainerScan,
    rows: &'a [crate::surface::SurfaceRow],
    namespace: &str,
) -> Vec<CreoSurfaceRowRecord<'a>> {
    let mut records = Vec::new();
    for row in rows {
        let id = format!("creo:{namespace}:surface_row#{}", row.id);
        records.push(CreoSurfaceRowRecord {
            id,
            surface_id: row.id,
            type_byte: row.kind.canonical_type_byte(),
            surface_family: surface_family(row.kind),
            surface_variant: surface_variant(row.kind),
            feature_id: row.feature_id,
            reversed: row.reversed,
            boundary_type: row.boundary_type.code(),
            next_surface: row.next_surface,
            offset: row.offset,
            source_section: source_section_ref(scan, row.offset),
        });
    }
    records
}

pub(super) fn surface_prototype_records<'a>(
    scan: &'a ContainerScan,
    prototypes: &'a [crate::surface::SurfacePrototypeRecord],
    id_namespace: &str,
) -> Vec<CreoSurfacePrototypeRecord<'a>> {
    use crate::surface::SurfacePrototypeFamily;
    let mut records = Vec::new();
    for record in prototypes {
        let id = format!("creo:{id_namespace}:surface_prototype#{}", record.offset);
        let family = match &record.family {
            SurfacePrototypeFamily::Plane => std::borrow::Cow::Borrowed("plane"),
            SurfacePrototypeFamily::Cylinder => std::borrow::Cow::Borrowed("cylinder"),
            SurfacePrototypeFamily::Cone => std::borrow::Cow::Borrowed("cone"),
            SurfacePrototypeFamily::Torus(_) => std::borrow::Cow::Borrowed("torus_or_sphere"),
            SurfacePrototypeFamily::Spline(_) => std::borrow::Cow::Borrowed("spline"),
            SurfacePrototypeFamily::Fillet(_) => std::borrow::Cow::Borrowed("fillet"),
            SurfacePrototypeFamily::Extrusion(_) => std::borrow::Cow::Borrowed("extrusion"),
            SurfacePrototypeFamily::Other(name) => std::borrow::Cow::Owned(format!("other:{name}")),
        };
        let mut parameters = Vec::new();
        for parameter in &record.parameters {
            parameters.push(CreoSurfaceNamedParameterRecord {
                name: &parameter.name,
                value: &parameter.value,
                body: (&parameter.body).into(),
                offset: parameter.offset,
                value_offset: parameter.value_offset,
            });
        }
        records.push(CreoSurfacePrototypeRecord {
            id,
            declared_family: record.family.name(),
            family,
            parameters,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn surface_contour_records<'a>(
    scan: &'a ContainerScan,
    contours: &'a [crate::surface::SurfaceContourRecord],
    namespace: &str,
) -> Vec<CreoSurfaceContourRecord<'a>> {
    let mut records = Vec::new();
    for record in contours {
        let id = format!(
            "creo:{namespace}:surface_contour#{}-{}",
            record.surface_id, record.offset
        );
        records.push(CreoSurfaceContourRecord {
            id,
            surface_id: record.surface_id,
            chain_index: record.chain_index,
            curve_header_id: record.curve_header_id,
            trv: record.trv,
            parameter_envelope: record.parameter_envelope,
            separator_reference: record.separator_reference,
            body: (&record.body).into(),
            offset: record.offset,
            envelope_offset: record.envelope_offset,
            surface_row_offset: record.surface_row_offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

#[cfg(test)]
mod surface_projection_tests {
    use super::surface_prototype_records;
    use crate::surface::arrays::{CountedScalars, DimensionedScalars};
    use crate::surface::{
        BoundaryType, SurfaceContourRecord, SurfaceKind, SurfaceNamedParameter, SurfaceNamedValue,
        SurfacePrototypeFamily, SurfacePrototypeRecord, SurfaceRow,
    };

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.rows.push(SurfaceRow {
            id: 7,
            kind: SurfaceKind::Plane,
            feature_id: 2,
            reversed: false,
            boundary_type: BoundaryType::Code01,
            next_surface: 0,
            offset: 3,
        });
        scan.surfaces.contours.push(SurfaceContourRecord {
            surface_id: 7,
            chain_index: 0,
            curve_header_id: 8,
            trv: 1,
            parameter_envelope: [None; 4],
            separator_reference: None,
            body: vec![8, 0xe3],
            offset: 5,
            envelope_offset: 6,
            surface_row_offset: 3,
        });
        let values = [
            SurfaceNamedValue::Empty,
            SurfaceNamedValue::CompactInt(4),
            SurfaceNamedValue::CompactIntArray(vec![4, 5]),
            SurfaceNamedValue::ContiguousEntityReferences(vec![7, 8]),
            SurfaceNamedValue::ScalarArray(
                DimensionedScalars::empty(1, 2).expect("test scalar grid"),
            ),
            SurfaceNamedValue::CountedScalarArray(
                CountedScalars::empty(2).expect("test scalar array"),
            ),
            SurfaceNamedValue::ScalarSequence(vec![1.0, 2.0]),
            SurfaceNamedValue::Opaque(vec![0xe3]),
        ];
        scan.surfaces
            .prototype_records
            .push(SurfacePrototypeRecord {
                family: SurfacePrototypeFamily::Plane,
                parameters: values
                    .into_iter()
                    .enumerate()
                    .map(|(offset, value)| SurfaceNamedParameter {
                        name: "parameter".into(),
                        value,
                        body: vec![0xe3],
                        offset,
                        value_offset: offset + 1,
                    })
                    .collect(),
                offset: 11,
            });
        scan
    }

    #[test]
    fn surface_named_values_preserve_json_without_copying() {
        let scan = scan();
        let records =
            surface_prototype_records(&scan, &scan.surfaces.prototype_records, "visibgeom");
        let values = records[0]
            .parameters
            .iter()
            .map(|parameter| serde_json::to_value(parameter).expect("parameter serializes"))
            .collect::<Vec<_>>();
        assert_eq!(values[1]["compact_values"], serde_json::json!([4]));
        assert_eq!(values[2]["compact_values"], serde_json::json!([4, 5]));
        assert_eq!(values[3]["compact_values"], serde_json::json!([7, 8]));
        assert_eq!(values[4]["scalar_values"], serde_json::json!([null, null]));
        assert_eq!(values[4]["scalar_tokens"], serde_json::json!([]));
        assert_eq!(values[5]["scalar_tokens"], serde_json::json!(["", ""]));
        assert_eq!(values[6]["scalar_values"], serde_json::json!([1.0, 2.0]));
        assert_eq!(
            crate::test_support::native_bytes(&values[7]["opaque"]),
            [0xe3]
        );
    }
}

struct CurveOccurrenceIdentity {
    curve_id: u32,
    offset: usize,
    occurrence_count: usize,
}

impl std::fmt::Display for CurveOccurrenceIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.occurrence_count == 1 {
            write!(formatter, "{}", self.curve_id)
        } else {
            // The separator sorts before a decimal digit, preserving the
            // source-order identity when repeated IDs have decimal prefixes.
            write!(formatter, "{}-{:020}", self.curve_id, self.offset)
        }
    }
}

fn curve_id_counts(ids: impl IntoIterator<Item = u32>) -> Result<BTreeMap<u32, usize>, CodecError> {
    let mut counts = BTreeMap::<u32, usize>::new();
    for id in ids {
        let count = match counts.entry(id) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => entry.insert(0),
        };
        *count = (*count)
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("Creo curve identity count overflows"))?;
    }
    Ok(counts)
}

pub(super) fn curve_parameter_records<'a>(
    scan: &'a ContainerScan,
    parameters: &'a [crate::curve::CurveParameterRecord],
    id_namespace: &str,
) -> Result<Vec<CreoCurveParameterRecord<'a>>, CodecError> {
    let counts = curve_id_counts(parameters.iter().map(|record| record.curve_id))?;
    let mut records = Vec::new();
    for record in parameters {
        let id = format!(
            "creo:{id_namespace}:curve_parameter#{}",
            CurveOccurrenceIdentity {
                curve_id: record.curve_id,
                offset: record.offset,
                occurrence_count: counts[&record.curve_id],
            }
        );
        records.push(CreoCurveParameterRecord {
            id,
            curve_id: record.curve_id,
            type_byte: record.type_byte,
            body: (&record.body).into(),
            scalar_values: &record.scalar_tokens,
            scalar_tokens: &record.scalar_tokens,
            skipped_references: &record.references,
            references: &record.references,
            opaque_spans: &record.opaque_spans,
            reference_geometry: record.reference_geometry,
            suffix: "unique",
            suffix_candidate_count: None,
            offset: record.offset,
            body_offset: record.body_offset,
            suffix_offset: record.suffix_offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    Ok(records)
}

pub(super) fn cross_section_curve_row_records<'a>(
    scan: &'a ContainerScan,
) -> Result<Vec<CreoCrossSectionCurveRowRecord<'a>>, CodecError> {
    let counts = curve_id_counts(scan.curves.cross_section_rows.iter().map(|row| row.id))?;
    let mut records = Vec::new();
    for row in &scan.curves.cross_section_rows {
        let id = format!(
            "creo:cross_section_geometry:curve_row#{}",
            CurveOccurrenceIdentity {
                curve_id: row.id,
                offset: row.offset,
                occurrence_count: counts[&row.id],
            }
        );
        records.push(CreoCrossSectionCurveRowRecord {
            id,
            curve_id: row.id,
            type_byte: row.type_byte,
            feature_id: row.feature_id,
            directions: (row.directions).into(),
            suffix: row.suffix,
            body: (&row.body).into(),
            scalar_values: &row.scalar_tokens,
            scalar_tokens: &row.scalar_tokens,
            references: &row.references,
            opaque_spans: &row.opaque_spans,
            offset: row.offset,
            source_section: source_section_ref(scan, row.offset),
        });
    }
    Ok(records)
}

pub(super) fn curve_topology_row_records<'a>(
    scan: &'a ContainerScan,
    rows: &'a [crate::curve::CurveTopologyRow],
    id_namespace: &str,
) -> Result<Vec<CreoCurveTopologyRowRecord<'a>>, CodecError> {
    let counts = curve_id_counts(rows.iter().map(|row| row.id))?;
    let mut records = Vec::new();
    for row in rows {
        let id = format!(
            "creo:{id_namespace}:curve_topology#{}",
            CurveOccurrenceIdentity {
                curve_id: row.id,
                offset: row.offset,
                occurrence_count: counts[&row.id],
            }
        );
        records.push(CreoCurveTopologyRowRecord {
            id,
            curve_id: row.id,
            type_byte: row.type_byte,
            feature_id: row.feature_id,
            directions: (row.directions).into(),
            faces: row.stored_face_ids(),
            next_edges: row.next_edges,
            offset: row.offset,
            source_section: source_section_ref(scan, row.offset),
        });
    }
    Ok(records)
}

pub(super) fn tabulated_cylinder_curve_replay_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoTabulatedCylinderCurveReplayRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.curves.tabulated_cylinder_replays {
        let id = format!(
            "creo:visibgeom:tabulated_cylinder_curve_replay#{}",
            record.surface_id
        );
        records.push(CreoTabulatedCylinderCurveReplayRecord {
            id,
            body: (&record.body).into(),
            surface_id: record.surface_id,
            curve_id: record.curve_id,
            curve_type: record.curve_type,
            flip: record.flip,
            tangent_condition: record.tangent_condition,
            degree: record.degree,
            parameter_body: (&record.parameter_body).into(),
            control_point_ids: record.control_point_ids,
            successor_reference: record.successor_reference,
            control_point_bodies: &record.control_point_bodies,
            control_points: record.control_points,
            terminal_reference: record.terminal_reference,
            offset: record.offset,
            surface_row_offset: record.surface_row_offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

#[cfg(test)]
mod curve_projection_tests {
    use super::{curve_parameter_records, tabulated_cylinder_curve_replay_records};
    use crate::curve::{
        CurveParameterOpaqueSpan, CurveParameterRecord, CurveParameterReference,
        CurveParameterScalar,
    };
    use crate::surface::TabulatedCylinderCurveReplay;

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.curves.parameters.push(CurveParameterRecord {
            curve_id: 8,
            type_byte: 1,
            body: vec![0xf9, 0, 0xe3],
            scalar_tokens: vec![CurveParameterScalar {
                value: 2.0,
                raw: vec![0xf9, 0],
                offset: 1,
            }],
            references: vec![CurveParameterReference {
                entity_id: 9,
                offset: 3,
                length: 2,
            }],
            opaque_spans: vec![CurveParameterOpaqueSpan {
                raw: vec![0xe3],
                offset: 5,
            }],
            reference_geometry: [0, 0],
            offset: 11,
            body_offset: 12,
            suffix_offset: 15,
        });
        scan.curves
            .tabulated_cylinder_replays
            .push(TabulatedCylinderCurveReplay {
                body: vec![0xf9, 0xe3],
                surface_id: 7,
                curve_id: 8,
                curve_type: 1,
                flip: 0,
                tangent_condition: 0,
                degree: 3,
                parameter_body: vec![0xf9],
                control_point_ids: [1, 2, 3, 4],
                successor_reference: 0,
                control_point_bodies: std::array::from_fn(|_| vec![0xe3].into()),
                control_points: [None; 4],
                terminal_reference: 0,
                offset: 23,
                surface_row_offset: 21,
            });
        scan
    }

    #[test]
    fn borrowed_curve_projection_preserves_nested_json() {
        let scan = scan();
        let parameters = curve_parameter_records(&scan, &scan.curves.parameters, "visibgeom")
            .expect("parameters are admitted");
        let replay = tabulated_cylinder_curve_replay_records(&scan);
        let parameter = serde_json::to_value(&parameters[0]).expect("parameter serializes");
        let replay = serde_json::to_value(&replay[0]).expect("replay serializes");
        assert_eq!(parameter["scalar_values"], serde_json::json!([2.0]));
        assert_eq!(
            parameter["scalar_tokens"],
            serde_json::json!([{"value":2.0,"raw":"f900","offset":1,"length":2}])
        );
        assert_eq!(parameter["skipped_references"], serde_json::json!([9]));
        assert_eq!(
            parameter["references"],
            serde_json::json!([{"entity_id":9,"offset":3,"length":2}])
        );
        assert_eq!(
            parameter["opaque_spans"],
            serde_json::json!([{"raw":"e3","offset":5,"length":1}])
        );
        assert_eq!(
            replay["control_point_bodies"],
            serde_json::json!(["e3", "e3", "e3", "e3"])
        );
    }
}

pub(super) fn surface_parameter_records<'a>(
    scan: &'a ContainerScan,
    rows: &'a [crate::surface::SurfaceRow],
    parameters: &'a [crate::surface::SurfaceParameterRecord],
    namespace: &str,
) -> Vec<CreoSurfaceParameterRecord<'a>> {
    let mut records = Vec::new();
    for record in parameters {
        let Some(row) = crate::surface::unique_surface_row(rows, record.surface_id) else {
            continue;
        };
        let surface_family = surface_family(row.kind);
        let boundary = match record.boundary {
            crate::surface::SurfaceBodyBoundary::CompoundClose => "compound_close",
            crate::surface::SurfaceBodyBoundary::NextRow => "next_row",
            crate::surface::SurfaceBodyBoundary::NamedRecord => "named_record",
            crate::surface::SurfaceBodyBoundary::SectionEnd => "section_end",
        };
        let id = format!("creo:{namespace}:surface_parameter#{}", record.surface_id);
        records.push(CreoSurfaceParameterRecord {
            id,
            surface_id: record.surface_id,
            surface_type_byte: row.kind.canonical_type_byte(),
            surface_family,
            boundary,
            body: (&record.body).into(),
            slots: &record.scalar_tokens,
            opaque_spans: &record.opaque_spans,
            scalar_frames: &record.scalar_frames,
            terminal_scalar_frame: record.terminal_scalar_frame(),
            tabulated_cylinder_frame: record.tabulated_cylinder_frame().map(|frame| {
                CreoTabulatedCylinderFrame {
                    values: frame.values().get(),
                    prefixes: frame.prefixes().into(),
                }
            }),
            positional_cylinder_frame: record.positional_cylinder_frame().map(|frame| {
                CreoPositionalCylinderFrame {
                    origin: frame.frame().origin(),
                    axis: frame.frame().axis(),
                    ref_direction: frame.frame().ref_direction(),
                    radius: frame.radius().get(),
                    length: frame.length().map(cadmpeg_ir::scalar::PositiveLength::get),
                }
            }),
            split_cylinder_outline_bounds: record.split_cylinder_outline_bounds(),
            positional_cone_frame: record.positional_cone_frame().map(|frame| {
                CreoPositionalConeFrame {
                    apex: frame.frame().origin(),
                    axis: frame.frame().axis(),
                    ref_direction: frame.frame().ref_direction(),
                    half_angle: frame.half_angle().get().get(),
                }
            }),
            positional_torus_frame: record.positional_torus_frame().map(|frame| {
                CreoPositionalTorusFrame {
                    center: frame.frame().origin(),
                    axis: frame.frame().axis(),
                    ref_direction: frame.frame().ref_direction(),
                    major_radius: frame.major_radius().get(),
                    minor_radius: frame.minor_radius().get(),
                }
            }),
            torus_outline_frame: record
                .torus_outline_frame()
                .map(|frame| CreoTorusOutlineFrame {
                    values: frame.values,
                    selector: frame.selector,
                    offset: frame.offset,
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
                    radians: half_angle.radians.get().get(),
                    offset: half_angle.offset,
                }
            }),
            extrusion_direction: record.extrusion_direction(),
            row_offset: record.offset,
            body_offset: record.body_offset,
            source_section: source_section_ref(scan, record.body_offset),
        });
    }
    records
}

#[cfg(test)]
mod surface_parameter_projection_tests {
    use super::surface_parameter_records;
    use crate::surface::{
        BoundaryType, SurfaceBodyBoundary, SurfaceKind, SurfaceParameterCarrier,
        SurfaceParameterOpaqueSpan, SurfaceParameterRecord, SurfaceParameterScalar,
        SurfaceParameterScalarFrame, SurfaceRow,
    };

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.rows.push(SurfaceRow {
            id: 7,
            kind: SurfaceKind::Plane,
            feature_id: 2,
            reversed: false,
            boundary_type: BoundaryType::Code01,
            next_surface: 0,
            offset: 3,
        });
        let token = SurfaceParameterScalar {
            value: Some(1.0),
            raw: vec![0xf9, 0].into(),
            offset: 0,
        };
        scan.surfaces.parameters.push(SurfaceParameterRecord {
            surface_id: 7,
            body: vec![0xf9, 0],
            scalar_tokens: vec![token.clone()],
            opaque_spans: vec![SurfaceParameterOpaqueSpan {
                raw: vec![0xe3].into(),
                offset: 2,
            }],
            scalar_frames: vec![SurfaceParameterScalarFrame {
                offset: 0,
                slots: vec![token],
            }],
            carrier: SurfaceParameterCarrier::Unresolved(SurfaceKind::Plane),
            boundary: SurfaceBodyBoundary::CompoundClose,
            offset: 3,
            body_offset: 4,
        });
        scan
    }

    #[test]
    fn borrowed_surface_parameter_preserves_nested_json() {
        let scan = scan();
        let records = surface_parameter_records(
            &scan,
            &scan.surfaces.rows,
            &scan.surfaces.parameters,
            "visibgeom",
        );
        let value = serde_json::to_value(&records[0]).expect("record serializes");
        assert_eq!(crate::test_support::native_bytes(&value["body"]), [249, 0]);
        assert_eq!(
            value["slots"],
            serde_json::json!([{"value":1.0,"raw":"f900","offset":0,"length":2}])
        );
        assert_eq!(
            value["opaque_spans"],
            serde_json::json!([{"raw":"e3","offset":2,"length":1}])
        );
    }
}

pub(super) fn feature_operation_state_records<'a>(
    scan: &'a ContainerScan<'_>,
) -> Result<Vec<CreoFeatureOperationState<'a>>, CodecError> {
    let mut current_offsets = BTreeMap::new();
    for state in &scan.features.operations {
        current_offsets.insert(state.feature_id, state.offset);
    }
    let mut ordinals = BTreeMap::<u32, usize>::new();
    let mut records = Vec::new();
    for state in &scan.features.operation_states {
        let state_ordinal = ordinals.get(&state.feature_id).copied().unwrap_or_default();
        let next_ordinal = state_ordinal.checked_add(1).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("Creo native feature state ordinal overflows")
        })?;
        ordinals.insert(state.feature_id, next_ordinal);
        let name = CreoOperationNameRecord {
            display_name_stored: state.name.display_name_stored(),
            stored_name: state
                .name
                .stored_name_bytes()
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned()),
            stored_name_bytes: state.name.stored_name_bytes().map(NativeBytes::from),
            identifier_keyword: state.name.identifier_keyword(),
            stored_name_prefix: state
                .name
                .stored_name_prefix()
                .map(|prefix| format!("{}", char::from(prefix))),
        };
        let id = format!(
            "creo:mdlstatus:feature_state#{}:{state_ordinal}",
            state.feature_id
        );
        records.push(CreoFeatureOperationState {
            id,
            feature_id: state.feature_id,
            state_ordinal,
            current: !state.display_state_conflict
                && current_offsets.get(&state.feature_id) == Some(&state.offset),
            family: state.kind.as_str(),
            name,
            recipe: state
                .recipe
                .candidate()
                .map(crate::feature::operations::FeatureRecipe::name),
            recipe_conflict: state.recipe.is_conflicting().then_some(true),
            display_state_conflict: state.display_state_conflict.then_some(true),
            root_schema_class: state.root_schema_class().map(SchemaClass::code),
            parent_feature_id: state.parent_feature_id(),
            offset: state.offset,
            state_offset: state.state_offset,
        });
    }
    Ok(records)
}

pub(super) fn feature_reference_name_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureReferenceNameRecord> {
    let mut records = Vec::new();
    for record in &scan.features.reference_names {
        let id = format!("creo:mdlrefinfo:feature_name#{}", record.offset);
        let name = String::from_utf8_lossy(&record.name_bytes).into_owned();
        let name_bytes = record.name_bytes.clone();
        records.push(CreoFeatureReferenceNameRecord {
            id,
            owner_feature_id: record.feature_id,
            name,
            name_bytes: (name_bytes).into(),
            own_reference_id: record.own_reference_id,
            reference_type: record.reference_type,
            offset: record.offset,
        });
    }
    records
}

#[derive(Serialize)]
pub(super) struct CreoPcurveEndpointRecord {
    pub(super) id: String,
    curve_id: u32,
    faces: [u32; 2],
    face_0_endpoints: [[f64; 2]; 2],
    face_1_endpoints: [[f64; 2]; 2],
    source_form: &'static str,
}

pub(super) fn pcurve_endpoint_records(
    scan: &ContainerScan,
) -> Vec<(CreoPcurveEndpointRecord, usize)> {
    let mut records = Vec::new();
    for pcurve in &scan.curves.pcurves {
        let id = format!("creo:visibgeom:pcurve_endpoints#{}", pcurve.curve_id);
        records.push((
            CreoPcurveEndpointRecord {
                id,
                curve_id: pcurve.curve_id,
                faces: pcurve.stored_face_ids(),
                face_0_endpoints: pcurve.face_0_endpoints,
                face_1_endpoints: pcurve.face_1_endpoints,
                source_form: "positional",
            },
            pcurve.offset,
        ));
    }
    for pcurve in &scan.curves.bound_prototype_pcurves {
        let id = format!(
            "creo:visibgeom:prototype_pcurve_endpoints#{}",
            pcurve.curve_id
        );
        records.push((
            CreoPcurveEndpointRecord {
                id,
                curve_id: pcurve.curve_id,
                faces: pcurve.stored_face_ids(),
                face_0_endpoints: pcurve.face_0_endpoints,
                face_1_endpoints: pcurve.face_1_endpoints,
                source_form: "prototype",
            },
            pcurve.offset,
        ));
    }
    records.as_mut_slice().sort_by_key(|(_, left)| *left);
    records
}

fn curve_expression_assignment_projection(
    assignment: &crate::curve::CurveExpressionAssignment,
) -> CreoCurveExpressionAssignment<'_> {
    CreoCurveExpressionAssignment {
        target: &assignment.target,
        expression: &assignment.expression,
        dependencies: &assignment.dependencies,
        value: assignment.value.as_ref(),
        activation: assignment.activation.token(),
        offset: assignment.offset,
    }
}

pub(super) fn curve_expression_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoCurveExpressionRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.curves.expressions {
        let id = curve_expression_record_id(record);
        let mut lines = Vec::new();
        for line in &record.lines {
            lines.push(CreoCurveExpressionLine {
                text: &line.text,
                offset: line.offset,
            });
        }
        let mut assignments = Vec::new();
        for assignment in &record.assignments {
            assignments.push(curve_expression_assignment_projection(assignment));
        }
        let mut solve_blocks = Vec::new();
        for block in &record.solve_blocks {
            let mut equations = Vec::new();
            for equation in &block.equations {
                equations.push(CreoCurveExpressionEquation {
                    left: &equation.left,
                    right: &equation.right,
                    dependencies: &equation.dependencies,
                    offset: equation.offset,
                });
            }
            let mut block_assignments = Vec::new();
            for assignment in &block.assignments {
                block_assignments.push(curve_expression_assignment_projection(assignment));
            }
            solve_blocks.push(CreoCurveExpressionSolveBlock {
                equations,
                assignments: block_assignments,
                variables: &block.unknowns,
                solutions: &block.unknowns,
                offset: block.offset,
                for_offset: block.for_offset,
            });
        }
        records.push(CreoCurveExpressionRecord {
            id,
            entity_id: record.entity_id,
            backup: record.backup,
            local_system: record.local_system.as_ref().map(|frame| {
                CreoCurveExpressionLocalSystem {
                    dimensions: frame.dimensions,
                    count: frame.count,
                    body: (&frame.body).into(),
                    explicit_slots: frame
                        .explicit_slots
                        .map(cadmpeg_ir::units::FiniteVector::get),
                    offset: frame.offset,
                }
            }),
            lines,
            assignments,
            solve_blocks,
            unresolved_solve_control: record.unresolved_solve_control,
            prohibited_constructs: &record.prohibited_constructs,
        });
    }
    records
}

#[cfg(test)]
mod curve_expression_projection_tests {
    use super::curve_expression_records;
    use crate::curve::{
        CurveExpressionActivation, CurveExpressionAssignment, CurveExpressionEquation,
        CurveExpressionLine, CurveExpressionLocalSystem, CurveExpressionRecord,
        CurveExpressionSolveBlock, CurveExpressionTarget, CurveExpressionValue, SolveUnknown,
    };

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        let assignment = CurveExpressionAssignment {
            target: CurveExpressionTarget::Parameter {
                name: "p".into(),
                declared_unit: None,
            },
            expression: "2".into(),
            dependencies: vec!["q".into()],
            value: Some(CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"),
            )),
            activation: CurveExpressionActivation::Active,
            offset: 6,
        };
        scan.curves.expressions.push(CurveExpressionRecord {
            entity_id: 7,
            backup: false,
            local_system: Some(CurveExpressionLocalSystem {
                dimensions: 4,
                count: 3,
                body: vec![0xf9],
                explicit_slots: None,
                offset: 4,
            }),
            lines: vec![CurveExpressionLine {
                text: "p=2".into(),
                offset: 5,
            }],
            assignments: vec![assignment.clone()],
            solve_blocks: vec![CurveExpressionSolveBlock {
                equations: vec![CurveExpressionEquation {
                    left: "p".into(),
                    right: "q".into(),
                    dependencies: vec!["q".into()],
                    offset: 7,
                }],
                assignments: vec![assignment],
                unknowns: vec![SolveUnknown {
                    name: "q".into(),
                    solution: Some(CurveExpressionValue::Number(
                        cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"),
                    )),
                }],
                offset: 8,
                for_offset: 9,
            }],
            unresolved_solve_control: false,
            prohibited_constructs: vec!["bad".into()],
            offset: 3,
            expression_offset: 5,
        });
        scan
    }

    #[test]
    fn borrowed_curve_expression_preserves_nested_json() {
        let scan = scan();
        let records = curve_expression_records(&scan);
        let value = serde_json::to_value(&records[0]).expect("record serializes");
        assert_eq!(
            crate::test_support::native_bytes(&value["local_system"]["body"]),
            [249]
        );
        assert_eq!(value["lines"][0]["text"], "p=2");
        assert_eq!(value["assignments"][0]["target"]["kind"], "parameter");
        assert_eq!(
            value["solve_blocks"][0]["variables"],
            serde_json::json!(["q"])
        );
        assert_eq!(
            value["solve_blocks"][0]["solutions"],
            serde_json::json!([2.0])
        );
        assert_eq!(value["prohibited_constructs"], serde_json::json!(["bad"]));
    }
}

pub(super) fn sketch_records<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoSketchRecord<'a>>, cadmpeg_core::CodecError> {
    let mut records = Vec::new();
    for definition in &scan.features.definitions {
        if !feature_definition_has_sketch_design(definition) {
            continue;
        }
        let id = if scan
            .features
            .definitions
            .iter()
            .filter(|candidate| candidate.identity.id() == definition.identity.id())
            .count()
            != 1
            || (definition.identity.schema_id().is_none()
                && definition.identity.owner_feature_id().is_none())
        {
            format!("creo:featdefs:sketch#offset:{}", definition.offset)
        } else {
            format!("creo:featdefs:sketch#{}", definition.identity.id())
        };
        let source_section = source_section_ref(scan, definition.offset).to_owned();
        let record = CreoSketchRecord {
            id,
            definition_id: definition.identity.id(),
            owner_feature_id: definition.identity.owner_feature_id(),
            source_section,
            offset: definition.offset,
            section_3d: definition
                .section_3d
                .as_ref()
                .map(|section| CreoSketchSection3d {
                    sketch_plane_entity_id: section.sketch_plane_entity_id,
                    sketch_plane_flip: section.sketch_plane_flip.map(binary_flag_value),
                    reference_planes: &section.reference_planes,
                    reference_plane_datum_geometry_id: section.reference_plane_datum_geometry_id,
                    orientation: CreoSketchSectionOrientation {
                        section_flip: section.orientation.section_flip.map(binary_flag_value),
                        reference_type: section.orientation.reference_type,
                        segment_id: section.orientation.segment_id,
                        reference_flip: section.orientation.reference_flip.map(binary_flag_value),
                    },
                    dimension_ids: &section.dimension_ids,
                    offset: section.offset,
                }),
            table_headers: sketch_table_headers(definition)?,
            section_points: sketch_section_point_records(definition),
            solved_external_ids: definition
                .trim_entities
                .iter()
                .flat_map(|table| table.solved_external_ids.iter().copied())
                .collect::<Vec<_>>(),
            variables: {
                let resolved_coordinates = resolved_section_coordinates(ctx, definition)?;
                let resolved_radii = resolved_section_radii(ctx, definition)?;
                let resolved_scalars = resolved_section_scalar_values(ctx, definition)?;
                (definition.variables.iter().flat_map(|table| &table.rows))
                    .map(|row| {
                        Ok::<_, CodecError>(CreoSketchVariable {
                            variable_type: row.variable_type.code(),
                            key: row.key,
                            value: row.value,
                            value_body: row.value_body.clone().into(),
                            guess: row.guess,
                            guess_body: row.guess_body.clone().into(),
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
                    })
                    .collect::<Result<Vec<_>, _>>()?
            },
            equations: (crate::feature::definitions::equation_table(
                &definition.body,
                0,
                definition.body.len(),
            )
            .into_iter()
            .flat_map(|table| table.rows))
            .map(|equation| {
                Ok::<_, CodecError>(CreoSketchEquation {
                    equation_id: equation.equation_id,
                    function_id: equation.function_id,
                    explicit_argument_count: equation.explicit_argument_count,
                    arguments: equation.arguments,
                    arguments_body: equation.arguments_body.into(),
                    auxiliary_body: equation.auxiliary_body.into(),
                    body: equation.body.into(),
                    offset: definition.body_position(equation.offset)?.source()?.get(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
            segments: (definition
                .segments
                .iter()
                .flat_map(|table| table.rows.ordinary()))
            .map(|segment| {
                Ok::<_, CodecError>(CreoSketchSegment {
                    external_id: segment.external_id,
                    kind: match segment.kind {
                        crate::feature::definitions::FeatureSegmentKind::Line(_) => "line",
                        crate::feature::definitions::FeatureSegmentKind::Arc(_) => "arc",
                        crate::feature::definitions::FeatureSegmentKind::Point(_) => "point",
                    },
                    point_ids: segment.point_ids(),
                    center_id: segment.center_id,
                    directions: segment.directions,
                    arc_orientation: segment.arc_orientation,
                    vertical_horizontal_constraint: segment.vertical_horizontal,
                    radius_dimension_id: segment.radius_ref,
                    secondary_radius_dimension_id: segment.radius2_ref,
                    body: segment.body.clone().into(),
                    offset: segment.offset,
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
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
                .collect::<Vec<_>>(),
            point_segments: definition
                .segments
                .iter()
                .flat_map(|table| table.rows.points())
                .map(|segment| CreoSketchPointSegment {
                    external_id: segment.external_id,
                    point_id: segment.point_id,
                    offset: segment.offset,
                })
                .collect::<Vec<_>>(),
            centered_line_segments: definition
                .segments
                .iter()
                .flat_map(|table| table.rows.centered_lines())
                .map(|segment| CreoSketchCenteredLineSegment {
                    external_id: segment.external_id,
                    center_id: segment.center_id,
                    offset: segment.offset,
                })
                .collect::<Vec<_>>(),
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
                .collect::<Vec<_>>(),
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
                .collect::<Vec<_>>(),
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
                .collect::<Vec<_>>(),
            opaque_segments: (definition
                .segments
                .iter()
                .flat_map(|table| table.rows.opaque()))
            .map(|segment| {
                Ok::<_, CodecError>(CreoSketchOpaqueSegment {
                    external_id: segment.external_id,
                    kind: segment.kind,
                    point_ids: segment.point_ids,
                    center_id: segment.center_id,
                    directions: segment.directions,
                    arc_orientation: segment.arc_orientation,
                    vertical_horizontal_constraint: segment.vertical_horizontal,
                    radius_dimension_id: segment.radius_ref,
                    secondary_radius_dimension_id: segment.radius2_ref,
                    body: segment.body.clone().into(),
                    offset: segment.offset,
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
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
                        crate::feature::definitions::TrimEntityKind::Line => "line",
                        crate::feature::definitions::TrimEntityKind::Arc { .. } => "arc",
                    },
                    offset: entity.offset,
                })
                .collect::<Vec<_>>(),
            trim_vertices: (definition
                .trim_vertices
                .iter()
                .flat_map(|table| &table.rows))
            .map(|vertex| {
                Ok::<_, CodecError>(CreoSketchTrimVertex {
                    vertex_id: vertex.vertex_id,
                    entities: vertex.entities.clone(),
                    section_coordinates: vertex.section_coordinates.map(|point| {
                        let point = point.get();
                        [point.u, point.v]
                    }),
                    offset: vertex.offset,
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
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
                .collect::<Vec<_>>(),
            saved_entities: definition
                .saved_section
                .iter()
                .flat_map(|section| &section.entities)
                .map(|entity| match entity {
                    crate::feature::definitions::FeatureSavedEntity::Line(line) => {
                        CreoSketchSavedEntity::Line {
                            entity_id: line.entity_id,
                            references: &line.references,
                            attributes: &line.attributes,
                            endpoints: line.endpoints,
                            body: (&line.body).into(),
                            offset: line.offset,
                        }
                    }
                    crate::feature::definitions::FeatureSavedEntity::Arc(arc) => {
                        CreoSketchSavedEntity::Arc {
                            entity_id: arc.entity_id,
                            center: arc.center,
                            radius: arc.radius,
                            endpoints: arc.endpoints,
                            parameters: arc.parameters,
                            body: (&arc.body).into(),
                            offset: arc.offset,
                        }
                    }
                    crate::feature::definitions::FeatureSavedEntity::Circle(circle) => {
                        CreoSketchSavedEntity::Circle {
                            entity_id: circle.entity_id,
                            center: circle.center,
                            radius: circle.radius,
                            body: (&circle.body).into(),
                            offset: circle.offset,
                        }
                    }
                    crate::feature::definitions::FeatureSavedEntity::Conic(conic) => {
                        CreoSketchSavedEntity::Conic {
                            entity_id: conic.entity_id,
                            endpoints: conic.endpoints,
                            parameters: conic.parameters,
                            coefficients: conic.coefficients,
                            local_system: conic
                                .local_system
                                .map(cadmpeg_ir::units::FiniteVector::get),
                            body: (&conic.body).into(),
                            offset: conic.offset,
                        }
                    }
                    crate::feature::definitions::FeatureSavedEntity::Spline(spline) => {
                        CreoSketchSavedEntity::Spline {
                            entity_id: spline.entity_id,
                            declared_point_count: spline.declared_point_count,
                            interpolation_points: &spline.interpolation_points,
                            interpolation_points_body: (&spline.interpolation_points_body).into(),
                            endpoint_tangents: crate::decode::native_records::SplineTangents(
                                spline.endpoint_tangents.as_ref(),
                            ),
                            parameters: crate::decode::native_records::SplineParameters(
                                spline.parameters.as_ref(),
                            ),
                            offset: spline.offset,
                        }
                    }
                    crate::feature::definitions::FeatureSavedEntity::Dummy(dummy) => {
                        CreoSketchSavedEntity::Dummy {
                            entity_id: dummy.entity_id,
                            body: (&dummy.body).into(),
                            offset: dummy.offset,
                        }
                    }
                })
                .collect::<Vec<_>>(),
            dimensions: (definition.dimensions.iter().flat_map(|table| &table.rows))
                .map(|dimension| {
                    Ok::<_, CodecError>(CreoSketchDimension {
                        external_id: dimension.external_id,
                        dimension_type: dimension.dimension_type,
                        value: match &dimension.value {
                            crate::feature::definitions::DimensionValue::Resolved(value) => {
                                crate::feature::definitions::DimensionValue::Resolved(*value)
                            }
                            crate::feature::definitions::DimensionValue::UnresolvedToken(token) => {
                                crate::feature::definitions::DimensionValue::UnresolvedToken(
                                    token.clone(),
                                )
                            }
                            crate::feature::definitions::DimensionValue::Undefined => {
                                crate::feature::definitions::DimensionValue::Undefined
                            }
                        },
                        value_body: dimension.value_body.clone().into(),
                        unit: match dimension.unit() {
                            crate::feature::definitions::DimensionUnit::Radians => "radians",
                            crate::feature::definitions::DimensionUnit::Millimeters => {
                                "millimeters"
                            }
                            crate::feature::definitions::DimensionUnit::SchemaDefined => {
                                "schema_defined"
                            }
                        },
                        direction_byte: dimension.direction_byte,
                        auxiliary_value: dimension.auxiliary_value,
                        auxiliary_body: dimension.auxiliary_body.clone().into(),
                        references: dimension
                            .references
                            .as_ref()
                            .map(
                                |table| -> Result<CreoSketchDimensionReferenceTable, CodecError> {
                                    Ok::<_, CodecError>(CreoSketchDimensionReferenceTable {
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
                                            .collect::<Vec<_>>(),
                                        offset: table.offset,
                                    })
                                },
                            )
                            .transpose()?,
                        offset: dimension.offset,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            relations: (definition.relations.iter().flat_map(|table| &table.rows))
                .map(|relation| {
                    Ok::<_, CodecError>(CreoSketchRelation {
                        relation_id: relation.relation_id,
                        used: relation.used,
                        operands: relation.operands.clone().into(),
                        operand_vectors: relation.operand_vectors,
                        sign: relation.sign,
                        dimension_id: relation.dimension_id,
                        relation_type: relation.relation_type,
                        body: relation.body.clone().into(),
                        offset: relation.offset,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            skamps: (definition
                .relations
                .iter()
                .flat_map(FeatureRelationTable::skamps))
            .map(|skamp| {
                Ok::<_, CodecError>(CreoSketchSkamp {
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
                        .collect::<Vec<_>>(),
                    offset: skamp.offset,
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
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
                .collect::<Vec<_>>(),
        };
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod sketch_projection_tests {
    use super::sketch_records;
    use crate::feature::definitions::{
        DefinitionIdentity, FeatureDefinition, FeatureSection3d, FeatureSectionOrientation,
        FeatureSectionReferencePlane, FeatureVariableRow, FeatureVariableTable, ReferencePlanes,
        ScalarLane, VariableType,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    fn definition() -> FeatureDefinition {
        FeatureDefinition {
            identity: DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(7),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(FeatureSection3d {
                sketch_plane_entity_id: Some(2),
                sketch_plane_flip: None,
                reference_planes: ReferencePlanes::Named(vec![3]),
                reference_plane_datum_geometry_id: None,
                orientation: FeatureSectionOrientation::default(),
                dimension_ids: vec![4],
                offset: 2,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 1,
        }
    }

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.features.definitions.push(definition());
        scan
    }

    #[test]
    fn borrowed_sketch_section_frames_preserve_json() {
        let mut scan = scan();
        scan.features.definitions[0]
            .section_3d
            .as_mut()
            .expect("section frame")
            .reference_planes = ReferencePlanes::Positional(vec![FeatureSectionReferencePlane {
            plane_entity_id: 3,
            reference_type: Some(2),
            external_reference_id: None,
            segment_id: None,
            sub_index: None,
            reference_flip: None,
        }]);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let records = sketch_records(&ctx, &scan).expect("sketch projection is admitted");
        let value = serde_json::to_value(&records[0]).expect("record serializes");
        assert_eq!(
            value["section_3d"]["reference_plane_entity_ids"],
            serde_json::json!([3])
        );
        assert_eq!(
            value["section_3d"]["reference_plane_rows"][0]["reference_type"],
            2
        );
        assert_eq!(value["section_3d"]["dimension_ids"], serde_json::json!([4]));
        assert!(value["section_3d"]["reference_planes"].is_null());
    }

    #[test]
    fn sketch_variable_body_preserves_value_bytes() {
        let mut scan = scan();
        scan.features.definitions[0].section_3d = None;
        scan.features.definitions[0].variables = Some(FeatureVariableTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![FeatureVariableRow {
                variable_type: VariableType::Parameter,
                key: 1,
                value: ScalarLane::Value(2.0),
                value_body: vec![0xf9],
                guess: ScalarLane::Undefined,
                guess_body: vec![0xe3],
                known: None,
                homogeneity: None,
                uvar_id: None,
                offset: 3,
            }],
            offset: 2,
        });
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let records = sketch_records(&ctx, &scan).expect("service profile admits one variable");
        let value = serde_json::to_value(&records[0]).expect("record serializes");
        assert_eq!(
            crate::test_support::native_bytes(&value["variables"][0]["value_body"]),
            [249]
        );
    }
}

pub(super) fn sketch_section_point_records(
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Vec<CreoSketchSectionPoint> {
    let Some(variables) = &definition.variables else {
        return Vec::new();
    };
    let crate::feature::definitions::ReconciledPoints { points, ambiguous } =
        variables.reconciled_points();
    let mut point_ids = BTreeSet::new();
    for point_id in points.keys().copied().chain(ambiguous.iter().copied()) {
        point_ids.insert(point_id);
    }
    point_ids
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
        .collect::<Vec<_>>()
}

pub(super) fn feature_definition_records<'a>(
    scan: &'a ContainerScan,
) -> Vec<CreoFeatureDefinitionRecord<'a>> {
    let mut records = Vec::new();
    for definition in &scan.features.definitions {
        let id = feature_definition_record_id(scan, definition);
        let mut parameter_frames = Vec::new();
        for frame in &definition.parameter_frames {
            parameter_frames.push(CreoFeatureParameterFrame {
                kind: match frame.kind {
                    crate::feature::definitions::FeatureParameterFrameKind::LocalSystem => {
                        "local_system"
                    }
                    crate::feature::definitions::FeatureParameterFrameKind::Transform => {
                        "transform"
                    }
                },
                body: (&frame.body).into(),
                decoded_values: frame
                    .decoded_values
                    .map(cadmpeg_ir::units::FiniteVector::get),
                offset: frame.offset,
            });
        }
        let mut outlines = Vec::new();
        for outline in &definition.outlines {
            outlines.push(CreoFeatureOutline {
                phase: match outline.phase {
                    crate::feature::definitions::OutlinePhase::PreRollback => "pre_rollback",
                    crate::feature::definitions::OutlinePhase::PostRollback => "post_rollback",
                    crate::feature::definitions::OutlinePhase::PostRegen => "post_regen",
                },
                local_values: &outline.local_scalars,
                local_value_bodies: &outline.local_scalars,
                offset: outline.offset,
            });
        }
        records.push(CreoFeatureDefinitionRecord {
            id,
            definition_id: definition.identity.id(),
            owner_feature_id: definition.identity.owner_feature_id(),
            source_section: source_section_ref(scan, definition.offset),
            body: (&definition.body).into(),
            parameter_frames,
            outlines,
            offset: definition.offset,
        });
    }
    records
}

#[cfg(test)]
mod feature_definition_projection_tests {
    use super::feature_definition_records;
    use crate::feature::definitions::{
        DecodedField, DefinitionIdentity, FeatureDefinition, FeatureOutline, FeatureParameterFrame,
        FeatureParameterFrameKind, OutlinePhase,
    };

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        let mut local_scalars = std::array::from_fn(|_| DecodedField {
            value: None,
            body: Vec::new(),
        });
        local_scalars[0] = DecodedField {
            value: Some(2.0),
            body: vec![0xf9],
        };
        scan.features.definitions.push(FeatureDefinition {
            identity: DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(7),
                owner_feature_id: Some(3),
            },
            body: vec![0xe3],
            parameter_frames: vec![FeatureParameterFrame {
                kind: FeatureParameterFrameKind::Transform,
                body: vec![0xf9],
                decoded_values: None,
                offset: 4,
            }],
            outlines: vec![FeatureOutline {
                phase: OutlinePhase::PostRegen,
                local_scalars,
                offset: 5,
            }],
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 3,
        });
        scan
    }

    #[test]
    fn borrowed_feature_definition_preserves_nested_json() {
        let scan = scan();
        let records = feature_definition_records(&scan);
        let value = serde_json::to_value(&records[0]).expect("record serializes");
        assert_eq!(crate::test_support::native_bytes(&value["body"]), [227]);
        assert_eq!(
            crate::test_support::native_bytes(&value["parameter_frames"][0]["body"]),
            [249]
        );
        assert_eq!(
            value["outlines"][0]["local_values"],
            serde_json::json!([2.0, null, null, null, null, null])
        );
        assert_eq!(
            value["outlines"][0]["local_value_bodies"],
            serde_json::json!(["f9", "", "", "", "", ""])
        );
    }
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
    use super::{
        expanded_section_records, feature_operation_state_records, feature_reference_name_records,
        feature_row_records,
    };
    use std::collections::BTreeSet;

    fn expanded_records() -> Vec<super::CreoExpandedSectionRecord> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.framing
            .expanded_sections
            .push(crate::container::ExpandedSection {
                name: "Body".to_string(),
                source_offset: 0,
                compressed_length: 3,
                data: b"abc".to_vec(),
            });
        expanded_section_records(&scan)
    }

    #[test]
    fn native_expanded_section_row_names_section() {
        let records = expanded_records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "creo:container:expanded_section#Body:0");
        assert_eq!(records[0].name, "Body");
        assert_eq!(records[0].sha256, cadmpeg_ir::hash::sha256_hex(b"abc"));
    }

    fn reference_name_scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.features
            .reference_names
            .push(crate::feature::operations::FeatureReferenceName {
                feature_id: 40,
                name_bytes: b"A\xff".to_vec(),
                own_reference_id: 3,
                reference_type: 1,
                offset: 0,
            });
        scan
    }

    fn reference_name_records() -> Vec<super::CreoFeatureReferenceNameRecord> {
        let scan = reference_name_scan();
        feature_reference_name_records(&scan)
    }

    fn operation_state_scan() -> crate::container::ContainerScan<'static> {
        use crate::feature::operations::{
            FeatureOperation, IdKeyword, OperationKind, OperationName, RecipeResolution,
            RecipeState,
        };
        let mut scan = crate::test_support::empty_container_scan();
        scan.features.operations.push(FeatureOperation {
            feature_id: 40,
            kind: OperationKind::Native,
            name: OperationName::Derived,
            recipe: RecipeResolution::None,
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
        scan.features.operation_states.push(FeatureOperation {
            feature_id: 40,
            kind: OperationKind::Native,
            name: OperationName::Stored {
                bytes: b"A\xff".to_vec(),
                keyword: IdKeyword::Id,
                prefix: Some(b'~'),
            },
            recipe: RecipeState::None,
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
        scan
    }

    fn operation_state_records() -> Result<Vec<serde_json::Value>, cadmpeg_core::CodecError> {
        let scan = operation_state_scan();
        let records = feature_operation_state_records(&scan)?;
        Ok(records
            .iter()
            .map(|record| serde_json::to_value(record).expect("record JSON"))
            .collect())
    }

    #[test]
    fn native_feature_state_row_preserves_stored_name() {
        let records = operation_state_records().expect("the service-profile state is admitted");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["id"], "creo:mdlstatus:feature_state#40:0");
        assert_eq!(records[0]["stored_name"], "A\u{fffd}");
        assert_eq!(
            crate::test_support::native_bytes(&records[0]["stored_name_bytes"]),
            [65, 255]
        );
        assert_eq!(records[0]["stored_name_prefix"], "~");
        assert_eq!(records[0]["identifier_keyword"], "id");
        assert_eq!(records[0]["family"], "Native Feature");
        assert_eq!(records[0]["current"], true);
    }

    #[test]
    fn native_feature_reference_row_preserves_name_bytes() {
        let records = reference_name_records();
        assert_eq!(records[0].name, "A\u{fffd}");
        assert_eq!(records[0].name_bytes, b"A\xff");
    }

    #[test]
    fn overlapping_feature_candidates_do_not_expose_short_headers() {
        let payload = [1, 0xe3, 2, 0, 0, 0xe3, 0xf6, 0x83, 0x8f, 0xe1];
        let mut scan = crate::test_support::empty_container_scan();
        scan.features.rows = crate::feature::rows::rows(&payload, &BTreeSet::from([1, 2]), 0);
        let records = feature_row_records(&scan);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].owner_feature_id, 2);
        assert_eq!(records[0].header, [0, 0]);
        assert_eq!(records[0].body, (&payload[3..]));
    }
    mod projection_admission;
}
