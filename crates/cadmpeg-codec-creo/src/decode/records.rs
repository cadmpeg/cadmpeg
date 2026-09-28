// SPDX-License-Identifier: Apache-2.0
//! Record shadow-layer structs and their `ContainerScan` mappers, moved
//! verbatim from `decode.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use cadmpeg_core::decode::DecodeContext;
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

use super::coverage::{
    source_section, source_section_ref, surface_family, surface_named_parameter_record,
    surface_prototype_family_name, surface_variant,
};
use super::curve_expressions::curve_expression_record_id;
use super::expanded::{affected_kind, extent_source, half_edge_ref};
use super::feature_history::round::replayed_torus_minor_radius;
use super::native_records::{
    CreoConeHalfAngleOverride, CreoCurveExpressionAssignment, CreoCurveExpressionEquation,
    CreoCurveExpressionLine, CreoCurveExpressionLocalSystem, CreoCurveExpressionSolveBlock,
    CreoCurveParameterOpaqueSpan, CreoCurveParameterReference, CreoCurveParameterScalar,
    CreoFeatureFieldValue, CreoFeatureOperationState, CreoOperationNameRecord, CreoFeatureOutline,
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
use super::sketch::coordinates::resolved_section_coordinates;
use super::sketch::equations_scalar::resolved_section_scalar_values;
use super::sketch::radii::resolved_section_radii;
use super::sketch_ids::{
    binary_flag_value, feature_definition_has_sketch_design, feature_definition_record_id,
    feature_sketch_record_id_in_scan, sketch_table_headers,
};

#[derive(Serialize)]
pub(super) struct CreoSketchRecord {
    pub(super) id: String,
    definition_id: u32,
    owner_feature_id: Option<u32>,
    pub(super) source_section: String,
    pub(super) offset: usize,
    section_3d: Option<CreoSketchSection3d>,
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
    saved_entities: Vec<CreoSketchSavedEntity>,
    dimensions: Vec<CreoSketchDimension>,
    relations: Vec<CreoSketchRelation>,
    skamps: Vec<CreoSketchSkamp>,
    relation_triples: Vec<CreoSketchRelationTriple>,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureDefinitionRecord {
    pub(super) id: String,
    definition_id: u32,
    owner_feature_id: Option<u32>,
    pub(super) source_section: String,
    body: Vec<u8>,
    parameter_frames: Vec<CreoFeatureParameterFrame>,
    outlines: Vec<CreoFeatureOutline>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveExpressionRecord {
    pub(super) id: String,
    entity_id: u32,
    backup: bool,
    local_system: Option<CreoCurveExpressionLocalSystem>,
    lines: Vec<CreoCurveExpressionLine>,
    assignments: Vec<CreoCurveExpressionAssignment>,
    solve_blocks: Vec<CreoCurveExpressionSolveBlock>,
    unresolved_solve_control: bool,
    prohibited_constructs: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct CreoFeatureReferenceNameRecord {
    pub(super) id: String,
    owner_feature_id: u32,
    name: String,
    name_bytes: Vec<u8>,
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
    serializer.collect_seq(entry.fields())
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
    payload: &'a [u8],
    payload_offset: usize,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureRowRecord<'a> {
    pub(super) id: String,
    owner_feature_id: u32,
    header: [u8; 2],
    root_schema_class: Option<u32>,
    stream_offset: usize,
    body: &'a [u8],
    body_offset: usize,
    pub(super) offset: usize,
    pub(super) source_section: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureChoiceFieldRecord {
    pub(super) id: String,
    owner_feature_id: u32,
    choice_label: String,
    name: String,
    type_byte: u8,
    value: CreoFeatureFieldValue,
    pub(super) offset: usize,
    pub(super) source_section: String,
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
    body: &'a [u8],
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
    body: &'a [u8],
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

pub(super) fn reference_line_records(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<CreoReferenceLineRecord>, CodecError> {
    let family = |kind: &crate::reference::ReferenceLineKind| match kind {
        crate::reference::ReferenceLineKind::Line => "line",
        crate::reference::ReferenceLineKind::Line3d { .. } => "line3d",
    };
    let mut records = Vec::new();
    for line in &scan.references.lines {
        let id = ctx.format_retained(
            format_args!("creo:mdl_ref_info:{}_record#{}", family(&line.kind), line.offset),
            "creo native reference line IDs",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native reference line records")?;
        records.push(CreoReferenceLineRecord {
            id,
            kind: line.kind.clone(),
            start: line.start.get().into(),
            end: line.end.get().into(),
            offset: line.offset,
        });
    }
    Ok(records)
}

pub(super) fn reference_circle_records(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<CreoReferenceCircleRecord>, CodecError> {
    let mut records = Vec::new();
    for circle in &scan.references.circles {
        let id = ctx.format_retained(
            format_args!("creo:mdl_ref_info:arc_z_record#{}", circle.offset),
            "creo native reference circle IDs",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native reference circle records")?;
        records.push(CreoReferenceCircleRecord {
            id,
            entity_id: circle.entity_id,
            center: circle.center.get().into(),
            center_source: if circle.center_stored {
                "stored"
            } else {
                "endpoint_midpoint"
            },
            radius: circle.radius.get(),
            axis: (*circle.axis.as_raw()).into(),
            endpoints: [circle.start.get().into(), circle.end.get().into()],
            offset: circle.offset,
        });
    }
    Ok(records)
}

pub(super) fn reference_conic_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<Vec<CreoReferenceConicRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for conic in &scan.references.conics {
        let id = ctx.format_retained(
            format_args!("creo:mdl_ref_info:conic_record#{}", conic.offset),
            "creo native reference conic IDs",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native reference conic records")?;
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
            body: &conic.body,
            offset: conic.offset,
        });
    }
    Ok(records)
}

pub(super) fn reference_ellipse_records(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<CreoReferenceEllipseRecord>, CodecError> {
    let mut records = Vec::new();
    for ellipse in &scan.references.ellipses {
        let id = ctx.format_retained(
            format_args!("creo:mdl_ref_info:ellipse_carrier#{}", ellipse.offset),
            "creo native reference ellipse IDs",
        )?;
        let source_conic_id = ctx.format_retained(
            format_args!("creo:mdl_ref_info:conic_record#{}", ellipse.offset),
            "creo native reference ellipse source IDs",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native reference ellipse records")?;
        records.push(CreoReferenceEllipseRecord {
            id,
            source_conic_id,
            source_entity_id: ellipse.source_entity_id,
            center: ellipse.center.get().into(),
            axis: (*ellipse.axis.as_raw()).into(),
            major_direction: (*ellipse.major_direction.as_raw()).into(),
            major_radius: ellipse.major_radius.get(),
            minor_radius: ellipse.minor_radius.get(),
            offset: ellipse.offset,
        });
    }
    Ok(records)
}

struct HexDigest([u8; 32]);

impl std::fmt::Display for HexDigest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const DIGITS: [char; 16] = [
            '0', '1', '2', '3', '4', '5', '6', '7',
            '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
        ];
        for byte in self.0 {
            formatter.write_char(DIGITS[usize::from(byte >> 4)])?;
            formatter.write_char(DIGITS[usize::from(byte & 0x0f)])?;
        }
        Ok(())
    }
}

pub(super) fn expanded_section_records(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<CreoExpandedSectionRecord>, CodecError> {
    let mut records = Vec::new();
    for section in &scan.framing.expanded_sections {
        let id = ctx.format_retained(
            format_args!("creo:container:expanded_section#{}:{}", section.name, section.source_offset),
            "creo native expanded section IDs",
        )?;
        let name = ctx.copy_retained_text(&section.name, "creo native expanded section names")?;
        let sha256 = ctx.format_retained(
            HexDigest(sha256(&section.data)),
            "creo native expanded section hashes",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native expanded section records")?;
        records.push(CreoExpandedSectionRecord {
            id,
            name,
            source_offset: section.source_offset,
            compressed_length: section.compressed_length,
            expanded_length: section.data.len(),
            sha256,
        });
    }
    Ok(records)
}

#[derive(Serialize)]
pub(super) struct CreoFcCurveCoordinateRecord<'a> {
    pub(super) id: String,
    curve_id: u32,
    subtype: u8,
    body: &'a [u8],
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
    body: &'a [u8],
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
    body: &'a [u8],
    envelope: CreoPlaneEnvelope,
    corner_coordinate_equal: [Option<bool>; 3],
    scalar_tokens: &'a [Vec<u8>],
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
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFeatureEntityRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for entity in &scan.features.entities {
        let id = ctx.format_retained(
            format_args!("creo:allfeatur:entity#{}", entity.entity_id),
            "creo feature entity record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo feature entity records")?;
        records.push(CreoFeatureEntityRecord {
            id,
            entity_id: entity.entity_id,
            type_byte: entity.type_byte,
            name: &entity.name,
            offset: entity.offset,
        });
    }
    Ok(records)
}

pub(super) fn feature_entity_reference_records(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<CreoFeatureEntityReferenceRecord>, CodecError> {
    let mut records = Vec::new();
    for reference in &scan.features.entity_references {
        let id = ctx.format_retained(
            format_args!("creo:allfeatur:entity_reference#{}", reference.offset),
            "creo feature entity reference record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo feature entity reference records")?;
        records.push(CreoFeatureEntityReferenceRecord {
            id,
            source_entity_id: reference.source_entity_id,
            target_entity_id: reference.target_entity_id,
            target_resolved: (reference.target_entity_id as usize) < scan.features.entities.len(),
            offset: reference.offset,
        });
    }
    Ok(records)
}

pub(super) fn feature_entity_table_records(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<CreoFeatureEntityTableRecord>, CodecError> {
    let mut records = Vec::new();
    for table in &scan.features.entity_tables {
        let mut entry_ids = Vec::new();
        let mut entries = Vec::new();
        let mut surface_ids = Vec::new();
        let mut non_surface_entity_ids = Vec::new();
        for entry in &table.entries {
            ctx.try_reserve_items(&mut entry_ids, 1, "creo feature entity table record entry ids")?;
            entry_ids.push(entry.entity_id);
            ctx.try_reserve_items(&mut entries, 1, "creo feature entity table record entries")?;
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
                ctx.try_reserve_items(&mut surface_ids, 1, "creo feature entity table record surface ids")?;
                surface_ids.push(entry.entity_id);
            } else {
                ctx.try_reserve_items(&mut non_surface_entity_ids, 1, "creo feature entity table record non surface ids")?;
                non_surface_entity_ids.push(entry.entity_id);
            }
        }
        let id = ctx.format_retained(
            format_args!("creo:allfeatur:entity_table#{}", table.offset),
            "creo feature entity table record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo feature entity table records")?;
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
    Ok(records)
}

#[cfg(test)]
mod feature_entity_table_record_tests {
    use super::feature_entity_table_records;
    use crate::feature::entity::{dummy_table_entry, FeatureEntityTable};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeSet;

    fn scan_with_table() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        scan.features.entity_tables.push(FeatureEntityTable::new(
            4,
            29,
            vec![dummy_table_entry(7), dummy_table_entry(9)],
            &BTreeSet::from([7]),
            12,
        ));
        scan
    }

    fn collection_error(limit: u64, operation: &'static str) {
        let scan = scan_with_table();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty source is admitted");
        let Err(error) = feature_entity_table_records(&ctx, &scan) else {
            panic!("record copy exceeds the collection limit");
        };
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == operation), "{error:?}");
    }

    #[test]
    fn entity_table_record_entry_ids_refuse_collection_limit() {
        collection_error(0, "creo feature entity table record entry ids");
    }

    #[test]
    fn entity_table_record_entries_refuse_collection_limit() {
        collection_error(1, "creo feature entity table record entries");
    }

    #[test]
    fn entity_table_record_surface_ids_refuse_collection_limit() {
        collection_error(2, "creo feature entity table record surface ids");
    }

    #[test]
    fn entity_table_record_non_surface_ids_refuse_collection_limit() {
        collection_error(5, "creo feature entity table record non surface ids");
    }

    #[test]
    fn entity_table_record_outer_rows_refuse_collection_limit() {
        collection_error(6, "creo feature entity table records");
    }

    #[test]
    fn entity_table_record_id_refuses_retained_limit() {
        let scan = scan_with_table();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = "creo:allfeatur:entity_table#12".len() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty source is admitted");
        let Err(error) = feature_entity_table_records(&ctx, &scan) else {
            panic!("record identity exceeds the retained limit");
        };
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo feature entity table record id"), "{error:?}");
    }

    #[test]
    fn entity_table_record_preserves_source_order_and_partition() {
        let scan = scan_with_table();
        crate::decode::with_test_decode_ctx(|ctx| {
            let records = feature_entity_table_records(ctx, &scan)?;
            assert_eq!(records.len(), 1);
            let record = &records[0];
            assert_eq!(record.id, "creo:allfeatur:entity_table#12");
            assert_eq!(record.entry_ids, [7, 9]);
            assert_eq!(record.surface_ids, [7]);
            assert_eq!(record.non_surface_entity_ids, [9]);
            Ok::<(), cadmpeg_core::CodecError>(())
        }).expect("service profile admits the record");
    }
}

pub(super) fn feature_geometry_table_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFeatureGeometryTableRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for table in &scan.features.geometry_tables {
        let id = ctx.format_retained(
            format_args!("creo:feature:geometry_table#{}", table.offset),
            "creo feature geometry table record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo feature geometry table records")?;
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
    Ok(records)
}

pub(super) fn feature_loop_history_entry_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFeatureLoopHistoryEntryRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for entry in &scan.features.loop_history_entries {
        let id = ctx.format_retained(
            format_args!("creo:feature:loop_history_entry#{}", entry.offset),
            "creo feature loop history record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo feature loop history records")?;
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
    Ok(records)
}

pub(super) fn feature_affected_id_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFeatureAffectedIdsRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.features.affected_ids {
        let id = ctx.format_retained(
            format_args!("creo:feature:affected_ids#{}", record.offset),
            "creo feature affected ids record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo feature affected ids records")?;
        records.push(CreoFeatureAffectedIdsRecord {
            id,
            owner_feature_id: record.feature_id,
            kind: affected_kind(record.kind),
            ids: &record.ids,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    Ok(records)
}

pub(super) fn feature_replay_affected_id_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFeatureReplayAffectedIdsRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.features.replay_affected_ids {
        let id = ctx.format_retained(
            format_args!("creo:feature:replay_affected_ids#{}", record.offset),
            "creo feature replay affected ids record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo feature replay affected ids records")?;
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
    Ok(records)
}

pub(super) fn surface_merge_replay_affected_id_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoSurfaceMergeReplayAffectedIdsRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.features.surface_merge_replay_affected_ids {
        let id = ctx.format_retained(
            format_args!("creo:feature:surface_merge_replay_affected_ids#{}", record.offset),
            "creo surface merge replay affected ids record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo surface merge replay affected ids records")?;
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
    Ok(records)
}

pub(super) fn feature_loop_restore_direction_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFeatureLoopRestoreDirectionRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.features.loop_restore_directions {
        let id = ctx.format_retained(
            format_args!("creo:feature:loop_restore_direction#{}", record.offset),
            "creo feature loop restore direction record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo feature loop restore direction records")?;
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
    Ok(records)
}

pub(super) fn feature_revolution_extent_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFeatureRevolutionExtentRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.features.revolution_extents {
        let id = ctx.format_retained(
            format_args!("creo:feature:revolution_extent#{}", record.offset),
            "creo feature revolution extent record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo feature revolution extent records")?;
        records.push(CreoFeatureRevolutionExtentRecord {
            id,
            owner_feature_id: record.feature_id,
            kind: "full_turn",
            angle_radians: std::f64::consts::TAU,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    Ok(records)
}

pub(super) fn feature_choice_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFeatureChoiceRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for choice in &scan.features.choices {
        let id = ctx.format_retained(
            format_args!("creo:feature:choice#{}", choice.offset),
            "creo feature choice record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo feature choice records")?;
        records.push(CreoFeatureChoiceRecord {
            id,
            owner_feature_id: choice.feature_id,
            label: &choice.label,
            type_byte: choice.type_byte,
            payload: &choice.payload,
            payload_offset: choice.payload_offset,
            offset: choice.offset,
            source_section: source_section_ref(scan, choice.offset),
        });
    }
    Ok(records)
}

pub(super) fn feature_row_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFeatureRowRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for row in &scan.features.rows {
        let id = ctx.format_retained(
            format_args!("creo:allfeatur:feature_row#{}", row.offset),
            "creo feature row record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo feature row records")?;
        records.push(CreoFeatureRowRecord {
            id,
            owner_feature_id: row.feature_id,
            header: row.body.header(),
            root_schema_class: row.root_schema_class.map(SchemaClass::code),
            stream_offset: row.stream_offset,
            body: &row.body,
            body_offset: row.body_offset,
            offset: row.offset,
            source_section: source_section_ref(scan, row.offset),
        });
    }
    Ok(records)
}

pub(super) fn depdb_recipe_row_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFeatureRowRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for row in &scan.features.depdb_recipe_rows {
        let id = ctx.format_retained(
            format_args!("creo:depdb:recipe_row#{}", row.offset),
            "creo depdb recipe row record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo depdb recipe row records")?;
        records.push(CreoFeatureRowRecord {
            id,
            owner_feature_id: row.feature_id,
            header: [0; 2],
            root_schema_class: row.root_schema_class.map(SchemaClass::code),
            stream_offset: row.stream_offset,
            body: &row.body,
            body_offset: row.body_offset,
            offset: row.offset,
            source_section: source_section_ref(scan, row.offset),
        });
    }
    Ok(records)
}

#[cfg(test)]
mod feature_projection_limit_tests {
    use super::{
        depdb_recipe_row_records, feature_affected_id_records, feature_choice_records,
        feature_entity_records, feature_entity_reference_records, feature_geometry_table_records,
        feature_loop_history_entry_records, feature_loop_restore_direction_records,
        feature_replay_affected_id_records, feature_revolution_extent_records, feature_row_records,
        surface_merge_replay_affected_id_records,
    };
    use crate::feature::entity::{FeatureEntity, FeatureEntityReference};
    use crate::feature::rows::{
        dummy_loop_history_entry, AffectedIdKind, FeatureAffectedIds, FeatureChoice,
        FeatureGeometryTable, FeatureGeometryTableKind, FeatureLoopRestoreDirection,
        FeatureReplayAffectedIds, FeatureRevolutionExtent, FeatureRow, FeatureRowBody,
        FeatureSurfaceMergeAffectedIds, LoopRestoreDirectionLane, ReplayExtentSource,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        scan.features.entities.push(FeatureEntity {
            entity_id: 4,
            type_byte: 1,
            name: "datum".into(),
            offset: 3,
        });
        scan.features.entity_references.push(FeatureEntityReference {
            source_entity_id: Some(4),
            target_entity_id: 4,
            offset: 5,
        });
        scan.features.geometry_tables.push(FeatureGeometryTable {
            feature_id: 7,
            kind: FeatureGeometryTableKind::DatumIds(Some(vec![4, 5])),
            count: 2,
            entity_class: 200,
            offset: 13,
        });
        scan.features.loop_history_entries.push(dummy_loop_history_entry());
        scan.features.affected_ids.push(FeatureAffectedIds {
            feature_id: 7,
            kind: AffectedIdKind::Geometry,
            ids: vec![4, 5],
            offset: 29,
        });
        scan.features.replay_affected_ids.push(FeatureReplayAffectedIds {
            feature_id: 7,
            geometry_ids: vec![4],
            edge_ids: vec![5],
            geometry_extent: ReplayExtentSource::Explicit,
            edge_extent: ReplayExtentSource::Inherited,
            offset: 31,
        });
        scan.features.surface_merge_replay_affected_ids.push(FeatureSurfaceMergeAffectedIds {
            feature_id: 7,
            geometry_ids: vec![4],
            edge_ids: vec![5],
            quilt_ids: vec![6],
            geometry_extent: ReplayExtentSource::Explicit,
            edge_extent: ReplayExtentSource::Inherited,
            quilt_extent: ReplayExtentSource::Explicit,
            offset: 33,
        });
        scan.features.loop_restore_directions.push(FeatureLoopRestoreDirection {
            feature_id: 7,
            lane: LoopRestoreDirectionLane::Primary,
            value: 1,
            offset: 35,
        });
        scan.features.revolution_extents.push(FeatureRevolutionExtent {
            feature_id: 7,
            offset: 37,
        });
        scan.features.choices.push(FeatureChoice {
            feature_id: 7,
            label: "depth_choice".into(),
            type_byte: Some(1),
            payload: vec![0xe3],
            payload_offset: 41,
            offset: 39,
        });
        let row = FeatureRow {
            feature_id: 7,
            root_schema_class: None,
            stream_offset: 0,
            body: FeatureRowBody::try_from(vec![0, 1, 2]).expect("complete test header"),
            body_offset: 44,
            offset: 42,
        };
        scan.features.rows.push(row.clone());
        scan.features.depdb_recipe_rows.push(row);
        scan
    }

    macro_rules! collection_limit_test {
        ($name:ident, $projection:ident, $operation:literal) => {
            #[test]
            fn $name() {
                let scan = scan();
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root is admitted");
                let error = match $projection(&ctx, &scan) {
                    Err(error) => error,
                    Ok(_) => panic!("one native record exceeds the collection limit"),
                };
                assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                    if resource.dimension == ResourceDimension::CollectionItems
                        && resource.operation == $operation), "{error:?}");
            }
        };
    }

    collection_limit_test!(feature_entity_record_refuses_limit, feature_entity_records, "creo feature entity records");
    collection_limit_test!(feature_entity_reference_record_refuses_limit, feature_entity_reference_records, "creo feature entity reference records");
    collection_limit_test!(feature_geometry_table_record_refuses_limit, feature_geometry_table_records, "creo feature geometry table records");
    collection_limit_test!(feature_loop_history_record_refuses_limit, feature_loop_history_entry_records, "creo feature loop history records");
    collection_limit_test!(feature_affected_ids_record_refuses_limit, feature_affected_id_records, "creo feature affected ids records");
    collection_limit_test!(feature_replay_affected_ids_record_refuses_limit, feature_replay_affected_id_records, "creo feature replay affected ids records");
    collection_limit_test!(surface_merge_replay_affected_ids_record_refuses_limit, surface_merge_replay_affected_id_records, "creo surface merge replay affected ids records");
    collection_limit_test!(feature_loop_restore_direction_record_refuses_limit, feature_loop_restore_direction_records, "creo feature loop restore direction records");
    collection_limit_test!(feature_revolution_extent_record_refuses_limit, feature_revolution_extent_records, "creo feature revolution extent records");
    collection_limit_test!(feature_choice_record_refuses_limit, feature_choice_records, "creo feature choice records");
    collection_limit_test!(feature_row_record_refuses_limit, feature_row_records, "creo feature row records");
    collection_limit_test!(depdb_recipe_row_record_refuses_limit, depdb_recipe_row_records, "creo depdb recipe row records");

    #[test]
    fn borrowed_feature_projection_preserves_json() {
        let scan = scan();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let geometry = feature_geometry_table_records(&ctx, &scan).expect("record is admitted");
        let history = feature_loop_history_entry_records(&ctx, &scan).expect("record is admitted");
        let geometry = serde_json::to_value(&geometry[0]).expect("record serializes");
        let history = serde_json::to_value(&history[0]).expect("record serializes");
        assert_eq!(geometry["entry_ids"], serde_json::json!([4, 5]));
        assert_eq!(history["field_bytes"], serde_json::json!([[1], [2], [3], [4]]));
    }
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
                crate::feature::rows::FeatureFieldValue::Empty => CreoFeatureFieldValue::Empty,
                crate::feature::rows::FeatureFieldValue::CompactInt(value) => {
                    CreoFeatureFieldValue::CompactInt { value: *value }
                }
                crate::feature::rows::FeatureFieldValue::CompactIntArray(values) => {
                    CreoFeatureFieldValue::CompactIntArray {
                        values: values.clone(),
                    }
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
                    body: body.clone(),
                    decoded_values: decoded_values.clone(),
                },
                crate::feature::rows::FeatureFieldValue::Raw(bytes) => CreoFeatureFieldValue::Raw {
                    bytes: bytes.clone(),
                },
            },
            offset: field.offset,
            source_section: source_section(scan, field.offset),
        })
        .collect()
}

pub(super) fn half_edge_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoHalfEdgeRecord<'a>>, CodecError> {
    let mut topology_rows = BTreeMap::new();
    for row in &scan.curves.topology_rows {
        match topology_rows.entry(row.id) {
            std::collections::btree_map::Entry::Occupied(mut entry) => { entry.insert(row); }
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo native half edge topology row nodes")?;
                entry.insert(row);
            }
        }
    }
    let mut records = Vec::new();
    for edge in &scan.topology.half_edges {
        let Some(row) = topology_rows.get(&edge.id.curve_id) else {
            continue;
        };
        let id = ctx.format_retained(
            format_args!("creo:topology:half_edge#{}:{}", edge.id.curve_id, edge.id.side),
            "creo native half edge record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native half edge records")?;
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
    Ok(records)
}

pub(super) fn loop_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoLoopRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for (index, record) in scan.topology.loops.iter().enumerate() {
        let id = ctx.format_retained(
            format_args!("creo:topology:loop#{}", index + 1),
            "creo native loop record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native loop records")?;
        records.push(CreoLoopRecord {
            id,
            face_id: record.face_id.map_or(0, std::num::NonZeroU32::get),
            half_edges: &record.half_edges,
        });
    }
    Ok(records)
}

pub(super) fn loop_array_frame_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoLoopArrayFrameRecord<'a>>, CodecError> {
    let mut counts = BTreeMap::<usize, usize>::new();
    for record in &scan.loop_arrays.records {
        let count = match counts.entry(record.frame_offset) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo native loop array frame count nodes")?;
                entry.insert(0)
            }
        };
        *count = count.checked_add(1).ok_or_else(|| ctx.refuse_codec_limit(
            "creo native loop array frame counts", u64::MAX, u64::MAX
        ))?;
    }
    let mut records = Vec::new();
    for frame in &scan.loop_arrays.frames {
        let id = ctx.format_retained(
            format_args!("creo:loop_array:frame#{}", frame.offset),
            "creo native loop array frame record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native loop array frame records")?;
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
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoLoopArrayRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.loop_arrays.records {
        let id = ctx.format_retained(
            format_args!("creo:loop_array:record#{}", record.offset),
            "creo native loop array record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native loop array records")?;
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
            body: &record.body,
            offset: record.offset,
            body_offset: record.body_offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    Ok(records)
}

pub(super) fn topological_vertex_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoTopologicalVertexRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.topology.vertices {
        let id = ctx.format_retained(
            format_args!("creo:topology:vertex#{}", record.id),
            "creo native topological vertex record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native topological vertex records")?;
        records.push(CreoTopologicalVertexRecord {
            id,
            vertex_id: record.id,
            half_edges: &record.half_edges,
        });
    }
    Ok(records)
}

pub(super) fn half_edge_vertex_incidence_records(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<CreoHalfEdgeVertexIncidenceRecord>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.topology.half_edge_vertex_incidence {
        let id = ctx.format_retained(
            format_args!("creo:topology:half_edge_vertex_incidence#{}:{}", record.half_edge.curve_id, record.half_edge.side),
            "creo native half edge vertex incidence record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native half edge vertex incidence records")?;
        records.push(CreoHalfEdgeVertexIncidenceRecord {
            id,
            half_edge: half_edge_ref(record.half_edge),
            start_vertex_id: record.start_vertex_id,
            end_vertex_id: record.end_vertex_id,
        });
    }
    Ok(records)
}

pub(super) fn face_component_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFaceComponentRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for (index, record) in scan.topology.face_components.iter().enumerate() {
        let id = ctx.format_retained(
            format_args!("creo:topology:face_component#{}", index + 1),
            "creo native face component record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native face component records")?;
        records.push(CreoFaceComponentRecord {
            id,
            face_ids: &record.face_ids,
            curve_ids: &record.curve_ids,
        });
    }
    Ok(records)
}

#[cfg(test)]
mod topology_projection_limit_tests {
    use super::{
        face_component_records, half_edge_records, half_edge_vertex_incidence_records,
        loop_array_frame_records, loop_array_record_records, loop_records,
        topological_vertex_records,
    };
    use crate::curve::CurveTopologyRow;
    use crate::loop_array::{LoopArrayFrame, LoopArrayRecord};
    use crate::topology::{
        FaceComponent, HalfEdge, HalfEdgeId, HalfEdgeVertexIncidence, Loop, Side,
        TopologicalVertex,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::num::NonZeroU32;

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        let half_edge = HalfEdgeId { curve_id: 8, side: Side::Zero };
        scan.curves.topology_rows.push(CurveTopologyRow {
            id: 8,
            type_byte: 1,
            feature_id: 2,
            directions: [0, 0],
            faces: [NonZeroU32::new(1), None],
            next_edges: [8, 0],
            offset: 13,
        });
        scan.topology.half_edges.push(HalfEdge {
            id: half_edge,
            face_id: NonZeroU32::new(1),
            next: Some(half_edge),
        });
        scan.topology.loops.push(Loop {
            face_id: NonZeroU32::new(1),
            half_edges: vec![half_edge],
        });
        scan.topology.vertices.push(TopologicalVertex {
            id: 1,
            half_edges: vec![half_edge],
        });
        scan.topology.half_edge_vertex_incidence.push(HalfEdgeVertexIncidence {
            half_edge,
            start_vertex_id: 1,
            end_vertex_id: Some(1),
        });
        scan.topology.face_components.push(FaceComponent {
            face_ids: vec![1],
            curve_ids: vec![8],
        });
        scan.loop_arrays.frames.push(LoopArrayFrame {
            offset: 17,
            variant: None,
            declared_count: 1,
            class_id: 4,
            prototype_end: 19,
            end: 23,
            overfull: false,
        });
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

    macro_rules! collection_limit_test {
        ($name:ident, $projection:ident, $limit:expr, $operation:literal) => {
            #[test]
            fn $name() {
                let scan = scan();
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = $limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root is admitted");
                let error = match $projection(&ctx, &scan) {
                    Err(error) => error,
                    Ok(_) => panic!("one native record exceeds the collection limit"),
                };
                assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                    if resource.dimension == ResourceDimension::CollectionItems
                        && resource.operation == $operation), "{error:?}");
            }
        };
    }

    collection_limit_test!(half_edge_topology_nodes_refuse_limit, half_edge_records, 0, "creo native half edge topology row nodes");
    collection_limit_test!(half_edge_records_refuse_limit, half_edge_records, 1, "creo native half edge records");
    collection_limit_test!(loop_records_refuse_limit, loop_records, 0, "creo native loop records");
    collection_limit_test!(loop_array_frame_count_nodes_refuse_limit, loop_array_frame_records, 0, "creo native loop array frame count nodes");
    collection_limit_test!(loop_array_frame_records_refuse_limit, loop_array_frame_records, 1, "creo native loop array frame records");
    collection_limit_test!(loop_array_records_refuse_limit, loop_array_record_records, 0, "creo native loop array records");
    collection_limit_test!(topological_vertex_records_refuse_limit, topological_vertex_records, 0, "creo native topological vertex records");
    collection_limit_test!(half_edge_vertex_incidence_records_refuse_limit, half_edge_vertex_incidence_records, 0, "creo native half edge vertex incidence records");
    collection_limit_test!(face_component_records_refuse_limit, face_component_records, 0, "creo native face component records");

    #[test]
    fn borrowed_topology_projection_preserves_half_edges_and_body() {
        let scan = scan();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let loops = loop_records(&ctx, &scan).expect("loop is admitted");
        let rows = loop_array_record_records(&ctx, &scan).expect("row is admitted");
        let loop_value = serde_json::to_value(&loops[0]).expect("loop serializes");
        let row_value = serde_json::to_value(&rows[0]).expect("row serializes");
        assert_eq!(loop_value["half_edges"], serde_json::json!([{"curve_id":8,"side":0}]));
        assert_eq!(row_value["body"], serde_json::json!([0xe3]));
        assert_eq!(row_value["end"], serde_json::json!(23));
    }
}

pub(super) fn fc_curve_coordinate_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFcCurveCoordinateRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.curves.fc_coordinates {
        let id = ctx.format_retained(
            format_args!("creo:curve:fc_coordinates#{}", record.curve_id),
            "creo native FC curve coordinate record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native FC curve coordinate records")?;
        records.push(CreoFcCurveCoordinateRecord {
            id,
            curve_id: record.curve_id,
            subtype: record.subtype,
            body: &record.body,
            values_mm: &record.values_mm,
            tokens: &record.tokens,
            opaque_spans: &record.opaque_spans,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    Ok(records)
}

pub(super) fn prototype_pcurve_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoPrototypePcurveRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.curves.prototype_pcurves {
        let id = ctx.format_retained(
            format_args!("creo:curve:prototype_pcurve#{}", record.curve_id),
            "creo native prototype pcurve record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native prototype pcurve records")?;
        records.push(CreoPrototypePcurveRecord {
            id,
            curve_id: record.curve_id,
            face_0_endpoints: record.face_0_endpoints,
            face_1_endpoints: record.face_1_endpoints,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    Ok(records)
}

pub(super) fn curve_prototype_topology_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoCurvePrototypeTopologyRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.curves.prototype_topology {
        let id = ctx.format_retained(
            format_args!("creo:curve:prototype_topology#{}", record.curve_id),
            "creo native curve prototype topology record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native curve prototype topology records")?;
        records.push(CreoCurvePrototypeTopologyRecord {
            id,
            curve_id: record.curve_id,
            faces: record.stored_face_ids(),
            next_edges: record.next_edges,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    Ok(records)
}

pub(super) fn curve_prototype_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
    prototypes: &'a [crate::curve::CurvePrototype],
    id_prefix: &str,
) -> Result<Vec<CreoCurvePrototypeRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in prototypes {
        let id = ctx.format_retained(
            format_args!("{id_prefix}#{}:{}", record.offset, record.id),
            "creo native curve prototype record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native curve prototype records")?;
        records.push(CreoCurvePrototypeRecord {
            id,
            curve_id: record.id,
            type_byte: record.type_byte,
            generating_feature_id: record.feature_id,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    Ok(records)
}

pub(super) fn plane_local_system_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
    systems: &'a [crate::surface::PlaneLocalSystem],
    id_prefix: &str,
) -> Result<Vec<CreoPlaneLocalSystemRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in systems {
        let frame = record.frame();
        let id = ctx.format_retained(
            format_args!("{id_prefix}#{}:{}", record.offset, record.surface_id),
            "creo native plane local system record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native plane local system records")?;
        records.push(CreoPlaneLocalSystemRecord {
                id,
                surface_id: record.surface_id,
                body: &record.body,
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
    Ok(records)
}

pub(super) fn plane_envelope_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
    envelopes: &'a [crate::surface::PlaneEnvelopeRecord],
    id_prefix: &str,
) -> Result<Vec<CreoPlaneEnvelopeRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in envelopes {
        let id = ctx.format_retained(
            format_args!("{id_prefix}#{}:{}", record.offset, record.surface_id),
            "creo native plane envelope record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native plane envelope records")?;
        records.push(CreoPlaneEnvelopeRecord {
            id,
            surface_id: record.surface_id,
            body: &record.body,
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
    Ok(records)
}

pub(super) fn outline_plane_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
    planes: &'a [crate::surface::OutlinePlane],
    id_prefix: &str,
) -> Result<Vec<CreoOutlinePlaneRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in planes {
        let id = ctx.format_retained(
            format_args!("{id_prefix}#{}:{}", record.offset, record.surface_id),
            "creo native outline plane record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native outline plane records")?;
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
    Ok(records)
}

pub(super) fn datum_plane_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoDatumPlaneRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.planes.datums {
        let id = ctx.format_retained(
            format_args!("creo:datum:plane#{}:{}", record.offset_in_payload, record.id),
            "creo native datum plane record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native datum plane records")?;
        records.push(CreoDatumPlaneRecord {
            id,
            datum_id: record.id,
            owner_feature_id: record.feature_id,
            normal: record.plane.normal(),
            plane_offset: record.plane.offset,
            corners: record.corners(),
            offset: record.offset_in_payload,
            source_section: source_section_ref(scan, record.offset_in_payload),
        });
    }
    Ok(records)
}

pub(super) fn datum_cylinder_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoDatumCylinderRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.planes.datum_cylinders {
        let id = ctx.format_retained(
            format_args!("creo:datum:cylinder#{}:{}", record.offset_in_payload, record.id),
            "creo native datum cylinder record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native datum cylinder records")?;
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
    Ok(records)
}

pub(super) fn feature_section_transform_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFeatureSectionTransformRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.features.section_transforms {
        let id = ctx.format_retained(
            format_args!("creo:feature:section_transform#{}:{}", record.definition_id, record.offset),
            "creo native section transform record id",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native section transform records")?;
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
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records.dedup_by(|left, right| left.id == right.id);
    Ok(records)
}

pub(super) fn feature_placement_instruction_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<Vec<CreoFeaturePlacementInstructionRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for definition in &scan.features.definitions {
        for instruction in crate::feature::definitions::placement_instructions(definition) {
            let id = ctx.format_retained(
                format_args!("creo:featdefs:placement_instruction#{}:{}", definition.identity.id(), instruction.offset),
                "creo native placement instruction record id",
            )?;
            ctx.try_reserve_items(&mut records, 1, "creo native placement instruction records")?;
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
    Ok(records)
}

#[cfg(test)]
mod curve_plane_projection_limit_tests {
    use super::{
        curve_prototype_records, curve_prototype_topology_records, datum_cylinder_records,
        datum_plane_records, fc_curve_coordinate_records, feature_placement_instruction_records,
        feature_section_transform_records, outline_plane_records, plane_envelope_records,
        plane_local_system_records, prototype_pcurve_records,
    };
    use crate::curve::{dummy_curve_prototype, CurvePrototypeTopology, FcCurveCoordinates, PrototypePcurveEndpoints};
    use crate::datum::{Axis, DatumCylinder, DatumPlane, DatumPlaneRecord};
    use crate::feature::definitions::{DefinitionIdentity, FeatureDefinition};
    use crate::placement::FeatureSectionTransform;
    use crate::surface::{LocalSystemClassification, OutlinePlane, PlaneEnvelope, PlaneEnvelopeRecord, PlaneLocalSystem, PositionalCylinderFrame};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::units::UnitVector3;

    fn scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        scan.curves.fc_coordinates.push(FcCurveCoordinates {
            curve_id: 8, subtype: 1, body: vec![0xfc, 1], values_mm: vec![2.0],
            tokens: Vec::new(), opaque_spans: Vec::new(), offset: 3,
        });
        scan.curves.prototype_pcurves.push(PrototypePcurveEndpoints {
            curve_id: 8, face_0_endpoints: [[0.0, 0.0], [1.0, 0.0]],
            face_1_endpoints: [[0.0, 0.0], [1.0, 0.0]], offset: 5,
        });
        scan.curves.prototype_topology.push(CurvePrototypeTopology {
            curve_id: 8, faces: [None, None], next_edges: [0, 0], offset: 7,
        });
        scan.curves.prototypes.push(dummy_curve_prototype());
        scan.planes.local_systems.push(PlaneLocalSystem {
            surface_id: 3, body: vec![0xe3], slots: [None; 12], layout: None,
            classification: LocalSystemClassification::Simple, row_offset: 13, offset: 15,
        });
        scan.planes.envelopes.push(PlaneEnvelopeRecord {
            surface_id: 3, body: vec![0xe3], envelope: PlaneEnvelope::Standard {
                bounds_2d: [[None; 2]; 2], corners_3d: [[None; 3]; 2],
            },
            corner_coordinate_equal: [None; 3], scalar_tokens: vec![vec![0xf9]],
            row_offset: 13, offset: 17,
        });
        scan.planes.outlines.push(OutlinePlane {
            surface_id: 3,
            origin: [0.0, 0.0, 0.0],
            normal: UnitVector3::Z_AXIS,
            u_axis: UnitVector3::X_AXIS,
            offset: 18,
        });
        scan.planes.datums.push(DatumPlaneRecord {
            id: 3, feature_id: 2, plane: DatumPlane { axis: Axis::X, offset: 1.0 },
            opposite_offset: 1.0, in_plane_corners: [[None; 2]; 2], offset_in_payload: 19,
        });
        scan.planes.datum_cylinders.push(DatumCylinder {
            id: 4, feature_id: 2, reversed: false,
            frame: PositionalCylinderFrame::new(
                [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 2.0, None,
            ).expect("valid cylinder frame"),
            offset_in_payload: 20,
        });
        scan.features.section_transforms.push(
            FeatureSectionTransform::new(
                2, Some(2), [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 21,
            ).expect("orthonormal test frame"),
        );
        scan.features.definitions.push(FeatureDefinition {
            identity: DefinitionIdentity::Parsed { schema_id: None, owner_feature_id: Some(2) },
            body: b"place_instruction_ptrs\0\xf8\x03\xf7\x0b\xfb\xe3\
                \xf1\xf7\x0b\xe3\xc0\x4e\x9f\x18\xf6\xf6\x02\xf6\x00\x00\x00\xe6".to_vec(),
            parameter_frames: Vec::new(), outlines: Vec::new(), variables: None,
            segments: None, trim_entities: None, trim_vertices: None, order_table: None,
            section_3d: None, dimensions: None, relations: None, saved_section: None,
            offset: 1000,
        });
        scan
    }

    macro_rules! collection_limit_test {
        ($name:ident, $project:expr, $operation:literal) => {
            #[test]
            fn $name() {
                let scan = scan();
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root is admitted");
                let error = match ($project)(&ctx, &scan) {
                    Err(error) => error,
                    Ok(_) => panic!("one native record exceeds the collection limit"),
                };
                assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                    if resource.dimension == ResourceDimension::CollectionItems
                        && resource.operation == $operation), "{error:?}");
            }
        };
    }

    collection_limit_test!(fc_curve_coordinate_record_refuses_limit, fc_curve_coordinate_records, "creo native FC curve coordinate records");
    collection_limit_test!(prototype_pcurve_record_refuses_limit, prototype_pcurve_records, "creo native prototype pcurve records");
    collection_limit_test!(curve_prototype_topology_record_refuses_limit, curve_prototype_topology_records, "creo native curve prototype topology records");
    collection_limit_test!(curve_prototype_record_refuses_limit, |ctx, scan| curve_prototype_records(ctx, scan, &scan.curves.prototypes, "creo:curve:prototype"), "creo native curve prototype records");
    collection_limit_test!(plane_local_system_record_refuses_limit, |ctx, scan| plane_local_system_records(ctx, scan, &scan.planes.local_systems, "creo:surface:plane_local_system"), "creo native plane local system records");
    collection_limit_test!(plane_envelope_record_refuses_limit, |ctx, scan| plane_envelope_records(ctx, scan, &scan.planes.envelopes, "creo:surface:plane_envelope"), "creo native plane envelope records");
    collection_limit_test!(outline_plane_record_refuses_limit, |ctx, scan| outline_plane_records(ctx, scan, &scan.planes.outlines, "creo:surface:outline_plane"), "creo native outline plane records");
    collection_limit_test!(datum_plane_record_refuses_limit, datum_plane_records, "creo native datum plane records");
    collection_limit_test!(datum_cylinder_record_refuses_limit, datum_cylinder_records, "creo native datum cylinder records");
    collection_limit_test!(section_transform_record_refuses_limit, feature_section_transform_records, "creo native section transform records");
    collection_limit_test!(placement_instruction_record_refuses_limit, feature_placement_instruction_records, "creo native placement instruction records");

    #[test]
    fn borrowed_curve_and_plane_projection_preserves_json() {
        let scan = scan();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let fc = fc_curve_coordinate_records(&ctx, &scan).expect("record is admitted");
        let plane = plane_envelope_records(&ctx, &scan, &scan.planes.envelopes, "creo:surface:plane_envelope")
            .expect("record is admitted");
        let fc = serde_json::to_value(&fc[0]).expect("record serializes");
        let plane = serde_json::to_value(&plane[0]).expect("record serializes");
        assert_eq!(fc["body"], serde_json::json!([0xfc, 1]));
        assert_eq!(fc["values_mm"], serde_json::json!([2.0]));
        assert_eq!(plane["scalar_tokens"], serde_json::json!([[0xf9]]));
    }
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceParameterRecord {
    pub(super) id: String,
    surface_id: u32,
    surface_type_byte: u8,
    surface_family: &'static str,
    boundary: &'static str,
    body: Vec<u8>,
    slots: Vec<SurfaceParameterScalar>,
    opaque_spans: Vec<SurfaceParameterOpaqueSpan>,
    scalar_frames: Vec<SurfaceParameterScalarFrame>,
    terminal_scalar_frame: Option<SurfaceParameterScalarFrame>,
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
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceRowRecord {
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
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceContourRecord {
    pub(super) id: String,
    surface_id: u32,
    chain_index: usize,
    curve_header_id: u32,
    trv: u8,
    parameter_envelope: [Option<f64>; 4],
    separator_reference: Option<u32>,
    body: Vec<u8>,
    pub(super) offset: usize,
    envelope_offset: usize,
    surface_row_offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoSurfacePrototypeRecord {
    pub(super) id: String,
    declared_family: String,
    family: String,
    parameters: Vec<CreoSurfaceNamedParameterRecord>,
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
        crate::surface::SurfaceNamedValue::ScalarArray(array) => (
            "scalar_array",
            Vec::new(),
            Some(array.dimensions()),
            Some(array.count()),
            array.values().to_vec(),
            array.tokens().unwrap_or_default().to_vec(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::CountedScalarArray(array) => (
            "counted_scalar_array",
            Vec::new(),
            None,
            Some(array.count()),
            array.values().to_vec(),
            array.tokens().map_or_else(
                || array.values().iter().map(|_| Vec::new()).collect(),
                <[Vec<u8>]>::to_vec,
            ),
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
    curve_id: u32,
    type_byte: u8,
    body: Vec<u8>,
    scalar_values: Vec<f64>,
    scalar_tokens: Vec<CreoCurveParameterScalar>,
    skipped_references: Vec<u32>,
    references: Vec<CreoCurveParameterReference>,
    opaque_spans: Vec<CreoCurveParameterOpaqueSpan>,
    reference_geometry: [u32; 2],
    suffix: &'static str,
    suffix_candidate_count: Option<usize>,
    pub(super) offset: usize,
    body_offset: usize,
    suffix_offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoCurveTopologyRowRecord {
    pub(super) id: String,
    curve_id: u32,
    type_byte: u8,
    feature_id: u32,
    directions: [u8; 2],
    faces: [u32; 2],
    next_edges: [u32; 2],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoCrossSectionCurveRowRecord {
    pub(super) id: String,
    curve_id: u32,
    type_byte: u8,
    feature_id: u32,
    directions: [u8; 2],
    suffix: crate::curve::DepdbCurveSuffix,
    body: Vec<u8>,
    scalar_values: Vec<f64>,
    scalar_tokens: Vec<CreoCurveParameterScalar>,
    references: Vec<CreoCurveParameterReference>,
    opaque_spans: Vec<CreoCurveParameterOpaqueSpan>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoTabulatedCylinderCurveReplayRecord {
    pub(super) id: String,
    body: Vec<u8>,
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
    pub(super) offset: usize,
    surface_row_offset: usize,
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
                terminal_scalar_frame: record.terminal_scalar_frame().cloned(),
                tabulated_cylinder_frame: record.tabulated_cylinder_frame().map(|frame| {
                    CreoTabulatedCylinderFrame {
                        values: frame.values().get(),
                        prefixes: frame.prefixes(),
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
                        radians: half_angle.radians.get().get(),
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

pub(super) fn feature_operation_state_records<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<Vec<CreoFeatureOperationState<'a>>, CodecError> {
    let mut current_offsets = BTreeMap::new();
    for state in &scan.features.operations {
        if !current_offsets.contains_key(&state.feature_id) {
            ctx.charge_collection_items(1, "creo native feature current-offset nodes")?;
        }
        current_offsets.insert(state.feature_id, state.offset);
    }
    let mut ordinals = BTreeMap::<u32, usize>::new();
    let mut records = Vec::new();
    for state in &scan.features.operation_states {
        let state_ordinal = ordinals.get(&state.feature_id).copied().unwrap_or_default();
        if !ordinals.contains_key(&state.feature_id) {
            ctx.charge_collection_items(1, "creo native feature ordinal nodes")?;
        }
        let next_ordinal = state_ordinal.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("creo native feature state ordinal", u64::MAX, u64::MAX)
        })?;
        ordinals.insert(state.feature_id, next_ordinal);
        let name = CreoOperationNameRecord {
            display_name_stored: state.name.display_name_stored(),
            stored_name: state.name.stored_name_bytes()
                .map(|bytes| ctx.copy_retained_lossy_utf8(bytes, "creo native feature state name"))
                .transpose()?,
            stored_name_bytes: state.name.stored_name_bytes(),
            identifier_keyword: state.name.identifier_keyword(),
            stored_name_prefix: state.name.stored_name_prefix()
                .map(|prefix| ctx.format_retained(char::from(prefix), "creo native feature state prefix"))
                .transpose()?,
        };
        let id = ctx.format_retained(
            format_args!("creo:mdlstatus:feature_state#{}:{state_ordinal}", state.feature_id),
            "creo native feature state IDs",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native feature state records")?;
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
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<CreoFeatureReferenceNameRecord>, CodecError> {
    let mut records = Vec::new();
    for record in &scan.features.reference_names {
        let id = ctx.format_retained(
            format_args!("creo:mdlrefinfo:feature_name#{}", record.offset),
            "creo native feature reference IDs",
        )?;
        let name = ctx.copy_retained_lossy_utf8(
            &record.name_bytes,
            "creo native feature reference text",
        )?;
        let name_bytes = ctx.copy_retained(
            &record.name_bytes,
            "creo native feature reference bytes",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native feature reference records")?;
        records.push(CreoFeatureReferenceNameRecord {
            id,
            owner_feature_id: record.feature_id,
            name,
            name_bytes,
            own_reference_id: record.own_reference_id,
            reference_type: record.reference_type,
            offset: record.offset,
        });
    }
    Ok(records)
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
                    explicit_slots: frame
                        .explicit_slots
                        .map(cadmpeg_ir::units::FiniteVector::get),
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

pub(super) fn sketch_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<CreoSketchRecord>, cadmpeg_core::CodecError> {
    let mut records = Vec::new();
    for definition in &scan.features.definitions {
        if !feature_definition_has_sketch_design(ctx, definition)? {
            continue;
        }
        ctx.try_reserve_items(&mut records, 1, "creo sketch records")?;
        records.push(CreoSketchRecord {
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
                        reference_plane_datum_geometry_id: section
                            .reference_plane_datum_geometry_id,
                        orientation: CreoSketchSectionOrientation {
                            section_flip: section.orientation.section_flip.map(binary_flag_value),
                            reference_type: section.orientation.reference_type,
                            segment_id: section.orientation.segment_id,
                            reference_flip: section
                                .orientation
                                .reference_flip
                                .map(binary_flag_value),
                        },
                        dimension_ids: section.dimension_ids.clone(),
                        offset: section.offset,
                    }),
                table_headers: sketch_table_headers(ctx, definition)?,
                section_points: sketch_section_point_records(ctx, definition)?,
                solved_external_ids: definition
                    .trim_entities
                    .as_ref()
                    .map_or_else(Vec::new, |table| table.solved_external_ids.clone()),
                variables: {
                    let resolved_coordinates = resolved_section_coordinates(ctx, definition)?;
                    let resolved_radii = resolved_section_radii(ctx, definition)?;
                    let resolved_scalars = resolved_section_scalar_values(ctx, definition)?;
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
                equations: crate::feature::definitions::equation_table(
                    ctx,
                    &definition.body,
                    0,
                    definition.body.len(),
                )?
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
                            crate::feature::definitions::TrimEntityKind::Line => "line",
                            crate::feature::definitions::TrimEntityKind::Arc { .. } => "arc",
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
                        section_coordinates: vertex.section_coordinates.map(|point| {
                            let point = point.get();
                            [point.u, point.v]
                        }),
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
                        crate::feature::definitions::FeatureSavedEntity::Line(line) => {
                            CreoSketchSavedEntity::Line {
                                entity_id: line.entity_id,
                                references: line.references.clone(),
                                attributes: line.attributes.clone(),
                                endpoints: line.endpoints,
                                body: line.body.clone(),
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
                                body: arc.body.clone(),
                                offset: arc.offset,
                            }
                        }
                        crate::feature::definitions::FeatureSavedEntity::Circle(circle) => {
                            CreoSketchSavedEntity::Circle {
                                entity_id: circle.entity_id,
                                center: circle.center,
                                radius: circle.radius,
                                body: circle.body.clone(),
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
                                body: conic.body.clone(),
                                offset: conic.offset,
                            }
                        }
                        crate::feature::definitions::FeatureSavedEntity::Spline(spline) => {
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
                        crate::feature::definitions::FeatureSavedEntity::Dummy(dummy) => {
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
        });
    }
    Ok(records)
}

pub(super) fn sketch_section_point_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<CreoSketchSectionPoint>, cadmpeg_core::CodecError> {
    let Some(variables) = &definition.variables else {
        return Ok(Vec::new());
    };
    let (points, ambiguous) = variables.reconciled_points(ctx)?;
    let mut point_ids = BTreeSet::new();
    for point_id in points
        .keys()
        .copied()
        .chain(ambiguous.iter().copied())
    {
        if !point_ids.contains(&point_id) {
            ctx.charge_collection_items(1, "creo sketch section point ID nodes")?;
            point_ids.insert(point_id);
        }
    }
    crate::decode::collect_items(ctx, point_ids
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
        , "creo sketch section point records")
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
                        crate::feature::definitions::FeatureParameterFrameKind::LocalSystem => {
                            "local_system"
                        }
                        crate::feature::definitions::FeatureParameterFrameKind::Transform => {
                            "transform"
                        }
                    },
                    body: frame.body.clone(),
                    decoded_values: frame
                        .decoded_values
                        .map(cadmpeg_ir::units::FiniteVector::get),
                    offset: frame.offset,
                })
                .collect(),
            outlines: definition
                .outlines
                .iter()
                .map(|outline| CreoFeatureOutline {
                    phase: match outline.phase {
                        crate::feature::definitions::OutlinePhase::PreRollback => "pre_rollback",
                        crate::feature::definitions::OutlinePhase::PostRollback => "post_rollback",
                        crate::feature::definitions::OutlinePhase::PostRegen => "post_regen",
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
    use super::{
        expanded_section_records, feature_operation_state_records, feature_reference_name_records, feature_row_records,
        reference_circle_records, reference_conic_records, reference_ellipse_records,
        reference_line_records,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeSet;

    fn expanded_records_with_limits(
        max_retained_bytes: u64,
        max_collection_items: u64,
    ) -> Result<Vec<super::CreoExpandedSectionRecord>, cadmpeg_core::CodecError> {
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        scan.framing.expanded_sections.push(crate::container::ExpandedSection {
            name: "Body".to_string(),
            source_offset: 0,
            compressed_length: 3,
            data: b"abc".to_vec(),
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = max_retained_bytes;
        policy.limits.max_collection_items = max_collection_items;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        expanded_section_records(&ctx, &scan)
    }

    #[test]
    fn native_expanded_section_id_refuses_retained_limit() {
        let id_len = "creo:container:expanded_section#Body:0".len() as u64;
        let error = expanded_records_with_limits(id_len - 1, 1)
            .expect_err("expanded-section ID needs full retained length");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native expanded section IDs"));
    }

    #[test]
    fn native_expanded_section_name_refuses_retained_limit() {
        let id_len = "creo:container:expanded_section#Body:0".len() as u64;
        let error = expanded_records_with_limits(id_len + 3, 1)
            .expect_err("section name needs four retained bytes");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native expanded section names"));
    }

    #[test]
    fn native_expanded_section_hash_refuses_retained_limit() {
        let id_len = "creo:container:expanded_section#Body:0".len() as u64;
        let error = expanded_records_with_limits(id_len + 4 + 63, 1)
            .expect_err("SHA-256 hex needs 64 retained bytes");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native expanded section hashes"));
    }

    #[test]
    fn native_expanded_section_row_refuses_collection_limit() {
        let error = expanded_records_with_limits(u64::MAX, 0)
            .expect_err("one expanded section needs one output row");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native expanded section records"));
        let records = expanded_records_with_limits(u64::MAX, 1)
            .expect("the expanded section is admitted");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "creo:container:expanded_section#Body:0");
        assert_eq!(records[0].name, "Body");
        assert_eq!(records[0].sha256, cadmpeg_ir::hash::sha256_hex(b"abc"));
    }

    fn reference_scan() -> crate::container::ContainerScan<'static> {
        use cadmpeg_ir::features::FinitePoint3;
        use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
        use cadmpeg_ir::units::UnitVector3;
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        let point = |coordinates: [f64; 3]| {
            FinitePoint3::new(coordinates.into()).expect("finite reference point")
        };
        scan.references.lines.push(crate::reference::ReferenceLine {
            kind: crate::reference::ReferenceLineKind::Line,
            start: point([0.0, 0.0, 0.0]),
            end: point([1.0, 0.0, 0.0]),
            offset: 0,
        });
        scan.references.circles.push(crate::reference::ReferenceCircle {
            entity_id: 7,
            center: point([0.0, 0.0, 0.0]),
            center_stored: true,
            radius: PositiveLength::new(1.0).expect("positive radius"),
            axis: UnitVector3::new([0.0, 0.0, 1.0].into()).expect("unit axis"),
            start: point([1.0, 0.0, 0.0]),
            end: point([0.0, 1.0, 0.0]),
            offset: 0,
        });
        scan.references.conics.push(crate::reference::ReferenceConic {
            entity_id: 8,
            type_id: crate::reference::ConicType::Ellipse,
            flip: 1,
            start: point([2.0, 0.0, 0.0]),
            end: point([0.0, 1.0, 0.0]),
            parameter_start: None,
            parameter_end: None,
            coefficient_1: FiniteReal::new(2.0).expect("finite coefficient"),
            coefficient_2: FiniteReal::new(1.0).expect("finite coefficient"),
            local_system: None,
            body: vec![0x31, 0x32],
            offset: 0,
        });
        scan.references.ellipses.push(crate::reference::ReferenceEllipse {
            source_entity_id: 8,
            center: point([0.0, 0.0, 0.0]),
            axis: UnitVector3::new([0.0, 0.0, 1.0].into()).expect("unit axis"),
            major_direction: UnitVector3::new([1.0, 0.0, 0.0].into()).expect("unit direction"),
            major_radius: PositiveLength::new(2.0).expect("positive radius"),
            minor_radius: PositiveLength::new(1.0).expect("positive radius"),
            offset: 0,
        });
        scan
    }

    fn reference_records_with_limits(
        max_retained_bytes: u64,
        max_collection_items: u64,
        project: impl FnOnce(&DecodeContext<'_>, &crate::container::ContainerScan<'_>)
            -> Result<usize, cadmpeg_core::CodecError>,
    ) -> Result<usize, cadmpeg_core::CodecError> {
        let scan = reference_scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = max_retained_bytes;
        policy.limits.max_collection_items = max_collection_items;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        project(&ctx, &scan)
    }

    #[test]
    fn native_reference_line_id_refuses_retained_limit() {
        let limit = "creo:mdl_ref_info:line_record#0".len() as u64 - 1;
        let error = reference_records_with_limits(limit, 1, |ctx, scan| {
            reference_line_records(ctx, scan).map(|records| records.len())
        }).expect_err("line ID needs its full retained length");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native reference line IDs"));
    }

    #[test]
    fn native_reference_line_row_refuses_collection_limit() {
        let error = reference_records_with_limits(u64::MAX, 0, |ctx, scan| {
            reference_line_records(ctx, scan).map(|records| records.len())
        }).expect_err("one line record needs an output row");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native reference line records"));
        assert_eq!(reference_records_with_limits(u64::MAX, 1, |ctx, scan| {
            reference_line_records(ctx, scan).map(|records| records.len())
        }).expect("one line record"), 1);
    }

    #[test]
    fn native_reference_circle_id_refuses_retained_limit() {
        let limit = "creo:mdl_ref_info:arc_z_record#0".len() as u64 - 1;
        let error = reference_records_with_limits(limit, 1, |ctx, scan| {
            reference_circle_records(ctx, scan).map(|records| records.len())
        }).expect_err("circle ID needs its full retained length");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native reference circle IDs"));
    }

    #[test]
    fn native_reference_circle_row_refuses_collection_limit() {
        let error = reference_records_with_limits(u64::MAX, 0, |ctx, scan| {
            reference_circle_records(ctx, scan).map(|records| records.len())
        }).expect_err("one circle record needs an output row");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native reference circle records"));
        assert_eq!(reference_records_with_limits(u64::MAX, 1, |ctx, scan| {
            reference_circle_records(ctx, scan).map(|records| records.len())
        }).expect("one circle record"), 1);
    }

    #[test]
    fn native_reference_conic_id_refuses_retained_limit() {
        let limit = "creo:mdl_ref_info:conic_record#0".len() as u64 - 1;
        let error = reference_records_with_limits(limit, 1, |ctx, scan| {
            reference_conic_records(ctx, scan).map(|records| records.len())
        }).expect_err("conic ID needs its full retained length");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native reference conic IDs"));
    }

    #[test]
    fn native_reference_conic_row_refuses_collection_limit() {
        let error = reference_records_with_limits(u64::MAX, 0, |ctx, scan| {
            reference_conic_records(ctx, scan).map(|records| records.len())
        }).expect_err("one conic record needs an output row");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native reference conic records"));
        assert_eq!(reference_records_with_limits(u64::MAX, 1, |ctx, scan| {
            reference_conic_records(ctx, scan).map(|records| records.len())
        }).expect("one conic record"), 1);
    }

    #[test]
    fn native_reference_ellipse_id_refuses_retained_limit() {
        let limit = "creo:mdl_ref_info:ellipse_carrier#0".len() as u64 - 1;
        let error = reference_records_with_limits(limit, 1, |ctx, scan| {
            reference_ellipse_records(ctx, scan).map(|records| records.len())
        }).expect_err("ellipse ID needs its full retained length");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native reference ellipse IDs"));
    }

    #[test]
    fn native_reference_ellipse_source_id_refuses_retained_limit() {
        let limit = "creo:mdl_ref_info:ellipse_carrier#0".len() as u64
            + "creo:mdl_ref_info:conic_record#0".len() as u64 - 1;
        let error = reference_records_with_limits(limit, 1, |ctx, scan| {
            reference_ellipse_records(ctx, scan).map(|records| records.len())
        }).expect_err("source conic ID needs its full retained length");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native reference ellipse source IDs"));
    }

    #[test]
    fn native_reference_ellipse_row_refuses_collection_limit() {
        let error = reference_records_with_limits(u64::MAX, 0, |ctx, scan| {
            reference_ellipse_records(ctx, scan).map(|records| records.len())
        }).expect_err("one ellipse record needs an output row");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native reference ellipse records"));
        assert_eq!(reference_records_with_limits(u64::MAX, 1, |ctx, scan| {
            reference_ellipse_records(ctx, scan).map(|records| records.len())
        }).expect("one ellipse record"), 1);
    }

    fn reference_name_scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        scan.features.reference_names.push(crate::feature::operations::FeatureReferenceName {
            feature_id: 40,
            name_bytes: b"A\xff".to_vec(),
            own_reference_id: 3,
            reference_type: 1,
            offset: 0,
        });
        scan
    }

    fn reference_name_records_with_limits(
        max_retained_bytes: u64,
        max_collection_items: u64,
    ) -> Result<Vec<super::CreoFeatureReferenceNameRecord>, cadmpeg_core::CodecError> {
        let scan = reference_name_scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = max_retained_bytes;
        policy.limits.max_collection_items = max_collection_items;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        feature_reference_name_records(&ctx, &scan)
    }

    fn operation_state_scan() -> crate::container::ContainerScan<'static> {
        use crate::feature::operations::{FeatureOperation, IdKeyword, OperationKind, OperationName, RecipeResolution, RecipeState};
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
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

    fn operation_state_records_with_limits(
        max_retained_bytes: u64,
        max_collection_items: u64,
    ) -> Result<Vec<serde_json::Value>, cadmpeg_core::CodecError> {
        let scan = operation_state_scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = max_retained_bytes;
        policy.limits.max_collection_items = max_collection_items;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let records = feature_operation_state_records(&ctx, &scan)?;
        Ok(records.iter().map(|record| serde_json::to_value(record).expect("record JSON")).collect())
    }

    #[test]
    fn native_feature_current_offset_refuses_node_limit() {
        let error = operation_state_records_with_limits(u64::MAX, 0)
            .expect_err("one current offset needs a BTreeMap node");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native feature current-offset nodes"));
    }

    #[test]
    fn native_feature_state_ordinal_refuses_node_limit() {
        let error = operation_state_records_with_limits(u64::MAX, 1)
            .expect_err("one state ordinal needs a BTreeMap node");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native feature ordinal nodes"));
    }

    #[test]
    fn native_feature_state_name_refuses_replacement_limit() {
        let error = operation_state_records_with_limits(3, 3)
            .expect_err("one invalid name needs four replacement bytes");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native feature state name"));
    }

    #[test]
    fn native_feature_state_prefix_refuses_retained_limit() {
        let error = operation_state_records_with_limits(4, 3)
            .expect_err("the source prefix needs another retained byte");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native feature state prefix"));
    }

    #[test]
    fn native_feature_state_id_refuses_retained_limit() {
        let id_len = "creo:mdlstatus:feature_state#40:0".len() as u64;
        let error = operation_state_records_with_limits(5 + id_len - 1, 3)
            .expect_err("the state ID needs its full retained length");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native feature state IDs"));
    }

    #[test]
    fn native_feature_state_row_refuses_collection_limit() {
        let error = operation_state_records_with_limits(u64::MAX, 2)
            .expect_err("one native state needs an output Vec row");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native feature state records"));
        let records = operation_state_records_with_limits(u64::MAX, 3)
            .expect("the service-profile state is admitted");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["id"], "creo:mdlstatus:feature_state#40:0");
        assert_eq!(records[0]["stored_name"], "A\u{fffd}");
        assert_eq!(records[0]["stored_name_bytes"], serde_json::json!([65, 255]));
        assert_eq!(records[0]["stored_name_prefix"], "~");
        assert_eq!(records[0]["identifier_keyword"], "id");
        assert_eq!(records[0]["family"], "Native Feature");
        assert_eq!(records[0]["current"], true);
    }

    #[test]
    fn native_feature_reference_id_refuses_retained_limit() {
        let id_len = "creo:mdlrefinfo:feature_name#0".len() as u64;
        let error = reference_name_records_with_limits(id_len - 1, 1)
            .expect_err("native reference ID needs its full retained length");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native feature reference IDs"));
    }

    #[test]
    fn native_feature_reference_text_refuses_replacement_limit() {
        let id_len = "creo:mdlrefinfo:feature_name#0".len() as u64;
        let error = reference_name_records_with_limits(id_len + 3, 1)
            .expect_err("invalid UTF-8 needs four retained bytes");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native feature reference text"));
    }

    #[test]
    fn native_feature_reference_bytes_refuse_retained_limit() {
        let id_len = "creo:mdlrefinfo:feature_name#0".len() as u64;
        let error = reference_name_records_with_limits(id_len + 4 + 1, 1)
            .expect_err("source bytes need their full retained length");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native feature reference bytes"));
    }

    #[test]
    fn native_feature_reference_row_refuses_collection_limit() {
        let error = reference_name_records_with_limits(u64::MAX, 0)
            .expect_err("one native record needs one collection item");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native feature reference records"));
        let records = reference_name_records_with_limits(u64::MAX, 1)
            .expect("one native record is admitted");
        assert_eq!(records[0].name, "A\u{fffd}");
        assert_eq!(records[0].name_bytes, b"A\xff");
    }

    #[test]
    fn overlapping_feature_candidates_do_not_expose_short_headers() {
        let payload = [1, 0xe3, 2, 0, 0, 0xe3, 0xf6, 0x83, 0x8f, 0xe1];
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy)
                .expect("root input is admitted");
        scan.features.rows = crate::feature::rows::rows(&ctx, &payload, &BTreeSet::from([1, 2]), 0)
            .expect("feature rows are admitted");
        let records = feature_row_records(&ctx, &scan).expect("feature row records are admitted");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].owner_feature_id, 2);
        assert_eq!(records[0].header, [0, 0]);
        assert_eq!(records[0].body, &payload[3..]);
    }
}
